//! W1.4/W1.5 owner-side Flow cognition operations on the shared application
//! Service.
//!
//! `flow contemplate` is the explicit owner operation behind the canonical
//! `action:contemplate-flow` Action. It resolves the Flow through the existing
//! knowledge runtime (Semantic Wiki identity + SourcePool exact-revision
//! material), runs the deterministic Flow preflight, discloses what will be
//! read and touched as Explain evidence, and only then — with a host-supplied
//! executor — crosses the Living Knowledge aperture exactly once. Without a
//! host executor the typed reading is explicitly `unavailable`; Contemplate is
//! never auto-invoked (#138 §7). A drifted or hand-forged preflight record is
//! explicitly `refused` before any executor call.

use aikit_core::context_resolution::{
    compose_context_resolution, ContextResolution, RequestedActors,
};
use aikit_core::explain_history::ExplainEvidence;
use aikit_core::flow::{
    bind_flow_for_act, first_party_flow_method, first_party_flow_resource_records,
    flow_contemplate_preflight, FlowAuthorityRef, FlowCapabilities, FlowContemplateExecutor,
    FlowContemplatePreflight, FlowContemplateRequest, FlowContextAuthority, FlowLifecycle,
    FlowProvider, FlowReadOutcome, FlowSourceDescriptor, FlowWriteRequest, FlowWriteResult,
};
use aikit_core::knowledge_living::{
    ContemplateRequest, KnowledgeChangeHorizon, KnowledgeDependency,
};
use aikit_core::knowledge_living_relations::KnowledgeResourceDependency;
use aikit_core::knowledge_wiki::{SemanticRevision, WikiNode, WikiObject};
use aikit_core::model_runtime::ModelRuntimeReadModel;
use aikit_core::praxis::PraxisResolution;
use aikit_core::resource::{MemoryResourceIndex, ResourceRef, SourceRevision};
use aikit_core::wiki_living_dependencies;
use aikit_core::{
    changed_since_thought, explain_flow_contemplate_preflight, explicit_flow_contemplate_validated,
    AikitError, EventId, FamiliarityObservation, FlowChangedSince, FlowChangedSinceState,
    FlowCognition, FlowThoughtRecord, ACTION_CONTEMPLATE_FLOW, FLOW_COGNITION_VERSION,
    FLOW_CONTEMPLATE_USE_RECORDED,
};
use aikit_store::append_familiarity_observation;
use aikit_tui::backend::PaletteBackend;
use serde::{Deserialize, Serialize};

use super::knowledge::{now_ms, KnowledgeRuntime};
use super::Service;

const FLOW_PROVIDER_REF: &str = "provider/knowledge-runtime";
const KNOWLEDGE_SURFACE_REF: &str = "surface/aikit/knowledge";

/// Owner seams the host must supply for one Contemplate(FlowRef). Both are
/// honest `None` states on surfaces without them — the reading is then
/// `unavailable`, never guessed.
#[derive(Debug, Clone, Default)]
pub struct FlowContemplateBasis {
    /// Owner change horizon (e.g. Central's `central.source-change-horizon/v1`).
    pub horizon: Option<KnowledgeChangeHorizon>,
    /// The host model-runtime read identifying model, Agent, Agency and
    /// AgentSession for attribution.
    pub runtime: Option<ModelRuntimeReadModel>,
    pub agent: Option<ResourceRef>,
    pub agency: Option<ResourceRef>,
}

/// Preflight-only result: the deterministic preflight record plus its Explain
/// disclosure, or an explicit unavailable state. Inert — records nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum FlowPreflightOutcome {
    Preflight {
        version: String,
        flow: ResourceRef,
        preflight: Box<FlowContemplatePreflight>,
        explain: Vec<ExplainEvidence>,
        /// Deterministic preflight crosses no Agent/model seam.
        automatic_agent_or_model_invocation: bool,
    },
    Unavailable {
        version: String,
        flow: ResourceRef,
        reason: String,
    },
}

