//! Regression coverage for W2's wired-in production Graph presentation:
//! Graph presentation state (focus/history/depth/filter) driven exclusively
//! through `UiAction`, depth reaching the application service, spatial vs
//! narrow projection, and keyboard/mouse parity for node selection and
//! recenter.

mod common;

use common::*;

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::graph_layout::{layout as graph_layout, GraphLayoutRequest, GraphViewport};
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use aikit_tui::RelationView;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

/// Two real skills, resolved through the real catalogue/trust/resolve
/// pipeline, `alpha` declaring `beta` and `gamma` as `related_skills` — the
/// same resolver-fallback shape `application_service.rs`'s own tests use.
fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let alpha = skill_with(
        "skill/alpha",
        "related_skills = [\"skill/beta\", \"skill/gamma\"]\n",
    );
    let backend = Fixture::new(
        dir.path(),
        vec![alpha, skill("skill/beta"), skill("skill/gamma")],
    );
    (dir, backend)
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn mouse(column: u16, row: u16, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers,
    })
}

fn draw(surface: &ApplicationSurfaceController, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
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

/// Enter the Graph projection with `alpha` selected: query for it, resize,
/// then Ctrl+T through List -> Tree -> Graph exactly as a viewer would.
fn enter_graph(
    surface: &mut ApplicationSurfaceController,
    backend: &mut Fixture,
    width: u16,
    height: u16,
) {
    surface
        .handle(backend, PaletteEvent::Resize(width, height))
        .unwrap();
    surface
        .handle(
            backend,
            PaletteEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        )
        .unwrap();
    for _ in 0..2 {
        surface
            .handle(
                backend,
                PaletteEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
            )
            .unwrap();
    }
    assert_eq!(surface.semantic().relation_view, RelationView::Graph);
}

#[test]
fn ctrl_t_rotates_into_graph_and_lays_out_the_resolver_neighbourhood() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);

    let relation = surface
        .relation()
        .expect("alpha has a relation neighbourhood");
    assert_eq!(relation.subject.as_str(), "skill/alpha");
    assert!(relation
        .view
        .nodes
        .iter()
        .any(|node| node.resource.as_str() == "skill/beta"));
    assert!(relation
        .view
        .nodes
        .iter()
        .any(|node| node.resource.as_str() == "skill/gamma"));

    let terminal = draw(&surface, 120, 30);
    let text = rendered(&terminal);
    assert!(
        text.contains("Outgoing"),
        "spatial legend must label the Outgoing band:\n{text}"
    );
    assert!(
        text.contains("Inspector"),
        "spatial rendering must show the Inspector section:\n{text}"
    );
}

#[test]
fn arrow_navigation_moves_the_highlight_without_recentering_the_neighbourhood() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    let alpha = surface.semantic().selected.clone().unwrap();
    assert_eq!(alpha.as_str(), "skill/alpha");
    // Entering Graph latches its focus onto whatever was selected at that
    // moment (see `reduce_tui`'s `SetRelationView` arm) — otherwise the
    // "follow selection when nothing has recentred" fallback would keep
    // tracking `selected` through every subsequent arrow-key move.
    assert_eq!(surface.semantic().graph.focus, Some(alpha.clone()));

    surface.handle(&mut backend, key(KeyCode::Right)).unwrap();

    let highlighted = surface.semantic().selected.clone().unwrap();
    assert_ne!(
        highlighted, alpha,
        "arrow navigation must move the highlight onto a neighbour"
    );
    assert!(
        highlighted.as_str() == "skill/beta" || highlighted.as_str() == "skill/gamma",
        "highlight must land on a real laid-out neighbour, got {highlighted}"
    );
    // Cursor movement alone must not recentre the neighbourhood: the fetched
    // relation subject stays `alpha`.
    assert_eq!(
        surface.relation().unwrap().subject.as_str(),
        "skill/alpha",
        "arrow navigation is not a recenter"
    );
    assert_eq!(surface.semantic().graph.focus, Some(alpha));
}

#[test]
fn enter_recenters_and_esc_walks_graph_history_before_falling_through_to_back() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);

    surface.handle(&mut backend, key(KeyCode::Right)).unwrap();
    let neighbour = surface.semantic().selected.clone().unwrap();
    assert_ne!(neighbour.as_str(), "skill/alpha");

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(surface.semantic().graph.focus, Some(neighbour.clone()));
    assert_eq!(
        surface.semantic().graph.history,
        vec![aikit_core::ResourceRef::parse("skill/alpha").unwrap()]
    );
    assert_eq!(surface.relation().unwrap().subject, neighbour);

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(
        surface.semantic().graph.focus,
        Some(aikit_core::ResourceRef::parse("skill/alpha").unwrap()),
        "Esc must pop the Graph's own recenter history first"
    );
    assert!(surface.semantic().graph.history.is_empty());
    assert_eq!(surface.relation().unwrap().subject.as_str(), "skill/alpha");

    // History is now empty: Esc must fall through to ordinary Back rather
    // than being swallowed. There is nothing on the navigation stack in this
    // scenario, so Back is a documented no-op — the assertion is that this
    // does not panic and leaves state exactly as ordinary Back would.
    let before = surface.semantic().clone();
    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(surface.semantic().selected, before.selected);
    assert_eq!(surface.semantic().graph.focus, before.graph.focus);
}

