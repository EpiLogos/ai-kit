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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aikit_adapters::central_temporal::project_ground_root;
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_adapters::work_repos::{query_terms, WorkRepoProject, WorkReposSourcePoolProvider};
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::id::CapsuleId;
use aikit_core::knowledge_source_pool::{SourcePoolProvider, SourceSearchMode};
use aikit_core::method::{praxis_form, praxis_payload, PraxisForm};
use aikit_core::resolve::ResolvedView;
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

pub fn resolve_scope(central: &Path, cwd: &Path) -> Option<EntryScope> {
    let primary = project_ground_root(central, cwd)?;
    let checkout = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| primary.clone());
    let project = primary.file_name()?.to_string_lossy().into_owned();
    let project_id = std::fs::read_to_string(primary.join("ProjectCentral/project.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["project_id"].as_str().map(str::to_owned))
        .unwrap_or_else(|| project.clone());
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
    Some(EntryScope {
        project,
        project_id,
        primary,
        checkout,
        branch,
        head,
    })
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
    ];
    let designish = design.iter().any(|word| lowered.contains(word));
    let repairish = repair.iter().any(|word| lowered.contains(word));
    let short = concern.chars().count() <= 400;
    if repairish && short && !designish {
        Depth::Shallow
    } else if short && !designish && query_terms(concern).len() <= 6 {
        Depth::Shallow
    } else {
        Depth::Substantial
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceCandidate {
    pub project: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Inventory {
    pub sources: Vec<SourceCandidate>,
    pub capabilities: Vec<CapabilityCandidate>,
    pub praxis: Vec<PraxisCandidate>,
    pub matrix: Option<Value>,
    pub absences: Vec<String>,
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
    project: &WorkRepoProject,
    concern: &str,
    limit: usize,
) -> std::result::Result<Vec<aikit_core::knowledge_source_pool::SourceHit>, String> {
    let provider = WorkReposSourcePoolProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(10)),
        aikit_adapters::ripgrep::executable(),
        vec![project.clone()],
    );
    provider
        .search(concern, SourceSearchMode::Fulltext, &[], limit)
        .map_err(|error| format!("{}: {}", error.code(), error.message()))
}

fn relative_of(project_id: &str, source: &str) -> Option<String> {
    source
        .strip_prefix(&format!("source:project:{project_id}:"))
        .map(str::to_owned)
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

    // Sources: this Project's checkout first, authored material ahead of code.
    let mut sources = Vec::new();
    let current = WorkRepoProject {
        name: scope.project.clone(),
        project_id: scope.project_id.clone(),
        root: scope.checkout.clone(),
    };
    match search_sources(&current, concern, 40) {
        Ok(hits) => {
            let mut docs = 0;
            let mut code = 0;
            for hit in hits {
                let Some(path) = relative_of(&scope.project_id, hit.source.as_str()) else {
                    continue;
                };
                let authored = is_authored(&path);
                if authored && docs >= DOC_LIMIT || !authored && code >= CODE_LIMIT {
                    continue;
                }
                let absolute = scope.checkout.join(&path);
                let Some(revision) = content_revision(&absolute) else {
                    continue;
                };
                if authored {
                    docs += 1;
                } else {
                    code += 1;
                }
                sources.push(SourceCandidate {
                    project: scope.project.clone(),
                    path,
                    absolute,
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
        }
        Err(reason) => absences.push(format!("source search unavailable: {reason}")),
    }
    for path in &mentioned_paths {
        if let Some(existing) = sources.iter_mut().find(|source| &source.path == path) {
            existing.mandatory = true;
            continue;
        }
        let absolute = scope.checkout.join(path);
        if let Some(revision) = content_revision(&absolute) {
            sources.insert(
                0,
                SourceCandidate {
                    project: scope.project.clone(),
                    path: path.clone(),
                    absolute,
                    revision,
                    score: 1.0,
                    snippet: String::new(),
                    line: None,
                    authored: is_authored(path),
                    mandatory: true,
                },
            );
        }
    }
    // Legitimate cross-project relations: only Projects the concern names,
    // authored material only, read from their primary checkouts.
    for name in named_projects(concern, central, &scope.project) {
        let root = central.join("Work").join(&name);
        let project_id = std::fs::read_to_string(root.join("ProjectCentral/project.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value["project_id"].as_str().map(str::to_owned))
            .unwrap_or_else(|| name.clone());
        let related = WorkRepoProject {
            name: name.clone(),
            project_id: project_id.clone(),
            root: root.clone(),
        };
        match search_sources(&related, concern, 20) {
            Ok(hits) => {
                for hit in hits
                    .into_iter()
                    .filter_map(|hit| {
                        relative_of(&project_id, hit.source.as_str()).map(|path| (hit, path))
                    })
                    .filter(|(_, path)| is_authored(path))
                    .take(CROSS_PROJECT_LIMIT)
                {
                    let (hit, path) = hit;
                    let absolute = root.join(&path);
                    let Some(revision) = content_revision(&absolute) else {
                        continue;
                    };
                    sources.push(SourceCandidate {
                        project: name.clone(),
                        path,
                        absolute,
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
            Err(reason) => absences.push(format!("{name}: source search unavailable: {reason}")),
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
    match crate::jev_now::project_matrix_rows(&scope.primary.join("ProjectCentral")) {
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

    // Praxis: the active catalogue, by form.
    let mut praxis = Vec::new();
    for entry in view.catalog_index.values() {
        if entry.kind != Kind::Skill || !view.is_active(&entry.id) {
            continue;
        }
        let form = praxis_form(&entry.description);
        let payload = praxis_payload(&entry.description);
        let haystack = format!("{} {}", entry.name, payload).to_lowercase();
        let matched = terms
            .iter()
            .filter(|term| haystack.contains(term.as_str()))
            .count();
        if matched == 0 {
            continue;
        }
        let skill_file = capsule_roots
            .get(&entry.id)
            .map(|root| root.join("payload/SKILL.md"))
            .filter(|path| path.is_file());
        praxis.push(PraxisCandidate {
            id: entry.id.to_string(),
            name: entry.name.clone(),
            form: form.as_str().to_owned(),
            payload: payload.chars().take(220).collect(),
            skill_file,
            score: matched as f64 / terms.len().max(1) as f64,
            mandatory: concern.contains(entry.name.as_str()),
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
    let pick = |form: PraxisForm| {
        inventory
            .praxis
            .iter()
            .filter(|p| p.form == form.as_str())
            .find(|p| p.mandatory || p.score >= 0.2)
            .cloned()
    };
    let (methodology, method, skills) = match depth {
        Depth::Shallow => (
            None,
            None,
            inventory
                .praxis
                .iter()
                .filter(|p| p.mandatory)
                .take(SKILL_LIMIT)
                .cloned()
                .collect(),
        ),
        Depth::Substantial => (
            pick(PraxisForm::Methodology),
            pick(PraxisForm::Method),
            inventory
                .praxis
                .iter()
                .filter(|p| p.form == PraxisForm::Skill.as_str())
                .filter(|p| p.mandatory || p.score >= 0.25)
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
        add(
            format!("source/{index}"),
            candidate_item(
                &format!("source:project:{}:{}", source.project, source.path),
                &source.revision,
                &source.path,
                source.snippet.clone(),
            ),
        );
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
        lines.push("practice: none selected".into());
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
    for degradation in &selection.degradations {
        lines.push(format!("degraded: {degradation}"));
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
    pub emitted_at_unix_ms: u64,
    /// Set at pre-compaction: the next prompt re-delivers the entry.
    #[serde(default)]
    pub redeliver_pending: bool,
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

fn publish_view(
    store: &RedisNowStore,
    client: &str,
    session: &str,
    scope: &EntryScope,
    concern: &str,
    selection: &Selection,
    rendered: &str,
) -> Result<(u64, String)> {
    let (participant, agent_session) = participant_refs(client, session)?;
    let current = store.current_version(&participant, None)?;
    let mut source_revisions = BTreeMap::new();
    let mut items = Vec::new();
    for source in &selection.sources {
        let reference = format!("source:project:{}:{}", source.project, source.path);
        source_revisions.insert(
            source.absolute.display().to_string(),
            source.revision.clone(),
        );
        items.push(candidate_item(
            &reference,
            &source.revision,
            &source.path,
            source.snippet.clone(),
        )?);
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
        .methodology
        .iter()
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
    let Some(central) = request.central else {
        return Ok(None);
    };
    let cwd = event
        .cwd
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let Some(scope) = resolve_scope(central, &cwd) else {
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

    // A later prompt: announce what changed under the prepared basis, once.
    if let Some(mut ledger) = ledger {
        if ledger.checkout != scope.checkout {
            // The session moved to another checkout: its entry no longer
            // describes where it stands. Prepare afresh for the new scope.
        } else {
            let mut changed = Vec::new();
            for (path, revision) in &ledger.source_revisions {
                let now = content_revision(Path::new(path)).unwrap_or_else(|| "absent".into());
                if &now != revision {
                    changed.push((path.clone(), revision.clone(), now));
                }
            }
            if changed.is_empty() {
                return Ok(None);
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
                        let change = NowContextChange {
                            change_id: format!(
                                "development-entry:{}",
                                &blake3::hash(format!("{path}\0{after}").as_bytes()).to_hex()[..24]
                            ),
                            kind: "source-revision".into(),
                            source_ref: ResourceRef::parse(format!("source:path:{path}"))?,
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
            return Ok(Some(lines.join("\n")));
        }
    }

    let prompt = event
        .payload
        .get("prompt")
        .or_else(|| event.payload.get("user_prompt"))
        .or_else(|| event.payload.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
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
            if let Ok(Some(view)) = store.read_prepared(&participant, false, None) {
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
    let inventory = collect_inventory(central, scope, concern, request.view, request.capsule_roots);
    let inventory_ms = started.elapsed().as_millis() as u64;
    let mut selection = match request.config.selection.as_deref() {
        Some("provider") => match (
            &request.config.provider_file,
            request.config.relevance_threshold,
        ) {
            (Some(file), Some(threshold)) => {
                provider_selection(&inventory, depth, concern, scope, file, threshold)
            }
            _ => {
                let mut ordinary = ordinary_selection(&inventory, depth);
                ordinary.degradations.push("provider selection configured without provider_file and an explicit relevance_threshold; ordinary selection used".into());
                ordinary
            }
        },
        _ => ordinary_selection(&inventory, depth),
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
        ) {
            Ok((version, digest)) => prepared = Some((version, digest)),
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
        source_revisions: selection
            .sources
            .iter()
            .map(|s| (s.absolute.display().to_string(), s.revision.clone()))
            .collect(),
        emitted_at_unix_ms: now_ms(),
        redeliver_pending: false,
    };
    store_ledger(request.state, &ledger);
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
            emitted_at_unix_ms: 0,
            redeliver_pending: false,
        };
        store_ledger(temp.path(), &ledger);
        assert_eq!(load_ledger(temp.path(), "claude", "s1"), Some(ledger));
        assert_eq!(load_ledger(temp.path(), "codex", "s1"), None);
    }
}
