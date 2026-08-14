//! The popup is one surface: Quick and Workspace are presentations, not terminal hand-offs.

mod common;

use common::*;

use aikit_core::resource::ResourceRef;
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

struct SurfaceFixture(Fixture);

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
        Ok(())
    }
}

fn fixture() -> SurfaceFixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.keep();
    SurfaceFixture(Fixture::new(
        &root,
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    ))
}

fn request(query: &str) -> SurfaceRequest {
    SurfaceRequest::new(UiHost::TmuxPopup).with_query(query)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

fn rref(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn switching_presentations_preserves_views_and_projects_canonical_staged_intent() {
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
    assert_eq!(
        surface
            .tui_state()
            .staged
            .get(&rref("skill/rust/review")),
        Some(&true),
        "TuiState is the semantic staged authority"
    );

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
fn activating_a_tree_leaf_preserves_the_palette_and_canonical_staging() {
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
    assert_eq!(
        surface
            .tui_state()
            .staged
            .get(&rref("skill/rust/review")),
        Some(&true)
    );
    assert!(surface
        .tree()
        .state()
        .staged
        .contains(&cid("skill/rust/review")));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| surface.draw(frame)).unwrap();
    let frame = buffer_text(terminal.backend());
    assert!(
        frame.contains("skill/rust/review"),
        "the preview must explain the activated tree capability, not the palette cursor: {frame}"
    );

    surface
        .handle(&mut backend, key(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    terminal.draw(|frame| surface.draw(frame)).unwrap();
    let frame = buffer_text(terminal.backend());
    assert!(
        frame.contains("Test script deploy") && !frame.contains("skill/rust/review"),
        "leaving the explicit preview must restore the resident palette cursor: {frame}"
    );
}

#[test]
fn one_terminal_loop_spans_quick_workspace_quick_and_uses_explicit_exit() {
    let mut backend = fixture();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let mut events = ScriptedEvents::new([
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Right, KeyModifiers::NONE),
        key(KeyCode::Down, KeyModifiers::NONE),
        key(KeyCode::Char(' '), KeyModifiers::NONE),
        key(KeyCode::Char('t'), KeyModifiers::CONTROL),
        key(KeyCode::Char('q'), KeyModifiers::CONTROL),
    ]);

    let outcome = event_loop(&mut terminal, &mut events, &mut backend, request("")).unwrap();

    assert_eq!(outcome, aikit_tui::PaletteOutcome::Closed);
}

#[test]
fn resting_back_never_clears_query_discards_staging_or_exits() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(&mut backend, request("")).unwrap();

    // Stage the current capability through the real Quick event path.
    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Char(' '), KeyModifiers::NONE))
            .unwrap(),
        SurfaceStep::Continue
    );
    assert_eq!(surface.tui_state().staged.len(), 1);

    // A typed query makes the old V1 Esc behaviour particularly dangerous: it
    // used to clear query on one press, discard staging on the next and exit on
    // the third. V2 Back does none of those things.
    surface
        .handle(&mut backend, key(KeyCode::Char('x'), KeyModifiers::NONE))
        .unwrap();
    let query = surface.tui_state().query.clone();
    let staged = surface.tui_state().staged.clone();

    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap(),
        SurfaceStep::Continue
    );
    assert_eq!(surface.tui_state().query, query);
    assert_eq!(surface.tui_state().staged, staged);
    assert!(surface.palette().state().outcome.is_none());

    assert_eq!(
        surface
            .handle(
                &mut backend,
                key(KeyCode::Char('q'), KeyModifiers::CONTROL),
            )
            .unwrap(),
        SurfaceStep::Outcome(aikit_tui::PaletteOutcome::Closed)
    );
}

#[test]
fn applying_from_workspace_uses_palette_gate_and_clears_canonical_staging() {
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

    assert_eq!(surface.tui_state().staged.len(), 1);
    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Enter, KeyModifiers::CONTROL),)
            .unwrap(),
        SurfaceStep::Continue,
        "a successful apply refreshes the surface rather than closing it"
    );
    assert_eq!(surface.mode(), SurfaceMode::Palette);
    assert_eq!(backend.applied.len(), 1);
    assert!(surface.tui_state().staged.is_empty());
    assert!(surface.palette().state().staged.is_empty());
    assert!(
        surface.tree().state().staged.is_empty(),
        "a successful apply must clear canonical staged state and both projections"
    );
    assert!(surface
        .palette()
        .state()
        .status
        .as_ref()
        .is_some_and(|status| status.message.contains("applied generation")));
}

#[test]
fn each_presentation_names_the_other_in_the_live_popup_chrome() {
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
