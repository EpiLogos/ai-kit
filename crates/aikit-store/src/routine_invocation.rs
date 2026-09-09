//! Canonical admission ledger for authorised Routine invocation evidence.
//!
//! The ledger persists AIKit-owned invocation envelopes and provider delivery
//! provenance. It does not execute Actions and contains no scheduler state.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::resource::routine::{
    RoutineInvocationAuthorisationRequest, RoutineInvocationEvidence, RoutineProviderDelivery,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};

use crate::{AikitHome, ContextLock, LockOptions};

pub const ROUTINE_INVOCATION_LEDGER_VERSION: &str = "aikit.routine-invocation-ledger/v1";
pub const ROUTINE_INVOCATION_ADMISSION_VERSION: &str = "aikit.routine-invocation-admission/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutineInvocationAdmissionStatus {
    Applied,
    AlreadyApplied,
    DeliveryRecorded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineInvocationAdmission {
    pub schema: String,
    pub status: RoutineInvocationAdmissionStatus,
    pub evidence: RoutineInvocationEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutineInvocationLedger {
    schema: String,
    #[serde(default)]
    invocations: Vec<RoutineInvocationEvidence>,
}

impl Default for RoutineInvocationLedger {
    fn default() -> Self {
        Self {
            schema: ROUTINE_INVOCATION_LEDGER_VERSION.into(),
            invocations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoutineInvocationStore {
    home: AikitHome,
}

impl RoutineInvocationStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    pub fn admit(
        &self,
        request: RoutineInvocationAuthorisationRequest,
    ) -> Result<RoutineInvocationAdmission> {
        // Authorise before acquiring the persistence lock. No envelope exists on
        // disabled, stale or revoked input, and therefore nothing can be written.
        let candidate = request.authorise()?;
        let _lock = ContextLock::acquire(
            &self.home,
            "routine-invocations",
            LockOptions::default().with_purpose("admit Routine invocation evidence"),
        )?;
        let mut ledger = self.load()?;

        if ledger.invocations.iter().any(|item| {
            item.trigger_observation_ref == candidate.trigger_observation_ref
                && item.invocation_ref != candidate.invocation_ref
        }) {
            return Err(AikitError::new(
                "routine.trigger_observation_identity_conflict",
                "trigger_observation_ref was already bound to another invocation",
            )
            .with(
                "trigger_observation_ref",
                candidate.trigger_observation_ref.to_string(),
            ));
        }
        for delivery in &candidate.provider_deliveries {
            if ledger.invocations.iter().any(|item| {
                item.invocation_ref != candidate.invocation_ref
                    && item
                        .provider_deliveries
                        .iter()
                        .any(|prior| prior.delivery_ref == delivery.delivery_ref)
            }) {
                return Err(AikitError::new(
                    "routine.provider_delivery_identity_conflict",
                    "delivery_ref was already bound to another invocation",
                )
                .with("delivery_ref", delivery.delivery_ref.to_string()));
            }
        }

        let (status, index) = match ledger
            .invocations
            .iter()
            .position(|item| item.invocation_ref == candidate.invocation_ref)
        {
            None => {
                let invocation_ref = candidate.invocation_ref.clone();
                ledger.invocations.push(candidate);
                ledger
                    .invocations
                    .sort_by(|left, right| left.invocation_ref.cmp(&right.invocation_ref));
                let index = ledger
                    .invocations
                    .iter()
                    .position(|item| item.invocation_ref == invocation_ref)
                    .expect("inserted invocation remains present");
                (RoutineInvocationAdmissionStatus::Applied, index)
            }
            Some(index) => {
                let existing = &mut ledger.invocations[index];
                if !existing.has_same_invocation_basis(&candidate) {
                    return Err(AikitError::new(
                        "routine.invocation_identity_conflict",
                        "invocation_ref was already admitted with different owner facts",
                    )
                    .with("invocation_ref", candidate.invocation_ref.to_string()));
                }
                let mut changed = false;
                for delivery in candidate.provider_deliveries {
                    changed |= merge_delivery(existing, delivery)?;
                }
                existing
                    .provider_deliveries
                    .sort_by(|left, right| left.delivery_ref.cmp(&right.delivery_ref));
                (
                    if changed {
                        RoutineInvocationAdmissionStatus::DeliveryRecorded
                    } else {
                        RoutineInvocationAdmissionStatus::AlreadyApplied
                    },
                    index,
                )
            }
        };

        if status != RoutineInvocationAdmissionStatus::AlreadyApplied {
            self.write(&ledger)?;
        }
        Ok(RoutineInvocationAdmission {
            schema: ROUTINE_INVOCATION_ADMISSION_VERSION.into(),
            status,
            evidence: ledger.invocations[index].clone(),
        })
    }

    pub fn get(&self, invocation_ref: &ResourceRef) -> Result<RoutineInvocationEvidence> {
        self.load()?
            .invocations
            .into_iter()
            .find(|item| &item.invocation_ref == invocation_ref)
            .ok_or_else(|| {
                AikitError::new(
                    "routine.invocation_not_found",
                    format!("no authorised Routine invocation {invocation_ref} is recorded"),
                )
            })
    }

    pub fn list(&self) -> Result<Vec<RoutineInvocationEvidence>> {
        Ok(self.load()?.invocations)
    }

    pub fn path(&self) -> PathBuf {
        self.home.state().join("routine-invocations.json")
    }

    fn load(&self) -> Result<RoutineInvocationLedger> {
        let path = self.path();
        if !path.exists() {
            return Ok(RoutineInvocationLedger::default());
        }
        let bytes =
            fs::read(&path).map_err(|error| io_error("routine.read_failed", &path, error))?;
        let ledger: RoutineInvocationLedger = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "routine.invalid_invocation_ledger",
                format!("{}: {error}", path.display()),
            )
        })?;
        if ledger.schema != ROUTINE_INVOCATION_LEDGER_VERSION {
            return Err(AikitError::new(
                "routine.unsupported_invocation_ledger",
                format!(
                    "{} uses unsupported schema {}",
                    path.display(),
                    ledger.schema
                ),
            ));
        }
        let mut previous: Option<&ResourceRef> = None;
        let mut trigger_refs = BTreeSet::new();
        let mut delivery_refs = BTreeMap::new();
        for evidence in &ledger.invocations {
            evidence.validate()?;
            if previous.is_some_and(|prior| prior >= &evidence.invocation_ref) {
                return Err(AikitError::new(
                    "routine.invalid_invocation_ledger",
                    "Routine invocation ledger identities must be unique and sorted",
                ));
            }
            if !trigger_refs.insert(&evidence.trigger_observation_ref) {
                return Err(AikitError::new(
                    "routine.invalid_invocation_ledger",
                    "Routine invocation ledger cannot bind one trigger observation to multiple invocations",
                ));
            }
            for delivery in &evidence.provider_deliveries {
                if delivery_refs
                    .insert(&delivery.delivery_ref, &evidence.invocation_ref)
                    .is_some()
                {
                    return Err(AikitError::new(
                        "routine.invalid_invocation_ledger",
                        "Routine invocation ledger cannot bind one provider delivery to multiple invocations",
                    ));
                }
            }
            previous = Some(&evidence.invocation_ref);
        }
        Ok(ledger)
    }

    fn write(&self, ledger: &RoutineInvocationLedger) -> Result<()> {
        let path = self.path();
        let parent = path.parent().expect("invocation ledger has a parent");
        fs::create_dir_all(parent)
            .map_err(|error| io_error("routine.write_failed", parent, error))?;
        let bytes = serde_json::to_vec_pretty(ledger).map_err(|error| {
            AikitError::new(
                "routine.invocation_ledger_unserializable",
                format!("could not encode Routine invocation ledger: {error}"),
            )
        })?;
        let temporary = parent.join(format!(
            ".routine-invocations-{}-{}.tmp",
            std::process::id(),
            ulid::Ulid::generate()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| io_error("routine.write_failed", &temporary, error))?;
        let result = (|| {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| io_error("routine.write_failed", &temporary, error))?;
            fs::rename(&temporary, &path)
                .map_err(|error| io_error("routine.commit_failed", &path, error))?;
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| io_error("routine.sync_parent_failed", parent, error))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn merge_delivery(
    existing: &mut RoutineInvocationEvidence,
    candidate: RoutineProviderDelivery,
) -> Result<bool> {
    if let Some(prior) = existing
        .provider_deliveries
        .iter()
        .find(|delivery| delivery.delivery_ref == candidate.delivery_ref)
    {
        if prior != &candidate {
            return Err(AikitError::new(
                "routine.provider_delivery_identity_conflict",
                "delivery_ref was already recorded with different provider facts",
            )
            .with("delivery_ref", candidate.delivery_ref.to_string()));
        }
        return Ok(false);
    }
    existing.provider_deliveries.push(candidate);
    Ok(true)
}

fn io_error(code: &'static str, path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {error}", path.display()))
        .with("path", path.display().to_string())
}
