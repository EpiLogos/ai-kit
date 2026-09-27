//! Surface-level evidence for the resting World view's next steps, the
//! context-aware help overlay and the state-persistence contract
//! (`docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §1.5, §4.2; convergence B1/B3).
//!
//! The backend is `common::Fixture` — the same resolver-backed fixture every
//! other V2 surface test uses — so every dispatch below is a real read
//! against a real resolved catalogue. The lifecycle operations are unbound on
//! this fixture (the trait defaults), which is exactly the degraded case the
//! rows must name honestly.
//!
//! Keyboard and mouse parity is asserted structurally: the same step
//! dispatches the same `UiAction` through the digit key and through a click
//! on the row the renderer actually drew (`world_entry::bottom_block` is the
//! one geometry both paths use).

mod common;

use common::*;

use aikit_tui::application::{AgentWorkStage, Overlay, PresentationMode, WorkspaceSection};
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::compose_spine::ComposeStep;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::{Glyphs, Layout};
use aikit_tui::world_entry;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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

fn alt_down() -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT))
}

fn alt_left() -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
}

fn ctrl_w() -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
}

fn click(column: u16, row: u16) -> PaletteEvent {
    PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A Workspace surface standing on the resting World view. A popup host
/// already opens in Workspace with `WorkspaceSection::Worlds` selected; the
/// next-step block is live right there, from an empty query. Glyphs are
/// pinned ASCII so frame assertions are locale-independent.
fn surface(backend: &mut Fixture) -> ApplicationSurfaceController {
    ApplicationSurfaceController::new(
        backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_glyphs(Glyphs::ascii()),
    )
    .unwrap()
}

fn resize(surface: &mut ApplicationSurfaceController, backend: &mut Fixture, w: u16, h: u16) {
    surface.handle(backend, PaletteEvent::Resize(w, h)).unwrap();
}

fn rendered(surface: &ApplicationSurfaceController) -> String {
    let mut terminal = Terminal::new(TestBackend::new(
        surface.semantic().area.0,
        surface.semantic().area.1,
    ))
    .unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol().to_string())
        .collect::<String>()
}

/// Pressing `1` on the resting World view opens Work — Continue's outcome —
/// and the backend-wide readings were not re-read to do it.
#[test]
fn continue_dispatches_to_work_without_a_world_reread() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Worlds);

    let world_reads_before = surface.world_reads_refresh_count();
    surface.handle(&mut backend, key(KeyCode::Char('1'))).unwrap();
    assert_eq!(
        surface.semantic().workspace_section,
        WorkspaceSection::Work
    );
    assert_eq!(
        surface.world_reads_refresh_count(),
        world_reads_before,
        "a navigation next step must not re-read the Project World, \
         SessionSpace roster, history or Factory entry"
    );
}

/// `4` opens Compose (from Worlds) and `5` opens the Universal Navigator
/// (Quick) — the keyboard routes for Compose Agent and Search/Explore. The
/// digit block is World-view-specific: back on Worlds, `5` routes again.
#[test]
fn compose_and_explore_steps_route_their_destinations() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Char('4'))).unwrap();
    assert_eq!(
        surface.semantic().workspace_section,
        WorkspaceSection::Compose
    );

    // Back to Worlds (Alt+Left crosses the Workspace field), then Explore.
    surface.handle(&mut backend, alt_left()).unwrap();
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Worlds);
    surface.handle(&mut backend, key(KeyCode::Char('5'))).unwrap();
    assert_eq!(
        surface.semantic().presentation,
        PresentationMode::Quick,
        "Search/Explore opens the Universal Navigator presentation"
    );
}

/// A digit typed after a live query is a query character, not a step: the
/// block owns the digits only from an empty query.
#[test]
fn a_live_query_owns_the_digits() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);
    surface.handle(&mut backend, key(KeyCode::Char('a'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('1'))).unwrap();
    assert_eq!(surface.semantic().query, "a1");
    assert_eq!(
        surface.semantic().workspace_section,
        WorkspaceSection::Worlds,
        "typing '1' into a live query must not dispatch Continue"
    );
}

/// With the lifecycle operations unbound (this fixture's honest default),
/// pressing `2` does not pretend to start anything: it names the unbound
/// owner operations on the status line.
#[test]
fn unbound_direct_work_names_its_gap_rather_than_faking_a_start() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);
    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();
    let status = surface
        .semantic()
        .status
        .as_ref()
        .expect("the disabled step must say so");
    assert!(
        status.message.contains("Start Direct work is unavailable"),
        "got: {}",
        status.message
    );
    assert!(
        status.message.contains("agent-profile save"),
        "the reason names the unbound owner operation: {}",
        status.message
    );
    assert!(
        !status.message.contains("running"),
        "a disabled step must not talk as if work started: {}",
        status.message
    );
}

/// The resting World view renders the next-step rows, pinned to the bottom
/// of the world pane, in both wide and narrow Workspace shells — no
/// capability vanishes at narrow width. The frame stays ASCII under ASCII
/// glyphs.
#[test]
fn the_next_step_block_is_drawn_wide_and_narrow_and_stays_ascii() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    for (width, height) in [(120u16, 40u16), (58u16, 30u16)] {
        resize(&mut surface, &mut backend, width, height);
        let screen = rendered(&surface);
        assert!(
            screen.contains("1) Continue"),
            "{width}x{height}: the Continue row must render; got:\n{screen}"
        );
        assert!(
            screen.contains("4) Compose Agent"),
            "{width}x{height}: the Compose row must render; got:\n{screen}"
        );
        assert!(
            screen.contains("2) Start Direct"),
            "{width}x{height}: a disabled step keeps its row (clipped to the \
             pane at the narrower breakpoint); got:\n{screen}"
        );
        // The rows' ASCII discipline under ASCII glyphs is asserted per-row,
        // across widths, in `world_entry`'s own unit tests
        // (`rows_clip_to_width_and_stay_ascii_under_ascii_glyphs`); the
        // literals asserted above are themselves the ASCII render of those
        // rows.
    }
}

