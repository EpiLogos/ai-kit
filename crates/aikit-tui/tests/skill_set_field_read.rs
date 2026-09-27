//! The SkillSet field read sits below the CLI/TUI split: the same store
//! `aikit set list` reads, projected by the same resolved view. This test
//! drives the shared application service over a real AIKit home with real
//! sets on disk, so the compose Praxis step's rows are proven to come from
//! the actual field — not from a fixture catalogue.

#![cfg(unix)]

use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::resolve::{resolve, ResolvedView, ResolveRequest};
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::trust::MemoryTrust;
use aikit_core::{AikitError, Capsule, ManagedPolicy, Result};
use aikit_store::{skillsets, AikitHome};
use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::{ApplicationService, SkillSetFieldRow, TuiApplicationService};

struct HomeBackend {
    context: ContextDescriptor,
    view: ResolvedView,
    home: AikitHome,
}

impl PaletteBackend for HomeBackend {
    fn context(&self) -> &ContextDescriptor {
        &self.context
    }

    fn view(&self) -> &ResolvedView {
        // The empty-catalogue projection: nothing is active, so every member
        // reads through the resolver's own opinion — the honest projection
        // for this reading, not a fake of activation.
        &self.view
    }

    fn documents(&self) -> Vec<SearchDoc> {
        Vec::new()
    }

    fn capsule(&self, _id: &CapsuleId) -> Option<&Capsule> {
        None
    }

    fn preview(&self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<Projected> {
        Err(AikitError::new("test.preview", "unused"))
    }

    fn apply(&mut self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<GenerationId> {
        Err(AikitError::new("test.apply", "unused"))
    }

    fn start(&mut self, _intent: &RunIntent) -> Result<JobOutput> {
        Err(AikitError::new("test.start", "unused"))
    }

    fn recent(&self) -> Vec<RunIntent> {
        Vec::new()
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        Vec::new()
    }

    fn promote(&mut self, _draft: &PromotionDraft) -> Result<CapsuleId> {
        Err(AikitError::new("test.promote", "unused"))
    }

    fn application_home(&self) -> Option<&AikitHome> {
        Some(&self.home)
    }
}

#[test]
fn the_skill_set_field_reads_the_real_store() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    skillsets::create(
        &home,
        "central-engineering",
        &[CapsuleId::parse("skill/rust/code-review").unwrap()],
        &[],
    )
    .unwrap();
    skillsets::create(&home, "research-deep", &[], &[]).unwrap();

    let context = ContextDescriptor::for_project("/work/aikit");
    let view = resolve(
        &aikit_core::catalog::MemoryCatalog::default(),
        &MemoryTrust::default(),
        &ResolveRequest {
            context: context.clone(),
            layers: Vec::<ScopeLayer>::new(),
            policy: ManagedPolicy::default(),
        },
    )
    .unwrap();
    let mut backend = HomeBackend {
        context,
        view,
        home: home.clone(),
    };
    let service = ApplicationService::new(&mut backend);
    let rows = service.skill_set_field().unwrap();

    let names: Vec<&str> = rows.iter().map(|row: &SkillSetFieldRow| row.name.as_str()).collect();
    assert!(names.contains(&"central-engineering"), "{names:?}");
    assert!(names.contains(&"research-deep"), "{names:?}");
    let engineering = rows.iter().find(|row| row.name == "central-engineering").unwrap();
    assert_eq!(engineering.members, 1, "the real set's own member count");
    assert!(
        !engineering.provenance.is_empty(),
        "the set's provenance rides the row"
    );
}
