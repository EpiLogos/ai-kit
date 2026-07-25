//! Keys to actions.
//!
//! Two things are worth a test here and both are about ambiguity the terminal
//! forces on us. `Space` has to be both "stage this" and a character in the query
//! box, and the rule that separates them is stated rather than guessed. And the
//! `Enter` chords only exist on terminals that speak the kitty keyboard protocol,
//! so each has a single-key alias that works everywhere — without which a user on
//! a stock terminal would press `Ctrl+Enter`, get a bare `Enter`, and run
//! something instead of applying.

mod common;

use common::*;

use aikit_core::capsule::Capsule;
use aikit_tui::app::{Action, AppState, Mode};
use aikit_tui::driver::step;
use aikit_tui::event::{action_for, PaletteEvent};
use aikit_tui::host::UiHost;
use aikit_tui::PaletteRequest;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn catalog() -> Vec<Capsule> {
    vec![script("script/ops/deploy"), script("script/test/lint")]
}

fn open(backend: &mut Fixture) -> AppState {
    let (state, effects) =
        AppState::open(&*backend, &PaletteRequest::new(UiHost::Inline(16))).unwrap();
    aikit_tui::driver::settle(backend, state, effects)
}

fn press(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

fn plain(code: KeyCode) -> PaletteEvent {
    press(code, KeyModifiers::NONE)
}

fn name(action: &Action) -> String {
    format!("{action:?}")
}

#[test]
fn space_stages_while_browsing_and_types_a_space_while_filtering() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    assert_eq!(
        name(&action_for(&plain(KeyCode::Char(' ')), &state).unwrap()),
        name(&Action::Space),
        "with an empty query box, Space stages"
    );

    let state = step(&mut fixture, state, Action::Input('d'));
    assert_eq!(
        name(&action_for(&plain(KeyCode::Char(' ')), &state).unwrap()),
        name(&Action::Input(' ')),
        "once you are typing, a space is a space"
    );
    assert_eq!(
        name(&action_for(&press(KeyCode::Char(' '), KeyModifiers::CONTROL), &state).unwrap()),
        name(&Action::Space),
        "Ctrl+Space stages whatever is in the query box"
    );
    assert_eq!(
        name(&action_for(&plain(KeyCode::Insert), &state).unwrap()),
        name(&Action::Space)
    );
}

#[test]
fn a_space_in_a_text_field_is_a_space_and_a_space_on_a_checkbox_is_a_press() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            "\n[[args]]\nname = \"note\"\ntype = \"string\"\nflag = \"--note\"\n\n[[args]]\nname = \"changed\"\ntype = \"bool\"\nflag = \"--changed\"\n",
            "entry = \"payload/run.sh\"",
        )],
    );
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);
    assert_eq!(state.mode, Mode::ArgForm);

    assert_eq!(
        name(&action_for(&plain(KeyCode::Char(' ')), &state).unwrap()),
        name(&Action::Input(' ')),
        "a string field must accept a space"
    );

    state = step(&mut fixture, state, Action::MoveDown);
    assert_eq!(
        name(&action_for(&plain(KeyCode::Char(' ')), &state).unwrap()),
        name(&Action::Space),
        "a boolean field has no use for a space character"
    );
}

#[test]
fn every_enter_chord_has_an_alias_that_works_without_the_kitty_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    for (chord, alias, expected) in [
        (
            press(KeyCode::Enter, KeyModifiers::SHIFT),
            press(KeyCode::Char('e'), KeyModifiers::CONTROL),
            Action::ShiftEnter,
        ),
        (
            press(KeyCode::Enter, KeyModifiers::CONTROL),
            press(KeyCode::Char('s'), KeyModifiers::CONTROL),
            Action::CtrlEnter,
        ),
        (
            press(KeyCode::Enter, KeyModifiers::ALT),
            press(KeyCode::Char('n'), KeyModifiers::CONTROL),
            Action::AltEnter,
        ),
    ] {
        assert_eq!(name(&action_for(&chord, &state).unwrap()), name(&expected));
        assert_eq!(name(&action_for(&alias, &state).unwrap()), name(&expected));
    }
}

#[test]
fn the_documented_single_key_bindings_all_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    for (event, expected) in [
        (plain(KeyCode::Enter), Action::Enter),
        (plain(KeyCode::Esc), Action::Esc),
        (plain(KeyCode::Backspace), Action::Backspace),
        (plain(KeyCode::Up), Action::MoveUp),
        (plain(KeyCode::Down), Action::MoveDown),
        (plain(KeyCode::Tab), Action::Tab),
        (plain(KeyCode::F(1)), Action::Help),
        (plain(KeyCode::Char('?')), Action::Help),
        (
            press(KeyCode::Char('o'), KeyModifiers::CONTROL),
            Action::CtrlO,
        ),
        (
            press(KeyCode::Char('r'), KeyModifiers::CONTROL),
            Action::CtrlR,
        ),
        (
            press(KeyCode::Char('t'), KeyModifiers::CONTROL),
            Action::Tree,
        ),
        (
            press(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Action::Esc,
        ),
    ] {
        assert_eq!(
            name(&action_for(&event, &state).unwrap()),
            name(&expected),
            "{event:?} is not bound as documented"
        );
    }
}

#[test]
fn a_question_mark_typed_into_a_query_is_a_question_mark() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);
    let state = step(&mut fixture, state, Action::Input('w'));
    assert_eq!(
        name(&action_for(&plain(KeyCode::Char('?')), &state).unwrap()),
        name(&Action::Input('?'))
    );
}

#[test]
fn a_key_release_is_ignored_so_a_keystroke_is_not_counted_twice() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert!(action_for(&PaletteEvent::Key(release), &state).is_none());
}

#[test]
fn a_resize_reaches_the_reducer_so_the_layout_can_change_under_a_running_palette() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);
    let action = action_for(&PaletteEvent::Resize(52, 14), &state).unwrap();
    assert_eq!(name(&action), name(&Action::Resized(52, 14)));

    let state = step(&mut fixture, state, action);
    assert_eq!(state.area, (52, 14));
}

#[test]
fn an_unbound_key_never_reaches_the_reducer() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);
    assert!(action_for(&plain(KeyCode::F(9)), &state).is_none());
    assert!(action_for(&PaletteEvent::Idle, &state).is_none());
}
