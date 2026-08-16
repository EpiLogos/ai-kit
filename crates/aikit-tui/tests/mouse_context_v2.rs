mod common;

use common::*;

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use aikit_tui::{ActionOutcome, PresentationMode, WorkspaceSection};
use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
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

fn draw_width(
    surface: &ApplicationSurfaceController,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

fn draw(surface: &ApplicationSurfaceController) -> Terminal<TestBackend> {
    draw_width(surface, 120, 30)
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
fn mouse_and_keyboard_invoke_the_same_contextual_action_operation() {
    let (_keyboard_dir, mut keyboard_backend) = fixture();
    let mut keyboard = ApplicationSurfaceController::new(
        &mut keyboard_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&keyboard);
    keyboard
        .handle(&mut keyboard_backend, key(KeyCode::Char(':')))
        .unwrap();
    keyboard
        .handle(&mut keyboard_backend, key(KeyCode::Enter))
        .unwrap();
    let keyboard_result = keyboard.semantic().action_result.clone();
    assert!(keyboard_result.is_some());

    let (_mouse_dir, mut mouse_backend) = fixture();
    let mut mouse_surface = ApplicationSurfaceController::new(
        &mut mouse_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&mouse_surface);
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
    let (_keyboard_dir, mut keyboard_backend) = fixture();
    let mut keyboard = ApplicationSurfaceController::new(
        &mut keyboard_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&keyboard);
    keyboard
        .handle(&mut keyboard_backend, alt(KeyCode::Right))
        .unwrap();
    assert_eq!(keyboard.semantic().workspace_section, WorkspaceSection::Compose);

    let (_mouse_dir, mut mouse_backend) = fixture();
    let mut mouse_surface = ApplicationSurfaceController::new(
        &mut mouse_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let _ = draw(&mouse_surface);
    mouse_surface
        .handle(&mut mouse_backend, mouse(33, 1))
        .unwrap();

    assert_eq!(
        mouse_surface.semantic().workspace_section,
        keyboard.semantic().workspace_section
    );
}

#[test]
fn mouse_and_keyboard_expand_and_collapse_the_same_presentation_state() {
    let (_keyboard_dir, mut keyboard_backend) = fixture();
    let mut keyboard = ApplicationSurfaceController::new(
        &mut keyboard_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let _ = draw(&keyboard);
    keyboard
        .handle(&mut keyboard_backend, ctrl(KeyCode::Char('w')))
        .unwrap();
    assert_eq!(keyboard.semantic().presentation, PresentationMode::Quick);

    let (_mouse_dir, mut mouse_backend) = fixture();
    let mut mouse_surface = ApplicationSurfaceController::new(
        &mut mouse_backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let _ = draw(&mouse_surface);
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
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let terminal = draw(&surface);
    let output = rendered(&terminal);

    assert!(output.contains("AIKit · Workspace · Project: payments"));
    assert!(output.contains("Host: test-host"));
    assert!(output.contains("Target: shell"));
    assert!(!output.contains("Profile:"));
    assert!(!output.contains("Agency:"));
}

#[test]
fn narrow_shell_keeps_compact_project_and_host_context_legible() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let terminal = draw_width(&surface, 60, 20);
    let output = rendered(&terminal);

    assert!(output.contains("AIKit · Workspace · payments · test-host"));
    assert!(!output.contains("Profile:"));
    assert!(!output.contains("Agency:"));
}
