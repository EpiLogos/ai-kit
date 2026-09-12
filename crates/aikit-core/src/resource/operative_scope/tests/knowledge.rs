use super::*;
use crate::composition::RetractionMode;
use crate::knowledge_living::{
    ContemplateGenerated, ContemplateRequest, IntegrativeWikiReading, KnowledgeChangeHorizon,
    KnowledgeDependency, KnowledgeFreshness, KnowledgeObservedSource, ReadingBasisNode,
};
use crate::knowledge_wiki::{WikiObject, WikiReading, OKF_WIKI_PROFILE};
use crate::model_runtime::{
    AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
    ModelAccessReading, ModelMaterialisationReading, ModelRuntimeReadModel, ModelRuntimeRelation,
    ModelSurfaceReading, ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
};
use crate::resource::operative_scope::knowledge::{
    explicit_scoped_contemplate, ScopedContemplateInput, ScopedKnowledgeReturnStanding,
    OPERATIVE_SCOPE_RETURN_EXTENSION,
};

fn resource(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn runtime() -> ModelRuntimeReadModel {
    ModelRuntimeReadModel {
        version: "aikit.model-runtime/v1".into(),
        project: Some(resource("project/one")),
        agent: Some(resource("agent/epii")),
        agency: Some(resource("agency/knowledge")),
        harness: resource("harness/test"),
        agent_session: Some("session/test".into()),
        harness_composition_fingerprint: "fixture-r1".into(),
        relation: ModelRuntimeRelation {
            model: ModelVariantReading {
                model: resource("model/test"),
                variant: "default".into(),
            },
            engine: InferenceEngineReading {
                engine: resource("engine/test"),
                provider: ProviderRef::parse("provider/test").unwrap(),
                form: InferenceEngineForm::External,
                revision: None,
                provider_native: BTreeMap::new(),
            },
            materialisation: ModelMaterialisationReading {
                binding_ref: "binding/test".into(),
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

fn horizon() -> KnowledgeChangeHorizon {
    KnowledgeChangeHorizon {
        provider: "test/native-source-provider".into(),
        cursor: 1,
        sources: vec![KnowledgeObservedSource {
            source: binding().sources[0].source.clone(),
            revision: Some(binding().sources[0].revision.clone()),
            available: true,
        }],
        changes: vec![],
    }
}

fn dependency() -> KnowledgeDependency {
    KnowledgeDependency {
        dependent: resource("wiki/reading"),
        source: binding().sources[0].source.clone(),
        basis_revision: Some(binding().sources[0].revision.clone()),
        relation: "integrates".into(),
        provenance_ref: None,
        integrative: true,
    }
}

fn generated() -> ContemplateGenerated {
    let reading = WikiReading {
        profile: OKF_WIKI_PROFILE.into(),
        ref_id: resource("wiki/reading"),
        revision: 1,
        provenance: vec![],
        frame_ref: resource("wiki/frame"),
        reading_type: "integrative/native-scope-test".into(),
        artifact_ref: None,
        derived_by_ref: Some(resource("agent/epii")),
        extensions: BTreeMap::new(),
    };
    ContemplateGenerated {
        wiki_upserts: vec![],
        integrative_readings: vec![IntegrativeWikiReading {
            reading,
            basis: vec![ReadingBasisNode {
                resource: resource("project/one"),
                source: Some(binding().sources[0].source.clone()),
                source_revision: Some(binding().sources[0].revision.clone()),
                roles: vec!["native-original-source".into()],
            }],
            relations: vec![],
            return_paths: vec![],
            freshness: KnowledgeFreshness::Fresh,
        }],
        candidates: vec!["native generated candidate".into()],
        tensions: vec![],
        human_source_proposals: vec![],
    }
}

#[test]
fn native_contamplate_performs_once_and_retains_original_scope_in_actual_reading() {
    let index = index();
    let context = context(&index);
    let provider = ObservingProvider::new();
    let resolution = compose_scoped_context(&request(), &index, &context, 16, &provider).unwrap();
    let runtime = runtime();
    let horizon = horizon();
    let dependencies = [dependency()];
    let objects: Vec<WikiObject> = vec![];
    let request = ContemplateRequest {
        project: context.project_binding.project.clone(),
        focus: vec![resource("project/one")],
        horizon: &horizon,
        dependencies: &dependencies,
        current_wiki_objects: &objects,
        runtime: &runtime,
        method: None,
        ql: None,
    };
    let mut calls = 0;
    let result = explicit_scoped_contemplate(
        ScopedContemplateInput {
            request: &request,
            resolution: &resolution,
            current_context: &context,
            resource_dependencies: &[],
            max_objects: 16,
            relation_depth: 2,
            shape_budget: 24,
        },
        &provider,
        |preflight, scope| {
            calls += 1;
            assert_eq!(scope.path().native().identity, resolution.path().native().identity);
            assert_eq!(preflight.operative.as_ref().unwrap().resolve_path_identity,
                resolution.path().native().identity);
            Ok(generated())
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(result.standing, ScopedKnowledgeReturnStanding::Current);
    assert_eq!(result.native.outcome.candidates, ["native generated candidate"]);
    let reading = &result.native.outcome.integrative_readings[0];
    let scope = &reading.reading.extensions[OPERATIVE_SCOPE_RETURN_EXTENSION];
    assert_eq!(scope["resolve_path_identity"], resolution.path().native().identity);
    assert_eq!(scope["scopes"][0]["binding"]["whole"], "whole/one");
    assert_eq!(reading.basis[0].source_revision, Some(binding().sources[0].revision.clone()));
}

#[test]
fn denied_stale_or_foreign_source_performs_nothing_and_backend_failure_is_not_retried() {
    let index = index();
    let context = context(&index);
    let provider = ObservingProvider::new();
    let resolution = compose_scoped_context(&request(), &index, &context, 16, &provider).unwrap();
    let runtime = runtime();
    let horizon = horizon();
    let dependencies = [dependency()];
    let request = ContemplateRequest {
        project: context.project_binding.project.clone(),
        focus: vec![resource("project/one")],
        horizon: &horizon,
        dependencies: &dependencies,
        current_wiki_objects: &[],
        runtime: &runtime,
        method: None,
        ql: None,
    };
    let input = || ScopedContemplateInput {
        request: &request,
        resolution: &resolution,
        current_context: &context,
        resource_dependencies: &[],
        max_objects: 16,
        relation_depth: 2,
        shape_budget: 24,
    };
    provider.current.borrow_mut().generation = SourceRevision::parse("changed").unwrap();
    let mut calls = 0;
    assert!(explicit_scoped_contemplate(input(), &provider, |_, _| {
        calls += 1;
        Ok(generated())
    }).is_err());
    assert_eq!(calls, 0);
    *provider.current.borrow_mut() = binding();
    let result = explicit_scoped_contemplate(input(), &provider, |_, _| {
        calls += 1;
        Err(AikitError::new("native.denied", "native execution authority withheld"))
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
}

#[test]
fn source_change_during_execution_preserves_late_return_without_claiming_currentness() {
    let index = index();
    let context = context(&index);
    let provider = ObservingProvider::new();
    let resolution = compose_scoped_context(&request(), &index, &context, 16, &provider).unwrap();
    let runtime = runtime();
    let horizon = horizon();
    let dependencies = [dependency()];
    let request = ContemplateRequest {
        project: context.project_binding.project.clone(),
        focus: vec![resource("project/one")],
        horizon: &horizon,
        dependencies: &dependencies,
        current_wiki_objects: &[],
        runtime: &runtime,
        method: None,
        ql: None,
    };
    let result = explicit_scoped_contemplate(
        ScopedContemplateInput {
            request: &request,
            resolution: &resolution,
            current_context: &context,
            resource_dependencies: &[],
            max_objects: 16,
            relation_depth: 2,
            shape_budget: 24,
        },
        &provider,
        |_, _| {
            provider.current.borrow_mut().sources[0].revision = SourceRevision::parse("r2").unwrap();
            Ok(generated())
        },
    ).unwrap();
    assert_eq!(result.standing, ScopedKnowledgeReturnStanding::ReobservationRequired);
    assert_eq!(result.native.outcome.candidates, ["native generated candidate"]);
    assert!(matches!(result.completion[0].observation, ScopeObservation::Stale { .. }));
    assert_eq!(result.original_resolution.observations()[0].scope.binding, binding());
}