/// One explicit Contemplate(FlowRef) result: the preflight record that gated
/// it, its Explain disclosure, the typed cognition reading and — only on a
/// successful cognition — the single recorded familiarity observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowContemplateReceipt {
    pub version: String,
    pub flow: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight: Option<FlowContemplatePreflight>,
    #[serde(default)]
    pub explain: Vec<ExplainEvidence>,
    pub cognition: FlowCognition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
}

/// One W1.5 changed-since-thought owner read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowChangedSinceReceipt {
    pub version: String,
    pub flow: ResourceRef,
    pub thought: ResourceRef,
    pub reading: FlowChangedSince,
}

/// The knowledge runtime as the Flow owner seam: Semantic Wiki identity plus
/// SourcePool exact-revision material. Owner-side read-only — `write` always
/// declines; returned Flow mutation intents remain owner requests.
struct KnowledgeFlowProvider<'a> {
    descriptor: FlowSourceDescriptor,
    material: &'a [aikit_core::knowledge_source_pool::SourceMaterial],
}

impl FlowProvider for KnowledgeFlowProvider<'_> {
    fn provider_ref(&self) -> &ResourceRef {
        &self.descriptor.provider
    }

    fn inspect(&self, flow: &ResourceRef) -> aikit_core::Result<FlowSourceDescriptor> {
        if flow != &self.descriptor.flow_ref {
            return Err(AikitError::new("flow.not_found", "unknown Flow"));
        }
        Ok(self.descriptor.clone())
    }

    fn read_exact(
        &self,
        flow: &ResourceRef,
        revision: &SourceRevision,
    ) -> aikit_core::Result<FlowReadOutcome> {
        if flow != &self.descriptor.flow_ref {
            return Err(AikitError::new("flow.not_found", "unknown Flow"));
        }
        if revision != &self.descriptor.revision {
            return Err(AikitError::new(
                "flow.revision_conflict",
                "stale Flow read: the Flow changed after the disclosed basis",
            ));
        }
        let Some(material) = self
            .material
            .iter()
            .find(|item| item.binding.source == self.descriptor.source_ref)
        else {
            return Ok(FlowReadOutcome::Undisclosed {
                flow: self.descriptor.clone(),
                reason: "Flow source is not present in the source pool in this context".into(),
            });
        };
        if &material.binding.revision != revision {
            return Err(AikitError::new(
                "flow.revision_conflict",
                "source pool material no longer matches the disclosed Flow revision",
            ));
        }
        Ok(FlowReadOutcome::Disclosed {
            flow: self.descriptor.clone(),
            body: material.body.clone(),
        })
    }

    fn write(&mut self, _request: &FlowWriteRequest) -> aikit_core::Result<FlowWriteResult> {
        Err(AikitError::new(
            "flow.write_not_available",
            "owner-side Flow cognition never writes the source owner",
        ))
    }
}

struct AssembledFlowContemplation {
    context: ContextResolution,
    praxis: PraxisResolution,
    standing: aikit_core::FlowStandingContext,
    method: aikit_core::Method,
    dependencies: Vec<KnowledgeDependency>,
    resource_dependencies: Vec<KnowledgeResourceDependency>,
    objects: Vec<WikiObject>,
    authority_refs: Vec<FlowAuthorityRef>,
}

impl Service {
    fn flow_node(&self, flow_ref: &ResourceRef) -> aikit_core::Result<Option<WikiNode>> {
        self.with_knowledge(|runtime, _| {
            let Some(index) = runtime.wiki_index() else {
                return Ok(None);
            };
            if !index.contains(flow_ref) {
                return Ok(None);
            }
            match index.resolve(flow_ref) {
                Some(WikiObject::Node(node)) if node.node_type == "flow" => Ok(Some(node)),
                _ => Ok(None),
            }
        })
    }

