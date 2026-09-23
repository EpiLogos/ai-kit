//! Droid adapter: the harness-admission contract for Factory Droid
//! (`droid`), admitted 2026-09-23 as a docs-level census from the connection
//! truth cards' expansion shortlist #2.
//!
//! The census is docs-level: Droid is not installed on this machine. The
//! connection posture is the point of the census: the headless runner is the
//! first-party face, and no spawn-and-speak ACP door is claimed.

use aikit_adapters::clients::droid::{DroidAdapter, ADAPTER_REF, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, TargetAdapter};

mod common;

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = DroidAdapter::new("/tmp/droid-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "droid");
    assert_eq!(PRODUCT, "Factory Droid");
    assert_eq!(ADAPTER_REF, "aikit:droid-adapter");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = DroidAdapter::new("/tmp/droid-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "droid");
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Not installed on this machine: no honest native_version is claimed.
    assert!(admission.native_version.is_none());
    // No Actuation descriptor exists for this slug; no actuation is claimed.
    assert!(admission.realised_actuation_ref.is_none());

    admission.validate().expect("admission must validate");

    use aikit_core::harness_admission::HarnessFaculty::*;
    let all = [
        StandingInstructions,
        ProjectInstructions,
        NativeSkills,
        SessionStartHook,
        LiveReload,
        NextSessionReload,
        RestartReload,
        ToolProtocol,
        NativeToolContribution,
        SessionResume,
        DelegatedAgents,
        ProjectRoots,
        Components,
        Surfaces,
        LiveRetraction,
    ];
    for faculty in all {
        assert!(
            admission.faculty(faculty).is_some(),
            "census must explicitly record {faculty:?}"
        );
    }
    // The MCP config paths are documented, so ToolProtocol is supported with
    // the never-verified-live caveat carried in the note.
    let tool_protocol = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::ToolProtocol)
        .unwrap();
    assert_eq!(tool_protocol.support, FacultySupport::Supported);
    let tool_note = tool_protocol.note.as_deref().unwrap();
    assert!(tool_note.contains("mcpServers"));
    assert!(tool_note.contains("never verified"));
}

#[test]
fn census_asserts_the_headless_face_and_refuses_an_acp_door() {
    // Every evidence ref must be a docs citation — nothing is machine-observed.
    let adapter = DroidAdapter::new("/tmp/droid-projection");
    let admission = adapter.admission();
    for faculty in &admission.faculties {
        assert!(
            !faculty.evidence_refs.is_empty(),
            "{:?} must cite its evidence even at docs level",
            faculty.faculty
        );
        for evidence in &faculty.evidence_refs {
            assert!(
                evidence.starts_with("docs:"),
                "{:?} evidence must be a docs citation, got: {evidence}",
                faculty.faculty
            );
        }
    }

    // The documented facts: the headless runner is the first-party family,
    // and the ACP registry's acp-daemon argv is named as unverified — no ACP
    // door is declared. The stream-jsonrpc family is the named future lane.
    let surfaces = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::Surfaces)
        .unwrap();
    assert_eq!(surfaces.support, FacultySupport::Supported);
    let note = surfaces.note.as_deref().unwrap();
    assert!(note.contains("droid exec --output-format"));
    assert!(note.contains("NO spawn-and-speak ACP face"));
    assert!(note.contains("future connection lane"));
    let acp_evidence = surfaces
        .evidence_refs
        .iter()
        .find(|e| e.contains("acp-daemon"))
        .expect("the acp-daemon registry entry must be cited as unverified");
    assert!(acp_evidence.contains("unverified"));
}

#[test]
fn profile_declares_process_protocol_with_documented_mcp_paths_and_no_door() {
    let profile =
        aikit_adapters::profiles::for_slug("droid").expect("droid profile must be embedded");
    assert_eq!(profile.slug, "droid");

    let presence = profile.presence.as_ref().expect("presence layer declared");
    assert_eq!(presence.executables, vec!["droid".to_string()]);
    assert_eq!(presence.config_dir.as_deref(), Some("~/.factory"));

    // The documented MCP client config paths are observations — disclosure,
    // never a managed projection.
    let tools = profile.tools.as_ref().expect("tools layer declared");
    assert_eq!(
        tools.posture,
        aikit_core::harness_profile::LayerPosture::Observed
    );
    assert!(
        tools.project.is_none(),
        "observed tools layers declare no project"
    );
    let paths: Vec<_> = tools.observe.iter().map(|o| o.path.as_str()).collect();
    assert!(paths.contains(&"~/.factory/mcp.json"));
    assert!(paths.contains(&".factory/mcp.json"));
    assert!(tools.observe.iter().all(|o| o.collection == "mcpServers"));

    // Process protocol, create-only, and NO connect — the stream-jsonrpc
    // framing is unpinned against an installed binary, so there is no
    // declared door yet; that family is the named future connection lane.
    let sessions = profile.sessions.as_ref().expect("sessions layer declared");
    assert_eq!(
        sessions.protocol,
        aikit_core::harness_profile::SessionProtocol::Process
    );
    assert!(sessions.connect.is_none(), "no door is declared yet");
    assert_eq!(sessions.open_modes, vec!["create".to_string()]);
}

#[test]
fn adapter_plan_posture_is_honest_no_managed_projection_claims() {
    let adapter = DroidAdapter::new("/tmp/droid-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(caps.brokered_fallback);

    let rc = common::ContextBuilder::new()
        .project_skill(
            "skill/droid-probe",
            "a skill the brokered census must not claim to project",
            "/tmp/droid-projection-tree",
        )
        .build();
    let plan = adapter.plan(&rc).expect("plan must build");
    assert!(
        matches!(plan.effect, ActivationEffect::Brokered { .. }),
        "a docs-level census with no declared managed seam must stay brokered, got {:?}",
        plan.effect
    );
    assert!(plan.items.is_empty(), "brokered plans write nothing");

    // A loaded observation would overclaim the brokered plan.
    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: adapter.target(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec![],
        native_revision: None,
        note: None,
    };
    assert!(verify_activation_truth(&plan, &observation).is_err());
}
