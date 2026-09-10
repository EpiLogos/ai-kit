//! Acceptance evidence for the Ctrl+K Universal Navigator's grouped result
//! presentation (`docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §3.3:
//! DESTINATIONS / RESOURCES / RECENT ROUTES), commissioned on top of PR
//! #221's Navigator (`tui/navigator-w1`).
//!
//! Coverage matches the four requirements the presentation itself must
//! satisfy: deterministic grouped order (wide/narrow, ASCII/Unicode
//! goldens), an empty group prints no header, selection/mouse parity
//! survives group boundaries and never lands on a header, and Recent Routes
//! actually reads off `TuiState::navigation`.
//!
//! `navigator_groups.rs`'s own `#[cfg(test)]` module already proves the
//! grouping/ordering/windowing functions correct as pure functions; this
//! file proves the same behaviour survives the real
//! `ApplicationSurfaceController` keyboard/mouse/render pipeline.

mod common;

use common::*;

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::{Glyphs, Layout};
use aikit_tui::PresentationMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Shared small helpers (same idiom as graph_surface_v2.rs / knowledge_graph_v2.rs)
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn mouse(column: u16, row: u16) -> PaletteEvent {
    PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn type_query(surface: &mut ApplicationSurfaceController, backend: &mut Fixture, query: &str) {
    for ch in query.chars() {
        surface.handle(backend, key(KeyCode::Char(ch))).unwrap();
    }
}

/// Two real capsules resolved through the real catalogue/trust/resolve
/// pipeline (the same shape every other V2 surface test uses), plus the six
/// Workspace destinations `workspace_navigation.rs` always installs into
/// the one shared index. `skill/ops/system-check`'s id/description contain
/// "system" as a plain substring, so querying "system" deterministically
/// yields both the `System` destination Surface and this one plain
/// resource — one hit per group, which is exactly what the boundary-
/// crossing/mouse/recent-route tests below need. `script/ops/deploy` never
/// matches "system" (no ordered s-y-s-t-e-m subsequence in its id or
/// description) and stays inert — present only to prove a non-matching
/// capsule does not leak into the grouped result.
fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![skill("skill/ops/system-check"), script("script/ops/deploy")],
    );
    (dir, backend)
}

/// Open the surface, switch to the Ctrl+K Navigator exactly as a viewer
/// would (`ctrl_k_navigator_finds_and_opens_a_workspace_destination` in
/// `project_world_surface_v2.rs` establishes this same sequence), then type
/// `query`. `glyphs` is pinned rather than left to `Glyphs::from_env()` so a
/// snapshot recorded here can never depend on the recording machine's
/// locale (see `knowledge_graph_v2.rs`'s own note on this for
/// `GraphGlyphs`).
fn navigator(backend: &mut Fixture, query: &str, glyphs: Glyphs) -> ApplicationSurfaceController {
    let mut surface = ApplicationSurfaceController::new(
        backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_glyphs(glyphs),
    )
    .unwrap();
    surface.handle(backend, ctrl(KeyCode::Char('k'))).unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Quick);
    type_query(&mut surface, backend, query);
    surface
}

