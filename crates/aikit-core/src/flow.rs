//! Provider-neutral Flow context, praxis and explicit Living Knowledge contemplation.
//!
//! A Flow is one developing linguistic/conceptual source thread owned by a source
//! provider. AIKit does not own Flow files, revision history, AgentSession identity,
//! Claims or human Ground. It owns the situated relation by which one exact,
//! authorised Flow revision becomes standing context for an act and by which an
//! explicit `Contemplate(FlowRef)` crosses the existing Living Knowledge execution
//! aperture.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context_resolution::ContextResolution;
use crate::guidance::GuidanceFragment;
use crate::id::CapsuleId;
use crate::knowledge_living::{
    explicit_contemplate, ContemplateExecutor, ContemplateGenerated, ContemplateOutcome,
    ContemplatePreflight, ContemplateRequest, KnowledgeDependency,
};
use crate::knowledge_living_context::{
    bounded_contemplate_preflight, BoundedContemplatePreflight, DEFAULT_CONTEMPLATE_OBJECT_BUDGET,
    DEFAULT_CONTEMPLATE_RELATION_DEPTH,
};
use crate::knowledge_living_relations::KnowledgeResourceDependency;
use crate::knowledge_living_transport::parse_contemplate_generated;
use crate::method::{Method, MethodSkillRef};
use crate::praxis::PraxisResolution;
use crate::project::ProjectRef;
use crate::resource::{
    ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, SourceRef, SourceRevision,
};
use crate::{AikitError, Result};

