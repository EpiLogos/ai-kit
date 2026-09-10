use std::collections::{BTreeMap, BTreeSet};

use aikit_core::model_runtime::{
    AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
    ModelAccessReading, ModelMaterialisationReading, ModelRuntimeReadModel, ModelRuntimeRelation,
    ModelSurfaceReading, ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
};
use aikit_core::{
    bounded_contemplate_preflight, deterministic_transitive_knowledge_impact,
    explicit_bounded_contemplate, parse_contemplate_generated, wiki_living_dependencies,
    BoundedContemplateExecutor, BoundedContemplatePreflight, ContemplateGenerated,
    ContemplateRequest, KnowledgeChangeHorizon, KnowledgeChangeKind, KnowledgeFreshness,
    KnowledgeObservedSource, KnowledgeSourceChange, ProjectRef, ProviderRef, ResourceRef,
    RetractionMode, SemanticRevision, SemanticWikiIndex, SemanticWikiReading as WikiReading,
    SourceRef, SourceRevision, WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject,
    WikiProvenanceRef, INTEGRATIVE_READING_EXTENSION,
};

fn resource(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn source(value: &str) -> SourceRef {
    SourceRef::parse(value).unwrap()
}

fn revision(value: &str) -> SourceRevision {
    SourceRevision::parse(value).unwrap()
}

fn runtime() -> ModelRuntimeReadModel {
    ModelRuntimeReadModel {
        version: "aikit.model-runtime/v1".into(),
        project: Some(resource("project:test")),
        agent: Some(resource("agent:test")),
        agency: Some(resource("agency:test")),
        harness: resource("harness:test"),
        agent_session: Some("agent-session/test".into()),
        harness_composition_fingerprint: "acceptance".into(),
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

fn world() -> (KnowledgeChangeHorizon, Vec<WikiObject>) {
    let src = source("central:source:project:test:README.md");
    let part = resource("wiki:node:part");
    let whole = resource("wiki:reading:whole");

    let node = WikiObject::Node(WikiNode {
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

    // The whole has no direct source provenance in this fixture. Its only change
    // dependency is the explicit integrative basis relation from the granular part,
    // so any impact on the whole below is genuinely transitive.
    let reading = WikiObject::Reading(WikiReading {
        profile: "okf-wiki/v1".into(),
        ref_id: whole.clone(),
        revision: 2,
        provenance: vec![],
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

    (horizon, vec![node, reading, edge])
}

struct TransportExecutor {
    calls: usize,
}

impl BoundedContemplateExecutor for TransportExecutor {
    fn execute(
        &mut self,
        preflight: &BoundedContemplatePreflight,
    ) -> aikit_core::Result<ContemplateGenerated> {
        self.calls += 1;
        assert!(preflight
            .field
            .objects
            .iter()
            .any(|value| value.resource == resource("wiki:reading:whole")));
        parse_contemplate_generated(
            r#"{
              "version":"aikit.contemplate-return/v1",
              "wiki_upserts":[],
              "integrative_readings":[],
              "candidates":["candidate:relation"],
              "tensions":["still open"],
              "human_source_proposals":[{
                "source":"central:source:project:test:README.md",
                "reason":"review wording",
                "evidence":[]
              }]
            }"#,
        )
    }
}

#[test]
fn public_living_knowledge_path_is_deterministic_until_explicit_contemplate() {
    let (horizon, objects) = world();
    let (source_dependencies, resource_dependencies) = wiki_living_dependencies(&objects).unwrap();

    // Derived index state is rebuildable and does not define canonical identity.
    let first_index = SemanticWikiIndex::rebuild(objects.clone()).unwrap();
    let second_index = SemanticWikiIndex::rebuild(objects.clone()).unwrap();
    assert_eq!(first_index.revision(), second_index.revision());

    let impact = deterministic_transitive_knowledge_impact(
        &horizon,
        &source_dependencies,
        &resource_dependencies,
        8,
        512,
    )
    .unwrap();
    assert!(impact
        .direct
        .affected
        .iter()
        .any(|value| value.resource == resource("wiki:node:part")));
    assert!(!impact
        .direct
        .affected
        .iter()
        .any(|value| value.resource == resource("wiki:reading:whole")));
    let whole_path = impact
        .paths
        .iter()
        .find(|value| value.resource == resource("wiki:reading:whole"))
        .expect("whole must have an inspectable transitive path");
    assert_eq!(whole_path.steps.len(), 2);
    assert!(impact
        .transitive
        .iter()
        .any(|value| value.resource == resource("wiki:reading:whole")));
    assert!(impact
        .pending_integration
        .contains(&resource("wiki:reading:whole")));
    assert!(!impact.automatic_agent_or_model_invocation);

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

    let preflight = bounded_contemplate_preflight(&request, &resource_dependencies, 32, 2).unwrap();
    assert!(!preflight.automatic_agent_or_model_invocation);
    assert!(!preflight.field.changed_source_payloads_retrieved);
    assert_eq!(
        preflight.field.sources[0].agent_retrieval_allowed,
        Some(false)
    );
    assert!(preflight
        .field
        .returns
        .iter()
        .any(|value| value.to_whole == resource("wiki:reading:whole")));

    let mut executor = TransportExecutor { calls: 0 };
    assert_eq!(executor.calls, 0);

    let outcome = explicit_bounded_contemplate(
        &request,
        &resource_dependencies,
        32,
        2,
        &mut executor,
    )
    .unwrap();

    assert_eq!(executor.calls, 1);
    assert_eq!(outcome.outcome.candidates, vec!["candidate:relation"]);
    assert_eq!(outcome.outcome.agent_wiki.human_source_proposals.len(), 1);
    assert!(outcome
        .outcome
        .agent_wiki
        .next_objects
        .iter()
        .any(|object| object.ref_id() == &resource("wiki:node:part")));
}
