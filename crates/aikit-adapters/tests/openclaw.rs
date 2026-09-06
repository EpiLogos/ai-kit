//! OpenClaw adapter: the harness-admission contract for OpenClaw, the
//! CLI+gateway agent harness (2026.1.30).
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to OpenClaw — rather than re-testing the shared projection
//! machinery already covered by the core harness-admission suite.

use aikit_adapters::clients::openclaw::{OpenclawAdapter, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HarnessFaculty, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = OpenclawAdapter::new("/tmp/openclaw-projection");
    assert_eq!(adapter.target().as_str(), "openclaw");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = OpenclawAdapter::new("/tmp/openclaw-projection");
    let caps = adapter.capabilities();
    // Reload is per-session (AGENTS.md "Every Session"), not a file watch.
    assert!(!caps.live_reload);
    assert!(!caps.watches_for_changes);
    // Named profiles and isolated agents give per-context state isolation.
    assert!(caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = OpenclawAdapter::new("/tmp/openclaw-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    assert_eq!(
        admission.native_version.as_deref(),
        Some("2026.1.30 (76b5208)")
    );
    assert_eq!(admission.source_revision.as_deref(), Some("76b5208"));
    // Bound to Actuation's detection identity; AIKit consumes the ref.
    assert_eq!(
        admission.realised_actuation_ref.as_deref(),
        Some("harness/openclaw")
    );

    // The census covers all 15 faculties explicitly.
    assert_eq!(admission.faculties.len(), 15);
    for faculty in [
        HarnessFaculty::StandingInstructions,
        HarnessFaculty::ProjectInstructions,
        HarnessFaculty::NativeSkills,
        HarnessFaculty::SessionStartHook,
        HarnessFaculty::LiveReload,
        HarnessFaculty::NextSessionReload,
        HarnessFaculty::RestartReload,
        HarnessFaculty::ToolProtocol,
        HarnessFaculty::NativeToolContribution,
        HarnessFaculty::SessionResume,
        HarnessFaculty::DelegatedAgents,
        HarnessFaculty::ProjectRoots,
        HarnessFaculty::Components,
        HarnessFaculty::Surfaces,
        HarnessFaculty::LiveRetraction,
    ] {
        assert!(
            admission.faculty(faculty).is_some(),
            "{faculty:?} must be censused explicitly"
        );
    }

    // Faculties observed natively on this machine are Supported with evidence,
    // and the evidence includes the Actuation detection record.
    let standing = admission
        .faculty(HarnessFaculty::StandingInstructions)
        .expect("standing instructions censused");
    assert_eq!(standing.support, FacultySupport::Supported);
    assert!(
        standing
            .evidence_refs
            .iter()
            .any(|r| r.starts_with("native:/Users/admin/.openclaw/workspace/AGENTS.md")),
        "standing instructions must cite the workspace AGENTS.md observed on disk"
    );
    let surfaces = admission
        .faculty(HarnessFaculty::Surfaces)
        .expect("surfaces censused");
    assert_eq!(surfaces.support, FacultySupport::Supported);
    assert!(
        surfaces
            .evidence_refs
            .iter()
            .any(|r| r.starts_with("actuation.harness-detection/v1")),
        "surfaces must cite the actuation detection record"
    );
    // The one genuinely-unverified faculty is honestly Unknown, not invented.
    assert_eq!(
        admission
            .faculty(HarnessFaculty::LiveReload)
            .expect("live reload censused")
            .support,
        FacultySupport::Unknown
    );

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
    let adapter = OpenclawAdapter::new("/tmp/openclaw-projection");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::brokered("brokered projection"),
    );
    assert!(
        matches!(plan.effect, ActivationEffect::Brokered { .. }),
        "the adapter revision brokers projection onto user-authored workspace files"
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
