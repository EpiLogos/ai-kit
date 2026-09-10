//! Cursor CLI adapter: the harness-admission contract for Cursor's terminal agent.
//!
//! Focused on the admission census, the native project-rule projection, and the
//! identity law — the parts of the contract that are specific to Cursor CLI —
//! rather than re-testing the shared projection machinery already covered by the
//! core harness-admission suite.

use aikit_adapters::clients::cursor::{CursorAdapter, PRODUCT, RULE_PATH};
use aikit_core::harness_admission::{
    FacultySupport, HARNESS_ADAPTER_SDK_VERSION, HarnessActivationObservation,
    HarnessActivationState, HarnessAdmissionAdapter, verify_activation_truth,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionItem, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = CursorAdapter::new("/tmp/cursor-projection");
    assert_eq!(adapter.target().as_str(), "cursor-cli");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = CursorAdapter::new("/tmp/cursor-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(!caps.watches_for_changes);
    assert!(!caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = CursorAdapter::new("/tmp/cursor-projection");
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
fn loaded_activation_overclaims_a_next_session_plan() {
    let adapter = CursorAdapter::new("/tmp/cursor-projection");
    // The real plan projects the documented `.cursor/rules/*.mdc` surface and is
    // honestly NextSessionOnly; observing it as Loaded must be rejected.
    let item = ProjectionItem::write(RULE_PATH, "rule contents").expect("valid write item");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::next_session_only("rule files are discovered at session start"),
    )
    .with_item(item);
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
        "a next-session plan must never be observed as Loaded"
    );
}
