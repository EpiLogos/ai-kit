//! Project-world projection for the shared V2 application service.
//!
//! The domain operation that composes ProjectBinding, Host, ordered scope
//! provenance and the ResourceRef-native field is core-owned. This module is only
//! the TUI/backend adapter plus disclosure of ContextSource horizon state.

use aikit_core::context_source::{ContextSourceEntry, ContextSourceIndex};
use aikit_core::resource::{ResourceIndex, ResourceKind};
use aikit_core::{
    application_context_resolution, disclose_project_world, ContextResolution,
    ProjectWorldReadModel, RequestedActors, Result,
};

use crate::PaletteBackend;

/// Obtain the canonical application ContextResolution from the current backend.
///
/// The retained `PaletteBackend` name is a compatibility seam only. Project/Host
/// identity and scope composition are resolved in `aikit-core`, so the renderer
/// cannot become a semantic boundary around Context.
pub fn context_resolution(backend: &dyn PaletteBackend) -> Result<ContextResolution> {
    context_resolution_with_actors(backend, RequestedActors::default())
}

/// As [`context_resolution`], with the caller's composed actor refs (Actuation
/// instantiation receipt + Central authored) supplied explicitly. Host falls back to the
/// descriptor's machine hostname when `actors.host` is unset.
pub fn context_resolution_with_actors(
    backend: &dyn PaletteBackend,
    actors: RequestedActors,
) -> Result<ContextResolution> {
    let resources = resource_index(backend)?;
    context_resolution_from_resources(backend, actors, &resources)
}

/// Resolve once-observed resources without fetching their owners again.
pub fn context_resolution_from_resources(
    backend: &dyn PaletteBackend,
    actors: RequestedActors,
    resources: &dyn ResourceIndex,
) -> Result<ContextResolution> {
    if let Some(binding) = backend.project_binding()? {
        return aikit_core::application_context_resolution_with_binding(
            backend.context(),
            backend.view(),
            backend.scope_layers().unwrap_or(&[]),
            resources,
            actors,
            binding,
        );
    }
    application_context_resolution(
        backend.context(),
        backend.view(),
        backend.scope_layers().unwrap_or(&[]),
        resources,
        actors,
    )
}

/// Shared canonical resource join used by Context and situated Method resolution.
pub fn resource_index(
    backend: &dyn PaletteBackend,
) -> Result<aikit_core::resource::ResourceSearchIndex> {
    resource_index_with_records(backend, backend.context_resource_records()?)
}

