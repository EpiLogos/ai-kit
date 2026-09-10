//! Bounded relational completion for Living Knowledge.
//!
//! This module extends the existing `KnowledgeApplication`/Living Knowledge contract. It does
//! not introduce a second graph, watcher, history store, or Agent runtime. Source revision truth
//! remains provider-owned; the relations below only follow explicit dependency manifests and
//! preserve one inspectable path from a moved source through dependent knowledge/readings.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge_living::{
    build_integrative_reading, contemplate_preflight, ContemplatePreflight, ContemplateRequest,
    IntegrativeWikiReading, KnowledgeChangeHorizon, KnowledgeDependency, KnowledgeFreshness,
    KnowledgeImpact, ReadingBasisEdge, ReadingBasisNode, ReadingReturnPath,
    INTEGRATIVE_READING_EXTENSION, LIVING_KNOWLEDGE_VERSION,
};
use crate::knowledge_navigation::KnowledgeApplication;
use crate::knowledge_wiki::{SemanticRevision, WikiObject, WikiReading};
use crate::method::Method;
use crate::model_runtime::ModelRuntimeReadModel;
use crate::project::ProjectRef;
use crate::ql::QlRefractionRequest;
use crate::resource::{ResourceRef, SourceRef};
use crate::{AikitError, Result};

pub const LIVING_RELATIONS_VERSION: &str = "aikit.living-knowledge-relations/v1";
pub const DEFAULT_LIVING_IMPACT_DEPTH: usize = 8;
pub const DEFAULT_LIVING_IMPACT_RESOURCES: usize = 512;

fn default_impact_depth() -> usize {
    DEFAULT_LIVING_IMPACT_DEPTH
}

fn default_impact_resources() -> usize {
    DEFAULT_LIVING_IMPACT_RESOURCES
}

/// Explicit reading/subject dependency. `basis -> dependent` is a basis dependency relation,
/// not an arbitrary semantic Wiki edge. Cycles in this manifest are therefore invalid even when
/// the underlying Wiki legitimately contains semantic cycles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeResourceDependency {
    pub basis: ResourceRef,
    pub dependent: ResourceRef,
    pub relation: String,
    #[serde(default)]
    pub provenance_ref: Option<ResourceRef>,
    #[serde(default)]
    pub integrative: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "ref", rename_all = "kebab-case")]
