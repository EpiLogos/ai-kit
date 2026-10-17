//! Capture: from something observed to something reviewable.
//!
//! The pipeline is fixed and its order is the point:
//!
//! ```text
//! normalize → scan → redact-or-quarantine → classify → exact dedup →
//! similarity → file in the inbox
//! ```
//!
//! ## Scan before store, not before show
//!
//! The scanner runs *before the body is written to disk*, and what is written is
//! the redacted text. Redacting at display time would mean the secret is in the
//! inbox directory, in whatever backs it up, and in any editor that opens it —
//! and would rely on every future reader remembering to redact. Here there is
//! nothing to remember: the raw text never reaches a file.
//!
//! A candidate with a finding is additionally **quarantined**: it lives in
//! `inbox/quarantine/`, and [`Inbox::promote`] refuses it outright with
//! `inbox.quarantined`. Release-blocking case 10 — "a captured secret never
//! enters the ordinary registry" — is therefore enforced twice, by where the file
//! is and by what promotion will do.
//!
//! ## Dedup before similarity
//!
//! Exact and normalized hashes are cheap and certain; shingle overlap is neither.
//! Running the certain check first means the common case — the same snippet
//! captured twice — never reaches the fuzzy one, and never asks a human a
//! question that has an obvious answer.
//!
//! ## Promotion generates the manifest
//!
//! Release-blocking case 11: a user must never have to hand-write a manifest.
//! [`Inbox::promote`] renders one from the candidate plus the user's edits, parses
//! it back through `Capsule::from_toml_str` **before writing anything**, and only
//! then creates the capsule directory. A rejected edit leaves no debris.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use aikit_core::catalog::Catalog;
use aikit_core::error::err;
use aikit_core::id::{InboxId, ProcedureId};
use aikit_core::procedure::{
    Inverse, MutationIsolation, Plan, Procedure, ProcedureKind, WorldEdit,
};
use aikit_core::{AikitError, Capsule, CapsuleId, Kind, Maturity, Result, SessionId};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::channel::{Evidence, InboxChannel, InboxKind, NewItem};
use crate::events::Timestamp;
use crate::home::{create_dir_all, io_error};
use crate::index::{sql_error, Index};
use crate::procedure::ProcedureRunner;
use crate::scan::{Finding, Scanner};
use crate::AikitHome;

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Where a saved operation is meant to apply.
///
/// The word is the capturer's intent, kept on the candidate until someone
/// decides. It also decides where promotion writes: `project` goes into the
/// project's own `.aikit` registry, the other two into the personal registry.
/// `session` is for an example that belongs to one working session; it lands in
/// the personal registry like `home` and is enabled in that session afterwards
/// (`aikit enable --scope session`) — promotion itself never enables anything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureScope {
    /// Every session on this machine.
    #[default]
    Home,
    /// Every session in the capturing project.
    Project,
    /// The capturing session only.
    Session,
}

impl CaptureScope {
    pub fn as_str(self) -> &'static str {
        match self {
            CaptureScope::Home => "home",
            CaptureScope::Project => "project",
            CaptureScope::Session => "session",
        }
    }
}

impl fmt::Display for CaptureScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CaptureScope {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "home" => Ok(CaptureScope::Home),
            "project" => Ok(CaptureScope::Project),
            "session" => Ok(CaptureScope::Session),
            other => err(
                "inbox.unknown_scope",
                format!("`{other}` is not a capture scope; use home, project or session"),
            ),
        }
    }
}

/// How sure the classification of a candidate is.
///
/// `Declared` means the capturer said what it is; the other three are the
/// classifier's own reading and say how much of the reading to trust. A UI shows
/// `Low` as a question, not an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    Low,
    Medium,
    High,
    Declared,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
            Confidence::Declared => "declared",
        }
    }
}

/// Something observed that might be worth keeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub title: String,
    /// The content itself: a script, a skill document, a guidance fragment.
    pub body: String,
    /// What the caller believes it is. Beats the classifier when present.
    pub suggested_kind: Option<Kind>,
    /// Command names it would export, when the caller knows them.
    pub exports: Vec<String>,
    pub project_root: Option<PathBuf>,
    pub session: Option<SessionId>,
    /// Where the saved operation is meant to apply.
    pub scope: CaptureScope,
    /// Where the example came from — a terminal pane, an agent run, a Factory
    /// frame. A reference, never the transcript itself.
    pub source_ref: Option<String>,
    /// Who observed it: `human`, or an agent ref. Unknown stays unknown.
    pub by: Option<String>,
}

impl Capture {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            suggested_kind: None,
            exports: Vec::new(),
            project_root: None,
            session: None,
            scope: CaptureScope::default(),
            source_ref: None,
            by: None,
        }
    }

    #[must_use]
    pub fn of_kind(mut self, kind: Kind) -> Self {
        self.suggested_kind = Some(kind);
        self
    }

    #[must_use]
    pub fn exporting<I: IntoIterator<Item = S>, S: Into<String>>(mut self, exports: I) -> Self {
        self.exports = exports.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn in_scope(mut self, scope: CaptureScope) -> Self {
        self.scope = scope;
        self
    }

    #[must_use]
    pub fn from_source(mut self, source_ref: impl Into<String>) -> Self {
        self.source_ref = Some(source_ref.into());
        self
    }

    #[must_use]
    pub fn by(mut self, by: impl Into<String>) -> Self {
        self.by = Some(by.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Candidates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateState {
    /// Awaiting a human decision.
    Ready,
    /// Held back: the scanner found something.
    Quarantined,
    /// Turned down.
    Rejected,
    /// Written into a registry.
    Promoted,
}

impl CandidateState {
    pub fn as_str(self) -> &'static str {
        match self {
            CandidateState::Ready => "ready",
            CandidateState::Quarantined => "quarantined",
            CandidateState::Rejected => "rejected",
            CandidateState::Promoted => "promoted",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "quarantined" => CandidateState::Quarantined,
            "rejected" => CandidateState::Rejected,
            "promoted" => CandidateState::Promoted,
            _ => CandidateState::Ready,
        }
    }

    /// Which inbox directory a candidate in this state belongs in.
    fn directory(self, home: &AikitHome) -> PathBuf {
        match self {
            CandidateState::Quarantined => home.inbox_quarantine(),
            CandidateState::Rejected => home.inbox_rejected(),
            _ => home.inbox_ready(),
        }
    }
}

/// A filed capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub kind: Kind,
    /// How sure `kind` is; `Declared` when the capturer named it.
    pub confidence: Confidence,
    /// One plain sentence on why the classifier read it that way.
    pub kind_reason: String,
    pub title: String,
    pub state: CandidateState,
    /// What the scanner found. Never contains the secrets themselves.
    pub findings: Vec<Finding>,
    /// blake3 of the stored (redacted) body.
    pub body_hash: String,
    /// blake3 of the normalized form, so cosmetic edits are recognized.
    pub normalized_hash: String,
    pub exports: Vec<String>,
    pub scope: CaptureScope,
    pub source_ref: Option<String>,
    pub by: Option<String>,
    pub session: Option<String>,
    pub project_root: Option<PathBuf>,
    pub path: PathBuf,
    pub created_at: Timestamp,
}

/// Everything about a candidate that the database row does not carry.
///
/// It lives beside the body as `meta.json` rather than in new columns: a column
/// is a schema step, and a schema step makes every older `aikit` on the machine
/// refuse the shared database. A candidate without the file (filed before this
/// existed) reads as an ordinary inferred capture at home scope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CandidateMeta {
    #[serde(default)]
    exports: Vec<String>,
    #[serde(default)]
    scope: Option<CaptureScope>,
    #[serde(default)]
    source_ref: Option<String>,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    confidence: Option<Confidence>,
    #[serde(default)]
    kind_reason: Option<String>,
}

