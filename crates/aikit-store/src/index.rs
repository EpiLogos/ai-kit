//! The SQLite index.
//!
//! ## Derived versus operational
//!
//! `ARCHITECTURE.md` §5 divides this database in two, and [`Index::reindex`] is
//! where the division is enforced:
//!
//! * **Derived** — `capsules`, `profiles`. Every byte of these can be
//!   reconstructed by walking the registries, so `reindex` deletes and rewrites
//!   them wholesale. That is what makes "delete the database and carry on" a true
//!   statement rather than an aspiration.
//! * **Operational** — `usage_events`, `contexts`, `context_bindings`,
//!   `generations`, `candidates`, `trust`, `bypasses`. These exist nowhere else.
//!   A `reindex` that dropped them would turn routine maintenance into data loss:
//!   the ranking history, the live tmux bindings and the record of who reviewed
//!   what would all be gone, and nothing on disk could bring them back.
//!
//! ## Why WAL
//!
//! There is no daemon. Every AIKit command is a fresh short-lived process, and
//! several of them — a palette repainting, a hook dispatcher, an `apply` — are
//! routinely alive at once. WAL is what lets the reader in the palette keep
//! reading while the dispatcher writes.
//!
//! ## Why migrations rather than "create if not exists"
//!
//! `schema_version` records each applied step. A future release that needs to add
//! a column has somewhere to put the statement, and an older binary meeting a
//! newer database gets a clear refusal instead of a confusing `no such column`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};

use aikit_core::catalog::Catalog;
use aikit_core::error::err;
use aikit_core::{
    AikitError, BypassScope, BypassToken, CapsuleId, ContextBinding, ContextId, Isolation, Kind,
    Maturity, MuxKind, ProfileId, RegistrySource, Result, Revision, SessionId, UsageStats,
};

use crate::events::{Event, EventAction, Outcome, Timestamp};
use crate::registry::RegistryLoad;

