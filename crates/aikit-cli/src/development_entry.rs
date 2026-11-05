//! Development entry: the concern-selected operative context an ordinary
//! harness session receives at its first real prompt.
//!
//! The encounter path already prepares a participant's NOW view before a
//! provider turn, but only for bodies launched through a configured encounter
//! with a static preparation request. A body entered directly — a terminal
//! `claude`, `codex` or `pi` in a Project checkout or workcell seat — received
//! orientation (temporal floor, Wiki projection, steering) and nothing chosen
//! for the work it was actually asked to do. This module is that missing seam,
//! built from the owners that already exist:
//!
//! * the concern is the person's own first prompt, not a configured file;
//! * the Project is resolved from the session's checkout, seat-aware
//!   ([`aikit_adapters::central_temporal::project_ground_root`]);
//! * source candidates come from the Work-repos pool's ranked literal search,
//!   scoped to this Project and to Projects the concern names (never the
//!   personal stores, bookmarks or another lane's NOW flows);
//! * capability candidates are the Project's own `ql-capability-matrix/1`
//!   rows, read through the NOW preparation's native matrix reader, and joined
//!   to the code the search found;
//! * practice candidates are the active praxis catalogue, classified by form;
//!   a small local repair selects no Methodology;
//! * selection is ordinary (deterministic) unless a decision provider is
//!   elected in the capsule configuration, and a provider failure degrades
//!   visibly to ordinary selection — it never blocks the turn;
//! * the prepared view is published to the participant's Redis NOW through the
//!   existing compare-and-swap store, and its rendered text rides the view as
//!   its continuation, so re-entry after compaction re-delivers the same bytes
//!   without re-selection. Redis is working context: a lost view is rebuilt
//!   from the concern this module records in AIKit state.
//!
//! What the body receives is references with revisions and reasons, not
//! bodies: the entry names what to open; the body opens it. Delivery here
//! means "emitted on the harness's context channel", never "read" or "used".
//!
//! A session that works a Wayfinder ticket (O-I 723) is given its practice by
//! the ticket, not by word-matching: where the concern names an issue, the
//! forge reads it (`gh api`, bounded); its map's Notes name the skills every
//! session loads, and its `wayfinder:<type>` selects its Method. Those are
//! mandatory, beside what the words select. What the session then loads is
//! read from its own tool events (a skill invoked, a skill file read), and the
//! close of the ticket is the use check: it is refused until the session has
//! loaded the type's Method, and the resolution's list of skills is checked
//! against what the session loaded — the list is the claim, the events are the
//! evidence.

use std::collections::{BTreeMap, BTreeSet};

use crate::inhabitation::Joined;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aikit_adapters::central_temporal::project_ground_root;
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_adapters::work_repos::{
    decode_work_file_source_ref, discover_native_work_projects, query_terms, work_file_source_ref,
    NativeWorkProjectEntry, NativeWorkRepoProject, WorkReposSourcePoolProvider,
};
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::id::CapsuleId;
use aikit_core::knowledge_source_pool::{SourcePoolProvider, SourceSearchMode};
use aikit_core::method::{praxis_form, praxis_payload, PraxisForm};
use aikit_core::resolve::ResolvedView;
use aikit_core::resource::SourceRef;
use aikit_core::Kind;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::now_context::{
    NowContextBasis, NowContextChange, NowContextItem, NowDeliveryReceipt, PreparedNowContext,
    RedisNowConfig, RedisNowStore, NOW_DELIVERY_SCHEMA, NOW_PREPARED_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const CAPABILITY: &str = "hook/aikit/development-entry";
pub const ENTRY_SCHEMA: &str = "aikit.development-entry/v1";

/// What a session receives once its prepared disclosure has been revoked.
const REVOKED_NOTICE: &str = "[Development entry withheld] this session's prepared disclosure was revoked; nothing is re-delivered, and nothing is rebuilt under the revoked disclosure. Ordinary operation continues.";

/// Rendered entry budget. The entry names; it does not carry bodies.
const MAX_RENDERED_CHARS: usize = 4_500;
const MAX_CONCERN_CHARS: usize = 2_000;
const DOC_LIMIT: usize = 5;
const CODE_LIMIT: usize = 4;
const CROSS_PROJECT_LIMIT: usize = 2;
const CAPABILITY_LIMIT: usize = 3;
const SKILL_LIMIT: usize = 2;
/// Candidates offered to a decision provider. Small models lose accuracy on
/// long states; the inventory is narrowed before it is asked.
const PROVIDER_CANDIDATES: usize = 16;

/// The capsule's configuration (`[config."hook/aikit/development-entry"]`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryConfig {
    /// `ordinary` (default) or `provider`.
    #[serde(default)]
    pub selection: Option<String>,
    /// An `aikit.decision-provider/v1` file; required for `provider`.
    #[serde(default)]
    pub provider_file: Option<PathBuf>,
    /// Explicit relevance threshold for provider selection. Never defaulted
    /// from another provider's calibration.
    #[serde(default)]
    pub relevance_threshold: Option<f64>,
    /// An `aikit.redis-now-config/v1` file. Without it the entry is still
    /// delivered and recorded in AIKit state; only the hot view is absent.
    #[serde(default)]
    pub redis_config: Option<PathBuf>,
}

impl EntryConfig {
    pub fn from_table(table: &toml::value::Table) -> Result<Self> {
        toml::Value::Table(table.clone())
            .try_into()
            .map_err(|error| fail("development_entry.config", error.to_string()))
    }
}

fn fail(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message.into())
}

/// Where the session stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryScope {
    pub project: String,
    pub project_id: String,
    /// `Work/<Name>`: where the Project's ground (ProjectCentral, matrix) is.
    pub primary: PathBuf,
    /// The checkout the body works in (the primary or a workcell seat).
    pub checkout: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

/// Legacy absence-only projection; owned mutation/delivery callers use the
/// fallible resolver so native declaration failures remain actual failures.
pub fn resolve_scope(central: &Path, cwd: &Path) -> Option<EntryScope> {
    resolve_scope_current(central, cwd).ok().flatten()
}

pub fn resolve_scope_current(central: &Path, cwd: &Path) -> Result<Option<EntryScope>> {
    let Some(primary) = project_ground_root(central, cwd) else {
        return Ok(None);
    };
    let checkout = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| primary.clone());
    let project = primary
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            fail(
                "development_entry.project_transport_unsupported",
                "The actual Project display is not exact UTF-8",
            )
        })?
        .to_owned();
    let native = NativeWorkRepoProject::inspect(&primary, &project, Some(central))?
        .with_checkout(&checkout)?;
    let project_id = native.project().project_id.clone();
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(3));
    let git = |args: &[&str]| -> Option<String> {
        let mut argv = vec![
            "git".to_owned(),
            "-C".to_owned(),
            checkout.display().to_string(),
        ];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        let output = runner.run(&argv).ok()?;
        output
            .ok()
            .then(|| output.stdout.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let head = git(&["rev-parse", "--short=12", "HEAD"]);
    Ok(Some(EntryScope {
        project,
        project_id,
        primary,
        checkout,
        branch,
        head,
    }))
}

/// A prompt the entry prepares for: the person's own words. Slash commands,
/// harness-generated turns (background-task notifications, system reminders)
/// and empty input are not a concern.
pub fn substantive_concern(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return None;
    }
    for generated in [
        "<task-notification>",
        "<system-reminder>",
        "[SYSTEM NOTIFICATION",
        "<local-command",
        "<command-name>",
    ] {
        if trimmed.contains(generated) {
            return None;
        }
    }
    if query_terms(trimmed).len() < 2 {
        return None;
    }
    Some(trimmed.chars().take(MAX_CONCERN_CHARS).collect())
}

/// How much practice the concern calls for. A small local repair — a typo, a
/// rename, a bump, a failing test with an obvious local cause — selects no
/// Methodology and no Method: it goes straight to the code and its own check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Depth {
    Shallow,
    Substantial,
}

