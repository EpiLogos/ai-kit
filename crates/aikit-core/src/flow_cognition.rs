//! W1.4/W1.5 owner-side Flow cognition: the typed `Contemplate(FlowRef)` reading
//! and the "what changed relative to this thought" owner read.
//!
//! This module adds no new execution aperture. A successful cognition reading is
//! produced only from the existing [`explicit_flow_contemplate`] outcome, and the
//! only path to that outcome runs the deterministic Flow preflight first and
//! validates the exact preflight record before the executor is called. Explicit
//! `unavailable`/`refused` states are first-class: no owner seam, no executor or
//! a drifted/forged record is reported as state, never papered over with a guess.

use serde::{Deserialize, Serialize};

use crate::explain_history::{
    EvidenceProvenance, ExplainEvidence, ExplainFact, EXPLAIN_HISTORY_VERSION,
};
use crate::flow::{
    explicit_flow_contemplate, FlowContemplateOutcome, FlowContemplatePreflight,
    FlowMutationIntent, FLOW_CONTEMPLATE_VERSION,
};
use crate::knowledge_living::{
    deterministic_knowledge_impact, KnowledgeAffectedResource, KnowledgeChangeHorizon,
    KnowledgeChangeKind, KnowledgeDependency, KnowledgeImpact,
};
use crate::projectcentral::HumanSourceRevisionProposal;
use crate::resource::{ResourceRef, SourceAuthority, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const FLOW_COGNITION_VERSION: &str = "aikit.flow-cognition/v1";
pub const FLOW_CHANGED_SINCE_VERSION: &str = "aikit.flow-changed-since/v1";
/// Familiarity law (C2 precedent): one successful Contemplate records exactly
/// one `familiarity/resource-use` observation, provable via log export replay.
pub const FLOW_CONTEMPLATE_USE_RECORDED: &str = "familiarity/resource-use";

/// Durable record of one explicit Contemplate(FlowRef): the "thought". It is
/// the basis for the W1.5 changed-since read and carries exactly the owner
/// seams the thought was executed under — never inferred state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowThoughtRecord {
    pub version: String,
    /// The deterministic invocation ref (`flow-contemplate/<digest>`) of the
    /// preflight this thought was executed under.
    pub invocation_ref: ResourceRef,
    pub flow_ref: ResourceRef,
    pub source_ref: SourceRef,
    pub basis_revision: SourceRevision,
    /// Owner change-horizon cursor at contemplation; the changed-since read
    /// reports change rows strictly above this cursor.
    pub horizon_cursor: u64,
    /// Present only after a successful explicit execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<FlowThoughtOutcome>,
}

/// Owner-returned material of one successful thought. Flow mutation intents
/// remain unapplied owner requests; human source proposals remain proposal-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowThoughtOutcome {
    #[serde(default)]
    pub candidates: Vec<String>,
    #[serde(default)]
    pub tensions: Vec<String>,
    #[serde(default)]
    pub human_source_proposals: Vec<HumanSourceRevisionProposal>,
    #[serde(default)]
    pub flow_mutations: Vec<FlowMutationIntent>,
}

/// Typed cognition reading for one explicit `Contemplate(FlowRef)` (schema
/// `aikit.flow-cognition/v1`). `cognition` records the one deliberate
/// Agent/model crossing; `unavailable` and `refused` are explicit terminal
/// states that carry a reason and never fake a reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum FlowCognition {
    Cognition {
        version: String,
        thought: Box<FlowThoughtRecord>,
        impact: KnowledgeImpact,
        /// `true` here and only here: this state records the single explicit
        /// Agent/model crossing performed by the host executor.
        automatic_agent_or_model_invocation: bool,
    },
    Unavailable {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation_ref: Option<ResourceRef>,
        flow_ref: ResourceRef,
        reason: String,
    },
    Refused {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation_ref: Option<ResourceRef>,
        flow_ref: ResourceRef,
        reason: String,
    },
}

impl FlowCognition {
    pub fn flow_ref(&self) -> &ResourceRef {
        match self {
            FlowCognition::Cognition { thought, .. } => &thought.flow_ref,
            FlowCognition::Unavailable { flow_ref, .. } => flow_ref,
            FlowCognition::Refused { flow_ref, .. } => flow_ref,
        }
    }

    pub fn invocation_ref(&self) -> Option<&ResourceRef> {
        match self {
            FlowCognition::Cognition { thought, .. } => Some(&thought.invocation_ref),
            FlowCognition::Unavailable { invocation_ref, .. } => invocation_ref.as_ref(),
            FlowCognition::Refused { invocation_ref, .. } => invocation_ref.as_ref(),
        }
    }