    /// Resolve the Flow identity the way the owner stores it: node identity
    /// and exact source revision from Semantic Wiki provenance. Never inferred
    /// from containers or directory names.
    fn flow_descriptor(
        &self,
        flow_ref: &ResourceRef,
    ) -> aikit_core::Result<Option<FlowSourceDescriptor>> {
        let Some(node) = self.flow_node(flow_ref)? else {
            return Ok(None);
        };
        let source_ref = node.source_refs.first().cloned().ok_or_else(|| {
            AikitError::new(
                "flow.node_without_source",
                format!("Flow node {flow_ref} names no source ref"),
            )
        })?;
        let revision = node
            .provenance
            .iter()
            .find(|entry| entry.source_ref == source_ref)
            .and_then(|entry| entry.source_revision.clone())
            .and_then(|value| match value {
                SemanticRevision::Text(text) => SourceRevision::parse(&text).ok(),
                _ => None,
            })
            .ok_or_else(|| {
                AikitError::new(
                    "flow.node_without_revision",
                    format!("Flow node {flow_ref} has no exact source revision provenance"),
                )
            })?;
        let mut provenance: Vec<String> = node
            .provenance
            .iter()
            .map(|entry| match &entry.source_revision {
                Some(SemanticRevision::Text(revision)) => {
                    format!("{}@{revision}", entry.source_ref)
                }
                _ => entry.source_ref.to_string(),
            })
            .collect();
        provenance.push(format!("revision {}", node.revision));
        Ok(Some(FlowSourceDescriptor {
            flow_ref: flow_ref.clone(),
            source_ref,
            revision,
            provider: ResourceRef::parse(FLOW_PROVIDER_REF)?,
            lifecycle: FlowLifecycle::Active,
            title: node.title.clone(),
            scope: None,
            container_hint: None,
            capabilities: FlowCapabilities {
                read: true,
                write: false,
                history: false,
            },
            provenance,
        }))
    }

    fn flow_context(&self) -> aikit_core::Result<Option<ContextResolution>> {
        let Some(binding) = PaletteBackend::project_binding(self)? else {
            return Ok(None);
        };
        let mut resources = MemoryResourceIndex::default();
        for record in first_party_flow_resource_records()? {
            resources.insert(record);
        }
        Ok(Some(compose_context_resolution(
            &self.view,
            binding,
            &self.layers,
            &resources,
            RequestedActors::default(),
        )))
    }

    fn assemble_flow_contemplation(
        &self,
        runtime: &KnowledgeRuntime,
        flow_ref: &ResourceRef,
        basis: &FlowContemplateBasis,
        descriptor: &FlowSourceDescriptor,
    ) -> aikit_core::Result<AssembledFlowContemplation> {
        let runtime_model = basis.runtime.as_ref().expect("runtime checked by caller");
        let session = runtime_model.agent_session.as_deref().ok_or_else(|| {
            AikitError::new(
                "flow.contemplate_session_unavailable",
                "the host model runtime identifies no AgentSession for Contemplate attribution",
            )
        })?;
        let context = self.flow_context()?.ok_or_else(|| {
            AikitError::new(
                "flow.contemplate_project_unavailable",
                "Contemplate(FlowRef) requires a bound Project (ProjectCentral/project.json)",
            )
        })?;
        let provider = KnowledgeFlowProvider {
            descriptor: descriptor.clone(),
            material: runtime.source_material(),
        };
        let standing = bind_flow_for_act(
            &provider,
            &context,
            flow_ref,
            ResourceRef::parse(session)?,
            basis.agent.clone(),
            basis.agency.clone(),
        )?;
        let mut resources = MemoryResourceIndex::default();
        for record in first_party_flow_resource_records()? {
            resources.insert(record);
        }
        let method = first_party_flow_method(None)?;
        let praxis = aikit_core::resolve_praxis(
            &context,
            &resources,
            std::slice::from_ref(&method),
            std::slice::from_ref(&method.id),
            &[],
        );
        let mut objects = Vec::new();
        if let Some(index) = runtime.wiki_index() {
            for resource in index.discover() {
                if let Some(object) = index.resolve(&resource) {
                    objects.push(object);
                }
            }
        }
        let (dependencies, resource_dependencies) = wiki_living_dependencies(&objects)?;
        Ok(AssembledFlowContemplation {
            context,
            praxis,
            standing,
            method,
            dependencies,
            resource_dependencies,
            objects,
            authority_refs: vec![
                FlowAuthorityRef {
                    authority: FlowContextAuthority::Flow,
                    reference: flow_ref.clone(),
                },
                FlowAuthorityRef {
                    authority: FlowContextAuthority::AgentSession,
                    reference: ResourceRef::parse(session)?,
                },
            ],
        })
    }

