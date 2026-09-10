//! Regression evidence for the Relations panel's opacity over the resource
//! list it is drawn on top of.
//!
//! `ApplicationSurfaceController::draw` (`application_surface.rs`) renders
//! the resting shell first — `v2_render::draw_with_context`/
//! `draw_with_project_world` fills `panes.list` with the ordinary resource
//! list — then, whenever the Workspace is showing the Knowledge section,
//! draws the Relations panel (`draw_relations`) directly on top of that
//! same `panes.list` Rect, later in the same frame. Neither `Paragraph` nor
//! `Block` clear the cells they do not themselves write (see
//! `ratatui::widgets::Clear`'s own doc comment: "this will clear/reset the
//! area first" is something a caller has to ask for), so whatever the list
//! drew a moment earlier stays in the buffer wherever the panel's own
//! content is shorter than the row it sits over. That is exactly what a
//! real tmux pane showed: the Tree view's "no genuine containment relation
//! in this neighbourhood — …" sentence with fragments of an unrelated
//! resource's label/summary bleeding through beside and below it.
//!
//! The fixture below deliberately matches more resources than the panel is
//! tall, each carrying a long, unmistakable marker in its description
//! instead of the fixture-standard "Test skill …" text. The Relations
//! panel's own content — a subject id and a fixed sentence
//! (`application_surface.rs`'s `tree_relation_lines`) — never prints a
//! resource's description, so finding a marker anywhere inside the panel's
//! own interior can only mean the list underneath showed through.

mod common;

use std::path::PathBuf;

use common::*;

use aikit_core::capsule::Capsule;
use aikit_core::id::{RegistrySource, Revision};

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use aikit_tui::RelationView;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Terminal;

/// More than any of the widths below can show in one screen's worth of
/// `panes.list` rows, so the pre-fix bug always has real content to bleed
/// regardless of exactly where the window happens to scroll.
const RESOURCE_COUNT: usize = 48;
const WIDTH: u16 = 200;
const HEIGHT: u16 = 50;

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// A marker distinctive enough that it cannot appear by coincidence: no
/// production or fixture string in this crate reads `MARKERLEAK`.
fn marker(n: usize) -> String {
    format!("MARKERLEAK{n:03}")
}

/// A plain skill with no relations of its own, carrying `marker(n)` in its
/// description in place of `common::manifest`'s standard "Test skill …"
/// text — built directly rather than through `common::skill_with`, whose
/// `top` parameter inserts a top-level field alongside the manifest's own
/// `description`, not in place of it.
fn skill_with_marker(id: &str, n: usize) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    let marker = marker(n);
    let src = format!(
        r#"schema = 1
id = "{id}"
kind = "skill"
name = "{leaf}"
description = "{marker} long description text, reaching well across the panel width, that must never be readable through the Relations panel's own interior."

[skill]
root = "payload"
"#
    );
    let mut capsule = Capsule::from_toml_str(&src)
        .unwrap_or_else(|e| panic!("fixture manifest for {id} should parse: {e}\n---\n{src}"));
    capsule.revision = Some(Revision::from_raw(format!("rev-{id}")));
    capsule.source = Some(RegistrySource::personal());
    capsule.root = Some(PathBuf::from(format!("/registry/{}", id.replace('/', "-"))));
    capsule
}

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let capsules = (0..RESOURCE_COUNT)
        .map(|n| skill_with_marker(&format!("skill/bleedtest/m{n:03}"), n))
        .collect();
    let backend = Fixture::new(dir.path(), capsules);
    (dir, backend)
}

fn draw(surface: &ApplicationSurfaceController) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

/// The Relations panel's own interior — inside its own border, inside the
/// resting shell's own outer border — read out one row per terminal line,
/// with no trailing trim: a bled character several columns past the
/// panel's own short content is exactly what this test is looking for, and
/// trimming would only ever hide it.
///
/// Recomputed independently from `WIDTH`/`HEIGHT` and `Layout::split`
/// rather than hand-picked coordinates, so this stays correct if either
/// pane's proportions ever change — the same reason
/// `wide_shell_inspector_v2.rs`'s own `rendered_inspector` recomputes
/// `panes.inspector` instead of hard-coding it.
fn relations_panel_interior(terminal: &Terminal<TestBackend>) -> String {
    let outer = Rect::new(0, 0, WIDTH, HEIGHT);
    let shell_inner = Block::default().borders(Borders::ALL).inner(outer);
    let panes = Layout::for_width(shell_inner.width).split(shell_inner);
    let relations_interior = Block::default().borders(Borders::ALL).inner(panes.list);
    let buffer = terminal.backend().buffer();
    (relations_interior.y..relations_interior.y + relations_interior.height)
        .map(|y| {
            (relations_interior.x..relations_interior.x + relations_interior.width)
                .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn relations_panel_is_opaque_over_a_resource_list_taller_than_itself() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("bleedtest")
            .opening_relations(RelationView::Tree),
    )
    .unwrap();
    assert_eq!(
        surface.semantic().read_model.resources.len(),
        RESOURCE_COUNT,
        "the fixture must actually match more resources than the panel is tall for this to prove anything"
    );

    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    assert!(surface.semantic().selected.is_some(), "Down must select the first result");
    assert_eq!(surface.semantic().relation_view, RelationView::Tree);

    let interior = relations_panel_interior(&draw(&surface));
    for n in 0..RESOURCE_COUNT {
        let needle = marker(n);
        assert!(
            !interior.contains(&needle),
            "resource list content ({needle}) bled through the Relations panel's own interior:\n{interior}"
        );
    }
}
