//! The Inbox as the system's own channel.
//!
//! Part I treated the inbox as a staging area for capture candidates
//! ([`crate::inbox`]). Spec II §2 makes it larger: the inbox is the place where
//! AIKit — and agents operating through it — address the user, durably and
//! addressably. A system that can observe, plan and act but has no channel of its
//! own is a tool; one that can also *say what it found and what it proposes* is an
//! agent.
//!
//! Three properties make it a channel rather than a list, and each is enforced
//! here:
//!
//! 1. **An item may carry a planned Procedure** ([`InboxItem::proposal`]), so
//!    resolution is often one keystroke.
//! 2. **It is readable by agents through the broker** — `aikit inbox list --json`
//!    is a brokered capability, so a session can open with real context.
//! 3. **Agents can write to it** — an [`InboxKind::AgentProposal`] lets a session
//!    publish an observation rather than lose it at session end.
//!
//! **Redaction is unconditional.** Every item's title and body pass through the
//! built-in [`crate::scan`] scanner before storage, so a secret can never reach a
//! preview, a log, or a git write — the same rule the capture pipeline enforces,
//! applied to every message on the channel.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use aikit_core::id::{InboxId, ProcedureId, ProjectId};
use aikit_core::{AikitError, Result};

use crate::events::Timestamp;
use crate::index::{sql_error, Index};
use crate::scan;

/// What kind of thing the system is saying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxKind {
    /// "You ran this three times; want it as a script?"
    CaptureCandidate,
    /// "superpowers is v4.2.0 in Codex and v6.1.1 in the plugin cache."
    VersionConflict,
    /// "This revision changed; it needs a look before it activates."
    TrustReview,
    /// ".aikit, .claude and AGENTS.md disagree" — or a projected payload was
    /// edited out of band (PRIOR-ART-ACTIONS L2).
    DriftNotice,
    /// "collate ran; here is what changed."
    ProcedureReport,
    /// An agent publishing a suggestion to the user.
    AgentProposal,
    /// "This symlink is dead; this bridge calls a missing subcommand."
    Breakage,
}

impl InboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InboxKind::CaptureCandidate => "capture-candidate",
            InboxKind::VersionConflict => "version-conflict",
            InboxKind::TrustReview => "trust-review",
            InboxKind::DriftNotice => "drift-notice",
            InboxKind::ProcedureReport => "procedure-report",
            InboxKind::AgentProposal => "agent-proposal",
            InboxKind::Breakage => "breakage",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        Ok(match raw {
            "capture-candidate" => InboxKind::CaptureCandidate,
            "version-conflict" => InboxKind::VersionConflict,
            "trust-review" => InboxKind::TrustReview,
            "drift-notice" => InboxKind::DriftNotice,
            "procedure-report" => InboxKind::ProcedureReport,
            "agent-proposal" => InboxKind::AgentProposal,
            "breakage" => InboxKind::Breakage,
            other => {
                return Err(AikitError::new(
                    "inbox.unknown_kind",
                    format!("`{other}` is not an inbox kind"),
                ))
            }
        })
    }
}

/// Where an item is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum InboxState {
    /// Awaiting the user.
    Open,
    /// Snoozed until a time; treated as open again once it passes.
    Deferred { until: Timestamp },
    /// The user decided; kept for audit with the decision recorded.
    Resolved { decision: String },
}

impl InboxState {
    fn label(&self) -> &'static str {
        match self {
            InboxState::Open => "open",
            InboxState::Deferred { .. } => "deferred",
            InboxState::Resolved { .. } => "resolved",
        }
    }

    fn detail(&self) -> Option<String> {
        match self {
            InboxState::Open => None,
            InboxState::Deferred { until } => Some(until.as_nanos().to_string()),
            InboxState::Resolved { decision } => Some(decision.clone()),
        }
    }

    fn from_parts(label: &str, detail: Option<String>) -> Result<Self> {
        Ok(match label {
            "open" => InboxState::Open,
            "deferred" => {
                let nanos = detail
                    .as_deref()
                    .and_then(|d| d.parse::<i64>().ok())
                    .ok_or_else(|| {
                        AikitError::new("inbox.corrupt_row", "a deferred item has no until time")
                    })?;
                InboxState::Deferred {
                    until: Timestamp::from_nanos(nanos),
                }
            }
            "resolved" => InboxState::Resolved {
                decision: detail.unwrap_or_default(),
            },
            other => {
                return Err(AikitError::new(
                    "inbox.corrupt_row",
                    format!("`{other}` is not an inbox state"),
                ))
            }
        })
    }

    /// Whether this item still wants the user's attention at `now` (open, or a
    /// deferral that has elapsed).
    pub fn is_pending(&self, now: Timestamp) -> bool {
        match self {
            InboxState::Open => true,
            InboxState::Deferred { until } => *until <= now,
            InboxState::Resolved { .. } => false,
        }
    }
}

