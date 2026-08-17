use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    ObservedRelationState, ReconstructionStatus, SessionSpaceAgentAttachmentIntent,
    SessionSpaceFocus, SessionSpaceMutation, SessionSpaceNativeObservation,
    SessionSpaceNativeReferenceBinding, SessionSpaceNativeReferenceKind,
    SessionSpaceSurfaceAttachmentIntent,
};
use aikit_store::{
    explain_session_space_with_receipts, AikitHome, SessionSpaceApplicationStore,
};

#[test]
fn durable_mutations_history_and_provider_loss_share_one_authority() {
    let dir = tempfile::tempdir().unwrap();
    let home = AikitHome::at(dir.path());
    home.ensure_layout().unwrap();
    let store = SessionSpaceApplicationStore::new(home);

    let space = SessionSpaceRef::parse("session-space/matrix").unwrap();
    let create = store
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("matrix".into()),
            },
        )
        .unwrap();
    store.apply(&create).unwrap();

    let agent = ResourceRef::parse("agent-session/matrix-agent").unwrap();
    let attach_agent = store
        .stage(
            Some(&space),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: agent.clone(),
                    purpose: Some("development".into()),
                    provenance: vec!["authored attachment intent".into()],
                },
            },
        )
        .unwrap();
    let agent_receipt = store.apply(&attach_agent).unwrap();

    let surface = ResourceRef::parse("surface/matrix-editor").unwrap();
    let attach_surface = store
        .stage(
            Some(&space),
            SessionSpaceMutation::AttachSurface {
                attachment: SessionSpaceSurfaceAttachmentIntent {
                    surface: surface.clone(),
                    component: None,
                    purpose: Some("editor".into()),
                    provenance: vec!["authored surface intent".into()],
                },
            },
        )
        .unwrap();
    let surface_receipt = store.apply(&attach_surface).unwrap();

    let material = ResourceRef::parse("material/workcell-a/candidate-1").unwrap();
    let bind_material = store
        .stage(
            Some(&space),
            SessionSpaceMutation::BindNativeReference {
                binding: SessionSpaceNativeReferenceBinding {
                    reference: material.clone(),
                    kind: SessionSpaceNativeReferenceKind::Material,
                    owner: Some(ResourceRef::parse("workcell/a").unwrap()),
                    provider: Some(ResourceRef::parse("provider/docker").unwrap()),
                    host: Some(ResourceRef::parse("host/worker-a").unwrap()),
                    purpose: Some("candidate preview".into()),
                    provenance: vec!["Workcell material reading".into()],
                },
            },
        )
        .unwrap();
    store.apply(&bind_material).unwrap();

    let focus = store
        .stage(
            Some(&space),
            SessionSpaceMutation::Focus {
                focus: Some(SessionSpaceFocus {
                    target: surface.clone(),
                    region: Some("primary".into()),
                    provenance: vec!["portable focus intent".into()],
                }),
            },
        )
        .unwrap();
    store.apply(&focus).unwrap();

    let absent = store.reconstruct(&space, None, &[], &[]).unwrap();
    let material_absent = absent
        .relations
        .iter()
        .find(|relation| relation.reference == material.as_str())
        .unwrap();
    assert_eq!(material_absent.status, ReconstructionStatus::Unavailable);
    assert_eq!(
        absent.provider_native_detail,
        ReconstructionStatus::IrrecoverableProviderDetail
    );

    let returned = store
        .reconstruct(
            &space,
            None,
            &[SessionSpaceNativeObservation {
                reference: material.clone(),
                state: ObservedRelationState::Available,
                provider: Some(ResourceRef::parse("provider/docker").unwrap()),
                reason: None,
            }],
            &[],
        )
        .unwrap();
    let material_returned = returned
        .relations
        .iter()
        .find(|relation| relation.reference == material.as_str())
        .unwrap();
    assert_eq!(material_returned.status, ReconstructionStatus::Reobserved);

    let detach_surface = store
        .stage(
            Some(&space),
            SessionSpaceMutation::DetachSurface {
                surface: surface.clone(),
            },
        )
        .unwrap();
    let detached_surface = store.apply(&detach_surface).unwrap();
    assert!(!detached_surface.after.surfaces.contains_key(&surface));
    assert!(detached_surface.after.focus.is_none());

    let detach_agent = store
        .stage(
            Some(&space),
            SessionSpaceMutation::DetachAgentSession {
                agent_session: agent.clone(),
            },
        )
        .unwrap();
    let detached_agent = store.apply(&detach_agent).unwrap();
    assert!(!detached_agent.after.agent_sessions.contains_key(&agent));

    let broad_comparison = store
        .compare_history(&space, agent_receipt.sequence, detached_agent.sequence)
        .unwrap();
    assert!(broad_comparison.agent_session_intent_changed);
    assert!(broad_comparison.native_reference_changed);
    assert!(!broad_comparison.focus_changed);

    let surface_comparison = store
        .compare_history(
            &space,
            surface_receipt.sequence,
            detached_surface.sequence,
        )
        .unwrap();
    assert!(surface_comparison.surface_intent_changed);

    let explanation =
        explain_session_space_with_receipts(&store, &space, Some(returned)).unwrap();
    assert_eq!(
        explanation.latest_receipt.as_ref().unwrap().sequence,
        detached_agent.sequence
    );
    assert_eq!(
        explanation.explanation.semantic_revision,
        detached_agent.after.revision
    );

    let restore_preview = store
        .stage_restore(&space, surface_receipt.sequence)
        .unwrap();
    assert!(restore_preview.proposed.surfaces.contains_key(&surface));
    assert!(store.load(&space).unwrap().surfaces.get(&surface).is_none());
}
