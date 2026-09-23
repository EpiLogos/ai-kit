//! Kiro CLI adapter: the harness-admission contract for AWS's Kiro CLI
//! (`kiro-cli`), admitted 2026-09-23 as a docs-level census from the
//! connection truth cards' expansion shortlist #4.
//!
//! The census is docs-level: Kiro CLI is not installed on this machine. The
//! q/kiro binary-identity split (renamed Amazon Q CLI) is pinned here — the
//! adapter declares the current-generation `kiro-cli` binary only.

use aikit_adapters::clients::kiro_cli::{KiroCliAdapter, ADAPTER_REF, CLIENT, PRODUCT};
use aikit_core::harness_admission::{
    verify_activation_truth, FacultySupport, HarnessActivationObservation, HarnessActivationState,
    HarnessAdmissionAdapter, HarnessEditionKind, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{ActivationEffect, TargetAdapter};

mod common;

#[test]
fn target_is_distinct_and_identity_non_collapsing() {
    let adapter = KiroCliAdapter::new("/tmp/kiro-cli-projection");
    assert_eq!(adapter.target().as_str(), CLIENT);
    assert_eq!(CLIENT, "kiro-cli");
    assert_eq!(PRODUCT, "Kiro CLI");
    assert_eq!(ADAPTER_REF, "aikit:kiro-cli-adapter");
    assert_ne!(adapter.target(), TargetId::codex());
    assert_ne!(adapter.target(), TargetId::claude_code());
}

#[test]
fn admission_is_evidence_backed_and_validates() {
    let adapter = KiroCliAdapter::new("/tmp/kiro-cli-projection");
    let admission = adapter.admission();
    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target.as_str(), "kiro-cli");
    assert_eq!(admission.edition, HarnessEditionKind::Cli);
    // Not installed on this machine: no honest native_version is claimed.
    assert!(admission.native_version.is_none());
    // No Actuation descriptor exists for this slug; no actuation is claimed.
    assert!(admission.realised_actuation_ref.is_none());

    admission.validate().expect("admission must validate");

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
    // Not installed: lifecycle faculties are honestly Unknown.
    let live_reload = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::LiveReload)
        .unwrap();
    assert_eq!(live_reload.support, FacultySupport::Unknown);
}

#[test]
fn census_asserts_the_acp_face_and_the_q_kiro_identity_split() {
    // Every evidence ref must be a docs citation — nothing is machine-observed.
    let adapter = KiroCliAdapter::new("/tmp/kiro-cli-projection");
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

    // The identity split must be named in the census: kiro-cli is the renamed
    // Amazon Q CLI (`q`), and no `q` fallback is declared.
    let surfaces = admission
        .faculty(aikit_core::harness_admission::HarnessFaculty::Surfaces)
        .unwrap();
    assert_eq!(surfaces.support, FacultySupport::Supported);
    let note = surfaces.note.as_deref().unwrap();
    assert!(
        note.contains("Amazon Q CLI"),
        "the rename must be stated: {note}"
    );
    assert!(
        note.contains("kiro-cli acp"),
        "the ACP face must be stated: {note}"
    );

    let identity_evidence = surfaces
        .evidence_refs
        .iter()
        .find(|e| e.contains("renamed Amazon Q CLI"))
        .expect("the identity evidence must be cited");
    assert!(
        identity_evidence.contains("no `q` fallback declared"),
        "the no-q-fallback position must be in the evidence: {identity_evidence}"
    );

    // The ACP face with its optional --agent flag is cited.
    assert!(surfaces.evidence_refs.iter().any(|e| e.contains("--agent")));
}

#[test]
fn profile_declares_the_acp_door_pinned_to_kiro_cli_only() {
    let profile =
        aikit_adapters::profiles::for_slug("kiro-cli").expect("kiro-cli profile must be embedded");
    assert_eq!(profile.slug, "kiro-cli");

    let presence = profile.presence.as_ref().expect("presence layer declared");
    // The current-generation binary only; `q` joins nothing.
    assert_eq!(presence.executables, vec!["kiro-cli".to_string()]);

    let sessions = profile.sessions.as_ref().expect("sessions layer declared");
    assert_eq!(
        sessions.protocol,
        aikit_core::harness_profile::SessionProtocol::Acp
    );
    // The ACP door is documented: `kiro-cli acp`.
    let connect = sessions.connect.as_ref().expect("acp requires a door");
    assert_eq!(
        connect.argv,
        vec!["kiro-cli".to_string(), "acp".to_string()]
    );
    // No --agent in the default door; no q fallback anywhere.
    assert!(!connect.argv.iter().any(|arg| arg.contains("agent")));
    assert!(connect.argv_fallback.is_empty());
    // ACP baseline capabilities only; everything else unverified.
    assert!(sessions.capabilities.ordered_streaming);
    assert!(sessions.capabilities.cancellation);
    assert!(!sessions.capabilities.permission_requests);
    assert!(!sessions.capabilities.reconnect);
    assert!(!sessions.capabilities.mcp_servers);
    assert_eq!(sessions.open_modes, vec!["create".to_string()]);
}

#[test]
fn adapter_plan_posture_is_honest_no_managed_projection_claims() {
    let adapter = KiroCliAdapter::new("/tmp/kiro-cli-projection");
    let caps = adapter.capabilities();
    assert!(!caps.live_reload);
    assert!(!caps.symlinks);
    assert!(!caps.isolated_per_context);
    assert!(caps.brokered_fallback);

    let rc = common::ContextBuilder::new()
        .project_skill(
            "skill/kiro-probe",
            "a skill the brokered census must not claim to project",
            "/tmp/kiro-cli-projection-tree",
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
