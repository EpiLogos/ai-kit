//! Compatibility Project-world composition for the shared CLI/TUI application service.
//!
//! The current CLI `Service` already exposes one resolved legacy view, its authoritative
//! `ContextDescriptor`, and the ResourceRef-native navigation index through `PaletteBackend`.
//! This adapter composes the V2 `ContextResolution` from those same inputs rather than
//! rebuilding resolution inside a renderer/controller.
//!
//! One limitation remains explicit: the current public palette service does not expose
//! the complete ordered `ScopeLayer` stack. `ContextResolution` can still recover profile
//! provenance already present in the resolved selection log, but scope layers are
//! intentionally left empty here rather than reconstructed from partial evidence.

use aikit_core::context_resolution::{compose_context_resolution, RequestedActors};
use aikit_core::context_source::{ContextSourceEntry, ContextSourceIndex};
use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
use aikit_core::resource::{ResourceIndex, ResourceKind};
use aikit_core::{disclose_project_world, ProjectWorldReadModel, Result};

use crate::PaletteBackend;

pub fn project_world(backend: &dyn PaletteBackend) -> Result<ProjectWorldReadModel> {
    let context = backend.context();
    let project_ref = project_ref(context)?;
    let constituent = ProjectConstituentRef::parse("source:working-tree")?;
    let binding = ProjectBinding::from_legacy_context(project_ref, constituent, context)?;

    let resources = backend.navigation_index();
    let resolution = compose_context_resolution(
        backend.view(),
        binding,
        &[],
        &resources,
        RequestedActors::default(),
    );

    let mut source_index = ContextSourceIndex::default();
    for record in ResourceIndex::resources(&resources) {
        if record.descriptor.kind == ResourceKind::ContextSource {
            if let Ok(mut entry) = ContextSourceEntry::new(record.clone()) {
                entry.disclosure.known_to_exist = true;
                entry.disclosure.askable = true;
                entry.disclosure.exists = matches!(
                    aikit_core::resource_availability(record),
                    aikit_core::Availability::Available
                );
                source_index.insert(entry);
            }
        }
    }

    let mut world = disclose_project_world(&resolution, &source_index, None);
    world.warnings.push(
        "compatibility Project-world basis does not include the ordered scope-layer stack because the current palette application-service boundary does not expose it; profile provenance already present in the resolved view remains disclosed"
            .into(),
    );
    Ok(world)
}

fn project_ref(context: &aikit_core::ContextDescriptor) -> Result<ProjectRef> {
    if let Some(id) = &context.project_id {
        return ProjectRef::parse(&format!("project:{}", id));
    }
    if let Some(name) = context
        .project_root
        .as_ref()
        .and_then(|root| root.file_name())
        .map(|name| name.to_string_lossy())
    {
        return ProjectRef::parse(&format!("project:{name}"));
    }
    ProjectRef::parse("project:unbound-context")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::catalog::MemoryCatalog;
    use aikit_core::context::ContextDescriptor;
    use aikit_core::policy::ManagedPolicy;
    use aikit_core::resolve::{resolve, ResolveRequest};
    use aikit_core::scope::ScopeKind;
    use aikit_core::trust::MemoryTrust;
    use std::path::PathBuf;

    struct Backend {
        context: ContextDescriptor,
        view: aikit_core::ResolvedView,
    }

    impl PaletteBackend for Backend {
        fn context(&self) -> &ContextDescriptor { &self.context }
        fn view(&self) -> &aikit_core::ResolvedView { &self.view }
        fn documents(&self) -> Vec<aikit_core::SearchDoc> { Vec::new() }
        fn capsule(&self, _id: &aikit_core::CapsuleId) -> Option<&aikit_core::Capsule> { None }
        fn recent(&self) -> Vec<crate::RunIntent> { Vec::new() }
        fn preview(
            &self,
            _scope: ScopeKind,
            _toggles: &[crate::Toggle],
        ) -> Result<crate::Projected> {
            Err(aikit_core::AikitError::new("test.preview", "unused"))
        }
        fn apply(
            &mut self,
            _scope: ScopeKind,
            _toggles: &[crate::Toggle],
        ) -> Result<aikit_core::GenerationId> {
            Err(aikit_core::AikitError::new("test.apply", "unused"))
        }
        fn start(&mut self, _intent: &crate::RunIntent) -> Result<crate::JobOutput> {
            Err(aikit_core::AikitError::new("test.start", "unused"))
        }
        fn open_source(&mut self, _id: &aikit_core::CapsuleId) -> Result<PathBuf> {
            Err(aikit_core::AikitError::new("test.open", "unused"))
        }
        fn promotion_drafts(&self) -> Vec<crate::PromotionDraft> { Vec::new() }
        fn promote(&mut self, _draft: &crate::PromotionDraft) -> Result<aikit_core::CapsuleId> {
            Err(aikit_core::AikitError::new("test.promote", "unused"))
        }
    }

    #[test]
    fn compatibility_service_does_not_invent_unexposed_scope_layers() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let catalog = MemoryCatalog::default();
        let trust = MemoryTrust::default();
        let view = resolve(
            &catalog,
            &trust,
            &ResolveRequest {
                context: context.clone(),
                layers: Vec::new(),
                policy: ManagedPolicy::default(),
            },
        )
        .unwrap();
        let backend = Backend { context, view };

        let world = project_world(&backend).unwrap();
        assert!(world.resolution_basis.scopes.is_empty());
        assert!(world
            .warnings
            .iter()
            .any(|warning| warning.contains("scope-layer stack")));
    }
}
