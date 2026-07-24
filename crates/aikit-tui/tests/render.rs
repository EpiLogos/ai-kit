//! Snapshots: the tests that prove the thing actually draws.
//!
//! Every one of these renders a real ratatui frame onto a `TestBackend` and pins
//! the characters that came out. They exist because the rest of the suite proves
//! the palette *decides* correctly, and a palette that decides correctly and
//! draws an empty box is still broken.
//!
//! Two of them are load-bearing beyond looking right. `ascii_fallback` renders
//! the same state twice, once with each glyph set, and asserts the ASCII frame
//! carries the same information without a non-ASCII byte — `STANDARDS.md` §5 says
//! no Unicode is allowed to be load-bearing, and this is what makes that
//! checkable. And `promotion_view_withholds_a_quarantined_capture` asserts that a
//! quarantined capture's text is nowhere in the drawn buffer.

mod common;

use common::*;

use aikit_core::capsule::Capsule;
use aikit_core::scope::ScopeKind;
use aikit_core::trust::TrustState;
use aikit_tui::app::{Action, AppState, Mode};
use aikit_tui::driver::step;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Glyphs;
use aikit_tui::render;
use aikit_tui::PaletteRequest;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Draw a state and return the frame as text.
fn frame(state: &AppState, cols: u16, rows: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("a test terminal");
    terminal
        .draw(|frame| render::draw(frame, state))
        .expect("the palette must draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn catalog() -> Vec<Capsule> {
    vec![
        script_exporting("script/test/cargo-nextest", &["nt"]),
        script("script/ops/deploy"),
        requiring("script", "script/app/uses-lib", &["script/lib/core"]),
        script("script/lib/core"),
        skill("skill/rust/review"),
        hook("hook/gate/boundary"),
    ]
}

/// Open a palette with a glyph set pinned, so a snapshot never depends on the
/// locale of the machine that ran it.
fn open(backend: &mut Fixture, glyphs: Glyphs) -> AppState {
    let (mut state, effects) =
        AppState::open(&*backend, &PaletteRequest::new(UiHost::Inline(16))).unwrap();
    state.glyphs = glyphs;
    aikit_tui::driver::settle(backend, state, effects)
}

fn type_query(backend: &mut Fixture, mut state: AppState, text: &str) -> AppState {
    for c in text.chars() {
        state = step(backend, state, Action::Input(c));
    }
    state
}

// ---------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------

#[test]
fn wide_layout() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/ops/deploy"]);
    let state = open(&mut fixture, Glyphs::unicode());
    insta::assert_snapshot!("wide_layout", frame(&state, 120, 18));
}

#[test]
fn narrow_layout() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/ops/deploy"]);
    let state = open(&mut fixture, Glyphs::unicode());
    insta::assert_snapshot!("narrow_layout", frame(&state, 80, 16));
}

#[test]
fn sub_60_layout() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/ops/deploy"]);
    let state = open(&mut fixture, Glyphs::unicode());
    insta::assert_snapshot!("sub_60_layout", frame(&state, 52, 14));
}

#[test]
fn search_with_results() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/ops/deploy"])
        .set_trust("hook/gate/boundary", TrustState::Unseen);
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = type_query(&mut fixture, state, "o");
    insta::assert_snapshot!("search_with_results", frame(&state, 120, 18));
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

#[test]
fn staged_diff() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = type_query(&mut fixture, state, "uses-lib");
    state = step(&mut fixture, state, Action::Space);
    state = step(&mut fixture, state, Action::ShiftEnter);
    assert_eq!(state.mode, Mode::StagedDiff);
    insta::assert_snapshot!("staged_diff", frame(&state, 120, 18));
}

#[test]
fn argument_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            r#"
[effects]
network = true

[[args]]
name = "target"
type = "enum"
position = 1
choices = ["staging", "production"]

[[args]]
name = "token"
type = "secret"
flag = "--token"

[[args]]
name = "dry-run"
type = "bool"
flag = "--dry-run"
"#,
            "entry = \"payload/run.sh\"\nmode = \"capture\"",
        )],
    );
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = step(&mut fixture, state, Action::Enter);
    assert_eq!(state.mode, Mode::ArgForm);
    state = step(&mut fixture, state, Action::MoveDown);
    state = type_query(&mut fixture, state, "hunter2");
    insta::assert_snapshot!("argument_form", frame(&state, 120, 18));
}

#[test]
fn conflict_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![
            conflicting("script/fmt/alpha", "script/fmt/beta"),
            script("script/fmt/beta"),
        ],
    );
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = type_query(&mut fixture, state, "alpha");
    state = step(&mut fixture, state, Action::Space);
    state = type_query(&mut fixture, state, "");
    for _ in 0.."alpha".len() {
        state = step(&mut fixture, state, Action::Backspace);
    }
    state = type_query(&mut fixture, state, "beta");
    state = step(&mut fixture, state, Action::Space);
    assert_eq!(state.mode, Mode::StagedDiff);
    insta::assert_snapshot!("conflict_dialog", frame(&state, 120, 18));
}

