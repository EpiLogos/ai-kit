//! Grok adapter: the harness-admission contract for xAI's Grok Build (`grok`),
//! repurposed 2026-09-22 from the misidentified grok-bot adapter per the
//! connection truth cards' roster correction.
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to Grok Build — rather than re-testing the shared projection
//! machinery already covered by the core harness-admission suite. The census is
//! docs-level: Grok Build is not installed on this machine.

use aikit_adapters::clients::grok::{GrokAdapter, ADAPTER_REF, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = GrokAdapter::new("/tmp/grok-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "grok");
    assert_eq!(PRODUCT, "Grok Build");
    assert_eq!(ADAPTER_REF, "aikit:grok-adapter");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = GrokAdapter::new("/tmp/grok-projection");
    let caps = adapter.capabilities();
    // Docs-level census: no verified projection surface, so nothing claims a
    // live faculty; the plan is brokered.
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
    assert!(!caps.watches_for_changes);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = GrokAdapter::new("/tmp/grok-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "grok");
    // The binary is a plain CLI per docs.
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Not installed on this machine: no honest native_version is claimed.
    assert!(admission.native_version.is_none());
    // Grok Build is not the Agent identity; no actuation is claimed (the
    // Actuation catalog still describes the wrong product, grok-bot).
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

    for faculty in &admission.faculties {
        if faculty.support == FacultySupport::Supported {
            assert!(
                !faculty.evidence_refs.is_empty(),
                "{:?} must carry evidence",
                faculty.faculty
            );
        }
    }

    // Not installed: nothing is machine-observed, so lifecycle faculties are
    // honestly Unknown, not invented.
    let live_reload = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::LiveReload)
        .unwrap();
    assert_eq!(live_reload.support, FacultySupport::Unknown);
}

#[test]
fn census_is_docs_level_end_to_end() {
    // Every evidence ref must be a docs citation — the machine-observed
    // prefixes (native:, npm:, actuation records) would overclaim for a
    // harness this machine does not have installed.
    let adapter = GrokAdapter::new("/tmp/grok-projection");
    let admission = adapter.admission();
    assert!(!admission.faculties.is_empty());
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
    // The docs-declared MCP posture is degraded, never a verified seam.
    let tool_protocol = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::ToolProtocol)
        .unwrap();
    assert_eq!(tool_protocol.support, FacultySupport::Degraded);
    // Resume flags are undocumented upstream: nothing declared.
    let resume = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::SessionResume)
        .unwrap();
    assert_eq!(resume.support, FacultySupport::Unknown);
}

#[test]
fn loaded_activation_overclaims_a_brokered_plan() {
    let adapter = GrokAdapter::new("/tmp/grok-projection");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::brokered("brokered: no on-disk projection seam is documented"),
    );
    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: adapter.target(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec![],
        native_revision: None,
        note: None,
    };
    assert!(
        verify_activation_truth(&plan, &observation).is_err(),
        "a brokered plan must never be observed as Loaded"
    );

    // Honest variant: the brokered state the plan promises is accepted.
    let honest = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: adapter.target(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Brokered,
        evidence_refs: vec![
            "docs:docs.x.ai/build/overview grok -p --output-format streaming-json".to_string(),
        ],
        native_revision: None,
        note: None,
    };
    verify_activation_truth(&plan, &honest).expect("Brokered is the truthful observation");
}
