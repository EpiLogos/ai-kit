//! Thin actor/harness bootstrap projection for V2.
//!
//! A bootstrap is an orientation seed, not a second ContextResolution and not a
//! serialized HarnessComposition. It preserves actor/project identity and enough
//! provenance to explain the binding, while summarising large horizons and giving
//! composition-capable harnesses only an inspectable body pointer.

use serde::{Deserialize, Serialize};

use crate::composition::{CompositionState, HarnessComposition};
use crate::context_resolution::{
    Availability, ContextResolution, HarnessDetectionGround, ReferenceResolution, ResolvedResource,
    ScopeResolution,
};
use crate::platform::TargetId;
use crate::project::ProjectBinding;
use crate::resource::{
    AddressHorizon, ProviderOffer, RelationOp, ResolveExpression, ResourceKind, ResourceRef,
    ResourceSource,
};
use crate::session_space::SessionSpaceRef;
use crate::{AikitError, Result};

pub const ACTOR_BOOTSTRAP_VERSION: &str = "aikit.actor-bootstrap/v2";
pub const BOOTSTRAP_RESOURCE_SAMPLE_LIMIT: usize = 12;

/// Why a selected reference resolved to nothing. Detection ground turns a
/// bare "missing" into a reasoned one: under the three-state law,
/// not-installed, could-not-prove, and never-looked are different facts
/// and must not read as each other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cause", rename_all = "kebab-case")]
pub enum MissingCause {
    /// Detection ran and records the referenced harness not installed here.
    NotInstalled { detection_ref: String },
    /// Detection could not prove presence or absence — the run failed, or
    /// this specific harness was unprovable.
    DetectionUnavailable { reason: String },
    /// Detection records the harness present, yet no candidate resolved —
    /// an intake or composition gap, disclosed as such.
    DetectedButUnresolved { detection_ref: String },
    /// The reference names nothing in the detection catalog.
    UnknownToDetection { detection_ref: String },
    /// No detection ground rode on this resolution; presence unproven.
    #[default]
    Unproven,
}

