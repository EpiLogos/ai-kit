//! Terminal responsiveness and state-coherence contracts (convergence B3;
//! map §2.3): no stale query replacing a newer one, zero backend-wide world
//! reads per ordinary keystroke, and no loss of staged changes, selection,
//! authored drafts or the lifecycle ladder through navigation.
//!
//! The no-stale guarantee here is structural, and the test says so: the
//! reducer's effect runtime (`TuiRuntime::settle`) executes every effect to
//! completion *before* `handle` returns — there is no background task that
//! could deliver an older query's result after a newer one was typed. What
//! a test can and must pin is the visible consequence: after any sequence
//! of keystrokes, the read model on screen is exactly the newest query's,
//! and the world-wide readings the dispatch contract skips were skipped.
//!
//! The backend is `common::Fixture` — real resolved catalogue, real search
//! — the same basis as `dispatch_recomputation_v2.rs`, whose witness
//! counters (`world_reads_refresh_count` and siblings) this file reuses.

mod common;

use common::*;

use aikit_tui::application::AgentWorkStage;
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Glyphs;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![
            skill("skill/alpha"),
            skill("skill/alpine"),
            skill("skill/beta"),
            script("script/ops/deploy"),
        ],
    );
    (dir, backend)
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_char(surface: &mut ApplicationSurfaceController, backend: &mut Fixture, ch: char) {
    surface.handle(backend, key(KeyCode::Char(ch))).unwrap();
}

fn surface(backend: &mut Fixture) -> ApplicationSurfaceController {
    ApplicationSurfaceController::new(
        backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_glyphs(Glyphs::ascii()),
    )
    .unwrap()
}

fn zero_query_model(surface: &ApplicationSurfaceController) -> aikit_tui::application::ResourceListReadModel {
    surface.semantic().read_model.clone()
}

/// Typing fast — including backspacing back through a prefix — always leaves
/// the read model matching the newest query. Because `handle` settles every
/// effect before returning, an older keystroke's search can never land
/// after a newer one: the newest query wins, every time, by construction.
#[test]
fn the_newest_query_always_wins_no_matter_how_fast_typing_arrives() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    let mut expected = String::new();
    for ch in "alpha".chars() {
        type_char(&mut surface, &mut backend, ch);
        expected.push(ch);
        // Fully settled at every point: the query on screen is the newest
        // typed prefix, never an earlier keystroke's, and the read model
        // carries that query's revision — the effect runtime settled the
        // newest search before `handle` returned.
        assert_eq!(surface.semantic().query, expected);
        assert!(
            surface
                .semantic()
                .read_model
                .revision
                .contains(&format!(":{expected}:")),
            "the model revision must be the newest query's, got {:?}",
            surface.semantic().read_model.revision
        );
    }
    assert_eq!(surface.semantic().query, "alpha");

    // Backspace twice, then extend differently: the model tracks the newest
    // query, never a ghost of the longer one.
    surface.handle(&mut backend, key(KeyCode::Backspace)).unwrap();
    surface.handle(&mut backend, key(KeyCode::Backspace)).unwrap();
    assert_eq!(surface.semantic().query, "alp");
    type_char(&mut surface, &mut backend, 't');
    assert_eq!(surface.semantic().query, "alpt");

    // A keystroke whose query is momentarily operative syntax ("x" alone is
    // a relation operator) must not kill the surface: the failure is named
    // on the status line, the last good model stands, and backspacing out
    // restores the zero-query navigation state exactly as it was.
    surface
        .handle(
            &mut backend,
            PaletteEvent::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
        )
        .unwrap();
    let cleared_model = zero_query_model(&surface);

    type_char(&mut surface, &mut backend, 'x');
    assert_eq!(
        surface.semantic().query, "x",
        "the live query still holds the keystroke"
    );
    let status = surface
        .semantic()
        .status
        .as_ref()
        .expect("the unsearchable query must be named");
    assert!(
        status.message.contains("not searchable yet"),
        "{}",
        status.message
    );
    surface.handle(&mut backend, key(KeyCode::Backspace)).unwrap();
    assert_eq!(surface.semantic().query, "");
    assert_eq!(
        surface.semantic().read_model.resources, cleared_model.resources,
        "backspacing out of an unsearchable query restores the cleared field"
    );
}

