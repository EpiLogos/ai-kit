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
use aikit_tui::tree::{Node, NodeKind, Root, TreeEffect, TreeState};
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

impl aikit_tui::PaletteBackend for SurfaceFixture {
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
        // Deliberately omit deploy from the tree. The canonical V2 read model must
        // still include it because Quick can address it through the same Resource field.
        Ok(TreeState::new(vec![Node::branch(
            NodeKind::Root(Root::Kinds),
            "one tree-visible capability",
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
fn palette_only_resource_is_part_of_the_canonical_read_model_and_selection() {
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
    assert_eq!(
        surface.semantic().selected,
        Some(rref("script/ops/deploy")),
        "Quick-only resources must not be rejected just because Tree cannot render them"
    );
}

#[test]
fn live_surface_initialises_mutation_scope_from_the_real_scope_selector() {
    let mut backend = fixture();
    let surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();

    assert_eq!(
        surface.semantic().mutation_scope,
        Some(surface.palette().state().scope.current())
    );
}

#[test]
fn live_quick_keyboard_and_mouse_selection_converge_on_the_same_resource_ref() {
    let mut keyboard_backend = fixture();
    let mut keyboard = SurfaceController::new(
        &mut keyboard_backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    let mut keyboard_terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    keyboard.draw_terminal(&mut keyboard_terminal).unwrap();
    keyboard
        .handle(&mut keyboard_backend, key(KeyCode::Down))
        .unwrap();
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
    assert_eq!(
        mouse_surface
            .palette()
            .state()
            .selected_row()
            .map(|row| row.doc.id.clone()),
        keyboard
            .palette()
            .state()
            .selected_row()
            .map(|row| row.doc.id.clone()),
        "legacy cursors are projections of the same semantic ResourceRef selection"
    );
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
    assert!(surface.palette().state().rows.is_empty());

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();

    let inner = Rect::new(1, 1, 98, 22);
    let list = Layout::for_width(inner.width).split(inner).list;
    surface
        .handle(&mut backend, mouse(list.x, list.y))
        .unwrap();
    assert_eq!(surface.semantic().selected, Some(rref("project/aikit")));
}

#[test]
fn resting_query_is_resolved_by_v2_runtime_and_projected_back_to_palette() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();

    type_text(&mut surface, &mut backend, "dep");

    assert_eq!(surface.semantic().query, "dep");
    assert_eq!(surface.palette().state().query, "dep");
    assert_eq!(
        surface.semantic().selected,
        Some(rref("script/ops/deploy"))
    );
    assert_eq!(
        surface
            .palette()
            .state()
            .selected_row()
            .map(|row| row.doc.id.to_string()),
        Some("script/ops/deploy".into())
    );
}

#[test]
fn action_text_mode_is_separate_from_resource_search_and_invokes_immediate_actions() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    assert_eq!(surface.semantic().query, "deploy");
    assert!(!surface.semantic().contextual_actions.is_empty());

    surface.handle(&mut backend, key(KeyCode::Char(':'))).unwrap();
    assert_eq!(surface.semantic().action_query.as_deref(), Some(""));
    type_text(&mut surface, &mut backend, "explain");
    assert_eq!(surface.semantic().query, "deploy");
    assert_eq!(surface.semantic().action_query.as_deref(), Some("explain"));
    assert!(surface.semantic().staged.is_empty());

    surface.handle(&mut backend, key(KeyCode::Char(' '))).unwrap();
    assert!(surface.semantic().staged.is_empty());
    assert!(surface
        .semantic()
        .status
        .as_ref()
        .unwrap()
        .message
        .contains("not stageable"));
    assert_eq!(surface.semantic().action_query.as_deref(), Some("explain"));

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
fn action_space_invokes_only_a_stageable_selected_action() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    surface.handle(&mut backend, key(KeyCode::Char(':'))).unwrap();
    type_text(&mut surface, &mut backend, "toggle");
    assert_eq!(surface.semantic().action_query.as_deref(), Some("toggle"));
    assert!(surface.semantic().staged.is_empty());

    surface.handle(&mut backend, key(KeyCode::Char(' '))).unwrap();
    assert!(surface.semantic().action_query.is_none());
    assert_eq!(surface.semantic().staged.len(), 1);
    assert_eq!(
        surface
            .semantic()
            .staged
            .get(&rref("script/ops/deploy")),
        Some(aikit_tui::ActivationIntent::Enable)
    );
    assert!(matches!(
        surface.semantic().action_result,
        Some(ActionOutcome::Staged { .. })
    ));
}

#[test]
fn live_staging_and_apply_follow_one_v2_preview_confirm_effect_path() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();

    // Outside text-Action mode, a single stageable Action may be used as the
    // explicit mutation operation for this Resource. The mutation still executes
    // through the same canonical Action application service rather than toggling
    // semantic state directly.
    surface.handle(&mut backend, key(KeyCode::Char(' '))).unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    assert_eq!(surface.palette().state().staged.toggles().len(), 1);

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('s')))
        .unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::CompositionPreview));
    assert!(surface.semantic().preview.is_some());

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('s')))
        .unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::ConfirmApply));

    surface
        .handle(&mut backend, ctrl(KeyCode::Char('s')))
        .unwrap();
    assert!(surface.semantic().staged.is_empty());
    assert!(surface.semantic().preview.is_none());
    assert!(backend.view().is_active(&cid("script/ops/deploy")));
}

#[test]
fn quick_workspace_switch_changes_presentation_without_rebuilding_semantic_state() {
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
        .handle(&mut backend, ctrl(KeyCode::Char('w')))
        .unwrap();

    assert_eq!(surface.semantic().presentation, PresentationMode::Quick);
    assert_eq!(surface.semantic().selected, selected);
    assert_eq!(surface.semantic().read_model.revision, revision);
}

#[test]
fn workspace_section_navigation_preserves_query_selection_and_staging() {
    let mut backend = fixture();
    let mut surface = SurfaceController::new(
        &mut backend,
        SurfaceRequest::new(UiHost::TmuxPopup).with_query("deploy"),
    )
    .unwrap();
    assert_eq!(surface.semantic().presentation, PresentationMode::Workspace);
    let selected = surface.semantic().selected.clone();

    surface.handle(&mut backend, key(KeyCode::Char(' '))).unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();

    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Compose);
    assert_eq!(surface.semantic().query, "deploy");
    assert_eq!(surface.semantic().selected, selected);
    assert_eq!(surface.semantic().staged.len(), 1);

    surface.handle(&mut backend, alt(KeyCode::Left)).unwrap();
    assert_eq!(surface.semantic().workspace_section, WorkspaceSection::Projects);
}