/// Join an already-observed owner snapshot; this function performs no source read.
pub fn resource_index_with_records(
    backend: &dyn PaletteBackend,
    records: Vec<aikit_core::resource::ResourceRecord>,
) -> Result<aikit_core::resource::ResourceSearchIndex> {
    let mut resources = backend.navigation_index();
    for record in records {
        let joined = if let Some(existing) = resources.resource(&record.descriptor.id) {
            if existing.descriptor.kind != record.descriptor.kind {
                return Err(aikit_core::AikitError::new(
                    "context.source_kind_conflict",
                    format!(
                        "Observed source kind conflicts with existing resource {}",
                        record.descriptor.id
                    ),
                ));
            }
            let mut joined = existing.clone();
            for source in record.descriptor.sources {
                if !joined.descriptor.sources.contains(&source) {
                    joined.descriptor.sources.push(source);
                }
            }
            for (key, value) in record.descriptor.annotations {
                joined.descriptor.annotations.entry(key).or_insert(value);
            }
            joined
        } else {
            record
        };
        resources.insert_resource(joined, Vec::new());
    }
    Ok(resources)
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

    // The versioned material World, when the backend can observe one. A
    // mismatched ProjectRef is refused by `with_versioned_world` rather than
    // renaming the canonical Project, and an observation that fails becomes a
    // warning on the reading: a Compose preview that cannot see the material
    // must say so, not fall back to a clean-looking absence that means
    // something else.
    match backend.versioned_world() {
        Ok(Some(versioned)) => match world.clone().with_versioned_world(versioned) {
            Ok(attached) => world = attached,
            Err(error) => world.warnings.push(format!(
                "versioned material was observed but does not belong to this Project: {}",
                error.message()
            )),
        },
        Ok(None) => {}
        Err(error) => world.warnings.push(format!(
            "versioned material provider could not observe this Project: {}",
            error.message()
        )),
    }

    if backend.scope_layers().is_none() {
        world.warnings.push(
            "Project-world basis does not include the ordered scope-layer stack because this application-service boundary does not expose it; scope provenance is not reconstructed from partial evidence"
                .into(),
        );
    }
    Ok(world)
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
        /// What this backend can observe about versioned material: nothing,
        /// an observation, or a provider that fell over.
        versioned: VersionedAnswer,
    }

    #[derive(Default)]
    enum VersionedAnswer {
        #[default]
        Nothing,
        Observed(Box<aikit_core::resource::VersionedProjectWorld>),
        Failed,
    }

    impl PaletteBackend for Backend {
        fn versioned_world(&self) -> Result<Option<aikit_core::resource::VersionedProjectWorld>> {
            match &self.versioned {
                VersionedAnswer::Nothing => Ok(None),
                VersionedAnswer::Observed(world) => Ok(Some(world.as_ref().clone())),
                VersionedAnswer::Failed => Err(aikit_core::AikitError::new(
                    "versioned_world.git_spawn_failed",
                    "failed to invoke git",
                )),
            }
        }

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
        resolve(
            &MemoryCatalog::default(),
            &MemoryTrust::default(),
            &ResolveRequest {
                context: context.clone(),
                layers,
                policy: ManagedPolicy::default(),
            },
        )
        .unwrap()
    }

    #[test]
    fn tui_adapter_uses_core_application_context_resolution() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let view = resolved(&context, Vec::new());
        let backend = Backend {
            context,
            view,
            layers: Some(Vec::new()),
            versioned: VersionedAnswer::Nothing,
        };

        let resolution = context_resolution(&backend).unwrap();
        assert!(matches!(
            resolution.host,
            Some(ReferenceResolution::Resolved { .. })
        ));
        assert_eq!(resolution.project_binding.project.as_str(), "project:aikit");
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
            versioned: VersionedAnswer::Nothing,
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
            versioned: VersionedAnswer::Nothing,
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

    /// A fixture observation for the Project the fake backend resolves to.
    fn observed(project: &str) -> aikit_core::resource::VersionedProjectWorld {
        use aikit_core::resource::{
            GitRepositoryRelation, GitWorkingState, VersionRevision, VersionedProjectWorld,
            VersionedWorldProviderDescriptor, VersionedWorldProviderStatus,
            VERSIONED_WORLD_VERSION,
        };
        VersionedProjectWorld {
            version: VERSIONED_WORLD_VERSION.to_string(),
            project: aikit_core::project::ProjectRef::parse(project).unwrap(),
            provider: VersionedWorldProviderDescriptor {
                provider: aikit_core::resource::ProviderRef::parse("provider:aikit.git-cli")
                    .unwrap(),
                status: VersionedWorldProviderStatus::Available,
                capabilities: Vec::new(),
                implementation_version: Some("2.43.0".into()),
            },
            repository: GitRepositoryRelation {
                repository_root: "/work/aikit".into(),
                worktree_root: "/work/aikit".into(),
                head: VersionRevision::new("c0ffee"),
                branch: Some("main".into()),
                detached: false,
                upstream: None,
                ahead: 0,
                behind: 0,
            },
            working: GitWorkingState::default(),
            worktrees: Vec::new(),
        }
    }

    fn world_from(versioned: VersionedAnswer) -> aikit_core::ProjectWorldReadModel {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let view = resolved(&context, Vec::new());
        project_world(&Backend {
            context,
            view,
            layers: Some(Vec::new()),
            versioned,
        })
        .unwrap()
    }

    /// Before this wiring the socket was permanently empty and the Compose
    /// preview said so on every reading. An observation the backend can make
    /// now reaches the model the preview renders.
    #[test]
    fn an_observation_the_backend_can_make_reaches_the_reading() {
        let world = world_from(VersionedAnswer::Observed(Box::new(observed(
            "project:aikit",
        ))));
        let versioned = world
            .versioned_world
            .as_ref()
            .expect("the observation reaches the reading");
        assert_eq!(versioned.repository.branch.as_deref(), Some("main"));
        assert!(world.warnings.iter().all(|w| !w.contains("versioned")));
    }

    /// A backend with nothing to say leaves the absence exactly as it was —
    /// this capability changes nothing for a Project nobody can observe.
    #[test]
    fn a_backend_that_observes_nothing_leaves_an_honest_absence() {
        let world = world_from(VersionedAnswer::Nothing);
        assert!(world.versioned_world.is_none());
        assert!(world.warnings.iter().all(|w| !w.contains("versioned")));
    }

    /// A provider that could not run is disclosed, not silently folded into
    /// the same absence as "this Project has no versioned material".
    #[test]
    fn a_provider_that_could_not_run_is_disclosed_rather_than_read_as_absence() {
        let world = world_from(VersionedAnswer::Failed);
        assert!(world.versioned_world.is_none());
        assert!(
            world
                .warnings
                .iter()
                .any(|w| w.contains("could not observe this Project")),
            "{:?}",
            world.warnings
        );
    }

    /// Material belonging to another Project is refused: a provider may not
    /// rename or rebind the canonical Project through this door.
    #[test]
    fn material_from_another_project_is_refused_and_disclosed() {
        let world = world_from(VersionedAnswer::Observed(Box::new(observed(
            "project:elsewhere",
        ))));
        assert!(world.versioned_world.is_none());
        assert!(
            world
                .warnings
                .iter()
                .any(|w| w.contains("does not belong to this Project")),
            "{:?}",
            world.warnings
        );
    }
}
