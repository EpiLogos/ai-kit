//! Resolved Project-world disclosure for the V2 Workspace.
//!
//! This is a read projection over the application service AIKit already has. It
//! does not resolve capabilities, invent actor bindings, or retrieve ContextSource
//! payloads. The purpose is to make the current Project world human-readable while
//! preserving the distinctions the resolver already owns: authored/declared
//! capability intent versus effective activation, shallow addressability versus
//! retrieval, and present resources versus explicit absence.

use std::collections::BTreeSet;

use aikit_core::resource::{ResourceKind, ResourceRef, ResourceSearchIndex};
use aikit_core::scope::ScopeKind;
use serde::{Deserialize, Serialize};

use crate::backend::PaletteBackend;
use crate::navigation::resolved_navigation_index;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorldIdentity {
    #[serde(default)]
    pub resource: Option<ResourceRef>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorldResource {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorldResourceSet {
    pub resources: Vec<ProjectWorldResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absence: Option<String>,
}

impl ProjectWorldResourceSet {
    fn from_index(index: &ResourceSearchIndex, kind: ResourceKind, absent: &str) -> Self {
        let resources = index
            .search(kind.as_str(), usize::MAX)
            .into_iter()
            .filter(|hit| hit.kind == kind)
            .map(|hit| ProjectWorldResource {
                resource: hit.resource,
                kind: hit.kind,
                label: hit.label,
                summary: hit.summary,
            })
            .collect::<Vec<_>>();
        let absence = resources.is_empty().then(|| absent.to_string());
        Self { resources, absence }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredCapability {
    pub resource: ResourceRef,
    pub enabled: bool,
    pub scope: ScopeKind,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnavailableCapability {
    pub resource: ResourceRef,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityHorizon {
    /// Authored intent after deterministic scope folding. This is deliberately
    /// separate from `effective`: declared does not mean active.
    pub declared: Vec<DeclaredCapability>,
    /// Capabilities actually active in the current resolved world.
    pub effective: Vec<ResourceRef>,
    /// Declared/catalogued capabilities whose effective use is currently blocked.
    pub unavailable: Vec<UnavailableCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InformationHorizon {
    /// Addressable descriptors only. Building this read model never invokes a
    /// provider and therefore never changes EXISTS/KNOWN/ASKABLE into RETRIEVED.
    pub context_sources: ProjectWorldResourceSet,
    pub knowledge_spaces: ProjectWorldResourceSet,
    pub knowledge_sources: ProjectWorldResourceSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRuntimeWorld {
    pub agents: ProjectWorldResourceSet,
    pub agencies: ProjectWorldResourceSet,
    pub models: ProjectWorldResourceSet,
    pub harnesses: ProjectWorldResourceSet,
    pub hosts: ProjectWorldResourceSet,
    pub execution_offers: ProjectWorldResourceSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionWorld {
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationDisclosure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorldReadModel {
    /// Resolver/catalog revision. Opaque to the renderer, useful for staleness.
    pub revision: String,
    pub project: ProjectWorldIdentity,
    /// Project-local region/focus exposed by ContextDescriptor rather than inferred
    /// from the current terminal presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    pub resolved_profiles: Vec<String>,
    pub mutation_scopes: Vec<ScopeKind>,
    pub capability_horizon: CapabilityHorizon,
    pub information_horizon: InformationHorizon,
    pub actor_runtime: ActorRuntimeWorld,
    pub projection: ProjectionWorld,
    pub generation: GenerationDisclosure,
    pub boundaries: Vec<String>,
    pub warnings: Vec<String>,
}

impl ProjectWorldReadModel {
    /// Build the complete currently-disclosable Project world from the same
    /// backend used by Quick, Compose and projection preview.
    ///
    /// Empty actor/information collections are represented as structured absence,
    /// never as fabricated defaults. Current Generation is likewise explicitly
    /// absent until the backend exposes that store-owned fact.
    pub fn from_backend(backend: &dyn PaletteBackend) -> Self {
        let context = backend.context();
        let view = backend.view();
        let index = resolved_navigation_index(backend);

        let project_resources = ProjectWorldResourceSet::from_index(
            &index,
            ResourceKind::Project,
            "no Project Resource is addressable in this context",
        );
        let project_resource = project_resources
            .resources
            .first()
            .map(|resource| resource.resource.clone());
        let label = context
            .project_root
            .as_ref()
            .and_then(|root| root.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .or_else(|| context.project_id.as_ref().map(ToString::to_string))
            .unwrap_or_else(|| "no project".to_string());
        let project = ProjectWorldIdentity {
            resource: project_resource,
            project_id: context.project_id.as_ref().map(ToString::to_string),
            root: context
                .project_root
                .as_ref()
                .map(|root| root.display().to_string()),
            label,
        };

        let mut profiles = BTreeSet::new();
        for operation in &view.selection_log {
            if let Some(profile) = operation.via_profile.as_ref() {
                profiles.insert(profile.to_string());
            }
        }
        for profile in ProjectWorldResourceSet::from_index(
            &index,
            ResourceKind::Profile,
            "no Profile Resource is addressable in this context",
        )
        .resources
        {
            profiles.insert(profile.resource.to_string());
        }

        let mut declared = view
            .declared
            .iter()
            .filter_map(|(id, state)| {
                ResourceRef::parse(&id.to_string()).ok().map(|resource| DeclaredCapability {
                    resource,
                    enabled: state.enabled,
                    scope: state.scope,
                    origin: state.origin.to_string(),
                    via_profile: state.via_profile.as_ref().map(ToString::to_string),
                })
            })
            .collect::<Vec<_>>();
        declared.sort_by(|left, right| left.resource.cmp(&right.resource));

        let mut effective = view
            .active
            .keys()
            .filter_map(|id| ResourceRef::parse(&id.to_string()).ok())
            .collect::<Vec<_>>();
        effective.sort();

        let mut unavailable = view
            .unavailable
            .iter()
            .filter_map(|(id, reason)| {
                ResourceRef::parse(&id.to_string()).ok().map(|resource| UnavailableCapability {
                    resource,
                    reason: reason.describe(),
                })
            })
            .collect::<Vec<_>>();
        unavailable.sort_by(|left, right| left.resource.cmp(&right.resource));

        let capability_horizon = CapabilityHorizon {
            declared,
            effective,
            unavailable,
        };

        let information_horizon = InformationHorizon {
            context_sources: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::ContextSource,
                "no ContextSource descriptors are disclosed in this context",
            ),
            knowledge_spaces: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::KnowledgeSpace,
                "no Knowledge Space provider is disclosed in this context",
            ),
            knowledge_sources: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::KnowledgeSource,
                "no Knowledge Source provider is disclosed in this context",
            ),
        };

        let actor_runtime = ActorRuntimeWorld {
            agents: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::Agent,
                "no Agent binding is disclosed in this context",
            ),
            agencies: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::Agency,
                "no Agency binding is disclosed in this context",
            ),
            models: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::Model,
                "no Model candidate is disclosed in this context",
            ),
            harnesses: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::Harness,
                "no Harness candidate is disclosed in this context",
            ),
            hosts: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::Host,
                "no Host Resource is disclosed in this context",
            ),
            execution_offers: ProjectWorldResourceSet::from_index(
                &index,
                ResourceKind::ExecutionOffer,
                "no execution offer is disclosed in this context",
            ),
        };

        let mut boundaries = vec![format!("isolation: {}", context.isolation.as_str())];
        boundaries.push(format!("platform: {}", context.platform));
        if let Some(session) = context.session_id.as_ref() {
            boundaries.push(format!("session: {session}"));
        }
        if let Some(mux) = context.mux {
            boundaries.push(format!("multiplexer: {}", mux.as_str()));
        }

        Self {
            revision: format!("{}:{}", view.catalog_revision, view.hash),
            project,
            focus: context
                .task
                .as_ref()
                .map(|task| format!("task:{task}"))
                .or_else(|| context.project_root.as_ref().map(|_| "project".to_string())),
            resolved_profiles: profiles.into_iter().collect(),
            mutation_scopes: context.permitted_scopes(),
            capability_horizon,
            information_horizon,
            actor_runtime,
            projection: ProjectionWorld {
                targets: context
                    .targets
                    .iter()
                    .map(|target| target.as_str().to_string())
                    .collect(),
            },
            generation: GenerationDisclosure {
                generation: None,
                absence: Some(
                    "current Generation is not exposed by this application backend".to_string(),
                ),
            },
            boundaries,
            warnings: view.warnings.clone(),
        }
    }
}
