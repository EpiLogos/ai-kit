//! Gemini CLI adapter: the harness-admission contract for Gemini CLI.
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to Gemini CLI — rather than re-testing the shared
//! projection machinery already covered by the core harness-admission suite.

use aikit_adapters::clients::gemini::{GeminiAdapter, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HarnessFaculty, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = GeminiAdapter::new("/tmp/gemini-projection");
    assert_eq!(adapter.target().as_str(), "gemini-cli");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = GeminiAdapter::new("/tmp/gemini-projection");
    let caps = adapter.capabilities();
    // Reload is manual (/memory, /skills, /commands reload); no file watching.
    assert!(!caps.live_reload);
    assert!(!caps.watches_for_changes);
    assert!(!caps.isolated_per_context);
    assert!(!caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = GeminiAdapter::new("/tmp/gemini-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Observed locally: `gemini --version` = 0.29.5.
    assert_eq!(admission.native_version.as_deref(), Some("0.29.5"));
    // The adapter may retain a stable realised-actuation ref bound from
    // Actuation's detection record (harness/gemini); if present it must be a
    // real non-empty ref, never a fabricated identity.
    if let Some(reference) = admission.realised_actuation_ref.as_deref() {
        assert!(!reference.trim().is_empty());
        assert_eq!(reference, "harness/gemini");
    }

    // All 15 faculties are censused, none silently dropped.
    assert_eq!(admission.faculties.len(), 15);
    for variant in [
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
            admission.faculty(variant).is_some(),
            "{variant:?} must be censused"
        );
    }

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
    let adapter = GeminiAdapter::new("/tmp/gemini-projection");
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
