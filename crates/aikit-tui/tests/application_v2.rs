use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::scope::ScopeKind;
use aikit_tui::{
    keyboard_select, mouse_select, reduce_tui, unresolved_staged, ActivationIntent,
    CompositionPreview, Overlay, PresentationMode, RelationView, ResourceListItem,
    ResourceListReadModel, TuiState, UiAction, UiEffect,
};

fn id(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn item(raw: &str) -> ResourceListItem {
    ResourceListItem {
        resource: id(raw),
        kind: ResourceKind::Capability,
        label: raw.into(),
        summary: format!("summary for {raw}"),
    }
}

fn model(revision: &str, refs: &[&str]) -> ResourceListReadModel {
    ResourceListReadModel {
        revision: revision.into(),
        resources: refs.iter().map(|raw| item(raw)).collect(),
    }
}

fn state_with_model(refs: &[&str]) -> TuiState {
    TuiState {
        read_model: model("r1", refs),
        mutation_scope: Some(ScopeKind::Project),
        ..Default::default()
    }
}

#[test]
fn keyboard_and_mouse_resolve_to_the_same_semantic_selection_action() {
    let resource = id("factory:capability:alpha");
    assert_eq!(
        keyboard_select(resource.clone()),
        mouse_select(resource.clone())
    );

    let state = state_with_model(&["factory:capability:alpha"]);
    let keyboard = reduce_tui(state.clone(), keyboard_select(resource.clone())).state;
    let mouse = reduce_tui(state, mouse_select(resource.clone())).state;
    assert_eq!(keyboard, mouse);
    assert_eq!(keyboard.selected, Some(resource));
}

#[test]
fn refresh_preserves_selection_by_resource_ref_when_rows_reorder() {
    let selected = id("factory:capability:beta");
    let mut state = state_with_model(&[
        "factory:capability:alpha",
        "factory:capability:beta",
        "factory:capability:gamma",
    ]);
    state.selected = Some(selected.clone());

    let next = reduce_tui(
        state,
        UiAction::Refresh(model(
            "r2",
            &[
                "factory:capability:gamma",
                "factory:capability:alpha",
                "factory:capability:beta",
            ],
        )),
    )
    .state;

    assert_eq!(next.selected, Some(selected));
    assert!(next.selection_invalidation.is_none());
}

#[test]
fn refresh_explains_selection_invalidation_instead_of_falling_to_a_row_index() {
    let selected = id("factory:capability:beta");
    let mut state = state_with_model(&["factory:capability:alpha", "factory:capability:beta"]);
    state.selected = Some(selected.clone());

    let next = reduce_tui(
        state,
        UiAction::Refresh(model("r2", &["factory:capability:alpha"])),
    )
    .state;

    assert_eq!(next.selected, None);
    let invalidation = next.selection_invalidation.unwrap();
    assert_eq!(invalidation.previous, selected);
    assert!(invalidation.reason.contains("disappeared"));
}

#[test]
fn presentation_resize_relation_views_and_navigation_do_not_own_semantic_state() {
    let selected = id("factory:capability:alpha");
    let staged = id("factory:capability:beta");
    let mut state = state_with_model(&["factory:capability:alpha", "factory:capability:beta"]);
    state.selected = Some(selected.clone());
    state.staged.stage(staged.clone(), ActivationIntent::Enable);

    let state = reduce_tui(
        state,
        UiAction::SetPresentation(PresentationMode::Workspace),
    )
    .state;
    let state = reduce_tui(state, UiAction::Resize(140, 42)).state;
    let state = reduce_tui(state, UiAction::SetRelationView(RelationView::Tree)).state;
    let state = reduce_tui(state, UiAction::SetRelationView(RelationView::Graph)).state;

    assert_eq!(state.presentation, PresentationMode::Workspace);
    assert_eq!(state.area, (140, 42));
    assert_eq!(state.relation_view, RelationView::Graph);
    assert_eq!(state.selected, Some(selected));
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));
    assert_eq!(state.mutation_scope, Some(ScopeKind::Project));
}

#[test]
fn staged_intent_survives_refresh_even_when_the_resource_temporarily_disappears() {
    let staged = id("factory:capability:beta");
    let mut state = state_with_model(&["factory:capability:alpha", "factory:capability:beta"]);
    state
        .staged
        .stage(staged.clone(), ActivationIntent::Disable);

    let state = reduce_tui(
        state,
        UiAction::Refresh(model("r2", &["factory:capability:alpha"])),
    )
    .state;

    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Disable));
    assert_eq!(unresolved_staged(&state), [staged].into_iter().collect());
    assert_eq!(state.mutation_scope, Some(ScopeKind::Project));
}

#[test]
fn dismiss_and_back_never_discard_staged_state_scope_or_request_exit() {
    let staged = id("factory:capability:alpha");
    let mut state = state_with_model(&["factory:capability:alpha"]);
    state.staged.stage(staged.clone(), ActivationIntent::Enable);
    state.overlay = Some(Overlay::Help);

    let state = reduce_tui(state, UiAction::Dismiss).state;
    assert!(state.overlay.is_none());
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));
    assert_eq!(state.mutation_scope, Some(ScopeKind::Project));
    assert!(!state.exit_requested);

    let state = reduce_tui(state, UiAction::Back).state;
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));
    assert_eq!(state.mutation_scope, Some(ScopeKind::Project));
    assert!(!state.exit_requested);
}

