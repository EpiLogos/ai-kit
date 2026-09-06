//! Aider adapter: the harness-admission contract for the aider CLI.
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to aider — rather than re-testing the shared projection
//! machinery already covered by the core harness-admission suite.

use aikit_adapters::clients::aider::{AiderAdapter, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = AiderAdapter::new("/tmp/aider-projection");
    assert_eq!(adapter.target().as_str(), "aider");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = AiderAdapter::new("/tmp/aider-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(caps.watches_for_changes);
    assert!(!caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = AiderAdapter::new("/tmp/aider-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(
        admission.edition,
        aikit_core::harness_admission::HarnessEditionKind::Cli
    );
    // All 15 faculties are censused, and no actuation is claimed.
    assert_eq!(admission.faculties.len(), 15);
    assert!(admission.realised_actuation_ref.is_none());

    admission.validate().expect("admission must validate");

    for faculty in &admission.faculties {
        if faculty.support == FacultySupport::Supported {
            assert!(
                !faculty.evidence_refs.is_empty(),
                "{:?} must carry evidence",
                faculty.faculty
            );
        }
    }
}

#[test]
fn loaded_activation_overclaims_a_brokered_plan() {
    let adapter = AiderAdapter::new("/tmp/aider-projection");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::brokered("brokered projection"),
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
}
