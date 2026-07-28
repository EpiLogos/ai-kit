//! The popup is one surface: palette and tree are modes, not terminal hand-offs.

mod common;

use common::*;

use aikit_core::Result;
use aikit_tui::event::{PaletteEvent, ScriptedEvents};
use aikit_tui::host::UiHost;
use aikit_tui::surface::{
    event_loop, SurfaceBackend, SurfaceController, SurfaceMode, SurfaceRequest, SurfaceStep,
};
use aikit_tui::tree::{Node, NodeKind, Root, TreeEffect, TreeState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

struct SurfaceFixture(Fixture, bool);

impl std::ops::Deref for SurfaceFixture {
    type Target = Fixture;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for SurfaceFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl aikit_tui::PaletteBackend for SurfaceFixture {
    fn context(&self) -> &aikit_core::ContextDescriptor {
        self.0.context()
    }

    fn view(&self) -> &aikit_core::resolve::ResolvedView {
        self.0.view()
    }

    fn documents(&self) -> Vec<aikit_core::search::SearchDoc> {
        self.0.documents()
    }

    fn capsule(&self, id: &aikit_core::CapsuleId) -> Option<&aikit_core::capsule::Capsule> {
        self.0.capsule(id)
    }

    fn recent(&self) -> Vec<aikit_tui::RunIntent> {
        self.0.recent()
    }

    fn preview(
        &self,
        scope: aikit_core::scope::ScopeKind,
        toggles: &[aikit_tui::Toggle],
    ) -> Result<aikit_tui::Projected> {
        self.0.preview(scope, toggles)
    }

    fn apply(
        &mut self,
        scope: aikit_core::scope::ScopeKind,
        toggles: &[aikit_tui::Toggle],
    ) -> Result<aikit_core::GenerationId> {
        self.0.apply(scope, toggles)
    }

    fn start(&mut self, intent: &aikit_tui::RunIntent) -> Result<aikit_tui::JobOutput> {
        self.0.start(intent)
    }

    fn open_source(&mut self, id: &aikit_core::CapsuleId) -> Result<std::path::PathBuf> {
        self.0.open_source(id)
    }

    fn promotion_drafts(&self) -> Vec<aikit_tui::PromotionDraft> {
        self.0.promotion_drafts()
    }

    fn promote(&mut self, draft: &aikit_tui::PromotionDraft) -> Result<aikit_core::CapsuleId> {
        self.0.promote(draft)
    }
}

impl SurfaceBackend for SurfaceFixture {
    fn surface_tree(&self) -> Result<TreeState> {
        Ok(TreeState::new(vec![Node::branch(
            NodeKind::Root(Root::Kinds),
            "one capability",
            vec![Node::leaf(
                NodeKind::Capability {
                    id: cid("skill/rust/review"),
                },
                "review",
            )],
        )]))
    }

    fn apply_tree_effect(&mut self, _effect: TreeEffect) -> Result<()> {
        if self.1 {
            Err(aikit_core::error::AikitError::new(
                "skillset.already_exists",
                "set already exists",
            ))
        } else {
            Ok(())
        }
    }
}

fn fixture() -> SurfaceFixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.keep();
    SurfaceFixture(
        Fixture::new(
            &root,
            vec![script("script/ops/deploy"), skill("skill/rust/review")],
        ),
        false,
    )
}

fn request(query: &str) -> SurfaceRequest {
    SurfaceRequest::new(UiHost::TmuxPopup).with_query(query)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

#[test]
fn switching_modes_preserves_both_views_and_shares_the_staged_graph() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(&mut backend, request("deploy")).unwrap();
    let ctrl = KeyModifiers::CONTROL;

    assert_eq!(surface.mode(), SurfaceMode::Palette);
    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Char('t'), ctrl))
            .unwrap(),
        SurfaceStep::Continue
    );
    assert_eq!(surface.mode(), SurfaceMode::Tree);
    surface
        .handle(&mut backend, key(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();
    surface
        .handle(&mut backend, key(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    surface
        .handle(&mut backend, key(KeyCode::Char(' '), KeyModifiers::NONE))
        .unwrap();
    let selected = surface.tree().state().selected;

    surface
        .handle(&mut backend, key(KeyCode::Char('t'), ctrl))
        .unwrap();
    assert_eq!(surface.mode(), SurfaceMode::Palette);
    assert_eq!(surface.palette().state().query, "deploy");
    assert_eq!(
        surface
            .palette()
            .state()
            .staged
            .state_of(&cid("skill/rust/review")),
        Some(true)
    );

    surface
        .handle(&mut backend, key(KeyCode::Char('t'), ctrl))
        .unwrap();
    assert_eq!(surface.tree().state().selected, selected);
    assert!(surface
        .tree()
        .state()
        .staged
        .contains(&cid("skill/rust/review")));
}

#[test]
fn activating_a_tree_leaf_preserves_the_palette_and_shared_staging() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(&mut backend, request("deploy")).unwrap();

    for event in [
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Right, KeyModifiers::NONE),
        key(KeyCode::Down, KeyModifiers::NONE),
        key(KeyCode::Char(' '), KeyModifiers::NONE),
        key(KeyCode::Enter, KeyModifiers::NONE),
    ] {
        assert_eq!(
            surface.handle(&mut backend, event).unwrap(),
            SurfaceStep::Continue
        );
    }

    assert_eq!(surface.mode(), SurfaceMode::Palette);
    assert_eq!(
        surface.palette().state().query,
        "deploy",
        "tree activation must not replace the resident palette controller"
    );
    assert_eq!(
        surface
            .palette()
            .state()
            .staged
            .state_of(&cid("skill/rust/review")),
        Some(true)
    );
    assert!(surface
        .tree()
        .state()
        .staged
        .contains(&cid("skill/rust/review")));
}

#[test]
fn tree_management_errors_stay_inside_the_tree_without_losing_state() {
    let mut backend = fixture();
    backend.1 = true;
    let mut surface = SurfaceController::new(&mut backend, request("")).unwrap();

    for event in [
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Right, KeyModifiers::NONE),
        key(KeyCode::Down, KeyModifiers::NONE),
        key(KeyCode::Char(' '), KeyModifiers::NONE),
        key(KeyCode::Char('a'), KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::NONE),
        key(KeyCode::Enter, KeyModifiers::NONE),
    ] {
        assert_eq!(
            surface.handle(&mut backend, event).unwrap(),
            SurfaceStep::Continue
        );
    }

    assert_eq!(surface.mode(), SurfaceMode::Tree);
    assert!(surface
        .tree()
        .state()
        .staged
        .contains(&cid("skill/rust/review")));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| surface.draw(frame)).unwrap();
    let frame = buffer_text(terminal.backend());
    assert!(
        frame.contains("set already exists") && frame.contains("skillset.already_exists"),
        "the domain message and stable code must be visible in place: {frame}"
    );
}