    pub fn thought(&self) -> Option<&FlowThoughtRecord> {
        match self {
            FlowCognition::Cognition { thought, .. } => Some(thought),
            _ => None,
        }
    }
}

/// Structurally validate one preflight record before it can gate execution.
/// A record that did not come out of [`crate::flow_contemplate_preflight`]
/// cannot pass: the invocation digest is derived inside that function and is
/// not reconstructable from the disclosed fields alone.
pub fn validate_flow_contemplate_record(record: &FlowContemplatePreflight) -> Result<()> {
    if record.version != FLOW_CONTEMPLATE_VERSION {
        return Err(AikitError::new(
            "flow.cognition_preflight_version",
            format!(
                "Flow Contemplate preflight record version `{}` is not `{FLOW_CONTEMPLATE_VERSION}`",
                record.version
            ),
        ));
    }
    if record.standing.disclosed_body().is_none() {
        return Err(AikitError::new(
            "flow.cognition_preflight_undisclosed",
            "a Flow Contemplate preflight record requires an authorised disclosed Flow body",
        ));
    }
    if record.automatic_agent_or_model_invocation {
        return Err(AikitError::new(
            "flow.cognition_preflight_invocation_invariant",
            "a deterministic preflight record must not claim Agent/model invocation",
        ));
    }
    if !record
        .invocation_ref
        .as_str()
        .starts_with("flow-contemplate/")
    {
        return Err(AikitError::new(
            "flow.cognition_preflight_invocation_ref",
            format!(
                "Flow Contemplate invocation ref `{}` is not a flow-contemplate digest ref",
                record.invocation_ref
            ),
        ));
    }
    Ok(())
}

/// Compose the typed cognition reading from one successful explicit Flow
/// contemplation outcome. The thought record retains the exact invocation
/// basis so the W1.5 read can be asked against it later.
pub fn flow_cognition_from_outcome(outcome: &FlowContemplateOutcome) -> FlowCognition {
    let binding = &outcome.preflight.standing.binding;
    let thought = FlowThoughtRecord {
        version: FLOW_COGNITION_VERSION.into(),
        invocation_ref: outcome.preflight.invocation_ref.clone(),
        flow_ref: binding.flow_ref.clone(),
        source_ref: binding.source_ref.clone(),
        basis_revision: binding.flow_revision.clone(),
        horizon_cursor: outcome.preflight.bounded.base.impact.horizon_cursor,
        outcome: Some(FlowThoughtOutcome {
            candidates: outcome.living.candidates.clone(),
            tensions: outcome.living.tensions.clone(),
            human_source_proposals: outcome.living.agent_wiki.human_source_proposals.clone(),
            flow_mutations: outcome.flow_mutations.clone(),
        }),
    };
    FlowCognition::Cognition {
        version: FLOW_COGNITION_VERSION.into(),
        impact: outcome.preflight.bounded.base.impact.clone(),
        automatic_agent_or_model_invocation: true,
        thought: thought.into(),
    }
}

