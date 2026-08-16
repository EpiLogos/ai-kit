mod common;

use common::*;

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::project_workspace_render::workspace_section_label;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );
    (dir, backend)
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn alt(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn draw_width(
    surface: &ApplicationSurfaceController,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn final_surface_uses_the_shared_project_world_read_model() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    let world = surface
        .project_world()
        .expect("a Project-bound surface must disclose its Project world");
    assert_eq!(world.project.project.as_str(), "project:payments");
    assert!(world
        .capability_horizon
        .capabilities
        .iter()
        .any(|resource| resource.resource.as_str() == "skill/rust/review"));
    assert!(world.resolution_basis.scopes.is_empty());
    assert!(world
        .warnings
        .iter()
        .any(|warning| warning.contains("scope-layer stack")));
}

#[test]
fn global_surface_remains_first_class_without_inventing_project_world() {
    let dir = tempfile::tempdir().unwrap();
    let mut global = descriptor();
    global.project_root = None;
    global.project_id = None;
    let mut backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    )
    .with_descriptor(global);
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    assert!(surface.project_world().is_none());
    let output = rendered(&draw_width(&surface, 120, 24));
    assert!(output.contains("AIKit · Workspace"));
    assert!(!output.contains("resolved Project world"));
}

#[test]
fn wide_workspace_renders_context_compose_and_explain_from_one_world() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    let context = rendered(&draw_width(&surface, 140, 30));
    assert!(context.contains("Context · resolved Project world"));
    assert!(context.contains("Project  project:payments"));
    assert!(context.contains("Scopes   not exposed by application boundary"));

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "Compose");
    let compose = rendered(&draw_width(&surface, 140, 30));
    assert!(compose.contains("Compose · resolved Project world"));
    assert!(compose.contains("Capabilities"));
    assert!(compose.contains("Information"));
    assert!(compose.contains("Actor/Runtime"));
    assert!(compose.contains("Projection"));
    assert!(compose.contains("Intent        eligibility unresolved"));
    assert!(compose.contains("Effective     availability unresolved"));

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "Explain");
    let explain = rendered(&draw_width(&surface, 140, 30));
    assert!(explain.contains("Explain · authored intent and effective state"));
    assert!(explain.contains("Catalog"));
    assert!(explain.contains("Resolution"));
}

#[test]
fn staged_composition_survives_field_navigation() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let selected = surface
        .semantic()
        .selected
        .clone()
        .expect("explicit selection should choose the review capability");

    surface.handle(&mut backend, ctrl(KeyCode::Char(' '))).unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    assert!(surface.semantic().staged.get(&selected).is_some());

    for _ in 0..6 {
        surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
        assert_eq!(surface.semantic().staged.len(), 1);
        assert!(surface.semantic().staged.get(&selected).is_some());
    }
    assert!(backend.applied.is_empty());
}

#[test]
fn narrow_workspace_progressively_discloses_project_world_without_a_second_controller() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    let selected = surface.semantic().selected.clone();
    let context = rendered(&draw_width(&surface, 60, 24));
    assert!(context.contains("Context · resolved Project world"));
    assert!(context.contains("Project  project:payments"));

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    let compose = rendered(&draw_width(&surface, 60, 24));
    assert!(compose.contains("Compose · resolved Project world"));
    assert!(compose.contains("Capabilities"));
    assert!(compose.contains("Information"));
    assert_eq!(
        surface.semantic().selected,
        selected,
        "responsive Project-world disclosure must not create a second selection state"
    );
}
