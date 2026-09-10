mod common;

use std::path::PathBuf;

use aikit_core::capsule::Capsule;
use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::resource::ResourceRef;
use aikit_core::scope::ScopeKind;
use aikit_core::search::SearchDoc;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{SessionSpaceFocus, SessionSpaceMutation};
use aikit_core::{ContextDescriptor, Result};
use aikit_store::AikitHome;
use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::{ApplicationService, SessionSpaceApplicationProjection};
use tempfile::TempDir;

use common::Fixture;

struct HomeBackend {
    inner: Fixture,
    home: AikitHome,
}

impl PaletteBackend for HomeBackend {
    fn context(&self) -> &ContextDescriptor {
        <Fixture as PaletteBackend>::context(&self.inner)
    }

    fn view(&self) -> &aikit_core::ResolvedView {
        <Fixture as PaletteBackend>::view(&self.inner)
    }

    fn application_home(&self) -> Option<&AikitHome> {
        Some(&self.home)
    }

    fn documents(&self) -> Vec<SearchDoc> {
        <Fixture as PaletteBackend>::documents(&self.inner)
    }

    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule> {
        <Fixture as PaletteBackend>::capsule(&self.inner, id)
    }

    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected> {
        <Fixture as PaletteBackend>::preview(&self.inner, scope, toggles)
    }

    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId> {
        <Fixture as PaletteBackend>::apply(&mut self.inner, scope, toggles)
    }

    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput> {
        <Fixture as PaletteBackend>::start(&mut self.inner, intent)
    }

    fn recent(&self) -> Vec<RunIntent> {
        <Fixture as PaletteBackend>::recent(&self.inner)
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        <Fixture as PaletteBackend>::promotion_drafts(&self.inner)
    }

    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId> {
        <Fixture as PaletteBackend>::promote(&mut self.inner, draft)
    }

    fn open_source(&mut self, id: &CapsuleId) -> Result<PathBuf> {
        <Fixture as PaletteBackend>::open_source(&mut self.inner, id)
    }
}

#[test]
fn canonical_application_service_projects_session_space_preview_apply_history_and_explain() {
    let temp = TempDir::new().unwrap();
    let home = AikitHome::at(temp.path().join("aikit-home"));
    home.ensure_layout().unwrap();
    let fixture = Fixture::new(temp.path(), vec![]);
    let mut backend = HomeBackend {
        inner: fixture,
        home,
    };
    let mut service = ApplicationService::new(&mut backend);
    let space = SessionSpaceRef::parse("session-space/tui-service").unwrap();

    let create = service
        .session_space_stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("TUI service".into()),
            },
        )
        .unwrap();
    let created = service.session_space_apply(&create).unwrap();
    assert_eq!(service.session_space_show(&space).unwrap(), created.after);

    let focus = service
        .session_space_stage(
            Some(&space),
            SessionSpaceMutation::Focus {
                focus: Some(SessionSpaceFocus {
                    target: ResourceRef::parse("surface/editor").unwrap(),
                    region: Some("primary".into()),
                    provenance: vec!["canonical TUI ApplicationService".into()],
                }),
            },
        )
        .unwrap();
    let focused = service.session_space_apply(&focus).unwrap();
    assert_eq!(focused.after.revision, 1);
    assert_eq!(service.session_space_history(&space).unwrap().len(), 2);

    let explained = service.session_space_explain(&space, None).unwrap();
    assert_eq!(explained.latest_receipt.as_ref().unwrap().sequence, 1);
    assert_eq!(explained.explanation.semantic_revision, 1);
}
