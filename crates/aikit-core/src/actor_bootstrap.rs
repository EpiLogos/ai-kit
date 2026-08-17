//! Thin actor/harness bootstrap projection for V2.
//!
//! A bootstrap is an orientation seed, not a second ContextResolution and not a
//! serialized HarnessComposition. It preserves actor/project identity and enough
//! provenance to explain the binding, while summarising large horizons and giving
//! composition-capable harnesses only an inspectable body pointer.

use serde::{Deserialize, Serialize};

use crate::composition::{CompositionState, HarnessComposition};
use crate::context_resolution::{
    Availability, ContextResolution, ReferenceResolution, ResolvedResource, ScopeResolution,
};
use crate::platform::TargetId;
use crate::project::ProjectBinding;
use crate::resource::{ProviderOffer, ResourceKind, ResourceRef, ResourceSource};
use crate::{AikitError, Result};

pub const ACTOR_BOOTSTRAP_VERSION: &str = "aikit.actor-bootstrap/v2";
pub const BOOTSTRAP_RESOURCE_SAMPLE_LIMIT: usize = 12;

/// Compact equivalent of ReferenceResolution. Resolved resources retain their
/// source/provider provenance, but the bootstrap does not copy the resource index
/// or unrelated candidates into standing prompt context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum BootstrapReference {
    Resolved {
        resource: ResourceRef,
        kind: ResourceKind,
        availability: Availability,
        #[serde(default)]
        sources: Vec<ResourceSource>,
        #[serde(default)]
        providers: Vec<ProviderOffer>,
    },
    Missing {
        reference: ResourceRef,
        expected: ResourceKind,
    },
    WrongKind {
        reference: ResourceRef,
        expected: ResourceKind,
        actual: ResourceKind,
    },
}

