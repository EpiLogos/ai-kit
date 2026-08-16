mod common;

use common::*;

use aikit_core::Result;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::surface::{SurfaceBackend, SurfaceController, SurfaceRequest, SurfaceStep};
use aikit_tui::tree::{Node, NodeKind, Root, TreeEffect, TreeState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

fn key(code: KeyCode, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, modifiers))
}

#[test]
fn escape_at_rest_is_back_only_and_ctrl_u_is_explicit_query_clear() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert_eq!(
        surface
            .handle(&mut backend, key(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap(),
        SurfaceStep::Continue
    );
    assert_eq!(surface.palette().state().query, "deploy");
    assert_eq!(surface.semantic().query, "deploy");
    assert!(!surface.semantic().exit_requested);

    assert_eq!(
        surface
            .handle(
                &mut backend,
                key(KeyCode::Char('u'), KeyModifiers::CONTROL),
            )
            .unwrap(),
        SurfaceStep::Continue
    );
    assert!(surface.palette().state().query.is_empty());
    assert!(surface.semantic().query.is_empty());
}

#[test]
fn explicit_exit_is_refused_while_staged_intent_remains() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    // Non-empty fuzzy search retains ordinary text semantics for Space. Explicit
    // staging uses Ctrl+Space/Insert and therefore cannot be confused with query
    // editing.
    surface
        .handle(
            &mut backend,
            key(KeyCode::Char(' '), KeyModifiers::CONTROL),
        )
        .unwrap();
    assert!(!surface.semantic().staged.is_empty());

    assert_eq!(
        surface
            .handle(
                &mut backend,
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            )
            .unwrap(),
        SurfaceStep::Continue
    );
    assert!(!surface.semantic().exit_requested);
    assert!(!surface.semantic().staged.is_empty());
    assert!(surface
        .palette()
        .state()
        .status
        .as_ref()
        .unwrap()
        .message
        .contains("apply or discard"));
}

#[test]
fn ctrl_q_is_an_explicit_exit_when_nothing_is_staged() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert_eq!(
        surface
            .handle(
                &mut backend,
                key(KeyCode::Char('q'), KeyModifiers::CONTROL),
            )
            .unwrap(),
        SurfaceStep::Outcome(aikit_tui::PaletteOutcome::Closed)
    );
    assert!(surface.semantic().exit_requested);
}
