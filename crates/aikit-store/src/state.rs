//! Runtime state: sessions, contexts, bindings and the promotion queue.
//!
//! ## Multiplexer ids are bindings, not identity
//!
//! A tmux server restart renumbers every session and pane. A restored cmux
//! workspace comes back with a new id. If AIKit keyed a session's overlay on
//! those, every restart would silently orphan the user's session-scoped
//! capabilities — the session would look new, resolve to the project baseline,
//! and the toggles made an hour ago would be gone with no error to explain it.
//!
//! So [`SessionRecord::session_id`] is authoritative and the mux id is an
//! attribute. [`StateStore::rebind_session`] re-attaches a restored session by
//! the pair that *is* durable — the human-chosen name and the project marker —
//! and refuses to match across projects or across multiplexers, because
//! "payments" is not an unusual session name and adopting the wrong overlay would
//! be worse than starting fresh.
//!
//! ## Staleness is an observation, not a decision
//!
//! [`StateStore::stale_sessions`] reports; it never deletes. Whether an abandoned
//! session should be torn down depends on things this layer cannot see — a dirty
//! worktree, an unpushed branch, an open pull request — and that judgement belongs
//! to the user.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use rusqlite::{params, OptionalExtension};

use aikit_core::error::err;
use aikit_core::{
    ContextBinding, ContextId, Isolation, MuxKind, ProjectId, Result, SessionId,
};

use crate::events::Timestamp;
use crate::index::{sql_error, Index};

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// What AIKit believes about a session space right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Attached or otherwise observed recently.
    Live,
    /// Known to exist, nobody attached.
    Detached,
    /// Deliberately ended.
    Closed,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Live => "live",
            SessionState::Detached => "detached",
            SessionState::Closed => "closed",
        }
    }

    /// A closed session is not a candidate for staleness: the user ended it.
    pub fn can_go_stale(self) -> bool {
        matches!(self, SessionState::Live | SessionState::Detached)
    }
}