/// Schema steps, applied in order. Append only — never edit one that shipped.
const MIGRATIONS: &[(&str, &str)] = &[(
    "0001-initial",
    r#"
CREATE TABLE capsules (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL,
    tags         TEXT NOT NULL,
    maturity     TEXT NOT NULL,
    revision     TEXT NOT NULL,
    source       TEXT NOT NULL,
    exports      TEXT NOT NULL,
    root         TEXT,
    updated_at   INTEGER NOT NULL
);
CREATE INDEX capsules_by_kind ON capsules(kind);
CREATE INDEX capsules_by_source ON capsules(source);

CREATE TABLE profiles (
    id           TEXT PRIMARY KEY,
    description  TEXT NOT NULL,
    extends      TEXT NOT NULL,
    source       TEXT NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE usage_events (
    event_id      TEXT PRIMARY KEY,
    timestamp_ns  INTEGER NOT NULL,
    timestamp     TEXT NOT NULL,
    action        TEXT NOT NULL,
    session_id    TEXT,
    context_id    TEXT,
    project_id    TEXT,
    capsule_id    TEXT,
    revision      TEXT,
    capsule_kind  TEXT,
    client        TEXT,
    mux           TEXT,
    scope         TEXT,
    outcome       TEXT NOT NULL,
    outcome_detail TEXT,
    duration_ms   INTEGER,
    generation    TEXT,
    bypass_reason TEXT,
    parent_event  TEXT,
    arguments     TEXT NOT NULL
);
CREATE INDEX usage_events_by_capsule ON usage_events(capsule_id, timestamp_ns);

CREATE TABLE contexts (
    context_id   TEXT PRIMARY KEY,
    session_id   TEXT,
    project_id   TEXT,
    project_root TEXT,
    task         TEXT,
    isolation    TEXT NOT NULL,
    created_ns   INTEGER NOT NULL,
    updated_ns   INTEGER NOT NULL
);

CREATE TABLE context_bindings (
    context_id   TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL,
    mux          TEXT NOT NULL,
    mux_session  TEXT,
    mux_surface  TEXT,
    project_root TEXT,
    isolation    TEXT NOT NULL,
    bound_ns     INTEGER NOT NULL
);

CREATE TABLE generations (
    generation_id TEXT NOT NULL,
    context_id    TEXT NOT NULL,
    resolution_hash TEXT NOT NULL,
    base_generation TEXT,
    created_ns    INTEGER NOT NULL,
    PRIMARY KEY (context_id, generation_id)
);

CREATE TABLE candidates (
    candidate_id  TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,
    title         TEXT NOT NULL,
    body_hash     TEXT NOT NULL,
    normalized_hash TEXT NOT NULL,
    state         TEXT NOT NULL,
    quarantined   INTEGER NOT NULL,
    project_root  TEXT,
    path          TEXT NOT NULL,
    created_ns    INTEGER NOT NULL
);

CREATE TABLE trust (
    source        TEXT NOT NULL,
    capsule_id    TEXT NOT NULL,
    revision      TEXT NOT NULL,
    state         TEXT NOT NULL,
    note          TEXT,
    reviewed_ns   INTEGER NOT NULL,
    PRIMARY KEY (source, capsule_id, revision)
);

CREATE TABLE bypasses (
    bypass_id     TEXT PRIMARY KEY,
    context_id    TEXT NOT NULL,
    scope         TEXT NOT NULL,
    capsule_id    TEXT,
    reason        TEXT,
    issued_ns     INTEGER NOT NULL,
    expires_ns    INTEGER,
    spent         INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX bypasses_by_context ON bypasses(context_id);

CREATE TABLE sessions (
    session_id    TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    project_root  TEXT,
    project_marker TEXT,
    mux           TEXT NOT NULL,
    mux_session   TEXT,
    state         TEXT NOT NULL,
    created_ns    INTEGER NOT NULL,
    last_seen_ns  INTEGER NOT NULL
);
CREATE INDEX sessions_by_name ON sessions(name);

CREATE TABLE promotion_queue (
    candidate_id  TEXT PRIMARY KEY,
    reason        TEXT NOT NULL,
    queued_ns     INTEGER NOT NULL
);
"#,
),
// The Inbox as the system's own channel (Spec II §2): drift notices, version
// conflicts, procedure reports and agent proposals the system addresses to the
// user. Operational — these records exist nowhere else — so it must never enter
// DERIVED_TABLES or a reindex would silently discard the system's messages.
(
    "0002-inbox-items",
    r#"
CREATE TABLE inbox_items (
    inbox_id     TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    project      TEXT,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    evidence     TEXT NOT NULL,
    proposal     TEXT,
    dedup_key    TEXT,
    state_label  TEXT NOT NULL,
    state_detail TEXT,
    created_ns   INTEGER NOT NULL
);
CREATE INDEX inbox_items_by_state ON inbox_items(state_label);
CREATE INDEX inbox_items_by_dedup ON inbox_items(dedup_key);
"#,
)];

/// Tables `reindex` is allowed to empty.
const DERIVED_TABLES: &[&str] = &["capsules", "profiles"];

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// An indexed capsule: everything the palette needs without opening a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsuleRow {
    pub id: CapsuleId,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub maturity: Maturity,
    pub revision: Revision,
    pub source: RegistrySource,
    pub exports: Vec<String>,
    pub root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRow {
    pub id: ProfileId,
    pub description: String,
    pub extends: Vec<ProfileId>,
    pub source: RegistrySource,
}

/// Counts by the dimensions the palette can filter on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    pub kinds: BTreeMap<Kind, u32>,
    pub tags: BTreeMap<String, u32>,
    pub sources: BTreeMap<String, u32>,
    pub maturities: BTreeMap<Maturity, u32>,
}

/// A narrowing filter. Different keys narrow (AND); values within `tags` widen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapsuleFilter {
    pub kinds: Vec<Kind>,
    pub tags: Vec<String>,
    pub sources: Vec<RegistrySource>,
    pub text: Option<String>,
}

