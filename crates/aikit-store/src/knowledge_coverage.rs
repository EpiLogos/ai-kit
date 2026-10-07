//! Coverage accounting over source reads (Central #242 packet B).
//!
//! AIKit owns the only honest answer to "what of this source was actually
//! considered": extents that passed through real reads, plus an agent's
//! declared semantic reading, each recorded separately and never inferred
//! from checksums, file copying, or successful search. Checksums prove
//! retention; only reads prove consideration.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::AikitHome;

pub const KNOWLEDGE_COVERAGE_STORE_VERSION: &str = "aikit.knowledge-coverage/v1";

/// Who claims the coverage. An `Observed` row is the machine's own
/// record that bytes passed through a read; a `Declared` row is a named
/// agent stating it read and considered material — observation and
/// interpretation stay distinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageKind {
    Observed,
    Declared,
}

/// One covered range, in Unicode scalar (char) offsets — the same unit the
/// TextSpan selector and `SpanSelection` declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageExtent {
    pub start: u64,
    pub end: u64,
}

/// Coverage against one source at one content revision. A row for an older
/// revision is history, never current coverage: a changed source restarts
/// consideration at zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageRow {
    pub schema: String,
    pub source_ref: String,
    pub content_revision: String,
    pub kind: CoverageKind,
    #[serde(default)]
    pub extents: Vec<CoverageExtent>,
    pub recorded_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Marks a member that cannot be read as text at all: visible as
    /// incomplete rather than silently absent.
    #[serde(default)]
    pub unreadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageReading {
    pub source_ref: String,
    pub content_revision: String,
    /// Total extent the current revision's rows account for, as merged
    /// char ranges. `None` when nothing was ever read at this revision.
    pub covered: Vec<CoverageExtent>,
    pub declared: Vec<CoverageExtent>,
    pub observed: Vec<CoverageExtent>,
    pub unreadable: bool,
    pub rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CoverageState {
    schema: String,
    #[serde(default)]
    rows: Vec<CoverageRow>,
}

impl Default for CoverageState {
    fn default() -> Self {
        Self {
            schema: KNOWLEDGE_COVERAGE_STORE_VERSION.to_owned(),
            rows: Vec::new(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn merge_extents(mut extents: Vec<CoverageExtent>) -> Vec<CoverageExtent> {
    extents.retain(|extent| extent.end > extent.start);
    extents.sort_by_key(|extent| (extent.start, extent.end));
    let mut merged: Vec<CoverageExtent> = Vec::new();
    for extent in extents {
        match merged.last_mut() {
            Some(last) if extent.start <= last.end => {
                last.end = last.end.max(extent.end);
            }
            _ => merged.push(extent),
        }
    }
    merged
}

/// Coverage store — AIKit-owned operational evidence, one JSON state file.
#[derive(Debug, Clone)]
pub struct KnowledgeCoverageStore {
    home: AikitHome,
}

impl KnowledgeCoverageStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    fn path(&self) -> PathBuf {
        self.home.state().join("knowledge/coverage.json")
    }

    fn load(&self) -> Result<CoverageState> {
        let path = self.path();
        if !path.exists() {
            return Ok(CoverageState::default());
        }
        let bytes = fs::read(&path).map_err(|error| {
            AikitError::new("knowledge.coverage_read_failed", format!("{error}"))
                .with("path", path.display().to_string())
        })?;
        let state: CoverageState = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "knowledge.coverage_decode_failed",
                format!("could not decode {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        if state.schema != KNOWLEDGE_COVERAGE_STORE_VERSION {
            return Err(AikitError::new(
                "knowledge.coverage_schema_mismatch",
                format!(
                    "Coverage schema {} is not supported; expected {}",
                    state.schema, KNOWLEDGE_COVERAGE_STORE_VERSION
                ),
            )
            .with("path", path.display().to_string()));
        }
        Ok(state)
    }

    fn save(&self, state: &CoverageState) -> Result<()> {
        let path = self.path();
        let parent = path.parent().expect("coverage path has a parent");
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new("knowledge.coverage_prepare_failed", format!("{error}"))
                .with("path", parent.display().to_string())
        })?;
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            AikitError::new("knowledge.coverage_encode_failed", format!("{error}"))
        })?;
        let temp = parent.join(format!(".coverage-{}.tmp", Ulid::generate()));
        {
            let mut file = fs::File::create(&temp).map_err(|error| {
                AikitError::new("knowledge.coverage_write_failed", format!("{error}"))
                    .with("path", temp.display().to_string())
            })?;
            file.write_all(&bytes).map_err(|error| {
                AikitError::new("knowledge.coverage_write_failed", format!("{error}"))
            })?;
            file.sync_all().map_err(|error| {
                AikitError::new("knowledge.coverage_write_failed", format!("{error}"))
            })?;
        }
        fs::rename(&temp, &path).map_err(|error| {
            AikitError::new("knowledge.coverage_commit_failed", format!("{error}"))
                .with("path", path.display().to_string())
        })?;
        Ok(())
    }

    /// Record extents a real read observed. Rows merge into the current
    /// revision's extents; a revision change starts a fresh row set.
    pub fn record_observed(
        &self,
        source_ref: &str,
        content_revision: &str,
        extents: Vec<CoverageExtent>,
    ) -> Result<CoverageRow> {
        self.append(
            source_ref,
            content_revision,
            CoverageKind::Observed,
            extents,
            None,
            None,
            false,
        )
    }

    /// Record an agent's declared reading: the named actor states it
    /// considered the named extents. The declaration is evidence that the
    /// actor expressed it, exactly like the source material it cites.
    pub fn declare_reading(
        &self,
        source_ref: &str,
        content_revision: &str,
        extents: Vec<CoverageExtent>,
        actor: Option<String>,
        note: Option<String>,
    ) -> Result<CoverageRow> {
        if actor.as_deref().is_none_or(str::is_empty) {
            return Err(AikitError::new(
                "knowledge.coverage_declaration_needs_actor",
                "a declared reading names the actor who read",
            ));
        }
        self.append(
            source_ref,
            content_revision,
            CoverageKind::Declared,
            extents,
            actor,
            note,
            false,
        )
    }

    /// Mark a member unreadable at this revision: retained, visible, never
    /// pretend-considered.
    pub fn mark_unreadable(
        &self,
        source_ref: &str,
        content_revision: &str,
        note: Option<String>,
    ) -> Result<CoverageRow> {
        self.append(
            source_ref,
            content_revision,
            CoverageKind::Observed,
            Vec::new(),
            None,
            note,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn append(
        &self,
        source_ref: &str,
        content_revision: &str,
        kind: CoverageKind,
        extents: Vec<CoverageExtent>,
        actor: Option<String>,
        note: Option<String>,
        unreadable: bool,
    ) -> Result<CoverageRow> {
        if source_ref.trim().is_empty() || content_revision.trim().is_empty() {
            return Err(AikitError::new(
                "knowledge.coverage_invalid_input",
                "coverage rows name a source ref and a content revision",
            ));
        }
        let mut state = self.load()?;
        let row = CoverageRow {
            schema: KNOWLEDGE_COVERAGE_STORE_VERSION.to_owned(),
            source_ref: source_ref.to_owned(),
            content_revision: content_revision.to_owned(),
            kind,
            extents,
            recorded_at_ms: now_ms(),
            actor,
            note,
            unreadable,
        };
        state.rows.push(row.clone());
        self.save(&state)?;
        Ok(row)
    }

    /// The honest coverage reading of one source at one revision.
    pub fn reading(&self, source_ref: &str, content_revision: &str) -> Result<CoverageReading> {
        let state = self.load()?;
        let current: Vec<_> = state
            .rows
            .iter()
            .filter(|row| row.source_ref == source_ref && row.content_revision == content_revision)
            .collect();
        let observed = merge_extents(
            current
                .iter()
                .filter(|row| row.kind == CoverageKind::Observed)
                .flat_map(|row| row.extents.iter().copied())
                .collect(),
        );
        let declared = merge_extents(
            current
                .iter()
                .filter(|row| row.kind == CoverageKind::Declared)
                .flat_map(|row| row.extents.iter().copied())
                .collect(),
        );
        Ok(CoverageReading {
            source_ref: source_ref.to_owned(),
            content_revision: content_revision.to_owned(),
            covered: merge_extents(observed.iter().chain(declared.iter()).copied().collect()),
            declared,
            observed,
            unreadable: current.iter().any(|row| row.unreadable),
            rows: current.len(),
        })
    }

    /// Coverage readings for a set of sources — the collection-level answer
    /// joins this with the collection record's entry identities.
    pub fn readings(&self, wanted: &[(String, String)]) -> Result<Vec<CoverageReading>> {
        wanted
            .iter()
            .map(|(source_ref, revision)| self.reading(source_ref, revision))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> KnowledgeCoverageStore {
        let home = AikitHome::at(std::env::temp_dir().join(format!(
            "aikit-coverage-test-{}-{}",
            std::process::id(),
            Ulid::generate()
        )));
        KnowledgeCoverageStore::new(home)
    }

    #[test]
    fn observed_and_declared_stay_distinguishable_and_merge() {
        let store = store();
        store
            .record_observed(
                "central:source:x:1",
                "rev-1",
                vec![CoverageExtent { start: 0, end: 40 }],
            )
            .unwrap();
        store
            .record_observed(
                "central:source:x:1",
                "rev-1",
                vec![CoverageExtent { start: 30, end: 60 }],
            )
            .unwrap();
        store
            .declare_reading(
                "central:source:x:1",
                "rev-1",
                vec![CoverageExtent { start: 60, end: 90 }],
                Some("agent:reader".to_owned()),
                Some("episode read whole".to_owned()),
            )
            .unwrap();

        let reading = store.reading("central:source:x:1", "rev-1").unwrap();
        assert_eq!(reading.observed, vec![CoverageExtent { start: 0, end: 60 }]);
        assert_eq!(
            reading.declared,
            vec![CoverageExtent { start: 60, end: 90 }]
        );
        assert_eq!(reading.covered.len(), 1);
        assert_eq!(reading.covered[0], CoverageExtent { start: 0, end: 90 });
    }

    #[test]
    fn a_changed_revision_restarts_coverage_at_zero() {
        let store = store();
        store
            .record_observed(
                "central:source:x:2",
                "rev-1",
                vec![CoverageExtent { start: 0, end: 100 }],
            )
            .unwrap();
        let reading = store.reading("central:source:x:2", "rev-2").unwrap();
        assert!(reading.covered.is_empty());
        assert_eq!(reading.rows, 0);
    }

    #[test]
    fn unreadable_members_are_visible_not_absent() {
        let store = store();
        store
            .mark_unreadable(
                "central:source:x:3",
                "rev-1",
                Some("not valid UTF-8".to_owned()),
            )
            .unwrap();
        let reading = store.reading("central:source:x:3", "rev-1").unwrap();
        assert!(reading.unreadable);
        assert!(reading.covered.is_empty());
    }

    #[test]
    fn declarations_name_their_actor_or_fail() {
        let store = store();
        let missing = store.declare_reading(
            "central:source:x:4",
            "rev-1",
            vec![CoverageExtent { start: 0, end: 1 }],
            None,
            None,
        );
        assert!(missing.is_err());
    }
}
