//! Live fetch + composition of the Actuation and Central projections.
//!
//! The intakes (`ActuationInstantiationProjection`, `CentralAgentProfileProjection`)
//! are pure deserializers. This module is the only place that fetches them from
//! their native surfaces and composes the resolution inputs. Discovery is
//! explicit and never guesses: a Central profile is used only when exactly one
//! resolves for the Project, and the Actuation receipt is read from a known
//! authored file. Absence is a valid "no projection" state, never a failure.

use std::path::Path;

use aikit_core::context_resolution::RequestedActors;
use aikit_core::resource::{
    ResourceDescriptor, ResourceKind, ResourceLocator, ResourceRecord, ResourceSource,
    SourceAuthority, SourceRef, SourceRevision, SourceState,
};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::actuation_instantiation::{
    compose_actor_inputs, ActuationInstantiationProjection, ComposedActorInputs,
};
use crate::central_agent_profile::CentralAgentProfileProjection;
use crate::runner::CommandRunner;

/// Authored Actuation instantiation receipt, relative to the Project root.
pub const ACTUATION_MODEL_BEARING_FILE: &str = ".aikit/actuation-model-bearing.json";

const AGENT_PROFILE_LIST: &str = "agent-profile.list";

/// Compose the live actor inputs for a Project. Returns `None` when neither a
/// Central-authored profile nor an Actuation instantiation receipt is present —
/// a valid "no projection" state, never a failure.
pub fn compose_live_actor_inputs<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project_root: &Path,
) -> Result<Option<ComposedActorInputs>> {
    let profile = read_project_agent_profile(runner, central_root, project_root)?;
    let central = profile
        .as_ref()
        .map(|(profile, _)| profile.authored_projection())
        .unwrap_or_default();
    let mut source_resources: Vec<ResourceRecord> =
        profile.into_iter().map(|(_, record)| record).collect();
    let actuation = read_actuation_instantiation(project_root)?;

    match actuation {
        Some((actuation, record)) => {
            source_resources.push(record);
            let mut composed = compose_actor_inputs(&actuation, &central);
            composed.source_resources = source_resources;
            Ok(Some(composed))
        }
        None if central.agent_ref.is_some() || !central.profile_refs.is_empty() => {
            Ok(Some(ComposedActorInputs {
                authored: central.clone(),
                source_resources,
                requested_actors: RequestedActors {
                    agent: central.agent_ref,
                    agency: None,
                    host: central.host_ref,
                },
                selected_harness: None,
                selected_model: None,
                agent_session: None,
            }))
        }
        None => Ok(None),
    }
}

/// Query Central's authored profile for a Project. Only a Project physically
/// under `central_root/Work` is mapped; a profile is used only when exactly one
/// resolves — ambiguity is never resolved by guessing.
fn read_project_agent_profile<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project_root: &Path,
) -> Result<Option<(CentralAgentProfileProjection, ResourceRecord)>> {
    let Some(member) = project_member(central_root, project_root) else {
        return Ok(None);
    };
    let list = central_action(
        runner,
        central_root,
        AGENT_PROFILE_LIST,
        json!({ "scope": "project", "project": member }),
    )?;
    let Some(profiles) = list.get("profiles").and_then(Value::as_array) else {
        return Ok(None);
    };
    if profiles.is_empty() {
        return Ok(None);
    }
    let [entry] = profiles.as_slice() else {
        return Err(AikitError::new(
            "actor_composition.ambiguous_profile",
            "Multiple Central profiles require explicit selection; none is guessed",
        ));
    };
    let source = entry.get("profile").ok_or_else(|| {
        AikitError::new(
            "actor_composition.invalid_profile_listing",
            "Central agent-profile.list returned an entry without a profile",
        )
    })?;
    let profile = CentralAgentProfileProjection::parse(source)?;
    let relative = entry
        .get("source_path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "actor_composition.profile_source_missing",
                "Central profile listing lacks source_path",
            )
        })?;
    let root = std::fs::canonicalize(project_root).map_err(source_error)?;
    let path = std::fs::canonicalize(root.join(relative)).map_err(source_error)?;
    if !path.starts_with(&root) {
        return Err(AikitError::new(
            "actor_composition.profile_source_outside",
            "Central profile source escapes Project",
        ));
    }
    let bytes = std::fs::read(&path).map_err(source_error)?;
    let current: Value = serde_json::from_slice(&bytes).map_err(source_error)?;
    if CentralAgentProfileProjection::parse(&current)? != profile {
        return Err(AikitError::new(
            "actor_composition.profile_source_changed",
            "Central profile changed after owner listing",
        ));
    }
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(root.join("ProjectCentral/project.json")).map_err(source_error)?,
    )
    .map_err(source_error)?;
    let project_id = manifest
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or(&member);
    let relative = path
        .strip_prefix(&root)
        .map_err(source_error)?
        .to_string_lossy()
        .replace('%', "%25")
        .replace(':', "%3A")
        .replace(' ', "%20");
    let reference = format!("central:source:project:{project_id}:{relative}");
    let mut descriptor = ResourceDescriptor::new(
        profile.agent_ref.clone(),
        ResourceKind::Agent,
        profile
            .role
            .clone()
            .unwrap_or_else(|| profile.agent_ref.to_string()),
        profile.purpose.clone().unwrap_or_default(),
    );
    descriptor
        .sources
        .push(observed_source(&reference, &profile.revision, &path)?);
    descriptor.annotations.insert(
        "source_bytes_blake3".into(),
        blake3::hash(&bytes).to_hex().to_string(),
    );
    descriptor.annotations.insert(
        "standing".into(),
        "Central source observed; runtime admission undetermined".into(),
    );
    Ok(Some((profile, ResourceRecord::new(descriptor))))
}