impl MissingCause {
    pub fn explanation(&self) -> String {
        match self {
            Self::NotInstalled { detection_ref } => {
                format!("not installed on this machine per {detection_ref}")
            }
            Self::DetectionUnavailable { reason } => {
                format!("presence unproven: {reason}")
            }
            Self::DetectedButUnresolved { detection_ref } => {
                format!("detected in {detection_ref} but no candidate resolved — intake gap")
            }
            Self::UnknownToDetection { detection_ref } => {
                format!("not in the detection catalog ({detection_ref})")
            }
            Self::Unproven => "no detection ground available; presence unproven".to_string(),
        }
    }
}

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
        #[serde(default)]
        cause: MissingCause,
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
    /// Harnesses detected on this machine, whether or not an authored source
    /// selected one. Detection-first: the full candidate set is disclosed so an
    /// owner (or a later model-bearing receipt) can narrow it; selection stays
    /// with authored sources, never with detection.
    #[serde(default)]
    pub harness_candidates: Vec<ResourceRef>,
    /// Models detected as eligible, same contract as `harness_candidates`.
    #[serde(default)]
    pub model_candidates: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    /// The canonical World identity this actor inhabits (`session-space/…`).
    /// This is a stable reference, not the World's contents; richer World state
    /// (recognition account, material capacities) stays resolvable on demand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_space: Option<SessionSpaceRef>,
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
    /// The World/SessionSpace identity to disclose. Supplied by the caller that
    /// owns the World relation (e.g. the O:I launcher), never inferred here.
    pub session_space: Option<SessionSpaceRef>,
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
            resolution.harness_detection.as_ref(),
        )
    });
    let model = request.selected_model.as_ref().map(|selected| {
        summarize_selected(selected, ResourceKind::Model, &resolution.model_candidates, None)
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
        harness_candidates: resolution
            .harness_candidates
            .iter()
            .map(|resource| resource.resource.descriptor.id.clone())
            .collect(),
        model_candidates: resolution
            .model_candidates
            .iter()
            .map(|resource| resource.resource.descriptor.id.clone())
            .collect(),
        agent_session: request.agent_session,
        session_space: request.session_space,
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
            cause: MissingCause::Unproven,
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
    detection: Option<&HarnessDetectionGround>,
) -> BootstrapReference {
    candidates
        .iter()
        .find(|candidate| candidate.resource.descriptor.id == *selected)
        .map(summarize_resolved)
        .unwrap_or_else(|| BootstrapReference::Missing {
            reference: selected.clone(),
            expected,
            cause: missing_cause(selected, detection),
        })
}

/// Reason a selected reference that matches no candidate. Only harness
/// references consult Actuation's detection ground; every other role —
/// and every case where no ground rode on the resolution — stays honestly
/// unproven rather than borrowing a harness fact it does not have.
fn missing_cause(
    selected: &ResourceRef,
    detection: Option<&HarnessDetectionGround>,
) -> MissingCause {
    let Some(ground) = detection else {
        return MissingCause::Unproven;
    };
    let Some(slug) = selected.as_str().strip_prefix("harness/") else {
        return MissingCause::Unproven;
    };
    match ground {
        HarnessDetectionGround::Unavailable { reason } => {
            MissingCause::DetectionUnavailable { reason: reason.clone() }
        }
        HarnessDetectionGround::Observed {
            detection_ref,
            states,
            reasons,
            ..
        } => match states.get(slug).map(String::as_str) {
            Some("detected") => MissingCause::DetectedButUnresolved {
                detection_ref: detection_ref.clone(),
            },
            Some("unavailable") => MissingCause::DetectionUnavailable {
                reason: reasons
                    .get(slug)
                    .cloned()
                    .unwrap_or_else(|| format!("could not prove presence or absence of {slug}")),
            },
            Some("not-installed") => MissingCause::NotInstalled {
                detection_ref: detection_ref.clone(),
            },
            _ => MissingCause::UnknownToDetection {
                detection_ref: detection_ref.clone(),
            },
        },
    }
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

/// Express the actual actor bootstrap as the shared six-horizon O:I disclosure.
pub fn actor_world_disclosure(bootstrap: &ActorBootstrap) -> ResolveExpression {
    fn actual(reference: &Option<BootstrapReference>) -> Option<ResourceRef> {
        match reference {
            Some(BootstrapReference::Resolved { resource, .. }) => Some(resource.clone()),
            _ => None,
        }
    }

    fn clause(horizon: AddressHorizon, resource: ResourceRef) -> ResolveExpression {
        ResolveExpression::Unary {
            op: RelationOp::Affirm,
            expression: Box::new(ResolveExpression::horizon(
                horizon,
                ResolveExpression::subject(resource.to_string()),
            )),
        }
    }

    let mut clauses = Vec::new();
    clauses.extend(
        bootstrap
            .context_sources
            .examples
            .iter()
            .cloned()
            .map(|resource| clause(AddressHorizon::H0, resource)),
    );
    clauses.extend(
        [
            actual(&bootstrap.host),
            actual(&bootstrap.harness),
            actual(&bootstrap.model),
        ]
        .into_iter()
        .flatten()
        .map(|resource| clause(AddressHorizon::H1, resource)),
    );
    clauses.extend(
        [actual(&bootstrap.agent), actual(&bootstrap.agency)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H2, resource)),
    );
    clauses.extend(
        [actual(&bootstrap.harness), actual(&bootstrap.model)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H3, resource)),
    );
    if let Ok(project) = ResourceRef::parse(bootstrap.project.project.as_str()) {
        clauses.push(clause(AddressHorizon::H4, project));
    }
    if let Some(run) = bootstrap.run.clone() {
        clauses.push(clause(AddressHorizon::H4, run));
    }
    if let Some(space) = bootstrap.session_space.clone() {
        clauses.push(clause(AddressHorizon::H4, space.as_resource_ref().clone()));
    }
    clauses.extend(
        [actual(&bootstrap.agent), actual(&bootstrap.host)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H4, resource)),
    );
    clauses.extend(
        bootstrap
            .capabilities
            .examples
            .iter()
            .chain(&bootstrap.actions.examples)
            .cloned()
            .map(|resource| clause(AddressHorizon::H5, resource)),
    );

    let expression = clauses
        .into_iter()
        .reduce(|left, right| ResolveExpression::Binary {
            op: RelationOp::Contextualise,
            left: Box::new(left),
            right: Box::new(right),
        })
        .unwrap_or_else(|| ResolveExpression::ordinary_search(""));
    ResolveExpression::Frame {
        expression: Box::new(expression),
    }
}
