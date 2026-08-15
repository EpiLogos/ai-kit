mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_core::{RelationQuery, ScopeKind};
use aikit_tui::{
    selected_contextual_action, ActivationIntent, KnowledgeNavigationService,
    PaletteApplicationService, PresentationMode, TuiApplicationService, TuiRuntime, TuiState,
    UiAction,
};

#[test]
fn resource_action_compose_explain_and_relations_share_one_identity_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );
    let resource = ResourceRef::parse("skill/rust/review").unwrap();

    let mut service = PaletteApplicationService::new(&mut backend);
    let mut runtime = TuiRuntime::new();
    let mut state = TuiState::default();

    // Quick and Workspace are two presentations over the same ResourceRef-native
    // search model. No row/capsule identity is introduced by the surface.
    state = runtime
        .step(&mut service, state, UiAction::SetQuery("review".into()))
        .unwrap();
    assert!(state.read_model.contains(&resource));

    state = runtime
        .step(&mut service, state, UiAction::Select(resource.clone()))
        .unwrap();
    assert_eq!(state.selected.as_ref(), Some(&resource));
    assert!(state
        .contextual_actions
        .iter()
        .any(|action| action.action.as_str() == "action/capability/toggle"));

    state = runtime
        .step(
            &mut service,
            state,
            UiAction::SetPresentation(PresentationMode::Workspace),
        )
        .unwrap();
    assert_eq!(state.selected.as_ref(), Some(&resource));

    // The contextual Action produces staged semantic intent; it does not bypass
    // preview/apply or own a second mutation implementation.
    state = runtime
        .step(&mut service, state, UiAction::BeginActionSearch)
        .unwrap();
    state = runtime
        .step(
            &mut service,
            state,
            UiAction::SetActionQuery("toggle".into()),
        )
        .unwrap();
    let action = selected_contextual_action(&state).expect("toggle Action should be searchable");
    assert_eq!(action.action.as_str(), "action/capability/toggle");
    state = runtime
        .step(
            &mut service,
            state,
            UiAction::InvokeAction(action.action.clone()),
        )
        .unwrap();
    assert_eq!(state.staged.get(&resource), Some(ActivationIntent::Enable));
    assert!(backend.applied.is_empty());

    state = runtime
        .step(
            &mut service,
            state,
            UiAction::SetMutationScope(ScopeKind::Project),
        )
        .unwrap();
    state = runtime
        .step(&mut service, state, UiAction::RequestCompositionPreview)
        .unwrap();
    assert!(state.preview.is_some());
    assert!(backend.applied.is_empty());

    state = runtime
        .step(&mut service, state, UiAction::RequestApply)
        .unwrap();
    state = runtime
        .step(&mut service, state, UiAction::ConfirmApply)
        .unwrap();
    assert!(state.staged.is_empty());
    assert_eq!(backend.applied.len(), 1);

    // Explain and relation navigation still resolve the same stable ResourceRef
    // after mutation rather than falling back to view-row identity.
    let explanation = service.explain(&resource).unwrap();
    assert_eq!(explanation["resource"], resource.as_str());
    assert!(explanation.get("resolutionHash").is_some());

    let relations = service
        .relation_view(RelationQuery::local(resource.clone()))
        .unwrap();
    assert_eq!(relations.query.focus, resource);
    assert!(relations
        .edges
        .iter()
        .any(|edge| edge.to.as_str() == "action/capability/explain"));
    assert!(relations
        .edges
        .iter()
        .any(|edge| edge.to.as_str() == "action/capability/toggle"));
}