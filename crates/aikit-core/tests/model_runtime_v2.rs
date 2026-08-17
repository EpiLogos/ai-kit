use std::collections::{BTreeMap, BTreeSet};

use aikit_core::composition::{
    CompositionState, HarnessComposition, ProjectionBinding, RetractionMode, SurfaceDescriptor,
    SurfaceKind,
};
use aikit_core::model_runtime::{
    disclose_model_runtime, model_provider_conformance_fixtures, AccessFieldReading,
    InferenceEngineForm, InferenceEngineReading, MaterialResourceReading, ModelAccessReading,
    ModelMaterialisationReading, ModelRuntimeRelation, ModelSurfaceReading, ModelVariantReading,
    PlacementObservation, RuntimeChangeApplication, LLAMA_CPP_CONFORMANCE_REVISION,
    OLLAMA_CONFORMANCE_REVISION, VLLM_CONFORMANCE_REVISION,
};
use aikit_core::resource::{ProviderRef, ResourceKind, ResourceRef};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn provider(raw: &str) -> ProviderRef {
    ProviderRef::parse(raw).unwrap()
}

fn composition(model: &ResourceRef) -> HarnessComposition {
    HarnessComposition {
        version: "aikit.harness-composition/v2".into(),
        harness: r("harness/pi"),
        project: Some(r("project/epi-logos")),
        agent: Some(r("agent/parasakti")),
        agency: Some(r("agency/design")),
        session: Some("agent-session-184".into()),
        model: Some(model.clone()),
        component_bindings: vec![],
        contract_bindings: vec![],
        contributions: vec![],
        surfaces: vec![],
        projections: vec![],
        absences: vec![],
        state: CompositionState::Resolved,
        target_revision: Some("target-revision".into()),
        generation: Some("generation-9".into()),
        fingerprint: "body-thin".into(),
    }
}

fn runtime(
    model: &ResourceRef,
    provider_ref: &str,
    engine_ref: &str,
    form: InferenceEngineForm,
    binding_ref: &str,
    placement: PlacementObservation,
    endpoint: Option<&str>,
) -> ModelRuntimeRelation {
    ModelRuntimeRelation {
        model: ModelVariantReading {
            model: model.clone(),
            variant: "deepseek-r1-32b-q4".into(),
        },
        engine: InferenceEngineReading {
            engine: r(engine_ref),
            provider: provider(provider_ref),
            form,
            revision: Some("source-pin".into()),
            provider_native: BTreeMap::new(),
        },
        materialisation: ModelMaterialisationReading {
            binding_ref: binding_ref.into(),
            workcell_ref: Some("workcell/reference-laptop".into()),
            placement,
            endpoint: endpoint.map(str::to_owned),
            provider_native: BTreeMap::new(),
            resources: MaterialResourceReading::default(),
            lifetime_owner: "workcell".into(),
            retraction: RetractionMode::Restart,
        },
        model_surface: ModelSurfaceReading {
            contract: None,
            protocol: "openai-compatible-chat".into(),
            capabilities: BTreeSet::from(["invoke".into(), "stream".into()]),
            access: ModelAccessReading {
                inference: AccessFieldReading::available(["invoke", "stream"]),
                material_control: AccessFieldReading::unavailable("provider owns lifecycle"),
                interior: AccessFieldReading::unavailable("no model-interior seam"),
            },
        },
        change_application: RuntimeChangeApplication::NextSession,
    }
}

#[test]
fn engine_and_material_rebinding_do_not_mint_semantic_identity() {
    let model = r("model/deepseek-r1");
    let body = composition(&model);
    let ollama = disclose_model_runtime(
        &body,
        runtime(
            &model,
            "provider/ollama",
            "engine/ollama",
            InferenceEngineForm::ManagedService,
            "binding/ollama-1",
            PlacementObservation::Local,
            Some("http://127.0.0.1:11434"),
        ),
    )
    .unwrap();
    let vllm = disclose_model_runtime(
        &body,
        runtime(
            &model,
            "provider/vllm",
            "engine/vllm",
            InferenceEngineForm::ServingRuntime,
            "binding/vllm-7",
            PlacementObservation::Remote,
            Some("https://example.invalid/v1"),
        ),
    )
    .unwrap();

    assert_eq!(ollama.relation.model, vllm.relation.model);
    assert_ne!(ollama.relation.engine, vllm.relation.engine);
    assert_ne!(
        ollama.relation.materialisation.binding_ref,
        vllm.relation.materialisation.binding_ref
    );
    assert_eq!(ollama.harness, vllm.harness);
    assert_eq!(ollama.project, vllm.project);
    assert_eq!(ollama.agent, vllm.agent);
    assert_eq!(ollama.agency, vllm.agency);
    assert_eq!(ollama.agent_session, vllm.agent_session);
}