/// Explain disclosure for one Flow Contemplate preflight record, in the same
/// shape as [`crate::praxis::explain_praxis`]: one `ExplainEvidence` whose
/// facts name exactly what the operation will read and touch before it runs.
pub fn explain_flow_contemplate_preflight(
    preflight: &FlowContemplatePreflight,
) -> Vec<ExplainEvidence> {
    let binding = &preflight.standing.binding;
    let impact = &preflight.bounded.base.impact;
    let digest = match &preflight.standing.disclosure {
        crate::flow::FlowStandingDisclosure::Disclosed { digest, .. } => digest.clone(),
        crate::flow::FlowStandingDisclosure::Undisclosed { reason } => {
            format!("undisclosed: {reason}")
        }
    };
    let source_resource = ResourceRef::parse(binding.source_ref.as_str()).ok();
    let facts = vec![
        ExplainFact {
            relation: "flow-source".into(),
            authority: Some(SourceAuthority::Authored),
            summary: format!(
                "Contemplate reads Flow {} source {} at exact revision {}",
                binding.flow_ref, binding.source_ref, binding.flow_revision
            ),
            canonical_refs: [binding.flow_ref.clone()].to_vec(),
            provenance: vec![EvidenceProvenance {
                source: source_resource.clone(),
                revision: Some(binding.flow_revision.to_string()),
                ..EvidenceProvenance::default()
            }],
        },
        ExplainFact {
            relation: "standing-context-read".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!("exact-revision Flow body disclosed under standing context (digest {digest})"),
            canonical_refs: Vec::new(),
            provenance: vec![EvidenceProvenance {
                source: source_resource,
                revision: Some(binding.flow_revision.to_string()),
                ..EvidenceProvenance::default()
            }],
        },
        ExplainFact {
            relation: "knowledge-impact".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "{} knowledge resource(s) affected at owner horizon cursor {}",
                impact.affected.len(),
                impact.horizon_cursor
            ),
            canonical_refs: impact
                .affected
                .iter()
                .map(|row| row.resource.clone())
                .collect(),
            provenance: Vec::new(),
        },
        ExplainFact {
            relation: "method-praxis".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "executes under ContextResolution {} with the explicitly selected Contemplate Flow Method",
                binding.context_resolution_version
            ),
            canonical_refs: impact
                .changed_sources
                .iter()
                .filter_map(|source| ResourceRef::parse(source.as_str()).ok())
                .collect(),
            provenance: Vec::new(),
        },
        ExplainFact {
            relation: "preflight-record".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "re-execution must present this exact preflight record ({}); a recomputed preflight that differs is refused",
                preflight.invocation_ref
            ),
            canonical_refs: vec![preflight.invocation_ref.clone()],
            provenance: Vec::new(),
        },
        ExplainFact {
            relation: "execution-invariant".into(),
            authority: Some(SourceAuthority::Derived),
            summary:
                "preflight is deterministic and crosses no Agent/model seam; execution happens only through an explicitly supplied host executor and is never auto-invoked"
                    .into(),
            canonical_refs: Vec::new(),
            provenance: Vec::new(),
        },
    ];
    vec![ExplainEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        subject: preflight.invocation_ref.clone(),
        facts,
    }]
}

// ---------------------------------------------------------------------------
// W1.5 changed-since-thought owner read
// ---------------------------------------------------------------------------

/// One source observed to have changed relative to the thought, with the
/// provenance of that observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThoughtChangedSource {
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis_revision: Option<SourceRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_revision: Option<SourceRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<KnowledgeChangeKind>,
    pub provenance: String,
    pub available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThoughtUnresolvedKind {
    Tension,
    Candidate,
    HumanSourceProposal,
    FlowMutationIntent,
}

/// One owner-returned item of the thought that remains open at the owner
/// seams. `unresolved` means no owner seam shows resolution; `basis-superseded`
/// means the exact expected Flow revision was overtaken at the owner (applied
/// or otherwise advanced) so the returned request can no longer stand as-is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThoughtUnresolved {
    pub kind: ThoughtUnresolvedKind,
    pub reference: String,
    #[serde(default)]
    pub provenance: Vec<String>,
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlowChangedSinceState {
    Available,
    Empty,
    Unavailable,
}

/// Typed "what changed relative to this thought" reading (schema
/// `aikit.flow-changed-since/v1`). Every row carries provenance; `empty` and
/// `unavailable` are explicit states and are never faked into rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowChangedSince {
    pub version: String,
    pub flow_ref: ResourceRef,
    pub thought_ref: ResourceRef,
    pub state: FlowChangedSinceState,
    #[serde(default)]
    pub changed_sources: Vec<ThoughtChangedSource>,
    #[serde(default)]
    pub affected_knowledge: Vec<KnowledgeAffectedResource>,
    #[serde(default)]
    pub unresolved: Vec<ThoughtUnresolved>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// This deterministic owner read never invokes an Agent or model.
    pub automatic_agent_or_model_invocation: bool,
}

