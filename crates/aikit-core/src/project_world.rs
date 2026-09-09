//! V2 Project-world disclosure for human and agent composition surfaces.
//!
//! This module is deliberately a read model over existing authoritative contracts.
//! It does not resolve capabilities, retrieve ContextSource payloads, mutate scopes,
//! or materialise projections. `ContextResolution` remains the operational answer;
//! this module makes that answer inspectable without collapsing authored/declared
//! intent into effective provider state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::context::ContextDescriptor;
use crate::credential_world::CredentialWorldDisclosure;
use crate::context_resolution::{
    Availability, ContextResolution, ReferenceResolution, ResolvedResource, ScopeResolution,
};
use crate::context_source::{ContextSourceHit, ContextSourceIndex, HorizonRequest};
use crate::id::{GenerationId, ProfileId};
use crate::platform::TargetId;
use crate::project::ProjectBinding;
use crate::resource::{
    Eligibility, PreferenceIntent, ProviderOffer, ResourceKind, ResourceRef, ResourceSource,
    VersionedProjectWorld,
};

pub const PROJECT_WORLD_VERSION: &str = "aikit.project-world/v2";

/// The declared/policy-facing half of a Resource. These fields explain why a
/// Resource is eligible and whether an explicit preference was authored without
/// claiming that any provider is currently available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceIntentDisclosure {
    pub eligibility: Eligibility,
    #[serde(default)]
    pub preference: Option<PreferenceIntent>,
    /// Sources retain their own authority (`authored`, `observed`, `derived`,
    /// `learned`, `generated`) so the UI never has to infer provenance from prose.
    pub sources: Vec<ResourceSource>,
}

/// The observed/effective half of a Resource. Availability and provider bindings
/// remain orthogonal to eligibility/preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEffectiveDisclosure {
    pub availability: Availability,
    pub providers: Vec<ProviderOffer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorldResource {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub name: String,
    pub description: String,
    /// Owner/provider annotations carried through unchanged from the admitted
    /// Resource. Presentation may disclose these receipts, but must not infer
    /// new domain state from their presence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
    pub intent: ResourceIntentDisclosure,
    pub effective: ResourceEffectiveDisclosure,
}

impl From<&ResolvedResource> for ProjectWorldResource {
    fn from(value: &ResolvedResource) -> Self {
        Self {
            resource: value.resource.descriptor.id.clone(),
            kind: value.resource.descriptor.kind,
            name: value.resource.descriptor.name.clone(),
            description: value.resource.descriptor.description.clone(),
            annotations: value.resource.descriptor.annotations.clone(),
            intent: ResourceIntentDisclosure {
                eligibility: value.resource.eligibility.clone(),
                preference: value.resource.preference.clone(),
                sources: value.resource.descriptor.sources.clone(),
            },
            effective: ResourceEffectiveDisclosure {
                availability: value.availability.clone(),
                providers: value.resource.providers.clone(),
            },
        }
    }
}

