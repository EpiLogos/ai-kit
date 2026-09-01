//! Living Knowledge semantics over the existing KnowledgeApplication and Semantic Wiki.
//!
//! Source revision/change truth is provider-owned (Central is one provider). AIKit owns only
//! deterministic dependency impact, integrative WikiReading freshness and the explicit
//! `Contemplate` execution aperture. No function in this module performs filesystem observation,
//! background Agent/model invocation, or direct human-source mutation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::knowledge_navigation::KnowledgeApplication;
use crate::knowledge_wiki::{SemanticRevision, WikiObject, WikiProvenanceRef, WikiReading};
use crate::method::Method;
use crate::model_runtime::ModelRuntimeReadModel;
use crate::project::ProjectRef;
use crate::projectcentral::{
    plan_agent_wiki_maintenance, AgentWikiMaintenancePlan, AgentWikiMaintenanceRequest,
    HumanSourceRevisionProposal,
};
use crate::ql::QlRefractionRequest;
use crate::resource::{ResourceRef, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const LIVING_KNOWLEDGE_VERSION: &str = "aikit.living-knowledge/v1";
pub const INTEGRATIVE_READING_EXTENSION: &str = "aikit.integrative-reading/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeChangeKind {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSourceChange {
    pub cursor: u64,
    pub world_ref: String,
    pub source: SourceRef,
    #[serde(default)]
    pub roles: Vec<String>,
    pub provenance: String,
    pub standing: String,
    #[serde(default)]
    pub before_revision: Option<SourceRevision>,
    #[serde(default)]
    pub after_revision: Option<SourceRevision>,
    pub kind: KnowledgeChangeKind,
    pub agent_retrieval_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeObservedSource {
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub available: bool,
}

/// Provider-neutral source horizon consumed by AIKit. Central may adapt its
/// `central.source-change-horizon/v1` here without AIKit taking ownership of Central state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeHorizon {
    pub provider: String,
    pub cursor: u64,
    #[serde(default)]
    pub sources: Vec<KnowledgeObservedSource>,
    #[serde(default)]
    pub changes: Vec<KnowledgeSourceChange>,
}

