//! Pure Project/Context/Compose workspace model for V2-E2.
//!
//! The model is intentionally a view over [`ProjectWorldReadModel`]. It cannot
//! resolve providers, write scope files, materialise projections or retrieve
//! ContextSource payloads. Durable capability mutation continues to use the
//! existing canonical staging/preview/apply path; this module only identifies the
//! target and horizon from which such an action is requested.

use aikit_core::resource::ResourceRef;
use aikit_core::{ContextSourceHit, ProjectWorldReadModel, ProjectWorldResource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeHorizon {
    Capability,
    Information,
    ActorRuntime,
    Projection,
}

impl ComposeHorizon {
    pub const ALL: [Self; 4] = [
        Self::Capability,
        Self::Information,
        Self::ActorRuntime,
        Self::Projection,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "Capabilities",
            Self::Information => "Information",
            Self::ActorRuntime => "Actor / Runtime",
            Self::Projection => "Projection",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectWorkspaceSelection {
    Resource(ResourceRef),
    ContextSource(ResourceRef),
    ProjectionTarget(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectWorkspaceState {
    pub world: ProjectWorldReadModel,
    pub horizon: ComposeHorizon,
    pub selection: Option<ProjectWorkspaceSelection>,
}

impl ProjectWorkspaceState {
    pub fn new(world: ProjectWorldReadModel) -> Self {
        Self {
            world,
            horizon: ComposeHorizon::Capability,
            selection: None,
        }
    }

    pub fn set_horizon(&mut self, horizon: ComposeHorizon) {
        self.horizon = horizon;
        self.selection = None;
    }

    pub fn capability_resources(&self) -> impl Iterator<Item = &ProjectWorldResource> {
        self.world
            .capability_horizon
            .capabilities
            .iter()
            .chain(self.world.capability_horizon.actions.iter())
    }

    pub fn information_sources(&self) -> impl Iterator<Item = &ContextSourceHit> {
        self.world.information_horizon.sources.iter()
    }

    pub fn actor_runtime_resources(&self) -> Vec<&ProjectWorldResource> {
        let mut values = Vec::new();
        if let Some(resource) = self.world.actor_runtime.agent.effective.as_ref() {
            values.push(resource);
        }
        if let Some(resource) = self.world.actor_runtime.agency.effective.as_ref() {
            values.push(resource);
        }
        if let Some(resource) = self.world.actor_runtime.host.effective.as_ref() {
            values.push(resource);
        }
        values.extend(self.world.actor_runtime.models.iter());
        values.extend(self.world.actor_runtime.harnesses.iter());
        values.extend(self.world.actor_runtime.execution_offers.iter());
        values
    }

    pub fn select_resource(&mut self, resource: ResourceRef) {
        self.selection = Some(ProjectWorkspaceSelection::Resource(resource));
    }

    /// Selecting an information source means "this descriptor matters to the
    /// current Compose view". It is deliberately *not* a retrieval operation.
    pub fn select_context_source(&mut self, resource: ResourceRef) -> bool {
        if self
            .world
            .information_horizon
            .sources
            .iter()
            .any(|source| source.resource == resource)
        {
            self.selection = Some(ProjectWorkspaceSelection::ContextSource(resource));
            true
        } else {
            false
        }
    }

    pub fn select_projection_target(&mut self, target: impl Into<String>) {
        self.selection = Some(ProjectWorkspaceSelection::ProjectionTarget(target.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::context::ContextDescriptor;
    use aikit_core::context_resolution::{Availability, ResolvedResource};
    use aikit_core::context_source::{
        AgentVisibility, ContextSourceEntry, ContextSourceIndex, ContextSourceScope,
        DisclosureState, HorizonRequest,
    };
    use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
    use aikit_core::resource::{ResourceDescriptor, ResourceKind, ResourceRecord};
    use aikit_core::{
        ActorRuntimeDisclosure, CapabilityHorizonDisclosure, EffectiveRevisionDisclosure,
        InformationHorizonDisclosure, ProjectWorldReadModel, ProjectWorldResource,
        ProjectionDisclosure, ResolutionBasisDisclosure, PROJECT_WORLD_VERSION,
    };

    fn rref(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn world_with_source() -> ProjectWorldReadModel {
        let source_record = ResourceRecord::new(ResourceDescriptor::new(
            rref("project:context-source:canon"),
            ResourceKind::ContextSource,
            "Design canon",
            "project design canon",
        ));
        let resolved_source = ResolvedResource {
            resource: source_record.clone(),
            availability: Availability::Available,
        };
        let mut entry = ContextSourceEntry::new(source_record).unwrap();
        entry.scope = ContextSourceScope::Project(ProjectRef::parse("project:aikit").unwrap());
        entry.visibility = AgentVisibility::MetadataOnly;
        entry.disclosure = DisclosureState {
            exists: true,
            known_to_exist: true,
            askable: true,
            retrieved: false,
            focused: false,
        };
        let mut index = ContextSourceIndex::default();
        index.insert(entry);
        let sources = index.horizon(&HorizonRequest::human(Some(
            ProjectRef::parse("project:aikit").unwrap(),
        )));
        let context = ContextDescriptor::for_project("/work/aikit");

        ProjectWorldReadModel {
            version: PROJECT_WORLD_VERSION.into(),
            project: ProjectBinding::from_legacy_context(
                ProjectRef::parse("project:aikit").unwrap(),
                ProjectConstituentRef::parse("source:working-tree").unwrap(),
                &context,
            )
            .unwrap(),
            context,
            resolution_basis: ResolutionBasisDisclosure {
                profiles: Vec::new(),
                scopes: Vec::new(),
            },
            capability_horizon: CapabilityHorizonDisclosure::default(),
            information_horizon: InformationHorizonDisclosure {
                resolved_sources: vec![ProjectWorldResource::from(&resolved_source)],
                sources,
                planned_retrieval: vec![rref("project:context-source:canon")],
            },
            actor_runtime: ActorRuntimeDisclosure::default(),
            projection: ProjectionDisclosure {
                targets: Vec::new(),
                active_capabilities: Vec::new(),
            },
            effective_revision: EffectiveRevisionDisclosure {
                generation: None,
                catalog_revision: "r1".into(),
                resolution_hash: "hash".into(),
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn four_compose_horizons_are_explicit_and_not_one_flat_list() {
        assert_eq!(
            ComposeHorizon::ALL.map(ComposeHorizon::as_str),
            ["Capabilities", "Information", "Actor / Runtime", "Projection"]
        );
    }

    #[test]
    fn selecting_context_source_does_not_retrieve_or_invoke_it() {
        let mut state = ProjectWorkspaceState::new(world_with_source());
        state.set_horizon(ComposeHorizon::Information);
        let source = rref("project:context-source:canon");
        assert!(state.select_context_source(source.clone()));
        assert_eq!(
            state.selection,
            Some(ProjectWorkspaceSelection::ContextSource(source))
        );
        let hit = &state.world.information_horizon.sources[0];
        assert!(!hit.disclosure.retrieved);
        assert!(!hit.operational.invoked);
        assert_eq!(
            state.world.information_horizon.planned_retrieval.len(),
            1,
            "selection must not silently consume or execute the retrieval plan"
        );
    }
}