/// A pointer to something concrete — never raw transcript text. Structured so a UI
/// can render it and so it can be redacted like everything else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "evidence", rename_all = "kebab-case")]
pub enum Evidence {
    /// A file the user can open.
    File { path: String },
    /// A content hash and what it names.
    Hash { label: String, value: String },
    /// A short, already-redacted diff or content summary.
    Summary { text: String },
}

/// One message on the channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: InboxId,
    pub kind: InboxKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectId>,
    pub created: Timestamp,
    pub state: InboxState,
    pub title: String,
    /// Markdown, always redacted before storage.
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    /// A planned Procedure the user can just run. `None` until Procedures exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ProcedureId>,
}

/// The input to [`InboxChannel::publish`] — everything but the assigned id and
/// timestamp, which the channel mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub kind: InboxKind,
    pub title: String,
    pub body: String,
    pub project: Option<ProjectId>,
    pub evidence: Vec<Evidence>,
    pub proposal: Option<ProcedureId>,
    /// When set, publishing is idempotent: an existing pending item with the same
    /// key is returned unchanged rather than a near-duplicate being filed. The
    /// curator and drift detector use this so a re-run does not nag.
    pub dedup_key: Option<String>,
}

impl NewItem {
    pub fn new(kind: InboxKind, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            body: body.into(),
            project: None,
            evidence: Vec::new(),
            proposal: None,
            dedup_key: None,
        }
    }

    #[must_use]
    pub fn in_project(mut self, project: Option<ProjectId>) -> Self {
        self.project = project;
        self
    }

    #[must_use]
    pub fn with_evidence(mut self, evidence: Vec<Evidence>) -> Self {
        self.evidence = evidence;
        self
    }

    #[must_use]
    pub fn proposing(mut self, procedure: ProcedureId) -> Self {
        self.proposal = Some(procedure);
        self
    }

    #[must_use]
    pub fn deduped_by(mut self, key: impl Into<String>) -> Self {
        self.dedup_key = Some(key.into());
        self
    }
}

/// Read and write the inbox channel against the database.
pub struct InboxChannel<'a> {
    index: &'a Index,
    scanner: scan::Scanner,
}

const COLUMNS: &str =
    "inbox_id, kind, project, title, body, evidence, proposal, state_label, state_detail, created_ns";

impl<'a> InboxChannel<'a> {
    pub fn new(index: &'a Index) -> Self {
        Self {
            index,
            scanner: scan::Scanner::new(),
        }
    }

    /// Publish a message to the channel, redacting title, body and any evidence
    /// summaries first. Returns the stored item — or, when a `dedup_key` matches a
    /// pending item, that existing item unchanged (so a re-run does not nag).
    pub fn publish(&self, item: NewItem) -> Result<InboxItem> {
        if let Some(key) = &item.dedup_key {
            if let Some(existing) = self.pending_with_dedup(key)? {
                return Ok(existing);
            }
        }

        // Redaction is unconditional and therefore exhaustive: this match names
        // every variant rather than falling through, so adding an `Evidence`
        // variant fails to compile until its redaction is decided. A path or a
        // label is as capable of carrying a token as a summary is — the earlier
        // catch-all `other => other.clone()` let exactly that through.
        let redacted_evidence: Vec<Evidence> = item
            .evidence
            .iter()
            .map(|e| match e {
                Evidence::Summary { text } => Evidence::Summary {
                    text: self.scanner.redact(text),
                },
                Evidence::File { path } => Evidence::File {
                    path: self.scanner.redact(path),
                },
                Evidence::Hash { label, value } => Evidence::Hash {
                    label: self.scanner.redact(label),
                    value: self.scanner.redact(value),
                },
            })
            .collect();

        let stored = InboxItem {
            id: InboxId::generate(),
            kind: item.kind,
            project: item.project,
            created: Timestamp::now(),
            state: InboxState::Open,
            title: self.scanner.redact(&item.title),
            body: self.scanner.redact(&item.body),
            evidence: redacted_evidence,
            proposal: item.proposal,
        };

        let evidence_json = serde_json::to_string(&stored.evidence)
            .map_err(|e| AikitError::new("inbox.unserializable", format!("evidence: {e}")))?;

        self.index
            .conn()
            .execute(
                "INSERT INTO inbox_items \
                 (inbox_id, kind, project, title, body, evidence, proposal, dedup_key, \
                  state_label, state_detail, created_ns) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    stored.id.as_str(),
                    stored.kind.as_str(),
                    stored.project.as_ref().map(|p| p.as_str()),
                    stored.title,
                    stored.body,
                    evidence_json,
                    stored.proposal.as_ref().map(|p| p.as_str()),
                    item.dedup_key,
                    stored.state.label(),
                    stored.state.detail(),
                    stored.created.as_nanos(),
                ],
            )
            .map_err(|e| sql_error("inbox.write_failed", &e))?;