const META_FILE: &str = "meta.json";

impl CandidateMeta {
    fn load(dir: &Path) -> Self {
        std::fs::read(dir.join(META_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn store(&self, dir: &Path) -> Result<()> {
        let path = dir.join(META_FILE);
        let text = serde_json::to_string_pretty(self).map_err(|e| {
            AikitError::new(
                "inbox.write_failed",
                format!("could not encode the candidate's details: {e}"),
            )
        })?;
        std::fs::write(&path, text.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &path, &e))
    }
}

impl Candidate {
    pub fn body_path(&self) -> PathBuf {
        self.path.join("body")
    }
}

/// What [`Inbox::capture`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOutcome {
    pub candidate: Candidate,
    /// Set when this body was already in the inbox; no second candidate is filed.
    pub duplicate_of: Option<String>,
    /// Ranked neighbours in the catalog, worst-first suppressed.
    pub similar: Vec<Similarity>,
    /// The message the capture put on the inbox channel, where `aikit inbox`
    /// and any agent reading through the broker will see it.
    pub announcement: Option<Announcement>,
}

/// A capture, as the inbox channel carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// The channel item.
    pub item: InboxId,
    /// The planned Procedure that saving it would run, when it is ready to save.
    pub proposal: Option<ProcedureId>,
}

/// What accepting a capture did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub candidate: Candidate,
    /// The capsule that now exists.
    pub id: CapsuleId,
    /// The Procedure that wrote it; `aikit procedure undo` reverses it.
    pub procedure: ProcedureId,
    /// Edits applied; zero when the same plan had already been applied.
    pub applied: usize,
    /// The channel item that was resolved, when there was one.
    pub item: Option<InboxId>,
}

// ---------------------------------------------------------------------------
// Similarity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimilarityBasis {
    /// Byte-identical.
    ExactContent,
    /// Identical once whitespace is normalized.
    NormalizedContent,
    /// Overlapping phrasing, measured by token shingles.
    Shingles,
    /// Different text, same exported command name.
    ExportNames,
}

impl SimilarityBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            SimilarityBasis::ExactContent => "exact-content",
            SimilarityBasis::NormalizedContent => "normalized-content",
            SimilarityBasis::Shingles => "shingles",
            SimilarityBasis::ExportNames => "export-names",
        }
    }
}

/// One neighbour, with a number and a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Similarity {
    /// The capsule id (or candidate id) this resembles.
    pub other: String,
    pub basis: SimilarityBasis,
    /// 0–100. A percentage because that is what the palette shows.
    pub percentage: u8,
    /// Plain language: what is the same and what is not.
    pub summary: String,
}

/// Something a capture can be compared against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparable {
    pub id: String,
    pub text: String,
    pub exports: Vec<String>,
}

/// Below this the neighbour is noise, and showing it would train the user to
/// ignore the list.
const SIMILARITY_FLOOR: u8 = 35;

// ---------------------------------------------------------------------------
// Promotion
// ---------------------------------------------------------------------------

/// The fields a human fills in when promoting. Everything else is generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionEdits {
    pub id: CapsuleId,
    pub description: String,
    pub name: Option<String>,
    pub tags: Vec<String>,
    pub exports: Vec<String>,
    pub maturity: Maturity,
}

impl PromotionEdits {
    pub fn new(id: CapsuleId, description: impl Into<String>) -> Self {
        Self {
            id,
            description: description.into(),
            name: None,
            tags: Vec::new(),
            exports: Vec::new(),
            // A promoted capture is a draft until someone says otherwise; usage
            // never promotes maturity on its own.
            maturity: Maturity::Draft,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    pub fn with_tags<I: IntoIterator<Item = S>, S: Into<String>>(mut self, tags: I) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_exports<I: IntoIterator<Item = S>, S: Into<String>>(mut self, exports: I) -> Self {
        self.exports = exports.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_maturity(mut self, maturity: Maturity) -> Self {
        self.maturity = maturity;
        self
    }
}

/// Where a promoted capsule landed.
///
/// An alias has no payload file — its command line lives in the manifest — so
/// its `payload_path` is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedCapsule {
    pub id: CapsuleId,
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub payload_path: PathBuf,
}

/// What promoting a candidate would write, before any of it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRender {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: String,
    /// The payload file and its text; absent for an alias.
    pub payload: Option<(PathBuf, String)>,
    /// Whether the payload must be made executable.
    pub executable: bool,
}

// ---------------------------------------------------------------------------
// The inbox
// ---------------------------------------------------------------------------

pub struct Inbox<'a> {
    home: &'a AikitHome,
    index: &'a Index,
    scanner: Scanner,
}

impl<'a> Inbox<'a> {
    pub fn new(home: &'a AikitHome, index: &'a Index) -> Self {
        Self {
            home,
            index,
            scanner: Scanner::new(),
        }
    }

    /// Use a scanner carrying the user's extra patterns.
    #[must_use]
    pub fn with_scanner(mut self, scanner: Scanner) -> Self {
        self.scanner = scanner;
        self
    }

    /// Run the capture pipeline, comparing only against the inbox.
    pub fn capture(&self, capture: Capture) -> Result<CaptureOutcome> {
        self.run(capture, &[])
    }

    /// Run the capture pipeline, also comparing against a catalog.
    pub fn capture_against(
        &self,
        capture: Capture,
        catalog: &dyn Catalog,
    ) -> Result<CaptureOutcome> {
        let comparables = comparables_from_catalog(catalog);
        self.run(capture, &comparables)
    }

