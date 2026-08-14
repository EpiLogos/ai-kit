mod common;

use common::*;

use aikit_core::Result;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use aikit_tui::surface::{SurfaceBackend, SurfaceController, SurfaceRequest};
use aikit_tui::tree::{TreeEffect, TreeState};
use aikit_tui::{ActionOutcome, PresentationMode, WorkspaceSection};
use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
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

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn alt(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn mouse(column: u16, row: u16) -> PaletteEvent {
    PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn draw(surface: &mut SurfaceController) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

#[test]
fn mouse_and_keyboard_invoke_the_same_contextual_action_operation() {
    let mut keyboard_backend = fixture();
    let mut keyboard = SurfaceController::new(
        &mut keyboard_backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&mut keyboard);
    keyboard
        .handle(&mut keyboard_backend, key(KeyCode::Char(':')))
        .unwrap();
    keyboard
        .handle(&mut keyboard_backend, key(KeyCode::Enter))
        .unwrap();
    let keyboard_result = keyboard.semantic().action_result.clone();
    assert!(keyboard_result.is_some());

    let mut mouse_backend = fixture();
    let mut mouse_surface = SurfaceController::new(
        &mut mouse_backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&mut mouse_surface);
    assert!(!mouse_surface.semantic().contextual_actions.is_empty());

    let inner = Rect::new(1, 1, 118, 28);
    let preview = Layout::for_width(inner.width)
        .split(inner)
        .preview
        .expect("120 columns must expose the V2 preview/action pane");
    mouse_surface
        .handle(&mut mouse_backend, mouse(preview.x + 1, preview.y + 8))
        .unwrap();

    assert_eq!(mouse_surface.semantic().action_result, keyboard_result);
    assert!(matches!(
        mouse_surface.semantic().action_result,
        Some(ActionOutcome::Explained { .. }) | Some(ActionOutcome::Staged { .. })
    ));
}

#[test]
fn mouse_and_keyboard_choose_the_same_workspace_section() {
    let mut keyboard_backend = fixture();
    let mut keyboard = SurfaceController::new(
        &mut keyboard_backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&mut keyboard);
    keyboard
        .handle(&mut keyboard_backend, alt(KeyCode::Right))
        .unwrap();
    assert_eq!(keyboard.semantic().workspace_section, WorkspaceSection::Compose);

    let mut mouse_backend = fixture();
    let mut mouse_surface = SurfaceController::new(
        &mut mouse_backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&mut mouse_surface);
    // query line: x=1 + "/ " + "deploy" + three-space separator +
    // "Projects" + " · "; Compose begins at x=23.
    mouse_surface
        .handle(&mut mouse_backend, mouse(24, 1))
        .unwrap();

    assert_eq!(
        mouse_surface.semantic().workspace_section,
        keyboard.semantic().workspace_section
    );
}

#[test]
fn mouse_and_keyboard_expand_and_collapse_the_same_presentation_state() {
    let mut keyboard_backend = fixture();
    let mut keyboard = SurfaceController::new(
        &mut keyboard_backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let _ = draw(&mut keyboard);
    keyboard
        .handle(&mut keyboard_backend, ctrl(KeyCode::Char('w')))
        .unwrap();
    assert_eq!(keyboard.semantic().presentation, PresentationMode::Quick);

    let mut mouse_backend = fixture();
    let mut mouse_surface = SurfaceController::new(
        &mut mouse_backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let _ = draw(&mut mouse_surface);
    mouse_surface
        .handle(&mut mouse_backend, mouse(2, 0))
        .unwrap();

    assert_eq!(
        mouse_surface.semantic().presentation,
        keyboard.semantic().presentation
    );
}

#[test]
fn live_shell_title_exposes_truthful_ambient_context_without_invented_identity() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let terminal = draw(&mut surface);
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(rendered.contains("AIKit · Workspace · Project: payments"));
    assert!(rendered.contains("Host: test-host"));
    assert!(rendered.contains("Target: shell"));
    assert!(!rendered.contains("Profile:"));
    assert!(!rendered.contains("Agency:"));
}