impl BootstrapReference {
    pub fn resource(&self) -> &ResourceRef {
        match self {
            Self::Resolved { resource, .. }
            | Self::Missing {
                reference: resource,
                ..
            }
            | Self::WrongKind {
                reference: resource,
                ..
            } => resource,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSetSummary {
    pub total: usize,
    pub available: usize,
    pub unresolved: usize,
    pub unavailable: usize,
    pub examples: Vec<ResourceRef>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeBodyInspection {
    ExplainComponent,
    DiffHistory,
}

/// A discovery pointer only. The body can be fetched/explained through the
/// application service when needed; its full Component graph is deliberately not
/// part of the standing actor bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionPointer {
    pub harness: ResourceRef,
    pub fingerprint: String,
    pub state: CompositionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub component_count: usize,
    pub contract_binding_count: usize,
    pub contribution_count: usize,
    pub surface_count: usize,
    pub absence_count: usize,
    pub inspection: Vec<RuntimeBodyInspection>,
}

impl From<&HarnessComposition> for HarnessCompositionPointer {
    fn from(body: &HarnessComposition) -> Self {
        Self {
            harness: body.harness.clone(),
            fingerprint: body.fingerprint.clone(),
            state: body.state,
            target_revision: body.target_revision.clone(),
            generation: body.generation.clone(),
            component_count: body.component_bindings.len(),
            contract_binding_count: body.contract_bindings.len(),
            contribution_count: body.contributions.len(),
            surface_count: body.surfaces.len(),
            absence_count: body.absences.len(),
            inspection: vec![
                RuntimeBodyInspection::ExplainComponent,
                RuntimeBodyInspection::DiffHistory,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorBootstrap {
    pub version: String,
    pub project: ProjectBinding,
    /// Run is client-supplied operational identity. AIKit preserves it verbatim;
    /// changing session/model/harness/body never manufactures a replacement Run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<ResourceRef>,
    pub profiles: Vec<String>,
    pub scopes: Vec<ScopeResolution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<BootstrapReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<BootstrapReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<BootstrapReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<BootstrapReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<BootstrapReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    pub capabilities: ResourceSetSummary,
    pub actions: ResourceSetSummary,
    pub context_sources: ResourceSetSummary,
    pub projection_targets: Vec<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_body: Option<HarnessCompositionPointer>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ActorBootstrapRequest<'a> {
    pub run: Option<ResourceRef>,
    pub selected_harness: Option<ResourceRef>,
    pub selected_model: Option<ResourceRef>,
    pub agent_session: Option<String>,
    pub runtime_body: Option<&'a HarnessComposition>,
}

pub fn project_actor_bootstrap(
    resolution: &ContextResolution,
    request: ActorBootstrapRequest<'_>,
) -> Result<ActorBootstrap> {
    let agent = resolution.agent.as_ref().map(summarize_reference);
    let agency = resolution.agency.as_ref().map(summarize_reference);
    let host = resolution.host.as_ref().map(summarize_reference);
    let harness = request.selected_harness.as_ref().map(|selected| {
        summarize_selected(
            selected,
            ResourceKind::Harness,
            &resolution.harness_candidates,
        )
    });
    let model = request.selected_model.as_ref().map(|selected| {
        summarize_selected(selected, ResourceKind::Model, &resolution.model_candidates)
    });

    if let Some(body) = request.runtime_body {
        validate_body_identity(resolution, &request, body, agent.as_ref(), agency.as_ref())?;
    }

    Ok(ActorBootstrap {
        version: ACTOR_BOOTSTRAP_VERSION.to_string(),
        project: resolution.project_binding.clone(),
        run: request.run,
        profiles: resolution
            .profiles
            .iter()
            .map(ToString::to_string)
            .collect(),
        scopes: resolution.scopes.clone(),
        agent,
        agency,
        host,
        harness,
        model,
        agent_session: request.agent_session,
        capabilities: summarize_set(&resolution.capabilities),
        actions: summarize_set(&resolution.actions),
        context_sources: summarize_set(&resolution.context_sources),
        projection_targets: resolution.projection.targets.clone(),
        runtime_body: request.runtime_body.map(HarnessCompositionPointer::from),
        warnings: resolution.warnings.clone(),
    })
}

fn summarize_reference(reference: &ReferenceResolution) -> BootstrapReference {
    match reference {
        ReferenceResolution::Resolved { resource } => summarize_resolved(resource),
        ReferenceResolution::Missing {
            reference,
            expected,
        } => BootstrapReference::Missing {
            reference: reference.clone(),
            expected: *expected,
        },
        ReferenceResolution::WrongKind {
            reference,
            expected,
            actual,
        } => BootstrapReference::WrongKind {
            reference: reference.clone(),
            expected: *expected,
            actual: *actual,
        },
    }
}

fn summarize_selected(
    selected: &ResourceRef,
    expected: ResourceKind,
    candidates: &[ResolvedResource],
) -> BootstrapReference {
    candidates
        .iter()
        .find(|candidate| candidate.resource.descriptor.id == *selected)
        .map(summarize_resolved)
        .unwrap_or_else(|| BootstrapReference::Missing {
            reference: selected.clone(),
            expected,
        })
}

fn summarize_resolved(resource: &ResolvedResource) -> BootstrapReference {
    BootstrapReference::Resolved {
        resource: resource.resource.descriptor.id.clone(),
        kind: resource.resource.descriptor.kind,
        availability: resource.availability.clone(),
        sources: resource.resource.descriptor.sources.clone(),
        providers: resource.resource.providers.clone(),
    }
}

fn summarize_set(resources: &[ResolvedResource]) -> ResourceSetSummary {
    let mut available = 0;
    let mut unresolved = 0;
    let mut unavailable = 0;
    for resource in resources {
        match &resource.availability {
            Availability::Available => available += 1,
            Availability::Unresolved { .. } => unresolved += 1,
            Availability::Unavailable { .. } => unavailable += 1,
        }
    }
    let examples = resources
        .iter()
        .take(BOOTSTRAP_RESOURCE_SAMPLE_LIMIT)
        .map(|resource| resource.resource.descriptor.id.clone())
        .collect::<Vec<_>>();
    ResourceSetSummary {
        total: resources.len(),
        available,
        unresolved,
        unavailable,
        truncated: resources.len() > examples.len(),
        examples,
    }
}

fn validate_body_identity(
    resolution: &ContextResolution,
    request: &ActorBootstrapRequest<'_>,
    body: &HarnessComposition,
    agent: Option<&BootstrapReference>,
    agency: Option<&BootstrapReference>,
) -> Result<()> {
    let selected_harness = request.selected_harness.as_ref().ok_or_else(|| {
        AikitError::new(
            "bootstrap.runtime_body_without_harness_binding",
            "a HarnessComposition pointer requires an explicit resolved Harness binding",
        )
    })?;
    if selected_harness != &body.harness {
        return Err(identity_error("harness", selected_harness, &body.harness));
    }
    if let (Some(selected_model), Some(body_model)) =
        (request.selected_model.as_ref(), body.model.as_ref())
    {
        if selected_model != body_model {
            return Err(identity_error("model", selected_model, body_model));
        }
    }
    if let (Some(session), Some(body_session)) =
        (request.agent_session.as_ref(), body.session.as_ref())
    {
        if session != body_session {
            return Err(AikitError::new(
                "bootstrap.runtime_body_session_mismatch",
                format!(
                    "runtime body session {body_session} does not match bound session {session}"
                ),
            ));
        }
    }
    if let Some(project) = body.project.as_ref() {
        if project.as_str() != resolution.project_binding.project.as_str() {
            return Err(AikitError::new(
                "bootstrap.runtime_body_project_mismatch",
                format!(
                    "runtime body project {} does not match resolved Project {}",
                    project,
                    resolution.project_binding.project.as_str()
                ),
            ));
        }
    }
    if let (Some(body_agent), Some(actor)) = (body.agent.as_ref(), agent) {
        if body_agent != actor.resource() {
            return Err(identity_error("agent", actor.resource(), body_agent));
        }
    }
    if let (Some(body_agency), Some(actor)) = (body.agency.as_ref(), agency) {
        if body_agency != actor.resource() {
            return Err(identity_error("agency", actor.resource(), body_agency));
        }
    }
    Ok(())
}

fn identity_error(role: &str, resolved: &ResourceRef, body: &ResourceRef) -> AikitError {
    AikitError::new(
        "bootstrap.runtime_body_identity_mismatch",
        format!("runtime body {role} {body} does not match resolved {role} {resolved}"),
    )
    .with("role", role)
    .with("resolved", resolved.to_string())
    .with("runtime_body", body.to_string())
}
