//! UI-neutral composition of the resolved V2 application Context.
//!
//! Project/actor/host ContextResolution is application/domain state. Renderers and
//! terminal adapters may supply the already-resolved inputs, but they must not own
//! the rules that turn those inputs into ProjectBinding or actor references.

#[path = "world_inhabitation.rs"]
pub mod world_inhabitation;

use crate::context::ContextDescriptor;
use crate::context_resolution::{compose_context_resolution, ContextResolution, RequestedActors};
use crate::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
use crate::resolve::ResolvedView;
use crate::resource::{ResourceIndex, ResourceRef};
use crate::scope::ScopeLayer;
use crate::Result;

/// Compose the canonical V2 ContextResolution from one resolved application state.
///
/// This function is deliberately I/O-free and surface-neutral. CLI, TUI, agent and
/// future desktop consumers can all feed it the same authoritative descriptor,
/// resolver output, ordered scope stack and ResourceRef-native index. No caller is
/// allowed to infer Project or Host identity from a row/cursor/presentation state.
///
/// `actors` carries the canonical who-is-acting refs (agent/agency) composed from
/// the Actuation instantiation receipt and Central authored projections. Host falls back to
/// the descriptor's machine hostname when the caller supplies none; a caller-supplied
/// host always wins, so the Actuation/Central machine relation stays authoritative.
pub fn application_context_resolution(
    context: &ContextDescriptor,
    view: &ResolvedView,
    scope_layers: &[ScopeLayer],
    resources: &dyn ResourceIndex,
    actors: RequestedActors,
) -> Result<ContextResolution> {
    let project_ref = project_ref(context)?;
    let constituent = ProjectConstituentRef::parse("source:working-tree")?;
    let binding = ProjectBinding::from_legacy_context(project_ref, constituent, context)?;
    application_context_resolution_with_binding(context, view, scope_layers, resources, actors, binding)
}

/// Compose with a native owner's already validated Project binding. Preserve its
/// identity and provenance instead of deriving another ProjectRef from a directory.
pub fn application_context_resolution_with_binding(
    context: &ContextDescriptor,
    view: &ResolvedView,
    scope_layers: &[ScopeLayer],
    resources: &dyn ResourceIndex,
    actors: RequestedActors,
    binding: ProjectBinding,
) -> Result<ContextResolution> {
    let host = (!context.host.trim().is_empty())
        .then(|| ResourceRef::parse(&format!("host/{}", context.host)))
        .transpose()?;

    Ok(compose_context_resolution(
        view,
        binding,
        scope_layers,
        resources,
        RequestedActors {
            agent: actors.agent,
            agency: actors.agency,
            host: actors.host.or(host),
        },
    ))
}

fn project_ref(context: &ContextDescriptor) -> Result<ProjectRef> {
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
    use crate::catalog::MemoryCatalog;
    use crate::context_resolution::ReferenceResolution;
    use crate::policy::ManagedPolicy;
    use crate::resolve::{resolve, ResolveRequest};
    use crate::resource::{ResourceDescriptor, ResourceKind, ResourceRecord, ResourceSearchIndex};
    use crate::scope::{LayerOrigin, ScopeKind, ScopeLayer};
    use crate::trust::MemoryTrust;

    fn resolved(context: &ContextDescriptor, layers: Vec<ScopeLayer>) -> ResolvedView {
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
    fn host_and_project_identity_are_composed_without_a_surface() {
        let mut context = ContextDescriptor::for_project("/work/aikit");
        context.host = "test-host".into();
        let view = resolved(&context, Vec::new());
        let mut resources = ResourceSearchIndex::default();
        resources.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                ResourceRef::parse("host/test-host").unwrap(),
                ResourceKind::Host,
                "test-host",
                "test host",
            )),
            vec![],
        );

        let resolution =
            application_context_resolution(&context, &view, &[], &resources, RequestedActors::default()).unwrap();

        assert!(matches!(
            resolution.host,
            Some(ReferenceResolution::Resolved { .. })
        ));
        assert_eq!(resolution.project_binding.project.as_str(), "project:aikit");
    }

    #[test]
    fn ordered_scope_provenance_is_preserved_by_the_application_seam() {
        let context = ContextDescriptor::for_project("/work/aikit");
        let layer = ScopeLayer::new(
            ScopeKind::Project,
            LayerOrigin::new("/work/aikit/.aikit/profile.toml"),
            Default::default(),
        );
        let view = resolved(&context, vec![layer.clone()]);
        let resources = ResourceSearchIndex::default();

        let resolution = application_context_resolution(&context, &view, &[layer], &resources, RequestedActors::default())
            .unwrap();

        assert_eq!(resolution.scopes.len(), 1);
        assert_eq!(resolution.scopes[0].kind, ScopeKind::Project);
        assert_eq!(
            resolution.scopes[0].origin,
            "/work/aikit/.aikit/profile.toml"
        );
    }

    #[test]
    fn supplied_actors_resolve_agent_agency_and_host() {
        let context = ContextDescriptor::for_project("/work/aikit");
        let view = resolved(&context, Vec::new());
        let mut resources = ResourceSearchIndex::default();
        for (ref_, kind, name) in [
            ("agent/mahamaya", ResourceKind::Agent, "mahamaya"),
            ("agency/mahamaya-build", ResourceKind::Agency, "mahamaya build"),
            ("host/central", ResourceKind::Host, "central"),
        ] {
            resources.insert_resource(
                ResourceRecord::new(ResourceDescriptor::new(
                    ResourceRef::parse(ref_).unwrap(),
                    kind,
                    name,
                    name,
                )),
                vec![],
            );
        }

        let actors = RequestedActors {
            agent: Some(ResourceRef::parse("agent/mahamaya").unwrap()),
            agency: Some(ResourceRef::parse("agency/mahamaya-build").unwrap()),
            host: Some(ResourceRef::parse("host/central").unwrap()),
        };
        let resolution =
            application_context_resolution(&context, &view, &[], &resources, actors).unwrap();

        assert!(matches!(
            resolution.agent,
            Some(ReferenceResolution::Resolved { .. })
        ));
        assert!(matches!(
            resolution.agency,
            Some(ReferenceResolution::Resolved { .. })
        ));
        assert!(matches!(
            resolution.host,
            Some(ReferenceResolution::Resolved { .. })
        ));
    }
}