pub fn classify_depth(concern: &str) -> Depth {
    let lowered = concern.to_lowercase();
    let design = [
        "design",
        "architecture",
        "vision",
        "whole",
        "programme",
        "wayfinder",
        "capability",
        "matrix",
        "mockup",
        "relation",
        "ownership",
        "contract",
        "protocol",
        "spec",
        "commission",
        "integrate",
        "integration",
        "end to end",
        "end-to-end",
        "cross-product",
        "documentation",
        "methodology",
    ];
    let repair = [
        "typo",
        "rename",
        "bump",
        "flaky",
        "lint",
        "clippy",
        "fmt",
        "format",
        "one-line",
        "one line",
        "small fix",
        "failing test",
        "fails to compile",
        "does not compile",
        "off-by-one",
        "off by one",
        "wrong exit code",
        "misspell",
        "smallest fix",
        "smallest change",
        "minimal fix",
        "fix the bug",
        "bug",
        "broken",
        "regression",
        "never reach",
        "is ignored",
        "is dropped",
    ];
    let designish = design.iter().any(|word| lowered.contains(word));
    let repairish = repair.iter().any(|word| lowered.contains(word));
    let short = concern.chars().count() <= 400;
    if short && !designish && (repairish || query_terms(concern).len() <= 6) {
        Depth::Shallow
    } else {
        Depth::Substantial
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceCandidate {
    pub project: String,
    /// Actual Work-owner address retained through selection and NOW context.
    pub source: SourceRef,
    /// Path relative to the checkout it was found in.
    pub path: String,
    /// Absolute path the body opens.
    pub absolute: PathBuf,
    pub revision: String,
    pub score: f64,
    pub snippet: String,
    pub line: Option<u64>,
    pub authored: bool,
    pub mandatory: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CapabilityCandidate {
    pub id: String,
    pub need: String,
    pub operation: String,
    pub outcome: String,
    pub implementation_status: String,
    pub standing: String,
    pub code_refs: Vec<String>,
    pub test_refs: Vec<String>,
    pub account_ref: String,
    pub score: f64,
    /// Why this row is here: which terms, and which found code it owns.
    pub reason: String,
    pub mandatory: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PraxisCandidate {
    pub id: String,
    pub name: String,
    pub form: String,
    pub payload: String,
    pub skill_file: Option<PathBuf>,
    pub score: f64,
    pub mandatory: bool,
    /// Why the claimed ticket makes it mandatory: its map's Notes, or its
    /// type's Method. Such practice is kept whole, outside the bounded picks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Inventory {
    pub sources: Vec<SourceCandidate>,
    pub capabilities: Vec<CapabilityCandidate>,
    pub praxis: Vec<PraxisCandidate>,
    pub matrix: Option<Value>,
    pub absences: Vec<String>,
}

/// Lower-case content words of a text: split on anything but letters and
/// digits, three characters or more, stop words dropped, a plural `s` folded.
fn words(text: &str) -> BTreeSet<String> {
    const STOP: [&str; 30] = [
        "the", "and", "for", "with", "this", "that", "from", "into", "when", "what", "which",
        "use", "how", "its", "are", "not", "any", "all", "one", "own", "has", "have", "does",
        "can", "you", "your", "never", "only", "every", "each",
    ];
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str()))
        .map(|w| {
            if w.len() > 4 && w.ends_with('s') && !w.ends_with("ss") {
                w[..w.len() - 1].to_owned()
            } else {
                w
            }
        })
        .collect()
}

fn is_authored(path: &str) -> bool {
    let lowered = path.to_lowercase();
    lowered.ends_with(".md")
        || lowered.ends_with(".html")
        || lowered.ends_with(".mmd")
        || lowered.starts_with("docs/")
        || lowered.starts_with(".wayfinder/")
}

fn content_revision(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

/// Projects the concern names, beyond the current one. Named, not guessed.
fn named_projects(concern: &str, central: &Path, current: &str) -> Vec<String> {
    let lowered = concern.to_lowercase();
    let aliases: [(&str, &[&str]); 7] = [
        ("O-I", &["o:i", "o-i", " oi ", "oi cli", "cradle"]),
        ("ai-kit", &["aikit", "ai-kit"]),
        ("Central", &["central", "ctrl "]),
        ("Factory", &["factory"]),
        ("Actuation", &["actuation"]),
        ("Workcell", &["workcell"]),
        ("Quaternal-Logic", &["ql-mef", "quaternal", " ql "]),
    ];
    let padded = format!(" {lowered} ");
    aliases
        .iter()
        .filter(|(name, _)| *name != current)
        .filter(|(_, spellings)| spellings.iter().any(|spelling| padded.contains(spelling)))
        .map(|(name, _)| (*name).to_owned())
        .filter(|name| {
            central
                .join("Work")
                .join(name)
                .join("ProjectCentral")
                .is_dir()
        })
        .collect()
}

/// Explicit references the concern itself makes: paths that exist in the
/// checkout and capability ids. These are mandatory and never pruned.
fn explicit_mentions(concern: &str, checkout: &Path) -> (Vec<String>, Vec<String>) {
    let mut paths = Vec::new();
    let mut capabilities = Vec::new();
    for raw in concern.split_whitespace() {
        let token = raw
            .trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/')));
        if token.starts_with("cap.") {
            capabilities.push(token.trim_end_matches('.').to_owned());
        } else if token.contains('/') && !token.contains("://") {
            let candidate = token.trim_start_matches("./").trim_end_matches('.');
            if checkout.join(candidate).is_file() {
                paths.push(candidate.to_owned());
            }
        }
    }
    (paths, capabilities)
}

fn search_sources(
    project: NativeWorkRepoProject,
    concern: &str,
    limit: usize,
) -> Result<(
    WorkReposSourcePoolProvider<SystemRunner>,
    Vec<aikit_core::knowledge_source_pool::SourceHit>,
)> {
    let provider = WorkReposSourcePoolProvider::connect_native(
        SystemRunner::new().with_timeout(Duration::from_secs(10)),
        aikit_adapters::ripgrep::executable(),
        vec![project],
    )?;
    let hits = provider.search(concern, SourceSearchMode::Fulltext, &[], limit)?;
    Ok((provider, hits))
}

fn relative_of(project_id: &str, source: &SourceRef) -> Result<Option<String>> {
    let Some(address) = decode_work_file_source_ref(source)? else {
        return Ok(None);
    };
    if address.project_id != project_id {
        return Ok(None);
    }
    Ok(address.member.to_str().map(str::to_owned))
}

fn candidate_revision(
    provider: &WorkReposSourcePoolProvider<SystemRunner>,
    source: &SourceRef,
    expected_native_revision: Option<&aikit_core::SourceRevision>,
) -> Result<String> {
    let bytes = provider.read_bytes(source)?.ok_or_else(|| {
        fail(
            "development_entry.source_owner_missing",
            "The retained Work owner declined its selected Source",
        )
    })?;
    if expected_native_revision.is_some_and(|revision| {
        revision.as_str() != aikit_adapters::now_field::content_revision(&bytes)
    }) {
        return Err(fail(
            "development_entry.source_revision_conflict",
            "The selected query's native byte basis changed before preparation",
        ));
    }
    // This local prepared-context basis remains blake3. It is not the native
    // Source binding's FNV revision and never mints a replacement SourceRef.
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

fn native_scope_project(central: &Path, scope: &EntryScope) -> Result<NativeWorkRepoProject> {
    let project = NativeWorkRepoProject::inspect(&scope.primary, &scope.project, Some(central))?;
    if project.project().project_id != scope.project_id {
        return Err(fail(
            "development_entry.native_project_changed",
            "The EntryScope no longer names its actual native Project; resolve afresh",
        ));
    }
    project.with_checkout(&scope.checkout)
}

pub fn collect_inventory(
    central: &Path,
    scope: &EntryScope,
    concern: &str,
    view: &ResolvedView,
    capsule_roots: &BTreeMap<CapsuleId, PathBuf>,
) -> Inventory {
    let mut absences = Vec::new();
    let terms: Vec<String> = query_terms(concern)
        .into_iter()
        .map(|term| term.to_lowercase())
        .collect();
    let (mentioned_paths, mentioned_capabilities) = explicit_mentions(concern, &scope.checkout);

    // Sources retain one native declaration/checkout attachment and the real
    // owner-issued reference. No display-name identity or copied-body route.
    let mut sources = Vec::new();
    let mut current_provider = None;
    match native_scope_project(central, scope)
        .and_then(|project| search_sources(project, concern, 40))
    {
        Ok((provider, hits)) => {
            let mut docs = 0;
            let mut code = 0;
            for hit in hits {
                let path = match relative_of(&scope.project_id, &hit.source) {
                    Ok(Some(path)) => path,
                    Ok(None) => continue,
                    Err(error) => {
                        absences.push(format!("source address unavailable: {error}"));
                        continue;
                    }
                };
                let authored = is_authored(&path);
                if authored && docs >= DOC_LIMIT || !authored && code >= CODE_LIMIT {
                    continue;
                }
                let revision =
                    match candidate_revision(&provider, &hit.source, hit.revision.as_ref()) {
                        Ok(revision) => revision,
                        Err(error) => {
                            absences.push(format!("selected source unavailable: {error}"));
                            continue;
                        }
                    };
                if authored {
                    docs += 1;
                } else {
                    code += 1;
                }
                sources.push(SourceCandidate {
                    project: scope.project.clone(),
                    source: hit.source,
                    absolute: scope.checkout.join(&path),
                    path,
                    revision,
                    score: hit.score.unwrap_or(0.0),
                    snippet: hit.snippet.chars().take(160).collect(),
                    line: hit
                        .provider_binding
                        .as_deref()
                        .and_then(|binding| binding.strip_prefix("line:"))
                        .and_then(|line| line.parse().ok()),
                    authored,
                    mandatory: false,
                });
            }
            current_provider = Some(provider);
        }
        Err(error) => absences.push(format!("source search unavailable: {error}")),
    }
    for path in &mentioned_paths {
        if let Some(existing) = sources.iter_mut().find(|source| &source.path == path) {
            existing.mandatory = true;
            continue;
        }
        let Some(provider) = current_provider.as_ref() else {
            continue;
        };
        match work_file_source_ref(&scope.project_id, Path::new(path)).and_then(|source| {
            candidate_revision(provider, &source, None).map(|revision| (source, revision))
        }) {
            Ok((source, revision)) => sources.insert(
                0,
                SourceCandidate {
                    project: scope.project.clone(),
                    source,
                    path: path.clone(),
                    absolute: scope.checkout.join(path),
                    revision,
                    score: 1.0,
                    snippet: String::new(),
                    line: None,
                    authored: is_authored(path),
                    mandatory: true,
                },
            ),
            Err(error) => absences.push(format!("explicit source unavailable: {error}")),
        }
    }
    // Related Projects are actual native declarations, never name fallbacks.
    for name in named_projects(concern, central, &scope.project) {
        let root = central.join("Work").join(&name);
        let related = NativeWorkRepoProject::inspect(&root, &name, Some(central));
        match related.and_then(|project| {
            let project_id = project.project().project_id.clone();
            search_sources(project, concern, 20)
                .map(|(provider, hits)| (project_id, provider, hits))
        }) {
            Ok((project_id, provider, hits)) => {
                let mut retained = 0;
                for hit in hits {
                    let path = match relative_of(&project_id, &hit.source) {
                        Ok(Some(path)) => path,
                        Ok(None) => continue,
                        Err(error) => {
                            absences.push(format!("{name}: source address unavailable: {error}"));
                            continue;
                        }
                    };
                    if !is_authored(&path) || retained >= CROSS_PROJECT_LIMIT {
                        continue;
                    }
                    let revision =
                        match candidate_revision(&provider, &hit.source, hit.revision.as_ref()) {
                            Ok(revision) => revision,
                            Err(error) => {
                                absences
                                    .push(format!("{name}: selected source unavailable: {error}"));
                                continue;
                            }
                        };
                    retained += 1;
                    sources.push(SourceCandidate {
                        project: name.clone(),
                        source: hit.source,
                        absolute: root.join(&path),
                        path,
                        revision,
                        score: hit.score.unwrap_or(0.0),
                        snippet: hit.snippet.chars().take(160).collect(),
                        line: hit
                            .provider_binding
                            .as_deref()
                            .and_then(|binding| binding.strip_prefix("line:"))
                            .and_then(|line| line.parse().ok()),
                        authored: true,
                        mandatory: false,
                    });
                }
            }
            Err(error) => absences.push(format!("{name}: source search unavailable: {error}")),
        }
    }

    // Capabilities: the Project's own matrix, telos-first.
    let mut capabilities = Vec::new();
    let mut matrix = None;
    let found_code: BTreeSet<String> = sources
        .iter()
        .filter(|source| source.project == scope.project)
        .map(|source| source.path.clone())
        .collect();
    // The matrix the body would read or edit: the checkout's own (a seat's
    // lane may be changing it), falling back to the primary's.
    let projectcentral = [
        scope.checkout.join("ProjectCentral"),
        scope.primary.join("ProjectCentral"),
    ]
    .into_iter()
    .find(|path| path.is_dir())
    .unwrap_or_else(|| scope.primary.join("ProjectCentral"));
    match crate::jev_now::project_matrix_rows(&projectcentral) {
        Ok(Some((rows, evidence))) => {
            matrix = Some(evidence);
            for row in rows {
                let haystack = format!(
                    "{} {} {} {} {}",
                    row.id,
                    row.need,
                    row.operation,
                    row.outcome,
                    row.code_refs.join(" ")
                )
                .to_lowercase();
                let matched: Vec<&String> = terms
                    .iter()
                    .filter(|term| haystack.contains(term.as_str()))
                    .collect();
                let owned: Vec<&String> = row
                    .code_refs
                    .iter()
                    .chain(row.test_refs.iter())
                    .filter(|reference| found_code.contains(reference.as_str()))
                    .collect();
                let mandatory = mentioned_capabilities.contains(&row.id);
                let score = matched.len() as f64 / terms.len().max(1) as f64
                    + 0.5 * owned.len().min(2) as f64
                    + if mandatory { 2.0 } else { 0.0 };
                if score <= 0.0 {
                    continue;
                }
                let mut reason = Vec::new();
                if !matched.is_empty() {
                    reason.push(format!(
                        "terms {}",
                        matched
                            .iter()
                            .map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !owned.is_empty() {
                    reason.push(format!(
                        "owns found code {}",
                        owned
                            .iter()
                            .map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if mandatory {
                    reason.push("named in the concern".into());
                }
                capabilities.push(CapabilityCandidate {
                    id: row.id,
                    need: row.need,
                    operation: row.operation,
                    outcome: row.outcome,
                    implementation_status: row.implementation_status,
                    standing: row.standing,
                    code_refs: row.code_refs,
                    test_refs: row.test_refs,
                    account_ref: row.account_ref,
                    score,
                    reason: reason.join("; "),
                    mandatory,
                });
            }
            capabilities.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        }
        Ok(None) => absences.push(format!(
            "no capability matrix under {}/ProjectCentral (user/telos, user, telos)",
            scope.project
        )),
        Err(error) => absences.push(format!(
            "capability matrix unreadable: {}: {}",
            error.code(),
            error.message()
        )),
    }

    // Praxis: the active catalogue, by form. Matching is on whole words with
    // rarity weighting across the catalogue — a word half the repertoire's
    // descriptions use ("design", "view") says little about which practice
    // this concern calls for.
    let active: Vec<_> = view
        .catalog_index
        .values()
        .filter(|entry| entry.kind == Kind::Skill && view.is_active(&entry.id))
        .collect();
    let concern_words = words(concern);
    let vocabularies: Vec<BTreeSet<String>> = active
        .iter()
        .map(|entry| {
            words(&format!(
                "{} {}",
                entry.name.replace('-', " "),
                praxis_payload(&entry.description)
            ))
        })
        .collect();
    let catalogue = active.len().max(1) as f64;
    let weight = |word: &String| {
        let df = vocabularies.iter().filter(|v| v.contains(word)).count();
        if df == 0 {
            0.0
        } else {
            (1.0 + catalogue / df as f64).ln()
        }
    };
    let total: f64 = concern_words.iter().map(weight).sum();
    let mut praxis = Vec::new();
    for (entry, vocabulary) in active.iter().zip(&vocabularies) {
        let matched: Vec<&String> = concern_words
            .iter()
            .filter(|w| vocabulary.contains(*w))
            .collect();
        let mandatory = concern.contains(entry.name.as_str());
        if matched.len() < 2 && !mandatory {
            continue;
        }
        let score = if total > 0.0 {
            matched.iter().map(|w| weight(w)).sum::<f64>() / total
        } else {
            0.0
        };
        let skill_file = capsule_roots
            .get(&entry.id)
            .map(|root| root.join("payload/SKILL.md"))
            .filter(|path| path.is_file());
        praxis.push(PraxisCandidate {
            id: entry.id.to_string(),
            name: entry.name.clone(),
            form: praxis_form(&entry.description).as_str().to_owned(),
            payload: praxis_payload(&entry.description)
                .chars()
                .take(220)
                .collect(),
            skill_file,
            score,
            mandatory,
            because: None,
        });
    }
    praxis.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));

    Inventory {
        sources,
        capabilities,
        praxis,
        matrix,
        absences,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Selection {
    pub mode: String,
    pub depth: Depth,
    pub sources: Vec<SourceCandidate>,
    pub capabilities: Vec<CapabilityCandidate>,
    pub methodology: Option<PraxisCandidate>,
    pub method: Option<PraxisCandidate>,
    pub skills: Vec<PraxisCandidate>,
    /// The Wayfinder ticket the session works, when the forge read one.
    pub ticket: Option<ClaimedTicket>,
    /// The practice the claimed ticket makes mandatory, whole and first.
    pub mandated: Vec<PraxisCandidate>,
    pub decision: Option<Value>,
    pub degradations: Vec<String>,
}

/// Deterministic selection: the ranked inventory, bounded, with practice
/// depth decided by the concern. Mandatory items are always kept.
pub fn ordinary_selection(inventory: &Inventory, depth: Depth) -> Selection {
    let sources = inventory.sources.clone();
    let capabilities: Vec<_> = inventory
        .capabilities
        .iter()
        .filter(|capability| capability.mandatory || capability.score >= 0.34)
        .take(CAPABILITY_LIMIT)
        .cloned()
        .collect();
    // Candidates already share at least two content words with the concern.
    // A Methodology orients a whole field, so its description shares few
    // words with any one concern: it is chosen relative to the other
    // Methodologies, not against an absolute bar. Methods and Skills name a
    // narrower act and must clear one.
    // What the claimed ticket mandates is kept whole, at any depth: its map's
    // Notes name it for every session. The bounded picks are the rest.
    let mandated: Vec<PraxisCandidate> = inventory
        .praxis
        .iter()
        .filter(|p| p.because.is_some())
        .cloned()
        .collect();
    let rest = || inventory.praxis.iter().filter(|p| p.because.is_none());
    let pick = |form: PraxisForm, floor: f64| {
        rest()
            .filter(|p| p.form == form.as_str())
            .find(|p| p.mandatory || p.score >= floor)
            .cloned()
    };
    let (methodology, method, skills) = match depth {
        Depth::Shallow => (
            None,
            None,
            rest()
                .filter(|p| p.mandatory)
                .take(SKILL_LIMIT)
                .cloned()
                .collect(),
        ),
        Depth::Substantial => (
            pick(PraxisForm::Methodology, 0.0),
            pick(PraxisForm::Method, 0.15),
            rest()
                .filter(|p| p.form == PraxisForm::Skill.as_str())
                .filter(|p| p.mandatory || p.score >= 0.18)
                .take(SKILL_LIMIT)
                .cloned()
                .collect(),
        ),
    };
    Selection {
        mode: "ordinary".into(),
        depth,
        sources,
        capabilities,
        methodology,
        method,
        skills,
        ticket: None,
        mandated,
        decision: None,
        degradations: Vec::new(),
    }
}

fn candidate_item(
    reference: &str,
    revision: &str,
    title: &str,
    excerpt: String,
) -> Result<NowContextItem> {
    // The store refuses empty material: a hit whose first matching line was
    // blank still carries its own name.
    let excerpt = if excerpt.trim().is_empty() {
        title.to_owned()
    } else {
        excerpt
    };
    Ok(NowContextItem {
        source_ref: ResourceRef::parse(reference)?,
        source_revision: revision.to_owned(),
        title: title.to_owned(),
        excerpt,
        route: None,
        agent_visibility: AgentVisibility::Payload,
        external_egress: ExternalEgress::Allowed,
    })
}

/// Work-source delivery retains its actual current owner privacy. Other
/// capability/praxis preparation continues through candidate_item unchanged.
fn work_candidate_item(source: &SourceCandidate) -> Result<NowContextItem> {
    let mut item = candidate_item(
        source.source.as_str(),
        &source.revision,
        &source.path,
        source.snippet.clone(),
    )?;
    item.external_egress = ExternalEgress::Denied;
    Ok(item)
}

/// Provider-assisted selection over the ordinary inventory: the elected
/// decision provider answers, per candidate, whether it materially
/// contributes to the concern. Mandatory candidates are never offered for
/// pruning. Any provider failure returns the ordinary selection with the
/// failure named — never a blocked turn, never a silent success.
pub fn provider_selection(
    inventory: &Inventory,
    depth: Depth,
    concern: &str,
    scope: &EntryScope,
    provider_file: &Path,
    threshold: f64,
) -> Selection {
    let mut ordinary = ordinary_selection(inventory, depth);
    let mut keyed: Vec<(String, NowContextItem)> = Vec::new();
    let mut add = |key: String, item: Result<NowContextItem>| {
        if let Ok(item) = item {
            keyed.push((key, item));
        }
    };
    for (index, source) in inventory
        .sources
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.mandatory)
    {
        add(format!("source/{index}"), work_candidate_item(source));
    }
    for (index, capability) in inventory
        .capabilities
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.mandatory)
        .take(6)
    {
        add(
            format!("capability/{index}"),
            candidate_item(
                &format!("capability:{}", capability.id),
                "matrix",
                &capability.id,
                format!(
                    "need: {} | operation: {} | outcome: {}",
                    capability.need, capability.operation, capability.outcome
                ),
            ),
        );
    }
    if depth == Depth::Substantial {
        for (index, practice) in inventory
            .praxis
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.mandatory)
            .take(6)
        {
            add(
                format!("praxis/{index}"),
                candidate_item(
                    &practice.id,
                    "catalogue",
                    &format!("{} ({})", practice.name, practice.form),
                    practice.payload.clone(),
                ),
            );
        }
    }
    keyed.truncate(PROVIDER_CANDIDATES);
    let items: Vec<NowContextItem> = keyed.iter().map(|(_, item)| item.clone()).collect();
    let state = json!({
        "undertaking": concern,
        "project": scope.project,
        "checkout_branch": scope.branch,
        "law": "Select what a developer must read or apply to do this undertaking well; several complementary candidates may all be relevant.",
    });
    match crate::jev_now::provider_select(provider_file, state, threshold, &items) {
        Ok((selected, evidence)) => {
            let chosen: BTreeSet<String> = selected
                .iter()
                .map(|item| item.source_ref.as_str().to_owned())
                .collect();
            let kept = |key: &str| {
                keyed
                    .iter()
                    .find(|(k, _)| k == key)
                    .is_some_and(|(_, item)| chosen.contains(item.source_ref.as_str()))
            };
            ordinary.sources = inventory
                .sources
                .iter()
                .enumerate()
                .filter(|(index, s)| s.mandatory || kept(&format!("source/{index}")))
                .map(|(_, s)| s.clone())
                .collect();
            ordinary.capabilities = inventory
                .capabilities
                .iter()
                .enumerate()
                .filter(|(index, c)| c.mandatory || kept(&format!("capability/{index}")))
                .take(CAPABILITY_LIMIT)
                .map(|(_, c)| c.clone())
                .collect();
            if depth == Depth::Substantial {
                let chosen_praxis: Vec<PraxisCandidate> = inventory
                    .praxis
                    .iter()
                    .enumerate()
                    .filter(|(index, p)| p.mandatory || kept(&format!("praxis/{index}")))
                    // The ticket's own practice stands in `mandated` already.
                    .filter(|(_, p)| p.because.is_none())
                    .map(|(_, p)| p.clone())
                    .collect();
                let first = |form: PraxisForm| {
                    chosen_praxis
                        .iter()
                        .find(|p| p.form == form.as_str())
                        .cloned()
                };
                ordinary.methodology = first(PraxisForm::Methodology);
                ordinary.method = first(PraxisForm::Method);
                ordinary.skills = chosen_praxis
                    .iter()
                    .filter(|p| p.form == PraxisForm::Skill.as_str())
                    .take(SKILL_LIMIT)
                    .cloned()
                    .collect();
            }
            ordinary.mode = "provider".into();
            ordinary.decision = Some(evidence);
            ordinary
        }
        Err(error) => {
            ordinary.degradations.push(format!(
                "decision provider unavailable ({}: {}); ordinary selection used",
                error.code(),
                error.message()
            ));
            ordinary
        }
    }
}

fn bounded(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn short_revision(revision: &str) -> String {
    revision
        .strip_prefix("blake3:")
        .map(|hex| format!("blake3:{}", &hex[..hex.len().min(12)]))
        .unwrap_or_else(|| revision.to_owned())
}

pub fn render(
    scope: &EntryScope,
    concern: &str,
    selection: &Selection,
    prepared: Option<(u64, &str)>,
) -> String {
    let mut lines = vec![format!(
        "[Development entry] {ENTRY_SCHEMA} — selected for this concern; references, not bodies. Open what the act needs; nothing here is permission or proof."
    )];
    lines.push(format!(
        "scope: Project {} · checkout {} · branch {} @ {}",
        scope.project,
        scope.checkout.display(),
        scope.branch.as_deref().unwrap_or("?"),
        scope.head.as_deref().unwrap_or("?")
    ));
    lines.push(format!(
        "concern: {}",
        bounded(&concern.replace('\n', " "), 220)
    ));
    lines.push(format!(
        "selection: {} · practice depth {}",
        selection.mode,
        match selection.depth {
            Depth::Shallow => "shallow (small local repair: no Methodology, no Vision — go to the code and its own check)",
            Depth::Substantial => "substantial",
        }
    ));
    // Degradations come before the lists: the entry is bounded, and what is
    // missing or unavailable must never be the part that gets cut.
    for degradation in &selection.degradations {
        lines.push(format!("degraded: {degradation}"));
    }
    // The claimed ticket's practice is law for the session, so it stands
    // above the bounded lists, never where the bound cuts.
    if let Some(ticket) = &selection.ticket {
        lines.extend(ticket_lines(ticket, &selection.mandated));
    }
    if !selection.sources.is_empty() {
        lines.push(
            "sources (open exactly these first; revision = content digest at preparation):".into(),
        );
        for source in &selection.sources {
            let location = if source.project == scope.project {
                source.path.clone()
            } else {
                source.absolute.display().to_string()
            };
            let line = source.line.map(|l| format!(":{l}")).unwrap_or_default();
            let marker = if source.mandatory {
                " [named in concern]"
            } else {
                ""
            };
            lines.push(format!(
                "  - {location}{line} ({}{marker}) — {}",
                short_revision(&source.revision),
                bounded(&source.snippet, 110)
            ));
        }
    }
    if !selection.capabilities.is_empty() {
        lines.push(
            "capabilities (Project matrix; standing is the row's own, not a verdict):".into(),
        );
        for capability in &selection.capabilities {
            lines.push(format!(
                "  - {} [{} · {}] need: {} | outcome: {} | code: {} | tests: {} — {}",
                capability.id,
                bounded(&capability.implementation_status, 60),
                capability.standing,
                bounded(&capability.need, 90),
                bounded(&capability.outcome, 90),
                bounded(&capability.code_refs.join(", "), 120),
                bounded(&capability.test_refs.join(", "), 90),
                capability.reason
            ));
        }
    }
    let practice: Vec<&PraxisCandidate> = selection
        .methodology
        .iter()
        .chain(selection.method.iter())
        .chain(selection.skills.iter())
        .collect();
    if practice.is_empty() {
        if selection.ticket.is_none() {
            lines.push("practice: none selected".into());
        }
    } else {
        lines.push("practice (read the file before acting on the form it names):".into());
        for p in practice {
            lines.push(format!(
                "  - {} {} — {} {}",
                p.form.to_uppercase(),
                p.id,
                bounded(&p.payload, 150),
                p.skill_file
                    .as_ref()
                    .map(|f| format!("(read: {})", f.display()))
                    .unwrap_or_default()
            ));
        }
    }
    if let Some((version, digest)) = prepared {
        lines.push(format!(
            "prepared NOW view v{version} ({}) — `aikit now-context inspect --participant-ref <this session>` reads it",
            &digest[..digest.len().min(19)]
        ));
    }
    bounded(&lines.join("\n"), MAX_RENDERED_CHARS)
}

/// What this module keeps in AIKit state per session: the concern (so a lost
/// Redis view can be rebuilt) and what was last emitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryLedger {
    pub schema: String,
    pub client: String,
    pub session: String,
    pub concern: String,
    pub project: String,
    pub checkout: PathBuf,
    pub rendered_digest: String,
    pub prepared_version: Option<u64>,
    pub source_revisions: BTreeMap<String, String>,
    /// Historical issued refs, never permission to read. Legacy path-only
    /// observations retain no invented native attribution.
    #[serde(default)]
    pub source_refs: BTreeMap<String, SourceRef>,
    pub emitted_at_unix_ms: u64,
    /// Set at pre-compaction: the next prompt re-delivers the entry.
    #[serde(default)]
    pub redeliver_pending: bool,
    /// What the emitted entry selected (`source:`, `capability:`, `praxis:`
    /// keys), so a later refinement can say what it changed.
    #[serde(default)]
    pub selected: Vec<String>,
    /// A decision-provider refinement that finished after the entry was
    /// emitted, waiting for the session's next prompt.
    #[serde(default)]
    pub refinement: Option<String>,
    /// The Wayfinder ticket the session works, as the forge read it.
    #[serde(default)]
    pub ticket: Option<ClaimedTicket>,
    /// The practice this session has loaded, by name, from its own tool
    /// events (`record_use`): the evidence the use check reads.
    #[serde(default)]
    pub loaded: BTreeSet<String>,
    /// The practice this session invoked by name (a skill call), a subset of
    /// what it loaded.
    #[serde(default)]
    pub invoked: BTreeSet<String>,
    /// The issue references already read as tickets, so a later prompt
    /// naming them again costs no forge read.
    #[serde(default)]
    pub tickets_read: BTreeSet<String>,
}

fn ledger_work_owner(
    central: &Path,
    scope: &EntryScope,
    ledger: &EntryLedger,
) -> Result<WorkReposSourcePoolProvider<SystemRunner>> {
    let current = native_scope_project(central, scope)?;
    let mut required = BTreeSet::new();
    for source in ledger.source_refs.values() {
        let address = decode_work_file_source_ref(source)?.ok_or_else(|| {
            fail(
                "development_entry.source_owner_unknown",
                "A retained source reference has no known Work owner language",
            )
        })?;
        required.insert(address.project_id);
    }
    let primary = std::fs::canonicalize(&scope.primary).map_err(|error| {
        fail(
            "development_entry.source_binding_unavailable",
            error.to_string(),
        )
        .with_io_source(error)
    })?;
    let mut projects = vec![current];
    for entry in discover_native_work_projects(central)? {
        match entry {
            NativeWorkProjectEntry::Project(project) => {
                if project.project().project_id == scope.project_id {
                    let root = std::fs::canonicalize(&project.project().root).map_err(|error| {
                        fail(
                            "development_entry.source_binding_unavailable",
                            error.to_string(),
                        )
                        .with_io_source(error)
                    })?;
                    if root != primary {
                        return Err(fail("development_entry.source_binding_conflict",
                        "The actual native World has conflicting roots for this literal Project ID"));
                    }
                } else if required.contains(&project.project().project_id) {
                    projects.push(project);
                }
            }
            NativeWorkProjectEntry::Absence { name, error } => {
                return Err(fail(
                    "development_entry.source_binding_unavailable",
                    format!("Work/{name}: {error}"),
                )
                .with("native_code", error.code())
                .with_io_source_from(&error))
            }
        }
    }
    WorkReposSourcePoolProvider::connect_native(
        SystemRunner::probe(),
        aikit_adapters::ripgrep::executable(),
        projects,
    )
}

fn current_ledger_changes(
    central: &Path,
    scope: &EntryScope,
    ledger: &EntryLedger,
) -> Result<Vec<(String, String, String)>> {
    let owner = if ledger.source_refs.is_empty() {
        None
    } else {
        Some(ledger_work_owner(central, scope, ledger)?)
    };
    let mut changed = Vec::new();
    for (path, revision) in &ledger.source_revisions {
        let now = if let Some(source) = ledger.source_refs.get(path) {
            let owner = owner.as_ref().expect("source refs required an owner");
            let address = decode_work_file_source_ref(source)?.ok_or_else(|| {
                fail(
                    "development_entry.source_owner_unknown",
                    "The retained source has no Work owner address",
                )
            })?;
            let project = owner
                .projects()
                .iter()
                .find(|project| project.project_id == address.project_id)
                .ok_or_else(|| {
                    fail(
                        "development_entry.source_owner_missing",
                        "The retained native Project is no longer discoverable",
                    )
                })?;
            if project.root.join(&address.member) != Path::new(path) {
                return Err(fail(
                    "development_entry.source_binding_changed",
                    "The issued Source no longer maps to its retained selected path",
                ));
            }
            candidate_revision(owner, source, None)?
        } else {
            // Matrix/legacy path observations stay local observations. They
            // cannot emit a newly reconstructed native Source change.
            content_revision(Path::new(path)).unwrap_or_else(|| "absent".into())
        };
        if &now != revision {
            changed.push((path.clone(), revision.clone(), now));
        }
    }
    Ok(changed)
}

fn selected_keys(selection: &Selection) -> Vec<String> {
    selection
        .sources
        .iter()
        .map(|s| s.source.as_str().to_owned())
        .chain(
            selection
                .capabilities
                .iter()
                .map(|c| format!("capability:{}", c.id)),
        )
        .chain(
            selection
                .mandated
                .iter()
                .chain(selection.methodology.iter())
                .chain(selection.method.iter())
                .chain(selection.skills.iter())
                .map(|p| format!("praxis:{}", p.id)),
        )
        .collect()
}

/// What a refinement changed, as the body will read it. `None` when the
/// decision provider confirmed the emitted selection unchanged.
fn render_refinement(before: &[String], after: &Selection, version: Option<u64>) -> Option<String> {
    let after_keys = selected_keys(after);
    let added: Vec<&String> = after_keys.iter().filter(|k| !before.contains(k)).collect();
    let dropped: Vec<&String> = before.iter().filter(|k| !after_keys.contains(k)).collect();
    if added.is_empty() && dropped.is_empty() {
        return None;
    }
    let mut lines = vec![format!(
        "[Development entry refined] the elected decision provider reviewed this session's entry{}; its judgement grants nothing and the sources stay authoritative.",
        version.map(|v| format!(" (prepared v{v})")).unwrap_or_default()
    )];
    for key in &added {
        let detail = after
            .sources
            .iter()
            .find(|s| s.source.as_str() == key.as_str())
            .map(|s| format!(" — {}", s.absolute.display()))
            .or_else(|| {
                after
                    .methodology
                    .iter()
                    .chain(after.method.iter())
                    .chain(after.skills.iter())
                    .find(|p| &&format!("praxis:{}", p.id) == key)
                    .and_then(|p| {
                        p.skill_file
                            .as_ref()
                            .map(|f| format!(" ({}; read: {})", p.form, f.display()))
                    })
            })
            .unwrap_or_default();
        lines.push(format!("  + {key}{detail}"));
    }
    for key in &dropped {
        lines.push(format!("  - {key} (judged not material to this concern)"));
    }
    Some(lines.join("\n"))
}

fn ledger_path(state: &Path, client: &str, session: &str) -> PathBuf {
    state.join("development-entry").join(format!(
        "{}.json",
        blake3::hash(format!("{client}\0{session}").as_bytes()).to_hex()
    ))
}

pub fn load_ledger(state: &Path, client: &str, session: &str) -> Option<EntryLedger> {
    let text = std::fs::read_to_string(ledger_path(state, client, session)).ok()?;
    serde_json::from_str(&text).ok()
}

fn store_ledger(state: &Path, ledger: &EntryLedger) {
    let path = ledger_path(state, &ledger.client, &ledger.session);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(body) = serde_json::to_vec_pretty(ledger) {
        let _ = std::fs::write(path, body);
    }
}

fn store_receipt(state: &Path, client: &str, session: &str, receipt: &Value) {
    let dir = state.join("development-entry").join("receipts");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = now_ms();
    let name = format!(
        "{}-{stamp}.json",
        &blake3::hash(format!("{client}\0{session}").as_bytes()).to_hex()[..16]
    );
    if let Ok(body) = serde_json::to_vec_pretty(receipt) {
        let _ = std::fs::write(dir.join(name), body);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn participant_refs(client: &str, session: &str) -> Result<(ResourceRef, ResourceRef)> {
    Ok((
        ResourceRef::parse(format!("participant/{client}/{session}"))?,
        ResourceRef::parse(format!("agent-session/{client}:{session}"))?,
    ))
}

fn redis_store(config: &EntryConfig) -> Option<std::result::Result<RedisNowStore, String>> {
    let path = config.redis_config.as_ref()?;
    Some((|| {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let redis: RedisNowConfig = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        redis.validate().map_err(|e| e.message().to_owned())?;
        if redis.credential_ref.is_some() {
            return Err("credentialed Redis NOW is not supported from a hook process".into());
        }
        RedisNowStore::new(redis).map_err(|e| e.message().to_owned())
    })())
}

#[allow(clippy::too_many_arguments)]
fn publish_view(
    store: &RedisNowStore,
    client: &str,
    session: &str,
    scope: &EntryScope,
    concern: &str,
    selection: &Selection,
    rendered: &str,
    expected: Option<u64>,
) -> Result<(u64, String)> {
    let (participant, agent_session) = participant_refs(client, session)?;
    // A refinement publishes against the exact version its hook emitted; a
    // newer preparation in between makes the store refuse it as stale.
    let current = match expected {
        Some(version) => version,
        None => store.current_version(&participant, None)?,
    };
    let mut source_revisions = BTreeMap::new();
    let mut items = Vec::new();
    for source in &selection.sources {
        source_revisions.insert(
            source.absolute.display().to_string(),
            source.revision.clone(),
        );
        items.push(work_candidate_item(source)?);
    }
    for capability in &selection.capabilities {
        items.push(candidate_item(
            &format!("capability:{}", capability.id),
            "matrix",
            &capability.id,
            format!(
                "need: {} | outcome: {}",
                capability.need, capability.outcome
            ),
        )?);
    }
    let practice_refs = selection
        .mandated
        .iter()
        .chain(selection.methodology.iter())
        .chain(selection.method.iter())
        .chain(selection.skills.iter())
        .map(|p| ResourceRef::parse(&p.id))
        .collect::<Result<Vec<_>>>()?;
    let basis = NowContextBasis {
        source_revisions,
        dependency_revisions: BTreeMap::new(),
        disclosure_revision: format!("{ENTRY_SCHEMA}:local"),
        factory_revision: None,
        decision_provider: selection.decision.as_ref().and_then(|d| {
            d.get("decisionProvider")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
        change_cursor: 0,
    };
    let view = PreparedNowContext {
        schema: NOW_PREPARED_SCHEMA.into(),
        project_ref: ResourceRef::parse(format!("project/{}", scope.project_id))?,
        now_ref: ResourceRef::parse(format!("central:now-field:project:{}", scope.project))?,
        participant_ref: participant.clone(),
        agent_session: agent_session.clone(),
        version: current + 1,
        basis_digest: basis.digest()?,
        basis,
        concern: concern.to_owned(),
        practice_refs,
        items,
        neighbours: Vec::new(),
        factory: None,
        knowledge_frames: Vec::new(),
        continuation: Some(rendered.to_owned()),
        jev_invocation_ref: None,
        praxis: None,
        prepared_at_unix_ms: now_ms(),
    };
    let version = store.publish(&view, current, None)?;
    let digest = view.digest()?;
    Ok((version, digest))
}

fn mark_emitted(
    store: &RedisNowStore,
    client: &str,
    session: &str,
    version: u64,
    digest: &str,
    basis: &str,
) {
    if let Ok((participant, agent_session)) = participant_refs(client, session) {
        let receipt = NowDeliveryReceipt {
            schema: NOW_DELIVERY_SCHEMA.into(),
            participant_ref: participant,
            agent_session,
            prepared_version: version,
            prepared_digest: digest.to_owned(),
            basis_digest: basis.to_owned(),
            change_cursor: 0,
            delivered_at_unix_ms: now_ms(),
            // The development-entry hook delivers a view it did not select; no decision stands behind it here.
            decision_provider: None,
            jev_invocation_ref: None,
        };
        let _ = store.mark_delivered(&receipt, None);
    }
}

/// The inputs the hook engine hands over.
pub struct EntryRequest<'a> {
    pub event: &'a HookEvent,
    pub client: &'a str,
    pub config: &'a EntryConfig,
    pub state: &'a Path,
    pub central: Option<&'a Path>,
    pub view: &'a ResolvedView,
    pub capsule_roots: &'a BTreeMap<CapsuleId, PathBuf>,
}

/// One hook event's contribution: `Ok(None)` when this event carries no
/// entry (not a concern, already delivered and unchanged, not a Project).
pub fn deliver(request: &EntryRequest<'_>) -> Result<Option<String>> {
    let started = Instant::now();
    let event = request.event;
    let Some(session) = crate::refocus::hook_session(&event.payload) else {
        return Ok(None);
    };
    // Tool events answer from the session's own record, before any scope is
    // resolved: they come with every tool call. The use check on PreToolUse
    // is `check_resolution`, which can refuse; this path only records.
    match event.kind {
        HookEventKind::PostToolUse => return observe(request, &session),
        HookEventKind::PreToolUse => return Ok(None),
        _ => {}
    }
    let Some(central) = request.central else {
        return Ok(None);
    };
    let cwd = event
        .cwd
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let Some(scope) = resolve_scope_current(central, &cwd)? else {
        return Ok(None);
    };
    let ledger = load_ledger(request.state, request.client, &session);
    let store = redis_store(request.config);

    match event.kind {
        // Re-entry after compaction, resume or clear: the body's context lost
        // what it was given. Claude and zcode report it as `source`, pi as
        // `reason`. Re-deliver the same prepared bytes (warm, no
        // re-selection); rebuild from the recorded concern if Redis lost it.
        HookEventKind::SessionStart => {
            let why = event
                .payload
                .get("source")
                .or_else(|| event.payload.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or("startup")
                .to_owned();
            if !matches!(
                why.as_str(),
                "compact" | "resume" | "clear" | "fork" | "reload"
            ) {
                return Ok(None);
            }
            let Some(mut ledger) = ledger else {
                return Ok(None);
            };
            let text = redeliver(
                request,
                &scope,
                &session,
                &ledger,
                store.as_ref(),
                started,
                &why,
            )?;
            ledger.redeliver_pending = false;
            store_ledger(request.state, &ledger);
            return Ok(Some(text));
        }
        // Compaction discards the entry from the body's context, and no
        // harness carries this event's output. Where the harness re-fires
        // SessionStart afterwards the entry returns there; everywhere else
        // (Codex, pi) the next prompt carries it.
        HookEventKind::PreCompact => {
            if let Some(mut ledger) = ledger {
                ledger.redeliver_pending = true;
                store_ledger(request.state, &ledger);
            }
            return Ok(None);
        }
        HookEventKind::UserPromptSubmit => {}
        _ => return Ok(None),
    }

    if let Some(mut refined) = ledger
        .clone()
        .filter(|l| l.refinement.is_some() && l.checkout == scope.checkout)
    {
        let text = refined.refinement.take().unwrap_or_default();
        store_ledger(request.state, &refined);
        store_receipt(
            request.state,
            request.client,
            &session,
            &json!({
                "schema": ENTRY_SCHEMA, "event": "UserPromptSubmit", "delivery": "refinement-emitted",
                "prepared_version": refined.prepared_version,
            }),
        );
        return Ok(Some(text));
    }
    if let Some(mut pending) = ledger
        .clone()
        .filter(|l| l.redeliver_pending && l.checkout == scope.checkout)
    {
        let text = redeliver(
            request,
            &scope,
            &session,
            &pending,
            store.as_ref(),
            started,
            "compaction",
        )?;
        pending.redeliver_pending = false;
        store_ledger(request.state, &pending);
        return Ok(Some(text));
    }

    let prompt = event
        .payload
        .get("prompt")
        .or_else(|| event.payload.get("user_prompt"))
        .or_else(|| event.payload.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    // A later prompt: announce what changed under the prepared basis, once,
    // and the ticket it names when the entry has none.
    if let Some(mut ledger) = ledger {
        if ledger.checkout != scope.checkout {
            // The session moved to another checkout: its entry no longer
            // describes where it stands. Prepare afresh for the new scope.
        } else {
            let mut ticket_text = None;
            if ledger.ticket.is_none() {
                if let Some(concern) = substantive_concern(prompt) {
                    let catalogue = active_catalogue(request.view, request.capsule_roots);
                    let read_before = ledger.tickets_read.len();
                    let mut gaps = Vec::new();
                    let found = ticket_named(
                        &SystemRunner::new().with_timeout(FORGE_CALL),
                        forge_binary().as_deref(),
                        &scope.checkout,
                        &concern,
                        &catalogue,
                        &mut ledger.tickets_read,
                        &mut gaps,
                    );
                    if let Some(ticket) = found {
                        let (block, keys) = ticket_block(
                            "[Development entry: this prompt names a ticket; its practice]",
                            &ticket,
                            &catalogue,
                            &gaps,
                        );
                        ticket_text = Some(block);
                        ledger.selected.extend(keys);
                        ledger.ticket = Some(ticket);
                    } else if !gaps.is_empty() {
                        ticket_text = Some(format!(
                            "[Development entry: ticket not read] {}",
                            gaps.join("; ")
                        ));
                    }
                    if ledger.tickets_read.len() != read_before {
                        store_ledger(request.state, &ledger);
                    }
                }
            }
            let changed = current_ledger_changes(central, &scope, &ledger)?;
            if changed.is_empty() {
                return Ok(ticket_text);
            }
            let mut lines = vec!["[Development entry: sources changed since preparation — reopen before relying on what you read]".to_owned()];
            for (path, before, after) in &changed {
                lines.push(format!(
                    "  - {path}: {} → {}",
                    short_revision(before),
                    short_revision(after)
                ));
                ledger.source_revisions.insert(path.clone(), after.clone());
            }
            if let Some(Ok(store)) = &store {
                if let Ok((participant, _)) = participant_refs(request.client, &session) {
                    for (path, before, after) in &changed {
                        let Some(source) = ledger.source_refs.get(path) else {
                            continue;
                        };
                        let change = NowContextChange {
                            change_id: format!(
                                "development-entry:{}",
                                &blake3::hash(format!("{path}\0{after}").as_bytes()).to_hex()[..24]
                            ),
                            kind: "source-revision".into(),
                            source_ref: ResourceRef::parse(source.as_str())?,
                            source_revision: after.clone(),
                            detail: format!("moved from {before} under the prepared entry"),
                            observed_at_unix_ms: now_ms(),
                        };
                        let _ = store.append_change(&participant, &change, None);
                    }
                }
            }
            ledger.emitted_at_unix_ms = now_ms();
            store_ledger(request.state, &ledger);
            lines.extend(ticket_text);
            return Ok(Some(lines.join("\n")));
        }
    }

    let Some(concern) = substantive_concern(prompt) else {
        return Ok(None);
    };
    prepare(
        request,
        &scope,
        &session,
        &concern,
        store.as_ref(),
        started,
        "prepared",
    )
    .map(Some)
}

/// Re-deliver a session's entry after its context lost it: the prepared view's
/// own rendered bytes from Redis when the hot view is present, otherwise a
/// rebuild from the concern AIKit recorded (Redis is working context, not the
/// owner of what was asked).
fn redeliver(
    request: &EntryRequest<'_>,
    scope: &EntryScope,
    session: &str,
    ledger: &EntryLedger,
    store: Option<&std::result::Result<RedisNowStore, String>>,
    started: Instant,
    why: &str,
) -> Result<String> {
    if let Some(Ok(store)) = store {
        if let Ok((participant, _)) = participant_refs(request.client, session) {
            let read = store.read_prepared(&participant, false, None);
            if read
                .as_ref()
                .is_err_and(|error| error.code() == "now_context.disclosure_revoked")
            {
                return Ok(REVOKED_NOTICE.to_owned());
            }
            if let Ok(Some(view)) = read {
                if let Some(text) = view.continuation.clone() {
                    mark_emitted(
                        store,
                        request.client,
                        session,
                        view.version,
                        &view.digest()?,
                        &view.basis_digest,
                    );
                    store_receipt(
                        request.state,
                        request.client,
                        session,
                        &json!({
                            "schema": ENTRY_SCHEMA, "event": request.event.kind.as_str(), "why": why,
                            "delivery": "re-delivered-warm", "prepared_version": view.version,
                            "elapsed_ms": started.elapsed().as_millis() as u64,
                        }),
                    );
                    return Ok(format!(
                        "{text}\n(re-delivered after {why}: prepared v{} from Redis NOW, not re-selected)",
                        view.version
                    ));
                }
            }
        }
    }
    let rebuilt = prepare(
        request,
        scope,
        session,
        &ledger.concern,
        store,
        started,
        "rebuilt-after-loss",
    )?;
    Ok(format!(
        "{rebuilt}\n(rebuilt after {why}: the hot view was unavailable; recomputed from the recorded concern)"
    ))
}

fn prepare(
    request: &EntryRequest<'_>,
    scope: &EntryScope,
    session: &str,
    concern: &str,
    store: Option<&std::result::Result<RedisNowStore, String>>,
    started: Instant,
    delivery: &str,
) -> Result<String> {
    let central = request.central.unwrap_or_else(|| Path::new("/"));
    let depth = classify_depth(concern);
    let mut inventory =
        collect_inventory(central, scope, concern, request.view, request.capsule_roots);
    let inventory_ms = started.elapsed().as_millis() as u64;
    // The claimed ticket: kept from this session's earlier entry, else read
    // from the forge when the concern names an issue. What it mandates
    // stands beside what the words selected.
    let prior = load_ledger(request.state, request.client, session);
    let catalogue = active_catalogue(request.view, request.capsule_roots);
    let mut tickets_read = prior
        .as_ref()
        .map(|l| l.tickets_read.clone())
        .unwrap_or_default();
    let mut ticket_gaps = Vec::new();
    let ticket = prior.as_ref().and_then(|l| l.ticket.clone()).or_else(|| {
        ticket_named(
            &SystemRunner::new().with_timeout(FORGE_CALL),
            forge_binary().as_deref(),
            &scope.checkout,
            concern,
            &catalogue,
            &mut tickets_read,
            &mut ticket_gaps,
        )
    });
    if let Some(ticket) = &ticket {
        mandate(&mut inventory, ticket, &catalogue);
        ticket_gaps.extend(ticket.gaps.iter().cloned());
    }
    // The provider never runs inside the hook: a decision model on a loaded
    // machine outlasts every harness's hook budget (pi kills at 10 s), and a
    // killed hook delivers nothing at all. The ordinary selection goes out
    // now; the provider refines it in a detached process and the next prompt
    // carries what it changed.
    let mut selection = ordinary_selection(&inventory, depth);
    selection.ticket = ticket.clone();
    selection.degradations.extend(ticket_gaps);
    let refine = match request.config.selection.as_deref() {
        Some("provider") => match (
            &request.config.provider_file,
            request.config.relevance_threshold,
        ) {
            (Some(_), Some(_)) => true,
            _ => {
                selection.degradations.push("provider selection configured without provider_file and an explicit relevance_threshold; ordinary selection used".into());
                false
            }
        },
        _ => false,
    };
    selection
        .degradations
        .extend(inventory.absences.iter().cloned());
    let selection_ms = started.elapsed().as_millis() as u64 - inventory_ms;
    let first = render(scope, concern, &selection, None);
    let mut prepared = None;
    match store {
        Some(Ok(store)) => match publish_view(
            store,
            request.client,
            session,
            scope,
            concern,
            &selection,
            &first,
            None,
        ) {
            Ok((version, digest)) => prepared = Some((version, digest)),
            // A revoked disclosure is not re-emitted under the same revision:
            // the selection stays unsent.
            Err(error) if error.code() == "now_context.disclosure_revoked" => {
                return Ok(REVOKED_NOTICE.to_owned());
            }
            Err(error) => selection.degradations.push(format!(
                "prepared NOW view not published ({}: {})",
                error.code(),
                error.message()
            )),
        },
        Some(Err(reason)) => selection.degradations.push(format!(
            "Redis NOW unavailable ({reason}); entry delivered without a hot view"
        )),
        None => {}
    }
    if refine {
        selection.mode = "ordinary now; the elected decision provider refines it in the background and the next prompt carries any change".into();
    }
    let rendered = render(
        scope,
        concern,
        &selection,
        prepared.as_ref().map(|(v, d)| (*v, d.as_str())),
    );
    if let (Some(Ok(store)), Some((version, digest))) = (store, &prepared) {
        let basis = store
            .read_prepared(&participant_refs(request.client, session)?.0, false, None)
            .ok()
            .flatten()
            .map(|view| view.basis_digest)
            .unwrap_or_default();
        mark_emitted(store, request.client, session, *version, digest, &basis);
    }
    let ledger = EntryLedger {
        schema: ENTRY_SCHEMA.into(),
        client: request.client.to_owned(),
        session: session.to_owned(),
        concern: concern.to_owned(),
        project: scope.project.clone(),
        checkout: scope.checkout.clone(),
        rendered_digest: blake3::hash(rendered.as_bytes()).to_hex().to_string(),
        prepared_version: prepared.as_ref().map(|(v, _)| *v),
        // The watched basis: every selected source and, when capability rows
        // were selected, the matrix carriers they came from.
        source_revisions: selection
            .sources
            .iter()
            .map(|s| (s.absolute.display().to_string(), s.revision.clone()))
            .chain(
                inventory
                    .matrix
                    .iter()
                    .filter(|_| !selection.capabilities.is_empty())
                    .flat_map(|matrix| {
                        ["manifest", "csv"]
                            .map(|key| matrix["carriers"][key].as_str().map(str::to_owned))
                    })
                    .flatten()
                    .filter_map(|path| {
                        content_revision(Path::new(&path)).map(|revision| (path, revision))
                    }),
            )
            .collect(),
        source_refs: selection
            .sources
            .iter()
            .map(|source| (source.absolute.display().to_string(), source.source.clone()))
            .collect(),
        emitted_at_unix_ms: now_ms(),
        redeliver_pending: false,
        selected: selected_keys(&selection),
        refinement: None,
        ticket,
        loaded: prior.as_ref().map(|l| l.loaded.clone()).unwrap_or_default(),
        invoked: prior
            .as_ref()
            .map(|l| l.invoked.clone())
            .unwrap_or_default(),
        tickets_read,
    };
    store_ledger(request.state, &ledger);
    if refine {
        if let Err(reason) = spawn_refinement(request.client, session, &scope.checkout) {
            store_receipt(
                request.state,
                request.client,
                session,
                &json!({
                    "schema": ENTRY_SCHEMA, "event": "refinement-spawn", "failed": reason,
                }),
            );
        }
    }
    store_receipt(
        request.state,
        request.client,
        session,
        &json!({
            "schema": ENTRY_SCHEMA,
            "event": request.event.kind.as_str(),
            "delivery": delivery,
            "scope": scope,
            "concern": concern,
            "depth": depth,
            "mode": selection.mode,
            "inventory": {
                "sources": inventory.sources.len(),
                "capabilities": inventory.capabilities.len(),
                "praxis": inventory.praxis.len(),
                "matrix": inventory.matrix,
            },
            "selected": {
                "sources": selection.sources.iter().map(|s| format!("{}:{}", s.project, s.path)).collect::<Vec<_>>(),
                "capabilities": selection.capabilities.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
                "methodology": selection.methodology.as_ref().map(|p| p.id.clone()),
                "method": selection.method.as_ref().map(|p| p.id.clone()),
                "skills": selection.skills.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
                "ticket": selection.ticket.as_ref().map(ClaimedTicket::label),
                "mandated": selection.mandated.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
            },
            "decision": selection.decision,
            "degradations": selection.degradations,
            "prepared": prepared.as_ref().map(|(v, d)| json!({"version": v, "digest": d})),
            "rendered_chars": rendered.chars().count(),
            "rendered_digest": ledger.rendered_digest,
            "elapsed_ms": {"inventory": inventory_ms, "selection": selection_ms, "total": started.elapsed().as_millis() as u64},
        }),
    );
    Ok(rendered)
}

/// Start the provider refinement as its own process group, so a harness
/// killing the hook process does not kill the refinement with it.
fn merge_additive(ordinary: &Selection, judged: &Selection) -> Selection {
    let mut merged = ordinary.clone();
    merged.mode = judged.mode.clone();
    merged.decision = judged.decision.clone();
    merged.degradations = judged.degradations.clone();
    for source in &judged.sources {
        if !merged.sources.iter().any(|s| s.absolute == source.absolute) {
            merged.sources.push(source.clone());
        }
    }
    for capability in &judged.capabilities {
        if !merged.capabilities.iter().any(|c| c.id == capability.id) {
            merged.capabilities.push(capability.clone());
        }
    }
    if merged.methodology.is_none() {
        merged.methodology = judged.methodology.clone();
    }
    if merged.method.is_none() {
        merged.method = judged.method.clone();
    }
    for skill in &judged.skills {
        if !merged.skills.iter().any(|p| p.id == skill.id) {
            merged.skills.push(skill.clone());
        }
    }
    merged
}

/// The refinement's own command line, after the executable: the CLI parses
/// exactly this (`now-context entry-refine`), and a test holds the two
/// together — a refinement spawned to a command the CLI no longer has fails
/// out of sight, and the promised refinement never comes.
fn refinement_args(client: &str, session: &str, cwd: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = [
        "--json",
        "now-context",
        "entry-refine",
        "--client",
        client,
        "--session",
        session,
        "--cwd",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(cwd.as_os_str().to_owned());
    args
}

fn spawn_refinement(client: &str, session: &str, cwd: &Path) -> std::result::Result<(), String> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .args(refinement_args(client, session, cwd))
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The detached refinement: the elected decision provider judges the same
/// inventory for the recorded concern, and its selection is published by
/// compare-and-swap against the version the hook emitted. A preparation that
/// moved on in the meantime makes the store refuse it as stale — a late
/// decision never replaces newer context. What changed waits in the
/// session's ledger for its next prompt.
pub fn refine(cwd: &Path, client: &str, session: &str) -> Result<Value> {
    let started = Instant::now();
    let service = crate::app::Service::discover(cwd)?;
    let id = CapsuleId::parse(CAPABILITY)?;
    let active = service.resolved().active.get(&id).ok_or_else(|| {
        fail(
            "development_entry.not_selected",
            "the development-entry capsule is not active here",
        )
    })?;
    let config = EntryConfig::from_table(&active.config)?;
    let (Some(provider_file), Some(threshold)) =
        (config.provider_file.clone(), config.relevance_threshold)
    else {
        return Err(fail(
            "development_entry.provider_unconfigured",
            "refinement needs provider_file and an explicit relevance_threshold",
        ));
    };
    let state = service.home().state();
    let central = crate::temporal::central_root_enclosing(Some(cwd)).ok_or_else(|| {
        fail(
            "development_entry.no_world",
            "no Central world encloses this checkout",
        )
    })?;
    let scope = resolve_scope_current(&central, cwd)?.ok_or_else(|| {
        fail(
            "development_entry.no_project",
            "this checkout is not a Project",
        )
    })?;
    let ledger = load_ledger(&state, client, session).ok_or_else(|| {
        fail(
            "development_entry.no_entry",
            "this session has no emitted entry to refine",
        )
    })?;
    let roots = service.snapshot().capsule_roots();
    let depth = classify_depth(&ledger.concern);
    let mut inventory = collect_inventory(
        &central,
        &scope,
        &ledger.concern,
        service.resolved(),
        &roots,
    );
    if let Some(ticket) = &ledger.ticket {
        mandate(
            &mut inventory,
            ticket,
            &active_catalogue(service.resolved(), &roots),
        );
    }
    let judged = provider_selection(
        &inventory,
        depth,
        &ledger.concern,
        &scope,
        &provider_file,
        threshold,
    );
    // Additive only. In the matched trials the elected local model dropped the
    // correct capability row and Methodology the ordinary ranking had found;
    // an optional decision service may add what the ranking missed, but what
    // it judges immaterial stays in the entry and is reported as its view.
    let mut refined = merge_additive(&ordinary_selection(&inventory, depth), &judged);
    refined.ticket = ledger.ticket.clone();
    let provider_failed = judged.mode != "provider";
    let mut outcome = json!({
        "schema": ENTRY_SCHEMA, "event": "refinement", "session": session,
        "base_version": ledger.prepared_version, "mode": refined.mode,
        "decision": refined.decision, "degradations": refined.degradations,
        "selected": selected_keys(&refined),
    });
    if provider_failed {
        outcome["outcome"] = json!("provider-unavailable; the emitted ordinary entry stands");
        // The entry promised a refinement; the body learns it is not coming.
        let mut current = load_ledger(&state, client, session).unwrap_or(ledger.clone());
        if current.prepared_version == ledger.prepared_version {
            current.refinement = Some(format!(
                "[Development entry: decision-provider refinement unavailable] {}. The ordinary selection already delivered stands; nothing was refined.",
                refined.degradations.first().cloned().unwrap_or_else(|| "the elected provider did not answer".into())
            ));
            store_ledger(&state, &current);
        }
    } else {
        let text = render(&scope, &ledger.concern, &refined, None);
        let mut published = None;
        if let Some(Ok(store)) = redis_store(&config) {
            match publish_view(
                &store,
                client,
                session,
                &scope,
                &ledger.concern,
                &refined,
                &text,
                ledger.prepared_version,
            ) {
                Ok(done) => published = Some(done),
                Err(error) => {
                    outcome["outcome"] =
                        json!(format!("refused: {}: {}", error.code(), error.message()));
                }
            }
        }
        // The ledger is the session's own record; re-read it so a prompt that
        // re-prepared meanwhile is never overwritten by this late answer.
        let mut current = load_ledger(&state, client, session).unwrap_or(ledger.clone());
        let moved = current.prepared_version != ledger.prepared_version
            || current.rendered_digest != ledger.rendered_digest;
        if outcome.get("outcome").is_none() {
            if moved {
                outcome["outcome"] = json!(
                    "refused: the session prepared a newer entry while the provider was deciding"
                );
            } else {
                let mut delta = render_refinement(
                    &ledger.selected,
                    &refined,
                    published.as_ref().map(|(v, _)| *v),
                );
                let judged_keys = selected_keys(&judged);
                let doubted: Vec<&String> = ledger
                    .selected
                    .iter()
                    .filter(|k| !judged_keys.contains(k))
                    .collect();
                if !doubted.is_empty() {
                    let note =
                        format!(
                        "  (the provider judged these not material; they stay in your entry: {})",
                        doubted.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
                    );
                    delta = Some(match delta {
                        Some(text) => format!("{text}\n{note}"),
                        None => format!(
                            "[Development entry reviewed] the elected decision provider added nothing.\n{note}"
                        ),
                    });
                }
                outcome["outcome"] = json!(if delta.is_some() {
                    "refined"
                } else {
                    "confirmed-unchanged"
                });
                current.refinement = delta;
                current.selected = selected_keys(&refined);
                if let Some((version, _)) = &published {
                    current.prepared_version = Some(*version);
                }
                for source in &refined.sources {
                    current.source_revisions.insert(
                        source.absolute.display().to_string(),
                        source.revision.clone(),
                    );
                }
                store_ledger(&state, &current);
            }
        }
        outcome["published"] = json!(published.map(|(v, d)| json!({"version": v, "digest": d})));
    }
    outcome["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
    store_receipt(&state, client, session, &outcome);
    Ok(outcome)
}

/// The work a body is carrying, exactly as the World join resolved it.
/// Restored with the occupancy readers that consume it (`refocus`,
/// `inhabitation`); the entry pipeline itself no longer names it.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkBinding {
    pub central_root: PathBuf,
    pub project: String,
    pub project_root: PathBuf,
    pub factory_state: PathBuf,
    pub position_ref: String,
    pub occupant_generation: String,
    pub work_ref: Option<String>,
    pub run_ref: String,
    pub workflow_unit_ref: String,
    /// The work's source-qualified child NOW, when Factory attaches one.
    /// Ordinary commissioned work has none; the work is still the work.
    pub child_now_ref: Option<String>,
    pub work_digest: Option<String>,
    /// The `factory development workflow-unit` reading.
    pub workflow_unit: serde_json::Value,
}

impl WorkBinding {
    /// `None` unless the join resolved exactly one current work with its
    /// Run, WorkflowUnit and source-qualified child NOW. Ambiguous or absent
    /// work has no entry: Factory refuses to guess, and so does this.
    pub fn from_joined(joined: &Joined) -> Option<Self> {
        let reading = &joined.reading;
        let identity = &reading.identity;
        if identity.current_work_outcome.as_deref() != Some("one") {
            return None;
        }
        // The child NOW follows the join's own verdict: present is prepared;
        // absent (Factory custody carries no work child NOW — ordinary
        // commissioned work) is delivered without a prepared view; a refusal
        // (stale custody, conflicting owners) means no entry at all.
        let child_now_ref = match reading.facets.child_now.state {
            aikit_core::inhabitation::FacetState::Present => Some(identity.child_now_ref.clone()?),
            aikit_core::inhabitation::FacetState::Absent => None,
            _ => return None,
        };
        let refs = &identity.current_work_refs;
        Some(Self {
            central_root: PathBuf::from(joined.trail.central_root.as_ref()?),
            project: joined.trail.project_name.clone()?,
            project_root: joined.trail.project_root.clone()?,
            factory_state: PathBuf::from(joined.trail.factory_state.as_ref()?),
            position_ref: reading.position_ref.clone()?,
            occupant_generation: reading.occupant_generation.clone()?,
            work_ref: refs.get("work_ref").cloned(),
            run_ref: refs.get("run_ref")?.clone(),
            workflow_unit_ref: refs.get("workflow_unit_ref")?.clone(),
            child_now_ref,
            work_digest: identity.current_work_digest.clone(),
            workflow_unit: joined.trail.workflow_unit.clone()?,
        })
    }

    pub fn participant_ref(&self) -> String {
        format!("participant/{}", self.position_ref)
    }
}

/// The authored concern of the work: the unit's own developmental concern and
/// the difference it must make — never the text of a chat turn.
pub fn work_concern(binding: &WorkBinding) -> Option<String> {
    let concern = unit_field(&binding.workflow_unit, "developmental_concern")?;
    Some(
        match unit_field(&binding.workflow_unit, "required_difference") {
            Some(difference) => format!("{concern} — required difference: {difference}"),
            None => concern,
        },
    )
}

/// The `factory.workflow-unit-reading/v1` field for a snake-case name: the
/// reading spells it camelCase (`developmentalConcern`); the Return is nested
/// (`requiredReturn.contract` / `.address`); agent requirements nest under
/// `agentRequirements`.
fn unit_value<'a>(unit: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    let camel = {
        let mut out = String::new();
        let mut upper = false;
        for c in key.chars() {
            if c == '_' {
                upper = true;
            } else if upper {
                out.extend(c.to_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        out
    };
    let nested = match key {
        "required_return_contract" => &unit["requiredReturn"]["contract"],
        "required_return_address" => &unit["requiredReturn"]["address"],
        "agent_refs" => &unit["agentRequirements"]["agentRefs"],
        "agent_set_refs" => &unit["agentRequirements"]["agentSetRefs"],
        _ => &serde_json::Value::Null,
    };
    [&unit[camel.as_str()], &unit[key], nested]
        .into_iter()
        .find(|value| !value.is_null())
        .unwrap_or(&serde_json::Value::Null)
}

fn unit_field(unit: &serde_json::Value, key: &str) -> Option<String> {
    unit_value(unit, key)
        .as_str()
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
}

// The claimed ticket (O-I 723). A session working a Wayfinder map is given
// its practice by the map's Notes and its ticket's type, read from the forge,
// not by shared words; word-matching stays the fallback where the forge
// cannot be read. What the session loads is read from its own tool events,
// and the close of the ticket is checked against them.

/// What the Wayfinder resolution owes (TRACKER.md, Resolving, step 1), as the
/// entry states it to the session.
const RESOLUTION_WORDS: &str = "the result comment names the skills this session loaded, on a line beginning `Skills:`. Closing the ticket is refused until this session has loaded its type's practice and the Notes' skills, and every skill that line names is checked against what this session loaded";

/// One forge read, and every read the hook makes for one event together:
/// the hook runs in front of the turn.
const FORGE_CALL: Duration = Duration::from_secs(3);
const FORGE_BUDGET: Duration = Duration::from_secs(5);
/// How many of the issue references a concern names are tried as tickets.
const TICKET_CANDIDATES: usize = 2;

/// The Wayfinder ticket a session works, as the forge read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedTicket {
    pub repo: String,
    pub number: u64,
    pub title: String,
    /// The type its `wayfinder:<type>` label gives.
    pub kind: String,
    /// Its `claim:*` label, when it carries one.
    pub claim: Option<String>,
    /// The `wayfinder:map` issue it hangs from.
    pub map: Option<u64>,
    /// The active skills the map's Notes name for every session, in order.
    pub notes: Vec<String>,
    /// The Notes' further sentences that name skills by the work: the
    /// session judges which apply, so they are carried, not mandated.
    pub by_work: Vec<String>,
    /// The active practice the ticket's type selects.
    pub method: Option<String>,
    /// What could not be read or resolved, in words.
    pub gaps: Vec<String>,
}

impl ClaimedTicket {
    fn label(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    /// Each mandated practice name with the reason the ticket gives for it.
    fn mandates(&self) -> Vec<(String, String)> {
        let map = self
            .map
            .map(|n| format!("map #{n}"))
            .unwrap_or_else(|| "map".into());
        self.notes
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    format!("the {map}'s Notes: every session loads it before its first act"),
                )
            })
            .chain(self.method.iter().map(|name| {
                (
                    name.clone(),
                    format!(
                        "#{} is `wayfinder:{}`, and its type selects it",
                        self.number, self.kind
                    ),
                )
            }))
            .collect()
    }
}

/// The active praxis catalogue as candidates, for what a ticket mandates
/// whether or not the concern's words found it.
fn active_catalogue(
    view: &ResolvedView,
    capsule_roots: &BTreeMap<CapsuleId, PathBuf>,
) -> Vec<PraxisCandidate> {
    view.catalog_index
        .values()
        .filter(|entry| entry.kind == Kind::Skill && view.is_active(&entry.id))
        .map(|entry| PraxisCandidate {
            id: entry.id.to_string(),
            name: entry.name.clone(),
            form: praxis_form(&entry.description).as_str().to_owned(),
            payload: praxis_payload(&entry.description)
                .chars()
                .take(220)
                .collect(),
            skill_file: capsule_roots
                .get(&entry.id)
                .map(|root| root.join("payload/SKILL.md"))
                .filter(|path| path.is_file()),
            score: 0.0,
            mandatory: false,
            because: None,
        })
        .collect()
}

/// A catalogue entry answers to its name and to its id's last segment.
fn names_practice(candidate: &PraxisCandidate, name: &str) -> bool {
    candidate.name == name || candidate.id.rsplit('/').next() == Some(name)
}

/// The forge repository a remote URL names (`git@github.com:o/r.git`,
/// `https://github.com/o/r`).
fn forge_repo(remote: &str) -> Option<String> {
    let rest = remote
        .trim()
        .strip_prefix("git@github.com:")
        .or_else(|| remote.trim().split_once("github.com/").map(|(_, r)| r))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

fn repo_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// The issue references a text names, in order and once each: a github.com
/// issue URL, `owner/repo#n`, or a bare `#n` against `origin`.
pub fn ticket_references(text: &str, origin: Option<&str>) -> Vec<(String, u64)> {
    let mut found: Vec<(usize, String, u64)> = Vec::new();
    let digits = |s: &str| -> Option<u64> {
        let run: String = s.chars().take_while(char::is_ascii_digit).collect();
        let after = s[run.len()..].chars().next();
        (!run.is_empty() && run.len() <= 7 && !after.is_some_and(|c| c.is_ascii_alphanumeric()))
            .then(|| run.parse().ok())
            .flatten()
    };
    for (at, _) in text.match_indices("github.com/") {
        let rest = &text[at + "github.com/".len()..];
        let mut parts = rest.splitn(4, '/');
        if let (Some(owner), Some(repo), Some("issues"), Some(tail)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        {
            if let Some(number) = digits(tail) {
                found.push((at, format!("{owner}/{repo}"), number));
            }
        }
    }
    for (at, _) in text.match_indices('#') {
        let Some(number) = digits(&text[at + 1..]) else {
            continue;
        };
        let before = &text[..at];
        if before.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '/') {
            // `owner/repo#n`, or a word glued to `#` (an anchor, a colour).
            let start = before
                .char_indices()
                .rev()
                .find(|(_, c)| !(repo_char(*c) || *c == '/'))
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            let named = &before[start..];
            let mut parts = named.split('/');
            if let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) {
                if !owner.is_empty() && !repo.is_empty() && !named.contains("github.com") {
                    found.push((start, format!("{owner}/{repo}"), number));
                }
            }
        } else if let Some(origin) = origin {
            found.push((at, origin.to_owned(), number));
        }
    }
    found.sort_by_key(|(at, _, _)| *at);
    let mut seen = BTreeSet::new();
    found
        .into_iter()
        .filter(|(_, repo, number)| seen.insert(format!("{}#{number}", repo.to_lowercase())))
        .map(|(_, repo, number)| (repo, number))
        .collect()
}

/// The forge client: `AIKIT_GH_BIN` when set, else `gh` on PATH.
fn forge_binary() -> Option<PathBuf> {
    std::env::var_os("AIKIT_GH_BIN")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::probe::which("gh"))
}

/// The forge's answer to one read, as JSON, inside the remaining budget.
fn forge_json(
    runner: &dyn CommandRunner,
    gh: &Path,
    args: &[String],
    deadline: Instant,
) -> std::result::Result<Value, String> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err("the forge reads ran past their budget".into());
    }
    let mut argv = vec![gh.display().to_string()];
    argv.extend(args.iter().cloned());
    let shown = format!("gh {}", args.join(" "));
    let output = runner
        .run_with_timeout(&argv, left.min(FORGE_CALL))
        .map_err(|error| format!("`{shown}` did not answer: {}", error.message()))?;
    if !output.ok() {
        return Err(format!(
            "`{shown}` exited with status {}: {}",
            output.status,
            bounded(output.stderr.trim(), 160)
        ));
    }
    serde_json::from_str(&output.stdout)
        .map_err(|error| format!("`{shown}` answered unreadable JSON: {error}"))
}

fn label_names(issue: &Value) -> Vec<String> {
    issue["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|label| label["name"].as_str().or_else(|| label.as_str()))
        .map(str::to_owned)
        .collect()
}

/// The skills a map's Notes name: the sentence naming what every session
/// loads (mandatory), and the further sentences naming skills by the work.
pub fn notes_skills(body: &str) -> (Vec<String>, Vec<String>) {
    let mut in_notes = false;
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_notes = heading.trim().eq_ignore_ascii_case("notes");
            continue;
        }
        if !in_notes || !line.to_lowercase().contains("every session") {
            continue;
        }
        let text = line.trim().trim_start_matches(['-', '*', ' ']);
        let mut every = Vec::new();
        let mut by_work = Vec::new();
        for (index, sentence) in text.split(". ").enumerate() {
            let names: Vec<String> = sentence
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|name| {
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
                })
                .map(str::to_owned)
                .collect();
            if index == 0 || sentence.to_lowercase().contains("every session") {
                every.extend(names);
            } else if !names.is_empty() {
                by_work.push(sentence.trim().trim_end_matches('.').to_owned());
            }
        }
        return (every, by_work);
    }
    (Vec::new(), Vec::new())
}

/// The practice a ticket's type selects: the active entry named for the type
/// (`grilling`, `prototype`, `research`), else the one active Method whose
/// name ends in `-<type>` (`task` → `central-authorised-task`).
fn type_practice(kind: &str, catalogue: &[PraxisCandidate]) -> std::result::Result<String, String> {
    if let Some(exact) = catalogue.iter().find(|p| names_practice(p, kind)) {
        return Ok(exact.name.clone());
    }
    let suffix = format!("-{kind}");
    let methods: Vec<&PraxisCandidate> = catalogue
        .iter()
        .filter(|p| p.form == PraxisForm::Method.as_str() && p.name.ends_with(&suffix))
        .collect();
    match methods.as_slice() {
        [one] => Ok(one.name.clone()),
        [] => Err(format!(
            "the ticket's type `{kind}` names no active practice here; its Method is not checked"
        )),
        many => Err(format!(
            "the ticket's type `{kind}` matches {} active Methods ({}); none is mandated",
            many.len(),
            many.iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Read one issue as a Wayfinder ticket: `Ok(None)` when it is not one (no
/// `wayfinder:<type>` label, the map itself, a pull request).
pub fn read_ticket(
    runner: &dyn CommandRunner,
    gh: &Path,
    repo: &str,
    number: u64,
    catalogue: &[PraxisCandidate],
    deadline: Instant,
) -> std::result::Result<Option<ClaimedTicket>, String> {
    let issue = forge_json(
        runner,
        gh,
        &["api".into(), format!("repos/{repo}/issues/{number}")],
        deadline,
    )?;
    let labels = label_names(&issue);
    let kind = labels
        .iter()
        .filter_map(|label| label.strip_prefix("wayfinder:"))
        .find(|kind| *kind != "map");
    let (Some(kind), false) = (kind, issue.get("pull_request").is_some()) else {
        return Ok(None);
    };
    let mut ticket = ClaimedTicket {
        repo: repo.to_owned(),
        number,
        title: issue["title"].as_str().unwrap_or_default().to_owned(),
        kind: kind.to_owned(),
        claim: labels.iter().find(|l| l.starts_with("claim:")).cloned(),
        map: None,
        notes: Vec::new(),
        by_work: Vec::new(),
        method: None,
        gaps: Vec::new(),
    };
    match type_practice(kind, catalogue) {
        Ok(name) => ticket.method = Some(name),
        Err(gap) => ticket.gaps.push(gap),
    }
    match forge_json(
        runner,
        gh,
        &["api".into(), format!("repos/{repo}/issues/{number}/parent")],
        deadline,
    ) {
        Ok(parent) if label_names(&parent).iter().any(|l| l == "wayfinder:map") => {
            ticket.map = parent["number"].as_u64();
            let (every, by_work) = notes_skills(parent["body"].as_str().unwrap_or_default());
            if every.is_empty() {
                ticket.gaps.push(format!(
                    "map #{} has no Notes line naming what every session loads",
                    ticket.map.unwrap_or_default()
                ));
            }
            for name in every {
                match catalogue.iter().find(|p| names_practice(p, &name)) {
                    Some(found) if !ticket.notes.contains(&found.name) => {
                        ticket.notes.push(found.name.clone())
                    }
                    Some(_) => {}
                    None => ticket.gaps.push(format!(
                        "the map's Notes name `{name}`, which no active skill here carries"
                    )),
                }
            }
            ticket.by_work = by_work;
        }
        Ok(_) => ticket
            .gaps
            .push("its parent is not a `wayfinder:map`; no Notes were read".into()),
        Err(error) => ticket
            .gaps
            .push(format!("its map could not be read ({error})")),
    }
    Ok(Some(ticket))
}

/// The forge repository of a checkout, from its `origin` remote.
fn checkout_repo(runner: &dyn CommandRunner, checkout: &Path) -> Option<String> {
    let argv = [
        "git".to_owned(),
        "-C".to_owned(),
        checkout.display().to_string(),
        "remote".to_owned(),
        "get-url".to_owned(),
        "origin".to_owned(),
    ];
    let output = runner
        .run_with_timeout(&argv, Duration::from_secs(2))
        .ok()?;
    output.ok().then(|| forge_repo(&output.stdout)).flatten()
}

/// The first of a text's issue references that is a Wayfinder ticket.
/// References already tried are not read again; what failed is in `gaps`.
pub fn ticket_named(
    runner: &dyn CommandRunner,
    gh: Option<&Path>,
    checkout: &Path,
    text: &str,
    catalogue: &[PraxisCandidate],
    tried: &mut BTreeSet<String>,
    gaps: &mut Vec<String>,
) -> Option<ClaimedTicket> {
    if !text.contains('#') && !text.contains("github.com/") {
        return None;
    }
    let origin = text
        .contains('#')
        .then(|| checkout_repo(runner, checkout))
        .flatten();
    let candidates: Vec<(String, u64)> = ticket_references(text, origin.as_deref())
        .into_iter()
        .filter(|(repo, number)| !tried.contains(&format!("{}#{number}", repo.to_lowercase())))
        .take(TICKET_CANDIDATES)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    for (repo, number) in &candidates {
        tried.insert(format!("{}#{number}", repo.to_lowercase()));
    }
    let Some(gh) = gh else {
        gaps.push("the concern names an issue, but no `gh` is on PATH (or AIKIT_GH_BIN); its ticket was not read, and practice is word-matched".into());
        return None;
    };
    let deadline = Instant::now() + FORGE_BUDGET;
    for (repo, number) in candidates {
        match read_ticket(runner, gh, &repo, number, catalogue, deadline) {
            Ok(Some(ticket)) => return Some(ticket),
            Ok(None) => {}
            Err(error) => gaps.push(format!(
                "{repo}#{number} could not be read as a ticket ({error}); practice is word-matched"
            )),
        }
    }
    None
}

/// Make the ticket's practice mandatory in the inventory, first and in the
/// ticket's order, whether or not the concern's words found it.
pub fn mandate(inventory: &mut Inventory, ticket: &ClaimedTicket, catalogue: &[PraxisCandidate]) {
    let mut mandated = Vec::new();
    for (name, because) in ticket.mandates() {
        if mandated
            .iter()
            .any(|p: &PraxisCandidate| names_practice(p, &name))
        {
            continue;
        }
        let found = match inventory
            .praxis
            .iter()
            .position(|p| names_practice(p, &name))
        {
            Some(index) => Some(inventory.praxis.remove(index)),
            None => catalogue.iter().find(|p| names_practice(p, &name)).cloned(),
        };
        if let Some(mut practice) = found {
            practice.mandatory = true;
            practice.because = Some(because);
            mandated.push(practice);
        }
    }
    inventory.praxis.splice(0..0, mandated);
}

/// The ticket's lines in an entry: what it is, the practice it mandates, and
/// what its resolution owes.
fn ticket_lines(ticket: &ClaimedTicket, mandated: &[PraxisCandidate]) -> Vec<String> {
    let mut lines = vec![format!(
        "ticket: {} `wayfinder:{}` · {} · {} — {}",
        ticket.label(),
        ticket.kind,
        ticket
            .claim
            .clone()
            .unwrap_or_else(|| "unclaimed: claim it before any work".into()),
        ticket
            .map
            .map(|n| format!("map #{n}"))
            .unwrap_or_else(|| "no map read".into()),
        bounded(&ticket.title, 100)
    )];
    if !mandated.is_empty() {
        lines.push("practice the ticket names (mandatory: load each before the first act):".into());
        for p in mandated {
            lines.push(format!(
                "  - {} {} — {} {}",
                p.form.to_uppercase(),
                p.id,
                p.because.as_deref().unwrap_or_default(),
                p.skill_file
                    .as_ref()
                    .map(|f| format!("(read: {})", f.display()))
                    .unwrap_or_default()
            ));
        }
    }
    if !ticket.by_work.is_empty() {
        lines.push(format!(
            "the map's Notes add, by the work: {}",
            bounded(&ticket.by_work.join("; "), 300)
        ));
    }
    lines.push(format!("resolution: {RESOLUTION_WORDS}"));
    lines
}

/// The block a session receives when a ticket is read after its entry went
/// out (a later prompt names it, or the session claims it), and the keys of
/// what it mandates.
fn ticket_block(
    heading: &str,
    ticket: &ClaimedTicket,
    catalogue: &[PraxisCandidate],
    gaps: &[String],
) -> (String, Vec<String>) {
    let mut inventory = Inventory {
        sources: Vec::new(),
        capabilities: Vec::new(),
        praxis: Vec::new(),
        matrix: None,
        absences: Vec::new(),
    };
    mandate(&mut inventory, ticket, catalogue);
    let mut lines = vec![heading.to_owned()];
    lines.extend(
        gaps.iter()
            .chain(&ticket.gaps)
            .map(|gap| format!("degraded: {gap}")),
    );
    lines.extend(ticket_lines(ticket, &inventory.praxis));
    let keys = inventory
        .praxis
        .iter()
        .map(|p| format!("praxis:{}", p.id))
        .collect();
    (bounded(&lines.join("\n"), MAX_RENDERED_CHARS), keys)
}

/// A practice name as a harness reports it: a plugin prefix and a leading
/// slash dropped.
fn practice_name(raw: &str) -> String {
    let name = raw.trim().trim_matches(['`', '"', '\'', '/']);
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

/// The practice a skill file's path names: `<name>/SKILL.md`, or an AIKit
/// capsule's `<name>/payload/SKILL.md`.
fn skill_file_name(path: &str) -> Option<String> {
    let dir = path.strip_suffix("/SKILL.md")?;
    let dir = dir.strip_suffix("/payload").unwrap_or(dir);
    let name = dir.rsplit('/').next()?;
    (!name.is_empty() && name != "payload").then(|| name.to_owned())
}

/// The command a tool call runs, whichever harness raised it.
fn tool_command(payload: &Value) -> Option<String> {
    let input = payload
        .get("tool_input")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("arguments"))?;
    let command = input.get("command").or_else(|| input.get("cmd"))?;
    match command {
        Value::String(text) => Some(text.clone()),
        Value::Array(words) => Some(
            words
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

/// The practice one finished tool call invoked by name: a skill call.
pub fn practice_invoked(event: &HookEvent) -> Option<String> {
    let payload = &event.payload;
    let input = payload
        .get("tool_input")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("arguments"))?;
    event
        .tool_name
        .as_deref()
        .filter(|tool| tool.eq_ignore_ascii_case("skill"))?;
    input
        .get("skill")
        .or_else(|| input.get("name"))
        .and_then(Value::as_str)
        .map(practice_name)
}

/// The practice one finished tool call loaded: a skill invoked by name, or a
/// skill file read or printed.
pub fn practice_loaded(event: &HookEvent) -> Vec<String> {
    let payload = &event.payload;
    let input = payload
        .get("tool_input")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("arguments"));
    let mut names: Vec<String> = practice_invoked(event).into_iter().collect();
    let mut texts: Vec<String> = ["file_path", "path", "filePath"]
        .iter()
        .filter_map(|key| input.and_then(|i| i.get(*key)).and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    texts.extend(tool_command(payload));
    for text in texts {
        for (at, _) in text.match_indices("SKILL.md") {
            let start = text[..at]
                .rfind(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '=' | '(' | '<'))
                .map(|i| i + 1)
                .unwrap_or(0);
            if let Some(name) = skill_file_name(&text[start..at + "SKILL.md".len()]) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// Shell words of a command line, enough to read a `gh` invocation: quotes
/// group, a backslash escapes, and a control operator ends one command.
fn shell_commands(line: &str) -> Vec<Vec<String>> {
    let mut commands: Vec<Vec<String>> = vec![Vec::new()];
    let mut word = String::new();
    let mut open = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    fn flush(word: &mut String, open: &mut bool, commands: &mut [Vec<String>]) {
        if *open {
            if let Some(last) = commands.last_mut() {
                last.push(std::mem::take(word));
            }
            *open = false;
        }
    }
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') if c == '\'' => quote = None,
            Some('\'') => word.push(c),
            Some(_) if c == '"' => quote = None,
            Some(_) if c == '\\' => word.extend(chars.next()),
            Some(_) => word.push(c),
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    open = true;
                }
                '\\' => {
                    if let Some(next) = chars.next().filter(|n| *n != '\n') {
                        word.push(next);
                        open = true;
                    }
                }
                ' ' | '\t' => flush(&mut word, &mut open, &mut commands),
                '\n' | ';' | '|' | '&' => {
                    flush(&mut word, &mut open, &mut commands);
                    if commands.last().is_some_and(|last| !last.is_empty()) {
                        commands.push(Vec::new());
                    }
                }
                _ => {
                    word.push(c);
                    open = true;
                }
            },
        }
    }
    flush(&mut word, &mut open, &mut commands);
    commands.retain(|command| !command.is_empty());
    commands
}

/// The issue a `gh` command acts on, with the repository its `--repo` names:
/// `gh issue <verb> <n|url>`, or `gh api repos/o/r/issues/<n> … state=closed`
/// for the close.
/// The value a command's flag carries (`--repo x`, `--repo=x`).
fn flag_value(words: &[String], names: &[&str]) -> Option<String> {
    words.iter().enumerate().find_map(|(i, word)| {
        names.iter().find_map(|name| {
            if word == name {
                words.get(i + 1).cloned()
            } else {
                word.strip_prefix(&format!("{name}=")).map(str::to_owned)
            }
        })
    })
}

fn gh_issue_target(words: &[String], verb: &str) -> Option<(Option<String>, u64)> {
    let gh = words.iter().position(|w| w == "gh" || w.ends_with("/gh"))?;
    let rest = &words[gh + 1..];
    let repo = flag_value(rest, &["--repo", "-R"]);
    if rest.first().map(String::as_str) == Some("issue")
        && rest.get(1).map(String::as_str) == Some(verb)
    {
        // The issue is the first word that is a number or an issue URL; a
        // flag's value (`--comment "…"`) is neither.
        return rest[2..].iter().find_map(|word| {
            if let Ok(number) = word.parse::<u64>() {
                return Some((repo.clone(), number));
            }
            word.contains("/issues/")
                .then(|| ticket_references(word, None).into_iter().next())
                .flatten()
                .map(|(url_repo, number)| (Some(url_repo), number))
        });
    }
    if verb == "close"
        && rest.first().map(String::as_str) == Some("api")
        && rest.iter().any(|w| w.ends_with("state=closed"))
    {
        return rest.iter().find_map(|w| {
            let path = w.trim_start_matches('/').strip_prefix("repos/")?;
            let mut parts = path.split('/');
            let (owner, name, issues, number) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            (issues == "issues" && parts.next().is_none())
                .then(|| {
                    number
                        .parse()
                        .ok()
                        .map(|n| (Some(format!("{owner}/{name}")), n))
                })
                .flatten()
        });
    }
    None
}

/// The skills a resolution claims: the names on its line beginning
/// `Skills` (`Skills loaded: a, b`, `**Skills:** `a`, `b``).
pub fn claimed_skills(text: &str) -> Option<Vec<String>> {
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '>', ' ']);
        if !line.to_lowercase().starts_with("skills") {
            continue;
        }
        let Some((_, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('*').trim();
        let names: Vec<String> = if rest.contains('`') {
            rest.split('`')
                .skip(1)
                .step_by(2)
                .map(practice_name)
                .collect()
        } else {
            rest.replace(" and ", ",")
                .split([',', ';'])
                .filter_map(|part| part.split_whitespace().next())
                .map(|word| practice_name(word.trim_end_matches(['.', ')'])))
                .collect()
        };
        return Some(names.into_iter().filter(|n| !n.is_empty()).collect());
    }
    None
}

/// Why the close a command makes must be refused, or `None` when the
/// command closes nothing this session claimed. The close's own `--comment`
/// is the resolution when it carries the skills line; otherwise
/// `latest_comment` reads the ticket's latest comment from the forge.
pub fn resolution_refusal(
    command: &str,
    ledger: &EntryLedger,
    latest_comment: impl FnOnce(&ClaimedTicket) -> std::result::Result<Option<String>, String>,
) -> Option<String> {
    resolution_check(command, ledger, latest_comment).and_then(|check| check.refusal)
}

/// A close of the session's claimed ticket, as the use check read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionCheck {
    /// Why the close is refused; `None` lets it through.
    pub refusal: Option<String>,
    /// The practice the resolution names as loaded: what it relied on.
    pub relied_upon: Vec<String>,
}

/// The use check over one command: `None` when it closes nothing this
/// session claimed (see [`resolution_refusal`]).
pub fn resolution_check(
    command: &str,
    ledger: &EntryLedger,
    latest_comment: impl FnOnce(&ClaimedTicket) -> std::result::Result<Option<String>, String>,
) -> Option<ResolutionCheck> {
    let ticket = ledger.ticket.as_ref()?;
    let close = shell_commands(command).into_iter().find(|words| {
        gh_issue_target(words, "close").is_some_and(|(repo, number)| {
            number == ticket.number
                && repo.is_none_or(|repo| repo.eq_ignore_ascii_case(&ticket.repo))
        })
    })?;
    let comment = flag_value(&close, &["--comment", "-c"]);
    let loaded = |name: &str| ledger.loaded.contains(name);
    let mut reasons = Vec::new();
    if let Some(method) = ticket.method.as_deref().filter(|m| !loaded(m)) {
        reasons.push(format!(
            "its type `{}` selects `{method}`, and this session has not loaded it",
            ticket.kind
        ));
    }
    let unloaded: Vec<&str> = ticket
        .notes
        .iter()
        .map(String::as_str)
        .filter(|name| !loaded(name))
        .collect();
    if !unloaded.is_empty() {
        reasons.push(format!(
            "the map's Notes have every session load {}, and this session has not",
            unloaded
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let claimed = match comment.as_deref().and_then(claimed_skills) {
        Some(names) => Ok(Some(names)),
        None => latest_comment(ticket).map(|text| text.as_deref().and_then(claimed_skills)),
    };
    let mut relied_upon = Vec::new();
    match claimed {
        Ok(Some(names)) => {
            relied_upon = names.iter().filter(|n| loaded(n)).cloned().collect();
            let unfounded: Vec<String> = names
                .into_iter()
                .filter(|name| !loaded(name))
                .map(|name| format!("`{name}`"))
                .collect();
            if !unfounded.is_empty() {
                reasons.push(format!(
                    "the result names {} as loaded, and this session never loaded {} (no skill call or skill-file read was recorded)",
                    unfounded.join(", "),
                    if unfounded.len() == 1 { "it" } else { "them" }
                ));
            }
        }
        Ok(None) => reasons.push(
            "the result names no skills: add a line `Skills: …` naming what this session loaded (TRACKER.md, Resolving, step 1)".into(),
        ),
        Err(error) => reasons.push(format!(
            "the result comment could not be read ({error}); put the `Skills: …` line in the close's --comment"
        )),
    }
    let refusal = (!reasons.is_empty()).then(|| {
        format!(
            "closing {} is refused: {}. This session has loaded: {}.",
            ticket.label(),
            reasons.join("; "),
            if ledger.loaded.is_empty() {
                "nothing recorded".to_owned()
            } else {
                ledger.loaded.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        )
    });
    Some(ResolutionCheck {
        refusal,
        relied_upon,
    })
}

/// What a session's close says it did with its practice, as AIKit's
/// operative evidence (`aikit.praxis-activity/v1`, which `aikit praxis
/// disclose --activity` reads; O-I 745's Skillset rack): Loaded and Invoked
/// from the session's own tool events, Relied on from the resolution naming
/// it. Where the two disagree — a loaded practice the resolution leaves out,
/// a name no active entry carries — it is a finding, beside the evidence.
pub fn session_activity(
    ledger: &EntryLedger,
    relied_upon: &[String],
    catalogue: &[PraxisCandidate],
) -> (aikit_core::agent_praxis::PraxisActivity, Vec<String>) {
    let mut findings: Vec<String> = Vec::new();
    let mut ids = |names: &mut dyn Iterator<Item = &String>| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in names {
            match catalogue.iter().find(|p| names_practice(p, name)) {
                Some(found) if !out.contains(&found.id) => out.push(found.id.clone()),
                Some(_) => {}
                None => {
                    let finding =
                        format!("`{name}` was used, and no active entry here carries that name");
                    if !findings.contains(&finding) {
                        findings.push(finding);
                    }
                }
            }
        }
        out
    };
    let loaded = ids(&mut ledger.loaded.iter());
    let invoked = ids(&mut ledger.invoked.iter());
    let relied = ids(&mut relied_upon.iter());
    for name in &ledger.loaded {
        if !relied_upon.contains(name) {
            findings.push(format!(
                "`{name}` was loaded, and the resolution does not name it as relied on"
            ));
        }
    }
    let ticket = ledger.ticket.as_ref();
    let activity = aikit_core::agent_praxis::PraxisActivity {
        schema: aikit_core::agent_praxis::PRAXIS_ACTIVITY_SCHEMA.into(),
        loaded,
        invoked,
        relied_upon: relied,
        succeeded: Vec::new(),
        verified: Vec::new(),
        evidence_refs: ticket
            .map(|t| format!("https://github.com/{}/issues/{}", t.repo, t.number))
            .into_iter()
            .collect(),
        return_destinations: ticket.map(ClaimedTicket::label).into_iter().collect(),
        activity_refs: vec![format!(
            "aikit:development-entry:{}:{}",
            ledger.client, ledger.session
        )],
    };
    (activity, findings)
}

/// Where a session's closing evidence is kept, beside its ledger.
pub fn activity_path(state: &Path, client: &str, session: &str) -> PathBuf {
    ledger_path(state, client, session).with_extension("activity.json")
}

/// The use check, on a PreToolUse: why this tool call's close of the
/// session's claimed ticket must be refused, or `None`. Every other call
/// answers from the command text alone, before any file is read.
pub fn check_resolution(request: &EntryRequest<'_>) -> Result<Option<String>> {
    let event = request.event;
    let Some(command) = tool_command(&event.payload) else {
        return Ok(None);
    };
    if !(command.contains("gh") && command.contains("close")) {
        return Ok(None);
    }
    let Some(session) = crate::refocus::hook_session(&event.payload) else {
        return Ok(None);
    };
    let Some(ledger) = load_ledger(request.state, request.client, &session) else {
        return Ok(None);
    };
    let runner = SystemRunner::new().with_timeout(FORGE_CALL);
    let check = resolution_check(&command, &ledger, |ticket| {
        let gh = forge_binary().ok_or_else(|| "no `gh` is on PATH".to_owned())?;
        let comments = forge_json(
            &runner,
            &gh,
            &[
                "api".into(),
                format!(
                    "repos/{}/issues/{}/comments?per_page=100",
                    ticket.repo, ticket.number
                ),
            ],
            Instant::now() + FORGE_BUDGET,
        )?;
        Ok(comments
            .as_array()
            .and_then(|all| all.last())
            .and_then(|last| last["body"].as_str())
            .map(str::to_owned))
    });
    let Some(check) = check else {
        return Ok(None);
    };
    // A close that goes through leaves the session's evidence of its
    // practice; a refused one leaves only the refusal.
    let mut findings = Vec::new();
    if check.refusal.is_none() {
        let catalogue = active_catalogue(request.view, request.capsule_roots);
        let (activity, found) = session_activity(&ledger, &check.relied_upon, &catalogue);
        findings = found;
        if let Ok(body) = serde_json::to_vec_pretty(&activity) {
            let _ = std::fs::write(activity_path(request.state, request.client, &session), body);
        }
    }
    store_receipt(
        request.state,
        request.client,
        &session,
        &json!({
            "schema": ENTRY_SCHEMA, "event": "PreToolUse", "check": "resolution",
            "ticket": ledger.ticket.as_ref().map(ClaimedTicket::label),
            "loaded": ledger.loaded, "invoked": ledger.invoked,
            "relied_upon": check.relied_upon, "findings": findings,
            "refused": check.refusal,
        }),
    );
    Ok(check.refusal)
}

/// A finished tool call, on a PostToolUse: record the practice it loaded,
/// and when it claimed a ticket the session's entry has not read, read that
/// ticket and return its block. No ledger, no record.
fn observe(request: &EntryRequest<'_>, session: &str) -> Result<Option<String>> {
    let event = request.event;
    let command = tool_command(&event.payload);
    let used = practice_loaded(event);
    let claim = command
        .as_deref()
        .filter(|c| c.contains("claim:"))
        .and_then(|c| {
            shell_commands(c)
                .iter()
                .filter(|words| words.iter().any(|w| w.contains("claim:")))
                .find_map(|words| gh_issue_target(words, "edit"))
        });
    if used.is_empty() && claim.is_none() {
        return Ok(None);
    }
    let Some(mut ledger) = load_ledger(request.state, request.client, session) else {
        return Ok(None);
    };
    let before = ledger.loaded.len() + ledger.invoked.len();
    ledger.loaded.extend(used);
    ledger.invoked.extend(practice_invoked(event));
    let mut text = None;
    if let Some((repo, number)) = claim {
        let held = ledger.ticket.as_ref().is_some_and(|t| t.number == number);
        if !held {
            let runner = SystemRunner::new().with_timeout(FORGE_CALL);
            let catalogue = active_catalogue(request.view, request.capsule_roots);
            let repo = repo.or_else(|| checkout_repo(&runner, &ledger.checkout));
            let mut gaps = Vec::new();
            let ticket = match (repo, forge_binary()) {
                (Some(repo), Some(gh)) => {
                    match read_ticket(
                        &runner,
                        &gh,
                        &repo,
                        number,
                        &catalogue,
                        Instant::now() + FORGE_BUDGET,
                    ) {
                        Ok(ticket) => ticket,
                        Err(error) => {
                            gaps.push(format!("#{number} could not be read as a ticket ({error})"));
                            None
                        }
                    }
                }
                (None, _) => {
                    gaps.push(format!(
                        "#{number}'s repository could not be told from this checkout"
                    ));
                    None
                }
                (_, None) => {
                    gaps.push("no `gh` is on PATH (or AIKIT_GH_BIN)".into());
                    None
                }
            };
            if let Some(ticket) = ticket {
                let (block, keys) = ticket_block(
                    "[Development entry: the ticket this session claimed names its practice]",
                    &ticket,
                    &catalogue,
                    &[],
                );
                text = Some(block);
                ledger.selected.extend(keys);
                ledger.ticket = Some(ticket);
            } else if !gaps.is_empty() {
                text = Some(format!(
                    "[Development entry: claimed ticket not read] {}; practice stays as the entry selected it.",
                    gaps.join("; ")
                ));
            }
        }
    }
    if ledger.loaded.len() + ledger.invoked.len() != before || text.is_some() {
        store_ledger(request.state, &ledger);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_generated_turns_and_commands_are_not_a_concern() {
        assert!(substantive_concern("/compact").is_none());
        assert!(substantive_concern("  ").is_none());
        assert!(
            substantive_concern("<task-notification> agent finished </task-notification>")
                .is_none()
        );
        assert!(substantive_concern("[SYSTEM NOTIFICATION - NOT USER INPUT] done").is_none());
        assert!(substantive_concern("ok").is_none());
        assert_eq!(
            substantive_concern("Fix the wrong exit code in hook dispatch").as_deref(),
            Some("Fix the wrong exit code in hook dispatch")
        );
    }

    #[test]
    fn a_small_local_repair_stays_shallow_and_design_work_is_substantial() {
        assert_eq!(
            classify_depth("fix the typo in the knowledge-route manifest"),
            Depth::Shallow
        );
        assert_eq!(
            classify_depth("the hook dispatch returns the wrong exit code for usage errors"),
            Depth::Shallow
        );
        assert_eq!(
            classify_depth(
                "pi prompts never reach the prompt reader because the carrier sends `text`. \
                 Make the smallest fix and show it as a unified diff with its unit test."
            ),
            Depth::Shallow
        );
        assert_eq!(
            classify_depth("Design how prepared NOW context reaches a direct harness session and reconcile the capability matrix"),
            Depth::Substantial
        );
    }

    fn inventory() -> Inventory {
        let praxis = |id: &str, form: PraxisForm, score: f64| PraxisCandidate {
            id: id.into(),
            name: id.rsplit('/').next().unwrap().into(),
            form: form.as_str().into(),
            payload: String::new(),
            skill_file: None,
            score,
            mandatory: false,
            because: None,
        };
        Inventory {
            sources: vec![],
            capabilities: vec![],
            praxis: vec![
                praxis(
                    "skill/central/docs-methodology",
                    PraxisForm::Methodology,
                    0.5,
                ),
                praxis("skill/oi/capability-matrix", PraxisForm::Method, 0.4),
                praxis("skill/aikit/verification", PraxisForm::Skill, 0.3),
            ],
            matrix: None,
            absences: vec![],
        }
    }

    #[test]
    fn shallow_selection_loads_no_methodology_or_method() {
        let selection = ordinary_selection(&inventory(), Depth::Shallow);
        assert!(selection.methodology.is_none());
        assert!(selection.method.is_none());
        assert!(selection.skills.is_empty());
        let rendered = render(
            &EntryScope {
                project: "ai-kit".into(),
                project_id: "ai-kit".into(),
                primary: "/c/Work/ai-kit".into(),
                checkout: "/c/worktrees/env-2/ai-kit".into(),
                branch: Some("lane".into()),
                head: Some("abc".into()),
            },
            "fix the typo",
            &selection,
            None,
        );
        assert!(rendered.contains("practice: none selected"), "{rendered}");
        assert!(rendered.contains("no Methodology"), "{rendered}");
    }

    #[test]
    fn substantial_selection_names_one_methodology_one_method_and_skills() {
        let selection = ordinary_selection(&inventory(), Depth::Substantial);
        assert_eq!(
            selection.methodology.unwrap().id,
            "skill/central/docs-methodology"
        );
        assert_eq!(selection.method.unwrap().id, "skill/oi/capability-matrix");
        assert_eq!(selection.skills.len(), 1);
    }

    #[test]
    fn explicit_paths_and_capability_ids_are_mandatory_mentions() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::write(temp.path().join("docs/JEV.md"), "x").unwrap();
        let (paths, capabilities) = explicit_mentions(
            "read docs/JEV.md and cap.aikit.continuity; docs/missing.md is not here",
            temp.path(),
        );
        assert_eq!(paths, vec!["docs/JEV.md"]);
        assert_eq!(capabilities, vec!["cap.aikit.continuity"]);
    }

    #[test]
    fn the_ledger_is_keyed_per_client_and_session() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = EntryLedger {
            schema: ENTRY_SCHEMA.into(),
            client: "claude".into(),
            session: "s1".into(),
            concern: "c".into(),
            project: "p".into(),
            checkout: "/x".into(),
            rendered_digest: "d".into(),
            prepared_version: Some(1),
            source_revisions: BTreeMap::new(),
            source_refs: BTreeMap::new(),
            emitted_at_unix_ms: 0,
            redeliver_pending: false,
            selected: vec![],
            refinement: None,
            ticket: None,
            loaded: BTreeSet::new(),
            invoked: BTreeSet::new(),
            tickets_read: BTreeSet::new(),
        };
        store_ledger(temp.path(), &ledger);
        assert_eq!(load_ledger(temp.path(), "claude", "s1"), Some(ledger));
        assert_eq!(load_ledger(temp.path(), "codex", "s1"), None);
    }

    #[test]
    fn a_degradation_survives_the_entry_bound() {
        let mut selection = ordinary_selection(&inventory(), Depth::Substantial);
        for index in 0..80 {
            selection.sources.push(SourceCandidate {
                project: "ai-kit".into(),
                source: work_file_source_ref(
                    "fixture-native-id",
                    Path::new(&format!("docs/long-document-{index}.md")),
                )
                .unwrap(),
                path: format!("docs/long-document-{index}.md"),
                absolute: format!("/c/docs/long-document-{index}.md").into(),
                revision: "blake3:0123456789abcdef".into(),
                score: 0.4,
                snippet: "x".repeat(120),
                line: Some(1),
                authored: true,
                mandatory: false,
            });
        }
        selection
            .degradations
            .push("prepared NOW view not published (now_context.redis_unavailable)".into());
        let rendered = render(
            &EntryScope {
                project: "ai-kit".into(),
                project_id: "ai-kit".into(),
                primary: "/c/Work/ai-kit".into(),
                checkout: "/c/Work/ai-kit".into(),
                branch: None,
                head: None,
            },
            "design something",
            &selection,
            None,
        );
        assert!(rendered.chars().count() <= MAX_RENDERED_CHARS);
        assert!(
            rendered.contains("degraded: prepared NOW view not published"),
            "the unavailable part must not be what the bound cuts"
        );
    }

    #[test]
    fn a_provider_may_add_but_never_drop_what_the_ordinary_ranking_found() {
        let ordinary = ordinary_selection(&inventory(), Depth::Substantial);
        let mut judged = ordinary.clone();
        judged.mode = "provider".into();
        judged.methodology = None;
        judged.skills.clear();
        let extra = PraxisCandidate {
            id: "skill/aikit/verification".into(),
            name: "verification-extra".into(),
            form: "skill".into(),
            payload: String::new(),
            skill_file: None,
            score: 0.1,
            mandatory: false,
            because: None,
        };
        judged.skills.push(PraxisCandidate {
            id: "skill/x/added".into(),
            ..extra
        });
        let merged = merge_additive(&ordinary, &judged);
        assert_eq!(
            merged.methodology, ordinary.methodology,
            "a dropped Methodology stays"
        );
        assert!(merged
            .skills
            .iter()
            .any(|p| p.id == "skill/aikit/verification"));
        assert!(merged.skills.iter().any(|p| p.id == "skill/x/added"));
        assert_eq!(merged.mode, "provider");
    }

    /// Map O-I 706 as the forge answers it, trimmed to its Destination, its
    /// Notes' first two lines and its next heading; the Notes line is verbatim.
    const MAP_706_BODY: &str = "## Destination\n\nThe DLW as the manual describes it, built across O:I and its six products, with every story walked to its declared grade on the native app from its starting world: from a bare machine through Setup to each mode's work on a focused constellation, and on your own world.\n\n## Notes\n\n- **This map carries execution.** A task ticket builds, verifies and lands its change; it does not stop at a decision.\n- **Skills every session loads before its first act:** `wayfinder` (read `Control/user/skills/wayfinder/SKILL.md` until \"Switch on every skill set and project the missing skills\" lands), `agent-worktree-lifecycle`, `engineering-methodology`, `collaboration-methodology`. Documentation tickets add `docs-methodology` and `authorship-methodology`; walks add `experience-campaign`; matrix work adds `capability-matrices`; landing adds `central-git-convergence`. A ticket's type selects its Method: grilling, prototype, research or task.\n\n## Decisions so far\n";

    fn catalogue() -> Vec<PraxisCandidate> {
        let entry = |id: &str, form: PraxisForm| PraxisCandidate {
            id: id.into(),
            name: id.rsplit('/').next().unwrap().into(),
            form: form.as_str().into(),
            payload: String::new(),
            skill_file: None,
            score: 0.0,
            mandatory: false,
            because: None,
        };
        vec![
            entry("skill/central/wayfinder", PraxisForm::Methodology),
            entry("skill/central/agent-worktree-lifecycle", PraxisForm::Skill),
            entry(
                "skill/central/engineering-methodology",
                PraxisForm::Methodology,
            ),
            entry(
                "skill/central/collaboration-methodology",
                PraxisForm::Methodology,
            ),
            entry("skill/central/central-authorised-task", PraxisForm::Method),
            entry("skill/central/grilling", PraxisForm::Method),
            entry("skill/central/docs-methodology", PraxisForm::Methodology),
        ]
    }

    /// The forge as `gh` answers for O-I 723 under map 706, and the
    /// checkout's origin.
    fn forge() -> aikit_adapters::runner::ScriptedRunner {
        aikit_adapters::runner::ScriptedRunner::new()
            .on(
                "remote get-url origin",
                "https://github.com/EpiLogos/O-I.git\n",
            )
            .on(
                "repos/EpiLogos/O-I/issues/706",
                &json!({"number": 706, "title": "[MAP] The DLW", "labels": [{"name": "wayfinder:map"}], "body": MAP_706_BODY}).to_string(),
            )
            .on(
                "repos/EpiLogos/O-I/issues/723",
                &json!({"number": 723, "title": "Let the map and the claimed ticket name each session's skills", "labels": [{"name": "wayfinder:task"}, {"name": "claim:account-2"}]}).to_string(),
            )
            .on(
                "repos/EpiLogos/O-I/issues/723/parent",
                &json!({"number": 706, "labels": [{"name": "wayfinder:map"}], "body": MAP_706_BODY}).to_string(),
            )
    }

    fn claimed_723() -> ClaimedTicket {
        let mut tried = BTreeSet::new();
        let mut gaps = Vec::new();
        let ticket = ticket_named(
            &forge(),
            Some(Path::new("/usr/bin/gh")),
            Path::new("/c/worktrees/env-1/o-i"),
            "work ticket #723",
            &catalogue(),
            &mut tried,
            &mut gaps,
        )
        .expect("723 is a Wayfinder ticket");
        assert!(gaps.is_empty(), "{gaps:?}");
        ticket
    }

    #[test]
    fn issue_references_are_urls_owner_repo_and_bare_numbers() {
        assert_eq!(
            ticket_references(
                "see https://github.com/EpiLogos/O-I/issues/723, EpiLogos/ai-kit#590 and #724; not colour#123 or a#b",
                Some("EpiLogos/O-I"),
            ),
            vec![
                ("EpiLogos/O-I".to_owned(), 723),
                ("EpiLogos/ai-kit".to_owned(), 590),
                ("EpiLogos/O-I".to_owned(), 724),
            ]
        );
        assert!(
            ticket_references("#723", None).is_empty(),
            "a bare number needs the checkout's forge"
        );
        assert_eq!(
            forge_repo("git@github.com:EpiLogos/O-I.git\n").as_deref(),
            Some("EpiLogos/O-I")
        );
    }

    #[test]
    fn the_map_notes_name_what_every_session_loads_and_what_the_work_adds() {
        let (every, by_work) = notes_skills(MAP_706_BODY);
        assert_eq!(
            every,
            [
                "wayfinder",
                "agent-worktree-lifecycle",
                "engineering-methodology",
                "collaboration-methodology"
            ],
            "the skill file's path is not a skill name"
        );
        assert_eq!(by_work.len(), 1, "{by_work:?}");
        assert!(by_work[0].starts_with("Documentation tickets add `docs-methodology`"));
    }

    #[test]
    fn a_claimed_ticket_makes_the_notes_skills_and_its_type_method_mandatory() {
        // The map itself is not a ticket: the first reference is skipped and
        // the second read.
        let runner = forge();
        let mut tried = BTreeSet::new();
        let mut gaps = Vec::new();
        let ticket = ticket_named(
            &runner,
            Some(Path::new("/usr/bin/gh")),
            Path::new("/c/worktrees/env-1/o-i"),
            "the map #706 names it; work ticket #723 now",
            &catalogue(),
            &mut tried,
            &mut gaps,
        )
        .unwrap();
        assert_eq!(ticket.number, 723);
        assert_eq!(ticket.kind, "task");
        assert_eq!(ticket.claim.as_deref(), Some("claim:account-2"));
        assert_eq!(ticket.map, Some(706));
        assert_eq!(
            ticket.method.as_deref(),
            Some("central-authorised-task"),
            "a task ticket's Method is the one active Method ending in -task"
        );
        assert!(ticket.gaps.is_empty(), "{:?}", ticket.gaps);
        assert_eq!(
            tried,
            BTreeSet::from(["epilogos/o-i#706".to_owned(), "epilogos/o-i#723".to_owned()])
        );
        assert!(runner
            .call_lines()
            .iter()
            .all(|line| line.starts_with("git -C") || line.starts_with("/usr/bin/gh api repos/")));

        let mut inventory = inventory();
        mandate(&mut inventory, &ticket, &catalogue());
        let mut selection = ordinary_selection(&inventory, Depth::Shallow);
        selection.ticket = Some(ticket);
        assert_eq!(
            selection
                .mandated
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            [
                "wayfinder",
                "agent-worktree-lifecycle",
                "engineering-methodology",
                "collaboration-methodology",
                "central-authorised-task"
            ],
            "mandatory at any depth, in the ticket's order"
        );
        assert!(selection.mandated.iter().all(|p| p.mandatory));
        let keys = selected_keys(&selection);
        assert!(keys.contains(&"praxis:skill/central/central-authorised-task".to_owned()));
        let rendered = render(
            &EntryScope {
                project: "O-I".into(),
                project_id: "O-I".into(),
                primary: "/c/Work/O-I".into(),
                checkout: "/c/worktrees/env-1/o-i".into(),
                branch: Some("main".into()),
                head: Some("abc".into()),
            },
            "work ticket #723",
            &selection,
            None,
        );
        assert!(
            rendered
                .contains("ticket: EpiLogos/O-I#723 `wayfinder:task` · claim:account-2 · map #706"),
            "{rendered}"
        );
        assert!(
            rendered.contains("practice the ticket names (mandatory"),
            "{rendered}"
        );
        assert!(rendered.contains("SKILL skill/central/agent-worktree-lifecycle — the map #706's Notes: every session loads it"), "{rendered}");
        assert!(rendered.contains("METHOD skill/central/central-authorised-task — #723 is `wayfinder:task`, and its type selects it"), "{rendered}");
        assert!(
            rendered.contains("the map's Notes add, by the work: Documentation tickets add"),
            "{rendered}"
        );
        assert!(
            rendered.contains("resolution: the result comment names the skills"),
            "{rendered}"
        );
        assert!(!rendered.contains("practice: none selected"), "{rendered}");

        // Substantial: the bounded picks are what the words chose beside it.
        let mut substantial = ordinary_selection(&inventory, Depth::Substantial);
        substantial.ticket = selection.ticket.clone();
        assert_eq!(substantial.mandated.len(), 5);
        assert_eq!(
            substantial.methodology.unwrap().id,
            "skill/central/docs-methodology"
        );
    }

    #[test]
    fn a_ticket_the_forge_cannot_read_leaves_word_matching_and_says_why() {
        let mut gaps = Vec::new();
        let none = ticket_named(
            &forge(),
            None,
            Path::new("/c/o-i"),
            "work ticket #723",
            &catalogue(),
            &mut BTreeSet::new(),
            &mut gaps,
        );
        assert!(none.is_none());
        assert!(gaps[0].contains("no `gh` is on PATH"), "{gaps:?}");

        let refused = aikit_adapters::runner::ScriptedRunner::new()
            .on("remote get-url origin", "git@github.com:EpiLogos/O-I.git\n")
            .failing(
                "repos/EpiLogos/O-I/issues/723",
                1,
                "HTTP 401: Bad credentials",
            );
        let mut gaps = Vec::new();
        assert!(ticket_named(
            &refused,
            Some(Path::new("gh")),
            Path::new("/c/o-i"),
            "work ticket #723",
            &catalogue(),
            &mut BTreeSet::new(),
            &mut gaps,
        )
        .is_none());
        assert!(
            gaps[0].contains("EpiLogos/O-I#723 could not be read as a ticket")
                && gaps[0].contains("Bad credentials"),
            "{gaps:?}"
        );

        // No map parent, and a type with no practice: the ticket still stands,
        // with each gap in words.
        let orphan = aikit_adapters::runner::ScriptedRunner::new()
            .on("remote get-url origin", "git@github.com:EpiLogos/O-I.git\n")
            .on(
                "repos/EpiLogos/O-I/issues/9",
                &json!({"title": "t", "labels": [{"name": "wayfinder:spike"}]}).to_string(),
            )
            .failing(
                "repos/EpiLogos/O-I/issues/9/parent",
                1,
                "Not Found (HTTP 404)",
            );
        let ticket = ticket_named(
            &orphan,
            Some(Path::new("gh")),
            Path::new("/c/o-i"),
            "do #9",
            &catalogue(),
            &mut BTreeSet::new(),
            &mut Vec::new(),
        )
        .unwrap();
        assert!(ticket.method.is_none() && ticket.notes.is_empty());
        assert_eq!(ticket.gaps.len(), 2, "{:?}", ticket.gaps);
        assert!(ticket.gaps[0].contains("type `spike` names no active practice"));
        assert!(ticket.gaps[1].contains("its map could not be read"));
    }

    #[test]
    fn the_practice_a_session_loads_is_read_from_its_tool_events() {
        let event = |tool: &str, input: Value| {
            HookEvent::new(
                "claude",
                HookEventKind::PostToolUse,
                json!({"tool_input": input}),
            )
            .with_tool_name(tool)
        };
        assert_eq!(
            practice_loaded(&event(
                "Skill",
                json!({"skill": "anthropic-skills:wayfinder"})
            )),
            ["wayfinder"]
        );
        assert_eq!(
            practice_loaded(&event(
                "Read",
                json!({"file_path": "/Users/admin/Central/Control/user/skills/engineering-methodology/SKILL.md"})
            )),
            ["engineering-methodology"]
        );
        assert_eq!(
            practice_loaded(&event(
                "Bash",
                json!({"command": "sed -n 1,80p \"/r/registry/capsules/skill/central/central-authorised-task/payload/SKILL.md\" && cat ~/.claude/skills/grilling/SKILL.md"})
            )),
            ["central-authorised-task", "grilling"]
        );
        assert_eq!(
            practice_loaded(&event(
                "shell",
                json!({"command": ["cat", "/x/agent-worktree-lifecycle/SKILL.md"]})
            )),
            ["agent-worktree-lifecycle"]
        );
        assert!(
            practice_loaded(&event("Read", json!({"file_path": "/x/NOT_SKILL.md"}))).is_empty()
        );
        assert!(practice_loaded(&event("Bash", json!({"command": "cargo test"}))).is_empty());
    }

    fn ledger_with(ticket: ClaimedTicket, loaded: &[&str]) -> EntryLedger {
        EntryLedger {
            schema: ENTRY_SCHEMA.into(),
            client: "claude".into(),
            session: "s-723".into(),
            concern: "work ticket #723".into(),
            project: "O-I".into(),
            checkout: "/c/worktrees/env-1/o-i".into(),
            rendered_digest: String::new(),
            prepared_version: None,
            source_revisions: BTreeMap::new(),
            source_refs: BTreeMap::new(),
            emitted_at_unix_ms: 0,
            redeliver_pending: false,
            selected: vec![],
            refinement: None,
            ticket: Some(ticket),
            loaded: loaded.iter().map(|n| (*n).to_owned()).collect(),
            invoked: BTreeSet::new(),
            tickets_read: BTreeSet::new(),
        }
    }

    const EVERY: [&str; 5] = [
        "wayfinder",
        "agent-worktree-lifecycle",
        "engineering-methodology",
        "collaboration-methodology",
        "central-authorised-task",
    ];

    #[test]
    fn closing_the_ticket_is_refused_until_its_method_is_loaded_and_the_claim_holds() {
        let unread = |_: &ClaimedTicket| -> std::result::Result<Option<String>, String> {
            panic!("the close's own comment is the resolution")
        };
        let close = "gh issue close 723 --comment \"Landed in ai-kit.\nSkills: wayfinder, agent-worktree-lifecycle, engineering-methodology, collaboration-methodology and central-authorised-task\"";

        // Nothing loaded: the Method, the Notes and the claim all fail.
        let refused = resolution_refusal(close, &ledger_with(claimed_723(), &[]), unread).unwrap();
        assert!(refused.starts_with("closing EpiLogos/O-I#723 is refused: its type `task` selects `central-authorised-task`, and this session has not loaded it"), "{refused}");
        assert!(
            refused.contains("the map's Notes have every session load `wayfinder`"),
            "{refused}"
        );
        assert!(refused.contains("never loaded them"), "{refused}");
        assert!(
            refused.ends_with("This session has loaded: nothing recorded."),
            "{refused}"
        );

        // Everything loaded and claimed truly: the close goes ahead.
        let ready = ledger_with(claimed_723(), &EVERY);
        assert_eq!(resolution_refusal(close, &ready, unread), None);

        // A claim of practice never loaded is refused even with all loaded.
        let overclaim = "gh issue close 723 -c '**Skills loaded:** `wayfinder`, `prototype`'";
        let refused = resolution_refusal(overclaim, &ready, unread).unwrap();
        assert!(
            refused.contains(
                "the result names `prototype` as loaded, and this session never loaded it"
            ),
            "{refused}"
        );

        // Without a --comment the ticket's latest comment is the resolution.
        let bare = "cd /c/o-i && gh issue close 723";
        let mut asked = 0;
        assert_eq!(
            resolution_refusal(bare, &ready, |ticket| {
                asked += 1;
                assert_eq!(ticket.number, 723);
                Ok(Some(
                    "Result: landed.\n- Skills: `wayfinder`, `central-authorised-task`".into(),
                ))
            }),
            None
        );
        assert_eq!(asked, 1);
        let refused =
            resolution_refusal(bare, &ready, |_| Ok(Some("Result: landed.".into()))).unwrap();
        assert!(refused.contains("the result names no skills"), "{refused}");
        let refused = resolution_refusal(bare, &ready, |_| Err("HTTP 502".into())).unwrap();
        assert!(
            refused.contains("could not be read (HTTP 502)"),
            "{refused}"
        );

        // The REST close is the same close.
        assert!(resolution_refusal(
            "gh api -X PATCH repos/EpiLogos/O-I/issues/723 -f state=closed",
            &ledger_with(claimed_723(), &[]),
            |_| Ok(None),
        )
        .is_some());

        // Other issues, other repositories and other commands are not checked.
        let fresh = ledger_with(claimed_723(), &[]);
        for other in [
            "gh issue close 724 --comment 'Skills: x'",
            "gh issue close 723 -R EpiLogos/ai-kit",
            "gh issue view 723",
            "git commit -m 'close the gap'",
        ] {
            assert_eq!(resolution_refusal(other, &fresh, unread), None, "{other}");
        }
    }

    #[test]
    fn a_close_leaves_loaded_invoked_and_relied_on_with_the_disagreements() {
        let mut ledger = ledger_with(claimed_723(), &EVERY);
        ledger.loaded.insert("grilling".into());
        ledger.loaded.insert("no-such-skill".into());
        ledger.invoked.insert("central-authorised-task".into());
        let relied = vec!["wayfinder".to_owned(), "central-authorised-task".to_owned()];
        let check = resolution_check(
            "gh issue close 723 -c 'Skills: wayfinder, central-authorised-task'",
            &ledger,
            |_| Ok(None),
        )
        .unwrap();
        assert_eq!(check.refusal, None);
        assert_eq!(check.relied_upon, relied);
        let (activity, findings) = session_activity(&ledger, &check.relied_upon, &catalogue());
        assert_eq!(activity.loaded.len(), 6, "{:?}", activity.loaded);
        assert_eq!(activity.invoked, ["skill/central/central-authorised-task"]);
        assert_eq!(
            activity.relied_upon,
            [
                "skill/central/wayfinder",
                "skill/central/central-authorised-task"
            ]
        );
        assert_eq!(
            activity.evidence_refs,
            ["https://github.com/EpiLogos/O-I/issues/723"]
        );
        assert!(findings.contains(
            &"`no-such-skill` was used, and no active entry here carries that name".to_owned()
        ));
        assert!(findings.contains(
            &"`grilling` was loaded, and the resolution does not name it as relied on".to_owned()
        ));
        assert!(
            !findings.iter().any(|f| f.starts_with("`wayfinder`")),
            "{findings:?}"
        );
    }

    #[test]
    fn the_refinement_the_hook_spawns_is_a_command_the_cli_parses() {
        use clap::Parser;
        let mut argv = vec![std::ffi::OsString::from("aikit")];
        argv.extend(refinement_args(
            "claude",
            "s-1",
            Path::new("/c/worktrees/env-2/o-i"),
        ));
        let parsed = crate::cli::Cli::try_parse_from(argv);
        assert!(parsed.is_ok(), "{:?}", parsed.err().map(|e| e.to_string()));
    }

    fn empty_view() -> ResolvedView {
        ResolvedView {
            context: aikit_core::context::ContextDescriptor::for_project("/entry-723-test"),
            policy: Default::default(),
            active: BTreeMap::new(),
            declared: BTreeMap::new(),
            unavailable: BTreeMap::new(),
            selection_log: Vec::new(),
            catalog_index: BTreeMap::new(),
            skill_usage_overlays: BTreeMap::new(),
            warnings: Vec::new(),
            hash: serde_json::from_value(json!("0".repeat(64))).unwrap(),
            catalog_revision: "entry-723-r1".into(),
            properties: BTreeMap::new(),
        }
    }

    #[test]
    fn a_session_records_what_it_loads_and_the_close_is_checked_against_it() {
        let temp = tempfile::tempdir().unwrap();
        store_ledger(temp.path(), &ledger_with(claimed_723(), &[]));
        let view = empty_view();
        let roots = BTreeMap::new();
        let config = EntryConfig::default();
        let tool = |kind: HookEventKind, tool: &str, input: Value| {
            HookEvent::new(
                "claude",
                kind,
                json!({"session_id": "s-723", "tool_input": input}),
            )
            .with_tool_name(tool)
        };
        fn on<'a>(
            event: &'a HookEvent,
            config: &'a EntryConfig,
            state: &'a Path,
            view: &'a ResolvedView,
            roots: &'a BTreeMap<CapsuleId, PathBuf>,
        ) -> EntryRequest<'a> {
            EntryRequest {
                event,
                client: "claude",
                config,
                state,
                central: None,
                view,
                capsule_roots: roots,
            }
        }
        macro_rules! run {
            ($event:expr) => {
                on($event, &config, temp.path(), &view, &roots)
            };
        }
        let close = tool(
            HookEventKind::PreToolUse,
            "Bash",
            json!({"command": "gh issue close 723 --comment 'Skills: wayfinder, central-authorised-task'"}),
        );
        let refused = check_resolution(&run!(&close)).unwrap().unwrap();
        assert!(
            refused.contains("selects `central-authorised-task`"),
            "{refused}"
        );

        for (name, input) in [
            ("Skill", json!({"skill": "central-authorised-task"})),
            (
                "Read",
                json!({"file_path": "/c/Control/user/skills/wayfinder/SKILL.md"}),
            ),
            (
                "Bash",
                json!({"command": "cat /s/agent-worktree-lifecycle/SKILL.md /s/engineering-methodology/SKILL.md /s/collaboration-methodology/SKILL.md"}),
            ),
        ] {
            let used = tool(HookEventKind::PostToolUse, name, input);
            assert_eq!(
                deliver(&run!(&used)).unwrap(),
                None,
                "recording injects nothing"
            );
        }
        let ledger = load_ledger(temp.path(), "claude", "s-723").unwrap();
        assert_eq!(
            ledger.loaded,
            EVERY
                .iter()
                .map(|n| (*n).to_owned())
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(
            ledger.invoked,
            BTreeSet::from(["central-authorised-task".to_owned()]),
            "only the skill call is an invocation"
        );
        assert!(!activity_path(temp.path(), "claude", "s-723").exists());
        assert_eq!(check_resolution(&run!(&close)).unwrap(), None);
        // The close that went through left the session's evidence, in the
        // shape `aikit praxis disclose --activity` reads.
        let activity: aikit_core::agent_praxis::PraxisActivity = serde_json::from_slice(
            &std::fs::read(activity_path(temp.path(), "claude", "s-723")).unwrap(),
        )
        .unwrap();
        assert_eq!(activity.schema, "aikit.praxis-activity/v1");
        assert_eq!(activity.return_destinations, ["EpiLogos/O-I#723"]);
        // A tool call that closes nothing is answered from its text alone.
        let other = tool(
            HookEventKind::PreToolUse,
            "Bash",
            json!({"command": "cargo test"}),
        );
        assert_eq!(check_resolution(&run!(&other)).unwrap(), None);
    }

    fn native_work_scratch() -> tempfile::TempDir {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        std::fs::create_dir_all(&parent).unwrap();
        tempfile::Builder::new()
            .prefix("entry-native-work-")
            .tempdir_in(parent)
            .unwrap()
    }

    #[test]
    #[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
    fn actual_native_entry_retains_owner_ref_local_byte_basis_and_declaration_failure() {
        let ctrl = PathBuf::from(
            std::env::var_os("AIKIT_CENTRAL_REAL_BIN").expect("real pinned ctrl is required"),
        );
        assert!(ctrl.is_file());
        let owned = native_work_scratch();
        let world = std::fs::canonicalize(owned.path()).unwrap();
        let run = |action: &str, input: Value| {
            let mut command = std::process::Command::new(&ctrl);
            command
                .args(["--json", "--root"])
                .arg(&world)
                .args(["action", "run", action])
                .arg(input.to_string());
            let output = SystemRunner::new()
                .with_timeout(Duration::from_secs(30))
                .with_strict_utf8()
                .capture_command(&mut command)
                .unwrap();
            let receipt: Value = serde_json::from_str(&output.stdout).unwrap();
            assert!(
                output.ok() && receipt["ok"] == true,
                "{receipt}; {}",
                output.stderr
            );
            receipt["data"].clone()
        };
        run("central.init", json!({}));
        let primary = world.join("Work/display-name");
        std::fs::create_dir_all(&primary).unwrap();
        run(
            "projectcentral.init",
            json!({"project":"display-name","project_id":"native:a:b"}),
        );
        let bytes = b"# Exact entry source\nNativeentryneedle owner basis.\n";
        std::fs::write(primary.join("README.md"), bytes).unwrap();
        let scope = resolve_scope_current(&world, &primary).unwrap().unwrap();
        assert_eq!(scope.project, "display-name");
        assert_eq!(scope.project_id, "native:a:b");
        let (provider, hits) = search_sources(
            native_scope_project(&world, &scope).unwrap(),
            "Nativeentryneedle",
            8,
        )
        .unwrap();
        let reference = work_file_source_ref("native:a:b", Path::new("README.md")).unwrap();
        let hit = hits
            .iter()
            .find(|hit| hit.source == reference)
            .expect("real native rg match");
        assert_eq!(
            relative_of(&scope.project_id, &hit.source)
                .unwrap()
                .as_deref(),
            Some("README.md")
        );
        let revision = candidate_revision(&provider, &hit.source, hit.revision.as_ref()).unwrap();
        assert_eq!(revision, format!("blake3:{}", blake3::hash(bytes).to_hex()));
        let candidate = SourceCandidate {
            project: scope.project.clone(),
            source: hit.source.clone(),
            path: "README.md".into(),
            absolute: primary.join("README.md"),
            revision: revision.clone(),
            score: hit.score.unwrap_or(0.0),
            snippet: hit.snippet.clone(),
            line: None,
            authored: true,
            mandatory: false,
        };
        let item = work_candidate_item(&candidate).unwrap();
        assert_eq!(
            item.external_egress,
            ExternalEgress::Denied,
            "selected Work cannot gain egress during preparation"
        );
        assert_eq!(item.source_ref.as_str(), reference.as_str());
        assert_ne!(
            item.source_ref.as_str(),
            "source:project:display-name:README.md"
        );
        let path = primary.join("README.md").display().to_string();
        let ledger = EntryLedger {
            schema: ENTRY_SCHEMA.into(),
            client: "native-test".into(),
            session: "source-return".into(),
            concern: "Nativeentryneedle".into(),
            project: scope.project.clone(),
            checkout: scope.checkout.clone(),
            rendered_digest: String::new(),
            prepared_version: None,
            source_revisions: BTreeMap::from([(path.clone(), revision.clone())]),
            source_refs: BTreeMap::from([(path.clone(), reference.clone())]),
            emitted_at_unix_ms: 0,
            redeliver_pending: false,
            selected: vec![reference.as_str().into()],
            refinement: None,
            ticket: None,
            loaded: BTreeSet::new(),
            invoked: BTreeSet::new(),
            tickets_read: BTreeSet::new(),
        };
        let state = world.join("owned-entry-state");
        store_ledger(&state, &ledger);
        let loaded = load_ledger(&state, "native-test", "source-return").unwrap();
        assert_eq!(
            loaded, ledger,
            "real persisted ledger retains issued identity"
        );
        assert!(current_ledger_changes(&world, &scope, &loaded)
            .unwrap()
            .is_empty());
        let mut legacy = serde_json::to_value(&loaded).unwrap();
        legacy.as_object_mut().unwrap().remove("source_refs");
        let old: EntryLedger = serde_json::from_value(legacy).unwrap();
        assert!(old.source_refs.is_empty());
        assert_eq!(
            old.source_revisions, ledger.source_revisions,
            "legacy observations survive without guessed native refs"
        );
        std::fs::write(primary.join("README.md"), b"changed current source\n").unwrap();
        assert_eq!(
            candidate_revision(&provider, &hit.source, hit.revision.as_ref())
                .unwrap_err()
                .code(),
            "development_entry.source_revision_conflict"
        );
        let changes = current_ledger_changes(&world, &scope, &loaded).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(loaded.source_refs[&changes[0].0], reference);
        assert_eq!(
            changes[0].2,
            format!(
                "blake3:{}",
                blake3::hash(b"changed current source\n").to_hex()
            )
        );
        std::fs::write(primary.join(".no-agent-retrieval"), "withdraw\n").unwrap();
        assert_eq!(
            current_ledger_changes(&world, &scope, &loaded)
                .unwrap_err()
                .code(),
            "work_repos.source_unauthorised"
        );
        assert_eq!(
            load_ledger(&state, "native-test", "source-return").unwrap(),
            loaded,
            "refusal does not rewrite retained history"
        );
        std::fs::remove_file(primary.join(".no-agent-retrieval")).unwrap();
        std::fs::write(primary.join("ProjectCentral/project.json"), "not json").unwrap();
        assert_eq!(
            resolve_scope_current(&world, &primary).unwrap_err().code(),
            "projectcentral.manifest_invalid"
        );
        assert!(
            resolve_scope(&world, &primary).is_none(),
            "legacy projection cannot fabricate folder-name identity"
        );
        assert_eq!(
            std::fs::read(primary.join("README.md")).unwrap(),
            b"changed current source\n"
        );
    }
}
