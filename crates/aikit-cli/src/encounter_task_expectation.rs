//! Exact task assertions travel with the addressed delivery, not a consumer's
//! cached claim that a resident is protected. The second preflight executes
//! under the same owner lock as task/Agency configuration and before transport.
use super::{error, EncounterAgencyBinding, EncounterService};
use aikit_core::{ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Keep this optional, comparatively large basis out of every IPC enum value.
/// Serde's Box representation preserves the existing JSON object unchanged.
pub type EncounterTaskExpectation = Box<EncounterTaskBasis>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterTaskBasis {
    pub revision: SourceRevision,
    pub task_ref: ResourceRef,
    pub now_ref: ResourceRef,
    pub now_revision: SourceRevision,
    pub policy_revision: SourceRevision,
    pub cwd: PathBuf,
    pub agent_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub world_binding_ref: ResourceRef,
    pub source_ref: ResourceRef,
    pub source_revision: SourceRevision,
    pub source_digest: String,
}

pub(super) fn check(
    service: &EncounterService,
    session: &ResourceRef,
    binding: &EncounterAgencyBinding,
    expected: &EncounterTaskExpectation,
) -> Result<()> {
    let task = EncounterService::read_task(&service.home, session)?;
    let pairs: [(&str, Value); 6] = [
        ("/revision", json!(expected.revision)),
        ("/request/central/task_ref", json!(expected.task_ref)),
        ("/allocation/allocation/now_ref", json!(expected.now_ref)),
        ("/allocation/allocation/revision/revision", json!(expected.now_revision)),
        ("/allocation/allocation/policy/revision", json!(expected.policy_revision)),
        ("/request/cwd", json!(expected.cwd)),
    ];
    if task["schema"] != "aikit.encounter-task/v1" || task["ready"] != true
        || pairs.iter().any(|(path, value)| task.pointer(path) != Some(value))
        || binding.agent_ref != expected.agent_ref
        || binding.agency_ref != expected.agency_ref
        || binding.world_binding_ref != expected.world_binding_ref
        || binding.agency_source.source_ref != expected.source_ref
        || binding.agency_source.revision != expected.source_revision
        || binding.agency_source.content_digest != expected.source_digest
    {
        return Err(error("Addressed task expectations differ from the actual task, working copy or selected Agency/source basis; explicitly re-resolve, never dispatch against a replacement"));
    }
    Ok(())
}