    fn run(&self, capture: Capture, comparables: &[Comparable]) -> Result<CaptureOutcome> {
        // Intent is checked before anything is stored: a refused capture leaves
        // nothing behind to clean up.
        validate_exports(&capture.exports)?;
        match capture.scope {
            CaptureScope::Project if capture.project_root.is_none() => {
                return err(
                    "inbox.scope_needs_project",
                    "a project-scoped capture needs a project, and the working directory is not \
                     inside one",
                )
            }
            CaptureScope::Session if capture.session.is_none() => {
                return err(
                    "inbox.scope_needs_session",
                    "a session-scoped capture needs an AIKit session, and this context has none",
                )
            }
            _ => {}
        }

        // Scan first. Everything downstream works on the redacted text, so there
        // is no path by which the raw secret reaches a file.
        let findings = self.scanner.scan(&capture.body);
        let stored_body = if findings.is_empty() {
            capture.body.clone()
        } else {
            self.scanner.redact(&capture.body)
        };

        let state = if findings.is_empty() {
            CandidateState::Ready
        } else {
            CandidateState::Quarantined
        };
        let classification = match capture.suggested_kind {
            Some(kind) => Classification {
                kind,
                confidence: Confidence::Declared,
                reason: "declared by the capturer".to_string(),
            },
            None => classify_with_confidence(&capture.body),
        };
        require_command_kind(classification.kind, &capture.exports)?;

        let body_hash = hash(&stored_body);
        let normalized_hash = hash(&normalize(&stored_body));

        // Exact and normalized dedup, before anything fuzzy is attempted.
        if let Some(existing) = self.find_duplicate(&body_hash, &normalized_hash)? {
            let existing = self.declare(existing, &capture)?;
            let announcement = self.announce(&existing)?;
            let similar = rank_similar(&capture.body, &capture.exports, comparables);
            return Ok(CaptureOutcome {
                duplicate_of: Some(existing.id.clone()),
                candidate: existing,
                similar,
                announcement,
            });
        }

        let id = format!("cnd_{}", ulid::Ulid::generate());
        let path = state.directory(self.home).join(&id);
        create_dir_all(&path)?;
        std::fs::write(path.join("body"), stored_body.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &path.join("body"), &e))?;

        let meta = CandidateMeta {
            exports: capture.exports.clone(),
            scope: Some(capture.scope),
            source_ref: capture.source_ref.clone(),
            by: capture.by.clone(),
            session: capture.session.as_ref().map(|s| s.as_str().to_string()),
            confidence: Some(classification.confidence),
            kind_reason: Some(classification.reason.clone()),
        };
        meta.store(&path)?;

        let candidate = Candidate {
            id,
            kind: classification.kind,
            confidence: classification.confidence,
            kind_reason: classification.reason,
            title: capture.title.clone(),
            state,
            findings,
            body_hash,
            normalized_hash,
            exports: capture.exports.clone(),
            scope: capture.scope,
            source_ref: capture.source_ref.clone(),
            by: capture.by.clone(),
            session: meta.session.clone(),
            project_root: capture.project_root.clone(),
            path,
            created_at: Timestamp::now(),
        };
        self.record(&candidate)?;
        let announcement = self.announce(&candidate)?;

