//! Durable store for Routine records: `<aikit-home>/state/routines.json`.
//!
//! The Routine is the semantic automation record; this store only persists it
//! with the same discipline as the invocation ledger: authorisation is never
//! performed here, writes are atomic under a context lock, and every stored
//! Routine carries the exact content-hash revision of its own body so trigger
//! admission can bind evidence to an immutable basis. The stored envelope keeps
//! the Routine together with the two records that travel with it but are not
//! part of its semantic identity: the `aikit.time-schedule/v1` schedule record
//! (passed through verbatim to Central's occurrence resolution) and a foreign
//! adoption intent (`routine import-foreign --adopt`).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::resource::routine::Routine;
use aikit_core::resource::ResourceRef;
use aikit_core::schedule::{routine_source_revision, ScheduleRecord};
use aikit_core::{AikitError, Result};

use crate::{AikitHome, ContextLock, LockOptions};

pub const ROUTINE_STORE_VERSION: &str = "aikit.routine-store/v1";

/// A Routine's declared intent to take over one foreign harness timer. AIKit
/// never writes another product's store: retirement of the harness timer is an
/// explicit owner act in the harness, checked read-only at report time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForeignAdoption {
    /// `openclaw-cron` or `hermes-cron` (the import provider id).
    pub provider: String,
    pub provider_job_id: String,
    /// RFC 3339 instant the adoption was declared.
    pub adopted_at: String,
}

/// One persisted Routine together with its schedule record and optional
/// foreign adoption intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredRoutine {
    pub routine: Routine,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_schedule: Option<ScheduleRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_adoption: Option<ForeignAdoption>,
}

impl StoredRoutine {
    /// Assemble one stored record and stamp the Routine's content-hash
    /// revision. The revision covers the Routine body with its revision field
    /// excluded, so any mutation of the body moves the revision.
    pub fn new(
        routine: Routine,
        time_schedule: Option<ScheduleRecord>,
        foreign_adoption: Option<ForeignAdoption>,
    ) -> Result<Self> {
        let mut record = Self {
            routine,
            time_schedule,
            foreign_adoption,
        };
        let revision = Self::body_revision(&record.routine)?;
        record.routine.revision = Some(revision);
        Ok(record)
    }

    /// The content-hash revision of a Routine body, independent of the
    /// revision field itself.
    fn body_revision(routine: &Routine) -> Result<aikit_core::resource::SourceRevision> {
        let mut bare = routine.clone();
        bare.revision = None;
        routine_source_revision(&bare)
    }