pub enum KnowledgeImpactRef {
    Source(SourceRef),
    Resource(ResourceRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeImpactStep {
    pub from: KnowledgeImpactRef,
    pub to: ResourceRef,
    pub relation: String,
    #[serde(default)]
    pub provenance_ref: Option<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeImpactPath {
    pub resource: ResourceRef,
    pub root_source: SourceRef,
    pub freshness: KnowledgeFreshness,
    pub steps: Vec<KnowledgeImpactStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeTransitiveAffected {
    pub resource: ResourceRef,
    pub root_source: SourceRef,
    pub relation: String,
    #[serde(default)]
    pub provenance_ref: Option<ResourceRef>,
    pub freshness: KnowledgeFreshness,
}

/// Portable input for deterministic/headless Living Knowledge impact queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeImpactRequest {
    pub horizon: KnowledgeChangeHorizon,
    #[serde(default)]
    pub source_dependencies: Vec<KnowledgeDependency>,
    #[serde(default)]
    pub resource_dependencies: Vec<KnowledgeResourceDependency>,
    #[serde(default = "default_impact_depth")]
    pub max_depth: usize,
    #[serde(default = "default_impact_resources")]
    pub max_affected: usize,
}

impl KnowledgeImpactRequest {
    pub fn evaluate(&self) -> Result<KnowledgeTransitiveImpact> {
        deterministic_transitive_knowledge_impact(
            &self.horizon,
            &self.source_dependencies,
            &self.resource_dependencies,
            self.max_depth,
            self.max_affected,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeTransitiveImpact {
    pub version: String,
    pub horizon_cursor: u64,
    pub direct: KnowledgeImpact,
    #[serde(default)]
    pub transitive: Vec<KnowledgeTransitiveAffected>,
    #[serde(default)]
    pub paths: Vec<KnowledgeImpactPath>,
    #[serde(default)]
    pub pending_integration: Vec<ResourceRef>,
    pub truncated: bool,
    /// Deterministic impact closure never invokes an Agent or model.
    pub automatic_agent_or_model_invocation: bool,
}

fn validate_resource_dependency_dag(dependencies: &[KnowledgeResourceDependency]) -> Result<()> {
    let mut nodes = BTreeSet::new();
    let mut indegree = BTreeMap::<ResourceRef, usize>::new();
    let mut outgoing = BTreeMap::<ResourceRef, Vec<ResourceRef>>::new();

    for dependency in dependencies {
        if dependency.relation.trim().is_empty() {
            return Err(AikitError::new(
                "knowledge.living_resource_dependency_relation_empty",
                "Living Knowledge resource dependency relation must be explicit",
            )
            .with("basis", dependency.basis.to_string())
            .with("dependent", dependency.dependent.to_string()));
        }
        if dependency.basis == dependency.dependent {
            return Err(AikitError::new(
                "knowledge.living_resource_dependency_cycle",
                "Living Knowledge reading dependency cannot depend on itself",
            )
            .with("resource", dependency.basis.to_string()));
        }
        nodes.insert(dependency.basis.clone());
        nodes.insert(dependency.dependent.clone());
        outgoing
            .entry(dependency.basis.clone())
            .or_default()
            .push(dependency.dependent.clone());
        *indegree.entry(dependency.dependent.clone()).or_default() += 1;
        indegree.entry(dependency.basis.clone()).or_default();
    }

    let mut queue = indegree
        .iter()
        .filter_map(|(resource, degree)| (*degree == 0).then_some(resource.clone()))
        .collect::<VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(resource) = queue.pop_front() {
        visited += 1;
        for dependent in outgoing.get(&resource).into_iter().flatten() {
            let degree = indegree
                .get_mut(dependent)
                .expect("resource dependency endpoint indexed");
            *degree -= 1;
            if *degree == 0 {
                queue.push_back(dependent.clone());
            }
        }
    }
    if visited != nodes.len() {
        return Err(AikitError::new(
            "knowledge.living_resource_dependency_cycle",
            "Living Knowledge reading dependency manifest contains an indirect cycle",
        ));
    }
    Ok(())
}

/// Follow source revision differences through explicit resource/reading basis dependencies.
///
/// The closure is deterministic, bounded and path-bearing. It deliberately does not follow every
/// semantic Wiki edge: only `KnowledgeResourceDependency` relations supplied by the current
/// Knowledge field establish transitive impact.
pub fn deterministic_transitive_knowledge_impact(
    horizon: &KnowledgeChangeHorizon,
    source_dependencies: &[KnowledgeDependency],
    resource_dependencies: &[KnowledgeResourceDependency],
    max_depth: usize,
    max_affected: usize,
) -> Result<KnowledgeTransitiveImpact> {
    if max_depth == 0 || max_affected == 0 {
        return Err(AikitError::new(
            "knowledge.living_impact_budget_invalid",
            "Living Knowledge impact budgets must both be greater than zero",
        ));
    }
    validate_resource_dependency_dag(resource_dependencies)?;

    let mut direct = crate::knowledge_living::deterministic_knowledge_impact(
        horizon,
        source_dependencies,
    )?;
    let mut truncated = direct.affected.len() > max_affected;
    direct.affected.truncate(max_affected);

    let mut source_dependencies = source_dependencies.to_vec();
    source_dependencies.sort_by(|left, right| {
        left.dependent
            .cmp(&right.dependent)
            .then(left.source.cmp(&right.source))
            .then(left.relation.cmp(&right.relation))
    });
    let mut resource_dependencies = resource_dependencies.to_vec();
    resource_dependencies.sort_by(|left, right| {
        left.basis
            .cmp(&right.basis)
            .then(left.dependent.cmp(&right.dependent))
            .then(left.relation.cmp(&right.relation))
    });

    let mut seen = direct
        .affected
        .iter()
        .map(|affected| affected.resource.clone())
        .collect::<BTreeSet<_>>();
    let mut paths = Vec::new();
    let mut queue = VecDeque::<(ResourceRef, SourceRef, Vec<KnowledgeImpactStep>, usize)>::new();

    for affected in &direct.affected {
        let dependency = source_dependencies.iter().find(|dependency| {
            dependency.dependent == affected.resource
                && dependency.source == affected.source
                && dependency.relation == affected.relation
        });
        let step = KnowledgeImpactStep {
            from: KnowledgeImpactRef::Source(affected.source.clone()),
            to: affected.resource.clone(),
            relation: affected.relation.clone(),
            provenance_ref: dependency.and_then(|value| value.provenance_ref.clone()),
        };
        let steps = vec![step];
        paths.push(KnowledgeImpactPath {
            resource: affected.resource.clone(),
            root_source: affected.source.clone(),
            freshness: affected.freshness,
            steps: steps.clone(),
        });
        queue.push_back((
            affected.resource.clone(),
            affected.source.clone(),
            steps,
            0,
        ));
    }

    let mut transitive = Vec::new();
    while let Some((basis, root_source, path, depth)) = queue.pop_front() {
        let outgoing = resource_dependencies
            .iter()
            .filter(|dependency| dependency.basis == basis)
            .collect::<Vec<_>>();
        if outgoing.is_empty() {
            continue;
        }
        if depth >= max_depth {
            truncated = true;
            continue;
        }
        for dependency in outgoing {
            if seen.contains(&dependency.dependent) {
                continue;
            }
            if seen.len() >= max_affected {
                truncated = true;
                break;
            }
            let freshness = if dependency.integrative {
                KnowledgeFreshness::IntegrationPending
            } else {
                KnowledgeFreshness::BasisChanged
            };
            let mut next_path = path.clone();
            next_path.push(KnowledgeImpactStep {
                from: KnowledgeImpactRef::Resource(dependency.basis.clone()),
                to: dependency.dependent.clone(),
                relation: dependency.relation.clone(),
                provenance_ref: dependency.provenance_ref.clone(),
            });
            seen.insert(dependency.dependent.clone());
            transitive.push(KnowledgeTransitiveAffected {
                resource: dependency.dependent.clone(),
                root_source: root_source.clone(),
                relation: dependency.relation.clone(),
                provenance_ref: dependency.provenance_ref.clone(),
                freshness,
            });
            paths.push(KnowledgeImpactPath {
                resource: dependency.dependent.clone(),
                root_source: root_source.clone(),
                freshness,
                steps: next_path.clone(),
            });
            queue.push_back((
                dependency.dependent.clone(),
                root_source.clone(),
                next_path,
                depth + 1,
            ));
        }
    }

    transitive.sort_by(|left, right| left.resource.cmp(&right.resource));
    paths.sort_by(|left, right| left.resource.cmp(&right.resource));
    let mut pending_integration = direct
        .affected
        .iter()
        .filter(|affected| affected.freshness == KnowledgeFreshness::IntegrationPending)
        .map(|affected| affected.resource.clone())
        .chain(
            transitive
                .iter()
                .filter(|affected| affected.freshness == KnowledgeFreshness::IntegrationPending)
                .map(|affected| affected.resource.clone()),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    pending_integration.sort();

    Ok(KnowledgeTransitiveImpact {
        version: LIVING_RELATIONS_VERSION.into(),
        horizon_cursor: horizon.cursor,
        direct,
        transitive,
        paths,
        pending_integration,
        truncated,
        automatic_agent_or_model_invocation: false,
    })
}

impl KnowledgeApplication<'_> {
    pub fn living_transitive_impact(
        &self,
        request: &KnowledgeImpactRequest,
    ) -> Result<KnowledgeTransitiveImpact> {
        request.evaluate()
    }
}

/// Exact semantic revision of each resource participating in an integrative reading basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingBasisRevision {
    pub resource: ResourceRef,
    pub revision: SemanticRevision,
}

fn validate_basis_revisions(
    basis: &[ReadingBasisNode],
    revisions: &[ReadingBasisRevision],
) -> Result<()> {
    let basis_refs = basis
        .iter()
        .map(|node| node.resource.clone())
        .collect::<BTreeSet<_>>();
    let revision_refs = revisions
        .iter()
        .map(|revision| revision.resource.clone())
        .collect::<BTreeSet<_>>();
    if revisions.len() != revision_refs.len() || basis_refs != revision_refs {
        return Err(AikitError::new(
            "knowledge.living_basis_revision_mismatch",
            "every integrative basis resource requires exactly one recorded semantic revision",
        ));
    }
    for revision in revisions {
        if matches!(&revision.revision, SemanticRevision::Text(value) if value.trim().is_empty()) {
            return Err(AikitError::new(
                "knowledge.living_basis_revision_empty",
                "textual integrative basis revisions must be non-empty",
            )
            .with("resource", revision.resource.to_string()));
        }
    }
    Ok(())
}

/// Build the existing canonical `WikiReading`, then attach exact resource-basis revisions and the
/// provider/change cursor actually integrated. This augments the accepted reading extension;
/// identity continues to be the `WikiReading` ref/revision.
pub fn build_revisioned_integrative_reading(
    reading: WikiReading,
    basis: Vec<ReadingBasisNode>,
    basis_revisions: Vec<ReadingBasisRevision>,
    relations: Vec<ReadingBasisEdge>,
    return_paths: Vec<ReadingReturnPath>,
    freshness: KnowledgeFreshness,
    integrated_through_cursor: Option<u64>,
) -> Result<IntegrativeWikiReading> {
    validate_basis_revisions(&basis, &basis_revisions)?;
    let mut built = build_integrative_reading(reading, basis, relations, return_paths, freshness)?;
    let extension = built
        .reading
        .extensions
        .get_mut(INTEGRATIVE_READING_EXTENSION)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.living_integrative_extension_missing",
                "canonical integrative reading extension was not materialised",
            )
        })?;
    extension.insert("basis_revisions".into(), json!(basis_revisions));
    extension.insert(
        "integrated_through_cursor".into(),
        json!(integrated_through_cursor),
    );
    WikiObject::Reading(built.reading.clone()).validate()?;
    Ok(built)
}

pub fn integrative_basis_refs(reading: &IntegrativeWikiReading) -> Vec<ResourceRef> {
    let mut refs = reading
        .basis
        .iter()
        .map(|basis| basis.resource.clone())
        .collect::<Vec<_>>();
    refs.sort();
    refs
}

pub fn integrating_readings(
    part: &ResourceRef,
    readings: &[IntegrativeWikiReading],
) -> Vec<ResourceRef> {
    let mut refs = readings
        .iter()
        .filter(|reading| reading.basis.iter().any(|basis| &basis.resource == part))
        .map(|reading| reading.reading.ref_id.clone())
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    refs
}

pub fn integrated_through_cursor(reading: &IntegrativeWikiReading) -> Option<u64> {
    reading
        .reading
        .extensions
        .get(INTEGRATIVE_READING_EXTENSION)
        .and_then(|value| value.get("integrated_through_cursor"))
        .and_then(Value::as_u64)
}

/// A reading becomes freshly reintegrated only through a new reading revision. Rewriting basis
/// metadata on the same revision is rejected.
pub fn validate_reintegration(
    previous: &IntegrativeWikiReading,
    next: &IntegrativeWikiReading,
) -> Result<()> {
    if previous.reading.ref_id != next.reading.ref_id {
        return Err(AikitError::new(
            "knowledge.living_reintegration_identity_changed",
            "reintegrating a reading cannot silently change its canonical identity",
        ));
    }
    if next.reading.revision <= previous.reading.revision {
        return Err(AikitError::new(
            "knowledge.living_reintegration_revision_not_advanced",
            "a freshly integrated reading must advance the canonical WikiReading revision",
        )
        .with("resource", next.reading.ref_id.to_string()));
    }
    if let (Some(previous_cursor), Some(next_cursor)) = (
        integrated_through_cursor(previous),
        integrated_through_cursor(next),
    ) {
        if next_cursor < previous_cursor {
            return Err(AikitError::new(
                "knowledge.living_reintegration_cursor_regressed",
                "integrated-through cursor cannot move backwards",
            ));
        }
    }
    Ok(())
}

/// Owned, serializable form of deterministic Contemplate preflight for CLI/Agent/desktop callers.
/// It remains a preflight: no `ContemplateExecutor` is available or invoked here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableContemplatePreflightRequest {
    pub project: ProjectRef,
    #[serde(default)]
    pub focus: Vec<ResourceRef>,
    pub horizon: KnowledgeChangeHorizon,
    #[serde(default)]
    pub source_dependencies: Vec<KnowledgeDependency>,
    #[serde(default)]
    pub resource_dependencies: Vec<KnowledgeResourceDependency>,
    pub runtime: ModelRuntimeReadModel,
    #[serde(default)]
    pub method: Option<Method>,
    #[serde(default)]
    pub ql: Option<QlRefractionRequest>,
    #[serde(default = "default_impact_depth")]
    pub max_depth: usize,
    #[serde(default = "default_impact_resources")]
    pub max_affected: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableContemplatePreflight {
    pub version: String,
    pub base: ContemplatePreflight,
    pub transitive_impact: KnowledgeTransitiveImpact,
    /// Explicit invariant: portable preflight does not cross the Agent/model execution seam.
    pub automatic_agent_or_model_invocation: bool,
}

pub fn portable_contemplate_preflight(
    request: &PortableContemplatePreflightRequest,
) -> Result<PortableContemplatePreflight> {
    let base_request = ContemplateRequest {
        project: request.project.clone(),
        focus: request.focus.clone(),
        horizon: &request.horizon,
        dependencies: &request.source_dependencies,
        current_wiki_objects: &[],
        runtime: &request.runtime,
        method: request.method.as_ref(),
        ql: request.ql.clone(),
    };
    let base = contemplate_preflight(&base_request)?;
    let transitive_impact = deterministic_transitive_knowledge_impact(
        &request.horizon,
        &request.source_dependencies,
        &request.resource_dependencies,
        request.max_depth,
        request.max_affected,
    )?;
    Ok(PortableContemplatePreflight {
        version: LIVING_KNOWLEDGE_VERSION.into(),
        base,
        transitive_impact,
        automatic_agent_or_model_invocation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::composition::RetractionMode;
    use crate::knowledge_living::{
        living_wiki_provenance, KnowledgeChangeKind, KnowledgeObservedSource,
        KnowledgeSourceChange,
    };
    use crate::model_runtime::{
        AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
        ModelAccessReading, ModelMaterialisationReading, ModelRuntimeRelation, ModelSurfaceReading,
        ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
    };
    use crate::resource::{ProviderRef, SourceRevision};

    fn resource(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn source(value: &str) -> SourceRef {
        SourceRef::parse(value).unwrap()
    }

    fn revision(value: &str) -> SourceRevision {
        SourceRevision::parse(value).unwrap()
    }

    fn horizon(provider: &str, changes: bool) -> KnowledgeChangeHorizon {
        let src = source("source:project:position");
        KnowledgeChangeHorizon {
            provider: provider.into(),
            cursor: if changes { 8 } else { 0 },
            sources: vec![KnowledgeObservedSource {
                source: src.clone(),
                revision: Some(revision("r2")),
                available: true,
            }],
            changes: if changes {
                vec![KnowledgeSourceChange {
                    cursor: 8,
                    world_ref: "project:example".into(),
                    source: src,
                    roles: vec!["purpose".into()],
                    provenance: "human-authored".into(),
                    standing: "authored-human-position".into(),
                    before_revision: Some(revision("r1")),
                    after_revision: Some(revision("r2")),
                    kind: KnowledgeChangeKind::Modified,
                    agent_retrieval_allowed: true,
                }]
            } else {
                vec![]
            },
        }
    }

    fn source_dependencies() -> Vec<KnowledgeDependency> {
        vec![
            KnowledgeDependency {
                dependent: resource("wiki:node:purpose"),
                source: source("source:project:position"),
                basis_revision: Some(revision("r1")),
                relation: "grounded-in".into(),
                provenance_ref: Some(resource("wiki:provenance:purpose")),
                integrative: false,
            },
            KnowledgeDependency {
                dependent: resource("wiki:node:unrelated"),
                source: source("source:project:position"),
                basis_revision: Some(revision("r2")),
                relation: "mentions".into(),
                provenance_ref: None,
                integrative: false,
            },
        ]
    }

    fn reading_dependencies() -> Vec<KnowledgeResourceDependency> {
        vec![
            KnowledgeResourceDependency {
                basis: resource("wiki:node:purpose"),
                dependent: resource("wiki:reading:section"),
                relation: "integrates-basis".into(),
                provenance_ref: None,
                integrative: true,
            },
            KnowledgeResourceDependency {
                basis: resource("wiki:reading:section"),
                dependent: resource("wiki:reading:essay"),
                relation: "integrates-reading".into(),
                provenance_ref: Some(resource("wiki:provenance:essay")),
                integrative: true,
            },
        ]
    }

    #[test]
    fn transitive_impact_crosses_multiple_reading_levels_with_paths() {
        let impact = deterministic_transitive_knowledge_impact(
            &horizon("central.source-change-horizon/v1", true),
            &source_dependencies(),
            &reading_dependencies(),
            8,
            32,
        )
        .unwrap();
        assert_eq!(impact.direct.affected.len(), 1);
        assert_eq!(impact.transitive.len(), 2);
        let essay = impact
            .paths
            .iter()
            .find(|path| path.resource == resource("wiki:reading:essay"))
            .unwrap();
        assert_eq!(essay.steps.len(), 3);
        assert_eq!(
            essay.steps.last().unwrap().relation,
            "integrates-reading"
        );
        assert_eq!(
            impact.pending_integration,
            vec![resource("wiki:reading:essay"), resource("wiki:reading:section")]
        );
        assert!(!impact.automatic_agent_or_model_invocation);
    }

    #[test]
    fn current_revision_only_provider_participates_without_change_stream_or_central_identity() {
        let impact = deterministic_transitive_knowledge_impact(
            &horizon("provider:git:current-revision-only", false),
            &source_dependencies(),
            &[],
            4,
            16,
        )
        .unwrap();
        assert_eq!(impact.direct.affected.len(), 1);
        assert!(impact.direct.changed_sources.is_empty());
        assert_eq!(impact.direct.affected[0].freshness, KnowledgeFreshness::BasisChanged);
    }

    #[test]
    fn dependency_manifest_cycles_fail_without_banning_semantic_wiki_cycles() {
        let dependencies = vec![
            KnowledgeResourceDependency {
                basis: resource("wiki:a"),
                dependent: resource("wiki:b"),
                relation: "basis".into(),
                provenance_ref: None,
                integrative: true,
            },
            KnowledgeResourceDependency {
                basis: resource("wiki:b"),
                dependent: resource("wiki:a"),
                relation: "basis".into(),
                provenance_ref: None,
                integrative: true,
            },
        ];
        let error = deterministic_transitive_knowledge_impact(
            &horizon("provider:test", true),
            &source_dependencies(),
            &dependencies,
            8,
            32,
        )
        .unwrap_err();
        assert_eq!(error.code(), "knowledge.living_resource_dependency_cycle");
    }

    fn revisioned_reading(
        revision_number: u64,
        cursor: u64,
    ) -> IntegrativeWikiReading {
        let source_ref = source("source:project:position");
        let source_revision = revision("r2");
        let basis_ref = resource("wiki:node:purpose");
        let whole = resource("wiki:reading:whole");
        build_revisioned_integrative_reading(
            WikiReading {
                profile: "okf-wiki/v1".into(),
                ref_id: whole.clone(),
                revision: revision_number,
                provenance: vec![living_wiki_provenance(
                    source_ref.clone(),
                    source_revision.clone(),
                )],
                frame_ref: resource("wiki:frame:project"),
                reading_type: "integrative-quilt".into(),
                artifact_ref: None,
                derived_by_ref: Some(resource("agent:epii")),
                extensions: BTreeMap::new(),
            },
            vec![ReadingBasisNode {
                resource: basis_ref.clone(),
                source: Some(source_ref),
                source_revision: Some(source_revision),
                roles: vec!["purpose".into()],
            }],
            vec![ReadingBasisRevision {
                resource: basis_ref.clone(),
                revision: SemanticRevision::Number(3),
            }],
            vec![],
            vec![ReadingReturnPath {
                from_basis: basis_ref,
                through: vec![],
                to_whole: whole,
            }],
            KnowledgeFreshness::Fresh,
            Some(cursor),
        )
        .unwrap()
    }

    #[test]
    fn revisioned_reading_is_reversible_and_cannot_refresh_by_metadata_only() {
        let first = revisioned_reading(1, 8);
        assert_eq!(
            integrative_basis_refs(&first),
            vec![resource("wiki:node:purpose")]
        );
        assert_eq!(integrated_through_cursor(&first), Some(8));
        assert_eq!(
            integrating_readings(&resource("wiki:node:purpose"), std::slice::from_ref(&first)),
            vec![resource("wiki:reading:whole")]
        );

        let same_revision = revisioned_reading(1, 9);
        assert_eq!(
            validate_reintegration(&first, &same_revision)
                .unwrap_err()
                .code(),
            "knowledge.living_reintegration_revision_not_advanced"
        );
        let advanced = revisioned_reading(2, 9);
        validate_reintegration(&first, &advanced).unwrap();
    }

    #[test]
    fn basis_revision_set_must_cover_every_basis_exactly_once() {
        let source_ref = source("source:test");
        let source_revision = revision("r1");
        let basis_ref = resource("wiki:basis");
        let whole = resource("wiki:whole");
        let result = build_revisioned_integrative_reading(
            WikiReading {
                profile: "okf-wiki/v1".into(),
                ref_id: whole.clone(),
                revision: 1,
                provenance: vec![living_wiki_provenance(source_ref.clone(), source_revision.clone())],
                frame_ref: resource("wiki:frame"),
                reading_type: "integrative".into(),
                artifact_ref: None,
                derived_by_ref: None,
                extensions: BTreeMap::new(),
            },
            vec![ReadingBasisNode {
                resource: basis_ref.clone(),
                source: Some(source_ref),
                source_revision: Some(source_revision),
                roles: vec![],
            }],
            vec![],
            vec![],
            vec![ReadingReturnPath {
                from_basis: basis_ref,
                through: vec![],
                to_whole: whole,
            }],
            KnowledgeFreshness::Fresh,
            Some(1),
        );
        assert_eq!(
            result.unwrap_err().code(),
            "knowledge.living_basis_revision_mismatch"
        );
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

    #[test]
    fn portable_preflight_includes_transitive_pending_without_invoking_agent() {
        let request = PortableContemplatePreflightRequest {
            project: ProjectRef::parse("example/project").unwrap(),
            focus: vec![resource("wiki:reading:essay")],
            horizon: horizon("central.source-change-horizon/v1", true),
            source_dependencies: source_dependencies(),
            resource_dependencies: reading_dependencies(),
            runtime: runtime(),
            method: None,
            ql: None,
            max_depth: 8,
            max_affected: 32,
        };
        let preflight = portable_contemplate_preflight(&request).unwrap();
        assert_eq!(preflight.transitive_impact.transitive.len(), 2);
        assert!(!preflight.base.automatic_agent_or_model_invocation);
        assert!(!preflight.automatic_agent_or_model_invocation);
    }
}
