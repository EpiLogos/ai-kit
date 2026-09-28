//! The text lanes' echo contract.
//!
//! Every lane that captures keystrokes must also be *seen*: the search row
//! echoes every typed character (and an empty query shows its visible
//! prompt, never an invisible buffer), and the Compose creator lane echoes
//! its in-flight draft while it captures it. A frame is rendered into a
//! `TestBackend` and the pane's cells are read back, so this proves what a
//! pane in front of the surface actually shows.

mod common;

use common::*;

use aikit_tui::application::WorkspaceSection;
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::compose_spine::ComposeStep;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![skill("skill/alpha"), script("script/ops/deploy")],
    );
    (dir, backend)
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn frame_rows(surface: &ApplicationSurfaceController) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(
        surface.semantic().area.0,
        surface.semantic().area.1,
    ))
    .unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    let buffer = terminal.backend().buffer();
    let (width, height) = (buffer.area.width, buffer.area.height);
    (0..height)
        .map(|row| {
            (0..width)
                .map(|column| {
                    buffer
                        .cell((column, row))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect()
}

fn typed_query_echoes(host: UiHost) {
    let (_dir, mut backend) = fixture();
    let mut surface =
        ApplicationSurfaceController::new(&mut backend, ApplicationSurfaceRequest::new(host))
            .unwrap();
    for character in "dep".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    assert_eq!(surface.semantic().query, "dep");

    let rows = frame_rows(&surface);
    let query_row = &rows[1]; // first row inside the border
    assert!(
        query_row.contains("/ dep"),
        "typed query must echo in the query row, got {query_row:?}"
    );
}

#[test]
fn typed_query_echoes_in_the_popup_shell() {
    typed_query_echoes(UiHost::TmuxPopup);
}

#[test]
fn typed_query_echoes_in_the_inline_shell() {
    typed_query_echoes(UiHost::Inline(10));
}

#[test]
fn empty_query_shows_a_visible_prompt_not_a_blank_row() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    assert!(surface.semantic().query.is_empty());

    let rows = frame_rows(&surface);
    let query_row = &rows[1];
    assert!(
        query_row.contains("/ Search resources and actions"),
        "an empty query must show its prompt, got {query_row:?}"
    );
}

#[test]
fn backspace_shortens_the_echo() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    for character in "dep".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    surface
        .handle(&mut backend, key(KeyCode::Backspace))
        .unwrap();
    assert_eq!(surface.semantic().query, "de");

    let rows = frame_rows(&surface);
    let query_row = &rows[1];
    assert!(
        query_row.contains("/ de") && !query_row.contains("/ dep"),
        "query row must match the buffer after backspace, got {query_row:?}"
    );
}

fn compose_surface(backend: &mut Fixture) -> ApplicationSurfaceController {
    let mut surface = ApplicationSurfaceController::new(
        backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    // A tall working pane: the Enter-work step's authored-fields block sits
    // below the spine rows, and this test reads it back from the buffer.
    surface
        .handle(backend, PaletteEvent::Resize(90, 60))
        .unwrap();
    // Worlds -> Compose (`4`), then walk the spine to its last step; the
    // creator lane lives on the Enter-work step.
    surface.handle(backend, key(KeyCode::Char('4'))).unwrap();
    for _ in 0..9 {
        surface
            .handle(backend, key_with(KeyCode::Down, KeyModifiers::ALT))
            .unwrap();
    }
    assert_eq!(
        surface.semantic().workspace_section,
        WorkspaceSection::Compose
    );
    assert_eq!(surface.semantic().compose_step, ComposeStep::EnterWork);
    surface
}

fn key_with(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

#[test]
fn the_open_creator_lane_shows_a_visible_empty_prompt() {
    let (_dir, mut backend) = fixture();
    let mut surface = compose_surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(
        surface.compose_text_lane(),
        Some(aikit_tui::application_surface::ComposeTextField::Purpose)
    );

    let pane = frame_rows(&surface).join("\n");
    assert!(
        pane.contains("purpose >"),
        "an empty draft must show its visible prompt, got:\n{pane}"
    );
    assert!(
        pane.contains("Esc abandons"),
        "the empty prompt must say how the lane closes, got:\n{pane}"
    );
}

#[test]
fn the_creator_lane_echoes_every_typed_character() {
    let (_dir, mut backend) = fixture();
    let mut surface = compose_surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    for character in "Prove the slice exactly".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    assert_eq!(
        surface.semantic().compose_text_draft.as_ref().unwrap().text,
        "Prove the slice exactly"
    );

    let pane = frame_rows(&surface).join("\n");
    assert!(
        pane.contains("purpose > Prove the slice exactly"),
        "the in-flight draft must echo in the pane while the lane captures it, got:\n{pane}"
    );

    // Backspace shortens the echo the same way the search row's does.
    surface
        .handle(&mut backend, key(KeyCode::Backspace))
        .unwrap();
    let pane = frame_rows(&surface).join("\n");
    assert!(
        pane.contains("purpose > Prove the slice exactl")
            && !pane.contains("purpose > Prove the slice exactly"),
        "the echoed draft must track the buffer after backspace, got:\n{pane}"
    );
}

#[test]
fn committing_the_purpose_moves_the_echo_to_the_name_lane() {
    let (_dir, mut backend) = fixture();
    let mut surface = compose_surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    for character in "Prove the slice exactly".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();

    // The purpose is committed human source; the lane continues on the name.
    assert_eq!(
        surface.semantic().compose_purpose,
        "Prove the slice exactly"
    );
    assert_eq!(
        surface.compose_text_lane(),
        Some(aikit_tui::application_surface::ComposeTextField::Name)
    );
    let pane = frame_rows(&surface).join("\n");
    assert!(
        pane.contains("purpose \"Prove the slice exactly\""),
        "the committed purpose stays visible, got:\n{pane}"
    );
    assert!(
        pane.contains("name    >"),
        "the name lane shows its own visible empty prompt, got:\n{pane}"
    );

    // Esc abandons the name draft without touching the committed purpose.
    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(surface.compose_text_lane(), None);
    assert_eq!(
        surface.semantic().compose_purpose,
        "Prove the slice exactly"
    );
}