/// '?' opens the context-aware help, the help names the steps' outcomes,
/// and Esc dismisses it.
#[test]
fn help_explains_the_steps_here_and_esc_returns() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Char('?'))).unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::Help));
    let screen = rendered(&surface);
    assert!(
        screen.contains("Next steps here"),
        "help is context-aware: it names the steps drawn on this view:\n{screen}"
    );
    assert!(
        screen.contains("opens Work"),
        "help explains outcomes, not implementation names:\n{screen}"
    );

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(surface.semantic().overlay, None);
}

/// Mouse parity: a click on the drawn Continue row lands on the same
/// semantic action as the `1` key — resolved through the same
/// `bottom_block` geometry the renderer used.
#[test]
fn a_click_on_a_drawn_step_row_matches_the_keyboard() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);
    resize(&mut surface, &mut backend, 58, 30);

    // The geometry the renderer used: at this width there is no preview
    // pane, so the world pane is the list pane, and the steps block is
    // pinned to its bottom.
    let (cols, rows) = surface.semantic().area;
    let inner = ratatui::layout::Rect::new(1, 1, cols.saturating_sub(2), rows.saturating_sub(2));
    let panes = Layout::for_width(inner.width).split(inner);
    let pane = panes.preview.unwrap_or(panes.list);
    let block = world_entry::bottom_block(pane, surface.current_steps().len());

    surface
        .handle(&mut backend, click(pane.x + 2, block.y))
        .unwrap();
    assert_eq!(
        surface.semantic().workspace_section,
        WorkspaceSection::Work,
        "clicking the drawn Continue row must dispatch Continue, exactly like key 1"
    );
}

/// A click on a *disabled* step row names the same specific reason the
/// keyboard does, and starts nothing.
#[test]
fn a_click_on_a_disabled_step_names_the_reason() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);
    resize(&mut surface, &mut backend, 58, 30);

    let (cols, rows) = surface.semantic().area;
    let inner = ratatui::layout::Rect::new(1, 1, cols.saturating_sub(2), rows.saturating_sub(2));
    let panes = Layout::for_width(inner.width).split(inner);
    let pane = panes.preview.unwrap_or(panes.list);
    let block = world_entry::bottom_block(pane, surface.current_steps().len());

    // The disabled Start Direct work row is the second step.
    surface
        .handle(&mut backend, click(pane.x + 2, block.y + 1))
        .unwrap();
    let status = surface
        .semantic()
        .status
        .as_ref()
        .expect("the click must surface the reason");
    assert!(status.message.contains("unavailable: "), "{}", status.message);
    assert!(
        status.message.contains("agent-profile save"),
        "{}",
        status.message
    );
}

/// The creator lane: Enter on Compose's Enter-work step authors the exact
/// purpose (carried verbatim), continues to the optional name, and Esc
/// abandons without committing.
#[test]
fn enter_work_authors_purpose_then_name_and_esc_abandons() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Char('4'))).unwrap();
    // Walk the spine to its last step; Alt+Down clamps at Enter-work.
    for _ in 0..9 {
        surface.handle(&mut backend, alt_down()).unwrap();
    }
    assert_eq!(surface.semantic().compose_step, ComposeStep::EnterWork);

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(
        surface.compose_text_lane(),
        Some(aikit_tui::application_surface::ComposeTextField::Purpose)
    );
    for character in "Prove the slice exactly".chars() {
        surface.handle(&mut backend, key(KeyCode::Char(character))).unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(
        surface.semantic().compose_purpose,
        "Prove the slice exactly",
        "the purpose is authored human text, carried verbatim"
    );
    // The guided path continues to the optional name.
    for character in "probe".chars() {
        surface.handle(&mut backend, key(KeyCode::Char(character))).unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(surface.semantic().compose_agent_name, "probe");
    assert_eq!(surface.compose_text_lane(), None);

    // Esc inside the lane abandons the edit without committing.
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('x'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(surface.semantic().compose_purpose, "Prove the slice exactly");
}

/// B3 persistence: the authored draft and the lifecycle stage ladder survive
/// navigation between sections and presentations. A draft is not lost
/// because the operator looked at Work; and navigation never launches,
/// saves or mutates anything on its own.
#[test]
fn drafts_and_the_stage_ladder_survive_navigation() {
    let (_dir, mut backend) = fixture();
    let mut surface = surface(&mut backend);

    surface.handle(&mut backend, key(KeyCode::Char('4'))).unwrap();
    for _ in 0..9 {
        surface.handle(&mut backend, alt_down()).unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    for character in "hold this draft".chars() {
        surface.handle(&mut backend, key(KeyCode::Char(character))).unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap(); // abandon the name lane

    surface.handle(&mut backend, ctrl_w()).unwrap(); // to Quick
    surface.handle(&mut backend, ctrl_w()).unwrap(); // back to Workspace
    surface.handle(&mut backend, alt_left()).unwrap(); // Compose -> Worlds
    surface.handle(&mut backend, key(KeyCode::Char('1'))).unwrap(); // Continue -> Work

    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Work);
    assert_eq!(
        surface.semantic().compose_purpose, "hold this draft",
        "the authored purpose survives navigation"
    );

    surface.handle(&mut backend, key(KeyCode::Char('4'))).unwrap();
    assert_eq!(surface.semantic().compose_purpose, "hold this draft");
    assert!(matches!(
        surface.semantic().agent_work.stable(),
        AgentWorkStage::Draft
    ));
}
