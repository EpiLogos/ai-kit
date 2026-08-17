//! Project-world and actor-context composition for the shared CLI/TUI application service.
//!
//! The application service already exposes one resolved legacy view, its authoritative
//! `ContextDescriptor`, the ResourceRef-native navigation index, and—where the concrete
//! backend owns it—the ordered scope-layer stack through `PaletteBackend`.
//!
//! This module composes one V2 [`ContextResolution`] from those same inputs rather than
//! rebuilding resolution inside a renderer/controller or target adapter. Project-world
//! disclosure and thin actor bootstrap can therefore consume the same typed operational
//! truth. When a compatibility backend cannot expose scope layers, that absence remains
//! explicit instead of being inferred from partial selection-log evidence.

use aikit_core::context_resolution::{compose_context_resolution, RequestedActors};
use aikit_core::context_source::{ContextSourceEntry, ContextSourceIndex};
use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::{disclose_project_world, ContextResolution, ProjectWorldReadModel, Result};

use crate::PaletteBackend;

/// Compose the wider V2 operational Context from the same application service
/// that already backs Quick/Workspace navigation and durable composition.
///
/// This is intentionally public inside the TUI/application crate so other
/// application projections—most importantly the managed actor bootstrap—can
/// consume the exact same resolved Project/Scope/Resource field without asking a
/// harness adapter to reconstruct identity.
pub fn context_resolution(backend: &dyn PaletteBackend) -> Result<ContextResolution> {
    let context = backend.context();
    let project_ref = project_ref(context)?;
    let constituent = ProjectConstituentRef::parse("source:working-tree")?;
    let binding = ProjectBinding::from_legacy_context(project_ref, constituent, context)?;
    let resources = backend.navigation_index();

    let host = (!context.host.trim().is_empty())
        .then(|| ResourceRef::parse(&format!("host/{}", context.host)))
        .transpose()?;
    let requested = RequestedActors {
        host,
        ..RequestedActors::default()
    };

    Ok(compose_context_resolution(
        backend.view(),
        binding,
        backend.scope_layers().unwrap_or(&[]),
        &resources,
        requested,
    ))
}

pub fn project_world(backend: &dyn PaletteBackend) -> Result<ProjectWorldReadModel> {
    let resolution = context_resolution(backend)?;

    let mut source_index = ContextSourceIndex::default();
    for resolved in &resolution.context_sources {
        let record = &resolved.resource;
        if record.descriptor.kind == ResourceKind::ContextSource {
            if let Ok(mut entry) = ContextSourceEntry::new(record.clone()) {
                entry.disclosure.known_to_exist = true;
                entry.disclosure.askable = true;
                entry.disclosure.exists =
                    matches!(resolved.availability, aikit_core::Availability::Available);
                source_index.insert(entry);
            }
        }
    }

    let mut world = disclose_project_world(&resolution, &source_index, None);
    if backend.scope_layers().is_none() {
        world.warnings.push(
            "Project-world basis does not include the ordered scope-layer stack because this application-service boundary does not expose it; scope provenance is not reconstructed from partial evidence"
                .into(),
        );
    }
    Ok(world)
}

fn project_ref(context: &aikit_core::ContextDescriptor) -> Result<ProjectRef> {
    if let Some(id) = &context.project_id {
        return ProjectRef::parse(&format!("project:{id}"));
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
    use aikit_core::context_resolution::ReferenceResolution;
    use aikit_core::policy::ManagedPolicy;
    use aikit_core::resolve::{resolve, ResolveRequest};
    use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
    use aikit_core::trust::MemoryTrust;
    use std::path::PathBuf;

    struct Backend {
        context: ContextDescriptor,
        view: aikit_core::ResolvedView,
        layers: Option<Vec<ScopeLayer>>,
    }

    impl PaletteBackend for Backend {
        fn context(&self) -> &ContextDescriptor {
            &self.context
        }
        fn view(&self) -> &aikit_core::ResolvedView {
            &self.view
        }
        fn scope_layers(&self) -> Option<&[ScopeLayer]> {
            self.layers.as_deref()
        }
        fn documents(&self) -> Vec<aikit_core::SearchDoc> {
            Vec::new()
        }
        fn capsule(&self, _id: &aikit_core::CapsuleId) -> Option<&aikit_core::Capsule> {
            None
        }
        fn recent(&self) -> Vec<crate::RunIntent> {
            Vec::new()
        }
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
        fn promotion_drafts(&self) -> Vec<crate::PromotionDraft> {
            Vec::new()
        }
        fn promote(&mut self, _draft: &crate::PromotionDraft) -> Result<aikit_core::CapsuleId> {
            Err(aikit_core::AikitError::new("test.promote", "unused"))
        }
    }

    fn resolved(context: &ContextDescriptor, layers: Vec<ScopeLayer>) -> aikit_core::ResolvedView {
        let catalog = MemoryCatalog::default();
        let trust = MemoryTrust::default();
        resolve(
            &catalog,
            &trust,
            &ResolveRequest {
                context: context.clone(),
                layers,
                policy: ManagedPolicy::default(),
            },
        )
        .unwrap()
    }

    #[test]
    fn shared_context_resolution_uses_the_current_host_resource() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let view = resolved(&context, Vec::new());
        let backend = Backend {
            context,
            view,
            layers: Some(Vec::new()),
        };

        let resolution = context_resolution(&backend).unwrap();
        assert!(matches!(
            resolution.host,
            Some(ReferenceResolution::Resolved { .. })
        ));
    }

    #[test]
    fn compatibility_service_does_not_invent_unexposed_scope_layers() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let view = resolved(&context, Vec::new());
        let backend = Backend {
            context,
            view,
            layers: None,
        };

        let world = project_world(&backend).unwrap();
        assert!(world.resolution_basis.scopes.is_empty());
        assert!(world
            .warnings
            .iter()
            .any(|warning| warning.contains("scope-layer stack")));
    }

    #[test]
    fn authoritative_scope_stack_is_disclosed_without_compatibility_warning() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let project_layer = ScopeLayer::new(
            ScopeKind::Project,
            LayerOrigin::new("/work/aikit/.aikit/profile.toml"),
            Default::default(),
        );
        let layers = vec![project_layer];
        let view = resolved(&context, layers.clone());
        let backend = Backend {
            context,
            view,
            layers: Some(layers),
        };

        let world = project_world(&backend).unwrap();
        assert_eq!(world.resolution_basis.scopes.len(), 1);
        assert_eq!(world.resolution_basis.scopes[0].kind, ScopeKind::Project);
        assert_eq!(
            world.resolution_basis.scopes[0].origin,
            "/work/aikit/.aikit/profile.toml"
        );
        assert!(!world
            .warnings
            .iter()
            .any(|warning| warning.contains("scope-layer stack")));
    }
}
