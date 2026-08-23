//! Bounded deterministic context assembly for explicit Living Knowledge Contemplate.
//!
//! This is the relation between the accepted change/impact model and the
//! `ContemplateExecutor`: before the explicit Agent/model aperture is crossed,
//! AIKit resolves a small inspectable Wiki field from canonical object provenance,
//! integrative-reading basis dependencies, relevant semantic edges and the current
//! source-change horizon. It exposes refs/revisions/relations and disclosure state;
//! it does not retrieve changed source payloads or invoke an Agent/model.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::knowledge_living::{
    contemplate_preflight, explicit_contemplate, ContemplateExecutor, ContemplateGenerated,
    ContemplateOutcome, ContemplatePreflight, ContemplateRequest, KnowledgeDependency,
    INTEGRATIVE_READING_EXTENSION,
};
use crate::knowledge_living_relations::{
    deterministic_transitive_knowledge_impact, KnowledgeResourceDependency,
    KnowledgeTransitiveImpact, DEFAULT_LIVING_IMPACT_DEPTH, DEFAULT_LIVING_IMPACT_RESOURCES,
};
use crate::knowledge_wiki::{SemanticRevision, WikiObject, WikiProvenanceRef};
use crate::resource::{ResourceRef, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const CONTEMPLATE_FIELD_VERSION: &str = "aikit.contemplate-field/v1";
pub const DEFAULT_CONTEMPLATE_OBJECT_BUDGET: usize = 128;
pub const DEFAULT_CONTEMPLATE_RELATION_DEPTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContemplateFieldChange {
    pub cursor: u64,
    pub source: SourceRef,
    #[serde(default)]
    pub roles: Vec<String>,
    pub provenance: String,
    pub standing: String,
    #[serde(default)]
    pub before_revision: Option<SourceRevision>,
    #[serde(default)]
    pub after_revision: Option<SourceRevision>,
    pub agent_retrieval_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContemplateFieldObject {
    pub resource: ResourceRef,
    pub revision: u64,
    pub object_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContemplateFieldRelation {
    pub edge_ref: ResourceRef,
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContemplateFieldReturn {
    pub from_basis: ResourceRef,
    #[serde(default)]
    pub through: Vec<ResourceRef>,
    pub to_whole: ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContemplateFieldSource {
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_retrieval_allowed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContemplateContextField {
    pub version: String,
    #[serde(default)]
    pub focus: Vec<ResourceRef>,
    pub impact: KnowledgeTransitiveImpact,
    #[serde(default)]
    pub changes: Vec<ContemplateFieldChange>,
    #[serde(default)]
    pub objects: Vec<ContemplateFieldObject>,
    #[serde(default)]
    pub relations: Vec<ContemplateFieldRelation>,
    #[serde(default)]
    pub sources: Vec<ContemplateFieldSource>,
    #[serde(default)]
    pub returns: Vec<ContemplateFieldReturn>,
    #[serde(default)]
    pub tensions: Vec<String>,
    pub object_budget: usize,
    pub relation_depth: usize,
    pub truncated: bool,
    /// Deterministic assembly never retrieves source payloads solely because they changed.
    pub changed_source_payloads_retrieved: bool,
    /// Deterministic assembly never invokes an Agent/model.
    pub automatic_agent_or_model_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundedContemplatePreflight {
    pub version: String,
    pub base: ContemplatePreflight,
    pub field: ContemplateContextField,
    pub automatic_agent_or_model_invocation: bool,
}

fn provenance(object: &WikiObject) -> &[WikiProvenanceRef] {
    match object {
        WikiObject::Space(value) => &value.provenance,
        WikiObject::Node(value) => &value.provenance,
        WikiObject::Edge(value) => &value.provenance,
        WikiObject::Frame(value) => &value.provenance,
        WikiObject::Reading(value) => &value.provenance,
    }
}

fn object_kind(object: &WikiObject) -> &'static str {
    match object {
        WikiObject::Space(_) => "space",
        WikiObject::Node(_) => "node",
        WikiObject::Edge(_) => "edge",
        WikiObject::Frame(_) => "frame",
        WikiObject::Reading(_) => "reading",
    }
}

fn object_label(object: &WikiObject) -> Option<String> {
    match object {
        WikiObject::Space(value) => value.title.clone(),
        WikiObject::Node(value) => value.title.clone().or_else(|| Some(value.node_type.clone())),
        WikiObject::Edge(value) => Some(value.relation.clone()),
        WikiObject::Frame(value) => value
            .inquiry_ref
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| Some("Wiki frame".into())),
        WikiObject::Reading(value) => Some(value.reading_type.clone()),
    }
}

fn semantic_as_source_revision(revision: &SemanticRevision) -> Option<SourceRevision> {
    match revision {
        SemanticRevision::Text(value) => SourceRevision::parse(value).ok(),
        // A semantic numeric revision is intentionally not invented into a
        // provider-specific source revision namespace.
        SemanticRevision::Number(_) => None,
    }
}

/// Recover deterministic source and reading-basis dependency manifests already
/// present in canonical Wiki provenance / AIKit integrative-reading metadata.
/// Arbitrary semantic Wiki edges do not become basis dependencies.
pub fn wiki_living_dependencies(
    objects: &[WikiObject],
) -> Result<(Vec<KnowledgeDependency>, Vec<KnowledgeResourceDependency>)> {
    let mut source_dependencies = Vec::new();
    let mut resource_dependencies = Vec::new();
    for object in objects {
        let resource = object.ref_id().clone();
        for entry in provenance(object) {
            let Some(basis_revision) = entry
                .source_revision
                .as_ref()
                .and_then(semantic_as_source_revision)
            else {
                continue;
            };
            source_dependencies.push(KnowledgeDependency {
                dependent: resource.clone(),
                source: entry.source_ref.clone(),
                basis_revision: Some(basis_revision),
                relation: "wiki-provenance".into(),
                provenance_ref: Some(resource.clone()),
                integrative: matches!(object, WikiObject::Reading(_)),
            });
        }
        if let WikiObject::Reading(reading) = object {
            let Some(basis) = reading
                .extensions
                .get(INTEGRATIVE_READING_EXTENSION)
                .and_then(|value| value.get("basis"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for item in basis {
                let raw = item
                    .get("resource")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AikitError::new(
                            "knowledge.living_integrative_basis_invalid",
                            "integrative reading basis item has no stable resource ref",
                        )
                        .with("reading", reading.ref_id.to_string())
                    })?;
                resource_dependencies.push(KnowledgeResourceDependency {
                    basis: ResourceRef::parse(raw)?,
                    dependent: reading.ref_id.clone(),
                    relation: "integrative-basis".into(),
                    provenance_ref: Some(reading.ref_id.clone()),
                    integrative: true,
                });
            }
        }
    }
    source_dependencies.sort_by(|left, right| {
        left.dependent
            .cmp(&right.dependent)
            .then(left.source.cmp(&right.source))
    });
    source_dependencies.dedup_by(|left, right| {
        left.dependent == right.dependent
            && left.source == right.source
            && left.basis_revision == right.basis_revision
    });
    resource_dependencies.sort_by(|left, right| {
        left.basis
            .cmp(&right.basis)
            .then(left.dependent.cmp(&right.dependent))
    });
    resource_dependencies.dedup_by(|left, right| {
        left.basis == right.basis && left.dependent == right.dependent
    });
    Ok((source_dependencies, resource_dependencies))
}

fn reading_returns(object: &WikiObject) -> Vec<ContemplateFieldReturn> {
    let WikiObject::Reading(reading) = object else {
        return Vec::new();
    };
    reading
        .extensions
        .get(INTEGRATIVE_READING_EXTENSION)
        .and_then(|value| value.get("return_paths"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let from_basis = ResourceRef::parse(value.get("from_basis")?.as_str()?).ok()?;
            let to_whole = ResourceRef::parse(value.get("to_whole")?.as_str()?).ok()?;
            let through = value
                .get("through")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|raw| ResourceRef::parse(raw).ok())
                .collect();
            Some(ContemplateFieldReturn {
                from_basis,
                through,
                to_whole,
            })
        })
        .collect()
}

fn reading_tensions(object: &WikiObject) -> Vec<String> {
    let WikiObject::Reading(reading) = object else {
        return Vec::new();
    };
    reading
        .extensions
        .get("tensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn expand_basis_neighbourhood(
    relevant: &mut BTreeSet<ResourceRef>,
    resource_dependencies: &[KnowledgeResourceDependency],
    max_depth: usize,
    max_objects: usize,
) -> bool {
    let mut queue = relevant
        .iter()
        .cloned()
        .map(|resource| (resource, 0usize))
        .collect::<VecDeque<_>>();
    let mut truncated = false;
    while let Some((resource, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for dependency in resource_dependencies
            .iter()
            .filter(|dependency| dependency.basis == resource || dependency.dependent == resource)
        {
            let neighbour = if dependency.basis == resource {
                dependency.dependent.clone()
            } else {
                dependency.basis.clone()
            };
            if relevant.contains(&neighbour) {
                continue;
            }
            if relevant.len() >= max_objects {
                truncated = true;
                continue;
            }
            relevant.insert(neighbour.clone());
            queue.push_back((neighbour, depth + 1));
        }
    }
    truncated
}

pub fn assemble_contemplate_context(
    request: &ContemplateRequest<'_>,
    resource_dependencies: &[KnowledgeResourceDependency],
    max_objects: usize,
    relation_depth: usize,
) -> Result<ContemplateContextField> {
    if max_objects == 0 || relation_depth == 0 {
        return Err(AikitError::new(
            "knowledge.living_context_budget_invalid",
            "Contemplate context object budget and relation depth must be greater than zero",
        ));
    }
    let impact = deterministic_transitive_knowledge_impact(
        request.horizon,
        request.dependencies,
        resource_dependencies,
        DEFAULT_LIVING_IMPACT_DEPTH,
        DEFAULT_LIVING_IMPACT_RESOURCES,
    )?;

    let mut relevant = request.focus.iter().cloned().collect::<BTreeSet<_>>();
    relevant.extend(
        impact
            .direct
            .affected
            .iter()
            .map(|value| value.resource.clone()),
    );
    relevant.extend(
        impact
            .transitive
            .iter()
            .map(|value| value.resource.clone()),
    );
    let mut truncated = expand_basis_neighbourhood(
        &mut relevant,
        resource_dependencies,
        relation_depth,
        max_objects,
    );

    let objects_by_ref = request
        .current_wiki_objects
        .iter()
        .map(|object| (object.ref_id().clone(), object))
        .collect::<BTreeMap<_, _>>();

    // Include a bounded semantic relation neighbourhood around relevant basis/whole refs.
    // These edges remain Wiki relations; inclusion never turns them into basis dependencies.
    for _ in 0..relation_depth {
        let mut additions = BTreeSet::new();
        for object in request.current_wiki_objects {
            let WikiObject::Edge(edge) = object else {
                continue;
            };
            if relevant.contains(&edge.from_ref) || relevant.contains(&edge.to_ref) {
                additions.insert(edge.ref_id.clone());
                additions.insert(edge.from_ref.clone());
                additions.insert(edge.to_ref.clone());
            }
        }
        let before = relevant.len();
        for addition in additions {
            if relevant.len() >= max_objects {
                truncated = true;
                break;
            }
            relevant.insert(addition);
        }
        if relevant.len() == before {
            break;
        }
    }

    let frame_refs = relevant
        .iter()
        .filter_map(|resource| objects_by_ref.get(resource))
        .filter_map(|object| match object {
            WikiObject::Reading(reading) => Some(reading.frame_ref.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for frame in frame_refs {
        if relevant.len() >= max_objects {
            truncated = true;
            break;
        }
        if objects_by_ref.contains_key(&frame) {
            relevant.insert(frame);
        }
    }

    let mut objects = relevant
        .iter()
        .filter_map(|resource| objects_by_ref.get(resource))
        .map(|object| ContemplateFieldObject {
            resource: object.ref_id().clone(),
            revision: object.revision(),
            object_kind: object_kind(object).into(),
            label: object_label(object),
            provenance: provenance(object).to_vec(),
        })
        .collect::<Vec<_>>();
    objects.sort_by(|left, right| left.resource.cmp(&right.resource));

    let mut relations = request
        .current_wiki_objects
        .iter()
        .filter_map(|object| match object {
            WikiObject::Edge(edge)
                if relevant.contains(&edge.from_ref) && relevant.contains(&edge.to_ref) =>
            {
                Some(ContemplateFieldRelation {
                    edge_ref: edge.ref_id.clone(),
                    from: edge.from_ref.clone(),
                    to: edge.to_ref.clone(),
                    relation: edge.relation.clone(),
                    origin: format!("{:?}", edge.origin).to_lowercase(),
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    relations.sort_by(|left, right| left.edge_ref.cmp(&right.edge_ref));

    let selected_objects = relevant
        .iter()
        .filter_map(|resource| objects_by_ref.get(resource))
        .copied()
        .collect::<Vec<_>>();
    let mut returns = selected_objects
        .iter()
        .flat_map(|object| reading_returns(object))
        .collect::<Vec<_>>();
    returns.sort_by(|left, right| {
        left.from_basis
            .cmp(&right.from_basis)
            .then(left.to_whole.cmp(&right.to_whole))
    });
    returns.dedup();
    let mut tensions = selected_objects
        .iter()
        .flat_map(|object| reading_tensions(object))
        .collect::<Vec<_>>();
    tensions.sort();
    tensions.dedup();

    let relevant_sources = selected_objects
        .iter()
        .flat_map(|object| provenance(object).iter().map(|entry| entry.source_ref.clone()))
        .chain(impact.paths.iter().map(|path| path.root_source.clone()))
        .collect::<BTreeSet<_>>();
    let visibility = request
        .horizon
        .changes
        .iter()
        .map(|change| (change.source.clone(), change.agent_retrieval_allowed))
        .collect::<BTreeMap<_, _>>();
    let mut sources = request
        .horizon
        .sources
        .iter()
        .filter(|source| relevant_sources.contains(&source.source))
        .map(|source| ContemplateFieldSource {
            source: source.source.clone(),
            revision: source.revision.clone(),
            available: source.available,
            agent_retrieval_allowed: visibility.get(&source.source).copied(),
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| left.source.cmp(&right.source));

    let mut changes = request
        .horizon
        .changes
        .iter()
        .filter(|change| relevant_sources.contains(&change.source))
        .map(|change| ContemplateFieldChange {
            cursor: change.cursor,
            source: change.source.clone(),
            roles: change.roles.clone(),
            provenance: change.provenance.clone(),
            standing: change.standing.clone(),
            before_revision: change.before_revision.clone(),
            after_revision: change.after_revision.clone(),
            agent_retrieval_allowed: change.agent_retrieval_allowed,
        })
        .collect::<Vec<_>>();
    changes.sort_by(|left, right| {
        left.cursor
            .cmp(&right.cursor)
            .then(left.source.cmp(&right.source))
    });

    Ok(ContemplateContextField {
        version: CONTEMPLATE_FIELD_VERSION.into(),
        focus: request.focus.clone(),
        impact,
        changes,
        objects,
        relations,
        sources,
        returns,
        tensions,
        object_budget: max_objects,
        relation_depth,
        truncated,
        changed_source_payloads_retrieved: false,
        automatic_agent_or_model_invocation: false,
    })
}

pub fn bounded_contemplate_preflight(
    request: &ContemplateRequest<'_>,
    resource_dependencies: &[KnowledgeResourceDependency],
    max_objects: usize,
    relation_depth: usize,
) -> Result<BoundedContemplatePreflight> {
    let base = contemplate_preflight(request)?;
    let field = assemble_contemplate_context(
        request,
        resource_dependencies,
        max_objects,
        relation_depth,
    )?;
    Ok(BoundedContemplatePreflight {
        version: CONTEMPLATE_FIELD_VERSION.into(),
        base,
        field,
        automatic_agent_or_model_invocation: false,
    })
}

pub trait BoundedContemplateExecutor {
    /// The explicit Agent/model seam over AIKit's bounded deterministic field.
    fn execute(&mut self, preflight: &BoundedContemplatePreflight) -> Result<ContemplateGenerated>;
}

struct BoundedExecutorAdapter<'a> {
    bounded: &'a BoundedContemplatePreflight,
    executor: &'a mut dyn BoundedContemplateExecutor,
}

impl ContemplateExecutor for BoundedExecutorAdapter<'_> {
    fn execute(&mut self, preflight: &ContemplatePreflight) -> Result<ContemplateGenerated> {
        if preflight != &self.bounded.base {
            return Err(AikitError::new(
                "knowledge.living_bounded_preflight_drift",
                "Contemplate base preflight changed between bounded assembly and explicit execution",
            ));
        }
        self.executor.execute(self.bounded)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundedContemplateOutcome {
    pub preflight: BoundedContemplatePreflight,
    pub outcome: ContemplateOutcome,
}

pub fn explicit_bounded_contemplate(
    request: &ContemplateRequest<'_>,
    resource_dependencies: &[KnowledgeResourceDependency],
    max_objects: usize,
    relation_depth: usize,
    executor: &mut dyn BoundedContemplateExecutor,
) -> Result<BoundedContemplateOutcome> {
    let preflight = bounded_contemplate_preflight(
        request,
        resource_dependencies,
        max_objects,
        relation_depth,
    )?;
    let mut adapter = BoundedExecutorAdapter {
        bounded: &preflight,
        executor,
    };
    let outcome = explicit_contemplate(request, &mut adapter)?;
    Ok(BoundedContemplateOutcome { preflight, outcome })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::RetractionMode;
    use crate::knowledge_living::{
        ContemplateGenerated, KnowledgeChangeHorizon, KnowledgeChangeKind, KnowledgeFreshness,
        KnowledgeObservedSource, KnowledgeSourceChange,
    };
    use crate::knowledge_wiki::{WikiEdge, WikiEdgeOrigin, WikiReading};
    use crate::model_runtime::{
        AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
        ModelAccessReading, ModelMaterialisationReading, ModelRuntimeRelation, ModelSurfaceReading,
        ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
    };
    use crate::project::ProjectRef;
    use crate::projectcentral::HumanSourceRevisionProposal;
    use crate::ql::QlRefractionRequest;
    use crate::resource::ProviderRef;

    fn resource(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn source(value: &str) -> SourceRef {
        SourceRef::parse(value).unwrap()
    }

    fn revision(value: &str) -> SourceRevision {
        SourceRevision::parse(value).unwrap()
    }

    fn runtime() -> crate::model_runtime::ModelRuntimeReadModel {
        crate::model_runtime::ModelRuntimeReadModel {
            version: "aikit.model-runtime/v1".into(),
            project: Some(resource("project:test")),
            agent: Some(resource("agent:test")),
            agency: Some(resource("agency:test")),
            harness: resource("harness:test"),
            agent_session: Some("agent-session/test".into()),
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

    fn field() -> (KnowledgeChangeHorizon, Vec<WikiObject>) {
        let src = source("central:source:project:test:README.md");
        let whole = resource("wiki:reading:whole");
        let part = resource("wiki:node:part");
        let mut reading_extensions = BTreeMap::new();
        reading_extensions.insert(
            INTEGRATIVE_READING_EXTENSION.into(),
            serde_json::json!({
                "basis": [{"resource": part}],
                "relations": [],
                "return_paths": [{"from_basis": part, "through": [], "to_whole": whole}],
                "freshness": KnowledgeFreshness::Fresh,
                "topology": "recursive-dag"
            }),
        );
        reading_extensions.insert("tensions".into(), serde_json::json!(["open question"]));
        let part_object = WikiObject::Node(crate::knowledge_wiki::WikiNode {
            profile: "okf-wiki/v1".into(),
            ref_id: part.clone(),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: src.clone(),
                source_revision: Some(SemanticRevision::Text("r1".into())),
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "concept".into(),
            title: Some("Part".into()),
            space_refs: vec![],
            source_refs: vec![src.clone()],
            local_space_ref: None,
            extensions: BTreeMap::new(),
        });
        let whole_object = WikiObject::Reading(WikiReading {
            profile: "okf-wiki/v1".into(),
            ref_id: whole.clone(),
            revision: 2,
            provenance: vec![WikiProvenanceRef {
                source_ref: src.clone(),
                source_revision: Some(SemanticRevision::Text("r1".into())),
                producer_ref: Some(resource("agent:test")),
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            frame_ref: resource("wiki:frame:test"),
            reading_type: "integrative".into(),
            artifact_ref: None,
            derived_by_ref: Some(resource("agent:test")),
            extensions: reading_extensions,
        });
        let edge = WikiObject::Edge(WikiEdge {
            profile: "okf-wiki/v1".into(),
            ref_id: resource("wiki:edge:part-whole"),
            revision: 1,
            provenance: vec![],
            from_ref: part,
            to_ref: whole,
            relation: "contributes-to".into(),
            origin: WikiEdgeOrigin::Authored,
            origin_ref: None,
            extensions: BTreeMap::new(),
        });
        let horizon = KnowledgeChangeHorizon {
            provider: "central.filesystem-reconcile/v1".into(),
            cursor: 4,
            sources: vec![KnowledgeObservedSource {
                source: src.clone(),
                revision: Some(revision("r2")),
                available: true,
            }],
            changes: vec![KnowledgeSourceChange {
                cursor: 4,
                world_ref: "project:test".into(),
                source: src,
                roles: vec!["purpose".into()],
                provenance: "human-authored".into(),
                standing: "authored-human-position".into(),
                before_revision: Some(revision("r1")),
                after_revision: Some(revision("r2")),
                kind: KnowledgeChangeKind::Modified,
                agent_retrieval_allowed: false,
            }],
        };
        (horizon, vec![part_object, whole_object, edge])
    }

    #[test]
    fn bounded_preflight_recovers_part_whole_ground_relation_and_privacy_without_payload() {
        let (horizon, objects) = field();
        let (source_dependencies, resource_dependencies) =
            wiki_living_dependencies(&objects).unwrap();
        let runtime = runtime();
        let request = ContemplateRequest {
            project: ProjectRef::parse("project:test").unwrap(),
            focus: vec![resource("wiki:node:part")],
            horizon: &horizon,
            dependencies: &source_dependencies,
            current_wiki_objects: &objects,
            runtime: &runtime,
            method: None,
            ql: None::<QlRefractionRequest>,
        };
        let preflight =
            bounded_contemplate_preflight(&request, &resource_dependencies, 16, 2).unwrap();
        assert!(
            preflight
                .field
                .objects
                .iter()
                .any(|value| value.resource == resource("wiki:node:part"))
        );
        assert!(
            preflight
                .field
                .objects
                .iter()
                .any(|value| value.resource == resource("wiki:reading:whole"))
        );
        assert!(
            preflight
                .field
                .relations
                .iter()
                .any(|value| value.relation == "contributes-to")
        );
        assert!(
            preflight
                .field
                .returns
                .iter()
                .any(|value| value.to_whole == resource("wiki:reading:whole"))
        );
        assert_eq!(preflight.field.changes[0].roles, vec!["purpose"]);
        assert_eq!(
            preflight.field.sources[0].agent_retrieval_allowed,
            Some(false)
        );
        assert!(!preflight.field.changed_source_payloads_retrieved);
        assert!(!preflight.automatic_agent_or_model_invocation);
    }

    struct CountingExecutor {
        calls: usize,
        saw_whole: bool,
    }

    impl BoundedContemplateExecutor for CountingExecutor {
        fn execute(
            &mut self,
            preflight: &BoundedContemplatePreflight,
        ) -> Result<ContemplateGenerated> {
            self.calls += 1;
            self.saw_whole = preflight
                .field
                .objects
                .iter()
                .any(|value| value.resource == resource("wiki:reading:whole"));
            Ok(ContemplateGenerated {
                wiki_upserts: vec![],
                integrative_readings: vec![],
                candidates: vec![],
                tensions: vec![],
                human_source_proposals: vec![HumanSourceRevisionProposal {
                    source: source("central:source:project:test:README.md"),
                    reason: "review wording".into(),
                    evidence: vec![],
                }],
            })
        }
    }

    #[test]
    fn agent_model_line_begins_only_at_explicit_bounded_contemplate() {
        let (horizon, objects) = field();
        let (source_dependencies, resource_dependencies) =
            wiki_living_dependencies(&objects).unwrap();
        let runtime = runtime();
        let request = ContemplateRequest {
            project: ProjectRef::parse("project:test").unwrap(),
            focus: vec![resource("wiki:node:part")],
            horizon: &horizon,
            dependencies: &source_dependencies,
            current_wiki_objects: &objects,
            runtime: &runtime,
            method: None,
            ql: None,
        };
        let preflight =
            bounded_contemplate_preflight(&request, &resource_dependencies, 16, 2).unwrap();
        assert!(!preflight.automatic_agent_or_model_invocation);
        let mut executor = CountingExecutor {
            calls: 0,
            saw_whole: false,
        };
        let outcome = explicit_bounded_contemplate(
            &request,
            &resource_dependencies,
            16,
            2,
            &mut executor,
        )
        .unwrap();
        assert_eq!(executor.calls, 1);
        assert!(executor.saw_whole);
        assert_eq!(outcome.outcome.agent_wiki.human_source_proposals.len(), 1);
        assert!(outcome.outcome.agent_wiki.next_objects.len() >= objects.len());
    }
}
