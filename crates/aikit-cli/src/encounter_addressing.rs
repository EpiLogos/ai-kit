//! Read-only participant addressing projection for an already selected Agency.
//!
//! SessionSpace attachment is necessary but does not make a session addressable.
//! This projection rechecks the recipient's persisted Agency binding and native
//! Actuation admission, then discloses only the exact identity and revision a
//! subsequent addressed fanout needs. It never opens a provider, writes a
//! membership relation, or reserves a delivery.
use super::EncounterService;
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const ADDRESSABLE_PARTICIPANTS_SCHEMA: &str = "aikit.encounter-addressable-participants/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterAddressableParticipantsRequest {
    pub sender: ResourceRef,
    pub source_refs: BTreeSet<ResourceRef>,
    pub candidate_sessions: Vec<ResourceRef>,
}

/// The minimum owner-disclosed basis for a future addressed delivery. Agency,
/// WorldBinding, authority receipt and sharing policy remain private to the
/// owner; the send preflight always repeats this admission before transport.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EncounterAddressableParticipant {
    pub agent_session: ResourceRef,
    pub agent_ref: ResourceRef,
    pub expected_binding_revision: SourceRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EncounterAddressableParticipantsReading {
    pub schema: &'static str,
    pub participants: Vec<EncounterAddressableParticipant>,
}

fn cleanly_not_admitted(error: &AikitError) -> bool {
    matches!(
        error.code(),
        "encounter.participant_withdrawn"
            | "encounter.agency_changed"
            | "encounter.action_denied"
            | "encounter.context_stale"
            | "agency_admission.stale"
            | "agency_admission.denied"
            | "agency_admission.scope_mismatch"
    )
}

impl EncounterService {
    pub(super) fn addressable_participants(
        &self,
        request: EncounterAddressableParticipantsRequest,
    ) -> Result<Value> {
        if request.candidate_sessions.len() > 128 {
            return Err(AikitError::new(
                "encounter.addressable_participants_limit",
                "Addressable participant discovery accepts at most 128 explicit sessions",
            ));
        }
        if request.source_refs.len() > 128 {
            return Err(AikitError::new(
                "encounter.addressable_sources_limit",
                "Addressed participant selection accepts at most 128 explicit source refs",
            ));
        }
        let mut sessions = BTreeSet::new();
        let mut participants = Vec::new();
        for session in request.candidate_sessions {
            if !sessions.insert(session.clone()) {
                return Err(AikitError::new(
                    "encounter.duplicate_candidate",
                    "Addressable participant candidates must name each session once",
                ));
            }
            // This owner gate also makes direct CLI callers prove attachment;
            // the O:I kernel separately verifies Project membership before it
            // forwards this read.
            self.require_attached(&session)?;
            let binding = match self.check_agency(&session) {
                Ok(Some((binding, _))) => binding,
                Ok(None) => continue,
                Err(error) if cleanly_not_admitted(&error) => continue,
                Err(error) => return Err(error),
            };
            if binding.allowed_senders.contains(&request.sender)
                && request
                    .source_refs
                    .is_subset(&binding.allowed_packet_sources)
            {
                participants.push(EncounterAddressableParticipant {
                    agent_session: session,
                    agent_ref: binding.agent_ref,
                    expected_binding_revision: binding.revision,
                });
            }
        }
        Ok(json!(EncounterAddressableParticipantsReading {
            schema: ADDRESSABLE_PARTICIPANTS_SCHEMA,
            participants,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_store::AikitHome;

    #[test]
    fn addressable_participants_reads_type_and_refuses_excess_candidates() {
        let root = tempfile::tempdir().unwrap();
        let service = EncounterService::new(AikitHome::at(root.path())).unwrap();
        let empty = service
            .addressable_participants(EncounterAddressableParticipantsRequest {
                sender: ResourceRef::parse("agent/sender").unwrap(),
                source_refs: BTreeSet::new(),
                candidate_sessions: Vec::new(),
            })
            .unwrap();
        assert_eq!(empty["schema"], ADDRESSABLE_PARTICIPANTS_SCHEMA);
        assert_eq!(empty["participants"].as_array().unwrap().len(), 0);

        let candidates: Vec<ResourceRef> = (0..129)
            .map(|index| ResourceRef::parse(format!("agent-session/candidate-{index}")).unwrap())
            .collect();
        let error = service
            .addressable_participants(EncounterAddressableParticipantsRequest {
                sender: ResourceRef::parse("agent/sender").unwrap(),
                source_refs: BTreeSet::new(),
                candidate_sessions: candidates,
            })
            .unwrap_err();
        assert_eq!(error.code(), "encounter.addressable_participants_limit");
    }
}
