//! Compatibility Project-world composition for the shared CLI/TUI application service.
//!
//! The current CLI `Service` already exposes one resolved legacy view, its authoritative
//! `ContextDescriptor`, and the ResourceRef-native navigation index through `PaletteBackend`.
//! This adapter composes the V2 `ContextResolution` from those same inputs rather than
//! rebuilding resolution inside a renderer/controller.
//!
//! One limitation remains explicit: the legacy view records scope/profile provenance only
//! for actual selection operations. Empty scope layers are not recoverable through the
//! present public service boundary, so the resulting Project-world model carries a warning
//! until the CLI service publishes its complete ordered scope stack.

use std::collections::BTreeSet;

use aikit_core::context_resolution::{
    compose_context_resolution, RequestedActors, ScopeResolution,
};
use aikit_core::context_source::{ContextSourceEntry, ContextSourceIndex};
use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
use aikit_core::resource::{ResourceIndex, ResourceKind};
use aikit_core::{disclose_project_world, ProfileId, ProjectWorldReadModel, Result};

use crate::PaletteBackend;

pub fn project_world(backend: &dyn PaletteBackend) -> Result<ProjectWorldReadModel> {
    let context = backend.context();
    let project_ref = project_ref(context)?;
    let constituent = ProjectConstituentRef::parse("source:working-tree")?;
    let binding = ProjectBinding::from_legacy_context(
        project_ref,
        constituent,
        context.project_id.clone(),
        context.project_root.clone(),
    )?;

    let resources = backend.navigation_index();
    let (profiles, scopes) = observed_resolution_basis(backend);
    let resolution = compose_context_resolution(
        backend.view().clone(),
        binding,
        profiles,
        scopes,
        &resources,
        RequestedActors::default(),
    );

    let mut source_index = ContextSourceIndex::default();
    for record in ResourceIndex::resources(&resources) {
        if record.descriptor.kind == ResourceKind::ContextSource {
            if let Ok(entry) = ContextSourceEntry::new(record.clone()) {
                source_index.insert(entry);
            }
        }
    }

    let mut world = disclose_project_world(&resolution, &source_index, None);
    world.warnings.push(
        "compatibility Project-world basis includes only scope/profile provenance observed in resolved selection operations; empty scope layers are not exposed by the current application service"
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

fn observed_resolution_basis(
    backend: &dyn PaletteBackend,
) -> (Vec<ProfileId>, Vec<ScopeResolution>) {
    let mut profiles = BTreeSet::new();
    let mut seen_scopes = BTreeSet::new();
    let mut scopes = Vec::new();

    for operation in &backend.view().selection_log {
        if let Some(profile) = operation.via_profile.clone() {
            profiles.insert(profile);
        }
        let key = (
            operation.scope.rank(),
            operation.origin.to_string(),
        );
        if seen_scopes.insert(key.clone()) {
            scopes.push(ScopeResolution {
                kind: operation.scope,
                depth: 0,
                origin: key.1,
            });
        }
    }
    scopes.sort_by_key(|scope| (scope.kind.rank(), scope.depth, scope.origin.clone()));
    (profiles.into_iter().collect(), scopes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::capsule::{Capsule, Facets, Kind, Payload};
    use aikit_core::catalog::MemoryCatalog;
    use aikit_core::context::ContextDescriptor;
    use aikit_core::policy::ManagedPolicy;
    use aikit_core::resolve::{resolve, ResolveRequest};
    use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
    use aikit_core::ProfileId;
    use std::path::PathBuf;

    struct Backend {
        context: ContextDescriptor,
        view: aikit_core::ResolvedView,
    }

    impl PaletteBackend for Backend {
        fn context(&self) -> &ContextDescriptor { &self.context }
        fn view(&self) -> &aikit_core::ResolvedView { &self.view }
        fn documents(&self) -> Vec<aikit_core::SearchDoc> { Vec::new() }
        fn capsule(&self, _id: &aikit_core::CapsuleId) -> Option<&Capsule> { None }
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
    fn compatibility_service_reports_observed_scope_profile_basis_without_inventing_empty_layers() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let capsule = Capsule {
            id: "skill/rust/review".parse().unwrap(),
            name: "Review".into(),
            description: "review".into(),
            kind: Kind::Skill,
            payload: Payload::default(),
            facets: Facets::default(),
            requires: Vec::new(),
            provides: Vec::new(),
            conflicts: Vec::new(),
            replaces: Vec::new(),
            platform: Default::default(),
            maturity: Default::default(),
            effects: Default::default(),
            blocked: false,
            revision: Default::default(),
            registry_source: Default::default(),
        };
        let mut catalog = MemoryCatalog::default();
        catalog.insert(capsule);
        let profile = ProfileId::parse("reviewer").unwrap();
        let mut layer = ScopeLayer::new(
            ScopeKind::Project,
            LayerOrigin::new("/work/aikit/.aikit/project.toml"),
            Default::default(),
        );
        layer.patch.enable.insert("skill/rust/review".parse().unwrap());
        layer.patch.profile = Some(profile.clone());
        let view = resolve(
            &catalog,
            ResolveRequest {
                context: context.clone(),
                layers: vec![layer],
                policy: ManagedPolicy::default(),
            },
        );
        let backend = Backend { context, view };

        let world = project_world(&backend).unwrap();
        assert_eq!(world.resolution_basis.profiles, vec![profile]);
        assert_eq!(world.resolution_basis.scopes.len(), 1);
        assert_eq!(world.resolution_basis.scopes[0].kind, ScopeKind::Project);
        assert!(world
            .warnings
            .iter()
            .any(|warning| warning.contains("empty scope layers")));
    }
}
