mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_core::Result;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::surface::{SurfaceBackend, SurfaceController, SurfaceRequest};
use aikit_tui::tree::{Node, NodeKind, Root, TreeEffect, TreeState};
use aikit_tui::{PaletteBackend, PresentationMode, WorkspaceSection};
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

impl PaletteBackend for SurfaceFixture {
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
            "capabilities",
            vec![Node::leaf(
                NodeKind::Capability {
                    id: cid("script/ops/deploy"),
                },
                "deploy",
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
    SurfaceFixture(Fixture::new(&root, vec![script("script/ops/deploy")]))
}

fn rref(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn alt(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

#[test]
fn colon_search_invokes_stageable_action_without_applying() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert_eq!(surface.semantic().selected, Some(rref("script/ops/deploy")));
    assert!(surface
        .semantic()
        .contextual_actions
        .iter()
        .any(|action| action.action == rref("action/capability/toggle")));

    surface.handle(&mut backend, key(KeyCode::Char(':'))).unwrap();
    for character in "toggle".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();

    assert_eq!(surface.semantic().action_query, None);
    assert_eq!(surface.semantic().staged.len(), 1);
    assert!(!backend.view().is_active(&cid("script/ops/deploy")));
}

#[test]
fn space_uses_the_stageable_contextual_action_and_second_space_unstages() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    surface.handle(&mut backend, key(KeyCode::Char(' '))).unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    assert!(!backend.view().is_active(&cid("script/ops/deploy")));

    surface.handle(&mut backend, key(KeyCode::Char(' '))).unwrap();
    assert!(surface.semantic().staged.is_empty());
}

#[test]
fn workspace_sections_are_real_state_and_survive_quick_switching() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Projects);

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);

    surface.handle(&mut backend, ctrl(KeyCode::Char('w'))).unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Quick);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);

    surface.handle(&mut backend, ctrl(KeyCode::Char('w'))).unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);
}