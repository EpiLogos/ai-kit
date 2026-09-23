//! Cline adapter: the harness-admission contract for the Cline CLI
//! (`cline`), admitted 2026-09-23 as a docs-level census from the connection
//! truth cards' expansion shortlist #3.
//!
//! The census is docs-level: Cline is not installed on this machine.

use aikit_adapters::clients::cline::{ClineAdapter, ADAPTER_REF, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, TargetAdapter};

mod common;

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = ClineAdapter::new("/tmp/cline-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "cline");
    assert_eq!(PRODUCT, "Cline");
    assert_eq!(ADAPTER_REF, "aikit:cline-adapter");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = ClineAdapter::new("/tmp/cline-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "cline");
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Not installed on this machine: no honest native_version is claimed.
    assert!(admission.native_version.is_none());
    // No Actuation descriptor exists for this slug; no actuation is claimed.
    assert!(admission.realised_actuation_ref.is_none());

    admission.validate().expect("admission must validate");

    // The census covers all 15 faculties explicitly, and unverified ones are
    // Unknown — never negatives claimed from silence.
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
    // No MCP surface is in the fact base: Unknown, not Unsupported.
    let tool_protocol = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::ToolProtocol)
        .unwrap();
    assert_eq!(tool_protocol.support, FacultySupport::Unknown);
}

#[test]
fn census_asserts_the_documented_acp_auth_and_headless_facts() {
    let adapter = ClineAdapter::new("/tmp/cline-projection");
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

    // The documented facts: first-party `cline --acp` with the optional
    // `--auto-approve true` permission bypass explicitly kept out of the
    // declared door; no batch headless JSON mode documented.
    let surfaces = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::Surfaces)
        .unwrap();
    assert_eq!(surfaces.support, FacultySupport::Supported);
    let note = surfaces.note.as_deref().unwrap();
    assert!(note.contains("cline --acp"));
    assert!(note.contains("no batch headless JSON"));

    let acp_evidence = surfaces
        .evidence_refs
        .iter()
        .find(|e| e.contains("--acp"))
        .expect("the ACP face must be cited");
    assert!(
        acp_evidence.contains("--auto-approve"),
        "the permission-bypass flag must be named and disclaimed"
    );
}

#[test]
fn profile_declares_the_acp_door_with_provider_plural_models() {
    let profile =
        aikit_adapters::profiles::for_slug("cline").expect("cline profile must be embedded");
    assert_eq!(profile.slug, "cline");

    let sessions = profile.sessions.as_ref().expect("sessions layer declared");
    assert_eq!(
        sessions.protocol,
        aikit_core::harness_profile::SessionProtocol::Acp
    );
    // The ACP door is documented: `cline --acp` (no auto-approve in the door).
    let connect = sessions.connect.as_ref().expect("acp requires a door");
    assert_eq!(connect.argv, vec!["cline".to_string(), "--acp".to_string()]);
    // ACP baseline capabilities only; everything else unverified.
    assert!(sessions.capabilities.ordered_streaming);
    assert!(sessions.capabilities.cancellation);
    assert!(!sessions.capabilities.permission_requests);
    assert!(!sessions.capabilities.reconnect);
    assert!(!sessions.capabilities.mcp_servers);
    // No resume face documented: only create.
    assert_eq!(sessions.open_modes, vec!["create".to_string()]);

    // Provider/model selection via env vars is documented: provider-plural
    // with the roster note carrying the evidence.
    let models = profile.models.as_ref().expect("models layer declared");
    assert_eq!(
        models.dispatch,
        aikit_core::harness_profile::ModelDispatchPosture::ProviderPlural
    );
    let roster_note = models.roster_note.as_deref().expect("roster note carried");
    assert!(roster_note.contains("CLINE_PROVIDER"));
    assert!(roster_note.contains("CLINE_MODEL"));
}

#[test]
fn adapter_plan_posture_is_honest_no_managed_projection_claims() {
    let adapter = ClineAdapter::new("/tmp/cline-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(caps.brokered_fallback);

    let rc = common::ContextBuilder::new()
        .project_skill(
            "skill/cline-probe",
            "a skill the brokered census must not claim to project",
            "/tmp/cline-projection-tree",
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
