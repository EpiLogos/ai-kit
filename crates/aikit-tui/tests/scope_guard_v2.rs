use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_tui::{
    reduce_tui, ActivationIntent, ResourceListItem, ResourceListReadModel, TuiState, UiAction,
};

fn resource() -> ResourceRef {
    ResourceRef::parse("factory:capability:scope-guard").unwrap()
}

#[test]
fn staged_changes_without_a_mutation_scope_cannot_preview_or_apply() {
    let resource = resource();
    let mut state = TuiState::default();
    state.read_model = ResourceListReadModel {
        revision: "r1".into(),
        resources: vec![ResourceListItem {
            resource: resource.clone(),
            kind: ResourceKind::Capability,
            label: "scope guard".into(),
            summary: "requires explicit mutation scope".into(),
        }],
    };
    state
        .staged
        .stage(resource.clone(), ActivationIntent::Enable);

    let preview = reduce_tui(state, UiAction::RequestCompositionPreview);
    assert!(preview.effects.is_empty());
    assert!(preview.state.preview.is_none());
    assert_eq!(
        preview.state.staged.get(&resource),
        Some(ActivationIntent::Enable)
    );
    assert!(preview
        .state
        .status
        .as_ref()
        .unwrap()
        .message
        .contains("choose a scope"));

    let apply = reduce_tui(preview.state, UiAction::RequestApply);
    assert!(apply.effects.is_empty());
    assert!(apply.state.preview.is_none());
    assert_eq!(
        apply.state.staged.get(&resource),
        Some(ActivationIntent::Enable)
    );
    assert!(!apply.state.exit_requested);
}