/// Requested actor identity is preserved even when it cannot be resolved. A
/// resolved record is a separate field rather than overwriting the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActorDisclosure {
    #[serde(default)]
    pub requested: Option<ResourceRef>,
    #[serde(default)]
    pub effective: Option<ProjectWorldResource>,
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActorRuntimeDisclosure {
    pub agent: ActorDisclosure,
    pub agency: ActorDisclosure,
    pub host: ActorDisclosure,
    pub models: Vec<ProjectWorldResource>,
    pub harnesses: Vec<ProjectWorldResource>,
    pub execution_offers: Vec<ProjectWorldResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CapabilityHorizonDisclosure {
    pub capabilities: Vec<ProjectWorldResource>,
    pub actions: Vec<ProjectWorldResource>,
}

/// Information horizon is addressability/disclosure, not prompt inclusion.
/// `sources` is populated through `ContextSourceIndex::horizon`, a descriptor-only
/// operation. `planned_retrieval` names what may be retrieved later but this read
/// model never calls `ContextSourceIndex::retrieve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InformationHorizonDisclosure {
    pub resolved_sources: Vec<ProjectWorldResource>,
    pub sources: Vec<ContextSourceHit>,
    pub planned_retrieval: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionBasisDisclosure {
    pub profiles: Vec<ProfileId>,
    pub scopes: Vec<ScopeResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionDisclosure {
    pub targets: Vec<TargetId>,
    /// Effective legacy capability projection retained while target adapters still
    /// consume `ResolvedView`. This must not be mistaken for authored preference.
    pub active_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveRevisionDisclosure {
    #[serde(default)]
    pub generation: Option<GenerationId>,
    pub catalog_revision: String,
    pub resolution_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProjectWorldReadModel {
    pub version: String,
    pub project: ProjectBinding,
    /// Existing Context identity/focus remains authoritative. Project-world does
    /// not mint another session/task/host identity.
    pub context: ContextDescriptor,
    pub resolution_basis: ResolutionBasisDisclosure,
    pub capability_horizon: CapabilityHorizonDisclosure,
    pub information_horizon: InformationHorizonDisclosure,
    pub actor_runtime: ActorRuntimeDisclosure,
    /// Factory owner observations admitted into the shared Resource/Navigator field.
    /// These categories carry no AIKit-owned developmental semantics.
    #[serde(default)]
    pub developmental_work: Vec<ProjectWorldResource>,
    pub projection: ProjectionDisclosure,
    pub effective_revision: EffectiveRevisionDisclosure,
    /// Optional material/version reading from an accepted provider such as native
    /// Git. This is attached only after Project identity has already been resolved.
    /// Repository/worktree/branch/commit identity therefore remains subordinate
    /// material/history evidence rather than a second Project resolver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versioned_world: Option<VersionedProjectWorld>,
    /// Credential/provider status for this world. Deliberately not an
    /// `Option`: an absent credential input is a
    /// `CredentialWorldDisclosure::not_attempted(..)` reading, which is a
    /// different fact from a positively observed empty provider roster. A
    /// `None` here would collapse that distinction back into the ambiguity
    /// `credential_world.rs` exists to remove.
    #[serde(default)]
    pub credential_world: CredentialWorldDisclosure,
    pub warnings: Vec<String>,
}

impl ProjectWorldReadModel {
    /// Attach an already-observed versioned material World to this resolved
    /// Project reading. A provider cannot use this operation to rename/rebind the
    /// canonical Project: mismatched ProjectRefs are rejected.
    pub fn with_versioned_world(mut self, versioned: VersionedProjectWorld) -> crate::Result<Self> {
        if versioned.project != self.project.project {
            return Err(crate::AikitError::new(
                "project_world.versioned_project_mismatch",
                format!(
                    "versioned material belongs to {}, resolved Project is {}",
                    versioned.project, self.project.project
                ),
            ));
        }
        self.versioned_world = Some(versioned);
        Ok(self)
    }

    /// Attach an already-composed credential/provider reading to this resolved
    /// Project reading, mirroring `with_versioned_world`. `aikit-core` is
    /// I/O-free: the caller gathers the provider roster and requirements and
    /// composes the disclosure through `disclose_credential_world`, which is a
    /// pure function over already-observed facts.
    pub fn with_credential_world(mut self, credential_world: CredentialWorldDisclosure) -> Self {
        self.credential_world = credential_world;
        self
    }

    /// A minimal well-formed reading bound to an already-resolved Project.
    ///
    /// This is the one construction path open to consumers outside this crate,
    /// and it is what makes `#[non_exhaustive]` bearable: a disclosure added
    /// here later gets an honest default rather than breaking every caller's
    /// struct literal — the exact fragility that kept `credential_world` out of
    /// this struct when it was first written. Every field stays `pub`, so a
    /// caller assigns whatever it actually knows and leaves the rest honestly
    /// unresolved.
    pub fn empty(project: ProjectBinding, context: ContextDescriptor) -> Self {
        Self {
            version: PROJECT_WORLD_VERSION.to_string(),
            project,
            context,
            resolution_basis: ResolutionBasisDisclosure {
                profiles: Vec::new(),
                scopes: Vec::new(),
            },
            capability_horizon: CapabilityHorizonDisclosure::default(),
            information_horizon: InformationHorizonDisclosure::default(),
            actor_runtime: ActorRuntimeDisclosure::default(),
            developmental_work: Vec::new(),
            projection: ProjectionDisclosure {
                targets: Vec::new(),
                active_capabilities: Vec::new(),
            },
            effective_revision: EffectiveRevisionDisclosure {
                generation: None,
                catalog_revision: "unresolved".to_string(),
                resolution_hash: "unresolved".to_string(),
            },
            versioned_world: None,
            credential_world: CredentialWorldDisclosure::not_attempted(
                "this reading was built as a shell; no credential input was composed into it",
            ),
            warnings: Vec::new(),
        }
    }
}

pub fn disclose_project_world(
    resolution: &ContextResolution,
    context_sources: &ContextSourceIndex,
    generation: Option<GenerationId>,
) -> ProjectWorldReadModel {
    let project = resolution.project_binding.project.clone();
    let information_sources = context_sources.horizon(&HorizonRequest::human(Some(project)));

    ProjectWorldReadModel {
        version: PROJECT_WORLD_VERSION.to_string(),
        project: resolution.project_binding.clone(),
        context: resolution.deterministic.context.clone(),
        resolution_basis: ResolutionBasisDisclosure {
            profiles: resolution.profiles.clone(),
            scopes: resolution.scopes.clone(),
        },
        capability_horizon: CapabilityHorizonDisclosure {
            capabilities: disclose_resources(&resolution.capabilities),
            actions: disclose_resources(&resolution.actions),
        },
        information_horizon: InformationHorizonDisclosure {
            resolved_sources: disclose_resources(&resolution.context_sources),
            sources: information_sources,
            planned_retrieval: resolution.retrieval.context_sources.clone(),
        },
        actor_runtime: ActorRuntimeDisclosure {
            agent: disclose_actor(resolution.agent.as_ref(), "agent"),
            agency: disclose_actor(resolution.agency.as_ref(), "agency"),
            host: disclose_actor(resolution.host.as_ref(), "host"),
            models: disclose_resources(&resolution.model_candidates),
            harnesses: disclose_resources(&resolution.harness_candidates),
            execution_offers: disclose_resources(&resolution.execution_offers),
        },
        developmental_work: disclose_resources(&resolution.developmental_resources),
        projection: ProjectionDisclosure {
            targets: resolution.projection.targets.clone(),
            active_capabilities: resolution.projection.active_capabilities.clone(),
        },
        effective_revision: EffectiveRevisionDisclosure {
            generation,
            catalog_revision: resolution.deterministic.catalog_revision.to_string(),
            resolution_hash: resolution.deterministic.hash.to_string(),
        },
        versioned_world: None,
        credential_world: CredentialWorldDisclosure::not_attempted(
            "disclose_project_world was given no credential roster; a caller attaches one with with_credential_world",
        ),
        warnings: resolution.warnings.clone(),
    }
}

fn disclose_resources(resources: &[ResolvedResource]) -> Vec<ProjectWorldResource> {
    resources.iter().map(ProjectWorldResource::from).collect()
}

fn disclose_actor(resolution: Option<&ReferenceResolution>, role: &str) -> ActorDisclosure {
    let Some(resolution) = resolution else {
        return ActorDisclosure::default();
    };
    match resolution {
        ReferenceResolution::Resolved { resource } => ActorDisclosure {
            requested: Some(resource.resource.descriptor.id.clone()),
            effective: Some(ProjectWorldResource::from(resource.as_ref())),
            warning: None,
        },
        ReferenceResolution::Missing {
            reference,
            expected,
        } => ActorDisclosure {
            requested: Some(reference.clone()),
            effective: None,
            warning: Some(format!(
                "requested {role} {reference} is missing (expected {})",
                expected.as_str()
            )),
        },
        ReferenceResolution::WrongKind {
            reference,
            expected,
            actual,
        } => ActorDisclosure {
            requested: Some(reference.clone()),
            effective: None,
            warning: Some(format!(
                "requested {role} {reference} is {}, expected {}",
                actual.as_str(),
                expected.as_str()
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_source::{ContextSourceEntry, DisclosureState};
    use crate::project::{ProjectBindingLocator, ProjectConstituentRef, ProjectRef};
    use crate::resource::{
        GitRepositoryRelation, GitWorkingState, ProviderOffer, ProviderRef, ProviderState,
        ResourceDescriptor, ResourceRecord, ResourceSource, SourceAuthority, SourceRef, SourceState,
        VersionRevision, VersionedWorldCapability, VersionedWorldProviderDescriptor,
        VersionedWorldProviderStatus, VERSIONED_WORLD_VERSION,
    };

    fn resource(id: &str, kind: ResourceKind) -> ResolvedResource {
        let mut descriptor = ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            kind,
            "Example",
            "example resource",
        );
        descriptor.sources.push(ResourceSource {
            source: SourceRef::parse("project-manifest").unwrap(),
            authority: Some(SourceAuthority::Authored),
            revision: None,
            locator: None,
            state: SourceState::Available,
        });
        let mut record = ResourceRecord::new(descriptor);
        record.eligibility = Eligibility::Eligible;
        record.preference = Some(PreferenceIntent {
            source: SourceRef::parse("project-preference").unwrap(),
            rank: 10,
            rationale: Some("explicit project choice".into()),
        });
        record.providers.push(ProviderOffer {
            provider: ProviderRef::parse("local-provider").unwrap(),
            locator: None,
            state: ProviderState::Unavailable {
                reason: "offline".into(),
            },
        });
        ResolvedResource {
            resource: record,
            availability: Availability::Unavailable {
                reasons: vec!["provider local-provider unavailable: offline".into()],
            },
        }
    }

    fn project_world(project_ref: &str) -> ProjectWorldReadModel {
        let project = ProjectRef::parse(project_ref).unwrap();
        ProjectWorldReadModel {
            version: PROJECT_WORLD_VERSION.to_string(),
            project: ProjectBinding::new(
                project,
                ProjectConstituentRef::parse("source:working-tree").unwrap(),
                ProjectBindingLocator::LocalDirectory { path: "/tmp/example".into() },
            ),
            context: ContextDescriptor::for_project("/tmp/example"),
            resolution_basis: ResolutionBasisDisclosure { profiles: vec![], scopes: vec![] },
            capability_horizon: CapabilityHorizonDisclosure::default(),
            information_horizon: InformationHorizonDisclosure::default(),
            actor_runtime: ActorRuntimeDisclosure::default(),
            developmental_work: Vec::new(),
            projection: ProjectionDisclosure { targets: vec![], active_capabilities: vec![] },
            effective_revision: EffectiveRevisionDisclosure {
                generation: None,
                catalog_revision: "catalog@1".into(),
                resolution_hash: "resolution@1".into(),
            },
            versioned_world: None,
            credential_world: CredentialWorldDisclosure::not_attempted(
                "test fixture composed no credential input",
            ),
            warnings: vec![],
        }
    }

    fn versioned_world(project_ref: &str) -> VersionedProjectWorld {
        VersionedProjectWorld {
            version: VERSIONED_WORLD_VERSION.to_string(),
            project: ProjectRef::parse(project_ref).unwrap(),
            provider: VersionedWorldProviderDescriptor {
                provider: ProviderRef::parse("native-git").unwrap(),
                status: VersionedWorldProviderStatus::Available,
                capabilities: vec![VersionedWorldCapability::Inspect, VersionedWorldCapability::History],
                implementation_version: Some("git test".into()),
            },
            repository: GitRepositoryRelation {
                repository_root: "/tmp/example".into(),
                worktree_root: "/tmp/example".into(),
                head: VersionRevision::new("abc123"),
                branch: Some("main".into()),
                detached: false,
                upstream: None,
                ahead: 0,
                behind: 0,
            },
            working: GitWorkingState::default(),
            worktrees: vec![],
        }
    }

    #[test]
    fn resource_intent_and_effective_binding_remain_separate() {
        let disclosed = ProjectWorldResource::from(&resource(
            "project:capability:review",
            ResourceKind::Capability,
        ));

        assert_eq!(disclosed.intent.eligibility, Eligibility::Eligible);
        assert_eq!(disclosed.intent.preference.as_ref().unwrap().rank, 10);
        assert_eq!(
            disclosed.intent.sources[0].authority,
            Some(SourceAuthority::Authored)
        );
        assert!(matches!(
            disclosed.effective.availability,
            Availability::Unavailable { .. }
        ));
        assert!(matches!(
            disclosed.effective.providers[0].state,
            ProviderState::Unavailable { .. }
        ));
    }

    #[test]
    fn missing_actor_request_is_preserved_without_fake_effective_identity() {
        let reference = ResourceRef::parse("project:agency:design").unwrap();
        let disclosed = disclose_actor(
            Some(&ReferenceResolution::Missing {
                reference: reference.clone(),
                expected: ResourceKind::Agency,
            }),
            "agency",
        );

        assert_eq!(disclosed.requested, Some(reference));
        assert!(disclosed.effective.is_none());
        assert!(disclosed.warning.unwrap().contains("missing"));
    }

    #[test]
    fn context_source_horizon_disclosure_does_not_mark_payload_retrieved() {
        let source = resource(
            "project:context-source:design-canon",
            ResourceKind::ContextSource,
        );
        let mut entry = ContextSourceEntry::new(source.resource).unwrap();
        entry.disclosure = DisclosureState {
            exists: true,
            known_to_exist: true,
            askable: true,
            retrieved: false,
            focused: false,
        };
        let mut index = ContextSourceIndex::default();
        index.insert(entry);

        let hits = index.horizon(&HorizonRequest::human(None));
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].disclosure.retrieved);
        assert!(!index.get(&hits[0].resource).unwrap().operational.invoked);
    }

    #[test]
    fn matching_versioned_world_attaches_without_rebinding_project_identity() {
        let reading = project_world("project:alpha")
            .with_versioned_world(versioned_world("project:alpha"))
            .unwrap();
        assert_eq!(reading.project.project.as_str(), "project:alpha");
        assert_eq!(reading.versioned_world.as_ref().unwrap().project.as_str(), "project:alpha");
        assert_eq!(
            reading.versioned_world.as_ref().unwrap().repository.branch.as_deref(),
            Some("main")
        );
    }

    /// A stored reading written before `credential_world` existed must load as
    /// `not_attempted` — an open question — rather than defaulting into a
    /// positive claim that the world observed no providers.
    #[test]
    fn a_reading_without_a_credential_field_loads_as_not_attempted() {
        let world = project_world("project:alpha");
        let mut payload: serde_json::Value = serde_json::to_value(&world).unwrap();
        payload.as_object_mut().unwrap().remove("credential_world").unwrap();

        let restored: ProjectWorldReadModel = serde_json::from_value(payload).unwrap();
        assert!(!restored.credential_world.fully_observed());
        assert_eq!(restored.credential_world.providers.providers(), None);
    }

    #[test]
    fn an_attached_credential_world_survives_a_round_trip() {
        let world = project_world("project:alpha").with_credential_world(
            crate::credential_world::disclose_credential_world(
                crate::credential_world::ProviderRosterKnowledge::Observed { providers: vec![] },
                &[],
                false,
                false,
            ),
        );
        assert!(world.credential_world.providers.is_known());

        let restored: ProjectWorldReadModel =
            serde_json::from_str(&serde_json::to_string(&world).unwrap()).unwrap();
        assert_eq!(restored.credential_world, world.credential_world);
    }

    #[test]
    fn versioned_provider_cannot_rebind_the_resolved_project() {
        let error = project_world("project:alpha")
            .with_versioned_world(versioned_world("project:other"))
            .unwrap_err();
        assert_eq!(error.code(), "project_world.versioned_project_mismatch");
    }
}
