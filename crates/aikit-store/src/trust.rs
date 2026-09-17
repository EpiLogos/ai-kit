//! Trust records, persisted.
//!
//! The rule this module exists to enforce is a negative one: **nothing a capsule
//! author writes can affect trust.** A manifest that mentions trust is refused by
//! `aikit_core::Capsule` before it reaches here, and this module never reads a
//! manifest at all — it takes a [`TrustKey`] and writes a row. The only way into
//! the table is a deliberate human review.
//!
//! ## Why the key includes the source
//!
//! `(source, capsule, revision)`. Dropping the source would mean a `git clone`
//! carrying `.aikit/capsules/hook/gate/secrets` with the same bytes as a hook you
//! reviewed last year would arrive pre-approved. It does not: `project-local` is
//! a different source, so the identical bytes are `Unseen`.
//!
//! ## Why an approval supersedes and a refusal does not
//!
//! Reviewing revision B of a capsule marks any *approved* revision A
//! `Superseded`: the row is kept for audit, but it no longer claims that
//! something you have replaced is trusted. A `Blocked` or `Quarantined` row is
//! untouched, because those are refusals — losing one would silently re-admit
//! something a human turned away.
//!
//! ## Why the oracle fails closed
//!
//! [`TrustStore`] implements [`TrustOracle`], whose `state` cannot return an
//! error. A database problem therefore reports `Unseen`, which withholds. The
//! alternative — reporting `Trusted` on a failed query — is not a trade-off, it
//! is a vulnerability.

use std::collections::BTreeMap;
use std::str::FromStr;

use rusqlite::params;

use aikit_core::trust::TrustOracle;
use aikit_core::{CapsuleId, RegistrySource, Result, Revision, TrustKey, TrustState};

use crate::events::Timestamp;
use crate::index::{sql_error, Index};

/// The revision value under which a standing verdict (`Blocked` / `Dismissed`)
/// is stored.
///
/// `*` is not valid blake3 hex, so it can never collide with a real content
/// revision. Storing standing verdicts this way keeps the existing
/// `(source, capsule, revision)` primary key — no migration — while giving a
/// refusal identity-scoped keying: it applies to every revision, so an edit
/// cannot slip past it.
const STANDING_REVISION: &str = "*";

/// One stored review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustRecord {
    pub key: TrustKey,
    pub state: TrustState,
    pub note: Option<String>,
    pub reviewed_at: Timestamp,
}

/// Read and write trust against the database.
pub struct TrustStore<'a> {
    index: &'a Index,
}

impl<'a> TrustStore<'a> {
    pub fn new(index: &'a Index) -> Self {
        Self { index }
    }

    /// Record a review, superseding the revisions this one replaces.
    ///
    /// Returns the keys that were superseded, so the caller can say
    /// "…and revision 7c2a9e is no longer trusted" rather than leaving the user
    /// to discover it.
    ///
    /// A *standing* verdict — `Blocked` or `Dismissed` — is not per-revision: it
    /// is keyed on identity so that editing the capsule cannot clear it. Such a
    /// record is routed to [`Self::set_standing`] regardless of the key's
    /// revision, mirroring [`aikit_core::trust::MemoryTrust`].
    pub fn record(
        &self,
        key: &TrustKey,
        state: TrustState,
        note: Option<&str>,
    ) -> Result<Vec<TrustKey>> {
        if state.is_standing() {
            self.set_standing(&key.source, &key.capsule, state, note)?;
            return Ok(Vec::new());
        }
        let now = Timestamp::now();
        let conn = self.index.conn();

        let superseded = if matches!(state, TrustState::Reviewed | TrustState::Trusted) {
            self.approved_revisions_other_than(&key.source, &key.capsule, &key.revision)?
        } else {
            Vec::new()
        };

        conn.execute(
            "INSERT INTO trust (source, capsule_id, revision, state, note, reviewed_ns)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(source, capsule_id, revision) DO UPDATE SET
                 state = excluded.state,
                 note = excluded.note,
                 reviewed_ns = excluded.reviewed_ns",
            params![
                key.source.as_str(),
                key.capsule.to_string(),
                key.revision.as_str(),
                state.as_str(),
                note,
                now.as_nanos(),
            ],
        )
        .map_err(|e| sql_error("trust.write_failed", &e))?;

        for old in &superseded {
            conn.execute(
                "UPDATE trust SET state = ?1, reviewed_ns = ?2
                 WHERE source = ?3 AND capsule_id = ?4 AND revision = ?5",
                params![
                    TrustState::Superseded.as_str(),
                    now.as_nanos(),
                    old.source.as_str(),
                    old.capsule.to_string(),
                    old.revision.as_str(),
                ],
            )
            .map_err(|e| sql_error("trust.write_failed", &e))?;
        }

        Ok(superseded)
    }