impl CapsuleFilter {
    #[must_use]
    pub fn with_kind(mut self, kind: Kind) -> Self {
        self.kinds.push(kind);
        self
    }

    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: RegistrySource) -> Self {
        self.sources.push(source);
        self
    }

    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    fn admits(&self, row: &CapsuleRow) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&row.kind) {
            return false;
        }
        if !self.tags.is_empty() && !self.tags.iter().any(|t| row.tags.contains(t)) {
            return false;
        }
        if !self.sources.is_empty() && !self.sources.contains(&row.source) {
            return false;
        }
        if let Some(text) = &self.text {
            let needle = text.to_lowercase();
            let haystack = [
                row.id.to_string(),
                row.name.clone(),
                row.description.clone(),
                row.tags.join(" "),
                row.exports.join(" "),
            ]
            .join("\n")
            .to_lowercase();
            if !haystack.contains(&needle) {
                return false;
            }
        }
        true
    }
}

/// What a reindex changed, for `aikit sync --json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReindexReport {
    pub capsules: usize,
    pub profiles: usize,
    pub problems: usize,
}

/// A bypass token as recorded, with the identity needed to spend or revoke it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BypassRecord {
    pub bypass_id: String,
    pub context_id: ContextId,
    pub token: BypassToken,
    pub issued_at: Timestamp,
    pub spent: bool,
}

// ---------------------------------------------------------------------------
// The index
// ---------------------------------------------------------------------------

pub struct Index {
    conn: Connection,
    path: PathBuf,
}

impl Index {
    /// Open (creating if needed) the database at `path`, in WAL mode, migrated.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            crate::home::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(|e| sql_error("index.open_failed", &e))?;

        // WAL is a persistent property of the file, but setting it on every open
        // costs nothing and means a database restored from a non-WAL backup is
        // fixed rather than silently slower and lock-prone.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| sql_error("index.open_failed", &e))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| sql_error("index.open_failed", &e))?;
        // Every command is a separate process; a few hundred milliseconds of
        // patience beats surfacing `database is locked` to a user.
        conn.busy_timeout(Duration::from_millis(5_000))
            .map_err(|e| sql_error("index.open_failed", &e))?;