#[test]
fn one_terminal_loop_spans_palette_tree_palette_and_closes_from_palette() {
    let mut backend = fixture();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let mut events = ScriptedEvents::new([
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Right, KeyModifiers::NONE),
        key(KeyCode::Down, KeyModifiers::NONE),
        key(KeyCode::Char(' '), KeyModifiers::NONE),
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Esc, KeyModifiers::NONE),
    ]);

    let outcome = event_loop(&mut terminal, &mut events, &mut backend, request("")).unwrap();

    assert_eq!(outcome, aikit_tui::PaletteOutcome::Closed);
}

#[test]
fn applying_from_the_tree_uses_the_palette_gate_and_keeps_the_surface_open() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(&mut backend, request("")).unwrap();

    for event in [
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Right, KeyModifiers::NONE),
        key(KeyCode::Down, KeyModifiers::NONE),
        key(KeyCode::Char(' '), KeyModifiers::NONE),
    ] {
        assert_eq!(
            surface.handle(&mut backend, event).unwrap(),
            SurfaceStep::Continue
        );
    }

    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Enter, KeyModifiers::CONTROL),)
            .unwrap(),
        SurfaceStep::Continue,
        "a successful apply refreshes the surface rather than closing it"
    );
    assert_eq!(surface.mode(), SurfaceMode::Palette);
    assert_eq!(backend.applied.len(), 1);
    assert!(surface.palette().state().staged.is_empty());
    assert!(surface
        .palette()
        .state()
        .status
        .as_ref()
        .is_some_and(|status| status.message.contains("applied generation")));
}

#[test]
fn each_mode_names_the_other_mode_in_the_live_popup_chrome() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(&mut backend, request("")).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

    terminal.draw(|frame| surface.draw(frame)).unwrap();
    let palette = buffer_text(terminal.backend());
    assert!(
        palette.contains("palette"),
        "palette title missing: {palette}"
    );
    assert!(
        palette.contains("Ctrl-T tree"),
        "palette switch hint missing: {palette}"
    );

    surface
        .handle(&mut backend, key(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();
    terminal.draw(|frame| surface.draw(frame)).unwrap();
    let tree = buffer_text(terminal.backend());
    assert!(tree.contains("AIKit tree"), "tree title missing: {tree}");
    assert!(
        tree.contains("Ctrl-T palette"),
        "tree switch hint missing: {tree}"
    );
}

fn buffer_text(backend: &TestBackend) -> String {
    let area = backend.buffer().area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| backend.buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
