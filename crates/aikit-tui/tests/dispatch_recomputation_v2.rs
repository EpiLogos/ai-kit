//! Deterministic evidence for the keystroke-latency defect fixed in
//! `ApplicationSurfaceController::dispatch` (`application_surface.rs`):
//! before the fix, every dispatched `UiAction` — a single typed character
//! included — unconditionally re-read the Project World, re-discovered
//! SessionSpaces, re-read history evidence, re-read the Factory entry, and
//! re-fetched the relation neighbourhood and Inspector, regardless of
//! whether the action could have changed any of them.
//!
//! The witness here is not a backend call count. `ApplicationService`'s own
//! plumbing shares a lot of surface area across unrelated readings — the
//! trait-default `PaletteBackend::navigation_index()` that `search()` needs
//! for every keystroke also happens to call `factory_work_entry()` and
//! `familiarity()` internally to decide what actions/evidence to annotate
//! the index with — so a raw count of calls made *to the backend* cannot
//! tell "the controller deliberately re-read this" apart from "some
//! unrelated read touched the same backend method along the way". The
//! precise, load-bearing question is what `dispatch` itself decided to do,
//! so `ApplicationSurfaceController` carries its own witness counters for
//! exactly that decision — `world_reads_refresh_count`, `relation_refresh_count`,
//! `inspector_refresh_count` — the same idiom the surface already uses for
//! `graph_layout_recompute_count` and for the identical reason documented
//! on that field: output stability cannot prove a "recompute only on
//! genuine change" contract when the unmutated inputs make the recomputed
//! answer identical to the skipped one, so only a direct counter can tell
//! "skipped" apart from "recomputed the same answer".
//!
//! The backend underneath is still a real one — `common::Fixture`, the same
//! resolver-backed fixture every other V2 surface test uses — so every
//! `search`/`explain`/`relations_at_depth` call these tests exercise is a
//! real read against a real resolved catalogue, not a stub.

mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![
            skill("skill/alpha"),
            skill("skill/beta"),
            skill("skill/gamma"),
        ],
    );
    (dir, backend)
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn id(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

/// Type one character by appending it to the live query, exactly as
/// `ApplicationSurfaceController::handle_key`'s generic `Char` arm does for
/// an ordinary keystroke — the same path the measured production defect
/// ("+\"i\" -> query repaint: 1052 ms") went through once per key.
fn type_char(surface: &mut ApplicationSurfaceController, backend: &mut Fixture, ch: char) {
    surface.handle(backend, key(KeyCode::Char(ch))).unwrap();
}

#[test]
fn typing_a_query_that_keeps_the_current_selection_never_rereads_the_backend_wide_world_relation_or_inspector(
) {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("al"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    assert_eq!(surface.semantic().selected, Some(id("skill/alpha")));

    // Selecting must have populated Relation/Inspector at least once, so the
    // "unchanged" assertions below are proof of a real skip, not a vacuous
    // count that was always zero.
    assert!(surface.relation_refresh_count() > 0);
    assert!(surface.inspector_refresh_count() > 0);

    let world_before = surface.world_reads_refresh_count();
    let relation_before = surface.relation_refresh_count();
    let inspector_before = surface.inspector_refresh_count();

    // Three more keystrokes, each one narrowing the query while
    // "skill/alpha" keeps matching and keeps being the current selection —
    // the exact shape of the measured defect (one `SetQuery` per key).
    for ch in ['p', 'h', 'a'] {
        type_char(&mut surface, &mut backend, ch);
        assert_eq!(
            surface.semantic().selected,
            Some(id("skill/alpha")),
            "the selection must survive a query that keeps matching it"
        );
    }
    assert_eq!(surface.semantic().query, "alpha");

    assert_eq!(
        surface.world_reads_refresh_count(),
        world_before,
        "a query keystroke must never re-read the Project World, SessionSpace \
         roster, history evidence or Factory entry"
    );
    assert_eq!(
        surface.relation_refresh_count(),
        relation_before,
        "a query keystroke that does not move the selection must never \
         re-fetch the relation neighbourhood"
    );
    assert_eq!(
        surface.inspector_refresh_count(),
        inspector_before,
        "a query keystroke that does not move the selection must never \
         re-fetch the Inspector"
    );
}

#[test]
fn moving_the_selection_refreshes_relation_and_inspector_but_never_the_backend_wide_world() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        // Narrowed to the three skills alone: an empty query's zero-query
        // ordering surfaces `host/test-host` first, which would make the
        // first `Down` select the host rather than a skill and defeat the
        // point of this test (moving *between* two real selections).
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("skill/"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let first = surface.semantic().selected.clone();
    assert!(first.is_some(), "the first Down must select a resource");

    let world_before = surface.world_reads_refresh_count();
    let relation_before = surface.relation_refresh_count();
    let inspector_before = surface.inspector_refresh_count();

    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    assert_ne!(
        surface.semantic().selected, first,
        "the second Down must move the selection to a different resource"
    );

    assert!(
        surface.relation_refresh_count() > relation_before,
        "a genuine selection change must re-fetch the relation neighbourhood"
    );
    assert!(
        surface.inspector_refresh_count() > inspector_before,
        "a genuine selection change must re-fetch the Inspector"
    );
    assert_eq!(
        surface.world_reads_refresh_count(),
        world_before,
        "moving the selection cannot have mutated the backend, so the \
         backend-wide readings must stay untouched"
    );
}

#[test]
fn increasing_graph_depth_refreshes_the_relation_but_not_the_inspector_or_the_backend_wide_world()
{
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("al"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    for _ in 0..2 {
        surface
            .handle(
                &mut backend,
                PaletteEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
            )
            .unwrap();
    }
    assert_eq!(
        surface.semantic().relation_view,
        aikit_tui::RelationView::Graph
    );

    let world_before = surface.world_reads_refresh_count();
    let relation_before = surface.relation_refresh_count();
    let inspector_before = surface.inspector_refresh_count();
    let depth_before = surface.semantic().graph.depth;

    surface.handle(&mut backend, key(KeyCode::Char('+'))).unwrap();
    assert!(surface.semantic().graph.depth > depth_before);

    assert!(
        surface.relation_refresh_count() > relation_before,
        "a genuine depth change must re-fetch the relation neighbourhood at \
         the new depth"
    );
    assert_eq!(
        surface.inspector_refresh_count(),
        inspector_before,
        "a depth change alone does not move the selection, so the Inspector \
         must not be re-fetched"
    );
    assert_eq!(
        surface.world_reads_refresh_count(),
        world_before,
        "a depth change cannot have mutated the backend"
    );
}

#[test]
fn invoking_a_contextual_action_refreshes_the_entire_backend_wide_world_because_it_may_have_mutated_it(
) {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("al"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    assert_eq!(surface.semantic().selected, Some(id("skill/alpha")));

    let world_before = surface.world_reads_refresh_count();
    let relation_before = surface.relation_refresh_count();
    let inspector_before = surface.inspector_refresh_count();

    // "Toggle activation" is the only `Stageable` contextual Action a plain
    // Capability exposes, so Insert invokes it directly
    // (`stage_selected`'s single-stageable-action branch) without a detour
    // through the Action search overlay.
    surface
        .handle(&mut backend, key(KeyCode::Insert))
        .unwrap();
    assert!(
        surface.semantic().staged.get(&id("skill/alpha")).is_some(),
        "Insert must have staged the Capability's one stageable Action"
    );

    assert!(
        surface.world_reads_refresh_count() > world_before,
        "invoking a contextual Action can reach an arbitrary backend \
         mutation (`invoke_action` always records familiarity use, and some \
         Actions do far more), so the backend-wide readings must be \
         re-read even though this one only staged a local intent"
    );
    assert!(
        surface.relation_refresh_count() > relation_before,
        "the backend-wide re-read subsumes the relation re-fetch"
    );
    assert!(
        surface.inspector_refresh_count() > inspector_before,
        "the backend-wide re-read subsumes the Inspector re-fetch"
    );
}