#[test]
fn three_access_axes_are_independent_and_absence_is_explained() {
    let model = r("model/deepseek-r1");
    let mut relation = runtime(
        &model,
        "provider/research",
        "engine/research",
        InferenceEngineForm::External,
        "binding/research",
        PlacementObservation::Remote,
        None,
    );
    relation.model_surface.access = ModelAccessReading {
        inference: AccessFieldReading::available(["invoke"]),
        material_control: AccessFieldReading::unavailable("provider owns lifecycle"),
        interior: AccessFieldReading::available(["internal-read", "causal-intervention"]),
    };
    let reading = disclose_model_runtime(&composition(&model), relation).unwrap();

    assert!(reading
        .relation
        .model_surface
        .access
        .inference
        .is_available());
    assert!(!reading
        .relation
        .model_surface
        .access
        .material_control
        .is_available());
    assert!(reading
        .relation
        .model_surface
        .access
        .interior
        .is_available());
    assert!(reading
        .unavailable
        .iter()
        .any(|item| item.field == "material-control-access"));
}

#[test]
fn thin_direct_target_is_valid_without_daemon_ui_trajectory_or_contract() {
    let model = r("model/tiny-static");
    let reading = disclose_model_runtime(
        &composition(&model),
        runtime(
            &model,
            "provider/llama-cpp",
            "engine/llama-cpp-cli",
            InferenceEngineForm::Direct,
            "binding/process-991",
            PlacementObservation::Local,
            None,
        ),
    )
    .unwrap();

    assert_eq!(reading.relation.engine.form, InferenceEngineForm::Direct);
    assert!(reading.relation.materialisation.endpoint.is_none());
    assert!(reading.components.is_empty());
    assert!(reading.contracts.is_empty());
    assert!(reading.surfaces.is_empty());
    assert!(reading
        .unavailable
        .iter()
        .any(|item| item.field == "inference-contract"));
}

#[test]
fn one_action_projects_across_surfaces_while_reading_remains_non_action() {
    let model = r("model/deepseek-r1");
    let mut body = composition(&model);
    body.surfaces = vec![
        SurfaceDescriptor {
            resource: r("surface/conversation"),
            kind: SurfaceKind::Conversation,
            target_native_id: None,
            owner_component: None,
        },
        SurfaceDescriptor {
            resource: r("surface/web"),
            kind: SurfaceKind::Web,
            target_native_id: None,
            owner_component: None,
        },
    ];
    for surface in [r("surface/conversation"), r("surface/web")] {
        body.projections.push(ProjectionBinding {
            canonical_ref: r("action/run-command"),
            canonical_kind: ResourceKind::Action,
            contribution: r(&format!(
                "contribution/action/{}",
                surface.as_str().replace('/', "-")
            )),
            component: r("component/tooling"),
            surface,
            target_native_surface: None,
        });
    }
    body.projections.push(ProjectionBinding {
        canonical_ref: r("knowledge/trajectory-reading"),
        canonical_kind: ResourceKind::KnowledgeNode,
        contribution: r("contribution/trajectory-reading"),
        component: r("component/trajectory"),
        surface: r("surface/conversation"),
        target_native_surface: None,
    });

    let reading = disclose_model_runtime(
        &body,
        runtime(
            &model,
            "provider/vllm",
            "engine/vllm",
            InferenceEngineForm::ServingRuntime,
            "binding/vllm",
            PlacementObservation::Remote,
            Some("https://example.invalid/v1"),
        ),
    )
    .unwrap();

    assert!(reading
        .surfaces
        .iter()
        .all(|surface| surface.action_refs.contains(&r("action/run-command"))));
    assert_eq!(
        reading
            .surfaces
            .iter()
            .find(|surface| surface.surface == r("surface/conversation"))
            .unwrap()
            .non_action_refs,
        vec![r("knowledge/trajectory-reading")]
    );
}

#[test]
fn semantic_model_mismatch_is_rejected_and_provider_matrix_is_not_daemon_normalised() {
    let selected = r("model/selected");
    let different = r("model/different");
    let error = disclose_model_runtime(
        &composition(&selected),
        runtime(
            &different,
            "provider/ollama",
            "engine/ollama",
            InferenceEngineForm::ManagedService,
            "binding/ollama",
            PlacementObservation::Local,
            Some("http://127.0.0.1:11434"),
        ),
    )
    .unwrap_err();
    assert_eq!(error.code(), "model_runtime.model_identity_mismatch");

    let fixtures = model_provider_conformance_fixtures();
    assert_eq!(fixtures[0].upstream_revision, OLLAMA_CONFORMANCE_REVISION);
    assert_eq!(
        fixtures[1].upstream_revision,
        LLAMA_CPP_CONFORMANCE_REVISION
    );
    assert_eq!(fixtures[2].upstream_revision, VLLM_CONFORMANCE_REVISION);
    assert!(fixtures[0].daemon_required);
    assert!(!fixtures[1].daemon_required);
    assert!(!fixtures[2].daemon_required);
}