fn draw(surface: &ApplicationSurfaceController, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

/// The rendered buffer as one row per terminal line, trailing blanks
/// trimmed — legible in a snapshot diff, unlike one flattened string.
fn rendered_rows(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Only the resource-list pane's own cells, excluding the shell's border
/// (ratatui's `BorderType::Plain`, always Unicode box-drawing regardless of
/// `Glyphs`), title and footer chrome (both carry pre-existing, unrelated,
/// unconditional `·`/arrow punctuation this ticket does not touch — see the
/// final report). This is the region the Navigator's own grouped-result
/// code actually draws into, and therefore the region an ASCII-glyphs
/// pin can honestly be asserted to be entirely ASCII.
fn rendered_list_pane(surface: &ApplicationSurfaceController, width: u16, height: u16) -> String {
    let terminal = draw(surface, width, height);
    let inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
    let list = Layout::for_width(inner.width).split(inner).list;
    let buffer = terminal.backend().buffer();
    (list.y..list.y + list.height)
        .map(|y| {
            (list.x..list.x + list.width)
                .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Snapshots — wide / narrow, ASCII / Unicode
// ---------------------------------------------------------------------------

#[test]
fn snapshot_wide_grouped_navigator_ascii() {
    let (_dir, mut backend) = fixture();
    let surface = navigator(&mut backend, "system", Glyphs::ascii());
    let text = rendered_rows(&draw(&surface, 120, 30));
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_wide_grouped_navigator_unicode() {
    let (_dir, mut backend) = fixture();
    let surface = navigator(&mut backend, "system", Glyphs::unicode());
    let text = rendered_rows(&draw(&surface, 120, 30));
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_narrow_grouped_navigator_ascii() {
    let (_dir, mut backend) = fixture();
    let surface = navigator(&mut backend, "system", Glyphs::ascii());
    let text = rendered_rows(&draw(&surface, 40, 20));
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_narrow_grouped_navigator_unicode() {
    let (_dir, mut backend) = fixture();
    let surface = navigator(&mut backend, "system", Glyphs::unicode());
    let text = rendered_rows(&draw(&surface, 40, 20));
    insta::assert_snapshot!(text);
}

// ---------------------------------------------------------------------------
// A group absent for a given query prints no header
// ---------------------------------------------------------------------------

#[test]
fn a_query_matching_no_destination_and_no_history_renders_only_the_resources_header() {
    let dir = tempfile::tempdir().unwrap();
    // No character of the six destinations' name/slug/description/kind
    // fields is a digit, so a digit-bearing query cannot fuzzy-subsequence-
    // match any of them — a clean way to force zero Destinations hits
    // without depending on the exact wording of any destination's copy.
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/ops/only9")]);
    let surface = navigator(&mut backend, "only9", Glyphs::unicode());
    let text = rendered_rows(&draw(&surface, 120, 30));

    assert!(
        text.contains("RESOURCES"),
        "the one matching resource must still render:\n{text}"
    );
    assert!(
        !text.contains("DESTINATIONS"),
        "no Surface matched this query:\n{text}"
    );
    assert!(
        !text.contains("RECENT ROUTES"),
        "nothing has been navigated yet:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// Selection crosses group boundaries without ever landing on a header
// ---------------------------------------------------------------------------

#[test]
fn keyboard_selection_walks_every_resource_across_group_boundaries_and_never_selects_a_header() {
    let (_dir, mut backend) = fixture();
    let mut surface = navigator(&mut backend, "system", Glyphs::unicode());

    let total = surface.semantic().read_model.resources.len();
    assert!(
        total >= 2,
        "fixture must expose more than one hit to cross a group boundary"
    );

    for expected in &surface.semantic().read_model.resources.clone() {
        surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
        assert_eq!(
            surface.semantic().selected.as_ref(),
            Some(&expected.resource),
            "Down must walk `read_model.resources` in order regardless of any header/spacer \
             row interleaved for presentation"
        );
    }

    // Confirm the currently-drawn cursor glyph lands on the final resource's
    // own line, never on a header line.
    let terminal = draw(&surface, 120, 30);
    let text = rendered_rows(&terminal);
    let last = surface.semantic().read_model.resources.last().unwrap();
    let cursor_line = text
        .lines()
        .find(|line| line.contains(Glyphs::unicode().list_cursor()))
        .expect("exactly one line carries the selection cursor");
    assert!(
        cursor_line.contains(last.label.as_str()),
        "the selection cursor must be on the last selected resource's own row, not a header:\n{cursor_line}"
    );
    assert!(!cursor_line.contains("DESTINATIONS"));
    assert!(!cursor_line.contains("RESOURCES"));
    assert!(!cursor_line.contains("RECENT ROUTES"));
}

#[test]
fn mouse_click_on_a_group_header_is_a_no_op_but_a_click_on_a_resource_row_selects_it() {
    let (_dir, mut backend) = fixture();
    let mut surface = navigator(&mut backend, "system", Glyphs::unicode());
    let _ = draw(&surface, 120, 30);

    let inner = Rect::new(1, 1, 118, 28);
    let list = Layout::for_width(inner.width).split(inner).list;

    // Compute the click targets as owned values first: `rows` borrows
    // `surface.semantic()`, and it must be dropped before the next
    // `surface.handle(&mut ...)` call below.
    let (header_offset, item_offset, item_resource) = {
        let rows = aikit_tui::navigator_groups::resource_pane_rows(surface.semantic());
        let header_offset = rows
            .iter()
            .position(|row| matches!(row, aikit_tui::navigator_groups::NavigatorRow::Header(_)))
            .expect("a grouped Navigator with any hits always has at least one header");
        let (item_offset, item_resource) = rows
            .iter()
            .enumerate()
            .find_map(|(offset, row)| match row {
                aikit_tui::navigator_groups::NavigatorRow::Item { item, .. } => {
                    Some((offset, item.resource.clone()))
                }
                _ => None,
            })
            .expect("at least one resource row is present");
        (header_offset, item_offset, item_resource)
    };
    let before = surface.semantic().selected.clone();

    surface
        .handle(
            &mut backend,
            mouse(list.x + 1, list.y + header_offset as u16),
        )
        .unwrap();
    assert_eq!(
        surface.semantic().selected,
        before,
        "clicking a group header must not change the selection"
    );

    // Now click a real resource row and confirm it lands on the same
    // resource keyboard navigation would select at that row.
    surface
        .handle(&mut backend, mouse(list.x + 1, list.y + item_offset as u16))
        .unwrap();
    assert_eq!(surface.semantic().selected.as_ref(), Some(&item_resource));
}

// ---------------------------------------------------------------------------
// Recent Routes reads off TuiState::navigation, not a name-matching table
// ---------------------------------------------------------------------------

#[test]
fn a_resource_the_viewer_already_navigated_through_reappears_under_recent_routes() {
    let (_dir, mut backend) = fixture();
    let mut surface = navigator(&mut backend, "system", Glyphs::unicode());

    // Select the plain (non-Surface) capability hit and invoke its global
    // History action — one of `reduce_tui`'s three `ActionOutcome` arms
    // (`Opened`/`History`/`NavigatedTo`) that push a `NavigationPoint`.
    let system_check = surface
        .semantic()
        .read_model
        .resources
        .iter()
        .find(|item| item.resource.as_str() == "skill/ops/system-check")
        .expect("the fixture skill must be present for query \"system\"")
        .clone();
    let index = surface
        .semantic()
        .read_model
        .position(&system_check.resource)
        .unwrap();
    for _ in 0..=index {
        surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    }
    assert_eq!(
        surface.semantic().selected.as_ref(),
        Some(&system_check.resource)
    );

    surface
        .handle(&mut backend, key(KeyCode::Char(':')))
        .unwrap();
    for ch in "history".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(ch)))
            .unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();

    assert!(
        surface
            .semantic()
            .navigation
            .iter()
            .any(|point| point.selected.as_ref() == Some(&system_check.resource)),
        "invoking History must push a NavigationPoint carrying the resource that was selected"
    );
    assert_eq!(
        surface.semantic().presentation,
        PresentationMode::Quick,
        "invoking History does not itself change presentation"
    );

    // The read model itself never changed (no new `SetQuery` was
    // dispatched) — only presentation grouping did, purely from
    // `state.navigation` now carrying this resource. This is the point of
    // constraint 3: grouping is read off structure already in hand, not
    // re-derived through a fresh search.
    use aikit_tui::navigator_groups::{group_for, NavigatorGroup};
    assert_eq!(
        group_for(&system_check, &surface.semantic().navigation),
        NavigatorGroup::RecentRoutes
    );

    // Restricted to the list pane itself so the query line's own echo of
    // "system" (`/ system`) cannot be mistaken for a second row.
    let text = rendered_list_pane(&surface, 120, 30);
    assert!(
        text.contains("RECENT ROUTES"),
        "recent route header must render:\n{text}"
    );
    // The fixture's only other hit for this query is "system-check" itself, so
    // once it moves to RECENT ROUTES the RESOURCES group is empty and must
    // print no header at all (§3.3 requirement 2).
    assert!(
        !text.contains("RESOURCES"),
        "the only resource for this query became a recent route, so RESOURCES must have no header:\n{text}"
    );
    assert_eq!(
        text.matches("system-check").count(),
        1,
        "the visited resource must appear exactly once, under RECENT ROUTES, never duplicated:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// Determinism: same input renders the same grouped order every time
// ---------------------------------------------------------------------------

#[test]
fn rendering_the_same_state_twice_produces_byte_identical_output() {
    let (_dir, mut backend) = fixture();
    let surface = navigator(&mut backend, "system", Glyphs::unicode());
    let first = rendered_rows(&draw(&surface, 120, 30));
    let second = rendered_rows(&draw(&surface, 120, 30));
    assert_eq!(first, second);
}

#[test]
fn two_independently_built_surfaces_over_the_same_fixture_group_identically() {
    let (_dir_a, mut backend_a) = fixture();
    let (_dir_b, mut backend_b) = fixture();
    let surface_a = navigator(&mut backend_a, "system", Glyphs::unicode());
    let surface_b = navigator(&mut backend_b, "system", Glyphs::unicode());
    assert_eq!(
        rendered_rows(&draw(&surface_a, 120, 30)),
        rendered_rows(&draw(&surface_b, 120, 30))
    );
}

// ---------------------------------------------------------------------------
// Glyphs are a capability, not a literal: the whole frame draws no non-ASCII
// byte under a pinned ASCII glyph set.
//
// This assertion was scoped to the resource-list pane while this branch sat
// on a base that predated the shell glyph capability, because
// `Theme::border_type()` still returned `BorderType::Plain` there and drew
// Unicode box-drawing whatever the glyph set said. On `main` the frame comes
// from `Glyphs::border_set` and the title/footer chrome from the same
// resolved set, so the guarantee now holds over the entire terminal buffer —
// which is what makes it a real guarantee rather than one true only of the
// rows this branch happened to touch.
// ---------------------------------------------------------------------------

#[test]
fn nothing_in_an_ascii_rendering_is_non_ascii() {
    let (_dir, mut backend) = fixture();
    let surface = navigator(&mut backend, "system", Glyphs::ascii());
    let list_text = rendered_rows(&draw(&surface, 120, 30));
    assert!(
        list_text.is_ascii(),
        "an ASCII-glyphs rendering must be entirely ASCII across the whole frame:\n{list_text}"
    );

    // Same guarantee at narrow width, where indentation/truncation budgets
    // are tightest.
    let narrow_surface = navigator(&mut backend, "system", Glyphs::ascii());
    let narrow_text = rendered_rows(&draw(&narrow_surface, 40, 20));
    assert!(
        narrow_text.is_ascii(),
        "narrow ASCII rendering must also be entirely ASCII:\n{narrow_text}"
    );
}