        let similar = rank_similar(&capture.body, &capture.exports, comparables);
        Ok(CaptureOutcome {
            candidate,
            duplicate_of: None,
            similar,
            announcement,
        })
    }

    /// Put a candidate on the inbox channel, once.
    ///
    /// The candidate store and the channel are two stores for one inbox: the
    /// store holds the example, the channel is where it is *said* — to the person
    /// at `aikit inbox`, and to any agent reading through the broker. A candidate
    /// with nothing waiting on it is announced; one already announced is not
    /// announced again, so a second capture of the same text never nags.
    ///
    /// The message's evidence is the candidate, and the Procedure it proposes is
    /// the promotion itself — the exact plan that accepting it runs.
    fn announce(&self, candidate: &Candidate) -> Result<Option<Announcement>> {
        if !matches!(
            candidate.state,
            CandidateState::Ready | CandidateState::Quarantined
        ) {
            return Ok(None);
        }
        let channel = InboxChannel::new(self.index);
        let key = announcement_key(&candidate.id);
        if let Some(existing) = channel.pending_by_dedup_key(&key)? {
            return Ok(Some(Announcement {
                item: existing.id,
                proposal: existing.proposal,
            }));
        }

        let proposed = Self::proposed_id(candidate).ok();
        let (planned, why_not) = if candidate.state == CandidateState::Ready {
            match self.plan_promotion(candidate) {
                Ok(procedure) => (Some(procedure), None),
                Err(error) => (None, Some(error.message().to_string())),
            }
        } else {
            (None, None)
        };

        let mut body = String::new();
        if candidate.state == CandidateState::Quarantined {
            body.push_str(&format!(
                "Held back: the scanner found {} possible secret(s), so this is never written \
                 into a registry. Remove it with `aikit inbox reject {}`.\n",
                candidate.findings.len().max(1),
                candidate.id
            ));
        } else {
            body.push_str(&format!(
                "Captured as a **{}** ({} confidence: {}).\n",
                candidate.kind,
                candidate.confidence.as_str(),
                candidate.kind_reason
            ));
        }
        if let Some(by) = &candidate.by {
            body.push_str(&format!("\nObserved by {by}."));
        }
        if let Some(source) = &candidate.source_ref {
            body.push_str(&format!("\nFrom {source}."));
        }
        if let (Some(id), None) = (&proposed, &why_not) {
            if candidate.state == CandidateState::Ready {
                body.push_str(&format!("\n\nSaving it writes `{id}`"));
                if !candidate.exports.is_empty() {
                    let names: Vec<String> =
                        candidate.exports.iter().map(|e| format!("`{e}`")).collect();
                    body.push_str(&format!(", run as {}", names.join(", ")));
                }
                body.push_str(&format!(
                    ". `aikit inbox accept {0}` saves it; `aikit inbox reject {0}` turns it down.",
                    candidate.id
                ));
            }
        }
        if let Some(reason) = &why_not {
            body.push_str(&format!(
                "\n\nNot ready to save as it stands: {reason}. `aikit promote {} --id <ID>` \
                 names it yourself.",
                candidate.id
            ));
        }

        let title = if candidate.state == CandidateState::Quarantined {
            format!(
                "Held back: \u{201c}{}\u{201d} may contain a secret",
                candidate.title
            )
        } else {
            format!(
                "Save \u{201c}{}\u{201d} as a {}?",
                candidate.title, candidate.kind
            )
        };

        let mut evidence = vec![Evidence::Hash {
            label: "candidate".to_string(),
            value: candidate.id.clone(),
        }];
        if let Some(id) = &proposed {
            evidence.push(Evidence::Hash {
                label: "proposed-id".to_string(),
                value: id.to_string(),
            });
        }
        evidence.push(Evidence::File {
            path: candidate.body_path().display().to_string(),
        });
        if let Ok(preview) = self.preview(&candidate.id) {
            let preview: String = preview.chars().take(240).collect();
            evidence.push(Evidence::Summary { text: preview });
        }

        let mut item = NewItem::new(InboxKind::CaptureCandidate, title, body)
            .with_evidence(evidence)
            .deduped_by(key);
        if let Some(procedure) = &planned {
            item = item.proposing(procedure.id.clone());
        }
        let stored = channel.publish(item)?;
        Ok(Some(Announcement {
            item: stored.id,
            proposal: stored.proposal,
        }))
    }

    /// The Procedure that promoting `candidate` under its proposed id would run,
    /// built and persisted but not applied. A person can read it
    /// (`aikit procedure diff`), and accepting runs exactly it.
    pub fn plan_promotion(&self, candidate: &Candidate) -> Result<Procedure> {
        let procedure = self.build_promotion(candidate)?;
        ProcedureRunner::new(self.home).save(&procedure)?;
        Ok(procedure)
    }

    /// [`Inbox::plan_promotion`] without persisting it: the same bytes, so the
    /// same digest, which is how a stored plan is told from a stale one.
    fn build_promotion(&self, candidate: &Candidate) -> Result<Procedure> {
        let id = Self::proposed_id(candidate)?;
        let registry_root = self.registry_root(candidate)?;
        let edits = PromotionEdits::new(id.clone(), candidate.title.clone());
        let render = self.render_promotion(candidate, &edits, &registry_root)?;

        let mut plan = Plan::new()
            .with_note(format!("promote {} as {id}", candidate.id))
            .with_edit(WorldEdit::CreateDir {
                path: render.root.clone(),
                inverse: Inverse::Remove,
            });
        if let Some((payload_path, body)) = &render.payload {
            if let Some(parent) = payload_path.parent().filter(|p| *p != render.root) {
                plan = plan.with_edit(WorldEdit::CreateDir {
                    path: parent.to_path_buf(),
                    inverse: Inverse::Remove,
                });
            }
            plan = plan.with_edit(WorldEdit::WriteFileMode {
                path: payload_path.clone(),
                contents: body.clone().into_bytes(),
                mode: if render.executable { 0o755 } else { 0o644 },
                inverse: Inverse::Remove,
            });
        }
        plan = plan.with_edit(WorldEdit::WriteFile {
            path: render.manifest_path.clone(),
            contents: render.manifest.clone().into_bytes(),
            inverse: Inverse::Remove,
        });

        let plan = crate::procedure::bind_current_preconditions(plan)?;
        // One new capsule is a small, fully reversible write. A project's own
        // registry sits inside its git repository, and the automatic rule would
        // answer that with a branch in the person's repository — a far bigger act
        // than the one asked for — so the plan is staged in a shadow tree instead.
        let shadow = self
            .home
            .state()
            .join("procedures")
            .join(".shadow")
            .join(plan.digest().short());
        Procedure::new(
            ProcedureKind::Promote {
                candidate: candidate.id.clone(),
            },
            plan,
            MutationIsolation::Staged { shadow },
        )
    }

    /// Save a captured example: run the Procedure its message proposed (or plan
    /// one now, when the message predates the example's last change), then mark
    /// the example promoted and its message resolved.
    pub fn accept(&self, candidate_id: &str) -> Result<Accepted> {
        let candidate = self.require(candidate_id)?;
        match candidate.state {
            CandidateState::Promoted => {
                return Err(AikitError::new(
                    "inbox.already_promoted",
                    format!("{} was already saved", candidate.id),
                )
                .with("candidate", candidate.id.clone()))
            }
            CandidateState::Rejected => {
                return Err(AikitError::new(
                    "inbox.already_rejected",
                    format!(
                        "{} was turned down; capture it again to reconsider",
                        candidate.id
                    ),
                )
                .with("candidate", candidate.id.clone()))
            }
            CandidateState::Quarantined | CandidateState::Ready => {}
        }

        let fresh = self.build_promotion(&candidate)?;
        let channel = InboxChannel::new(self.index);
        let item = channel.pending_by_dedup_key(&announcement_key(&candidate.id))?;
        let runner = ProcedureRunner::new(self.home);
        let stored = item
            .as_ref()
            .and_then(|item| item.proposal.as_ref())
            .and_then(|id| runner.load(id).ok())
            .filter(|stored| stored.digest == fresh.digest);
        let procedure = match stored {
            Some(stored) => stored,
            None => {
                runner.save(&fresh)?;
                if let Some(item) = &item {
                    channel.set_proposal(&item.id, &fresh.id)?;
                }
                fresh
            }
        };

        let outcome = runner.run(&procedure)?;
        let id = Self::proposed_id(&candidate)?;
        self.finish_promotion(&candidate, &id)?;
        Ok(Accepted {
            candidate,
            id,
            procedure: procedure.id,
            applied: outcome.applied,
            item: item.map(|item| item.id),
        })
    }

    /// A second capture of text already in the inbox files nothing new — but if
    /// the second capturer *said* what it is (a kind, command names), that is
    /// worth more than the first guess. A declared kind replaces an inferred
    /// one, and the declared command names are added, on a candidate nobody has
    /// decided yet.
    fn declare(&self, existing: Candidate, capture: &Capture) -> Result<Candidate> {
        let undecided = matches!(
            existing.state,
            CandidateState::Ready | CandidateState::Quarantined
        );
        let says_kind = capture
            .suggested_kind
            .filter(|kind| *kind != existing.kind || existing.confidence != Confidence::Declared);
        let new_exports: Vec<String> = capture
            .exports
            .iter()
            .filter(|name| !existing.exports.contains(name))
            .cloned()
            .collect();
        if !undecided || (says_kind.is_none() && new_exports.is_empty()) {
            return Ok(existing);
        }

        let mut updated = existing.clone();
        if let Some(kind) = says_kind {
            updated.kind = kind;
            updated.confidence = Confidence::Declared;
            updated.kind_reason = "declared by the capturer".to_string();
        }
        updated.exports.extend(new_exports);
        require_command_kind(updated.kind, &updated.exports)?;

        let meta = CandidateMeta {
            exports: updated.exports.clone(),
            scope: Some(updated.scope),
            source_ref: updated.source_ref.clone(),
            by: updated.by.clone(),
            session: updated.session.clone(),
            confidence: Some(updated.confidence),
            kind_reason: Some(updated.kind_reason.clone()),
        };
        meta.store(&updated.path)?;
        self.index
            .conn()
            .execute(
                "UPDATE candidates SET kind = ?1 WHERE candidate_id = ?2",
                params![updated.kind.as_str(), updated.id],
            )
            .map_err(|e| sql_error("inbox.write_failed", &e))?;
        Ok(updated)
    }

    /// Every candidate, newest first.
    pub fn candidates(&self) -> Result<Vec<Candidate>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!(
                "{CANDIDATE_COLUMNS} ORDER BY created_ns DESC, candidate_id"
            ))
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        let rows = stmt
            .query_map([], candidate_row)
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| sql_error("inbox.query_failed", &e))??);
        }
        Ok(out)
    }

    pub fn candidate(&self, id: &str) -> Result<Option<Candidate>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!("{CANDIDATE_COLUMNS} WHERE candidate_id = ?1"))
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        let row = stmt
            .query_row(params![id], candidate_row)
            .optional()
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        row.transpose()
    }

    /// The body as it may be shown to a person.
    ///
    /// Redacted a second time on the way out. The stored body is already redacted,
    /// so this is belt and braces — but a preview is the one place a secret would
    /// be seen, and the cost of scanning twice is nothing next to the cost of
    /// being wrong once.
    pub fn preview(&self, id: &str) -> Result<String> {
        let candidate = self.require(id)?;
        let body = std::fs::read_to_string(candidate.body_path())
            .map_err(|e| io_error("inbox.unreadable", &candidate.body_path(), &e))?;
        Ok(self.scanner.redact(&body))
    }

    /// Move a quarantined candidate to the ready queue, if it is now clean.
    ///
    /// The check is a re-scan rather than a flag: "a human said it is fine" is not
    /// evidence that the secret is gone.
    pub fn release_from_quarantine(&self, id: &str) -> Result<Candidate> {
        let candidate = self.require(id)?;
        if candidate.state != CandidateState::Quarantined {
            return err(
                "inbox.not_quarantined",
                format!("{id} is not in quarantine"),
            );
        }
        let body = std::fs::read_to_string(candidate.body_path())
            .map_err(|e| io_error("inbox.unreadable", &candidate.body_path(), &e))?;
        let findings = self.scanner.scan(&body);
        if !findings.is_empty() {
            return Err(AikitError::new(
                "inbox.still_contains_a_secret",
                format!("{id} still matches {} secret rules", findings.len()),
            )
            .with("candidate", id)
            .with("findings", findings.len().to_string()));
        }
        let released = self.move_to(&candidate, CandidateState::Ready)?;
        // The message that said "held back" is stale now: close it and announce
        // the candidate as the ready proposal it has become.
        InboxChannel::new(self.index)
            .resolve_by_dedup_key(&announcement_key(&released.id), "released from quarantine")?;
        self.announce(&released)?;
        Ok(released)
    }

    pub fn reject(&self, id: &str, reason: &str) -> Result<Candidate> {
        let candidate = self.require(id)?;
        let moved = self.move_to(&candidate, CandidateState::Rejected)?;
        std::fs::write(moved.path.join("rejected-because"), reason.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &moved.path, &e))?;
        InboxChannel::new(self.index)
            .resolve_by_dedup_key(&announcement_key(&moved.id), &format!("rejected: {reason}"))?;
        Ok(moved)
    }

    /// The capsule id promotion would use when the person names none: the
    /// candidate's kind, its scope's namespace and a name taken from its first
    /// exported command, or else from its title.
    ///
    /// `script/personal/deploy` for a script captured with `--export deploy`;
    /// `skill/personal/rust-review` for a skill titled "Rust review".
    pub fn proposed_id(candidate: &Candidate) -> Result<CapsuleId> {
        let namespace = match candidate.scope {
            CaptureScope::Project => "project",
            CaptureScope::Home | CaptureScope::Session => "personal",
        };
        let from_exports = match candidate.kind {
            Kind::Script | Kind::Alias => candidate.exports.first().map(|name| slug(name)),
            _ => None,
        };
        let leaf = from_exports
            .filter(|leaf| !leaf.is_empty())
            .or_else(|| Some(slug(&candidate.title)).filter(|leaf| !leaf.is_empty()))
            .unwrap_or_else(|| slug(&candidate.id));
        CapsuleId::parse(&format!("{}/{namespace}/{leaf}", candidate.kind))
    }

    /// The registry a candidate is promoted into: the project's own `.aikit`
    /// for a project-scoped capture, the personal registry otherwise.
    pub fn registry_root(&self, candidate: &Candidate) -> Result<PathBuf> {
        match candidate.scope {
            CaptureScope::Project => {
                let project = candidate.project_root.as_ref().ok_or_else(|| {
                    AikitError::new(
                        "inbox.scope_needs_project",
                        format!(
                            "{} was captured for a project, but no project is recorded on it",
                            candidate.id
                        ),
                    )
                })?;
                Ok(project.join(".aikit"))
            }
            CaptureScope::Home | CaptureScope::Session => Ok(self.home.registry("personal")),
        }
    }

    /// Everything promotion would write, without writing any of it.
    ///
    /// Promotion and a planned Procedure are the same act seen twice, so they
    /// share this: what a person reviews is, byte for byte, what runs.
    pub fn render_promotion(
        &self,
        candidate: &Candidate,
        edits: &PromotionEdits,
        registry_root: &Path,
    ) -> Result<PromotionRender> {
        if candidate.state == CandidateState::Quarantined {
            return Err(AikitError::new(
                "inbox.quarantined",
                format!(
                    "{} was quarantined because the scanner found {} possible secrets; a \
                     quarantined capture is never written into a registry",
                    candidate.id,
                    candidate.findings.len()
                ),
            )
            .with("candidate", candidate.id.clone())
            .with("findings", candidate.findings.len().to_string()));
        }
        match candidate.state {
            CandidateState::Promoted => {
                return Err(AikitError::new(
                    "inbox.already_promoted",
                    format!("{} was already saved", candidate.id),
                )
                .with("candidate", candidate.id.clone()))
            }
            CandidateState::Rejected => {
                return Err(AikitError::new(
                    "inbox.already_rejected",
                    format!(
                        "{} was turned down; capture it again to reconsider",
                        candidate.id
                    ),
                )
                .with("candidate", candidate.id.clone()))
            }
            CandidateState::Ready | CandidateState::Quarantined => {}
        }
        if edits.id.kind() != candidate.kind {
            return Err(AikitError::new(
                "inbox.kind_mismatch",
                format!(
                    "this candidate was classified as a {} but `{}` names a {}",
                    candidate.kind,
                    edits.id,
                    edits.id.kind()
                ),
            )
            .with("candidate", candidate.id.clone()));
        }

        let root = registry_root.join(edits.id.registry_path());
        if root.join(crate::registry::MANIFEST_FILE).exists() {
            return Err(AikitError::new(
                "inbox.id_taken",
                format!("`{}` already exists in this registry", edits.id),
            )
            .with("capability", edits.id.to_string())
            .with("path", root.display().to_string()));
        }

        let body = std::fs::read_to_string(candidate.body_path())
            .map_err(|e| io_error("inbox.unreadable", &candidate.body_path(), &e))?;
        let exports = if edits.exports.is_empty() {
            candidate.exports.clone()
        } else {
            edits.exports.clone()
        };
        let entry = entry_for(candidate.kind);
        let manifest = render_manifest(candidate, edits, &exports, entry, &body);

        // Parse before writing. A generated manifest that would not load is a bug
        // in this function, and the user should never meet it as a broken capsule
        // in their registry.
        Capsule::from_toml_str(&manifest)?;

        // An alias carries its command line in the manifest; every other kind
        // keeps the captured text as a payload file beside it.
        let payload = (candidate.kind != Kind::Alias).then(|| (root.join(entry), body));
        Ok(PromotionRender {
            manifest_path: root.join(crate::registry::MANIFEST_FILE),
            manifest,
            payload,
            executable: candidate.kind.is_executable(),
            root,
        })
    }

    /// Write a candidate into a registry as a real capsule.
    pub fn promote(
        &self,
        id: &str,
        edits: &PromotionEdits,
        registry_root: &Path,
    ) -> Result<PromotedCapsule> {
        let candidate = self.require(id)?;
        let render = self.render_promotion(&candidate, edits, registry_root)?;

        create_dir_all(&render.root)?;
        if let Some((payload_path, body)) = &render.payload {
            if let Some(parent) = payload_path.parent() {
                create_dir_all(parent)?;
            }
            std::fs::write(payload_path, body.as_bytes())
                .map_err(|e| io_error("inbox.write_failed", payload_path, &e))?;
            if render.executable {
                make_executable(payload_path)?;
            }
        }
        std::fs::write(&render.manifest_path, render.manifest.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &render.manifest_path, &e))?;

        self.finish_promotion(&candidate, &edits.id)?;

        Ok(PromotedCapsule {
            id: edits.id.clone(),
            root: render.root,
            manifest_path: render.manifest_path,
            payload_path: render
                .payload
                .map(|(path, _)| path)
                .unwrap_or_else(PathBuf::new),
        })
    }

    /// Mark a candidate promoted and close its message. The files are written by
    /// whoever promoted it — [`Inbox::promote`], or a Procedure that ran the same
    /// render — so saving it by either road leaves the inbox in one state.
    pub fn finish_promotion(&self, candidate: &Candidate, saved_as: &CapsuleId) -> Result<()> {
        self.set_state(&candidate.id, CandidateState::Promoted)?;
        InboxChannel::new(self.index).resolve_by_dedup_key(
            &announcement_key(&candidate.id),
            &format!("saved as {saved_as}"),
        )?;
        Ok(())
    }

    // -- internals ---------------------------------------------------------

    fn require(&self, id: &str) -> Result<Candidate> {
        self.candidate(id)?.ok_or_else(|| {
            AikitError::new("inbox.unknown_candidate", format!("no candidate `{id}`"))
                .with("candidate", id)
        })
    }

    fn find_duplicate(&self, body_hash: &str, normalized_hash: &str) -> Result<Option<Candidate>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!(
                "{CANDIDATE_COLUMNS} WHERE (body_hash = ?1 OR normalized_hash = ?2)
                 AND state != 'rejected' ORDER BY created_ns LIMIT 1"
            ))
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        let row = stmt
            .query_row(params![body_hash, normalized_hash], candidate_row)
            .optional()
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        row.transpose()
    }

    fn record(&self, candidate: &Candidate) -> Result<()> {
        self.index
            .conn()
            .execute(
                "INSERT INTO candidates
                     (candidate_id, kind, title, body_hash, normalized_hash, state, quarantined,
                      project_root, path, created_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(candidate_id) DO UPDATE SET
                     state = excluded.state,
                     quarantined = excluded.quarantined,
                     path = excluded.path",
                params![
                    candidate.id,
                    candidate.kind.as_str(),
                    candidate.title,
                    candidate.body_hash,
                    candidate.normalized_hash,
                    candidate.state.as_str(),
                    i32::from(candidate.state == CandidateState::Quarantined),
                    candidate
                        .project_root
                        .as_ref()
                        .map(|p| p.display().to_string()),
                    candidate.path.display().to_string(),
                    candidate.created_at.as_nanos(),
                ],
            )
            .map_err(|e| sql_error("inbox.write_failed", &e))?;
        Ok(())
    }

    fn set_state(&self, id: &str, state: CandidateState) -> Result<()> {
        self.index
            .conn()
            .execute(
                "UPDATE candidates SET state = ?1, quarantined = ?2 WHERE candidate_id = ?3",
                params![
                    state.as_str(),
                    i32::from(state == CandidateState::Quarantined),
                    id
                ],
            )
            .map_err(|e| sql_error("inbox.write_failed", &e))?;
        Ok(())
    }

    fn move_to(&self, candidate: &Candidate, state: CandidateState) -> Result<Candidate> {
        let destination = state.directory(self.home).join(&candidate.id);
        if destination != candidate.path {
            create_dir_all(state.directory(self.home).as_path())?;
            std::fs::rename(&candidate.path, &destination)
                .map_err(|e| io_error("inbox.write_failed", &destination, &e))?;
        }
        let mut moved = candidate.clone();
        moved.state = state;
        moved.path = destination;
        self.record(&moved)?;
        Ok(moved)
    }
}

