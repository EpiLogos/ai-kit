use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::scope::ScopeKind;
use aikit_tui::{
    reduce_tui, unresolved_staged, ActivationIntent, PresentationMode, RelationView,
    ResourceListItem, ResourceListReadModel, TuiState, UiAction,
};

fn id(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn model(revision: &str, refs: &[&str]) -> ResourceListReadModel {
    ResourceListReadModel {
        revision: revision.into(),
        resources: refs
            .iter()
            .map(|raw| ResourceListItem {
                resource: id(raw),
                kind: ResourceKind::Capability,
                label: (*raw).into(),
                summary: format!("summary for {raw}"),
            })
            .collect(),
    }
}

#[test]
fn presentation_and_provider_refresh_never_take_ownership_of_semantic_state() {
    let selected = id("capability/alpha");
    let staged = id("capability/beta");
    let mut state = TuiState {
        read_model: model("r1", &["capability/alpha", "capability/beta"]),
        selected: Some(selected.clone()),
        mutation_scope: Some(ScopeKind::Project),
        ..Default::default()
    };
    state
        .staged
        .stage(staged.clone(), ActivationIntent::Enable);

    state = reduce_tui(
        state,
        UiAction::SetPresentation(PresentationMode::Workspace),
    )
    .state;
    state = reduce_tui(state, UiAction::SetRelationView(RelationView::Tree)).state;
    state = reduce_tui(state, UiAction::SetRelationView(RelationView::Graph)).state;

    // A provider/index refresh may reorder Resources without changing identity.
    state = reduce_tui(
        state,
        UiAction::Refresh(model(
            "r2",
            &["capability/beta", "capability/alpha"],
        )),
    )
    .state;
    assert_eq!(state.selected, Some(selected.clone()));
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));

    // Temporary provider loss may remove the staged Resource from the read model,
    // but presentation refresh has no authority to discard authored staged intent.
    state = reduce_tui(
        state,
        UiAction::Refresh(model("r3", &["capability/alpha"])),
    )
    .state;
    assert_eq!(state.selected, Some(selected.clone()));
    assert_eq!(unresolved_staged(&state), [staged.clone()].into_iter().collect());
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));

    // When provider/index state recovers, stable identity reconnects without a new
    // controller or a second staging store.
    state = reduce_tui(
        state,
        UiAction::Refresh(model(
            "r4",
            &["capability/alpha", "capability/beta"],
        )),
    )
    .state;

    assert_eq!(state.presentation, PresentationMode::Workspace);
    assert_eq!(state.relation_view, RelationView::Graph);
    assert_eq!(state.selected, Some(selected));
    assert!(unresolved_staged(&state).is_empty());
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));
    assert_eq!(state.mutation_scope, Some(ScopeKind::Project));
}
