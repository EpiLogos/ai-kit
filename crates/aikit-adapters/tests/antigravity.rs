//! Google Antigravity adapter: the harness-admission contract for the
//! Antigravity IDE harness.
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to Antigravity — rather than re-testing the shared
//! projection machinery already covered by the core harness-admission suite.

use aikit_adapters::clients::antigravity::{AntigravityAdapter, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = AntigravityAdapter::new("/tmp/antigravity-projection");
    assert_eq!(adapter.target().as_str(), "gemini-antigravity");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = AntigravityAdapter::new("/tmp/antigravity-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(!caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
    assert!(!caps.watches_for_changes);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = AntigravityAdapter::new("/tmp/antigravity-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.edition, HarnessEditionKind::Ide);
    // The IDE app bundle is absent and the detection record carries no
    // version probe for this slug, so no native version is claimed.
    assert!(admission.native_version.is_none());
    // A model running in Antigravity is not the Agent identity; no actuation is claimed.
    assert!(admission.realised_actuation_ref.is_none());

    admission.validate().expect("admission must validate");

    assert_eq!(
        admission.faculties.len(),
        15,
        "census must cover every HarnessFaculty variant exactly once"
    );
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
    let adapter = AntigravityAdapter::new("/tmp/antigravity-projection");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::brokered("IDE-managed lifecycle; brokered projection"),
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