        Ok(stored)
    }

    /// Every item, newest first.
    pub fn items(&self) -> Result<Vec<InboxItem>> {
        self.query(
            "SELECT {C} FROM inbox_items ORDER BY created_ns DESC, inbox_id",
            params![],
        )
    }

    /// Only the items still wanting attention at `now` (open, or a deferral that
    /// has elapsed), newest first.
    pub fn pending(&self, now: Timestamp) -> Result<Vec<InboxItem>> {
        Ok(self
            .items()?
            .into_iter()
            .filter(|i| i.state.is_pending(now))
            .collect())
    }

    pub fn get(&self, id: &InboxId) -> Result<Option<InboxItem>> {
        Ok(self
            .query(
                "SELECT {C} FROM inbox_items WHERE inbox_id = ?1",
                params![id.as_str()],
            )?
            .into_iter()
            .next())
    }

    /// Record the user's decision on an item, keeping it for audit.
    pub fn resolve(&self, id: &InboxId, decision: &str) -> Result<()> {
        self.set_state(
            id,
            &InboxState::Resolved {
                decision: decision.to_string(),
            },
        )
    }

    /// Snooze an item until a time.
    pub fn defer(&self, id: &InboxId, until: Timestamp) -> Result<()> {
        self.set_state(id, &InboxState::Deferred { until })
    }

    fn set_state(&self, id: &InboxId, state: &InboxState) -> Result<()> {
        let changed = self
            .index
            .conn()
            .execute(
                "UPDATE inbox_items SET state_label = ?1, state_detail = ?2 WHERE inbox_id = ?3",
                params![state.label(), state.detail(), id.as_str()],
            )
            .map_err(|e| sql_error("inbox.write_failed", &e))?;
        if changed == 0 {
            return Err(
                AikitError::new("inbox.unknown_item", format!("no inbox item {id}"))
                    .with("inbox_id", id.to_string()),
            );
        }
        Ok(())
    }

    /// The pending item with a given dedup key, if any (used for idempotency).
    fn pending_with_dedup(&self, key: &str) -> Result<Option<InboxItem>> {
        let items = self.query(
            "SELECT {C} FROM inbox_items WHERE dedup_key = ?1 ORDER BY created_ns DESC",
            params![key],
        )?;
        let now = Timestamp::now();
        Ok(items.into_iter().find(|i| i.state.is_pending(now)))
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<InboxItem>> {
        let sql = sql.replace("{C}", COLUMNS);
        let mut stmt = self
            .index
            .conn()
            .prepare(&sql)
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        let rows = stmt
            .query_map(params, |row| Ok(item_from_row(row)))
            .map_err(|e| sql_error("inbox.query_failed", &e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| sql_error("inbox.query_failed", &e))??);
        }
        Ok(out)
    }
}

/// Map one SQLite row to an `InboxItem`. Doubly-wrapped like the rest of the crate.
fn item_from_row(row: &rusqlite::Row) -> Result<InboxItem> {
    let id = InboxId::parse(&row_string(row, 0)?)?;
    let kind = InboxKind::parse(&row_string(row, 1)?)?;
    let project = row
        .get::<_, Option<String>>(2)
        .map_err(row_err)?
        .map(|p| ProjectId::parse(&p))
        .transpose()?;
    let title = row_string(row, 3)?;
    let body = row_string(row, 4)?;
    let evidence: Vec<Evidence> = serde_json::from_str(&row_string(row, 5)?)
        .map_err(|e| AikitError::new("inbox.corrupt_row", format!("evidence: {e}")))?;
    let proposal = row
        .get::<_, Option<String>>(6)
        .map_err(row_err)?
        .map(|p| ProcedureId::parse(&p))
        .transpose()?;
    let state = InboxState::from_parts(
        &row_string(row, 7)?,
        row.get::<_, Option<String>>(8).map_err(row_err)?,
    )?;
    let created = Timestamp::from_nanos(row.get::<_, i64>(9).map_err(row_err)?);

    Ok(InboxItem {
        id,
        kind,
        project,
        created,
        state,
        title,
        body,
        evidence,
        proposal,
    })
}

fn row_string(row: &rusqlite::Row, idx: usize) -> Result<String> {
    row.get::<_, String>(idx).map_err(row_err)
}

fn row_err(e: rusqlite::Error) -> AikitError {
    sql_error("inbox.corrupt_row", &e)
}
