mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_tui::application::{Overlay, PresentationMode, RelationView, WorkspaceSection};
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::PaletteBackend;
use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );
    (dir, backend)
}

fn rref(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

#[test]
fn initial_query_selects_one_stable_resource_without_tree_state() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    assert_eq!(surface.semantic().selected, Some(rref("script/ops/deploy")));
    assert!(surface
        .semantic()
        .read_model
        .contains(&rref("script/ops/deploy")));
    assert!(surface.relation().is_some());
}

#[test]
fn explicit_staging_preview_confirmation_and_apply_live_only_in_tui_state() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let resource = rref("script/ops/deploy");

    surface
        .handle(&mut backend, ctrl(KeyCode::Char(' ')))
        .unwrap();
    assert!(surface.semantic().staged.get(&resource).is_some());
    assert_eq!(surface.semantic().query, "deploy");
    assert!(!backend.view().is_active(&cid("script/ops/deploy")));

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('s')))
        .unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::CompositionPreview));
    surface
        .handle(&mut backend, ctrl(KeyCode::Char('s')))
        .unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::ConfirmApply));
    surface
        .handle(&mut backend, ctrl(KeyCode::Char('s')))
        .unwrap();

    assert!(surface.semantic().staged.is_empty());
    assert!(backend.view().is_active(&cid("script/ops/deploy")));
    assert_eq!(backend.applied.len(), 1);
}

#[test]
fn list_tree_graph_are_views_of_one_selection_and_staging_state() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let selected = surface.semantic().selected.clone();

    surface
        .handle(&mut backend, ctrl(KeyCode::Char(' ')))
        .unwrap();
    assert_eq!(surface.semantic().relation_view, RelationView::List);

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('t')))
        .unwrap();
    assert_eq!(surface.semantic().relation_view, RelationView::Tree);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);
    assert_eq!(surface.semantic().selected, selected);
    assert_eq!(surface.semantic().staged.len(), 1);

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('t')))
        .unwrap();
    assert_eq!(surface.semantic().relation_view, RelationView::Graph);
    assert_eq!(surface.semantic().selected, selected);
    assert_eq!(surface.semantic().staged.len(), 1);

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('t')))
        .unwrap();
    assert_eq!(surface.semantic().relation_view, RelationView::List);
    assert_eq!(surface.semantic().selected, selected);
    assert_eq!(surface.semantic().staged.len(), 1);
}

#[test]
fn legacy_tree_opening_flag_maps_to_relation_projection_not_a_controller() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("deploy")
            .opening_relations(RelationView::Tree),
    )
    .unwrap();

    assert_eq!(surface.semantic().relation_view, RelationView::Tree);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);
    assert_eq!(surface.semantic().selected, Some(rref("script/ops/deploy")));
}

#[test]
fn keyboard_and_mouse_select_the_same_resource_ref() {
    let (_dir_a, mut keyboard_backend) = fixture();
    let mut keyboard = ApplicationSurfaceController::new(
        &mut keyboard_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    keyboard
        .handle(&mut keyboard_backend, PaletteEvent::Resize(100, 30))
        .unwrap();
    keyboard
        .handle(&mut keyboard_backend, key(KeyCode::Down))
        .unwrap();
    let keyboard_selected = keyboard.semantic().selected.clone();
    assert!(keyboard_selected.is_some());

    let (_dir_b, mut mouse_backend) = fixture();
    let mut mouse_surface = ApplicationSurfaceController::new(
        &mut mouse_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    mouse_surface
        .handle(&mut mouse_backend, PaletteEvent::Resize(100, 30))
        .unwrap();
    let event = PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    mouse_surface.handle(&mut mouse_backend, event).unwrap();

    assert_eq!(mouse_surface.semantic().selected, keyboard_selected);
}

#[test]
fn exit_refuses_to_discard_staged_semantic_intent() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    surface
        .handle(&mut backend, ctrl(KeyCode::Char(' ')))
        .unwrap();
    assert!(!surface.semantic().staged.is_empty());

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('q')))
        .unwrap();
    assert!(!surface.semantic().exit_requested);
    assert!(surface
        .semantic()
        .status
        .as_ref()
        .is_some_and(|status| status.message.contains("apply or discard")));
}