#[test]
fn depth_control_reaches_the_application_service() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);

    assert_eq!(surface.semantic().graph.depth, 1);
    assert_eq!(surface.relation().unwrap().view.query.depth, 1);

    surface
        .handle(&mut backend, key(KeyCode::Char('+')))
        .unwrap();
    assert_eq!(surface.semantic().graph.depth, 2);
    assert_eq!(
        surface.relation().unwrap().view.query.depth,
        2,
        "a graph depth change must reach the application service, not stay a hardcoded fetch"
    );

    surface
        .handle(&mut backend, key(KeyCode::Char('-')))
        .unwrap();
    surface
        .handle(&mut backend, key(KeyCode::Char('-')))
        .unwrap();
    assert_eq!(
        surface.semantic().graph.depth,
        1,
        "depth is bounded at MIN_DEPTH"
    );
}

#[test]
fn narrow_geometry_falls_back_to_the_grouped_projection_while_staying_semantically_graph() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 40, 20);

    assert_eq!(surface.semantic().relation_view, RelationView::Graph);
    let terminal = draw(&surface, 40, 20);
    let text = rendered(&terminal);
    assert!(
        text.contains("alpha"),
        "narrow fallback still names the focus:\n{text}"
    );
    assert!(
        text.contains("Outgoing"),
        "narrow fallback still bands relations, just without a spatial canvas:\n{text}"
    );
    // The narrow canvas legend/Inspector text is absent from this projection.
    assert!(!text.contains("Inspector"));
}

#[test]
fn mouse_click_and_shift_click_resolve_to_the_same_actions_as_keyboard_navigation_and_recenter() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    let _ = draw(&surface, 120, 30);

    // Reproduce the controller's own content-rect and canvas-viewport math
    // (documented in `graph_content_rect`/`graph_viewport`) to find a real
    // node's screen coordinates without depending on any private accessor.
    let inner = Rect::new(1, 1, 118, 28);
    let list = Layout::for_width(inner.width).split(inner).list;
    let content = Rect::new(
        list.x.saturating_add(1),
        list.y.saturating_add(1),
        list.width.saturating_sub(2),
        list.height.saturating_sub(2),
    );
    const MIN_CANVAS_HEIGHT: u16 = 5;
    let canvas_height = if content.height <= MIN_CANVAS_HEIGHT {
        content.height
    } else {
        (content.height * 3 / 5).max(MIN_CANVAS_HEIGHT)
    };
    let relation = surface.relation().unwrap().clone();
    let request =
        GraphLayoutRequest::for_viewport(GraphViewport::new(content.width, canvas_height));
    let laid = graph_layout(&relation.view, &request);
    let beta = laid
        .nodes
        .iter()
        .find(|node| node.resource.as_str() == "skill/beta")
        .expect("skill/beta must be laid out");
    let column = content.x + u16::try_from(beta.position.x).unwrap();
    let row = content.y + u16::try_from(beta.position.y).unwrap();

    surface
        .handle(&mut backend, mouse(column, row, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        surface.semantic().selected,
        Some(aikit_core::ResourceRef::parse("skill/beta").unwrap()),
        "a plain click must select the clicked node, same as arrow navigation"
    );
    assert_eq!(
        surface.semantic().graph.focus,
        Some(aikit_core::ResourceRef::parse("skill/alpha").unwrap()),
        "a plain click must not recentre"
    );

    surface
        .handle(&mut backend, mouse(column, row, KeyModifiers::SHIFT))
        .unwrap();
    assert_eq!(
        surface.semantic().graph.focus,
        Some(aikit_core::ResourceRef::parse("skill/beta").unwrap()),
        "a modifier-click must recentre, same as Enter"
    );
}

#[test]
fn tree_view_refuses_to_fabricate_hierarchy_from_non_containment_relations() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    surface
        .handle(&mut backend, PaletteEvent::Resize(120, 30))
        .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    // List -> Tree (one Ctrl+T).
    surface
        .handle(
            &mut backend,
            PaletteEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
        )
        .unwrap();
    assert_eq!(surface.semantic().relation_view, RelationView::Tree);

    // The resolver's "related-skill" edges are real and typed (List/Graph
    // show them), but they are Bidirectional, not containment — Tree must
    // decline to invent a hierarchy out of them.
    let relation = surface.relation().unwrap();
    assert!(!relation.view.edges.is_empty());

    let terminal = draw(&surface, 120, 30);
    let text = rendered(&terminal);
    assert!(
        text.contains("no genuine containment relation"),
        "related-skill edges are not containment and must not be drawn as a fake tree:\n{text}"
    );
    assert!(
        !text.contains("skill/beta"),
        "Tree must not fabricate a hierarchy row for a non-containment relation:\n{text}"
    );
}

#[test]
fn a_wide_but_shallow_terminal_also_falls_back_to_the_grouped_projection() {
    // Width alone (>= 60 cols, i.e. not `Width::Narrow`) is not sufficient
    // geometry for the spatial canvas: a wide-but-short pane cannot show a
    // useful vertical Incoming/Outgoing picture either, so Graph must still
    // fall back — this is the height floor `graph_is_spatial` adds on top of
    // the crate's own `Width::Narrow` breakpoint.
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    // 120 columns is comfortably `Width::Wide`; 11 rows yields a content
    // rect just under `MIN_SPATIAL_HEIGHT` (5 < 6) once the shell's own
    // chrome (title/query/footer borders) is accounted for.
    enter_graph(&mut surface, &mut backend, 120, 11);

    assert_eq!(surface.semantic().relation_view, RelationView::Graph);
    let terminal = draw(&surface, 120, 11);
    let text = rendered(&terminal);
    assert!(text.contains("Outgoing"), "still bands relations:\n{text}");
    assert!(
        !text.contains("Inspector"),
        "a wide-but-shallow pane must use the grouped fallback, not the spatial canvas:\n{text}"
    );
}