/// Read an authored Actuation instantiation receipt. Absence is `None`, never an
/// error and never a synthesized model-bearing object.
fn read_actuation_instantiation(
    project_root: &Path,
) -> Result<Option<(ActuationInstantiationProjection, ResourceRecord)>> {
    let path = project_root.join(ACTUATION_MODEL_BEARING_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AikitError::new(
                "actor_composition.model_bearing_unreadable",
                format!("{}: {error}", path.display()),
            ));
        }
    };
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "actor_composition.model_bearing_invalid",
            format!("{}: {error}", path.display()),
        )
    })?;
    let projection = ActuationInstantiationProjection::parse(&value)?;
    let path = std::fs::canonicalize(path).map_err(source_error)?;
    let revision = format!("blake3:{}", blake3::hash(text.as_bytes()));
    let mut descriptor = ResourceDescriptor::new(
        projection.agency_ref.clone(),
        ResourceKind::Agency,
        projection.agency_ref.to_string(),
        "Agency referenced by observed native Actuation instantiation source",
    );
    // This native receipt supplies its own identity; path/revision disclose the
    // observed source, without asserting fresh detection or an authority grant.
    descriptor.sources.push(observed_source(
        projection.actuation_ref.as_str(),
        &revision,
        &path,
    )?);
    descriptor.annotations.insert(
        "standing".into(),
        "Actuation receipt observed; runtime admission undetermined".into(),
    );
    Ok(Some((projection, ResourceRecord::new(descriptor))))
}

fn source_error(error: impl std::fmt::Display) -> AikitError {
    AikitError::new("actor_composition.source_unavailable", error.to_string())
}
fn observed_source(reference: &str, revision: &str, path: &Path) -> Result<ResourceSource> {
    Ok(ResourceSource {
        source: SourceRef::parse(reference)?,
        revision: Some(SourceRevision::parse(revision)?),
        locator: Some(ResourceLocator::Path(path.to_path_buf())),
        authority: Some(SourceAuthority::Observed),
        state: SourceState::Available,
    })
}

/// Central Action invocation, mirroring the temporal adapter's owner-call path:
/// `ctrl --json --root ROOT action run <id> <input>`.
fn central_action<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    id: &str,
    input: Value,
) -> Result<Value> {
    let argv = vec![
        "ctrl".to_owned(),
        "--json".to_owned(),
        "--root".to_owned(),
        central_root.display().to_string(),
        "action".to_owned(),
        "run".to_owned(),
        id.to_owned(),
        input.to_string(),
    ];
    let output = runner.run(&argv)?;
    if !output.ok() {
        return Err(AikitError::new(
            "actor_composition.central_action_failed",
            format!("Central Action {id} exited {}", output.status),
        )
        .with("action", id)
        .with("stderr", output.stderr.trim().to_owned()));
    }
    let result: Value = serde_json::from_str(output.stdout.trim()).map_err(|error| {
        AikitError::new(
            "actor_composition.central_action_invalid",
            format!("Central Action {id} returned invalid JSON: {error}"),
        )
    })?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = result
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Central Action failed");
        return Err(AikitError::new(
            "actor_composition.central_action_failed",
            message,
        ));
    }
    result.get("data").cloned().ok_or_else(|| {
        AikitError::new(
            "actor_composition.central_action_invalid",
            format!("Central Action {id} succeeded without data"),
        )
    })
}