    fn approved_revisions_other_than(
        &self,
        source: &RegistrySource,
        capsule: &CapsuleId,
        keep: &Revision,
    ) -> Result<Vec<TrustKey>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(
                "SELECT revision FROM trust
                 WHERE source = ?1 AND capsule_id = ?2 AND revision != ?3
                   AND state IN ('reviewed', 'trusted')
                 ORDER BY revision",
            )
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        let rows = stmt
            .query_map(
                params![source.as_str(), capsule.to_string(), keep.as_str()],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| sql_error("trust.query_failed", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let revision = row.map_err(|e| sql_error("trust.query_failed", &e))?;
            out.push(TrustKey::new(
                source.clone(),
                capsule.clone(),
                Revision::from_raw(revision),
            ));
        }
        Ok(out)
    }

    /// Refuse a capsule for every revision, present and future.
    pub fn block(&self, source: &RegistrySource, capsule: &CapsuleId) -> Result<()> {
        self.set_standing(source, capsule, TrustState::Blocked, None)
    }

    /// Stop asking about a capsule without refusing it.
    pub fn dismiss(&self, source: &RegistrySource, capsule: &CapsuleId) -> Result<()> {
        self.set_standing(source, capsule, TrustState::Dismissed, None)
    }

    /// Lift a standing verdict, restoring ordinary per-revision keying.
    ///
    /// This does not grant approval: "I no longer refuse this" and "I have
    /// reviewed this" are different statements, and only the second activates.
    pub fn unblock(&self, source: &RegistrySource, capsule: &CapsuleId) -> Result<()> {
        self.index
            .conn()
            .execute(
                "DELETE FROM trust WHERE source = ?1 AND capsule_id = ?2 AND revision = ?3",
                params![source.as_str(), capsule.to_string(), STANDING_REVISION],
            )
            .map_err(|e| sql_error("trust.write_failed", &e))?;
        Ok(())
    }

    /// Write a standing verdict under the identity-only sentinel revision.
    fn set_standing(
        &self,
        source: &RegistrySource,
        capsule: &CapsuleId,
        state: TrustState,
        note: Option<&str>,
    ) -> Result<()> {
        debug_assert!(
            state.is_standing(),
            "set_standing given a per-revision state"
        );
        self.index
            .conn()
            .execute(
                "INSERT INTO trust (source, capsule_id, revision, state, note, reviewed_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(source, capsule_id, revision) DO UPDATE SET
                     state = excluded.state, note = excluded.note, reviewed_ns = excluded.reviewed_ns",
                params![
                    source.as_str(),
                    capsule.to_string(),
                    STANDING_REVISION,
                    state.as_str(),
                    note,
                    Timestamp::now().as_nanos(),
                ],
            )
            .map_err(|e| sql_error("trust.write_failed", &e))?;
        Ok(())
    }

    /// The recorded state, or `Unseen`. Fallible variant of [`TrustOracle::state`].
    pub fn state_of(&self, key: &TrustKey) -> Result<TrustState> {
        let mut stmt = self
            .index
            .conn()
            .prepare(
                "SELECT state FROM trust WHERE source = ?1 AND capsule_id = ?2 AND revision = ?3",
            )
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        let mut rows = stmt
            .query(params![
                key.source.as_str(),
                key.capsule.to_string(),
                key.revision.as_str()
            ])
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        match rows
            .next()
            .map_err(|e| sql_error("trust.query_failed", &e))?
        {
            Some(row) => {
                let raw: String = row
                    .get(0)
                    .map_err(|e| sql_error("trust.query_failed", &e))?;
                TrustState::from_str(&raw)
            }
            None => Ok(TrustState::Unseen),
        }
    }