        let index = Self {
            conn,
            path: path.to_path_buf(),
        };
        index.migrate()?;
        Ok(index)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (
                     version    INTEGER PRIMARY KEY,
                     name       TEXT NOT NULL,
                     applied_ns INTEGER NOT NULL
                 );",
            )
            .map_err(|e| sql_error("index.migrate_failed", &e))?;

        let applied: u32 = self
            .conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| {
                r.get(0)
            })
            .map_err(|e| sql_error("index.migrate_failed", &e))?;

        if applied as usize > MIGRATIONS.len() {
            return err(
                "index.schema_from_the_future",
                format!(
                    "this database is at schema {applied} but this build only knows {}; \
                     upgrade AIKit rather than letting an older binary write to it",
                    MIGRATIONS.len()
                ),
            );
        }

        for (offset, (name, sql)) in MIGRATIONS.iter().enumerate() {
            let version = offset as u32 + 1;
            if version <= applied {
                continue;
            }
            self.conn
                .execute_batch(sql)
                .map_err(|e| sql_error("index.migrate_failed", &e))?;
            self.conn
                .execute(
                    "INSERT INTO schema_version (version, name, applied_ns) VALUES (?1, ?2, ?3)",
                    params![version, name, Timestamp::now().as_nanos()],
                )
                .map_err(|e| sql_error("index.migrate_failed", &e))?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        self.conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| {
                r.get(0)
            })
            .map_err(|e| sql_error("index.query_failed", &e))
    }

    pub fn applied_migrations(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM schema_version ORDER BY version")
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| sql_error("index.query_failed", &e))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| sql_error("index.query_failed", &e))
    }

    pub fn journal_mode(&self) -> Result<String> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(|e| sql_error("index.query_failed", &e))
    }

    pub fn tables(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| sql_error("index.query_failed", &e))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| sql_error("index.query_failed", &e))
    }

    // -- derived tables ----------------------------------------------------

    /// Rebuild the derived tables from a registry load.
    ///
    /// Wholesale replacement inside one transaction, so a reader either sees the
    /// old catalog or the new one and never a half-written mixture. Operational
    /// tables are untouched by construction: this only ever names
    /// [`DERIVED_TABLES`].
    pub fn reindex(&mut self, load: &RegistryLoad) -> Result<ReindexReport> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| sql_error("index.write_failed", &e))?;
        let now = Timestamp::now().as_nanos();

        for table in DERIVED_TABLES {
            tx.execute(&format!("DELETE FROM {table}"), [])
                .map_err(|e| sql_error("index.write_failed", &e))?;
        }

        let capsules = load.catalog.capsules();
        for capsule in &capsules {
            let source = capsule
                .source
                .clone()
                .unwrap_or_else(RegistrySource::personal);
            let revision = capsule
                .revision
                .clone()
                .unwrap_or_else(|| Revision::from_raw(""));
            tx.execute(
                "INSERT INTO capsules
                   (id, kind, name, description, tags, maturity, revision, source, exports, root, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    capsule.id.to_string(),
                    capsule.kind.as_str(),
                    capsule.name,
                    capsule.description,
                    join(&capsule.tags),
                    capsule.maturity.as_str(),
                    revision.as_str(),
                    source.as_str(),
                    join(&capsule.exported_commands()),
                    capsule.root.as_ref().map(|p| p.display().to_string()),
                    now,
                ],
            )
            .map_err(|e| sql_error("index.write_failed", &e))?;
        }

        let profiles = load.catalog.profiles();
        for profile in &profiles {
            tx.execute(
                "INSERT INTO profiles (id, description, extends, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    profile.id.to_string(),
                    profile.description,
                    join(&profile
                        .extends
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()),
                    RegistrySource::PERSONAL,
                    now,
                ],
            )
            .map_err(|e| sql_error("index.write_failed", &e))?;
        }

        let report = ReindexReport {
            capsules: capsules.len(),
            profiles: profiles.len(),
            problems: load.problems.len(),
        };
        tx.commit().map_err(|e| sql_error("index.write_failed", &e))?;
        Ok(report)
    }

    pub fn capsules(&self) -> Result<Vec<CapsuleRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, name, description, tags, maturity, revision, source, exports, root
                 FROM capsules ORDER BY id",
            )
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map([], capsule_row)
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| sql_error("index.query_failed", &e))??);
        }
        Ok(out)
    }

    pub fn capsule(&self, id: &CapsuleId) -> Result<Option<CapsuleRow>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, name, description, tags, maturity, revision, source, exports, root
                 FROM capsules WHERE id = ?1",
            )
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let row = stmt
            .query_row(params![id.to_string()], capsule_row)
            .optional()
            .map_err(|e| sql_error("index.query_failed", &e))?;
        row.transpose()
    }

    pub fn profiles(&self) -> Result<Vec<ProfileRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, description, extends, source FROM profiles ORDER BY id")
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| sql_error("index.query_failed", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let (id, description, extends, source) =
                row.map_err(|e| sql_error("index.query_failed", &e))?;
            out.push(ProfileRow {
                id: ProfileId::parse(&id)?,
                description,
                extends: split(&extends)
                    .iter()
                    .map(|s| ProfileId::parse(s))
                    .collect::<Result<Vec<_>>>()?,
                source: RegistrySource::new(source),
            });
        }
        Ok(out)
    }

    pub fn find(&self, filter: &CapsuleFilter) -> Result<Vec<CapsuleRow>> {
        Ok(self
            .capsules()?
            .into_iter()
            .filter(|row| filter.admits(row))
            .collect())
    }

    pub fn facets(&self) -> Result<Facets> {
        let mut facets = Facets::default();
        for row in self.capsules()? {
            *facets.kinds.entry(row.kind).or_insert(0) += 1;
            *facets.maturities.entry(row.maturity).or_insert(0) += 1;
            *facets
                .sources
                .entry(row.source.as_str().to_string())
                .or_insert(0) += 1;
            for tag in row.tags {
                *facets.tags.entry(tag).or_insert(0) += 1;
            }
        }
        Ok(facets)
    }

    // -- events ------------------------------------------------------------

    pub fn record_event(&self, event: &Event) -> Result<()> {
        let arguments = serde_json::to_string(&event.arguments).map_err(|e| {
            AikitError::new(
                "event.serialize_failed",
                format!("could not encode event arguments: {e}"),
            )
        })?;
        self.conn
            .execute(
                "INSERT INTO usage_events (
                     event_id, timestamp_ns, timestamp, action, session_id, context_id, project_id,
                     capsule_id, revision, capsule_kind, client, mux, scope, outcome,
                     outcome_detail, duration_ms, generation, bypass_reason, parent_event, arguments)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, ?19, ?20)",
                params![
                    event.event_id.as_str(),
                    event.timestamp.as_nanos(),
                    event.timestamp.to_string(),
                    event.action.as_str(),
                    event.session.as_ref().map(|s| s.as_str()),
                    event.context.as_ref().map(|c| c.as_str()),
                    event.project.as_ref().map(|p| p.as_str()),
                    event.capsule.as_ref().map(ToString::to_string),
                    event.revision.as_ref().map(|r| r.as_str()),
                    event.kind.map(|k| k.as_str()),
                    event.client.as_ref().map(|c| c.as_str()),
                    event.mux.map(|m| m.as_str()),
                    event.scope.map(|s| s.as_str()),
                    event.outcome.label(),
                    event.outcome.detail(),
                    event.duration_ms,
                    event.generation.as_ref().map(|g| g.as_str()),
                    event.bypass_reason,
                    event.parent_event.as_ref().map(|p| p.as_str()),
                    arguments,
                ],
            )
            .map_err(|e| sql_error("index.write_failed", &e))?;
        Ok(())
    }

    pub fn event_count(&self) -> Result<u32> {
        self.conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
            .map_err(|e| sql_error("index.query_failed", &e))
    }

    /// The most recent events, newest first.
    pub fn recent_events(&self, limit: u32) -> Result<Vec<EventSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT event_id, timestamp_ns, action, capsule_id, outcome, outcome_detail,
                        bypass_reason
                 FROM usage_events ORDER BY timestamp_ns DESC, event_id DESC LIMIT ?1",
            )
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|e| sql_error("index.query_failed", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let (id, ns, action, capsule, outcome, detail, bypass) =
                row.map_err(|e| sql_error("index.query_failed", &e))?;
            out.push(EventSummary {
                event_id: id,
                timestamp: Timestamp::from_nanos(ns),
                action: EventAction::from_str(&action)?,
                capsule: capsule.as_deref().map(CapsuleId::parse).transpose()?,
                outcome: Outcome::from_parts(&outcome, detail),
                bypass_reason: bypass,
            });
        }
        Ok(out)
    }

    /// Ranking facts for one capsule. Only successful runs carry a boost, so the
    /// two counters are kept apart rather than summed.
    pub fn usage(&self, id: &CapsuleId) -> Result<UsageStats> {
        self.usage_at(id, Timestamp::now())
    }

    pub fn usage_at(&self, id: &CapsuleId, now: Timestamp) -> Result<UsageStats> {
        let (successes, failures): (u32, u32) = self
            .conn
            .query_row(
                "SELECT
                     COALESCE(SUM(CASE WHEN outcome = 'success' THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN outcome != 'success' THEN 1 ELSE 0 END), 0)
                 FROM usage_events WHERE capsule_id = ?1 AND action = 'run'",
                params![id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| sql_error("index.query_failed", &e))?;

        let last: Option<i64> = self
            .conn
            .query_row(
                "SELECT MAX(timestamp_ns) FROM usage_events
                 WHERE capsule_id = ?1 AND action = 'run' AND outcome = 'success'",
                params![id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| sql_error("index.query_failed", &e))?
            .flatten();

        Ok(UsageStats {
            successful_runs: successes,
            failed_runs: failures,
            last_success_age: last.map(|ns| Timestamp::from_nanos(ns).age(now)),
        })
    }

    // -- bindings ----------------------------------------------------------

    pub fn put_binding(&self, binding: &ContextBinding) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO context_bindings
                     (context_id, session_id, mux, mux_session, mux_surface, project_root,
                      isolation, bound_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(context_id) DO UPDATE SET
                     session_id = excluded.session_id,
                     mux = excluded.mux,
                     mux_session = excluded.mux_session,
                     mux_surface = excluded.mux_surface,
                     project_root = excluded.project_root,
                     isolation = excluded.isolation,
                     bound_ns = excluded.bound_ns",
                params![
                    binding.context_id.as_str(),
                    binding.session_id.as_str(),
                    binding.mux.as_str(),
                    binding.mux_session,
                    binding.mux_surface,
                    binding.project_root.as_ref().map(|p| p.display().to_string()),
                    binding.isolation.as_str(),
                    Timestamp::now().as_nanos(),
                ],
            )
            .map_err(|e| sql_error("index.write_failed", &e))?;
        Ok(())
    }

    pub fn bindings(&self) -> Result<Vec<ContextBinding>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT context_id, session_id, mux, mux_session, mux_surface, project_root,
                        isolation
                 FROM context_bindings ORDER BY context_id",
            )
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })
            .map_err(|e| sql_error("index.query_failed", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let (context, session, mux, mux_session, mux_surface, root, isolation) =
                row.map_err(|e| sql_error("index.query_failed", &e))?;
            out.push(ContextBinding {
                context_id: ContextId::parse(&context)?,
                session_id: SessionId::parse(&session)?,
                mux: MuxKind::from_str(&mux)?,
                mux_session,
                mux_surface,
                project_root: root.map(PathBuf::from),
                isolation: parse_isolation(&isolation)?,
            });
        }
        Ok(out)
    }

    pub fn binding(&self, context: &ContextId) -> Result<Option<ContextBinding>> {
        Ok(self
            .bindings()?
            .into_iter()
            .find(|b| &b.context_id == context))
    }

    // -- bypasses ----------------------------------------------------------

    /// Record an issued bypass token. Recording is not optional: a bypass that
    /// left no trace would defeat the point of allowing one at all.
    pub fn issue_bypass(&self, context: &ContextId, token: &BypassToken) -> Result<String> {
        let issued = Timestamp::now();
        let bypass_id = format!("byp_{}", ulid::Ulid::generate());
        let expires = match &token.scope {
            BypassScope::Duration(d) => {
                Some(issued.as_nanos() + (d.as_duration().as_nanos().min(i64::MAX as u128) as i64))
            }
            _ => None,
        };
        self.conn
            .execute(
                "INSERT INTO bypasses
                     (bypass_id, context_id, scope, capsule_id, reason, issued_ns, expires_ns, spent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
                params![
                    bypass_id,
                    context.as_str(),
                    token.scope.as_str(),
                    token.issued_for.as_ref().map(ToString::to_string),
                    token.reason,
                    issued.as_nanos(),
                    expires,
                ],
            )
            .map_err(|e| sql_error("index.write_failed", &e))?;
        Ok(bypass_id)
    }

    /// Unspent, unexpired bypasses for a context.
    pub fn open_bypasses(&self, context: &ContextId) -> Result<Vec<BypassRecord>> {
        let now = Timestamp::now().as_nanos();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT bypass_id, scope, capsule_id, reason, issued_ns, spent
                 FROM bypasses
                 WHERE context_id = ?1 AND spent = 0 AND (expires_ns IS NULL OR expires_ns > ?2)
                 ORDER BY issued_ns",
            )
            .map_err(|e| sql_error("index.query_failed", &e))?;
        let rows = stmt
            .query_map(params![context.as_str(), now], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| sql_error("index.query_failed", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let (id, scope, capsule, reason, issued, spent) =
                row.map_err(|e| sql_error("index.query_failed", &e))?;
            let mut token = BypassToken::new(match scope.as_str() {
                "session" => BypassScope::Session,
                "duration" => BypassScope::Session,
                _ => BypassScope::NextEvent,
            });
            token.reason = reason;
            token.issued_for = capsule.as_deref().map(CapsuleId::parse).transpose()?;
            out.push(BypassRecord {
                bypass_id: id,
                context_id: context.clone(),
                token,
                issued_at: Timestamp::from_nanos(issued),
                spent: spent != 0,
            });
        }
        Ok(out)
    }

    pub fn spend_bypass(&self, bypass_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE bypasses SET spent = 1 WHERE bypass_id = ?1",
                params![bypass_id],
            )
            .map_err(|e| sql_error("index.write_failed", &e))?;
        Ok(())
    }
}

