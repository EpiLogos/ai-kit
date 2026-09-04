//! Live fetch + composition of the Actuation and Central projections.
//!
//! The intakes (`ActuationModelBearingProjection`, `CentralAgentProfileProjection`)
//! are pure deserializers. This module is the only place that fetches them from
//! their native surfaces and composes the resolution inputs. Discovery is
//! explicit and never guesses: a Central profile is used only when exactly one
//! resolves for the Project, and the Actuation receipt is read from a known
//! authored file. Absence is a valid "no projection" state, never a failure.

use std::path::Path;

use aikit_core::context_resolution::RequestedActors;
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::actuation_model_bearing::{
    compose_actor_inputs, ActuationModelBearingProjection, ComposedActorInputs,
};
use crate::central_agent_profile::CentralAgentProfileProjection;
use crate::runner::CommandRunner;

/// Authored Actuation model-bearing receipt, relative to the Project root.
pub const ACTUATION_MODEL_BEARING_FILE: &str = ".aikit/actuation-model-bearing.json";

const AGENT_PROFILE_LIST: &str = "agent-profile.list";

/// Compose the live actor inputs for a Project. Returns `None` when neither a
/// Central-authored profile nor an Actuation model-bearing receipt is present —
/// a valid "no projection" state, never a failure.
pub fn compose_live_actor_inputs<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project_root: &Path,
) -> Result<Option<ComposedActorInputs>> {
    let central = read_project_agent_profile(runner, central_root, project_root)?
        .map(|profile| profile.authored_projection())
        .unwrap_or_default();
    let actuation = read_actuation_model_bearing(project_root)?;

    match actuation {
        Some(actuation) => Ok(Some(compose_actor_inputs(&actuation, &central))),
        None if central.agent_ref.is_some() || !central.profile_refs.is_empty() => {
            Ok(Some(ComposedActorInputs {
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
) -> Result<Option<CentralAgentProfileProjection>> {
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
    let [entry] = profiles.as_slice() else {
        return Ok(None);
    };
    let source = entry.get("profile").ok_or_else(|| {
        AikitError::new(
            "actor_composition.invalid_profile_listing",
            "Central agent-profile.list returned an entry without a profile",
        )
    })?;
    Ok(Some(CentralAgentProfileProjection::parse(source)?))
}

/// Read an authored Actuation model-bearing receipt. Absence is `None`, never an
/// error and never a synthesized model-bearing object.
fn read_actuation_model_bearing(
    project_root: &Path,
) -> Result<Option<ActuationModelBearingProjection>> {
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
    Ok(Some(ActuationModelBearingProjection::parse(&value)?))
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
        return Err(AikitError::new("actor_composition.central_action_failed", message));
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
    use aikit_core::resource::ResourceRef;
    use crate::runner::ScriptedRunner;

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
        let central = std::env::temp_dir().join(format!(
            "actor-central-{}-{nonce}",
            std::process::id()
        ));
        let project = central.join("Work/example");
        std::fs::create_dir_all(project.join(".aikit")).unwrap();
        (central, project)
    }

    #[test]
    fn composes_project_profile_and_model_bearing_into_actor_inputs() {
        let (central, project) = temp_project();
        let runner = ScriptedRunner::new().on(
            "agent-profile.list",
            &list_data(vec![json!({ "source_path": "p.json", "profile": profile() })]),
        );
        std::fs::write(
            project.join(ACTUATION_MODEL_BEARING_FILE),
            model_bearing().to_string(),
        )
        .unwrap();

        let composed = compose_live_actor_inputs(&runner, &central, &project).unwrap().unwrap();
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
        assert_eq!(composed.agent_session, Some("agent-session/codex-7".to_string()));
        std::fs::remove_dir_all(&central).unwrap();
    }

    #[test]
    fn model_bearing_absence_yields_central_only_slice() {
        let (central, project) = temp_project();
        let runner = ScriptedRunner::new().on(
            "agent-profile.list",
            &list_data(vec![json!({ "source_path": "p.json", "profile": profile() })]),
        );

        let composed = compose_live_actor_inputs(&runner, &central, &project).unwrap().unwrap();
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
        assert!(compose_live_actor_inputs(&two, &central, &project).unwrap().is_none());

        // Zero profiles: same, nothing is invented.
        let zero = ScriptedRunner::new().on("agent-profile.list", &list_data(vec![]));
        assert!(compose_live_actor_inputs(&zero, &central, &project).unwrap().is_none());
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
