//! Optional Central task participation on the existing canonical Agency/session.
//! No Factory state, Central Profile, QL projection or second scheduler is needed.
use super::{error, EncounterAgencyBinding};
use aikit_adapters::{
    agency_admission::AdmittedAgency,
    central_task::{admit_task, CentralTaskAdmission, CentralTaskRequest},
    runner::SystemRunner,
};
use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EncounterTask {
    /// Only the local owner configuration operation accepts a request. Ingress
    /// cannot allocate a task or turn a request into an admission.
    Requested { request: CentralTaskRequest },
    /// Persisted by the owner after actual Central and Workcell operations.
    /// Reading these bytes alone is not fresh execution authority.
    Admitted { admission: CentralTaskAdmission },
}
impl EncounterTask {
    fn request(&self) -> &CentralTaskRequest {
        match self {
            Self::Requested { request } => request,
            Self::Admitted { admission } => &admission.request,
        }
    }
}

pub(super) fn configure(
    requested: Option<&EncounterTask>,
    agency: &AdmittedAgency,
) -> Result<Option<EncounterTask>> {
    requested
        .map(|task| {
            // Even a caller-supplied "admitted" value is recomputed through the
            // real owners. The caller cannot install a claimed receipt.
            let admission = admit_task(&SystemRunner::new(), task.request().clone(), agency)?;
            Ok(EncounterTask::Admitted { admission })
        })
        .transpose()
}

pub(super) fn selected(
    agency: Option<&(EncounterAgencyBinding, AdmittedAgency)>,
    actual_cwd: &Path,
) -> Result<Option<CentralTaskAdmission>> {
    let Some((binding, admitted)) = agency else {
        return Ok(None);
    };
    match &binding.task {
        None => Ok(None),
        Some(EncounterTask::Requested { .. }) => Err(AikitError::new(
            "encounter.task_not_admitted",
            "Task configuration was not admitted by the native owner",
        )),
        Some(EncounterTask::Admitted { admission }) => {
            admission.revalidate(&SystemRunner::new(), admitted, actual_cwd)?;
            Ok(Some(admission.clone()))
        }
    }
}

pub(super) fn pin(admission: Option<&CentralTaskAdmission>) -> Result<Option<String>> {
    admission
        .map(|admission| {
            Ok(blake3::hash(&serde_json::to_vec(admission).map_err(error)?)
                .to_hex()
                .to_string())
        })
        .transpose()
}

/// Re-resolution may replace a lease or current policy basis but cannot silently
/// move the same task to another working copy, clearing or Return destination.
/// The full old admission remains in the previous native binding journal entry.
pub(super) fn continuity(admission: Option<&CentralTaskAdmission>) -> Value {
    admission.map_or(Value::Null, |admission| {
        json!({
            "task_ref": admission.request.task_ref,
            "now_ref": admission.allocation["now_ref"],
            "now_source_ref": admission.allocation["source"]["ref"],
            "cwd": admission.request.cwd,
            "writable_destination": admission.allocation["writable_destination"],
            "return_destination": admission.request.return_destination,
            "participant_refs": admission.request.participant_refs,
            "source_refs": admission.request.source_refs
        })
    })
}

pub(super) struct Launch {
    pub argv: Vec<String>,
    pub requirements: Option<tempfile::NamedTempFile>,
    pub pin: Option<String>,
    pub continuity: Value,
    pub admission: Value,
}

pub(super) fn launch(
    home: &AikitHome,
    admission: Option<&CentralTaskAdmission>,
    provider_argv: &[String],
) -> Result<Launch> {
    let Some(admission) = admission else {
        return Ok(Launch {
            argv: provider_argv.to_vec(),
            requirements: None,
            pin: None,
            continuity: Value::Null,
            admission: Value::Null,
        });
    };
    let directory = home.state().join("encounter-task-boundaries");
    std::fs::create_dir_all(&directory).map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(error)?;
    }
    let mut requirements = tempfile::NamedTempFile::new_in(&directory).map_err(error)?;
    use std::io::Write;
    requirements
        .write_all(&serde_json::to_vec(&admission.requirements).map_err(error)?)
        .map_err(error)?;
    requirements.as_file().sync_all().map_err(error)?;
    Ok(Launch {
        argv: admission.protocol_argv(requirements.path(), provider_argv)?,
        requirements: Some(requirements),
        pin: pin(Some(admission))?,
        continuity: continuity(Some(admission)),
        admission: serde_json::to_value(admission).map_err(error)?,
    })
}
