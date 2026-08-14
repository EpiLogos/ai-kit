use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::ResourceRef;
use aikit_tui::tui_state::{
    reduce_tui, Overlay, Presentation, PreviewState, TuiState, UiAction, UiEffect,
};

fn resource(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn resources(values: &[&str]) -> BTreeSet<ResourceRef> {
    values.iter().map(|value| resource(value)).collect()
}

fn staged(resource: ResourceRef, enable: bool) -> BTreeMap<ResourceRef, bool> {
    [(resource, enable)].into_iter().collect()
}

fn apply(state: TuiState, action: UiAction) -> TuiState {
    reduce_tui(state, action).state
}

#[test]
fn refresh_reconciles_selection_by_stable_ref_not_position() {
    let selected = resource("capability:review");
    let state = TuiState {
        selected: Some(selected.clone()),
        ..TuiState::default()
    };

    // Order is deliberately unrelated to a prior list position.
    let reduction = reduce_tui(
        state,
        UiAction::ResourcesRefreshed(resources(&[
            "context-source:docs",
            "capability:deploy",
            "capability:review",
        ])),
    );

    assert_eq!(reduction.state.selected, Some(selected));
    assert!(reduction.state.selection_invalidation.is_none());
}

#[test]
fn missing_selected_ref_is_explicitly_invalidated_with_explanation() {
    let selected = resource("capability:review");
    let state = TuiState {
        selected: Some(selected.clone()),
        ..TuiState::default()
    };

    let reduction = reduce_tui(
        state,
        UiAction::ResourcesRefreshed(resources(&["capability:deploy"])),
    );

    assert!(reduction.state.selected.is_none());
    let invalidation = reduction.state.selection_invalidation.unwrap();
    assert_eq!(invalidation.resource, selected);
    assert!(invalidation.reason.contains("refreshed read model"));
}

#[test]
fn resize_and_quick_workspace_switch_preserve_semantic_state() {
    let selected = resource("context-source:design");
    let staged_resource = resource("capability:review");
    let mut state = TuiState {
        selected: Some(selected.clone()),
        query: "review".into(),
        staged: staged(staged_resource.clone(), true),
        preview: PreviewState::Ready,
        ..TuiState::default()
    };

    state = apply(state, UiAction::Present(Presentation::Workspace));
    state = apply(state, UiAction::Resize(140, 48));
    state = apply(state, UiAction::Present(Presentation::Quick));

    assert_eq!(state.presentation, Presentation::Quick);
    assert_eq!(state.area, (140, 48));
    assert_eq!(state.selected, Some(selected));
    assert_eq!(state.query, "review");
    assert_eq!(state.staged.get(&staged_resource), Some(&true));
    assert_eq!(state.preview, PreviewState::Ready);
}

#[test]
fn back_never_clears_query_discards_stage_applies_or_exits() {
    let staged_resource = resource("capability:review");
    let state = TuiState {
        query: "keep me".into(),
        staged: staged(staged_resource.clone(), true),
        preview: PreviewState::Ready,
        overlay: Some(Overlay::ApplyConfirmation),
        ..TuiState::default()
    };

    let reduction = reduce_tui(state, UiAction::Back);

    assert!(reduction.effects.is_empty());
    assert_eq!(reduction.state.query, "keep me");
    assert_eq!(reduction.state.staged.get(&staged_resource), Some(&true));
    assert_eq!(reduction.state.preview, PreviewState::Ready);
    assert!(reduction.state.overlay.is_none());
}

#[test]
fn clear_query_discard_stage_and_exit_are_explicit_independent_actions() {
    let staged_resource = resource("capability:review");
    let state = TuiState {
        query: "review".into(),
        staged: staged(staged_resource, true),
        preview: PreviewState::Ready,
        ..TuiState::default()
    };

    let clear = reduce_tui(state.clone(), UiAction::ClearQuery);
    assert!(clear.state.query.is_empty());
    assert_eq!(clear.state.staged.len(), 1);
    assert!(clear.effects.is_empty());

    let discard = reduce_tui(state.clone(), UiAction::DiscardStaged);
    assert_eq!(discard.state.query, "review");
    assert!(discard.state.staged.is_empty());
    assert!(discard.effects.is_empty());

    let exit = reduce_tui(state, UiAction::RequestExit);
    assert_eq!(exit.effects, vec![UiEffect::Exit]);
    assert_eq!(exit.state.query, "review");
    assert_eq!(exit.state.staged.len(), 1);
}

#[test]
fn stage_always_previews_before_an_apply_can_be_emitted() {
    let target = resource("capability:review");

    let staged = reduce_tui(
        TuiState::default(),
        UiAction::Stage {
            resource: target.clone(),
            enable: true,
        },
    );
    assert_eq!(staged.state.preview, PreviewState::Required);
    assert_eq!(
        staged.effects,
        vec![UiEffect::PreviewComposition(
            staged.state.staged_mutations()
        )]
    );

    let requested_too_early = reduce_tui(staged.state.clone(), UiAction::RequestApply);
    assert_eq!(requested_too_early.state.preview, PreviewState::Required);
    assert!(matches!(
        requested_too_early.effects.as_slice(),
        [UiEffect::PreviewComposition(_)]
    ));

    let ready = apply(staged.state, UiAction::PreviewReady);
    let requested = reduce_tui(ready, UiAction::RequestApply);
    assert!(requested.effects.is_empty());
    assert_eq!(requested.state.overlay, Some(Overlay::ApplyConfirmation));

    let confirmed = reduce_tui(requested.state, UiAction::ConfirmApply);
    assert!(matches!(
        confirmed.effects.as_slice(),
        [UiEffect::ApplyComposition(changes)] if changes.len() == 1 && changes[0].resource == target
    ));
}

#[test]
fn refresh_navigation_and_presentation_never_drop_staged_intent() {
    let staged_resource = resource("capability:review");
    let selected = resource("context-source:docs");
    let state = apply(
        TuiState::default(),
        UiAction::Stage {
            resource: staged_resource.clone(),
            enable: true,
        },
    );
    let state = apply(state, UiAction::Select(Some(selected.clone())));
    let state = apply(state, UiAction::Present(Presentation::Workspace));
    let state = apply(
        state,
        UiAction::ResourcesRefreshed(resources(&["context-source:docs"])),
    );
    let state = apply(state, UiAction::Back);

    assert_eq!(state.selected, Some(selected));
    assert_eq!(state.staged.get(&staged_resource), Some(&true));
    assert_eq!(state.preview, PreviewState::Required);
}

#[test]
fn selection_action_is_input_source_agnostic() {
    let selected = resource("capability:review");

    // Keyboard and mouse adapters are required to converge on this same semantic
    // action. The reducer cannot observe which input device produced it.
    let keyboard = reduce_tui(TuiState::default(), UiAction::Select(Some(selected.clone())));
    let mouse = reduce_tui(TuiState::default(), UiAction::Select(Some(selected)));

    assert_eq!(keyboard, mouse);
}
