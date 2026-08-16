mod common;

use common::*;

use aikit_core::resource::{
    NavigationEvidence, NavigationEvidenceClass, ResourceDescriptor, ResourceKind, ResourceRecord,
    ResourceRef, ResourceSearchIndex,
};
use aikit_core::Result;
use aikit_tui::application::{ActionOutcome, Overlay, PresentationMode, WorkspaceSection};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use aikit_tui::surface::{SurfaceBackend, SurfaceController, SurfaceRequest};
use aikit_tui::tree::{TreeEffect, TreeState};
use aikit_tui::PaletteBackend;
use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

struct SurfaceFixture {
    fixture: Fixture,
    include_project: bool,
}

impl std::ops::Deref for SurfaceFixture {
    type Target = Fixture;

    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}

impl std::ops::DerefMut for SurfaceFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fixture
    }
}

impl PaletteBackend for SurfaceFixture {
    fn context(&self) -> &aikit_core::ContextDescriptor {
        self.fixture.context()
    }

    fn view(&self) -> &aikit_core::resolve::ResolvedView {
        self.fixture.view()
    }

    fn documents(&self) -> Vec<aikit_core::search::SearchDoc> {
        self.fixture.documents()
    }

    fn navigation_index(&self) -> ResourceSearchIndex {
        let mut index = aikit_tui::PaletteBackend::navigation_index(&self.fixture);
        if self.include_project {
            index.insert_resource(
                ResourceRecord::new(ResourceDescriptor::new(
                    rref("project/aikit"),
                    ResourceKind::Project,
                    "AIKit V2",
                    "current project destination",
                )),
                vec![NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)],
            );
        }
        index
    }

    fn capsule(&self, id: &aikit_core::CapsuleId) -> Option<&aikit_core::capsule::Capsule> {
        self.fixture.capsule(id)
    }

    fn recent(&self) -> Vec<aikit_tui::RunIntent> {
        self.fixture.recent()
    }

    fn preview(
        &self,
        scope: aikit_core::scope::ScopeKind,
        toggles: &[aikit_tui::Toggle],
    ) -> Result<aikit_tui::Projected> {
        self.fixture.preview(scope, toggles)
    }

    fn apply(
        &mut self,
        scope: aikit_core::scope::ScopeKind,
        toggles: &[aikit_tui::Toggle],
    ) -> Result<aikit_core::GenerationId> {
        self.fixture.apply(scope, toggles)
    }

    fn start(&mut self, intent: &aikit_tui::RunIntent) -> Result<aikit_tui::JobOutput> {
        self.fixture.start(intent)
    }

    fn open_source(&mut self, id: &aikit_core::CapsuleId) -> Result<std::path::PathBuf> {
        self.fixture.open_source(id)
    }

    fn promotion_drafts(&self) -> Vec<aikit_tui::PromotionDraft> {
        self.fixture.promotion_drafts()
    }

    fn promote(&mut self, draft: &aikit_tui::PromotionDraft) -> Result<aikit_core::CapsuleId> {
        self.fixture.promote(draft)
    }
}

impl SurfaceBackend for SurfaceFixture {
    fn surface_tree(&self) -> Result<TreeState> {
        // Compatibility presentation is deliberately empty: no V2 semantic test
        // may depend on TreeState membership or cursor identity.
        Ok(TreeState::new(Vec::new()))
    }

    fn apply_tree_effect(&mut self, _effect: TreeEffect) -> Result<()> {
        panic!("V2 live acceptance must never route mutation through TreeEffect")
    }
}

fn fixture_with_project(include_project: bool) -> SurfaceFixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.keep();
    SurfaceFixture {
        fixture: Fixture::new(
            &root,
            vec![script("script/ops/deploy"), skill("skill/rust/review")],
        ),
        include_project,
    }
}

