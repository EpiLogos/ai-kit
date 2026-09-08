//! Explicit native Central acceptance: no scripted runner or fabricated CLI reply.
//! Requires a real `ctrl` on PATH. All writes are confined to a temporary ground.
use std::fs;
use std::path::Path;
use std::process::Command;

use aikit_adapters::actor_composition::{compose_live_actor_inputs, ACTUATION_MODEL_BEARING_FILE};
use aikit_adapters::central_agent_profile::CentralAgentProfileProjection;
use aikit_adapters::runner::SystemRunner;
use serde_json::{json, Value};

fn central(root: &Path, arguments: &[&str]) -> Value {
    let result = Command::new("ctrl")
        .arg("--json")
        .arg("--root")
        .arg(root)
        .args(arguments)
        .output()
        .expect("native ctrl must be installed for this explicit acceptance test");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).expect("native Central JSON");
    assert_eq!(value["ok"], true, "{value}");
    value
}

#[test]
#[ignore = "requires installed native Central ctrl; writes only temporary ground"]
fn native_profile_revision_and_assignments_survive_both_composition_paths() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("Central");
    central(&root, &["init"]);
    let project = root.join("Work/acceptance");
    fs::create_dir_all(project.join("ProjectCentral/user")).unwrap();
    fs::create_dir_all(project.join(".aikit")).unwrap();
    fs::write(
        project.join("ProjectCentral/project.json"),
        json!({
            "schema": "central.project/v1", "project_id": "project:acceptance",
            "human_source": "ProjectCentral/user",
            "wiki": {"profile": "okf-wiki/v1", "source": "ProjectCentral/agents/wiki/wiki.json"}
        })
        .to_string(),
    )
    .unwrap();
    // Real local source material; it is generated test ground, never human adoption.
    fs::write(
        project.join("ProjectCentral/user/intent.md"),
        "Inspect the temporary artifact and return evidence.\n",
    )
    .unwrap();
    let mut profile = json!({
        "schema": "central.agent-profile/v1", "ref": "profile/acceptance",
        "revision": "r1", "agent_ref": "agent/acceptance", "scope": "project",
        "world_ref": "world/acceptance", "source_profile_ref": "profile/parent",
        "role": "Verifier", "purpose": "Preserve commissioned source without granting authority",
        "governance_refs": ["source/acceptance/law"],
        "skill_refs": ["skill/acceptance/verify"], "skill_set_refs": ["skill-set/acceptance"],
        "method_refs": ["method/acceptance"], "routine_refs": ["routine/acceptance"],
        "ratified_world_refs": ["world/acceptance"],
        "knowledge_source_refs": ["source/acceptance/intent"],
        "computer_access_intent_refs": ["access/acceptance"],
        "placement_intent_refs": ["placement/acceptance"], "provenance_refs": ["source/acceptance/origin"]
    });
    let save = json!({"scope":"project", "project":"acceptance", "profile":profile}).to_string();
    central(&root, &["action", "run", "agent-profile.save", &save]);
    let runner = SystemRunner::new();
    let expected = CentralAgentProfileProjection::parse(&profile).unwrap();
    let profile_only = compose_live_actor_inputs(&runner, &root, &project)
        .unwrap()
        .unwrap();
    assert_eq!(
        profile_only.authored.profile_source.as_ref(),
        Some(&expected)
    );
    assert_eq!(
        profile_only.authored.profile_refs,
        expected.authored_projection().profile_refs
    );
    assert_eq!(profile_only.source_resources.len(), 1);
    let observed = &profile_only.source_resources[0];
    assert_eq!(
        observed.descriptor.kind,
        aikit_core::resource::ResourceKind::Agent
    );
    assert!(observed.providers.is_empty());
    assert_eq!(
        observed.eligibility,
        aikit_core::resource::Eligibility::Undetermined
    );
    assert_eq!(
        observed.descriptor.sources[0].state,
        aikit_core::resource::SourceState::Available
    );
    assert_eq!(
        observed.descriptor.sources[0]
            .revision
            .as_ref()
            .unwrap()
            .as_str(),
        "r1"
    );
    assert!(profile_only.requested_actors.agency.is_none());
    assert!(profile_only.selected_harness.is_none());

    fs::write(
        project.join(ACTUATION_MODEL_BEARING_FILE),
        json!({
            "schema":"actuation.instantiation/v1", "actuation_ref":"actuation/acceptance",
            "agency_ref":"agency/acceptance", "world_binding_ref":"world-binding/acceptance",
            "harness_ref":"harness/pi", "agent_session_ref":"agent-session/acceptance",
            "model_relation":{"model_ref":"model/acceptance"}
        })
        .to_string(),
    )
    .unwrap();
    let bound = compose_live_actor_inputs(&runner, &root, &project)
        .unwrap()
        .unwrap();
    assert_eq!(bound.source_resources.len(), 2);
    let agency = &bound.source_resources[1];
    assert_eq!(
        agency.descriptor.kind,
        aikit_core::resource::ResourceKind::Agency
    );
    assert!(agency.providers.is_empty());
    assert_eq!(
        agency.eligibility,
        aikit_core::resource::Eligibility::Undetermined
    );
    assert_eq!(bound.authored, profile_only.authored);
    assert_eq!(
        bound.requested_actors.agent,
        profile_only.requested_actors.agent
    );
    assert!(bound.requested_actors.agency.is_some());

    // A real owner CAS update changes the retained source revision and category,
    // rather than leaving consumers with a stale flattened profile assignment.
    profile["revision"] = json!("r2");
    profile["governance_refs"] = json!(["source/acceptance/revised-law"]);
    let update = json!({"scope":"project", "project":"acceptance", "profile":profile,
        "expected_revision":"r1"})
    .to_string();
    central(&root, &["action", "run", "agent-profile.save", &update]);
    let revised = compose_live_actor_inputs(&runner, &root, &project)
        .unwrap()
        .unwrap();
    assert_eq!(
        revised.authored.profile_source,
        Some(CentralAgentProfileProjection::parse(&profile).unwrap())
    );
    assert_eq!(
        bound.authored.profile_source.as_ref().unwrap().revision,
        "r1"
    );
    assert_eq!(
        revised.source_resources[0].descriptor.sources[0]
            .revision
            .as_ref()
            .unwrap()
            .as_str(),
        "r2"
    );
    let mut second = profile.clone();
    second["ref"] = json!("profile/another");
    central(
        &root,
        &[
            "action",
            "run",
            "agent-profile.save",
            &json!({"scope":"project","project":"acceptance","profile":second}).to_string(),
        ],
    );
    let ambiguous = compose_live_actor_inputs(&runner, &root, &project).unwrap_err();
    assert_eq!(ambiguous.code(), "actor_composition.ambiguous_profile");
}
