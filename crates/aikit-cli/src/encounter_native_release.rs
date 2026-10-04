//! Exact native body retirement and explicit Task successor admission.
//! The fresh provider session is visible; it is never called native resumption.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReleasedPredecessor {
    pub expected_native_session_id: String,
    pub expected_generation: String,
    pub release_cursor: u64,
    pub expected_task_revision: SourceRevision,
}

pub(super) fn validate_predecessor(
    service: &EncounterService,
    session: &ResourceRef,
    current_binding: Option<&Value>,
    expected: &NativeReleasedPredecessor,
    owned_successor: bool,
) -> Result<Value> {
    let Some(binding) = current_binding else {
        return Err(AikitError::new(
            "encounter.released_predecessor_absent",
            "Exact prior native binding is absent",
        ));
    };
    let original = binding["native_session_id"].as_str()
        == Some(expected.expected_native_session_id.as_str())
        && binding["connection_generation"].as_str() == Some(expected.expected_generation.as_str());
    let successor = owned_successor
        && binding["continuation"] == "fresh-native-successor"
        && binding["released_predecessor"] == serde_json::to_value(expected).map_err(error)?;
    if !original && !successor {
        return Err(AikitError::new(
            "encounter.released_predecessor_changed",
            "Another native generation owns this session; no replacement effect",
        ));
    }
    let basis = service.store.released_predecessor_basis(
        session,
        &expected.expected_native_session_id,
        &expected.expected_generation,
        expected.release_cursor,
    )?;
    let current_agency = service
        .check_agency(session)?
        .map(|(binding, _)| binding)
        .ok_or_else(|| error("Task successor requires the current native Agency"))?;
    let current_agency = serde_json::to_value(current_agency).map_err(error)?;
    let previous_agency = &basis["opening"]["basis"]["agency_basis"];
    for key in ["agent_ref", "agency_ref", "world_ref", "world_binding_ref"] {
        if previous_agency[key].is_null() || previous_agency[key] != current_agency[key] {
            return Err(error(
                "Task successor changed canonical Agent, Agency or World",
            ));
        }
    }
    let task = service.validate_successor_task(session, &expected.expected_task_revision, &basis)?;
    Ok(json!({"predecessor":basis,"task":task,"native_resume":false}))
}

impl EncounterService {
    pub(super) fn release_native(
        &self,
        session: ResourceRef,
        native: String,
        generation: String,
        deadline: Instant,
    ) -> Result<Value> {
        // Cleanup belongs to this exact owned body even if its semantic space
        // was later detached. Replacement separately requires current attachment.
        // An exact retry returns the immutable old cleanup receipt even if a
        // successor is now resident. It never stops that successor.
        if let Some(receipt) = self
            .store
            .native_release_receipt(&session, &native, &generation)?
        {
            return if receipt["receipt"]["cleanup_confirmed"] == true {
                Ok(receipt)
            } else {
                Err(AikitError::new(
                    "encounter.native_release_uncertain",
                    "The retained exact release did not confirm cleanup",
                )
                .with("native_release", receipt.to_string()))
            };
        }
        let held = self.resident(&session)?;
        let operation = native_control_lease(&held, deadline)?;
        if held.generation != generation || held.lane.binding().native_session_id != native {
            return Err(AikitError::new(
                "encounter.native_release_basis",
                "Native session or connection generation changed; no process stopped",
            ));
        }
        if held.host.identity(&session)?.state != aikit_adapters::SessionLaneState::Resident {
            return Err(AikitError::new("encounter.native_release_busy", "An active or interrupted turn must settle through its ordinary lifecycle before idle release"));
        }
        if self
            .permissions
            .lock()
            .map_err(error)?
            .get(&session)
            .is_some_and(|requests| !requests.is_empty())
        {
            return Err(AikitError::new(
                "encounter.native_release_permission_pending",
                "Actual native consent requests must settle before idle-body release",
            ));
        }
        // Reuse the existing startup lease so cleanup and startup cannot race.
        // There is no new ownership registry and no map lock during native IO.
        let mut lease = self.begin_native_open(&session)?;
        lease.terminal_recorded = true;
        let removed = {
            let mut residents = self.residents.lock().map_err(error)?;
            if !residents
                .get(&session)
                .is_some_and(|current| Arc::ptr_eq(current, &held))
            {
                return Err(AikitError::new(
                    "encounter.native_release_basis",
                    "Resident changed before exact cleanup",
                ));
            }
            residents.remove(&session).expect("checked owned resident")
        };
        drop(operation);
        drop(held);
        let resident = match Arc::try_unwrap(removed) {
            Ok(resident) => resident,
            Err(held) => {
                self.residents.lock().map_err(error)?.insert(session, held);
                return Err(AikitError::new(
                    "encounter.resident_in_use",
                    "The body is borrowed by another owner operation; no cleanup occurred",
                ));
            }
        };
        if let Err(failure) = self
            .store
            .reserve_native_release(&session, &native, &generation)
        {
            self.residents
                .lock()
                .map_err(error)?
                .insert(session, Arc::new(resident));
            return Err(failure);
        }
        // Current committed Resident has no child-message watcher. The same
        // actual host shutdown owns its transport/process retirement.
        let cleanup = resident.host.shutdown();
        let cleanup_confirmed = cleanup.is_ok();
        let cleanup_error = cleanup.as_ref().err().map(ToString::to_string);
        let receipt = self
            .store
            .finish_native_release(
                &session,
                &native,
                &generation,
                cleanup_confirmed,
                cleanup
                    .as_ref()
                    .ok()
                    .and_then(|status| status.as_ref().map(ToString::to_string)),
                cleanup_error,
            )
            .map_err(|failure| {
                let cause = cleanup.as_ref().err().unwrap_or(&failure);
                AikitError::new(
                    "encounter.native_release_outcome_uncertain",
                    "Owned cleanup was attempted but its outcome could not be retained",
                )
                .with_io_source_from(cause)
                .with("cleanup_confirmed", cleanup_confirmed.to_string())
                .with(
                    "cleanup_error",
                    cleanup
                        .as_ref()
                        .err()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                )
                .with("journal_error", failure.to_string())
                .with("journal_error_code", failure.code())
            })?;
        self.permissions.lock().map_err(error)?.remove(&session);
        if !cleanup_confirmed {
            let cause = cleanup
                .err()
                .expect("unconfirmed native cleanup has an actual error");
            return Err(AikitError::new(
                "encounter.native_release_uncertain",
                "Exact owned cleanup is not fully confirmed; replacement remains fenced",
            )
            .with_io_source_from(&cause)
            .with("native_cleanup_code", cause.code())
            .with("native_release", receipt.to_string()));
        }
        Ok(receipt)
    }
}