pub const FLOW_CONTEXT_VERSION: &str = "aikit.flow-context/v1";
pub const FLOW_CONTEMPLATE_VERSION: &str = "aikit.flow-contemplate/v1";
pub const FLOW_CONTEMPLATE_RETURN_VERSION: &str = "aikit.flow-contemplate-return/v1";
pub const FLOW_GUIDANCE_CAPSULE: &str = "guidance/flow/standing-context";
pub const FLOW_SKILL_REF: &str = "cap:flow-working";
pub const FLOW_KNOWLEDGE_NAVIGATION_REF: &str = "cap:knowledge-navigation";
pub const FLOW_LIVING_KNOWLEDGE_REF: &str = "cap:living-knowledge";
pub const FLOW_CONTEMPLATE_ACTION_REF: &str = "action:contemplate-flow";
pub const FLOW_METHOD_REF: &str = "method:contemplate-flow";
pub const FLOW_METHOD_SOURCE: &str = "source:aikit:first-party:contemplate-flow";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlowLifecycle {
    Active,
    Dormant,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FlowCapabilities {
    pub read: bool,
    pub write: bool,
    pub history: bool,
}

/// Owner-provided Flow identity and current source relation. `container_hint` is
/// descriptive only; AIKit never infers Flow identity or authority from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowSourceDescriptor {
    pub flow_ref: ResourceRef,
    pub source_ref: SourceRef,
    pub revision: SourceRevision,
    pub provider: ResourceRef,
    pub lifecycle: FlowLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_hint: Option<String>,
    pub capabilities: FlowCapabilities,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowReadOutcome {
    Disclosed {
        flow: FlowSourceDescriptor,
        body: String,
    },
    Undisclosed {
        flow: FlowSourceDescriptor,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowWriteRequest {
    pub flow_ref: ResourceRef,
    pub expected_revision: SourceRevision,
    pub replacement: String,
    pub actor: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_ref: Option<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum FlowWriteResult {
    Applied { current: FlowSourceDescriptor },
    Conflict { current: FlowSourceDescriptor },
}

/// Source-owner application seam. A Central adapter is one provider; another
/// source house can expose the same semantics from any retained ordinary-file
/// container without adopting Central paths.
pub trait FlowProvider {
    fn provider_ref(&self) -> &ResourceRef;
    fn inspect(&self, flow: &ResourceRef) -> Result<FlowSourceDescriptor>;
    fn read_exact(&self, flow: &ResourceRef, revision: &SourceRevision) -> Result<FlowReadOutcome>;
    fn write(&mut self, request: &FlowWriteRequest) -> Result<FlowWriteResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowBinding {
    pub version: String,
    pub flow_ref: ResourceRef,
    pub source_ref: SourceRef,
    pub flow_revision: SourceRevision,
    pub provider: ResourceRef,
    pub project: ProjectRef,
    pub context_resolution_version: String,
    pub context_resolution_hash: String,
    pub agent_session: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum FlowStandingDisclosure {
    Disclosed { body: String, digest: String },
    Undisclosed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowStandingContext {
    pub version: String,
    pub binding: FlowBinding,
    pub lifecycle: FlowLifecycle,
    pub disclosure: FlowStandingDisclosure,
    /// Binding/inspection/read never invokes an Agent/model.
    pub automatic_agent_or_model_invocation: bool,
}

impl FlowStandingContext {
    pub fn disclosed_body(&self) -> Option<&str> {
        match &self.disclosure {
            FlowStandingDisclosure::Disclosed { body, .. } => Some(body),
            FlowStandingDisclosure::Undisclosed { .. } => None,
        }
    }
}

/// Resolve exactly one Flow as distinguished standing context for one act. The
/// existing ContextResolution remains owner of the rest of the operative field;
/// no unrelated Project source is fetched by this operation.
pub fn bind_flow_for_act(
    provider: &dyn FlowProvider,
    context: &ContextResolution,
    flow_ref: &ResourceRef,
    agent_session: ResourceRef,
    agent: Option<ResourceRef>,
    agency: Option<ResourceRef>,
) -> Result<FlowStandingContext> {
    let inspected = provider.inspect(flow_ref)?;
    validate_provider_identity(provider, &inspected)?;
    if inspected.flow_ref != *flow_ref {
        return Err(AikitError::new(
            "flow.provider_identity_mismatch",
            "Flow provider returned a different FlowRef than requested",
        ));
    }
    if inspected.lifecycle != FlowLifecycle::Active {
        return Err(AikitError::new(
            "flow.not_active",
            "only an active Flow can become standing operative context",
        )
        .with("flow", flow_ref.to_string()));
    }
    if let Some(scope) = &inspected.scope {
        let project = ResourceRef::parse(context.project_binding.project.as_str())?;
        if scope != &project {
            return Err(AikitError::new(
                "flow.scope_mismatch",
                "Flow scope does not match the ContextResolution Project",
            )
            .with("flow", flow_ref.to_string())
            .with("scope", scope.to_string())
            .with("project", project.to_string()));
        }
    }

    let binding = FlowBinding {
        version: FLOW_CONTEXT_VERSION.into(),
        flow_ref: inspected.flow_ref.clone(),
        source_ref: inspected.source_ref.clone(),
        flow_revision: inspected.revision.clone(),
        provider: inspected.provider.clone(),
        project: context.project_binding.project.clone(),
        context_resolution_version: context.version.clone(),
        context_resolution_hash: context.deterministic.hash.to_string(),
        agent_session,
        agent,
        agency,
        provenance: vec!["explicit Flow-bound act over canonical ContextResolution".into()],
    };

    let disclosure = if !inspected.capabilities.read {
        FlowStandingDisclosure::Undisclosed {
            reason: "owner exposes Flow identity but not read capability in this context".into(),
        }
    } else {
        match provider.read_exact(flow_ref, &inspected.revision)? {
            FlowReadOutcome::Undisclosed { flow, reason } => {
                validate_same_revision(&inspected, &flow)?;
                FlowStandingDisclosure::Undisclosed { reason }
            }
            FlowReadOutcome::Disclosed { flow, body } => {
                validate_same_revision(&inspected, &flow)?;
                FlowStandingDisclosure::Disclosed {
                    digest: blake3::hash(body.as_bytes()).to_hex().to_string(),
                    body,
                }
            }
        }
    };

    Ok(FlowStandingContext {
        version: FLOW_CONTEXT_VERSION.into(),
        binding,
        lifecycle: inspected.lifecycle,
        disclosure,
        automatic_agent_or_model_invocation: false,
    })
}

fn validate_provider_identity(
    provider: &dyn FlowProvider,
    flow: &FlowSourceDescriptor,
) -> Result<()> {
    if flow.provider != *provider.provider_ref() {
        return Err(AikitError::new(
            "flow.provider_identity_mismatch",
            "Flow descriptor provider does not match the provider seam used",
        )
        .with("declared_provider", flow.provider.to_string())
        .with("actual_provider", provider.provider_ref().to_string()));
    }
    Ok(())
}

fn validate_same_revision(
    expected: &FlowSourceDescriptor,
    returned: &FlowSourceDescriptor,
) -> Result<()> {
    if expected.flow_ref != returned.flow_ref
        || expected.source_ref != returned.source_ref
        || expected.revision != returned.revision
    {
        return Err(AikitError::new(
            "flow.read_revision_drift",
            "Flow changed between owner inspection and exact standing-context read",
        )
        .with("flow", expected.flow_ref.to_string())
        .with("expected_revision", expected.revision.to_string())
        .with("returned_revision", returned.revision.to_string()));
    }
    Ok(())
}

/// Attributable owner mutation intent. AIKit never applies replacement text by
/// writing the source itself; it delegates to `FlowProvider::write` once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowMutationIntent {
    pub version: String,
    pub flow_ref: ResourceRef,
    pub expected_revision: SourceRevision,
    pub replacement: String,
    pub actor: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    pub agent_session: ResourceRef,
    pub context_resolution_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_ref: Option<ResourceRef>,
}

pub fn apply_flow_mutation(
    provider: &mut dyn FlowProvider,
    standing: &FlowStandingContext,
    intent: &FlowMutationIntent,
) -> Result<FlowWriteResult> {
    if intent.flow_ref != standing.binding.flow_ref
        || intent.expected_revision != standing.binding.flow_revision
    {
        return Err(AikitError::new(
            "flow.mutation_basis_mismatch",
            "Flow mutation must use the exact FlowRef/revision disclosed to the act",
        ));
    }
    if intent.agent_session != standing.binding.agent_session {
        return Err(AikitError::new(
            "flow.mutation_session_mismatch",
            "Flow mutation attribution must retain the bound AgentSession",
        ));
    }
    if intent.context_resolution_version != standing.binding.context_resolution_version {
        return Err(AikitError::new(
            "flow.mutation_context_mismatch",
            "Flow mutation attribution must retain the bound ContextResolution",
        ));
    }
    let current = provider.inspect(&intent.flow_ref)?;
    validate_provider_identity(provider, &current)?;
    if !current.capabilities.write {
        return Err(AikitError::new(
            "flow.write_not_available",
            "Flow owner does not expose write capability in this context",
        ));
    }
    provider.write(&FlowWriteRequest {
        flow_ref: intent.flow_ref.clone(),
        expected_revision: intent.expected_revision.clone(),
        replacement: intent.replacement.clone(),
        actor: intent.actor.clone(),
        agency: intent.agency.clone(),
        agent_session: Some(intent.agent_session.clone()),
        invocation_ref: intent.invocation_ref.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlowContextAuthority {
    Flow,
    WikiReading,
    Claim,
    Ground,
    Run,
    AgentSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowAuthorityRef {
    pub authority: FlowContextAuthority,
    pub reference: ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowContemplatePreflight {
    pub version: String,
    pub invocation_ref: ResourceRef,
    pub standing: FlowStandingContext,
    pub bounded: BoundedContemplatePreflight,
    pub praxis: PraxisResolution,
    #[serde(default)]
    pub authority_refs: Vec<FlowAuthorityRef>,
    /// Preflight remains deterministic; this field changes only in explicit execution.
    pub automatic_agent_or_model_invocation: bool,
}

pub struct FlowContemplateRequest<'a> {
    pub standing: &'a FlowStandingContext,
    pub living: &'a ContemplateRequest<'a>,
    pub resource_dependencies: &'a [KnowledgeResourceDependency],
    pub praxis: &'a PraxisResolution,
    pub authority_refs: &'a [FlowAuthorityRef],
    pub object_budget: usize,
    pub relation_depth: usize,
}

impl<'a> FlowContemplateRequest<'a> {
    pub fn with_defaults(
        standing: &'a FlowStandingContext,
        living: &'a ContemplateRequest<'a>,
        resource_dependencies: &'a [KnowledgeResourceDependency],
        praxis: &'a PraxisResolution,
        authority_refs: &'a [FlowAuthorityRef],
    ) -> Self {
        Self {
            standing,
            living,
            resource_dependencies,
            praxis,
            authority_refs,
            object_budget: DEFAULT_CONTEMPLATE_OBJECT_BUDGET,
            relation_depth: DEFAULT_CONTEMPLATE_RELATION_DEPTH,
        }
    }
}

pub fn flow_contemplate_preflight(
    request: &FlowContemplateRequest<'_>,
) -> Result<FlowContemplatePreflight> {
    if request.standing.disclosed_body().is_none() {
        return Err(AikitError::new(
            "flow.contemplate_undisclosed",
            "Contemplate(FlowRef) requires the current Flow body to be authorised and disclosed",
        ));
    }
    if request.standing.binding.project != request.living.project {
        return Err(AikitError::new(
            "flow.contemplate_project_mismatch",
            "Flow standing context and Living Knowledge request identify different Projects",
        ));
    }
    if request.praxis.context_resolution_version
        != request.standing.binding.context_resolution_version
    {
        return Err(AikitError::new(
            "flow.contemplate_praxis_context_mismatch",
            "Flow contemplation praxis must be selected under the same ContextResolution",
        ));
    }
    let method = request.living.method.ok_or_else(|| {
        AikitError::new(
            "flow.contemplate_method_required",
            "Contemplate(FlowRef) requires an explicit current Method/praxis selection",
        )
    })?;
    if !request
        .praxis
        .methods
        .iter()
        .any(|selected| selected.method == method.id)
    {
        return Err(AikitError::new(
            "flow.contemplate_method_not_selected",
            "Living Knowledge Method is not selected in the current PraxisResolution",
        )
        .with("method", method.id.to_string()));
    }
    if let Some(runtime_session) = &request.living.runtime.agent_session {
        if runtime_session != request.standing.binding.agent_session.as_str() {
            return Err(AikitError::new(
                "flow.contemplate_agent_session_mismatch",
                "Flow binding and current model runtime identify different AgentSessions",
            ));
        }
    }
    let observed = request
        .living
        .horizon
        .sources
        .iter()
        .find(|source| source.source == request.standing.binding.source_ref)
        .ok_or_else(|| {
            AikitError::new(
                "flow.contemplate_source_absent",
                "current Flow source is absent from the supplied ChangeHorizon",
            )
        })?;
    if observed.revision.as_ref() != Some(&request.standing.binding.flow_revision) {
        return Err(AikitError::new(
            "flow.contemplate_revision_stale",
            "current Flow revision changed after standing context was disclosed",
        ));
    }

    let bounded = bounded_contemplate_preflight(
        request.living,
        request.resource_dependencies,
        request.object_budget,
        request.relation_depth,
    )?;
    let invocation_ref = flow_invocation_ref(
        &request.standing.binding,
        &bounded.base,
        request.praxis,
        request.authority_refs,
    )?;
    Ok(FlowContemplatePreflight {
        version: FLOW_CONTEMPLATE_VERSION.into(),
        invocation_ref,
        standing: request.standing.clone(),
        bounded,
        praxis: request.praxis.clone(),
        authority_refs: request.authority_refs.to_vec(),
        automatic_agent_or_model_invocation: false,
    })
}

fn flow_invocation_ref(
    binding: &FlowBinding,
    base: &ContemplatePreflight,
    praxis: &PraxisResolution,
    authority_refs: &[FlowAuthorityRef],
) -> Result<ResourceRef> {
    let bytes = serde_json::to_vec(&(binding, base, praxis, authority_refs)).map_err(|error| {
        AikitError::new(
            "flow.preflight_unserializable",
            format!("could not encode Flow contemplation basis: {error}"),
        )
    })?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    ResourceRef::parse(&format!("flow-contemplate/{}", &digest[..24]))
}

/// Typed output of the same single Living Knowledge execution aperture. The
/// model/Agent may return ordinary Living Knowledge changes and zero or more
/// Flow-owner mutation intents, but those intents remain unapplied owner requests.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowContemplateGenerated {
    pub living: ContemplateGenerated,
    pub flow_mutations: Vec<FlowMutationIntent>,
}

#[derive(Debug, Deserialize)]
struct FlowContemplateReturnEnvelope {
    version: String,
    living: Value,
    #[serde(default)]
    flow_mutations: Vec<FlowMutationIntent>,
}

/// Parse one transport response for explicit `Contemplate(FlowRef)` into the
/// native AIKit return. The nested `living` object is validated by the already-
/// accepted Living Knowledge transport parser; Flow mutation intents remain
/// typed, owner-directed expected-revision requests.
pub fn parse_flow_contemplate_generated(input: &str) -> Result<FlowContemplateGenerated> {
    let envelope: FlowContemplateReturnEnvelope = serde_json::from_str(input).map_err(|error| {
        AikitError::new(
            "flow.contemplate_return_invalid_json",
            format!("Flow Contemplate return must be structured JSON: {error}"),
        )
    })?;
    if envelope.version != FLOW_CONTEMPLATE_RETURN_VERSION {
        return Err(AikitError::new(
            "flow.contemplate_return_version_unsupported",
            format!(
                "Flow Contemplate return version `{}` is not `{FLOW_CONTEMPLATE_RETURN_VERSION}`",
                envelope.version
            ),
        ));
    }
    let living = serde_json::to_string(&envelope.living).map_err(|error| {
        AikitError::new(
            "flow.contemplate_return_living_unserializable",
            format!("could not recover nested Living Knowledge return: {error}"),
        )
    })?;
    Ok(FlowContemplateGenerated {
        living: parse_contemplate_generated(&living)?,
        flow_mutations: envelope.flow_mutations,
    })
}

pub trait FlowContemplateExecutor {
    fn execute(&mut self, preflight: &FlowContemplatePreflight)
        -> Result<FlowContemplateGenerated>;
}

struct FlowExecutorAdapter<'a> {
    preflight: &'a FlowContemplatePreflight,
    executor: &'a mut dyn FlowContemplateExecutor,
    flow_mutations: Option<Vec<FlowMutationIntent>>,
}

impl ContemplateExecutor for FlowExecutorAdapter<'_> {
    fn execute(&mut self, base: &ContemplatePreflight) -> Result<ContemplateGenerated> {
        if base != &self.preflight.bounded.base {
            return Err(AikitError::new(
                "flow.contemplate_preflight_drift",
                "Living Knowledge base changed between Flow preflight and explicit execution",
            ));
        }
        let generated = self.executor.execute(self.preflight)?;
        self.flow_mutations = Some(generated.flow_mutations);
        Ok(generated.living)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowContemplateOutcome {
    pub preflight: FlowContemplatePreflight,
    pub living: ContemplateOutcome,
    pub flow_mutations: Vec<FlowMutationIntent>,
}

/// Deliberate Flow contemplation. All deterministic context/praxis work happens
/// before `explicit_contemplate` calls the adapter, and the supplied executor is
/// called exactly once by that existing aperture.
pub fn explicit_flow_contemplate(
    request: &FlowContemplateRequest<'_>,
    executor: &mut dyn FlowContemplateExecutor,
) -> Result<FlowContemplateOutcome> {
    let preflight = flow_contemplate_preflight(request)?;
    let mut adapter = FlowExecutorAdapter {
        preflight: &preflight,
        executor,
        flow_mutations: None,
    };
    let living = explicit_contemplate(request.living, &mut adapter)?;
    let flow_mutations = adapter.flow_mutations.unwrap_or_default();
    for intent in &flow_mutations {
        if intent.flow_ref != preflight.standing.binding.flow_ref
            || intent.expected_revision != preflight.standing.binding.flow_revision
            || intent.agent_session != preflight.standing.binding.agent_session
            || intent.context_resolution_version
                != preflight.standing.binding.context_resolution_version
        {
            return Err(AikitError::new(
                "flow.contemplate_return_basis_mismatch",
                "Flow mutation returned by contemplation does not preserve invocation basis",
            ));
        }
        if intent.invocation_ref.as_ref() != Some(&preflight.invocation_ref) {
            return Err(AikitError::new(
                "flow.contemplate_return_invocation_mismatch",
                "Flow mutation returned by contemplation must retain its exact invocation ref",
            ));
        }
    }
    Ok(FlowContemplateOutcome {
        preflight,
        living,
        flow_mutations,
    })
}

/// Flow is a normal Living Knowledge dependency when a Wiki object/reading has
/// explicitly consumed the Flow source at an exact revision.
pub fn flow_knowledge_dependency(
    dependent: ResourceRef,
    flow: &FlowSourceDescriptor,
    relation: impl Into<String>,
    provenance_ref: Option<ResourceRef>,
    integrative: bool,
) -> KnowledgeDependency {
    KnowledgeDependency {
        dependent,
        source: flow.source_ref.clone(),
        basis_revision: Some(flow.revision.clone()),
        relation: relation.into(),
        provenance_ref,
        integrative,
    }
}

/// Positive standing guidance composed by the existing bounded Guidance system.
pub fn first_party_flow_guidance() -> Result<GuidanceFragment> {
    Ok(GuidanceFragment::new(
        CapsuleId::parse(FLOW_GUIDANCE_CAPSULE)?,
        "Language retained in an active Flow participates in later cognition. Work in the current thread as consequential shared context: preserve human authorship and actual Project meaning, continue and refine the live articulation rather than producing a parallel transcript, use exact source and provenance relations, make useful Agent contributions attributable, and replace language that no longer earns its place when its history is safely preserved by the source owner.",
    )
    .with_dedup_key("flow-standing-context")
    .with_per_fragment_budget(160))
}

/// Stable first-party resources for the reusable Flow skill, existing knowledge
/// faculties and explicit Flow contemplation action. These enter the same V2
/// ResourceIndex used by Method/ContextResolution; they are not a private Flow registry.
pub fn first_party_flow_resource_records() -> Result<Vec<ResourceRecord>> {
    let values = [
        (FLOW_SKILL_REF, ResourceKind::Capability, "Flow working", "Read, continue, refine and safely return changes to one current Flow through its owner."),
        (FLOW_KNOWLEDGE_NAVIGATION_REF, ResourceKind::Capability, "Knowledge Navigation", "Use the current Semantic Wiki, sources and relation field around the Flow."),
        (FLOW_LIVING_KNOWLEDGE_REF, ResourceKind::Capability, "Living Knowledge", "Use explicit source/revision dependencies and ChangeHorizon impact/freshness."),
        (FLOW_CONTEMPLATE_ACTION_REF, ResourceKind::Action, "Contemplate Flow", "Deliberately cross the Living Knowledge Agent/model aperture for one exact Flow revision."),
    ];
    values
        .into_iter()
        .map(|(id, kind, name, description)| {
            Ok(ResourceRecord::new(ResourceDescriptor::new(
                ResourceRef::parse(id)?,
                kind,
                name,
                description,
            )))
        })
        .collect()
}

/// Small first-party Method. Projects/personal scopes can select other Methods or
/// overlay the unchanged Flow skill; this function does not confer capability or
/// Action authority.
pub fn first_party_flow_method(revision: Option<SourceRevision>) -> Result<Method> {
    Ok(Method {
        id: ResourceRef::parse(FLOW_METHOD_REF)?,
        source: SourceRef::parse(FLOW_METHOD_SOURCE)?,
        revision,
        name: "Contemplate Flow".into(),
        description: "Situate the active Flow inside Knowledge Navigation and Living Knowledge, then return attributable Flow/Wiki/knowledge differences through their native owners.".into(),
        focus: vec![],
        project_domain: vec![],
        skills: vec![
            MethodSkillRef {
                skill: ResourceRef::parse(FLOW_SKILL_REF)?,
                usage_overlay: None,
            },
            MethodSkillRef {
                skill: ResourceRef::parse(FLOW_KNOWLEDGE_NAVIGATION_REF)?,
                usage_overlay: None,
            },
            MethodSkillRef {
                skill: ResourceRef::parse(FLOW_LIVING_KNOWLEDGE_REF)?,
                usage_overlay: None,
            },
        ],
        actions: vec![ResourceRef::parse(FLOW_CONTEMPLATE_ACTION_REF)?],
        capabilities: vec![],
        context_sources: vec![],
        verification: vec![],
        expected_resolve: None,
        expected_return_forms: vec![
            "flow-mutation-intent".into(),
            "agent-wiki-maintenance".into(),
            "integrative-reading".into(),
            "ground-proposal".into(),
            "tension".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::composition::RetractionMode;
    use crate::context::ContextDescriptor;
    use crate::context_resolution::{compose_context_resolution, RequestedActors};
    use crate::knowledge_living::{
        ContemplateGenerated, KnowledgeChangeHorizon, KnowledgeChangeKind, KnowledgeObservedSource,
        KnowledgeSourceChange,
    };
    use crate::knowledge_wiki::WikiObject;
    use crate::model_runtime::{
        AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
        ModelAccessReading, ModelMaterialisationReading, ModelRuntimeReadModel,
        ModelRuntimeRelation, ModelSurfaceReading, ModelVariantReading, PlacementObservation,
        RuntimeChangeApplication,
    };
    use crate::policy::ManagedPolicy;
    use crate::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
    use crate::projectcentral::HumanSourceRevisionProposal;
    use crate::resolve::{resolve, ResolveRequest};
    use crate::resource::{MemoryResourceIndex, ProviderRef};
    use crate::trust::AlwaysTrusted;
    use crate::MemoryCatalog;

    fn resource(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }
    fn source(value: &str) -> SourceRef {
        SourceRef::parse(value).unwrap()
    }
    fn revision(value: &str) -> SourceRevision {
        SourceRevision::parse(value).unwrap()
    }

    #[derive(Clone)]
    struct MemoryFlowProvider {
        provider: ResourceRef,
        flow: FlowSourceDescriptor,
        body: String,
        disclose: bool,
        writes: usize,
    }

    impl MemoryFlowProvider {
        fn central_style(container: &str) -> Self {
            Self {
                provider: resource("provider:central-flow"),
                flow: FlowSourceDescriptor {
                    flow_ref: resource("central:flow:project:test:thread-1"),
                    source_ref: source("central:source:project:test:notes%2Fthread.md"),
                    revision: revision("central.content-fnv1a64/v1:7:aaaaaaaaaaaaaaaa"),
                    provider: resource("provider:central-flow"),
                    lifecycle: FlowLifecycle::Active,
                    title: Some("Thread".into()),
                    scope: Some(resource("project:test")),
                    container_hint: Some(container.into()),
                    capabilities: FlowCapabilities {
                        read: true,
                        write: true,
                        history: true,
                    },
                    provenance: vec!["owner FlowRef + revision".into()],
                },
                body: "current Flow body".into(),
                disclose: true,
                writes: 0,
            }
        }

        fn non_central_style(container: &str) -> Self {
            Self {
                provider: resource("provider:notes-house-flow"),
                flow: FlowSourceDescriptor {
                    flow_ref: resource("flow:notes-house:thread-1"),
                    source_ref: source("source:notes-house:thread-1"),
                    revision: revision("notes-house-r7"),
                    provider: resource("provider:notes-house-flow"),
                    lifecycle: FlowLifecycle::Active,
                    title: Some("Thread".into()),
                    scope: Some(resource("project:test")),
                    container_hint: Some(container.into()),
                    capabilities: FlowCapabilities {
                        read: true,
                        write: true,
                        history: true,
                    },
                    provenance: vec!["mock non-Central source-house Flow capability".into()],
                },
                body: "current Flow body".into(),
                disclose: true,
                writes: 0,
            }
        }
    }

    impl FlowProvider for MemoryFlowProvider {
        fn provider_ref(&self) -> &ResourceRef {
            &self.provider
        }

        fn inspect(&self, flow: &ResourceRef) -> Result<FlowSourceDescriptor> {
            if flow != &self.flow.flow_ref {
                return Err(AikitError::new("flow.not_found", "unknown Flow"));
            }
            Ok(self.flow.clone())
        }

        fn read_exact(
            &self,
            flow: &ResourceRef,
            revision: &SourceRevision,
        ) -> Result<FlowReadOutcome> {
            if flow != &self.flow.flow_ref || revision != &self.flow.revision {
                return Err(AikitError::new("flow.revision_conflict", "stale Flow read"));
            }
            if self.disclose {
                Ok(FlowReadOutcome::Disclosed {
                    flow: self.flow.clone(),
                    body: self.body.clone(),
                })
            } else {
                Ok(FlowReadOutcome::Undisclosed {
                    flow: self.flow.clone(),
                    reason: "private in current actor context".into(),
                })
            }
        }

        fn write(&mut self, request: &FlowWriteRequest) -> Result<FlowWriteResult> {
            self.writes += 1;
            if request.expected_revision != self.flow.revision {
                return Ok(FlowWriteResult::Conflict {
                    current: self.flow.clone(),
                });
            }
            self.body = request.replacement.clone();
            self.flow.revision = revision(&format!("owner-r{}", self.writes + 1));
            Ok(FlowWriteResult::Applied {
                current: self.flow.clone(),
            })
        }
    }

    fn context() -> ContextResolution {
        let catalog = MemoryCatalog::default();
        let trust = AlwaysTrusted;
        let descriptor = ContextDescriptor::for_project("/tmp/test");
        let resolved = resolve(
            &catalog,
            &trust,
            &ResolveRequest {
                context: descriptor,
                layers: vec![],
                policy: ManagedPolicy::default(),
            },
        )
        .unwrap();
        compose_context_resolution(
            &resolved,
            ProjectBinding::new(
                ProjectRef::parse("project:test").unwrap(),
                ProjectConstituentRef::parse("constituent:test").unwrap(),
                ProjectBindingLocator::LocalDirectory {
                    path: "/tmp/test".into(),
                },
            ),
            &[],
            &MemoryResourceIndex::default(),
            RequestedActors::default(),
        )
    }

    fn runtime(session: &str) -> ModelRuntimeReadModel {
        ModelRuntimeReadModel {
            version: "aikit.model-runtime/v1".into(),
            project: Some(resource("project:test")),
            agent: Some(resource("agent:test")),
            agency: Some(resource("agency:test")),
            harness: resource("harness:test"),
            agent_session: Some(session.into()),
            harness_composition_fingerprint: "abc".into(),
            relation: ModelRuntimeRelation {
                model: ModelVariantReading {
                    model: resource("model:test"),
                    variant: "default".into(),
                },
                engine: InferenceEngineReading {
                    engine: resource("engine:test"),
                    provider: ProviderRef::parse("provider:test").unwrap(),
                    form: InferenceEngineForm::External,
                    revision: None,
                    provider_native: BTreeMap::new(),
                },
                materialisation: ModelMaterialisationReading {
                    binding_ref: "binding:test".into(),
                    workcell_ref: None,
                    placement: PlacementObservation::Local,
                    endpoint: None,
                    provider_native: BTreeMap::new(),
                    resources: MaterialResourceReading::default(),
                    lifetime_owner: "test".into(),
                    retraction: RetractionMode::Live,
                },
                model_surface: ModelSurfaceReading {
                    contract: None,
                    protocol: "test".into(),
                    capabilities: BTreeSet::new(),
                    access: ModelAccessReading {
                        inference: AccessFieldReading::available(["text"]),
                        material_control: AccessFieldReading::unavailable("not required"),
                        interior: AccessFieldReading::unavailable("not required"),
                    },
                },
                change_application: RuntimeChangeApplication::Live,
            },
            components: vec![],
            contracts: vec![],
            surfaces: vec![],
            unavailable: vec![],
        }
    }

    fn horizon(flow: &FlowSourceDescriptor) -> KnowledgeChangeHorizon {
        KnowledgeChangeHorizon {
            provider: "central.source-change-horizon/v1".into(),
            cursor: 12,
            sources: vec![KnowledgeObservedSource {
                source: flow.source_ref.clone(),
                revision: Some(flow.revision.clone()),
                available: true,
            }],
            changes: vec![KnowledgeSourceChange {
                cursor: 12,
                world_ref: "project:test".into(),
                source: flow.source_ref.clone(),
                roles: vec!["flow-source".into()],
                provenance: "collaborative-revision-provenance".into(),
                standing: "working-source".into(),
                before_revision: Some(revision("owner-r0")),
                after_revision: Some(flow.revision.clone()),
                kind: KnowledgeChangeKind::Modified,
                agent_retrieval_allowed: true,
            }],
        }
    }

    fn praxis(context: &ContextResolution, method: &Method) -> PraxisResolution {
        let mut resources = MemoryResourceIndex::default();
        for record in first_party_flow_resource_records().unwrap() {
            resources.insert(record);
        }
        crate::resolve_praxis(
            context,
            &resources,
            std::slice::from_ref(method),
            std::slice::from_ref(&method.id),
            &[],
        )
    }

    #[test]
    fn central_and_non_central_containers_use_same_flow_provider_seam() {
        let context = context();
        let central =
            MemoryFlowProvider::central_style("ProjectCentral/now/flows/2026-08-24-0100.md");
        let notes = MemoryFlowProvider::non_central_style("notes/2026-08-24-0100.md");
        let a = bind_flow_for_act(
            &central,
            &context,
            &central.flow.flow_ref,
            resource("agent-session/one"),
            None,
            None,
        )
        .unwrap();
        let b = bind_flow_for_act(
            &notes,
            &context,
            &notes.flow.flow_ref,
            resource("agent-session/two"),
            None,
            None,
        )
        .unwrap();
        assert_ne!(a.binding.flow_ref, b.binding.flow_ref);
        assert_ne!(a.binding.provider, b.binding.provider);
        assert_eq!(b.binding.provider.as_str(), "provider:notes-house-flow");
        assert_eq!(
            notes.flow.container_hint.as_deref(),
            Some("notes/2026-08-24-0100.md")
        );
        assert_eq!(a.disclosed_body(), Some("current Flow body"));
        assert_eq!(b.disclosed_body(), Some("current Flow body"));
        assert!(!a.automatic_agent_or_model_invocation);
    }

    #[test]
    fn one_flow_crosses_agent_sessions_without_becoming_session_identity() {
        let context = context();
        let provider = MemoryFlowProvider::central_style("notes/thread.md");
        let first = bind_flow_for_act(
            &provider,
            &context,
            &provider.flow.flow_ref,
            resource("agent-session/first"),
            Some(resource("agent:test")),
            Some(resource("agency:test")),
        )
        .unwrap();
        let second = bind_flow_for_act(
            &provider,
            &context,
            &provider.flow.flow_ref,
            resource("agent-session/second"),
            Some(resource("agent:test")),
            Some(resource("agency:test")),
        )
        .unwrap();
        assert_eq!(first.binding.flow_ref, second.binding.flow_ref);
        assert_eq!(first.binding.flow_revision, second.binding.flow_revision);
        assert_ne!(first.binding.agent_session, second.binding.agent_session);
    }

    #[test]
    fn unavailable_flow_remains_visible_without_leaking_body() {
        let context = context();
        let mut provider = MemoryFlowProvider::central_style("notes/private.md");
        provider.disclose = false;
        let standing = bind_flow_for_act(
            &provider,
            &context,
            &provider.flow.flow_ref,
            resource("agent-session/private"),
            None,
            None,
        )
        .unwrap();
        assert!(standing.disclosed_body().is_none());
        assert_eq!(standing.binding.flow_ref, provider.flow.flow_ref);
    }

    #[test]
    fn owner_write_is_expected_revision_once_and_stale_result_is_not_retried() {
        let context = context();
        let mut provider = MemoryFlowProvider::central_style("notes/thread.md");
        let standing = bind_flow_for_act(
            &provider,
            &context,
            &provider.flow.flow_ref,
            resource("agent-session/write"),
            Some(resource("agent:test")),
            Some(resource("agency:test")),
        )
        .unwrap();
        let intent = FlowMutationIntent {
            version: FLOW_CONTEXT_VERSION.into(),
            flow_ref: standing.binding.flow_ref.clone(),
            expected_revision: standing.binding.flow_revision.clone(),
            replacement: "refined thread".into(),
            actor: resource("agent:test"),
            agency: Some(resource("agency:test")),
            agent_session: standing.binding.agent_session.clone(),
            context_resolution_version: standing.binding.context_resolution_version.clone(),
            method: Some(resource(FLOW_METHOD_REF)),
            invocation_ref: None,
        };
        let applied = apply_flow_mutation(&mut provider, &standing, &intent).unwrap();
        assert!(matches!(applied, FlowWriteResult::Applied { .. }));
        assert_eq!(provider.writes, 1);
        let conflict = apply_flow_mutation(&mut provider, &standing, &intent).unwrap();
        assert!(matches!(conflict, FlowWriteResult::Conflict { .. }));
        assert_eq!(provider.writes, 2);
    }

    #[test]
    fn flow_change_uses_living_knowledge_impact_without_invocation() {
        let provider = MemoryFlowProvider::central_style("notes/thread.md");
        let basis = provider.flow.clone();
        let dependency = flow_knowledge_dependency(
            resource("wiki:reading:thread"),
            &basis,
            "integrates-flow",
            Some(resource("wiki:reading:thread")),
            true,
        );
        let mut current = basis.clone();
        current.revision = revision("central.content-fnv1a64/v1:9:bbbbbbbbbbbbbbbb");
        let impact =
            crate::deterministic_knowledge_impact(&horizon(&current), &[dependency]).unwrap();
        assert!(!impact.automatic_agent_or_model_invocation);
        assert_eq!(impact.changed_sources, vec![basis.source_ref.clone()]);
        assert_eq!(impact.affected.len(), 1);
        assert_eq!(
            impact.affected[0].freshness,
            crate::KnowledgeFreshness::IntegrationPending
        );
    }

    #[derive(Default)]
    struct OneCallExecutor {
        calls: usize,
    }

    impl FlowContemplateExecutor for OneCallExecutor {
        fn execute(
            &mut self,
            preflight: &FlowContemplatePreflight,
        ) -> Result<FlowContemplateGenerated> {
            self.calls += 1;
            let whole = resource("wiki:reading:flow-whole");
            let reading = crate::knowledge_wiki::WikiReading {
                profile: crate::knowledge_wiki::OKF_WIKI_PROFILE.into(),
                ref_id: whole.clone(),
                revision: 1,
                provenance: vec![crate::living_wiki_provenance(
                    preflight.standing.binding.source_ref.clone(),
                    preflight.standing.binding.flow_revision.clone(),
                )],
                frame_ref: resource("wiki:frame:flow-contemplate"),
                reading_type: "integrative-flow".into(),
                artifact_ref: None,
                derived_by_ref: Some(resource("agent:test")),
                extensions: BTreeMap::new(),
            };
            let integrated = crate::build_integrative_reading(
                reading,
                vec![crate::ReadingBasisNode {
                    resource: preflight.standing.binding.flow_ref.clone(),
                    source: Some(preflight.standing.binding.source_ref.clone()),
                    source_revision: Some(preflight.standing.binding.flow_revision.clone()),
                    roles: vec!["flow-source".into()],
                }],
                vec![],
                vec![crate::ReadingReturnPath {
                    from_basis: preflight.standing.binding.flow_ref.clone(),
                    through: vec![],
                    to_whole: whole,
                }],
                crate::KnowledgeFreshness::Fresh,
            )?;
            Ok(FlowContemplateGenerated {
                living: ContemplateGenerated {
                    wiki_upserts: vec![WikiObject::Reading(integrated.reading.clone())],
                    integrative_readings: vec![integrated],
                    candidates: vec!["candidate understanding".into()],
                    tensions: vec!["open question".into()],
                    human_source_proposals: vec![HumanSourceRevisionProposal {
                        source: source("source:human-ground:test"),
                        reason:
                            "Flow contemplation exposes a possible authored-position refinement"
                                .into(),
                        evidence: vec![preflight.standing.binding.source_ref.clone()],
                    }],
                },
                flow_mutations: vec![FlowMutationIntent {
                    version: FLOW_CONTEXT_VERSION.into(),
                    flow_ref: preflight.standing.binding.flow_ref.clone(),
                    expected_revision: preflight.standing.binding.flow_revision.clone(),
                    replacement: "refined by contemplation".into(),
                    actor: resource("agent:test"),
                    agency: Some(resource("agency:test")),
                    agent_session: preflight.standing.binding.agent_session.clone(),
                    context_resolution_version: preflight
                        .standing
                        .binding
                        .context_resolution_version
                        .clone(),
                    method: Some(resource(FLOW_METHOD_REF)),
                    invocation_ref: Some(preflight.invocation_ref.clone()),
                }],
            })
        }
    }

    #[test]
    fn contemplate_flow_uses_bounded_living_context_current_praxis_and_exactly_one_invocation() {
        let context = context();
        let provider = MemoryFlowProvider::central_style("notes/thread.md");
        let session = resource("agent-session/contemplate");
        let standing = bind_flow_for_act(
            &provider,
            &context,
            &provider.flow.flow_ref,
            session.clone(),
            Some(resource("agent:test")),
            Some(resource("agency:test")),
        )
        .unwrap();
        let horizon = horizon(&provider.flow);
        let method = first_party_flow_method(Some(revision("method-r1"))).unwrap();
        let praxis = praxis(&context, &method);
        assert!(praxis.warnings.is_empty());
        let runtime = runtime(session.as_str());
        let objects = Vec::<WikiObject>::new();
        let dependencies = Vec::<KnowledgeDependency>::new();
        let living = ContemplateRequest {
            project: ProjectRef::parse("project:test").unwrap(),
            focus: vec![],
            horizon: &horizon,
            dependencies: &dependencies,
            current_wiki_objects: &objects,
            runtime: &runtime,
            method: Some(&method),
            ql: None,
        };
        let authority_refs = vec![
            FlowAuthorityRef {
                authority: FlowContextAuthority::Flow,
                reference: standing.binding.flow_ref.clone(),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::WikiReading,
                reference: resource("wiki:reading:prior-flow"),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::Claim,
                reference: resource("claim:external:test"),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::Ground,
                reference: resource("ground:human:test"),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::Run,
                reference: resource("run:external:test"),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::AgentSession,
                reference: session,
            },
        ];
        let request = FlowContemplateRequest::with_defaults(
            &standing,
            &living,
            &[],
            &praxis,
            &authority_refs,
        );
        let preflight = flow_contemplate_preflight(&request).unwrap();
        assert!(!preflight.automatic_agent_or_model_invocation);
        assert_eq!(preflight.praxis.methods[0].method, method.id);
        assert_eq!(
            preflight.standing.disclosed_body(),
            Some("current Flow body")
        );
        assert!(preflight
            .authority_refs
            .iter()
            .any(|entry| entry.authority == FlowContextAuthority::Claim));

        let mut executor = OneCallExecutor::default();
        let outcome = explicit_flow_contemplate(&request, &mut executor).unwrap();
        assert_eq!(executor.calls, 1);
        assert_eq!(outcome.flow_mutations.len(), 1);
        assert_eq!(
            outcome.flow_mutations[0].expected_revision,
            provider.flow.revision
        );
        assert_eq!(outcome.living.candidates, vec!["candidate understanding"]);
        assert_eq!(outcome.living.integrative_readings.len(), 1);
        assert_eq!(
            outcome.living.integrative_readings[0].reading.reading_type,
            "integrative-flow"
        );
        assert!(outcome
            .living
            .agent_wiki
            .next_objects
            .iter()
            .any(|object| matches!(object, WikiObject::Reading(reading) if reading.ref_id.as_str() == "wiki:reading:flow-whole")));
        assert_eq!(outcome.living.agent_wiki.human_source_proposals.len(), 1);
        assert_eq!(
            outcome.living.agent_wiki.human_source_proposals[0]
                .source
                .as_str(),
            "source:human-ground:test"
        );
        assert!(outcome.preflight.authority_refs.iter().any(|entry| {
            entry.authority == FlowContextAuthority::Claim
                && entry.reference.as_str() == "claim:external:test"
        }));
        assert!(outcome.preflight.authority_refs.iter().any(|entry| {
            entry.authority == FlowContextAuthority::Run
                && entry.reference.as_str() == "run:external:test"
        }));
    }

    #[test]
    fn flow_transport_keeps_living_validation_and_owner_mutation_intent_typed() {
        let parsed = parse_flow_contemplate_generated(
            r#"{
              "version":"aikit.flow-contemplate-return/v1",
              "living":{
                "version":"aikit.contemplate-return/v1",
                "wiki_upserts":[],
                "integrative_readings":[],
                "candidates":["candidate:flow"],
                "tensions":[],
                "human_source_proposals":[]
              },
              "flow_mutations":[{
                "version":"aikit.flow-context/v1",
                "flow_ref":"flow:notes-house:thread-1",
                "expected_revision":"notes-house-r7",
                "replacement":"refined source",
                "actor":"agent:test",
                "agency":"agency:test",
                "agent_session":"agent-session/test",
                "context_resolution_version":"aikit.context-resolution/v2",
                "method":"method:contemplate-flow",
                "invocation_ref":"flow-contemplate/abc"
              }]
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.living.candidates, vec!["candidate:flow"]);
        assert_eq!(parsed.flow_mutations.len(), 1);
        assert_eq!(
            parsed.flow_mutations[0].flow_ref.as_str(),
            "flow:notes-house:thread-1"
        );
        assert_eq!(
            parsed.flow_mutations[0].expected_revision.as_str(),
            "notes-house-r7"
        );

        let prose = parse_flow_contemplate_generated("please edit the Flow").unwrap_err();
        assert_eq!(prose.code(), "flow.contemplate_return_invalid_json");
        let bad_nested = parse_flow_contemplate_generated(
            r#"{"version":"aikit.flow-contemplate-return/v1","living":{"version":"wrong"}}"#,
        )
        .unwrap_err();
        assert_eq!(
            bad_nested.code(),
            "knowledge.contemplate_return_version_unsupported"
        );
    }

    #[test]
    fn first_party_flow_praxis_uses_guidance_skill_method_and_explainable_resolution() {
        let context = context();
        let guidance = first_party_flow_guidance().unwrap();
        assert!(guidance.body.contains("active Flow"));
        let method = first_party_flow_method(Some(revision("method-r1"))).unwrap();
        let praxis = praxis(&context, &method);
        assert!(praxis.warnings.is_empty());
        assert_eq!(praxis.methods.len(), 1);
        assert_eq!(praxis.methods[0].resolution.skills.len(), 3);
        let evidence = crate::praxis::explain_praxis(&praxis);
        assert_eq!(evidence.len(), 1);
        assert!(evidence[0]
            .facts
            .iter()
            .any(|fact| fact.relation == "method-source"));
    }
}
