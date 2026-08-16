mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_tui::application::{Overlay, PresentationMode, RelationView, WorkspaceSection};
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::project_workspace_render::workspace_section_label;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn key(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );
    (dir, backend)
}

#[test]
fn shipped_workspace_exposes_one_canonical_product_field() {
    assert_eq!(
        WorkspaceSection::ALL.map(workspace_section_label),
        ["Context", "Compose", "Knowledge", "Explain", "History"]
    );

    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "Context");
    assert_eq!(
        surface.semantic().selected.as_ref().map(ResourceRef::as_str),
        Some("skill/rust/review")
    );
}

#[test]
fn final_surface_preserves_identity_across_field_navigation_and_relation_views() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    let selected = surface.semantic().selected.clone();

    for expected in ["Compose", "Knowledge", "Explain", "History"] {
        surface
            .handle(&mut backend, key(KeyCode::Right, KeyModifiers::ALT))
            .unwrap();
        assert_eq!(workspace_section_label(surface.semantic().workspace_section), expected);
        assert_eq!(surface.semantic().selected, selected);
    }

    surface
        .handle(
            &mut backend,
            key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "Knowledge");
    assert_eq!(surface.semantic().relation_view, RelationView::Tree);
    assert_eq!(surface.semantic().selected, selected);
}

#[test]
fn final_surface_owns_the_only_staging_preview_confirmation_apply_route() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    surface
        .handle(
            &mut backend,
            key(KeyCode::Char(' '), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    assert_eq!(surface.semantic().query, "deploy");

    surface
        .handle(
            &mut backend,
            key(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::CompositionPreview));
    surface
        .handle(
            &mut backend,
            key(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::ConfirmApply));
    surface
        .handle(
            &mut backend,
            key(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .unwrap();

    assert!(surface.semantic().staged.is_empty());
    assert_eq!(backend.applied.len(), 1);
}

#[test]
fn final_surface_renders_narrow_medium_and_wide_without_alternate_state() {
    for (width, height) in [(48, 16), (88, 24), (140, 34)] {
        let (_dir, mut backend) = fixture();
        let surface = ApplicationSurfaceController::new(
            &mut backend,
            ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        surface.draw_terminal(&mut terminal).unwrap();
        let rendered = terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect::<String>();
        assert!(rendered.contains("AIKit"));
        assert!(rendered.contains("Search"));
    }
}