// ---------------------------------------------------------------------------
// Pipeline stages
// ---------------------------------------------------------------------------

/// Cosmetic differences are not new capabilities.
///
/// Line endings, trailing whitespace, runs of blank lines and a trailing newline
/// all vary between the terminal, an editor and a clipboard, and none of them
/// changes what a script does.
pub fn normalize(text: &str) -> String {
    let unified = text.replace("\r\n", "\n");
    let mut out: Vec<String> = Vec::new();
    let mut blank_run = 0usize;
    for line in unified.lines().map(str::trim_end) {
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push(line.to_string());
    }
    out.join("\n").trim().to_string()
}

/// What the classifier made of a capture, and how far to trust it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: Kind,
    pub confidence: Confidence,
    /// One plain sentence a person can read next to the answer.
    pub reason: String,
}

/// Infer what a capture is, when the caller has not said.
///
/// Deliberately conservative and explainable: a shebang is a script, YAML front
/// matter naming a skill is a skill, shell syntax is a script, prose is guidance.
/// Nothing here guesses from statistics, because a wrong guess costs the user an
/// edit and a wrong *confident* guess costs them trust.
pub fn classify(body: &str) -> Kind {
    classify_with_confidence(body).kind
}

/// [`classify`], with how sure it is and why.
///
/// Text it cannot place is *prose*, at low confidence — never a script. A note
/// that happens to be captured is not a command, and filing it as one would put
/// an "executable" in the registry that nobody wrote.
pub fn classify_with_confidence(body: &str) -> Classification {
    let trimmed = body.trim_start();
    let said = |kind, confidence, reason: &str| Classification {
        kind,
        confidence,
        reason: reason.to_string(),
    };
    if trimmed.starts_with("#!") {
        return said(Kind::Script, Confidence::High, "starts with a shebang line");
    }
    if trimmed.starts_with("---") {
        let front = trimmed.trim_start_matches("---");
        if front.contains("name:") || front.contains("description:") {
            return said(
                Kind::Skill,
                Confidence::High,
                "opens with YAML front matter that names a skill",
            );
        }
    }
    if body.contains("PreToolUse") || body.contains("PostToolUse") || body.contains("SessionStart")
    {
        return said(
            Kind::Hook,
            Confidence::Medium,
            "mentions a hook event (PreToolUse, PostToolUse or SessionStart)",
        );
    }
    if trimmed.starts_with('#') || trimmed.starts_with("> ") {
        return said(
            Kind::Guidance,
            Confidence::Medium,
            "opens with a markdown heading or quote",
        );
    }
    if let Some((cued, total)) = shell_shape(body) {
        return Classification {
            kind: Kind::Script,
            confidence: Confidence::Medium,
            reason: format!(
                "{cued} of {total} lines read as shell commands (flags, pipes, redirects, \
                 variables or paths)"
            ),
        };
    }
    said(
        Kind::Guidance,
        Confidence::Low,
        "reads as prose: no shebang, front matter, hook event or shell syntax — say what it is \
         with --kind",
    )
}