    /// Re-stamp the revision after any mutation of the Routine body.
    pub fn restamp_revision(&mut self) -> Result<()> {
        let revision = Self::body_revision(&self.routine)?;
        self.routine.revision = Some(revision);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RoutineStore {
    home: AikitHome,
}

impl RoutineStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    /// Load every stored Routine in stable identity order, validating the
    /// store's schema and each record's revision integrity on the way.
    pub fn list(&self) -> Result<Vec<StoredRoutine>> {
        let store = self.load()?;
        Ok(store.routines)
    }

    pub fn get(&self, routine_ref: &ResourceRef) -> Result<StoredRoutine> {
        self.load()?
            .routines
            .into_iter()
            .find(|record| &record.routine.id == routine_ref)
            .ok_or_else(|| {
                AikitError::new(
                    "routine.not_found",
                    format!("no Routine {routine_ref} is stored"),
                )
                .with("routine", routine_ref.to_string())
            })
    }

    /// Insert or replace one Routine record. The record is validated and its
    /// revision re-stamped from the exact stored body before the write.
    pub fn put(&self, mut record: StoredRoutine) -> Result<StoredRoutine> {
        record.restamp_revision()?;
        record.routine.validate_stored()?;
        let _lock = ContextLock::acquire(
            &self.home,
            "routines",
            LockOptions::default().with_purpose("store Routine record"),
        )?;
        let mut store = self.load()?;
        let id = record.routine.id.clone();
        store.routines.retain(|existing| existing.routine.id != id);
        store.routines.push(record.clone());
        store
            .routines
            .sort_by(|left, right| left.routine.id.cmp(&right.routine.id));
        self.write(&store)?;
        Ok(record)
    }

    pub fn delete(&self, routine_ref: &ResourceRef) -> Result<()> {
        let _lock = ContextLock::acquire(
            &self.home,
            "routines",
            LockOptions::default().with_purpose("delete Routine record"),
        )?;
        let mut store = self.load()?;
        let before = store.routines.len();
        store
            .routines
            .retain(|existing| existing.routine.id != *routine_ref);
        if store.routines.len() == before {
            return Err(AikitError::new(
                "routine.not_found",
                format!("no Routine {routine_ref} is stored"),
            )
            .with("routine", routine_ref.to_string()));
        }
        self.write(&store)
    }

    pub fn path(&self) -> PathBuf {
        self.home.state().join("routines.json")
    }

    fn load(&self) -> Result<RoutineStoreFile> {
        let path = self.path();
        if !path.exists() {
            return Ok(RoutineStoreFile::default());
        }
        let bytes =
            fs::read(&path).map_err(|error| io_error("routine.store_read_failed", &path, error))?;
        let store: RoutineStoreFile = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "routine.store_invalid",
                format!("{}: {error}", path.display()),
            )
        })?;
        if store.schema != ROUTINE_STORE_VERSION {
            return Err(AikitError::new(
                "routine.store_unsupported",
                format!(
                    "{} uses unsupported schema {}",
                    path.display(),
                    store.schema
                ),
            ));
        }
        let mut previous: Option<&ResourceRef> = None;
        for record in &store.routines {
            let expected = StoredRoutine::body_revision(&record.routine)?;
            if record.routine.revision.as_ref() != Some(&expected) {
                return Err(AikitError::new(
                    "routine.store_revision_mismatch",
                    format!(
                        "stored Routine {} does not carry the revision of its own body; the record \
                         was changed outside this store",
                        record.routine.id
                    ),
                ));
            }
            if previous.is_some_and(|prior| prior >= &record.routine.id) {
                return Err(AikitError::new(
                    "routine.store_invalid",
                    "Routine store identities must be unique and sorted",
                ));
            }
            previous = Some(&record.routine.id);
        }
        Ok(store)
    }

    fn write(&self, store: &RoutineStoreFile) -> Result<()> {
        let path = self.path();
        let parent = path.parent().expect("routine store has a parent");
        fs::create_dir_all(parent)
            .map_err(|error| io_error("routine.store_write_failed", parent, error))?;
        let bytes = serde_json::to_vec_pretty(store).map_err(|error| {
            AikitError::new(
                "routine.store_unserializable",
                format!("could not encode Routine store: {error}"),
            )
        })?;
        let temporary = parent.join(format!(
            ".routines-{}-{}.tmp",
            std::process::id(),
            ulid::Ulid::generate()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| io_error("routine.store_write_failed", &temporary, error))?;
        let result = (|| {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| io_error("routine.store_write_failed", &temporary, error))?;
            fs::rename(&temporary, &path)
                .map_err(|error| io_error("routine.store_commit_failed", &path, error))?;
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| io_error("routine.store_sync_parent_failed", parent, error))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutineStoreFile {
    schema: String,
    #[serde(default)]
    routines: Vec<StoredRoutine>,
}

impl Default for RoutineStoreFile {
    fn default() -> Self {
        Self {
            schema: ROUTINE_STORE_VERSION.into(),
            routines: Vec::new(),
        }
    }
}

fn io_error(code: &'static str, path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {error}", path.display()))
        .with("path", path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::method::{Method, MethodSkillRef};
    use aikit_core::resource::routine::{
        ProvenMethodBasis, RoutineAuthority, RoutineTrigger, METHOD_PROOF_VERSION,
    };
    use aikit_core::resource::{ProviderRef, SourceRef, SourceRevision};
    use aikit_core::schedule::{ScheduleShape, TIME_SCHEDULE_VERSION};

    fn revision(raw: &str) -> SourceRevision {
        SourceRevision::parse(raw).unwrap()
    }

    fn method() -> Method {
        Method {
            id: ResourceRef::parse("skill/method/demo").unwrap(),
            source: SourceRef::parse("source/aikit/personal-registry/skill/method/demo").unwrap(),
            revision: Some(revision(
                "blake3:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )),
            name: "Demo Method".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: Vec::<MethodSkillRef>::new(),
            actions: vec![ResourceRef::parse("action/capability/run").unwrap()],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![ResourceRef::parse("verification:demo").unwrap()],
            expected_resolve: None,
            expected_return_forms: vec![],
        }
    }

    fn proof() -> ProvenMethodBasis {
        ProvenMethodBasis {
            version: METHOD_PROOF_VERSION.into(),
            method: method().id,
            method_revision: method().revision.unwrap(),
            proof_ref: ResourceRef::parse("proof/method/demo").unwrap(),
            context_resolution_ref: ResourceRef::parse("context-resolution:demo").unwrap(),
            activity_refs: vec![ResourceRef::parse("activity:demo:1").unwrap()],
            return_refs: vec![ResourceRef::parse("return:demo:1").unwrap()],
            evidence_refs: vec![ResourceRef::parse("evidence:demo:1").unwrap()],
            verification_refs: vec![ResourceRef::parse("verification:demo:1").unwrap()],
        }
    }

    fn stored_routine() -> StoredRoutine {
        let method = method();
        let routine = Routine::new(
            ResourceRef::parse("routine/daily-demo").unwrap(),
            SourceRef::parse("source:aikit:routines/routine/daily-demo").unwrap(),
            None,
            "Daily demo",
            "",
            &method,
            proof(),
            RoutineTrigger::Schedule {
                schedule_ref: "schedule/daily-demo".into(),
            },
            RoutineAuthority {
                authority_ref: ResourceRef::parse("authority:routine:demo").unwrap(),
                revision: Some(revision("authority-rev-1")),
                action_refs: vec![ResourceRef::parse("action/capability/run").unwrap()],
                granted: true,
                unattended: true,
            },
            None,
            vec![],
        )
        .unwrap();
        StoredRoutine::new(routine, None, None).unwrap()
    }

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().join("home"));
        (dir, home)
    }

    #[test]
    fn put_stamps_revision_and_list_round_trips() {
        let (_dir, home) = home();
        let store = RoutineStore::new(home);
        let record = store.put(stored_routine()).unwrap();
        assert!(record.routine.revision.is_some());
        let loaded = store
            .get(&ResourceRef::parse("routine/daily-demo").unwrap())
            .unwrap();
        assert_eq!(loaded, record);
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn mutation_moves_the_revision_and_the_store_detects_tampering() {
        let (_dir, home) = home();
        let store = RoutineStore::new(home);
        let first = store.put(stored_routine()).unwrap();
        let mut second = first.clone();
        second.routine.disable();
        let re_stamped = store.put(second).unwrap();
        assert_ne!(first.routine.revision, re_stamped.routine.revision);

        // A hand edit that does not move the revision is refused on read.
        let path = store.path();
        let mut tampered: RoutineStoreFile =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        tampered.routines[0].routine.name = "Tampered".into();
        fs::write(&path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        let error = store.list().unwrap_err();
        assert_eq!(error.code(), "routine.store_revision_mismatch");
    }

    #[test]
    fn schedule_record_and_adoption_travel_with_the_routine() {
        let (_dir, home) = home();
        let store = RoutineStore::new(home);
        let mut record = stored_routine();
        record.time_schedule = Some(
            ScheduleRecord::new(
                ResourceRef::parse("schedule/daily-demo").unwrap(),
                ScheduleShape::Daily {
                    time: "06:00".into(),
                },
                None,
            )
            .unwrap(),
        );
        record.foreign_adoption = Some(ForeignAdoption {
            provider: "openclaw-cron".into(),
            provider_job_id: "9efb7069-a72b-4ccc-8b4e-4e9134c58b57".into(),
            adopted_at: "2026-09-23T12:00:00Z".into(),
        });
        store.put(record.clone()).unwrap();
        let loaded = store
            .get(&ResourceRef::parse("routine/daily-demo").unwrap())
            .unwrap();
        assert_eq!(loaded.time_schedule.unwrap().schema, TIME_SCHEDULE_VERSION);
        assert_eq!(loaded.foreign_adoption.unwrap().provider, "openclaw-cron");
    }

    #[test]
    fn delete_removes_and_refuses_unknown_identities() {
        let (_dir, home) = home();
        let store = RoutineStore::new(home);
        store.put(stored_routine()).unwrap();
        let routine_ref = ResourceRef::parse("routine/daily-demo").unwrap();
        store.delete(&routine_ref).unwrap();
        assert!(store.list().unwrap().is_empty());
        assert_eq!(
            store.delete(&routine_ref).unwrap_err().code(),
            "routine.not_found"
        );
    }

    #[test]
    fn scheduler_binding_persists_for_dispatch_selection() {
        let (_dir, home) = home();
        let store = RoutineStore::new(home);
        let mut record = stored_routine();
        record
            .routine
            .set_scheduler_binding(aikit_core::resource::routine::RoutineSchedulerBinding {
                provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
                provider_job_id: None,
                observed_state: aikit_core::resource::routine::RoutineSchedulerState::Planned,
            })
            .unwrap();
        store.put(record).unwrap();
        let loaded = store
            .get(&ResourceRef::parse("routine/daily-demo").unwrap())
            .unwrap();
        assert_eq!(
            loaded.routine.scheduler.unwrap().provider.as_str(),
            "provider:aikit-gateway"
        );
    }
}