/// A keystroke performs zero backend-wide world re-reads and zero relation
/// or inspector re-fetches beyond what a moved selection justifies — the
/// §2.3 structural budget, witnessed on the controller's own counters.
#[test]
fn ordinary_keystrokes_read_nothing_world_wide() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    // Select something so the relation/inspector skip is a real skip, not a
    // vacuous zero.
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let world_before = surface.world_reads_refresh_count();
    let relation_before = surface.relation_refresh_count();
    let inspector_before = surface.inspector_refresh_count();

    for ch in "alph".chars() {
        type_char(&mut surface, &mut backend, ch);
    }

    assert_eq!(surface.world_reads_refresh_count(), world_before);
    // The query narrows; if the selection fell out of the narrowed model,
    // the inspector legitimately re-fetched for the NEW selection — but a
    // keystroke must never re-fetch for an UNCHANGED one. "alpha" keeps
    // matching "alph" here, so nothing moved.
    assert_eq!(
        surface.relation_refresh_count(),
        relation_before,
        "a keystroke that does not move the selection must not re-fetch relations"
    );
    assert_eq!(
        surface.inspector_refresh_count(),
        inspector_before,
        "a keystroke that does not move the selection must not re-fetch the inspector"
    );
}

/// Staged changes, mutation scope, the authored draft and the lifecycle
/// ladder all survive query typing and section/presentation navigation.
/// Nothing in navigation mutates or drops operator state — and nothing in
/// it saves, prepares or launches either.
#[test]
fn operator_state_survives_navigation_untouched() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    // Plant a staged change (the staging path itself is covered by the
    // existing surface tests; this test is about what navigation preserves).
    {
        let state = surface.semantic_mut_for_test();
        state.staged.stage(
            aikit_core::resource::ResourceRef::parse("skill/alpha").unwrap(),
            aikit_tui::application::ActivationIntent::Enable,
        );
    }
    let staged_before = surface.semantic().staged.len();
    assert!(staged_before > 0, "the planted change must be staged");

    // Author a draft and put the ladder mid-flight.
    let state = surface.semantic_mut_for_test();
    state.compose_purpose = "a held exact purpose".into();
    state.agent_work = AgentWorkStage::Saved {
        profile_ref: "agent-profile/held".into(),
        agent_ref: "agent/held".into(),
        revision: "rev-held".into(),
    };
    state
        .compose_intent
        .replace(aikit_tui::application::ComposeIntent::SaveAndStartDirect);

    // Navigate hard: query typing, presentation flip, section walk.
    type_char(&mut surface, &mut backend, 'x');
    surface
        .handle(
            &mut backend,
            PaletteEvent::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        )
        .unwrap();
    surface
        .handle(
            &mut backend,
            PaletteEvent::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        )
        .unwrap();
    surface
        .handle(&mut backend, key(KeyCode::Right))
        .unwrap(); // inert without Alt
    surface.handle(&mut backend, key(KeyCode::Char('1'))).unwrap();

    let state = surface.semantic();
    assert_eq!(state.staged.len(), staged_before, "staged changes survive");
    assert_eq!(state.compose_purpose, "a held exact purpose", "draft survives");
    assert!(matches!(
        state.agent_work,
        AgentWorkStage::Saved { .. }
    ), "the lifecycle ladder survives");
    assert_eq!(
        state.compose_intent,
        Some(aikit_tui::application::ComposeIntent::SaveAndStartDirect)
    );
    assert!(
        matches!(state.agent_work.stable(), AgentWorkStage::Saved { .. }),
        "navigation must not advance, save or launch anything on its own"
    );
}