/// `(lines with shell syntax, command lines)` when at least half of the
/// non-comment lines carry some.
fn shell_shape(body: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let cued = lines.iter().filter(|line| has_shell_syntax(line)).count();
    (cued > 0 && cued * 2 >= lines.len()).then_some((cued, lines.len()))
}

/// Syntax a sentence does not have: operators, flags, `$variables`, paths and
/// `NAME=value` prefixes. Each is a structural cue, so the verdict can be
/// explained and does not depend on which commands this machine happens to have.
fn has_shell_syntax(line: &str) -> bool {
    if ["&&", "||", "$(", "${", "2>&1", ">>"]
        .iter()
        .any(|operator| line.contains(operator))
    {
        return true;
    }
    line.split_whitespace().any(|word| {
        let mut chars = word.chars();
        let first = chars.next();
        let second = chars.next();
        matches!(word, "|" | ">" | "<" | "&")
            || (word.starts_with("--")
                && word.len() > 2
                && word[2..].starts_with(|c: char| c.is_ascii_alphanumeric()))
            || (first == Some('-')
                && second.is_some_and(|c| c.is_ascii_alphabetic())
                && word.len() == 2)
            || (first == Some('$') && second.is_some_and(|c| c.is_ascii_alphabetic() || c == '_'))
            || word.starts_with("./")
            || word.starts_with("../")
            || word.starts_with("~/")
            || (first == Some('/') && second.is_some_and(|c| c.is_ascii_alphanumeric()))
            || is_env_assignment(word)
    })
}

fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    })
}

/// Command names become files in the contextual `bin/`, so each must be usable
/// as one: no path separators, no whitespace, not `.` or `..`.
fn validate_exports(exports: &[String]) -> Result<()> {
    for name in exports {
        let bad = name.is_empty()
            || name.len() > 64
            || name == "."
            || name == ".."
            || name.starts_with('-')
            || name
                .chars()
                .any(|c| c.is_whitespace() || c == '/' || c == '\\' || c == '\0');
        if bad {
            return Err(AikitError::new(
                "inbox.invalid_export",
                format!(
                    "`{name}` cannot be a command name: use letters, digits, `-`, `_` or `.`, \
                     with no spaces or slashes"
                ),
            )
            .with("export", name.clone()));
        }
    }
    Ok(())
}

/// `--export` names commands, and only a script or an alias is one.
fn require_command_kind(kind: Kind, exports: &[String]) -> Result<()> {
    if exports.is_empty() || matches!(kind, Kind::Script | Kind::Alias) {
        return Ok(());
    }
    Err(AikitError::new(
        "inbox.exports_need_a_command",
        format!(
            "--export names a command, but this capture is a {kind}; \
             say `--kind script` or `--kind alias` if it is one"
        ),
    )
    .with("kind", kind.as_str()))
}

/// The dedup key that ties a candidate to its message on the channel.
pub fn announcement_key(candidate_id: &str) -> String {
    format!("capture-candidate:{candidate_id}")
}

/// A lowercase, id-safe slug: letters and digits, with `-` between words.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    trimmed
        .chars()
        .take(48)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

// ---------------------------------------------------------------------------
// Similarity
// ---------------------------------------------------------------------------

fn comparables_from_catalog(catalog: &dyn Catalog) -> Vec<Comparable> {
    catalog
        .capsules()
        .into_iter()
        .filter_map(|capsule| {
            let root = capsule.root.as_ref()?;
            let entry = primary_entry(capsule)?;
            let text = std::fs::read_to_string(root.join(entry)).ok()?;
            Some(Comparable {
                id: capsule.id.to_string(),
                text,
                exports: capsule.exported_commands(),
            })
        })
        .collect()
}

fn primary_entry(capsule: &Capsule) -> Option<String> {
    Some(match &capsule.payload {
        aikit_core::Payload::Script(s) => s.entry.clone(),
        aikit_core::Payload::Hook(h) => h.entry.clone(),
        aikit_core::Payload::Guidance(g) => g.entry.clone(),
        aikit_core::Payload::Skill(s) => format!("{}/SKILL.md", s.root),
        _ => return None,
    })
}

/// Rank neighbours by the four measures, best basis first.
///
/// `body` is the raw capture text: the exact-match test has to see the bytes,
/// and everything after it normalizes for itself.
pub fn rank_similar(body: &str, exports: &[String], comparables: &[Comparable]) -> Vec<Similarity> {
    let mine = normalize(body);
    let my_shingles = shingles(&mine);

    let mut out: Vec<Similarity> = Vec::new();
    for other in comparables {
        let theirs = normalize(&other.text);
        let shared_exports: Vec<&String> = exports
            .iter()
            .filter(|e| other.exports.contains(e))
            .collect();

        let similarity = if other.text == body {
            Some(Similarity {
                other: other.id.clone(),
                basis: SimilarityBasis::ExactContent,
                percentage: 100,
                summary: "identical content".to_string(),
            })
        } else if theirs == mine {
            Some(Similarity {
                other: other.id.clone(),
                basis: SimilarityBasis::NormalizedContent,
                percentage: 99,
                summary: "identical once whitespace and blank lines are normalized".to_string(),
            })
        } else {
            let overlap = jaccard(&my_shingles, &shingles(&theirs));
            let percentage = (overlap * 100.0).round() as u8;
            if percentage >= SIMILARITY_FLOOR {
                Some(Similarity {
                    other: other.id.clone(),
                    basis: SimilarityBasis::Shingles,
                    percentage,
                    summary: line_summary(&mine, &theirs, &other.id, percentage),
                })
            } else if !shared_exports.is_empty() {
                let names: Vec<String> = shared_exports.iter().map(|s| format!("`{s}`")).collect();
                Some(Similarity {
                    other: other.id.clone(),
                    basis: SimilarityBasis::ExportNames,
                    percentage: percentage.max(1),
                    summary: format!(
                        "different content, but both export {} — activating both in one context \
                         would collide",
                        names.join(", ")
                    ),
                })
            } else {
                None
            }
        };

        if let Some(similarity) = similarity {
            out.push(similarity);
        }
    }

    out.sort_by(|a, b| {
        b.percentage
            .cmp(&a.percentage)
            .then_with(|| a.basis.cmp(&b.basis))
            .then_with(|| a.other.cmp(&b.other))
    });
    out
}