fn fixture() -> SurfaceFixture {
    fixture_with_project(false)
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

fn mouse(column: u16, row: u16) -> PaletteEvent {
    PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn rref(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn type_text(surface: &mut SurfaceController, backend: &mut SurfaceFixture, text: &str) {
    for character in text.chars() {
        surface
            .handle(backend, key(KeyCode::Char(character)))
            .unwrap();
    }
}

#[test]
fn universal_resource_search_and_selection_do_not_depend_on_tree_membership() {
    let mut backend = fixture();
    let surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    assert!(surface
        .semantic()
        .read_model
        .contains(&rref("script/ops/deploy")));
    assert_eq!(surface.semantic().selected, Some(rref("script/ops/deploy")));
    assert!(surface
        .semantic()
        .contextual_actions
        .iter()
        .any(|action| action.action == rref("action/capability/toggle")));
}

#[test]
fn keyboard_and_mouse_selection_converge_on_the_same_resource_ref() {
    let mut keyboard_backend = fixture();
    let mut keyboard = SurfaceController::new(
        &mut keyboard_backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let mut keyboard_terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    keyboard.draw_terminal(&mut keyboard_terminal).unwrap();
    keyboard.handle(&mut keyboard_backend, key(KeyCode::Down)).unwrap();
    let keyboard_selection = keyboard.semantic().selected.clone();
    assert!(keyboard_selection.is_some());

    let mut mouse_backend = fixture();
    let mut mouse_surface = SurfaceController::new(
        &mut mouse_backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let mut mouse_terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    mouse_surface.draw_terminal(&mut mouse_terminal).unwrap();
    let inner = Rect::new(1, 1, 98, 28);
    let list = Layout::for_width(inner.width).split(inner).list;
    mouse_surface
        .handle(&mut mouse_backend, mouse(list.x, list.y + 1))
        .unwrap();

    assert_eq!(mouse_surface.semantic().selected, keyboard_selection);
}

#[test]
fn generic_project_resource_is_searchable_drawable_and_mouse_addressable() {
    let mut backend = fixture_with_project(true);
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("AIKit V2"),
    )
    .unwrap();
    assert_eq!(surface.semantic().selected, Some(rref("project/aikit")));
    assert_eq!(
        surface.semantic().read_model.resources[0].kind,
        ResourceKind::Project
    );

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    let inner = Rect::new(1, 1, 98, 22);
    let list = Layout::for_width(inner.width).split(inner).list;
    surface.handle(&mut backend, mouse(list.x, list.y)).unwrap();
    assert_eq!(surface.semantic().selected, Some(rref("project/aikit")));
}

#[test]
fn action_text_mode_is_separate_from_resource_search_and_invokes_immediate_actions() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    surface.handle(&mut backend, key(KeyCode::Char(':'))).unwrap();
    type_text(&mut surface, &mut backend, "explain");
    assert_eq!(surface.semantic().query, "deploy");
    assert_eq!(surface.semantic().action_query.as_deref(), Some("explain"));
    assert!(surface.semantic().staged.is_empty());

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert!(surface.semantic().action_query.is_none());
    assert_eq!(surface.semantic().overlay, Some(Overlay::Explain));
    assert!(matches!(
        surface.semantic().action_result,
        Some(ActionOutcome::Explained { .. })
    ));
    assert!(surface.semantic().staged.is_empty());
}

#[test]
fn explicit_staging_preview_confirmation_and_apply_are_one_reducer_path() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    // Plain Space remains query text while fuzzy search is active; Ctrl+Space is
    // the explicit mutation operation.
    surface
        .handle(&mut backend, ctrl(KeyCode::Char(' ')))
        .unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    assert_eq!(surface.semantic().query, "deploy");
    assert!(!backend.view().is_active(&cid("script/ops/deploy")));

    surface.handle(&mut backend, ctrl(KeyCode::Char('s'))).unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::CompositionPreview));
    surface.handle(&mut backend, ctrl(KeyCode::Char('s'))).unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::ConfirmApply));
    surface.handle(&mut backend, ctrl(KeyCode::Char('s'))).unwrap();

    assert!(surface.semantic().staged.is_empty());
    assert!(surface.semantic().preview.is_none());
    assert!(backend.view().is_active(&cid("script/ops/deploy")));
    assert_eq!(backend.applied.len(), 1);
}

#[test]
fn quick_workspace_and_workspace_sections_preserve_semantic_state() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    let selected = surface.semantic().selected.clone();
    let revision = surface.semantic().read_model.revision.clone();
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);

    surface
        .handle(&mut backend, ctrl(KeyCode::Char(' ')))
        .unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);

    surface.handle(&mut backend, ctrl(KeyCode::Char('w'))).unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Quick);
    assert_eq!(surface.semantic().selected, selected);
    assert_eq!(surface.semantic().read_model.revision, revision);
    assert_eq!(surface.semantic().staged.len(), 1);

    surface.handle(&mut backend, ctrl(KeyCode::Char('w'))).unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Explore);
}