#[test]
fn exit_is_explicit_but_cannot_implicitly_discard_staged_intent() {
    let staged = id("factory:capability:alpha");
    let mut state = state_with_model(&["factory:capability:alpha"]);
    state.staged.stage(staged.clone(), ActivationIntent::Enable);

    let state = reduce_tui(state, UiAction::Exit).state;
    assert!(!state.exit_requested);
    assert_eq!(state.staged.get(&staged), Some(ActivationIntent::Enable));
    assert!(state
        .status
        .as_ref()
        .unwrap()
        .message
        .contains("apply or discard"));
}

#[test]
fn discard_and_exit_are_explicit_actions() {
    let staged = id("factory:capability:alpha");
    let mut state = state_with_model(&["factory:capability:alpha"]);
    state.staged.stage(staged, ActivationIntent::Enable);

    let state = reduce_tui(state, UiAction::DiscardStaged).state;
    assert!(state.staged.is_empty());
    assert!(!state.exit_requested);
    assert!(state
        .status
        .as_ref()
        .unwrap()
        .message
        .contains("discarded explicitly"));

    let state = reduce_tui(state, UiAction::Exit).state;
    assert!(state.exit_requested);
}

#[test]
fn apply_requires_scoped_preview_and_separate_confirmation() {
    let staged = id("factory:capability:alpha");
    let mut state = state_with_model(&["factory:capability:alpha"]);
    state.staged.stage(staged.clone(), ActivationIntent::Enable);

    let reduction = reduce_tui(state, UiAction::RequestApply);
    assert_eq!(
        reduction.effects,
        vec![UiEffect::PreviewComposition {
            scope: ScopeKind::Project,
            staged: reduction.state.staged.clone(),
        }]
    );
    assert!(reduction.state.overlay.is_none());

    let preview = CompositionPreview {
        revision: "preview-r1".into(),
        scope: ScopeKind::Project,
        staged: reduction.state.staged.clone(),
        summary: "enable alpha".into(),
    };
    let state = reduce_tui(
        reduction.state,
        UiAction::CompositionPreviewed(preview.clone()),
    )
    .state;
    assert_eq!(state.overlay, Some(Overlay::CompositionPreview));

    let state = reduce_tui(state, UiAction::RequestApply).state;
    assert_eq!(state.overlay, Some(Overlay::ConfirmApply));

    let reduction = reduce_tui(state, UiAction::ConfirmApply);
    assert_eq!(
        reduction.effects,
        vec![UiEffect::ApplyComposition { preview }]
    );
    assert_eq!(
        reduction.state.staged.get(&staged),
        Some(ActivationIntent::Enable)
    );
}

#[test]
fn changing_scope_invalidates_preview_and_requires_a_new_one() {
    let staged = id("factory:capability:alpha");
    let mut state = state_with_model(&["factory:capability:alpha"]);
    state.staged.stage(staged, ActivationIntent::Enable);
    state.preview = Some(CompositionPreview {
        revision: "preview-project".into(),
        scope: ScopeKind::Project,
        staged: state.staged.clone(),
        summary: "project preview".into(),
    });

    let state = reduce_tui(state, UiAction::SetMutationScope(ScopeKind::Global)).state;
    assert_eq!(state.mutation_scope, Some(ScopeKind::Global));
    assert!(state.preview.is_none());

    let reduction = reduce_tui(state, UiAction::RequestApply);
    assert_eq!(
        reduction.effects,
        vec![UiEffect::PreviewComposition {
            scope: ScopeKind::Global,
            staged: reduction.state.staged.clone(),
        }]
    );
}

#[test]
fn query_changes_request_search_without_reimplementing_search_in_the_reducer() {
    let state = state_with_model(&["factory:capability:alpha"]);
    let reduction = reduce_tui(state, UiAction::SetQuery("wiki graph".into()));
    assert_eq!(reduction.state.query, "wiki graph");
    assert_eq!(
        reduction.effects,
        vec![UiEffect::Search {
            query: "wiki graph".into(),
        }]
    );
}

#[test]
fn selection_navigation_is_stable_ref_based_not_index_identity() {
    let mut state = state_with_model(&[
        "factory:capability:alpha",
        "factory:capability:beta",
        "factory:capability:gamma",
    ]);
    state = reduce_tui(state, UiAction::SelectNext).state;
    assert_eq!(state.selected, Some(id("factory:capability:alpha")));
    state = reduce_tui(state, UiAction::SelectNext).state;
    assert_eq!(state.selected, Some(id("factory:capability:beta")));
    state = reduce_tui(state, UiAction::SelectPrevious).state;
    assert_eq!(state.selected, Some(id("factory:capability:alpha")));
}
