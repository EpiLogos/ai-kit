//! Pi adapter: the harness-admission contract for Mario Zechner's pi agent.
//!
//! Focused on the admission census and identity law — the parts of the contract
//! that are specific to pi — rather than re-testing the shared projection
//! machinery already covered by the core harness-admission suite.

mod common;

use aikit_adapters::clients::pi::{PiAdapter, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = PiAdapter::new("/tmp/pi-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "pi");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn capabilities_are_described_not_default() {
    let adapter = PiAdapter::new("/tmp/pi-projection");
    let caps = adapter.capabilities();
    // pi's TUI `/reload` picks up changed skills/extensions/prompts.
    assert!(caps.live_reload);
    assert!(caps.symlinks);
    // The projected surface is project-relative `.pi/skills/`, so per-context
    // isolation genuinely requires an isolated working tree.
    assert!(!caps.isolated_per_context);
    assert!(caps.requires_isolated_tree_for_isolation);
    assert!(caps.brokered_fallback);
    assert!(!caps.watches_for_changes);
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = PiAdapter::new("/tmp/pi-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "pi");
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Native version observed from the installed binary and the detection record.
    assert_eq!(admission.native_version.as_deref(), Some("0.84.4"));
    // A model running in pi is not the Agent identity; no actuation is claimed
    // (Actuation declared no capability descriptor for pi at admission time).
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
}

#[test]
fn loaded_activation_overclaims_a_next_session_plan() {
    // The real plan, built against a resolved context with one project skill:
    // pi provably discovers `.pi/skills/`, so the plan writes real items.
    let registry = tempfile::tempdir().unwrap();
    let capsule_root = registry.path().join("skill/rust/code-review");
    common::write_payload_skill(
        &capsule_root,
        "code-review",
        "Reviews Rust for correctness.",
    );
    let context = common::ContextBuilder::new()
        .project_skill(
            "skill/rust/code-review",
            "Reviews Rust for correctness.",
            &capsule_root,
        )
        .build();

    let adapter = PiAdapter::new("/tmp/pi-projection");
    let plan = adapter.plan(&context).expect("plan must build");
    assert!(
        matches!(plan.effect, ActivationEffect::NextSessionOnly { .. }),
        "pi reads .pi/skills at session start; the plan must not claim immediate activation"
    );
    assert!(
        plan.items.iter().any(|item| {
            item.destination()
                .map(|path| path.display().to_string().starts_with(".pi/skills/"))
                .unwrap_or(false)
        }),
        "the plan must write skill material under .pi/skills/"
    );

    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: adapter.target(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec!["test:materialized-on-disk".to_string()],
        native_revision: None,
        note: None,
    };
    assert!(
        verify_activation_truth(&plan, &observation).is_err(),
        "a next-session plan must never be observed as Loaded, even with evidence refs"
    );

    // Honest variant: the state the plan actually promises is accepted.
    let honest = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: adapter.target(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::NextSession,
        evidence_refs: vec!["native:pi session start reads .pi/skills".to_string()],
        native_revision: Some("0.84.4".to_string()),
        note: None,
    };
    verify_activation_truth(&plan, &honest).expect("NextSession is the truthful observation");

    // The dsh-shape manual construction stays rejected too.
    let bare = ProjectionPlan::new(
        adapter.target(),
        ActivationEffect::next_session_only("session-start pickup"),
    );
    let loaded = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: adapter.target(),
        projection_digest: bare.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec![],
        native_revision: None,
        note: None,
    };
    assert!(verify_activation_truth(&bare, &loaded).is_err());
}