/// Deterministic owner read: what changed relative to one recorded thought.
///
/// * `changed_sources` — owner change rows with cursor strictly above the
///   thought's horizon cursor, plus an owner-inspection drift row when the
///   Flow revision advanced without a change row.
/// * `affected_knowledge` — the deterministic Living Knowledge impact over the
///   supplied dependencies at the current horizon (non-fresh rows only, each
///   carrying its provenance ref and basis/observed revisions).
/// * `unresolved` — the thought's returned tensions, candidates, human-source
///   proposals and Flow mutation intents that remain open at the owner seams.
///
/// `current_flow: None` means the Flow no longer inspects at its owner; the
/// reading is then `unavailable` with empty rows rather than a guess.
pub fn changed_since_thought(
    thought: &FlowThoughtRecord,
    current_horizon: &KnowledgeChangeHorizon,
    dependencies: &[KnowledgeDependency],
    current_flow: Option<&crate::flow::FlowSourceDescriptor>,
) -> Result<FlowChangedSince> {
    if let Some(flow) = current_flow {
        if flow.flow_ref != thought.flow_ref {
            return Err(AikitError::new(
                "flow.changed_since_flow_mismatch",
                "the inspected Flow does not match the thought's FlowRef",
            )
            .with("thought_flow", thought.flow_ref.to_string())
            .with("inspected_flow", flow.flow_ref.to_string()));
        }
    }

    let mut changed_sources = Vec::new();
    for change in &current_horizon.changes {
        if change.cursor <= thought.horizon_cursor {
            continue;
        }
        let available = current_horizon
            .sources
            .iter()
            .find(|observed| observed.source == change.source)
            .map(|observed| observed.available)
            .unwrap_or(false);
        changed_sources.push(ThoughtChangedSource {
            source: change.source.clone(),
            basis_revision: change.before_revision.clone(),
            observed_revision: change.after_revision.clone().or_else(|| {
                current_horizon
                    .sources
                    .iter()
                    .find(|observed| observed.source == change.source)
                    .and_then(|observed| observed.revision.clone())
            }),
            kind: Some(change.kind),
            provenance: change.provenance.clone(),
            available,
        });
    }
    if let Some(flow) = current_flow {
        let covered = changed_sources
            .iter()
            .any(|row| row.source == flow.source_ref);
        if !covered && flow.revision != thought.basis_revision {
            changed_sources.push(ThoughtChangedSource {
                source: flow.source_ref.clone(),
                basis_revision: Some(thought.basis_revision.clone()),
                observed_revision: Some(flow.revision.clone()),
                kind: None,
                provenance:
                    "owner-inspect: Flow revision advanced beyond the thought basis without a horizon change row"
                        .into(),
                available: true,
            });
        }
    }
    changed_sources.sort_by(|left, right| left.source.cmp(&right.source));
    changed_sources.dedup_by(|next, prev| {
        if next.source == prev.source {
            prev.observed_revision = next
                .observed_revision
                .clone()
                .or(prev.observed_revision.clone());
            true
        } else {
            false
        }
    });

    let impact = deterministic_knowledge_impact(current_horizon, dependencies)?;
    let affected_knowledge = impact.affected;

    let mut unresolved = Vec::new();
    if let Some(outcome) = &thought.outcome {
        let invocation = thought.invocation_ref.to_string();
        for tension in &outcome.tensions {
            unresolved.push(ThoughtUnresolved {
                kind: ThoughtUnresolvedKind::Tension,
                reference: tension.clone(),
                provenance: vec![invocation.clone()],
                state: "unresolved".into(),
            });
        }
        for candidate in &outcome.candidates {
            unresolved.push(ThoughtUnresolved {
                kind: ThoughtUnresolvedKind::Candidate,
                reference: candidate.clone(),
                provenance: vec![invocation.clone()],
                state: "unresolved".into(),
            });
        }
        for proposal in &outcome.human_source_proposals {
            unresolved.push(ThoughtUnresolved {
                kind: ThoughtUnresolvedKind::HumanSourceProposal,
                reference: proposal.source.to_string(),
                provenance: vec![invocation.clone(), format!("reason: {}", proposal.reason)],
                state: "unresolved".into(),
            });
        }
        for mutation in &outcome.flow_mutations {
            let superseded = current_flow
                .map(|flow| flow.revision != mutation.expected_revision)
                .unwrap_or(false);
            unresolved.push(ThoughtUnresolved {
                kind: ThoughtUnresolvedKind::FlowMutationIntent,
                reference: format!("{}@{}", mutation.flow_ref, mutation.expected_revision),
                provenance: vec![invocation.clone()],
                state: if superseded {
                    "basis-superseded".into()
                } else {
                    "unresolved".into()
                },
            });
        }
    }

    let (state, unavailable_reason) = match current_flow {
        None => (
            FlowChangedSinceState::Unavailable,
            Some(
                "the Flow no longer inspects at its owner; changed-since rows would be guesses"
                    .into(),
            ),
        ),
        Some(_)
            if changed_sources.is_empty()
                && affected_knowledge.is_empty()
                && unresolved.is_empty() =>
        {
            (FlowChangedSinceState::Empty, None)
        }
        Some(_) => (FlowChangedSinceState::Available, None),
    };

    Ok(FlowChangedSince {
        version: FLOW_CHANGED_SINCE_VERSION.into(),
        flow_ref: thought.flow_ref.clone(),
        thought_ref: thought.invocation_ref.clone(),
        state,
        changed_sources,
        affected_knowledge,
        unresolved,
        unavailable_reason,
        automatic_agent_or_model_invocation: false,
    })
}

