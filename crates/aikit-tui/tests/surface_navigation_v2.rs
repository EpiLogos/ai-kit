mod common;

use common::*;

use aikit_tui::application_surface::{
    ApplicationSurfaceController, ApplicationSurfaceRequest, ApplicationSurfaceStep,
};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::PaletteOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );
    (dir, backend)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

#[test]
fn escape_at_rest_is_back_only_and_ctrl_u_is_explicit_query_clear() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap(),
        ApplicationSurfaceStep::Continue
    );
    assert_eq!(surface.semantic().query, "deploy");
    assert!(!surface.semantic().exit_requested);

    surface
        .handle(
            &mut backend,
            key(KeyCode::Char('u'), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert!(surface.semantic().query.is_empty());
}

#[test]
fn explicit_exit_is_refused_while_staged_intent_remains() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    surface
        .handle(&mut backend, key(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    surface
        .handle(
            &mut backend,
            key(KeyCode::Char(' '), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert!(!surface.semantic().staged.is_empty());

    assert_eq!(
        surface
            .handle(
                &mut backend,
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            )
            .unwrap(),
        ApplicationSurfaceStep::Continue
    );
    assert!(!surface.semantic().exit_requested);
    assert!(!surface.semantic().staged.is_empty());
    assert!(surface
        .semantic()
        .status
        .as_ref()
        .unwrap()
        .message
        .contains("apply or discard"));
}

#[test]
fn ctrl_q_is_an_explicit_exit_when_nothing_is_staged() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert_eq!(
        surface
            .handle(
                &mut backend,
                key(KeyCode::Char('q'), KeyModifiers::CONTROL),
            )
            .unwrap(),
        ApplicationSurfaceStep::Outcome(PaletteOutcome::Closed)
    );
    assert!(surface.semantic().exit_requested);
}