pub trait KnowledgeChangeProvider {
    fn horizon(&self, since_cursor: Option<u64>) -> Result<KnowledgeChangeHorizon>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeDependency {
    pub dependent: ResourceRef,
    pub source: SourceRef,
    #[serde(default)]
    pub basis_revision: Option<SourceRevision>,
    pub relation: String,
    #[serde(default)]
    pub provenance_ref: Option<ResourceRef>,
    #[serde(default)]
    pub integrative: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeFreshness {
    Fresh,
    BasisChanged,
    IntegrationPending,
    BasisUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeAffectedResource {
    pub resource: ResourceRef,
    pub source: SourceRef,
    pub relation: String,
    #[serde(default)]
    pub provenance_ref: Option<ResourceRef>,
    #[serde(default)]
    pub basis_revision: Option<SourceRevision>,
    #[serde(default)]
    pub observed_revision: Option<SourceRevision>,
    pub freshness: KnowledgeFreshness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeImpact {
    pub version: String,
    pub horizon_cursor: u64,
    #[serde(default)]
    pub changed_sources: Vec<SourceRef>,
    #[serde(default)]
    pub affected: Vec<KnowledgeAffectedResource>,
    /// This deterministic operation never invokes an Agent or model.
    pub automatic_agent_or_model_invocation: bool,
}

pub fn deterministic_knowledge_impact(
    horizon: &KnowledgeChangeHorizon,
    dependencies: &[KnowledgeDependency],
) -> Result<KnowledgeImpact> {
    let observed = horizon
        .sources
        .iter()
        .map(|source| (source.source.clone(), source))
        .collect::<BTreeMap<_, _>>();
    let changed_sources = horizon
        .changes
        .iter()
        .map(|change| change.source.clone())
        .collect::<BTreeSet<_>>();

    let mut affected = Vec::new();
    for dependency in dependencies {
        if dependency.relation.trim().is_empty() {
            return Err(AikitError::new(
                "knowledge.living_dependency_relation_empty",
                "Living Knowledge dependency relation must be explicit",
            )
            .with("resource", dependency.dependent.to_string()));
        }
        let current = observed.get(&dependency.source).copied();
        let observed_revision = current.and_then(|value| value.revision.clone());
        let freshness = match current {
            None => KnowledgeFreshness::BasisUnavailable,
            Some(value) if !value.available => KnowledgeFreshness::BasisUnavailable,
            Some(_) if dependency.basis_revision == observed_revision => KnowledgeFreshness::Fresh,
            Some(_) if dependency.integrative => KnowledgeFreshness::IntegrationPending,
            Some(_) => KnowledgeFreshness::BasisChanged,
        };
        if freshness != KnowledgeFreshness::Fresh {
            affected.push(KnowledgeAffectedResource {
                resource: dependency.dependent.clone(),
                source: dependency.source.clone(),
                relation: dependency.relation.clone(),
                provenance_ref: dependency.provenance_ref.clone(),
                basis_revision: dependency.basis_revision.clone(),
                observed_revision,
                freshness,
            });
        }
    }
    affected.sort_by(|left, right| {
        left.resource
            .cmp(&right.resource)
            .then(left.source.cmp(&right.source))
            .then(left.relation.cmp(&right.relation))
    });

    Ok(KnowledgeImpact {
        version: LIVING_KNOWLEDGE_VERSION.into(),
        horizon_cursor: horizon.cursor,
        changed_sources: changed_sources.into_iter().collect(),
        affected,
        automatic_agent_or_model_invocation: false,
    })
}

impl<'a> KnowledgeApplication<'a> {
    /// Extend the existing KnowledgeApplication; this does not create a second knowledge store.
    pub fn living_impact(
        &self,
        horizon: &KnowledgeChangeHorizon,
        dependencies: &[KnowledgeDependency],
    ) -> Result<KnowledgeImpact> {
        deterministic_knowledge_impact(horizon, dependencies)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingBasisNode {
    pub resource: ResourceRef,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default)]
    pub source_revision: Option<SourceRevision>,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingBasisEdge {
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingReturnPath {
    pub from_basis: ResourceRef,
    #[serde(default)]
    pub through: Vec<ResourceRef>,
    pub to_whole: ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntegrativeWikiReading {
    pub reading: WikiReading,
    #[serde(default)]
    pub basis: Vec<ReadingBasisNode>,
    #[serde(default)]
    pub relations: Vec<ReadingBasisEdge>,
    #[serde(default)]
    pub return_paths: Vec<ReadingReturnPath>,
    pub freshness: KnowledgeFreshness,
}

fn validate_basis_dag(nodes: &[ReadingBasisNode], edges: &[ReadingBasisEdge]) -> Result<()> {
    let ids = nodes
        .iter()
        .map(|node| node.resource.clone())
        .collect::<BTreeSet<_>>();
    if ids.len() != nodes.len() {
        return Err(AikitError::new(
            "knowledge.living_duplicate_basis",
            "integrative reading basis resources must be unique",
        ));
    }

    let mut indegree = ids
        .iter()
        .map(|id| (id.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<ResourceRef, Vec<ResourceRef>>::new();
    for edge in edges {
        if edge.relation.trim().is_empty() {
            return Err(AikitError::new(
                "knowledge.living_relation_empty",
                "integrative reading relations must be explicit",
            ));
        }
        if !ids.contains(&edge.from) || !ids.contains(&edge.to) {
            return Err(AikitError::new(
                "knowledge.living_relation_outside_basis",
                "integrative reading relation endpoint is not in its basis",
            ));
        }
        *indegree.get_mut(&edge.to).expect("endpoint checked") += 1;
        outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
    }

    let mut queue = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(current) = queue.pop_front() {
        visited += 1;
        for next in outgoing.get(&current).into_iter().flatten() {
            let degree = indegree.get_mut(next).expect("endpoint checked");
            *degree -= 1;
            if *degree == 0 {
                queue.push_back(next.clone());
            }
        }
    }
    if visited != ids.len() {
        return Err(AikitError::new(
            "knowledge.living_basis_cycle",
            "integrative reading basis must remain an acyclic dependency graph",
        ));
    }
    Ok(())
}

fn semantic_revision_matches(value: &SemanticRevision, revision: &SourceRevision) -> bool {
    matches!(value, SemanticRevision::Text(text) if text == revision.as_str())
}

pub fn build_integrative_reading(
    mut reading: WikiReading,
    basis: Vec<ReadingBasisNode>,
    relations: Vec<ReadingBasisEdge>,
    return_paths: Vec<ReadingReturnPath>,
    freshness: KnowledgeFreshness,
) -> Result<IntegrativeWikiReading> {
    validate_basis_dag(&basis, &relations)?;

    for node in &basis {
        if node.source.is_some() != node.source_revision.is_some() {
            return Err(AikitError::new(
                "knowledge.living_incomplete_basis_revision",
                "source-backed reading basis requires both SourceRef and exact SourceRevision",
            )
            .with("basis", node.resource.to_string()));
        }
        if let (Some(source), Some(revision)) = (&node.source, &node.source_revision) {
            let exact = reading.provenance.iter().any(|entry| {
                entry.source_ref == *source
                    && entry
                        .source_revision
                        .as_ref()
                        .is_some_and(|value| semantic_revision_matches(value, revision))
            });
            if !exact {
                return Err(AikitError::new(
                    "knowledge.living_basis_provenance_missing",
                    "source-backed integrative basis must appear in WikiReading exact provenance",
                )
                .with("basis", node.resource.to_string())
                .with("source", source.to_string()));
            }
        }
        let returns = return_paths
            .iter()
            .filter(|path| path.from_basis == node.resource && path.to_whole == reading.ref_id)
            .count();
        if returns == 0 {
            return Err(AikitError::new(
                "knowledge.living_return_path_missing",
                "every integrative basis resource must retain a reversible path to the reading whole",
            )
            .with("basis", node.resource.to_string()));
        }
    }

    let extension = json!({
        "basis": basis,
        "relations": relations,
        "return_paths": return_paths,
        "freshness": freshness,
        "topology": "recursive-dag",
    });
    reading
        .extensions
        .insert(INTEGRATIVE_READING_EXTENSION.into(), extension);
    WikiObject::Reading(reading.clone()).validate()?;

    Ok(IntegrativeWikiReading {
        reading,
        basis,
        relations,
        return_paths,
        freshness,
    })
}

#[derive(Debug)]
pub struct ContemplateRequest<'a> {
    pub project: ProjectRef,
    pub focus: Vec<ResourceRef>,
    pub horizon: &'a KnowledgeChangeHorizon,
    pub dependencies: &'a [KnowledgeDependency],
    pub current_wiki_objects: &'a [WikiObject],
    pub runtime: &'a ModelRuntimeReadModel,
    pub method: Option<&'a Method>,
    pub ql: Option<QlRefractionRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContemplatePreflight {
    pub version: String,
    pub project: ProjectRef,
    pub focus: Vec<ResourceRef>,
    pub impact: KnowledgeImpact,
    pub runtime_model: ResourceRef,
    pub harness: ResourceRef,
    #[serde(default)]
    pub agent: Option<ResourceRef>,
    #[serde(default)]
    pub agency: Option<ResourceRef>,
    #[serde(default)]
    pub method: Option<ResourceRef>,
    #[serde(default)]
    pub ql: Option<QlRefractionRequest>,
    /// Explicit invariant: deterministic preflight never invokes an Agent/model.
    pub automatic_agent_or_model_invocation: bool,
}

pub fn contemplate_preflight(request: &ContemplateRequest<'_>) -> Result<ContemplatePreflight> {
    if let Some(method) = request.method {
        method.validate()?;
    }
    let impact = deterministic_knowledge_impact(request.horizon, request.dependencies)?;
    Ok(ContemplatePreflight {
        version: LIVING_KNOWLEDGE_VERSION.into(),
        project: request.project.clone(),
        focus: request.focus.clone(),
        impact,
        runtime_model: request.runtime.relation.model.model.clone(),
        harness: request.runtime.harness.clone(),
        agent: request.runtime.agent.clone(),
        agency: request.runtime.agency.clone(),
        method: request.method.map(|method| method.id.clone()),
        ql: request.ql.clone(),
        automatic_agent_or_model_invocation: false,
    })
}

impl<'a> KnowledgeApplication<'a> {
    pub fn contemplate_preflight(
        &self,
        request: &ContemplateRequest<'_>,
    ) -> Result<ContemplatePreflight> {
        contemplate_preflight(request)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContemplateGenerated {
    pub wiki_upserts: Vec<WikiObject>,
    pub integrative_readings: Vec<IntegrativeWikiReading>,
    pub candidates: Vec<String>,
    pub tensions: Vec<String>,
    /// Proposal-only type owned by the existing ProjectCentral return semantics.
    pub human_source_proposals: Vec<HumanSourceRevisionProposal>,
}

pub trait ContemplateExecutor {
    /// The one explicit Agent/model execution seam for Living Knowledge.
    fn execute(&mut self, preflight: &ContemplatePreflight) -> Result<ContemplateGenerated>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContemplateOutcome {
    pub preflight: ContemplatePreflight,
    pub agent_wiki: AgentWikiMaintenancePlan,
    pub integrative_readings: Vec<IntegrativeWikiReading>,
    pub candidates: Vec<String>,
    pub tensions: Vec<String>,
}

pub fn explicit_contemplate(
    request: &ContemplateRequest<'_>,
    executor: &mut dyn ContemplateExecutor,
) -> Result<ContemplateOutcome> {
    let preflight = contemplate_preflight(request)?;
    let generated = executor.execute(&preflight)?;

    let observed_source_revisions = request
        .horizon
        .sources
        .iter()
        .filter_map(|source| {
            source.revision.as_ref().map(|revision| {
                (
                    source.source.clone(),
                    SemanticRevision::Text(revision.as_str().to_owned()),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();

    // Reuse the accepted Agent-Wiki maintenance/return contract. Human source changes remain
    // proposals; this operation has no direct human-source write representation.
    let agent_wiki = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
        current_objects: request.current_wiki_objects.to_vec(),
        upserts: generated.wiki_upserts,
        observed_source_revisions,
        human_source_proposals: generated.human_source_proposals,
    })?;

    Ok(ContemplateOutcome {
        preflight,
        agent_wiki,
        integrative_readings: generated.integrative_readings,
        candidates: generated.candidates,
        tensions: generated.tensions,
    })
}

/// Convenience helper for producing exact provenance when an integrative reading consumes a
/// current source revision.
pub fn living_wiki_provenance(source: SourceRef, revision: SourceRevision) -> WikiProvenanceRef {
    WikiProvenanceRef {
        source_ref: source,
        source_revision: Some(SemanticRevision::Text(revision.as_str().to_owned())),
        producer_ref: None,
        generation_ref: None,
        extensions: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::RetractionMode;
    use crate::familiarity::FamiliarityContext;
    use crate::method::Method;
    use crate::model_runtime::{
        AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
        ModelAccessReading, ModelMaterialisationReading, ModelRuntimeReadModel,
        ModelRuntimeRelation, ModelSurfaceReading, ModelVariantReading, PlacementObservation,
        RuntimeChangeApplication,
    };
    use crate::ql::QlClientSubject;
    use crate::resource::ProviderRef;
    use std::collections::{BTreeMap, BTreeSet};

    fn resource(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }
    fn source(value: &str) -> SourceRef {
        SourceRef::parse(value).unwrap()
    }
    fn revision(value: &str) -> SourceRevision {
        SourceRevision::parse(value).unwrap()
    }

    fn horizon() -> KnowledgeChangeHorizon {
        KnowledgeChangeHorizon {
            provider: "central.source-change-horizon/v1".into(),
            cursor: 8,
            sources: vec![KnowledgeObservedSource {
                source: source("central:source:position"),
                revision: Some(revision("r2")),
                available: true,
            }],
            changes: vec![KnowledgeSourceChange {
                cursor: 8,
                world_ref: "project:example".into(),
                source: source("central:source:position"),
                roles: vec!["purpose".into()],
                provenance: "human-authored".into(),
                standing: "authored-human-position".into(),
                before_revision: Some(revision("r1")),
                after_revision: Some(revision("r2")),
                kind: KnowledgeChangeKind::Modified,
                agent_retrieval_allowed: true,
            }],
        }
    }

    fn dependencies() -> Vec<KnowledgeDependency> {
        vec![
            KnowledgeDependency {
                dependent: resource("wiki:node:purpose"),
                source: source("central:source:position"),
                basis_revision: Some(revision("r1")),
                relation: "derived-from".into(),
                provenance_ref: Some(resource("wiki:provenance:purpose")),
                integrative: false,
            },
            KnowledgeDependency {
                dependent: resource("wiki:reading:whole"),
                source: source("central:source:position"),
                basis_revision: Some(revision("r1")),
                relation: "integrates".into(),
                provenance_ref: None,
                integrative: true,
            },
        ]
    }

    fn runtime() -> ModelRuntimeReadModel {
        ModelRuntimeReadModel {
            version: "aikit.model-runtime/v1".into(),
            project: Some(resource("project:example")),
            agent: Some(resource("agent:epii")),
            agency: Some(resource("agency:knowledge")),
            harness: resource("harness:test"),
            agent_session: Some("session:test".into()),
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

    fn method() -> Method {
        Method {
            id: resource("method:living-wiki"),
            source: source("source:method:living-wiki"),
            revision: Some(revision("method-r1")),
            name: "Living Wiki contemplation".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![],
            actions: vec![],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![],
            expected_return_forms: vec!["integrative-reading".into()],
        }
    }

    #[test]
    fn deterministic_impact_is_relation_only_and_zero_agent() {
        let impact = deterministic_knowledge_impact(&horizon(), &dependencies()).unwrap();
        assert_eq!(impact.affected.len(), 2);
        assert_eq!(
            impact.affected[0].freshness,
            KnowledgeFreshness::BasisChanged
        );
        assert_eq!(
            impact.affected[1].freshness,
            KnowledgeFreshness::IntegrationPending
        );
        assert!(!impact.automatic_agent_or_model_invocation);
        assert_eq!(
            impact.changed_sources,
            vec![source("central:source:position")]
        );
    }

    #[test]
    fn unavailable_basis_is_truthful_instead_of_guessed() {
        let mut h = horizon();
        h.sources.clear();
        let impact = deterministic_knowledge_impact(&h, &dependencies()).unwrap();
        assert!(impact
            .affected
            .iter()
            .all(|item| item.freshness == KnowledgeFreshness::BasisUnavailable));
    }

    #[test]
    fn integrative_reading_preserves_exact_basis_and_reversible_return() {
        let src = source("central:source:position");
        let rev = revision("r2");
        let whole = resource("wiki:reading:whole");
        let basis_ref = resource("wiki:node:purpose");
        let reading = WikiReading {
            profile: "okf-wiki/v1".into(),
            ref_id: whole.clone(),
            revision: 2,
            provenance: vec![living_wiki_provenance(src.clone(), rev.clone())],
            frame_ref: resource("wiki:frame:project"),
            reading_type: "integrative-quilt".into(),
            artifact_ref: None,
            derived_by_ref: Some(resource("agent:epii")),
            extensions: BTreeMap::new(),
        };
        let built = build_integrative_reading(
            reading,
            vec![ReadingBasisNode {
                resource: basis_ref.clone(),
                source: Some(src),
                source_revision: Some(rev),
                roles: vec!["purpose".into()],
            }],
            vec![],
            vec![ReadingReturnPath {
                from_basis: basis_ref,
                through: vec![],
                to_whole: whole,
            }],
            KnowledgeFreshness::Fresh,
        )
        .unwrap();
        assert!(built
            .reading
            .extensions
            .contains_key(INTEGRATIVE_READING_EXTENSION));
    }

    #[test]
    fn integrative_reading_rejects_cycles_and_missing_return() {
        let a = resource("wiki:a");
        let b = resource("wiki:b");
        let reading = WikiReading {
            profile: "okf-wiki/v1".into(),
            ref_id: resource("wiki:whole"),
            revision: 1,
            provenance: vec![],
            frame_ref: resource("wiki:frame"),
            reading_type: "quilt".into(),
            artifact_ref: None,
            derived_by_ref: None,
            extensions: BTreeMap::new(),
        };
        let result = build_integrative_reading(
            reading,
            vec![
                ReadingBasisNode {
                    resource: a.clone(),
                    source: None,
                    source_revision: None,
                    roles: vec![],
                },
                ReadingBasisNode {
                    resource: b.clone(),
                    source: None,
                    source_revision: None,
                    roles: vec![],
                },
            ],
            vec![
                ReadingBasisEdge {
                    from: a.clone(),
                    to: b.clone(),
                    relation: "supports".into(),
                },
                ReadingBasisEdge {
                    from: b,
                    to: a,
                    relation: "depends-on".into(),
                },
            ],
            vec![],
            KnowledgeFreshness::Fresh,
        );
        assert_eq!(result.unwrap_err().code(), "knowledge.living_basis_cycle");
    }

    struct CountingExecutor {
        calls: usize,
    }
    impl ContemplateExecutor for CountingExecutor {
        fn execute(&mut self, _preflight: &ContemplatePreflight) -> Result<ContemplateGenerated> {
            self.calls += 1;
            Ok(ContemplateGenerated {
                wiki_upserts: vec![],
                integrative_readings: vec![],
                candidates: vec!["candidate:re-read purpose in whole".into()],
                tensions: vec!["standing wording versus implementation evidence".into()],
                human_source_proposals: vec![HumanSourceRevisionProposal {
                    source: source("central:source:position"),
                    reason: "return this tension for human Recognition".into(),
                    evidence: vec![source("source:evidence:test")],
                }],
            })
        }
    }

    #[test]
    fn only_explicit_contemplate_crosses_agent_model_execution_seam() {
        let h = horizon();
        let deps = dependencies();
        let rt = runtime();
        let m = method();
        let request = ContemplateRequest {
            project: ProjectRef::parse("example/project").unwrap(),
            focus: vec![resource("wiki:reading:whole")],
            horizon: &h,
            dependencies: &deps,
            current_wiki_objects: &[],
            runtime: &rt,
            method: Some(&m),
            ql: None,
        };
        let application = KnowledgeApplication::new(FamiliarityContext::default());
        let preflight = application.contemplate_preflight(&request).unwrap();
        assert!(!preflight.automatic_agent_or_model_invocation);
        let mut executor = CountingExecutor { calls: 0 };
        assert_eq!(executor.calls, 0);
        let outcome = explicit_contemplate(&request, &mut executor).unwrap();
        assert_eq!(executor.calls, 1);
        assert_eq!(outcome.agent_wiki.human_source_proposals.len(), 1);
        assert_eq!(outcome.agent_wiki.next_objects.len(), 0);
    }

    #[test]
    fn ql_is_optional_additive_preflight_depth() {
        let h = horizon();
        let deps = dependencies();
        let rt = runtime();
        let ql = QlRefractionRequest::new(
            QlClientSubject::new(resource("wiki:reading:whole"), Some("2".into())),
            "living-wiki",
        );
        let with_ql = ContemplateRequest {
            project: ProjectRef::parse("example/project").unwrap(),
            focus: vec![],
            horizon: &h,
            dependencies: &deps,
            current_wiki_objects: &[],
            runtime: &rt,
            method: None,
            ql: Some(ql),
        };
        let without_ql = ContemplateRequest {
            ql: None,
            ..with_ql
        };
        let ordinary = contemplate_preflight(&without_ql).unwrap();
        assert!(ordinary.ql.is_none());
        assert_eq!(ordinary.impact.affected.len(), 2);
    }
}