impl FromStr for SessionState {
    type Err = aikit_core::AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "live" => SessionState::Live,
            "detached" => SessionState::Detached,
            "closed" => SessionState::Closed,
            other => {
                return err(
                    "state.unknown_session_state",
                    format!("`{other}` is not a session state"),
                )
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub session_id: SessionId,
    /// The human-chosen name. Half of the durable rebinding key.
    pub name: String,
    pub project_root: Option<PathBuf>,
    /// The durable project marker — deliberately not a path, because worktrees
    /// and checkouts move.
    pub project_marker: Option<ProjectId>,
    pub mux: MuxKind,
    /// The multiplexer's own id. Rebindable; never identity.
    pub mux_session: Option<String>,
    pub state: SessionState,
    pub created_at: Timestamp,
    pub last_seen: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRecord {
    pub context_id: ContextId,
    pub session_id: Option<SessionId>,
    pub project_id: Option<ProjectId>,
    pub project_root: Option<PathBuf>,
    pub task: Option<String>,
    /// Recorded, never inferred: a shared task must not later be mistaken for an
    /// isolated one by anything reading this row.
    pub isolation: Isolation,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A capture candidate waiting for a human decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPromotion {
    pub candidate_id: String,
    pub reason: String,
    pub queued_at: Timestamp,
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

pub struct StateStore<'a> {
    index: &'a Index,
}

impl<'a> StateStore<'a> {
    pub fn new(index: &'a Index) -> Self {
        Self { index }
    }

    // -- sessions ----------------------------------------------------------

    pub fn put_session(&self, record: &SessionRecord) -> Result<()> {
        self.index
            .conn()
            .execute(
                "INSERT INTO sessions
                     (session_id, name, project_root, project_marker, mux, mux_session, state,
                      created_ns, last_seen_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(session_id) DO UPDATE SET
                     name = excluded.name,
                     project_root = excluded.project_root,
                     project_marker = excluded.project_marker,
                     mux = excluded.mux,
                     mux_session = excluded.mux_session,
                     state = excluded.state,
                     last_seen_ns = excluded.last_seen_ns",
                params![
                    record.session_id.as_str(),
                    record.name,
                    record.project_root.as_ref().map(|p| p.display().to_string()),
                    record.project_marker.as_ref().map(|p| p.as_str()),
                    record.mux.as_str(),
                    record.mux_session,
                    record.state.as_str(),
                    record.created_at.as_nanos(),
                    record.last_seen.as_nanos(),
                ],
            )
            .map_err(|e| sql_error("state.write_failed", &e))?;
        Ok(())
    }

    pub fn session(&self, id: &SessionId) -> Result<Option<SessionRecord>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!("{SESSION_COLUMNS} WHERE session_id = ?1"))
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let row = stmt
            .query_row(params![id.as_str()], session_row)
            .optional()
            .map_err(|e| sql_error("state.query_failed", &e))?;
        row.transpose()
    }

    pub fn sessions(&self) -> Result<Vec<SessionRecord>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!("{SESSION_COLUMNS} ORDER BY created_ns, session_id"))
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let rows = stmt
            .query_map([], session_row)
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| sql_error("state.query_failed", &e))??);
        }
        Ok(out)
    }

    /// Re-attach a restored session whose multiplexer id has changed.
    ///
    /// Matches on `(name, project marker, mux)` — the three things that survive a
    /// server restart — and returns the updated record, or `None` when this really
    /// is a session AIKit has not seen. Returning `None` rather than creating one
    /// is deliberate: inventing a record here would hide the fact that the
    /// overlay is gone.
    pub fn rebind_session(
        &self,
        name: &str,
        project_marker: Option<&ProjectId>,
        mux: MuxKind,
        mux_session: Option<&str>,
    ) -> Result<Option<SessionRecord>> {
        let candidates: Vec<SessionRecord> = self
            .sessions()?
            .into_iter()
            .filter(|s| {
                s.name == name
                    && s.mux == mux
                    && s.project_marker.as_ref() == project_marker
                    && s.state != SessionState::Closed
            })
            .collect();

        // Ambiguity is not something to guess at: two live sessions with the same
        // name in the same project would each have their own overlay, and picking
        // one at random would hand the user somebody else's capabilities.
        let Some(mut record) = candidates.into_iter().next_back() else {
            return Ok(None);
        };

        record.mux_session = mux_session.map(ToString::to_string);
        record.state = SessionState::Live;
        record.last_seen = Timestamp::now();
        self.put_session(&record)?;
        Ok(Some(record))
    }

    /// Record that a session was just observed.
    pub fn touch_session(&self, id: &SessionId, at: Timestamp) -> Result<()> {
        self.index
            .conn()
            .execute(
                "UPDATE sessions SET last_seen_ns = ?1, state = 'live' WHERE session_id = ?2",
                params![at.as_nanos(), id.as_str()],
            )
            .map_err(|e| sql_error("state.write_failed", &e))?;
        Ok(())
    }

    pub fn close_session(&self, id: &SessionId) -> Result<()> {
        self.index
            .conn()
            .execute(
                "UPDATE sessions SET state = 'closed' WHERE session_id = ?1",
                params![id.as_str()],
            )
            .map_err(|e| sql_error("state.write_failed", &e))?;
        Ok(())
    }

    /// Sessions not seen for `idle_for`. Reports only; never deletes.
    pub fn stale_sessions(&self, idle_for: Duration, now: Timestamp) -> Result<Vec<SessionRecord>> {
        let cutoff = now
            .as_nanos()
            .saturating_sub(idle_for.as_nanos().min(i64::MAX as u128) as i64);
        Ok(self
            .sessions()?
            .into_iter()
            .filter(|s| s.state.can_go_stale() && s.last_seen.as_nanos() < cutoff)
            .collect())
    }

    // -- contexts ----------------------------------------------------------

    pub fn put_context(&self, record: &ContextRecord) -> Result<()> {
        self.index
            .conn()
            .execute(
                "INSERT INTO contexts
                     (context_id, session_id, project_id, project_root, task, isolation,
                      created_ns, updated_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(context_id) DO UPDATE SET
                     session_id = excluded.session_id,
                     project_id = excluded.project_id,
                     project_root = excluded.project_root,
                     task = excluded.task,
                     isolation = excluded.isolation,
                     updated_ns = excluded.updated_ns",
                params![
                    record.context_id.as_str(),
                    record.session_id.as_ref().map(|s| s.as_str()),
                    record.project_id.as_ref().map(|p| p.as_str()),
                    record.project_root.as_ref().map(|p| p.display().to_string()),
                    record.task,
                    record.isolation.as_str(),
                    record.created_at.as_nanos(),
                    record.updated_at.as_nanos(),
                ],
            )
            .map_err(|e| sql_error("state.write_failed", &e))?;
        Ok(())
    }

    pub fn context(&self, id: &ContextId) -> Result<Option<ContextRecord>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!("{CONTEXT_COLUMNS} WHERE context_id = ?1"))
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let row = stmt
            .query_row(params![id.as_str()], context_row)
            .optional()
            .map_err(|e| sql_error("state.query_failed", &e))?;
        row.transpose()
    }

    pub fn contexts(&self) -> Result<Vec<ContextRecord>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(&format!("{CONTEXT_COLUMNS} ORDER BY created_ns, context_id"))
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let rows = stmt
            .query_map([], context_row)
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| sql_error("state.query_failed", &e))??);
        }
        Ok(out)
    }

    /// Every context a session owns: one per project/worktree/task overlay.
    pub fn contexts_of_session(&self, session: &SessionId) -> Result<Vec<ContextRecord>> {
        Ok(self
            .contexts()?
            .into_iter()
            .filter(|c| c.session_id.as_ref() == Some(session))
            .collect())
    }

    // -- bindings ----------------------------------------------------------

    pub fn bind_context(&self, binding: &ContextBinding) -> Result<()> {
        self.index.put_binding(binding)
    }

    pub fn bindings(&self) -> Result<Vec<ContextBinding>> {
        self.index.bindings()
    }

    pub fn binding(&self, context: &ContextId) -> Result<Option<ContextBinding>> {
        self.index.binding(context)
    }

    // -- promotion queue ---------------------------------------------------

    pub fn queue_promotion(&self, candidate_id: &str, reason: &str) -> Result<()> {
        self.index
            .conn()
            .execute(
                "INSERT INTO promotion_queue (candidate_id, reason, queued_ns)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(candidate_id) DO UPDATE SET
                     reason = excluded.reason,
                     queued_ns = excluded.queued_ns",
                params![candidate_id, reason, Timestamp::now().as_nanos()],
            )
            .map_err(|e| sql_error("state.write_failed", &e))?;
        Ok(())
    }

    pub fn promotion_queue(&self) -> Result<Vec<QueuedPromotion>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(
                "SELECT candidate_id, reason, queued_ns FROM promotion_queue
                 ORDER BY queued_ns, candidate_id",
            )
            .map_err(|e| sql_error("state.query_failed", &e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(QueuedPromotion {
                    candidate_id: r.get(0)?,
                    reason: r.get(1)?,
                    queued_at: Timestamp::from_nanos(r.get(2)?),
                })
            })
            .map_err(|e| sql_error("state.query_failed", &e))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| sql_error("state.query_failed", &e))
    }

    pub fn dequeue_promotion(&self, candidate_id: &str) -> Result<()> {
        self.index
            .conn()
            .execute(
                "DELETE FROM promotion_queue WHERE candidate_id = ?1",
                params![candidate_id],
            )
            .map_err(|e| sql_error("state.write_failed", &e))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

const SESSION_COLUMNS: &str = "SELECT session_id, name, project_root, project_marker, mux, \
                               mux_session, state, created_ns, last_seen_ns FROM sessions";

const CONTEXT_COLUMNS: &str = "SELECT context_id, session_id, project_id, project_root, task, \
                               isolation, created_ns, updated_ns FROM contexts";

fn session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<SessionRecord>> {
    let session_id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let project_root: Option<String> = row.get(2)?;
    let project_marker: Option<String> = row.get(3)?;
    let mux: String = row.get(4)?;
    let mux_session: Option<String> = row.get(5)?;
    let state: String = row.get(6)?;
    let created: i64 = row.get(7)?;
    let last_seen: i64 = row.get(8)?;

    Ok((|| {
        Ok(SessionRecord {
            session_id: SessionId::parse(&session_id)?,
            name,
            project_root: project_root.map(PathBuf::from),
            project_marker: project_marker.as_deref().map(ProjectId::parse).transpose()?,
            mux: MuxKind::from_str(&mux)?,
            mux_session,
            state: SessionState::from_str(&state)?,
            created_at: Timestamp::from_nanos(created),
            last_seen: Timestamp::from_nanos(last_seen),
        })
    })())
}

fn context_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ContextRecord>> {
    let context_id: String = row.get(0)?;
    let session_id: Option<String> = row.get(1)?;
    let project_id: Option<String> = row.get(2)?;
    let project_root: Option<String> = row.get(3)?;
    let task: Option<String> = row.get(4)?;
    let isolation: String = row.get(5)?;
    let created: i64 = row.get(6)?;
    let updated: i64 = row.get(7)?;

    Ok((|| {
        Ok(ContextRecord {
            context_id: ContextId::parse(&context_id)?,
            session_id: session_id.as_deref().map(SessionId::parse).transpose()?,
            project_id: project_id.as_deref().map(ProjectId::parse).transpose()?,
            project_root: project_root.map(PathBuf::from),
            task,
            isolation: match isolation.as_str() {
                "directory" => Isolation::Directory,
                "worktree" => Isolation::Worktree,
                // Shared is the default and the fallback: a row we cannot read
                // must never be taken to mean "this task has its own tree".
                _ => Isolation::Shared,
            },
            created_at: Timestamp::from_nanos(created),
            updated_at: Timestamp::from_nanos(updated),
        })
    })())
}