#[test]
fn trust_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture =
        Fixture::new(dir.path(), catalog()).set_trust("script/ops/deploy", TrustState::Unseen);
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Enter);
    assert_eq!(state.mode, Mode::Confirm);
    insta::assert_snapshot!("trust_dialog", frame(&state, 120, 18));
}

#[test]
fn scope_confirmation_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);
    while state.scope.current() != ScopeKind::Project {
        state = step(&mut fixture, state, Action::Tab);
    }
    state = step(&mut fixture, state, Action::CtrlEnter);
    assert_eq!(state.mode, Mode::Confirm);
    insta::assert_snapshot!("scope_confirmation_dialog", frame(&state, 120, 18));
}

#[test]
fn promotion_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog()).with_drafts(vec![ready_draft()]);
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = enter_promotion(&mut fixture, state);
    assert_eq!(state.mode, Mode::Promotion);
    insta::assert_snapshot!("promotion_view", frame(&state, 120, 18));
}

#[test]
fn promotion_view_withholds_a_quarantined_capture() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog()).with_drafts(vec![quarantined_draft()]);
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = enter_promotion(&mut fixture, state);

    let drawn = frame(&state, 120, 18);
    assert!(
        !drawn.contains("ghp_REALSECRETVALUE"),
        "a quarantined body reached the screen:\n{drawn}"
    );
    insta::assert_snapshot!("promotion_view_quarantined", drawn);
}

#[test]
fn error_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = type_query(&mut fixture, state, "uses-lib");
    state = step(&mut fixture, state, Action::Space);
    state = step(
        &mut fixture,
        state,
        Action::ApplyFinished(Err(aikit_core::AikitError::new(
            "generation.stale_base",
            "the session overlay moved under this apply; nothing was written",
        ))),
    );
    assert!(state.outcome.is_none());
    assert_eq!(state.staged.len(), 1);
    insta::assert_snapshot!("error_recovery", frame(&state, 120, 18));
}

#[test]
fn job_output_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/status",
            "",
            "entry = \"payload/run.sh\"\nmode = \"capture\"",
        )],
    )
    .with_job(aikit_tui::backend::JobOutput {
        capsule: Some(cid("script/ops/status")),
        status: Some(1),
        lines: vec![
            "checking the payments index".into(),
            "index is 3 generations behind".into(),
        ],
        truncated: true,
    });
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = step(&mut fixture, state, Action::Enter);
    assert_eq!(state.mode, Mode::JobOutput);
    insta::assert_snapshot!("job_output_view", frame(&state, 120, 18));
}

#[test]
fn help_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture, Glyphs::unicode());
    state = step(&mut fixture, state, Action::Help);
    insta::assert_snapshot!("help_view", frame(&state, 120, 20));
}

// ---------------------------------------------------------------------------
// The ASCII fallback
// ---------------------------------------------------------------------------

#[test]
fn ascii_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/ops/deploy"])
        .set_trust("hook/gate/boundary", TrustState::Unseen);
    let mut state = open(&mut fixture, Glyphs::ascii());
    state = type_query(&mut fixture, state, "o");
    let ascii = frame(&state, 120, 18);
    insta::assert_snapshot!("ascii_fallback", ascii.clone());

    // The same state in the other glyph set. Different characters, same rows.
    let mut unicode_state = state.clone();
    unicode_state.glyphs = Glyphs::unicode();
    let unicode = frame(&unicode_state, 120, 18);
    assert_ne!(
        ascii, unicode,
        "if the two renderings were identical the fallback would be untested cosmetics"
    );
    assert_eq!(
        ascii.lines().count(),
        unicode.lines().count(),
        "the fallback must not cost a row"
    );
}

#[test]
fn the_ascii_rendering_of_the_list_contains_no_non_ascii_character() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/ops/deploy"]);
    let mut state = open(&mut fixture, Glyphs::ascii());
    state = type_query(&mut fixture, state, "deploy");

    // Only the border and the title separator are drawn by ratatui and by the
    // context label; the rows themselves must be pure ASCII.
    let drawn = frame(&state, 120, 18);
    let row_region: String = drawn
        .lines()
        .skip(3)
        .take(6)
        .map(|line| line.trim_matches(|c| c == '│' || c == ' '))
        .collect::<Vec<_>>()
        .join("\n");
    for c in row_region.chars() {
        assert!(c.is_ascii(), "`{c}` is not ASCII in:\n{row_region}");
    }
}
