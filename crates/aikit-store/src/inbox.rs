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
use std::path::{Path, PathBuf};

use aikit_core::catalog::Catalog;
use aikit_core::error::err;
use aikit_core::{AikitError, Capsule, CapsuleId, Kind, Maturity, Result, SessionId};
use rusqlite::{params, OptionalExtension};

use crate::events::Timestamp;
use crate::home::{create_dir_all, io_error};
use crate::index::{sql_error, Index};
use crate::scan::{Finding, Scanner};
use crate::AikitHome;

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

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
        }
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
    pub title: String,
    pub state: CandidateState,
    /// What the scanner found. Never contains the secrets themselves.
    pub findings: Vec<Finding>,
    /// blake3 of the stored (redacted) body.
    pub body_hash: String,
    /// blake3 of the normalized form, so cosmetic edits are recognized.
    pub normalized_hash: String,
    pub exports: Vec<String>,
    pub project_root: Option<PathBuf>,
    pub path: PathBuf,
    pub created_at: Timestamp,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedCapsule {
    pub id: CapsuleId,
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub payload_path: PathBuf,
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
        let kind = capture
            .suggested_kind
            .unwrap_or_else(|| classify(&capture.body));

        let body_hash = hash(&stored_body);
        let normalized_hash = hash(&normalize(&stored_body));

        // Exact and normalized dedup, before anything fuzzy is attempted.
        if let Some(existing) = self.find_duplicate(&body_hash, &normalized_hash)? {
            let similar = rank_similar(&capture.body, &capture.exports, comparables);
            return Ok(CaptureOutcome {
                duplicate_of: Some(existing.id.clone()),
                candidate: existing,
                similar,
            });
        }

        let id = format!("cnd_{}", ulid::Ulid::generate());
        let path = state.directory(self.home).join(&id);
        create_dir_all(&path)?;
        std::fs::write(path.join("body"), stored_body.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &path.join("body"), &e))?;

        let candidate = Candidate {
            id,
            kind,
            title: capture.title.clone(),
            state,
            findings,
            body_hash,
            normalized_hash,
            exports: capture.exports.clone(),
            project_root: capture.project_root.clone(),
            path,
            created_at: Timestamp::now(),
        };
        self.record(&candidate)?;

        let similar = rank_similar(&capture.body, &capture.exports, comparables);
        Ok(CaptureOutcome {
            candidate,
            duplicate_of: None,
            similar,
        })
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
        self.move_to(&candidate, CandidateState::Ready)
    }

    pub fn reject(&self, id: &str, reason: &str) -> Result<Candidate> {
        let candidate = self.require(id)?;
        let moved = self.move_to(&candidate, CandidateState::Rejected)?;
        std::fs::write(moved.path.join("rejected-because"), reason.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &moved.path, &e))?;
        Ok(moved)
    }

    /// Write a candidate into a registry as a real capsule.
    pub fn promote(
        &self,
        id: &str,
        edits: &PromotionEdits,
        registry_root: &Path,
    ) -> Result<PromotedCapsule> {
        let candidate = self.require(id)?;

        if candidate.state == CandidateState::Quarantined {
            return Err(AikitError::new(
                "inbox.quarantined",
                format!(
                    "{id} was quarantined because the scanner found {} possible secrets; a \
                     quarantined capture is never written into a registry",
                    candidate.findings.len()
                ),
            )
            .with("candidate", id)
            .with("findings", candidate.findings.len().to_string()));
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
            .with("candidate", id));
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

        let entry = entry_for(candidate.kind);
        let manifest = render_manifest(&candidate, edits, entry);

        // Parse before writing. A generated manifest that would not load is a bug
        // in this function, and the user should never meet it as a broken capsule
        // in their registry.
        Capsule::from_toml_str(&manifest)?;

        let body = std::fs::read_to_string(candidate.body_path())
            .map_err(|e| io_error("inbox.unreadable", &candidate.body_path(), &e))?;

        let manifest_path = root.join(crate::registry::MANIFEST_FILE);
        let payload_path = root.join(entry);
        create_dir_all(&root)?;
        if let Some(parent) = payload_path.parent() {
            create_dir_all(parent)?;
        }
        std::fs::write(&payload_path, body.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &payload_path, &e))?;
        if candidate.kind.is_executable() {
            make_executable(&payload_path)?;
        }
        std::fs::write(&manifest_path, manifest.as_bytes())
            .map_err(|e| io_error("inbox.write_failed", &manifest_path, &e))?;

        self.set_state(&candidate.id, CandidateState::Promoted)?;
        self.index
            .conn()
            .execute(
                "DELETE FROM promotion_queue WHERE candidate_id = ?1",
                params![candidate.id],
            )
            .map_err(|e| sql_error("inbox.write_failed", &e))?;

        Ok(PromotedCapsule {
            id: edits.id.clone(),
            root,
            manifest_path,
            payload_path,
        })
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

/// Infer what a capture is, when the caller has not said.
///
/// Deliberately conservative and explainable: a shebang is a script, YAML front
/// matter naming a skill is a skill, prose is guidance. Nothing here guesses from
/// statistics, because a wrong guess costs the user an edit and a wrong *confident*
/// guess costs them trust.
pub fn classify(body: &str) -> Kind {
    let trimmed = body.trim_start();
    if trimmed.starts_with("#!") {
        return Kind::Script;
    }
    if trimmed.starts_with("---") {
        let front = trimmed.trim_start_matches("---");
        if front.contains("name:") || front.contains("description:") {
            return Kind::Skill;
        }
    }
    if body.contains("PreToolUse") || body.contains("PostToolUse") || body.contains("SessionStart")
    {
        return Kind::Hook;
    }
    if trimmed.starts_with('#') || trimmed.starts_with("> ") {
        return Kind::Guidance;
    }
    Kind::Script
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
    }
}

/// Render a manifest for a promoted candidate.
///
/// This is the function that makes release-blocking case 11 true. It is plain
/// string building rather than a serializer because the result is a file a person
/// will read and edit next: the comment at the top, the field order and the
/// spacing are part of the output, not incidental to it.
fn render_manifest(candidate: &Candidate, edits: &PromotionEdits, entry: &str) -> String {
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
            if !edits.exports.is_empty() {
                let rendered: Vec<String> = edits.exports.iter().map(|e| quote(e)).collect();
                out.push_str(&format!("exports = [{}]\n", rendered.join(", ")));
            }
        }
        Kind::Alias => {
            out.push_str(&format!("name = {}\n", quote(edits.id.leaf())));
            out.push_str("body = \"\"\n");
        }
        Kind::Session => out.push_str(&format!("spec = {}\n", quote(entry))),
        Kind::Template => out.push_str("root = \"payload\"\n"),
        Kind::Tool => out.push_str(&format!("commands = [{}]\n", quote(edits.id.leaf()))),
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
        Ok(Candidate {
            id,
            kind: std::str::FromStr::from_str(&kind)?,
            title,
            state,
            // The findings are not re-hydrated from the row: what matters after
            // the fact is *that* it was quarantined, and the redacted body plus a
            // fresh scan can always say more than a stale copy would.
            findings: Vec::new(),
            body_hash,
            normalized_hash,
            exports: Vec::new(),
            project_root: project_root.map(PathBuf::from),
            path: PathBuf::from(path),
            created_at: Timestamp::from_nanos(created),
        })
    })())
}