/// A compact event row for `aikit log`.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSummary {
    pub event_id: String,
    pub timestamp: Timestamp,
    pub action: EventAction,
    pub capsule: Option<CapsuleId>,
    pub outcome: Outcome,
    pub bypass_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Multi-valued columns are newline-joined rather than JSON-encoded: they are
/// read far more often than written, `LIKE` still works on them, and a tag
/// containing a comma stops being a parsing puzzle.
fn join(values: &[String]) -> String {
    values.join("\n")
}

fn split(raw: &str) -> Vec<String> {
    raw.lines()
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_isolation(raw: &str) -> Result<Isolation> {
    Ok(match raw {
        "shared" => Isolation::Shared,
        "directory" => Isolation::Directory,
        "worktree" => Isolation::Worktree,
        other => {
            return err(
                "index.corrupt_row",
                format!("`{other}` is not an isolation mode"),
            )
        }
    })
}

#[allow(clippy::type_complexity)]
fn capsule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<CapsuleRow>> {
    let id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let name: String = row.get(2)?;
    let description: String = row.get(3)?;
    let tags: String = row.get(4)?;
    let maturity: String = row.get(5)?;
    let revision: String = row.get(6)?;
    let source: String = row.get(7)?;
    let exports: String = row.get(8)?;
    let root: Option<String> = row.get(9)?;

    Ok((|| {
        Ok(CapsuleRow {
            id: CapsuleId::parse(&id)?,
            kind: Kind::from_str(&kind)?,
            name,
            description,
            tags: split(&tags),
            maturity: parse_maturity(&maturity)?,
            revision: Revision::from_raw(revision),
            source: RegistrySource::new(source),
            exports: split(&exports),
            root: root.map(PathBuf::from),
        })
    })())
}

fn parse_maturity(raw: &str) -> Result<Maturity> {
    Ok(match raw {
        "draft" => Maturity::Draft,
        "candidate" => Maturity::Candidate,
        "stable" => Maturity::Stable,
        "deprecated" => Maturity::Deprecated,
        "blocked" => Maturity::Blocked,
        other => {
            return err(
                "index.corrupt_row",
                format!("`{other}` is not a maturity level"),
            )
        }
    })
}

pub(crate) fn sql_error(code: &'static str, source: &rusqlite::Error) -> AikitError {
    AikitError::new(code, format!("sqlite: {source}"))
}