/// A sentence a person can act on, rather than a bare number.
fn line_summary(mine: &str, theirs: &str, other: &str, percentage: u8) -> String {
    let my_lines: BTreeSet<&str> = mine.lines().collect();
    let their_lines: BTreeSet<&str> = theirs.lines().collect();
    let only_mine = my_lines.difference(&their_lines).count();
    let only_theirs = their_lines.difference(&my_lines).count();

    format!(
        "{percentage}% of the phrasing is shared; {only_mine} line(s) appear only in the capture \
         and {only_theirs} only in {other}"
    )
}

/// Word trigrams. Trigrams rather than single words because a shared vocabulary
/// ("cargo", "run", "the") says much less than a shared phrase.
fn shingles(text: &str) -> BTreeSet<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 3 {
        return words.iter().map(|w| w.to_string()).collect();
    }
    words.windows(3).map(|w| w.join(" ")).collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

// ---------------------------------------------------------------------------
// Manifest generation
// ---------------------------------------------------------------------------

fn entry_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Script => "payload/run.sh",
        Kind::Hook => "payload/check",
        Kind::Guidance => "payload/guidance.md",
        Kind::Skill => "payload/SKILL.md",
        Kind::Alias => "payload/alias.sh",
        Kind::Session => "payload/session.toml",
        Kind::Template => "payload/template",
        Kind::Tool => "payload/tool",
        Kind::ToolProtocol => "payload/tool-protocol",
    }
}

/// Render a manifest for a promoted candidate.
///
/// This is the function that makes release-blocking case 11 true. It is plain
/// string building rather than a serializer because the result is a file a person
/// will read and edit next: the comment at the top, the field order and the
/// spacing are part of the output, not incidental to it.
fn render_manifest(
    candidate: &Candidate,
    edits: &PromotionEdits,
    exports: &[String],
    entry: &str,
    body: &str,
) -> String {
    let name = edits
        .name
        .clone()
        .unwrap_or_else(|| edits.id.leaf().to_string());
    let mut out = String::new();
    out.push_str(
        "# Generated by `aikit promote`. Edit freely — this is now an ordinary capsule.\n",
    );
    out.push_str("schema = 1\n");
    out.push_str(&format!("id = {}\n", quote(&edits.id.to_string())));
    out.push_str(&format!("kind = {}\n", quote(edits.id.kind().as_str())));
    out.push_str(&format!("name = {}\n", quote(&name)));
    out.push_str(&format!("description = {}\n", quote(&edits.description)));
    out.push_str(&format!("maturity = {}\n", quote(edits.maturity.as_str())));
    if !edits.tags.is_empty() {
        let rendered: Vec<String> = edits.tags.iter().map(|t| quote(t)).collect();
        out.push_str(&format!("tags = [{}]\n", rendered.join(", ")));
    }

    out.push_str("\n[provenance]\n");
    out.push_str("source = \"harvested\"\n");
    out.push_str(&format!(
        "created_at = {}\n",
        quote(&candidate.created_at.to_string())
    ));
    out.push_str(&format!("source_event = {}\n", quote(&candidate.id)));
    if let Some(by) = &candidate.by {
        out.push_str(&format!("author = {}\n", quote(by)));
    }
    if let Some(session) = &candidate.session {
        out.push_str(&format!("session_id = {}\n", quote(session)));
    }
    if let Some(source_ref) = &candidate.source_ref {
        out.push_str(&format!("source_ref = {}\n", quote(source_ref)));
    }

    out.push_str(&format!("\n[{}]\n", edits.id.kind().as_str()));
    match edits.id.kind() {
        Kind::Skill => {
            out.push_str("root = \"payload\"\n");
        }
        Kind::Hook => {
            out.push_str(&format!("entry = {}\n", quote(entry)));
            out.push_str("events = [\"PreToolUse\"]\n");
        }
        Kind::Guidance => {
            out.push_str(&format!("entry = {}\n", quote(entry)));
        }
        Kind::Script => {
            out.push_str(&format!("entry = {}\n", quote(entry)));
            // A captured command line has no shebang, and a file with none cannot
            // be executed directly. Say how to run it rather than rewriting what
            // was captured.
            if !body.trim_start().starts_with("#!") {
                out.push_str("interpreter = [\"sh\"]\n");
            }
            if !exports.is_empty() {
                let rendered: Vec<String> = exports.iter().map(|e| quote(e)).collect();
                out.push_str(&format!("exports = [{}]\n", rendered.join(", ")));
            }
        }
        Kind::Alias => {
            let command = exports.first().map_or(edits.id.leaf(), String::as_str);
            out.push_str(&format!("name = {}\n", quote(command)));
            out.push_str(&format!("body = {}\n", quote(body.trim_end())));
        }
        Kind::Session => out.push_str(&format!("spec = {}\n", quote(entry))),
        Kind::Template => out.push_str("root = \"payload\"\n"),
        Kind::Tool => out.push_str(&format!("commands = [{}]\n", quote(edits.id.leaf()))),
        Kind::ToolProtocol => out.push_str(&format!(
            "[tool-protocol.server]\ncommand = {}\n",
            quote(edits.id.leaf())
        )),
    }
    out
}

/// TOML basic-string quoting for the small set of values a manifest carries.
fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| io_error("inbox.write_failed", path, &e))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

const CANDIDATE_COLUMNS: &str = "SELECT candidate_id, kind, title, body_hash, normalized_hash, \
                                 state, project_root, path, created_ns FROM candidates";

fn candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Candidate>> {
    let id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let title: String = row.get(2)?;
    let body_hash: String = row.get(3)?;
    let normalized_hash: String = row.get(4)?;
    let state: String = row.get(5)?;
    let project_root: Option<String> = row.get(6)?;
    let path: String = row.get(7)?;
    let created: i64 = row.get(8)?;

    Ok((|| {
        let state = CandidateState::parse(&state);
        let path = PathBuf::from(path);
        let meta = CandidateMeta::load(&path);
        Ok(Candidate {
            id,
            kind: std::str::FromStr::from_str(&kind)?,
            // A candidate filed before intent was recorded was always inferred.
            confidence: meta.confidence.unwrap_or(Confidence::Medium),
            kind_reason: meta
                .kind_reason
                .unwrap_or_else(|| "classified when it was captured".to_string()),
            title,
            state,
            // The findings are not re-hydrated from the row: what matters after
            // the fact is *that* it was quarantined, and the redacted body plus a
            // fresh scan can always say more than a stale copy would.
            findings: Vec::new(),
            body_hash,
            normalized_hash,
            exports: meta.exports,
            scope: meta.scope.unwrap_or_default(),
            source_ref: meta.source_ref,
            by: meta.by,
            session: meta.session,
            project_root: project_root.map(PathBuf::from),
            path,
            created_at: Timestamp::from_nanos(created),
        })
    })())
}