fn project_member(central_root: &Path, project_root: &Path) -> Option<String> {
    let relative = project_root.strip_prefix(central_root.join("Work")).ok()?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::ScriptedRunner;
    use aikit_core::resource::ResourceRef;

    fn profile() -> Value {
        json!({
            "schema": "central.agent-profile/v1",
            "ref": "profile/central/build",
            "revision": "r1",
            "agent_ref": "agent/mahamaya",
            "scope": "project",
            "world_ref": "world/central",
            "skill_set_refs": ["skill-set/build"]
        })
    }

    fn model_bearing() -> Value {
        json!({
            "schema": "actuation.model-bearing/v1",
            "actuation_ref": "actuation/root",
            "agency_ref": "agency/mahamaya-build",
            "world_binding_ref": "world-binding/central",
            "harness_ref": "harness/codex",
            "agent_session_ref": "agent-session/codex-7",
            "model_relation": { "model_ref": "model/deepseek-chat" }
        })
    }

    fn list_data(profiles: Vec<Value>) -> String {
        json!({
            "ok": true,
            "data": {
                "scope": "project",
                "profiles": profiles,
                "source_payloads_disclosed": false
            }
        })
        .to_string()
    }

    fn temp_project() -> (std::path::PathBuf, std::path::PathBuf) {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let central =
            std::env::temp_dir().join(format!("actor-central-{}-{nonce}", std::process::id()));
        let project = central.join("Work/example");
        std::fs::create_dir_all(project.join(".aikit")).unwrap();
        std::fs::create_dir_all(project.join("ProjectCentral")).unwrap();
        std::fs::write(
            project.join("ProjectCentral/project.json"),
            json!({"project_id":"example"}).to_string(),
        )
        .unwrap();
        std::fs::write(project.join("p.json"), profile().to_string()).unwrap();
        (central, project)
    }

    #[test]
    fn composes_project_profile_and_model_bearing_into_actor_inputs() {
        let (central, project) = temp_project();
        let runner = ScriptedRunner::new().on(
            "agent-profile.list",
            &list_data(vec![
                json!({ "source_path": "p.json", "profile": profile() }),
            ]),
        );
        std::fs::write(
            project.join(ACTUATION_MODEL_BEARING_FILE),
            model_bearing().to_string(),
        )
        .unwrap();

        let composed = compose_live_actor_inputs(&runner, &central, &project)
            .unwrap()
            .unwrap();
        assert_eq!(
            composed.requested_actors.agent,
            Some(ResourceRef::parse("agent/mahamaya").unwrap())
        );
        assert_eq!(
            composed.requested_actors.agency,
            Some(ResourceRef::parse("agency/mahamaya-build").unwrap())
        );
        assert_eq!(
            composed.selected_harness,
            Some(ResourceRef::parse("harness/codex").unwrap())
        );
        assert_eq!(
            composed.selected_model,
            Some(ResourceRef::parse("model/deepseek-chat").unwrap())
        );
        assert_eq!(
            composed.agent_session,
            Some("agent-session/codex-7".to_string())
        );
        std::fs::remove_dir_all(&central).unwrap();
    }

    #[test]
    fn model_bearing_absence_yields_central_only_slice() {
        let (central, project) = temp_project();
        let runner = ScriptedRunner::new().on(
            "agent-profile.list",
            &list_data(vec![
                json!({ "source_path": "p.json", "profile": profile() }),
            ]),
        );

        let composed = compose_live_actor_inputs(&runner, &central, &project)
            .unwrap()
            .unwrap();
        assert_eq!(
            composed.requested_actors.agent,
            Some(ResourceRef::parse("agent/mahamaya").unwrap())
        );
        assert_eq!(composed.requested_actors.agency, None);
        assert_eq!(composed.selected_harness, None);
        assert_eq!(composed.selected_model, None);
        std::fs::remove_dir_all(&central).unwrap();
    }

    #[test]
    fn ambiguous_or_absent_profile_is_never_guessed() {
        let (central, project) = temp_project();
        // Two profiles: ambiguity must not resolve by guessing.
        let two = ScriptedRunner::new().on(
            "agent-profile.list",
            &list_data(vec![
                json!({ "source_path": "a.json", "profile": profile() }),
                json!({ "source_path": "b.json", "profile": profile() }),
            ]),
        );
        assert_eq!(
            compose_live_actor_inputs(&two, &central, &project)
                .unwrap_err()
                .code(),
            "actor_composition.ambiguous_profile"
        );

        // Zero profiles: same, nothing is invented.
        let zero = ScriptedRunner::new().on("agent-profile.list", &list_data(vec![]));
        assert!(compose_live_actor_inputs(&zero, &central, &project)
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(&central).unwrap();
    }

    #[test]
    fn non_central_project_is_not_mapped_to_a_member() {
        assert!(project_member(
            Path::new("/home/me/Central"),
            Path::new("/elsewhere/project")
        )
        .is_none());
        assert_eq!(
            project_member(
                Path::new("/home/me/Central"),
                Path::new("/home/me/Central/Work/example")
            ),
            Some("example".to_string())
        );
    }
}
