//! Qoder adapter: the harness-admission contract for the Qoder CLI
//! (`qoder`), admitted 2026-09-23 as a docs-level census from the connection
//! truth cards' expansion shortlist #9.
//!
//! The census is docs-level: Qoder is not installed on this machine.

use aikit_adapters::clients::qoder::{QoderAdapter, ADAPTER_REF, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, TargetAdapter};

mod common;

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = QoderAdapter::new("/tmp/qoder-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "qoder");
    assert_eq!(PRODUCT, "Qoder CLI");
    assert_eq!(ADAPTER_REF, "aikit:qoder-adapter");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = QoderAdapter::new("/tmp/qoder-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "qoder");
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
    // Headless is undocumented and nothing is installed: lifecycle faculties
    // are honestly Unknown.
    let live_reload = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::LiveReload)
        .unwrap();
    assert_eq!(live_reload.support, FacultySupport::Unknown);
}

#[test]
fn census_asserts_the_documented_acp_and_auth_facts() {
    // Every evidence ref must be a docs citation — nothing is machine-observed.
    let adapter = QoderAdapter::new("/tmp/qoder-projection");
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

    // The documented facts: first-party `qoder --acp`, auth `qoder login` or
    // QODER_PERSONAL_ACCESS_TOKEN, headless undocumented.
    let surfaces = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::Surfaces)
        .unwrap();
    assert_eq!(surfaces.support, FacultySupport::Supported);
    let note = surfaces.note.as_deref().unwrap();
    assert!(note.contains("qoder --acp"));
    assert!(note.contains("no documented headless face"));
    assert!(surfaces
        .evidence_refs
        .iter()
        .any(|e| e.contains("QODER_PERSONAL_ACCESS_TOKEN")));
}

#[test]
fn profile_declares_the_acp_door_with_baseline_capabilities_only() {
    let profile =
        aikit_adapters::profiles::for_slug("qoder").expect("qoder profile must be embedded");
    assert_eq!(profile.slug, "qoder");

    let sessions = profile.sessions.as_ref().expect("sessions layer declared");
    assert_eq!(
        sessions.protocol,
        aikit_core::harness_profile::SessionProtocol::Acp
    );
    // The ACP door is documented: `qoder --acp`.
    let connect = sessions.connect.as_ref().expect("acp requires a door");
    assert_eq!(connect.argv, vec!["qoder".to_string(), "--acp".to_string()]);
    // Baseline capabilities only — everything else false/unverified.
    assert!(sessions.capabilities.ordered_streaming);
    assert!(sessions.capabilities.cancellation);
    assert!(!sessions.capabilities.permission_requests);
    assert!(!sessions.capabilities.reconnect);
    assert!(!sessions.capabilities.mcp_servers);
    assert!(!sessions.capabilities.additional_directories);
    // Headless undocumented, resume unverified: only create.
    assert_eq!(sessions.open_modes, vec!["create".to_string()]);
}

#[test]
fn adapter_plan_posture_is_honest_no_managed_projection_claims() {
    let adapter = QoderAdapter::new("/tmp/qoder-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(caps.brokered_fallback);

    let rc = common::ContextBuilder::new()
        .project_skill(
            "skill/qoder-probe",
            "a skill the brokered census must not claim to project",
            "/tmp/qoder-projection-tree",
        )
        .build();
    let plan = adapter.plan(&rc).expect("plan must build");
    assert!(
        matches!(plan.effect, ActivationEffect::Brokered { .. }),
        "a docs-level census with no documented projection seam must stay brokered, got {:?}",
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
