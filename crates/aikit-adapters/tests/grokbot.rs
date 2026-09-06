//! Grokbot adapter: the harness-admission contract for xAI's Grok Bot
//! (cli+service edition as cataloged by Actuation).
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to Grok Bot — rather than re-testing the shared projection
//! machinery already covered by the core harness-admission suite.

use aikit_adapters::clients::grokbot::{GrokbotAdapter, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = GrokbotAdapter::new("/tmp/grokbot-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "grok-bot");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = GrokbotAdapter::new("/tmp/grokbot-projection");
    let caps = adapter.capabilities();
    // The daemon was not running at detection and exposes no projection tree.
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
    assert!(!caps.watches_for_changes);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = GrokbotAdapter::new("/tmp/grokbot-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "grok-bot");
    // Catalog edition is "cli+service"; no enum variant covers it, so the
    // admission records Custom and cites the catalog descriptor as evidence.
    assert_eq!(admission.edition, HarnessEditionKind::Custom);
    // The version probe is keychain-gated and fails headless; no honest
    // native_version is claimed.
    assert!(admission.native_version.is_none());
    // A Grok Bot is not the Agent identity; no actuation is claimed
    // (Actuation declared no capability descriptor for grok-bot).
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

    // Daemon-dependent lifecycle faculties are honestly Unknown, not invented.
    let live_reload = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::LiveReload)
        .unwrap();
    assert_eq!(live_reload.support, FacultySupport::Unknown);
}

#[test]
fn loaded_activation_overclaims_a_brokered_plan() {
    let adapter = GrokbotAdapter::new("/tmp/grokbot-projection");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::brokered("brokered projection through the gateway management API"),
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
        evidence_refs: vec!["native:gbot bots update --instructions".to_string()],
        native_revision: None,
        note: None,
    };
    verify_activation_truth(&plan, &honest).expect("Brokered is the truthful observation");
}