/// Convenience: explicit Contemplate execution with record validation. The
/// supplied preflight record is structurally validated and the deterministic
/// preflight is recomputed over the same request; any mismatch refuses
/// execution before the executor is called. This is the core gate that makes
/// it impossible to cross the aperture without a genuine preflight record.
pub fn explicit_flow_contemplate_validated(
    request: &crate::flow::FlowContemplateRequest<'_>,
    record: &FlowContemplatePreflight,
    executor: &mut dyn crate::flow::FlowContemplateExecutor,
) -> Result<FlowCognition> {
    validate_flow_contemplate_record(record)?;
    let recomputed = crate::flow::flow_contemplate_preflight(request)?;
    if recomputed != *record {
        return Err(AikitError::new(
            "flow.cognition_preflight_drift",
            "the supplied preflight record does not match a freshly computed preflight over the same basis",
        )
        .with("record", record.invocation_ref.to_string())
        .with("recomputed", recomputed.invocation_ref.to_string()));
    }
    let outcome = explicit_flow_contemplate(request, executor)?;
    Ok(flow_cognition_from_outcome(&outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::resource::SourceAuthority;
    use crate::{AikitError, Result};

    use crate::composition::RetractionMode;
    use crate::context::ContextDescriptor;
    use crate::context_resolution::{compose_context_resolution, RequestedActors};
    use crate::flow::{
        bind_flow_for_act, first_party_flow_method, first_party_flow_resource_records,
        flow_contemplate_preflight, FlowCapabilities, FlowContemplateRequest, FlowLifecycle,
        FlowProvider, FlowReadOutcome, FlowSourceDescriptor, FlowWriteRequest, FlowWriteResult,
        FLOW_CONTEMPLATE_VERSION, FLOW_CONTEXT_VERSION, FLOW_METHOD_REF,
    };
    use crate::knowledge_living::{
        ContemplateRequest, KnowledgeChangeHorizon, KnowledgeChangeKind, KnowledgeObservedSource,
        KnowledgeSourceChange,
    };
    use crate::method::Method;
    use crate::model_runtime::{
        AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
        ModelAccessReading, ModelMaterialisationReading, ModelRuntimeReadModel,
        ModelRuntimeRelation, ModelSurfaceReading, ModelVariantReading, PlacementObservation,
        RuntimeChangeApplication,
    };
    use crate::policy::ManagedPolicy;
    use crate::project::ProjectRef;
    use crate::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
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

    struct FixtureProvider {
        flow: FlowSourceDescriptor,
        body: String,
    }

    impl FlowProvider for FixtureProvider {
        fn provider_ref(&self) -> &ResourceRef {
            &self.flow.provider
        }
        fn inspect(&self, flow: &ResourceRef) -> Result<FlowSourceDescriptor> {
            if flow != &self.flow.flow_ref {
                return Err(AikitError::new("flow.not_found", "unknown Flow"));
            }
            Ok(self.flow.clone())
        }
        fn read_exact(&self, flow: &ResourceRef, rev: &SourceRevision) -> Result<FlowReadOutcome> {
            if flow != &self.flow.flow_ref || rev != &self.flow.revision {
                return Err(AikitError::new("flow.revision_conflict", "stale Flow read"));
            }
            Ok(FlowReadOutcome::Disclosed {
                flow: self.flow.clone(),
                body: self.body.clone(),
            })
        }
        fn write(&mut self, _request: &FlowWriteRequest) -> Result<FlowWriteResult> {
            Err(AikitError::new(
                "flow.write_not_available",
                "fixture Flow owner is read-only",
            ))
        }
    }

    fn provider() -> FixtureProvider {
        FixtureProvider {
            flow: FlowSourceDescriptor {
                flow_ref: resource("central:flow:project:test:thread-1"),
                source_ref: source("central:source:project:test:notes%2Fthread.md"),
                revision: revision("central.content-fnv1a64/v1:7:aaaaaaaaaaaaaaaa"),
                provider: resource("provider:central-flow"),
                lifecycle: FlowLifecycle::Active,
                title: Some("Thread".into()),
                scope: Some(resource("project:test")),
                container_hint: Some("ProjectCentral/now/flows/thread.md".into()),
                capabilities: FlowCapabilities {
                    read: true,
                    write: false,
                    history: false,
                },
                provenance: vec!["owner FlowRef + revision".into()],
            },
            body: "current Flow body".into(),
        }
    }

    fn context() -> crate::context_resolution::ContextResolution {
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

    fn praxis(
        context: &crate::context_resolution::ContextResolution,
        method: &Method,
    ) -> crate::praxis::PraxisResolution {
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

    fn parts(provider: &FixtureProvider) -> FlowContemplateRequest<'static> {
        let context = Box::leak(Box::new(context()));
        let method = Box::leak(Box::new(first_party_flow_method(None).unwrap()));
        let praxis = Box::leak(Box::new(praxis(context, method)));
        let horizon = Box::leak(Box::new(horizon(&provider.flow)));
        let runtime = Box::leak(Box::new(runtime("agent-session/contemplate")));
        let dependencies = Box::leak(Box::new(Vec::new()));
        let objects = Box::leak(Box::new(Vec::new()));
        let standing = bind_flow_for_act(
            provider,
            context,
            &provider.flow.flow_ref,
            resource("agent-session/contemplate"),
            Some(resource("agent:test")),
            Some(resource("agency:test")),
        )
        .unwrap();
        let living = Box::leak(Box::new(ContemplateRequest {
            project: ProjectRef::parse("project:test").unwrap(),
            focus: vec![],
            horizon,
            dependencies,
            current_wiki_objects: objects,
            runtime,
            method: Some(method),
            ql: None,
        }));
        let authority_refs = Box::leak(Box::new(vec![
            crate::flow::FlowAuthorityRef {
                authority: crate::flow::FlowContextAuthority::Flow,
                reference: standing.binding.flow_ref.clone(),
            },
            crate::flow::FlowAuthorityRef {
                authority: crate::flow::FlowContextAuthority::AgentSession,
                reference: resource("agent-session/contemplate"),
            },
        ]));
        // Leak-backed fixture so FlowContemplateRequest<'a> borrows stay simple in tests.
        FlowContemplateRequest::with_defaults(
            Box::leak(Box::new(standing)),
            living,
            &[],
            praxis,
            authority_refs,
        )
    }

    #[test]
    fn validate_accepts_real_preflight_and_refuses_tampered_records() {
        let provider = provider();
        let parts = parts(&provider);
        let record = flow_contemplate_preflight(&parts).unwrap();
        assert_eq!(record.version, FLOW_CONTEMPLATE_VERSION);
        validate_flow_contemplate_record(&record).unwrap();

        let mut bad_version = record.clone();
        bad_version.version = "aikit.flow-contemplate/v0".into();
        assert_eq!(
            validate_flow_contemplate_record(&bad_version)
                .unwrap_err()
                .code(),
            "flow.cognition_preflight_version"
        );

        let mut forged = record.clone();
        forged.invocation_ref = resource("flow-tampered/forgeddigest0000");
        assert_eq!(
            validate_flow_contemplate_record(&forged)
                .unwrap_err()
                .code(),
            "flow.cognition_preflight_invocation_ref"
        );

        let mut claimed = record.clone();
        claimed.automatic_agent_or_model_invocation = true;
        assert_eq!(
            validate_flow_contemplate_record(&claimed)
                .unwrap_err()
                .code(),
            "flow.cognition_preflight_invocation_invariant"
        );

        let mut undisclosed = record.clone();
        undisclosed.standing.disclosure = crate::flow::FlowStandingDisclosure::Undisclosed {
            reason: "withheld".into(),
        };
        assert_eq!(
            validate_flow_contemplate_record(&undisclosed)
                .unwrap_err()
                .code(),
            "flow.cognition_preflight_undisclosed"
        );
    }

    #[test]
    fn explain_discloses_exact_reads_and_execution_invariant() {
        let provider = provider();
        let parts = parts(&provider);
        let record = flow_contemplate_preflight(&parts).unwrap();
        let explain = explain_flow_contemplate_preflight(&record);
        assert_eq!(explain.len(), 1);
        let evidence = &explain[0];
        assert_eq!(evidence.subject, record.invocation_ref);
        let relations = evidence
            .facts
            .iter()
            .map(|fact| fact.relation.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "flow-source",
            "standing-context-read",
            "knowledge-impact",
            "method-praxis",
            "preflight-record",
            "execution-invariant",
        ] {
            assert!(relations.contains(&expected), "missing fact {expected}");
        }
        let source_fact = evidence
            .facts
            .iter()
            .find(|fact| fact.relation == "flow-source")
            .unwrap();
        assert_eq!(source_fact.authority, Some(SourceAuthority::Authored));
        assert!(source_fact
            .provenance
            .iter()
            .any(|entry| entry.revision.as_deref()
                == Some(record.standing.binding.flow_revision.as_str())));
        assert!(evidence
            .facts
            .iter()
            .any(|fact| fact.relation == "preflight-record"
                && fact.canonical_refs.contains(&record.invocation_ref)));
    }

    #[test]
    fn validated_execution_refuses_record_from_a_different_basis() {
        let fixture = provider();
        let parts_a = parts(&fixture);
        let record = flow_contemplate_preflight(&parts_a).unwrap();

        // A record computed over a different horizon (different cursor) cannot
        // gate execution over this basis, even though it is itself genuine.
        let mut other_provider = provider();
        other_provider.flow.revision = revision("owner-r9");
        let parts_b = parts(&other_provider);
        let other_record = flow_contemplate_preflight(&parts_b).unwrap();
        assert_ne!(record.invocation_ref, other_record.invocation_ref);

        struct PanicExecutor;
        impl crate::flow::FlowContemplateExecutor for PanicExecutor {
            fn execute(
                &mut self,
                _preflight: &crate::flow::FlowContemplatePreflight,
            ) -> Result<crate::flow::FlowContemplateGenerated> {
                panic!("executor must not be called for a drifted record");
            }
        }
        let mut executor = PanicExecutor;
        let error = explicit_flow_contemplate_validated(&parts_a, &other_record, &mut executor)
            .unwrap_err();
        assert_eq!(error.code(), "flow.cognition_preflight_drift");
    }

    fn thought_fixture() -> (FlowThoughtRecord, FlowSourceDescriptor) {
        let provider = provider();
        let flow = provider.flow.clone();
        let record = FlowThoughtRecord {
            version: FLOW_COGNITION_VERSION.into(),
            invocation_ref: resource("flow-contemplate/abcd1234abcd1234abcd12"),
            flow_ref: flow.flow_ref.clone(),
            source_ref: flow.source_ref.clone(),
            basis_revision: flow.revision.clone(),
            horizon_cursor: 12,
            outcome: Some(FlowThoughtOutcome {
                candidates: vec!["candidate:re-read the thread".into()],
                tensions: vec!["tension:wording versus evidence".into()],
                human_source_proposals: vec![HumanSourceRevisionProposal {
                    source: source("source:human-ground:test"),
                    reason: "return for human Recognition".into(),
                    evidence: vec![flow.source_ref.clone()],
                }],
                flow_mutations: vec![FlowMutationIntent {
                    version: FLOW_CONTEXT_VERSION.into(),
                    flow_ref: flow.flow_ref.clone(),
                    expected_revision: flow.revision.clone(),
                    replacement: "refined".into(),
                    actor: resource("agent:test"),
                    agency: Some(resource("agency:test")),
                    agent_session: resource("agent-session/contemplate"),
                    context_resolution_version: "aikit.context-resolution/v2".into(),
                    method: Some(resource(FLOW_METHOD_REF)),
                    invocation_ref: Some(resource("flow-contemplate/abcd1234abcd1234abcd12")),
                }],
            }),
        };
        (record, flow)
    }

    fn later_horizon(
        flow: &FlowSourceDescriptor,
        cursor: u64,
        after: &str,
    ) -> KnowledgeChangeHorizon {
        let mut horizon = horizon(flow);
        horizon.cursor = cursor;
        for change in &mut horizon.changes {
            change.cursor = cursor;
            change.before_revision = Some(flow.revision.clone());
            change.after_revision = Some(revision(after));
        }
        horizon.sources[0].revision = Some(revision(after));
        horizon
    }

    #[test]
    fn changed_since_reports_rows_with_provenance_and_superseded_mutations() {
        let (thought, flow) = thought_fixture();
        let current = {
            let mut current = flow.clone();
            current.revision = revision("owner-r3");
            current
        };
        let horizon = later_horizon(&flow, 13, "owner-r3");
        let dependencies = vec![crate::knowledge_living::KnowledgeDependency {
            dependent: resource("wiki:reading:thread"),
            source: flow.source_ref.clone(),
            basis_revision: Some(flow.revision.clone()),
            relation: "integrates-flow".into(),
            provenance_ref: Some(resource("wiki:reading:thread")),
            integrative: true,
        }];

        let reading =
            changed_since_thought(&thought, &horizon, &dependencies, Some(&current)).unwrap();
        assert_eq!(reading.version, FLOW_CHANGED_SINCE_VERSION);
        assert_eq!(reading.state, FlowChangedSinceState::Available);
        assert!(!reading.automatic_agent_or_model_invocation);
        assert_eq!(reading.thought_ref, thought.invocation_ref);

        assert_eq!(reading.changed_sources.len(), 1);
        let changed = &reading.changed_sources[0];
        assert_eq!(changed.source, flow.source_ref);
        assert_eq!(changed.provenance, "collaborative-revision-provenance");
        assert_eq!(changed.observed_revision, Some(revision("owner-r3")));

        assert_eq!(reading.affected_knowledge.len(), 1);
        assert_eq!(
            reading.affected_knowledge[0].freshness,
            crate::knowledge_living::KnowledgeFreshness::IntegrationPending
        );

        let kinds = reading
            .unresolved
            .iter()
            .map(|row| row.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&ThoughtUnresolvedKind::Tension));
        assert!(kinds.contains(&ThoughtUnresolvedKind::Candidate));
        assert!(kinds.contains(&ThoughtUnresolvedKind::HumanSourceProposal));
        assert!(kinds.contains(&ThoughtUnresolvedKind::FlowMutationIntent));
        // The owner revision advanced past the exact expected revision: the
        // returned mutation intent no longer stands as-is.
        let mutation = reading
            .unresolved
            .iter()
            .find(|row| row.kind == ThoughtUnresolvedKind::FlowMutationIntent)
            .unwrap();
        assert_eq!(mutation.state, "basis-superseded");
        assert!(reading
            .unresolved
            .iter()
            .all(|row| !row.provenance.is_empty()));
    }

    #[test]
    fn changed_since_marks_mutation_unresolved_when_owner_revision_unchanged() {
        let (thought, flow) = thought_fixture();
        let horizon = later_horizon(&flow, 13, "owner-r3");
        let reading = changed_since_thought(&thought, &horizon, &[], Some(&flow)).unwrap();
        let mutation = reading
            .unresolved
            .iter()
            .find(|row| row.kind == ThoughtUnresolvedKind::FlowMutationIntent)
            .unwrap();
        assert_eq!(mutation.state, "unresolved");
    }

    #[test]
    fn changed_since_is_empty_when_nothing_moved_and_unavailable_without_owner_flow() {
        let (thought, flow) = thought_fixture();

        // Same cursor horizon: no change rows above the thought cursor and the
        // owner revision is unchanged. The thought's own returned items remain
        // open, so this reading is Available through its unresolved rows.
        let horizon = horizon(&flow);
        let reading = changed_since_thought(&thought, &horizon, &[], Some(&flow)).unwrap();
        assert_eq!(reading.state, FlowChangedSinceState::Available);
        assert!(reading.changed_sources.is_empty());
        assert!(reading.affected_knowledge.is_empty());
        assert!(!reading.unresolved.is_empty());

        // With no outcome and no movement, every section is genuinely empty.
        let mut quiet = thought.clone();
        quiet.outcome = None;
        let reading = changed_since_thought(&quiet, &horizon, &[], Some(&flow)).unwrap();
        assert_eq!(reading.state, FlowChangedSinceState::Empty);

        let reading = changed_since_thought(&thought, &horizon, &[], None).unwrap();
        assert_eq!(reading.state, FlowChangedSinceState::Unavailable);
        assert!(reading
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no longer inspects")));
        assert!(reading.changed_sources.is_empty());
        assert!(reading.affected_knowledge.is_empty());
    }

    #[test]
    fn changed_since_rejects_a_flow_identity_mismatch() {
        let (thought, flow) = thought_fixture();
        let mut other = flow.clone();
        other.flow_ref = resource("central:flow:project:test:thread-2");
        let error =
            changed_since_thought(&thought, &horizon(&flow), &[], Some(&other)).unwrap_err();
        assert_eq!(error.code(), "flow.changed_since_flow_mismatch");
    }

    #[test]
    fn owner_drift_without_change_row_is_disclosed_from_owner_inspection() {
        let (thought, flow) = thought_fixture();
        // No horizon change rows, but the owner inspect seam shows drift.
        let horizon = KnowledgeChangeHorizon {
            provider: "central.source-change-horizon/v1".into(),
            cursor: 13,
            sources: vec![KnowledgeObservedSource {
                source: flow.source_ref.clone(),
                revision: Some(revision("owner-r4")),
                available: true,
            }],
            changes: vec![],
        };
        let mut current = flow.clone();
        current.revision = revision("owner-r4");
        let reading = changed_since_thought(&thought, &horizon, &[], Some(&current)).unwrap();
        assert_eq!(reading.changed_sources.len(), 1);
        assert!(reading.changed_sources[0]
            .provenance
            .contains("owner-inspect"));
    }
}
