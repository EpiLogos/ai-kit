//! Ollama adapter: the harness-admission contract for Ollama, censused as what
//! it actually is — a local model runtime (cli+service), not an agentic
//! coding harness.
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to Ollama — rather than re-testing the shared projection
//! machinery already covered by the core harness-admission suite.

use aikit_adapters::clients::ollama::{OllamaAdapter, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HarnessFaculty, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = OllamaAdapter::new("/tmp/ollama-projection");
    assert_eq!(adapter.target().as_str(), "ollama");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = OllamaAdapter::new("/tmp/ollama-projection");
    let caps = adapter.capabilities();
    // A model server has no reloadable authored surface and no watch.
    assert!(!caps.live_reload);
    assert!(!caps.watches_for_changes);
    assert!(!caps.isolated_per_context);
    assert!(caps.brokered_fallback);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = OllamaAdapter::new("/tmp/ollama-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    // Catalog descriptor edition is "cli+service" (model runtime); Custom is
    // the honest HarnessEditionKind mapping.
    assert_eq!(admission.edition, HarnessEditionKind::Custom);
    assert_eq!(admission.native_version.as_deref(), Some("0.12.6"));
    // Bound to Actuation's detection identity; AIKit consumes the ref.
    assert_eq!(
        admission.realised_actuation_ref.as_deref(),
        Some("harness/ollama")
    );

    // The census covers all 15 faculties explicitly — a model server must say
    // Unsupported with reasons rather than pad the census.
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
    // The one Degraded entry (Surfaces) is the HTTP API / REPL, and it cites
    // the detection record.
    let surfaces = admission
        .faculty(HarnessFaculty::Surfaces)
        .expect("surfaces censused");
    assert_eq!(surfaces.support, FacultySupport::Degraded);
    assert!(
        surfaces
            .evidence_refs
            .iter()
            .any(|r| r.starts_with("actuation.harness-detection/v1")),
        "surfaces must cite the actuation detection record"
    );
    // Every instruction/session faculty is honestly Unsupported, not silently
    // absent or overclaimed.
    for faculty in [
        HarnessFaculty::StandingInstructions,
        HarnessFaculty::ProjectInstructions,
        HarnessFaculty::SessionStartHook,
        HarnessFaculty::NextSessionReload,
        HarnessFaculty::SessionResume,
        HarnessFaculty::DelegatedAgents,
    ] {
        assert_eq!(
            admission.faculty(faculty).expect("censused").support,
            FacultySupport::Unsupported,
            "{faculty:?} is not a faculty a model server has"
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
    let adapter = OllamaAdapter::new("/tmp/ollama-projection");
    let plan = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::brokered("brokered projection"),
    );
    assert!(
        matches!(plan.effect, ActivationEffect::Brokered { .. }),
        "a model server with no readable instruction surface must broker"
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
