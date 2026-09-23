//! Copilot adapter: the harness-admission contract for GitHub Copilot CLI
//! (`copilot`), admitted 2026-09-23 as a docs-level census from the connection
//! truth cards' expansion shortlist #1.
//!
//! Focused on the admission census and identity law — the parts of the
//! contract that are specific to GitHub Copilot CLI — rather than re-testing
//! the shared projection machinery already covered by the core
//! harness-admission suite. The census is docs-level: the Copilot CLI is not
//! installed on this machine.

use aikit_adapters::clients::copilot::{CopilotAdapter, ADAPTER_REF, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, TargetAdapter};

mod common;

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = CopilotAdapter::new("/tmp/copilot-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "copilot");
    assert_eq!(PRODUCT, "GitHub Copilot CLI");
    assert_eq!(ADAPTER_REF, "aikit:copilot-adapter");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = CopilotAdapter::new("/tmp/copilot-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "copilot");
    // The binary is a plain CLI per docs.
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Not installed on this machine: no honest native_version is claimed.
    assert!(admission.native_version.is_none());
    // No Actuation descriptor exists for this slug; no actuation is claimed.
    assert!(admission.realised_actuation_ref.is_none());

    admission.validate().expect("admission must validate");

    // The census covers all 15 faculties explicitly.
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
    // Not installed: nothing is machine-observed, so lifecycle faculties are
    // honestly Unknown, not invented.
    let live_reload = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::LiveReload)
        .unwrap();
    assert_eq!(live_reload.support, FacultySupport::Unknown);
}

#[test]
fn census_asserts_the_documented_acp_and_mcp_facts() {
    // Every evidence ref must be a docs citation — the machine-observed
    // prefixes (native:, npm:, actuation records) would overclaim for a
    // harness this machine does not have installed.
    let adapter = CopilotAdapter::new("/tmp/copilot-projection");
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

    // The documented facts: first-party ACP face `copilot --acp` (public
    // preview 2026-01-28) on the Surfaces census; MCP via session/new
    // mcpServers with no file-based config documented, so the tool seam is
    // degraded — declared, never a verified config path.
    let surfaces = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::Surfaces)
        .unwrap();
    assert_eq!(surfaces.support, FacultySupport::Supported);
    let surfaces_note = surfaces.note.as_deref().unwrap();
    assert!(surfaces_note.contains("copilot --acp"));
    assert!(surfaces_note.contains("preview"));

    let tool_protocol = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::ToolProtocol)
        .unwrap();
    assert_eq!(tool_protocol.support, FacultySupport::Degraded);
    let tool_note = tool_protocol.note.as_deref().unwrap();
    assert!(tool_note.contains("session/new"));
    assert!(tool_note.contains("no file-based MCP config"));
}

#[test]
fn profile_declares_the_acp_door_with_baseline_capabilities_only() {
    let profile =
        aikit_adapters::profiles::for_slug("copilot").expect("copilot profile must be embedded");
    assert_eq!(profile.slug, "copilot");

    let sessions = profile.sessions.as_ref().expect("sessions layer declared");
    assert_eq!(
        sessions.protocol,
        aikit_core::harness_profile::SessionProtocol::Acp
    );
    // The ACP door is documented: `copilot --acp`.
    let connect = sessions.connect.as_ref().expect("acp requires a door");
    assert_eq!(
        connect.argv,
        vec!["copilot".to_string(), "--acp".to_string()]
    );
    // ordered-streaming/cancellation are the ACP baseline; every other
    // capability stays false — unverified, not claimed.
    assert!(sessions.capabilities.ordered_streaming);
    assert!(sessions.capabilities.cancellation);
    assert!(!sessions.capabilities.permission_requests);
    assert!(!sessions.capabilities.reconnect);
    assert!(!sessions.capabilities.mcp_servers);
    assert!(!sessions.capabilities.additional_directories);
    // No resume face is documented: only create.
    assert_eq!(sessions.open_modes, vec!["create".to_string()]);

    // Model dispatch is undeclared with its reason carried: preview; the
    // BYOK/auth split is undocumented.
    let models = profile.models.as_ref().expect("models layer declared");
    match &models.dispatch {
        aikit_core::harness_profile::ModelDispatchPosture::None { reason } => {
            assert!(
                reason.contains("preview"),
                "reason must name the posture: {reason}"
            );
        }
        other => panic!("copilot dispatch must be none, got {other:?}"),
    }
}

#[test]
fn adapter_plan_posture_is_honest_no_managed_projection_claims() {
    let adapter = CopilotAdapter::new("/tmp/copilot-projection");
    let caps = adapter.capabilities();
    // Docs-level census: no verified projection surface, so nothing claims a
    // live faculty; the plan is brokered.
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(caps.brokered_fallback);

    let rc = common::ContextBuilder::new()
        .project_skill(
            "skill/copilot-probe",
            "a skill the brokered census must not claim to project",
            "/tmp/copilot-projection-tree",
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