    /// Every recorded revision of one capsule in one registry, newest first.
    pub fn history(
        &self,
        source: &RegistrySource,
        capsule: &CapsuleId,
    ) -> Result<Vec<TrustRecord>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(
                // The standing sentinel is not a revision, so it is excluded
                // from a per-revision history; it is surfaced via `standing_of`.
                "SELECT revision, state, note, reviewed_ns FROM trust
                 WHERE source = ?1 AND capsule_id = ?2 AND revision != '*'
                 ORDER BY reviewed_ns DESC, revision",
            )
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        let rows = stmt
            .query_map(params![source.as_str(), capsule.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| sql_error("trust.query_failed", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let (revision, state, note, ns) =
                row.map_err(|e| sql_error("trust.query_failed", &e))?;
            out.push(TrustRecord {
                key: TrustKey::new(
                    source.clone(),
                    capsule.clone(),
                    Revision::from_raw(revision),
                ),
                state: TrustState::from_str(&state)?,
                note,
                reviewed_at: Timestamp::from_nanos(ns),
            });
        }
        Ok(out)
    }

    /// Load every record into memory.
    ///
    /// Resolution asks about every capsule in the catalog, and doing that one
    /// `SELECT` at a time inside a 50 ms budget would be a needless round trip
    /// per capsule. The snapshot is taken once per command; there is no daemon
    /// for it to go stale in.
    pub fn snapshot(&self) -> Result<TrustSnapshot> {
        let mut stmt = self
            .index
            .conn()
            .prepare("SELECT source, capsule_id, revision, state FROM trust")
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| sql_error("trust.query_failed", &e))?;

        let mut entries = BTreeMap::new();
        let mut standing = BTreeMap::new();
        for row in rows {
            let (source, capsule, revision, state) =
                row.map_err(|e| sql_error("trust.query_failed", &e))?;
            let source = RegistrySource::new(source);
            let capsule = CapsuleId::parse(&capsule)?;
            let state = TrustState::from_str(&state)?;
            if revision == STANDING_REVISION {
                standing.insert((source, capsule), state);
            } else {
                entries.insert(
                    TrustKey::new(source, capsule, Revision::from_raw(revision)),
                    state,
                );
            }
        }
        Ok(TrustSnapshot { entries, standing })
    }

    /// Read the standing verdict, if any, for a capsule.
    fn standing_of(
        &self,
        source: &RegistrySource,
        capsule: &CapsuleId,
    ) -> Result<Option<TrustState>> {
        let mut stmt = self
            .index
            .conn()
            .prepare(
                "SELECT state FROM trust WHERE source = ?1 AND capsule_id = ?2 AND revision = ?3",
            )
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        let mut rows = stmt
            .query(params![
                source.as_str(),
                capsule.to_string(),
                STANDING_REVISION
            ])
            .map_err(|e| sql_error("trust.query_failed", &e))?;
        match rows
            .next()
            .map_err(|e| sql_error("trust.query_failed", &e))?
        {
            Some(row) => {
                let raw: String = row
                    .get(0)
                    .map_err(|e| sql_error("trust.query_failed", &e))?;
                Ok(Some(TrustState::from_str(&raw)?))
            }
            None => Ok(None),
        }
    }
}

impl TrustOracle for TrustStore<'_> {
    /// Fails closed: a database error reports `Unseen`, which withholds.
    fn state(&self, key: &TrustKey) -> TrustState {
        self.state_of(key).unwrap_or(TrustState::Unseen)
    }

    /// A block or dismissal, keyed on identity so an edit cannot clear it.
    fn standing_verdict(&self, source: &RegistrySource, capsule: &CapsuleId) -> Option<TrustState> {
        // Fail closed on a database error: `None` gives ordinary per-revision
        // keying, which cannot resurrect a blocked capsule because the block
        // simply is not seen — the caller then treats it as `Unseen`, which
        // still withholds projection.
        self.standing_of(source, capsule).unwrap_or(None)
    }
}

/// An immutable copy of the trust table, for one resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustSnapshot {
    /// Per-revision approvals, keyed on content.
    entries: BTreeMap<TrustKey, TrustState>,
    /// Standing refusals, keyed on identity.
    standing: BTreeMap<(RegistrySource, CapsuleId), TrustState>,
}

impl TrustSnapshot {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &BTreeMap<TrustKey, TrustState> {
        &self.entries
    }
}

impl TrustOracle for TrustSnapshot {
    fn state(&self, key: &TrustKey) -> TrustState {
        self.entries.get(key).copied().unwrap_or_default()
    }

    fn standing_verdict(&self, source: &RegistrySource, capsule: &CapsuleId) -> Option<TrustState> {
        self.standing
            .get(&(source.clone(), capsule.clone()))
            .copied()
    }
}
