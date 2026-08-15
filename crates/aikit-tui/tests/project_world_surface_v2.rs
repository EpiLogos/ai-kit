mod common;

use common::*;

use aikit_core::Result;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::surface::{SurfaceBackend, SurfaceController, SurfaceRequest};
use aikit_tui::tree::{TreeEffect, TreeState};
use aikit_tui::WorkspaceSection;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

impl SurfaceBackend for Fixture {
    fn surface_tree(&self) -> Result<TreeState> {
        Ok(TreeState::new(Vec::new()))
    }

    fn apply_tree_effect(&mut self, _effect: TreeEffect) -> Result<()> {
        Ok(())
    }
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    )
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn draw_width(surface: &mut SurfaceController, width: u16, height: u16) -> Terminal<TestBackend> {
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
fn live_surface_uses_the_shared_project_world_read_model() {
    let mut backend = fixture();
    let surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
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
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    assert!(surface.project_world().is_none());
    let output = rendered(&draw_width(&mut surface, 120, 24));
    assert!(output.contains("AIKit · Workspace"));
    assert!(!output.contains("Project world"));
}

#[test]
fn wide_workspace_renders_project_compose_and_projection_from_one_world() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    let project = rendered(&draw_width(&mut surface, 140, 30));
    assert!(project.contains("Project world"));
    assert!(project.contains("Project  project:payments"));
    assert!(project.contains("Scopes   not exposed by compatibility service"));

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('2')))
        .unwrap();
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Compose);
    let compose = rendered(&draw_width(&mut surface, 140, 30));
    assert!(compose.contains("Compose · resolved Project world"));
    assert!(compose.contains("Capabilities"));
    assert!(compose.contains("Information"));
    assert!(compose.contains("Actor/Runtime"));
    assert!(compose.contains("Projection"));
    assert!(compose.contains("Intent        eligibility unresolved"));
    assert!(compose.contains("Effective     availability unresolved"));

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('4')))
        .unwrap();
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Projection);
    let projection = rendered(&draw_width(&mut surface, 140, 30));
    assert!(projection.contains("Projection"));
    assert!(projection.contains("Targets"));
    assert!(projection.contains("Catalog"));
    assert!(projection.contains("Resolution"));
}

#[test]
fn narrow_workspace_progressively_discloses_project_world_without_a_second_controller() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    let selected = surface.semantic().selected.clone();
    let project = rendered(&draw_width(&mut surface, 60, 24));
    assert!(project.contains("Project world"));
    assert!(project.contains("Project  project:payments"));

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('2')))
        .unwrap();
    let compose = rendered(&draw_width(&mut surface, 60, 24));
    assert!(compose.contains("Compose · resolved Project world"));
    assert!(compose.contains("Capabilities"));
    assert!(compose.contains("Information"));
    assert_eq!(
        surface.semantic().selected,
        selected,
        "responsive Project-world disclosure must not create a second selection state"
    );
}