    fn preflight_from_assembly<'a>(
        assembled: &'a AssembledFlowContemplation,
        basis: &'a FlowContemplateBasis,
        flow_ref: &ResourceRef,
    ) -> aikit_core::Result<(FlowContemplatePreflight, Vec<ExplainEvidence>)> {
        let horizon = basis.horizon.as_ref().expect("horizon checked by caller");
        let runtime_model = basis.runtime.as_ref().expect("runtime checked by caller");
        let living = ContemplateRequest {
            project: assembled.context.project_binding.project.clone(),
            focus: vec![flow_ref.clone()],
            horizon,
            dependencies: &assembled.dependencies,
            current_wiki_objects: &assembled.objects,
            runtime: runtime_model,
            method: Some(&assembled.method),
            ql: None,
        };
        let request = FlowContemplateRequest::with_defaults(
            &assembled.standing,
            &living,
            &assembled.resource_dependencies,
            &assembled.praxis,
            &assembled.authority_refs,
        );
        let preflight = flow_contemplate_preflight(&request)?;
        let explain = explain_flow_contemplate_preflight(&preflight);
        Ok((preflight, explain))
    }

    /// W1.4 preflight half: resolve the Flow, assemble the deterministic
    /// Contemplate preflight and disclose what will be read and touched. Inert:
    /// no Agent/model invocation and no familiarity observation.
    pub fn flow_contemplate_preflight(
        &mut self,
        flow_ref: &ResourceRef,
        basis: &FlowContemplateBasis,
    ) -> aikit_core::Result<FlowPreflightOutcome> {
        let descriptor = self.flow_descriptor(flow_ref)?.ok_or_else(|| {
            AikitError::new(
                "flow.not_found",
                format!("no Flow node resolves {flow_ref}"),
            )
        })?;
        let unavailable = |reason: &str| FlowPreflightOutcome::Unavailable {
            version: FLOW_COGNITION_VERSION.into(),
            flow: flow_ref.clone(),
            reason: reason.into(),
        };
        let (Some(_), Some(_)) = (&basis.horizon, &basis.runtime) else {
            return Ok(unavailable(
                "no owner change horizon and host model runtime supplied; the deterministic Flow preflight cannot be computed without both",
            ));
        };
        self.with_knowledge(|runtime, _| {
            let assembled =
                self.assemble_flow_contemplation(runtime, flow_ref, basis, &descriptor)?;
            let (preflight, explain) = Self::preflight_from_assembly(&assembled, basis, flow_ref)?;
            Ok(FlowPreflightOutcome::Preflight {
                version: FLOW_COGNITION_VERSION.into(),
                flow: flow_ref.clone(),
                preflight: preflight.into(),
                explain,
                automatic_agent_or_model_invocation: false,
            })
        })
    }

    /// W1.4 execution half: the supplied preflight record is structurally
    /// validated and the deterministic preflight is recomputed over a fresh
    /// assembly of the same basis; any mismatch refuses execution before the
    /// executor is called. Without a host executor the reading is explicitly
    /// `unavailable` — Contemplate is never auto-invoked.
    pub fn flow_contemplate_with_record(
        &mut self,
        flow_ref: &ResourceRef,
        basis: &FlowContemplateBasis,
        record: &FlowContemplatePreflight,
        executor: Option<&mut dyn FlowContemplateExecutor>,
    ) -> aikit_core::Result<FlowContemplateReceipt> {
        let mut receipt = FlowContemplateReceipt {
            version: FLOW_COGNITION_VERSION.into(),
            flow: flow_ref.clone(),
            preflight: Some(record.clone()),
            explain: Vec::new(),
            cognition: FlowCognition::Unavailable {
                version: FLOW_COGNITION_VERSION.into(),
                invocation_ref: Some(record.invocation_ref.clone()),
                flow_ref: flow_ref.clone(),
                reason: String::new(),
            },
            recorded: None,
            observation_id: None,
        };
        let descriptor = self.flow_descriptor(flow_ref)?.ok_or_else(|| {
            AikitError::new(
                "flow.not_found",
                format!("no Flow node resolves {flow_ref}"),
            )
        })?;
        let (Some(_), Some(_)) = (&basis.horizon, &basis.runtime) else {
            receipt.cognition = FlowCognition::Unavailable {
                version: FLOW_COGNITION_VERSION.into(),
                invocation_ref: Some(record.invocation_ref.clone()),
                flow_ref: flow_ref.clone(),
                reason: "no owner change horizon and host model runtime supplied; execution without a computed basis would be a guess".into(),
            };
            receipt.preflight = None;
            return Ok(receipt);
        };
        if let Err(error) = aikit_core::validate_flow_contemplate_record(record) {
            receipt.cognition = FlowCognition::Refused {
                version: FLOW_COGNITION_VERSION.into(),
                invocation_ref: Some(record.invocation_ref.clone()),
                flow_ref: flow_ref.clone(),
                reason: format!("{}: {}", error.code(), error.message()),
            };
            return Ok(receipt);
        }
        let cognition = self.with_knowledge(|runtime, _| {
            let assembled =
                self.assemble_flow_contemplation(runtime, flow_ref, basis, &descriptor)?;
            let (fresh, explain) = Self::preflight_from_assembly(&assembled, basis, flow_ref)?;
            receipt.explain = explain;
            if fresh != *record {
                return Ok(FlowCognition::Refused {
                    version: FLOW_COGNITION_VERSION.into(),
                    invocation_ref: Some(record.invocation_ref.clone()),
                    flow_ref: flow_ref.clone(),
                    reason: "preflight record drifted: a freshly computed preflight over the current owner state differs from the supplied record".into(),
                });
            }
            let Some(executor) = executor else {
                return Ok(FlowCognition::Unavailable {
                    version: FLOW_COGNITION_VERSION.into(),
                    invocation_ref: Some(record.invocation_ref.clone()),
                    flow_ref: flow_ref.clone(),
                    reason: "no host ContemplateExecutor supplied on this surface; Contemplate(FlowRef) is never auto-invoked".into(),
                });
            };
            let horizon = basis.horizon.as_ref().expect("checked above");
            let runtime_model = basis.runtime.as_ref().expect("checked above");
            let living = ContemplateRequest {
                project: assembled.context.project_binding.project.clone(),
                focus: vec![flow_ref.clone()],
                horizon,
                dependencies: &assembled.dependencies,
                current_wiki_objects: &assembled.objects,
                runtime: runtime_model,
                method: Some(&assembled.method),
                ql: None,
            };
            let request = FlowContemplateRequest::with_defaults(
                &assembled.standing,
                &living,
                &assembled.resource_dependencies,
                &assembled.praxis,
                &assembled.authority_refs,
            );
            explicit_flow_contemplate_validated(&request, record, executor).map_err(|error| {
                AikitError::new(
                    "flow.contemplate_execution_refused",
                    format!("{}: {}", error.code(), error.message()),
                )
            })
        });
        receipt.cognition = match cognition {
            Ok(reading) => reading,
            Err(error) => FlowCognition::Refused {
                version: FLOW_COGNITION_VERSION.into(),
                invocation_ref: Some(record.invocation_ref.clone()),
                flow_ref: flow_ref.clone(),
                reason: format!("{}: {}", error.code(), error.message()),
            },
        };
        if matches!(receipt.cognition, FlowCognition::Cognition { .. }) {
            let observation_id = format!("flow-contemplate-use/{}", EventId::generate());
            let observation = FamiliarityObservation::destination(
                observation_id.clone(),
                flow_ref.clone(),
                self.knowledge_context(),
                now_ms(),
            )
            .from_surface(ResourceRef::parse(KNOWLEDGE_SURFACE_REF)?)
            .via_action(ResourceRef::parse(ACTION_CONTEMPLATE_FLOW)?);
            append_familiarity_observation(&self.index, observation)?;
            receipt.recorded = Some(FLOW_CONTEMPLATE_USE_RECORDED.into());
            receipt.observation_id = Some(observation_id);
        }
        Ok(receipt)
    }

    /// One explicit Contemplate(FlowRef): preflight → Explain disclosure →
    /// gated execution. This is the owner operation the kernel dispatch seam
    /// (`oi.cradle.action-dispatch/v1`, `action:contemplate-flow`) can invoke
    /// once a later kernel cell supplies the owner seams and a host executor.
    pub fn flow_contemplate(
        &mut self,
        flow_ref: &ResourceRef,
        basis: &FlowContemplateBasis,
        executor: Option<&mut dyn FlowContemplateExecutor>,
    ) -> aikit_core::Result<FlowContemplateReceipt> {
        match self.flow_contemplate_preflight(flow_ref, basis)? {
            FlowPreflightOutcome::Preflight { preflight, .. } => {
                self.flow_contemplate_with_record(flow_ref, basis, &preflight, executor)
            }
            FlowPreflightOutcome::Unavailable { flow, reason, .. } => Ok(FlowContemplateReceipt {
                version: FLOW_COGNITION_VERSION.into(),
                flow,
                preflight: None,
                explain: Vec::new(),
                cognition: FlowCognition::Unavailable {
                    version: FLOW_COGNITION_VERSION.into(),
                    invocation_ref: None,
                    flow_ref: flow_ref.clone(),
                    reason,
                },
                recorded: None,
                observation_id: None,
            }),
        }
    }

    /// W1.5 owner read: what changed relative to one recorded thought. Every
    /// row carries provenance; `empty` and `unavailable` are explicit states.
    pub fn flow_changed_since(
        &mut self,
        thought: &FlowThoughtRecord,
        current_horizon: Option<KnowledgeChangeHorizon>,
    ) -> aikit_core::Result<FlowChangedSinceReceipt> {
        let flow_ref = thought.flow_ref.clone();
        let reading = self.with_knowledge(|runtime, _| {
            let current_flow = self.flow_descriptor(&flow_ref)?;
            let Some(horizon) = &current_horizon else {
                return Ok(FlowChangedSince {
                    version: aikit_core::FLOW_CHANGED_SINCE_VERSION.into(),
                    flow_ref: flow_ref.clone(),
                    thought_ref: thought.invocation_ref.clone(),
                    state: FlowChangedSinceState::Unavailable,
                    changed_sources: Vec::new(),
                    affected_knowledge: Vec::new(),
                    unresolved: Vec::new(),
                    unavailable_reason: Some(
                        "no owner change horizon supplied; changed-since rows would be guesses"
                            .into(),
                    ),
                    automatic_agent_or_model_invocation: false,
                });
            };
            let mut objects = Vec::new();
            if let Some(index) = runtime.wiki_index() {
                for resource in index.discover() {
                    if let Some(object) = index.resolve(&resource) {
                        objects.push(object);
                    }
                }
            }
            let (dependencies, _) = wiki_living_dependencies(&objects)?;
            changed_since_thought(thought, horizon, &dependencies, current_flow.as_ref())
        })?;
        Ok(FlowChangedSinceReceipt {
            version: aikit_core::FLOW_CHANGED_SINCE_VERSION.into(),
            flow: flow_ref,
            thought: thought.invocation_ref.clone(),
            reading,
        })
    }
}
