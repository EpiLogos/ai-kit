//! End-to-end modality resolution acceptance: both speech body shapes resolve
//! through the ordinary HarnessComposition resolver, keep per-stage
//! provider/model/materialisation relations, explain themselves through the
//! existing Explain seam, and degrade honestly.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::composition::{
    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentContribution,
    ComponentDescriptor, ComponentRequirement, ComponentSelection, CompositionActivationMode,
    CompositionCatalog, ContributionKind, ContractProvider, HarnessCompositionRequest,
    LifetimeOwner, LifetimeOwnerKind, RequirementStrength, ResolutionScope, RetractionMode,
    SurfaceDescriptor, SurfaceKind, TargetNativeComponentBinding,
};
use aikit_core::composition_view::diff_harness_compositions;
use aikit_core::model_modality::{
    explain_model_modality, explain_staged_model_runtime, CredentialScope, DeclaredSupport,
    InteractionCapability, ModalityDirection, ModalitySupport, ModelModality,
    ModelModalityContract, SurfaceAvailability, TransformCapability, TransportKind,
};
use aikit_core::model_runtime::{
    disclose_model_runtime, disclose_staged_model_runtime, AccessFieldReading,
    InferenceEngineForm, InferenceEngineReading, MaterialResourceReading, ModelAccessReading,
    ModelMaterialisationReading, ModelRuntimeRelation, ModelStageRelation, ModelSurfaceReading,
    ModelVariantReading, PlacementObservation, RuntimeChangeApplication, RuntimeSurfaceReading,
};
use aikit_core::resource::{
    rank_model_roster, ModelRankingPolicy, ModelRosterCandidate, ModelRosterDemand,
};
use aikit_core::resource::{CredentialCondition, ProviderRef, ResourceKind, ResourceRef};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn provider(raw: &str) -> ProviderRef {
    ProviderRef::parse(raw).unwrap()
}

fn selection(component: &str) -> ComponentSelection {
    ComponentSelection {
        component: r(component),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "project/voice-fixtures"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
        activation_mode: CompositionActivationMode::LiveMounted,
    }
}

use aikit_core::scope::ScopeKind;

fn contribution(
    id: &str,
    component: &str,
    surface: &str,
    exposed: &str,
    exposed_kind: ResourceKind,
) -> ComponentContribution {
    ComponentContribution {
        id: r(id),
        component: r(component),
        kind: ContributionKind::Tool,
        target_contract: None,
        exposed_ref: Some(r(exposed)),
        exposed_kind: Some(exposed_kind),
        surface: Some(r(surface)),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
        activation_mode: CompositionActivationMode::LiveMounted,
        retraction_mode: RetractionMode::Live,
        provenance: vec!["fixture:openai-realtime-session/2026-09-17".into()],
    }
}

/// A native realtime speech-to-speech body: one model-adapter component on a
/// conversation surface, its inference contract bound to the provider.
fn realtime_catalog() -> CompositionCatalog {
    let mut catalog = CompositionCatalog::default();
    let mut adapter = ComponentDescriptor::new(r("component/openai-realtime-adapter"));
    adapter.implementation = Some(TargetNativeComponentBinding {
        implementation_target: "openai-realtime".into(),
        native_id: "gpt-realtime".into(),
        revision: Some("fixture-2026-09-17".into()),
    });
    adapter.provisions = vec![r("contract:realtime-speech")];
    adapter.supported_surfaces = vec![r("surface/realtime-conversation")];
    adapter.activation_modes = BTreeSet::from([CompositionActivationMode::LiveMounted]);
    adapter.contributions = vec![contribution(
        "contribution/realtime-tool-requests",
        "component/openai-realtime-adapter",
        "surface/realtime-conversation",
        "capability/realtime-tool-request-channel",
        ResourceKind::Capability,
    )];
    catalog.insert_component(adapter);
    catalog.insert_surface(SurfaceDescriptor {
        resource: r("surface/realtime-conversation"),
        kind: SurfaceKind::Conversation,
        target_native_id: Some("wss://realtime.example.invalid/v1/realtime".into()),
        owner_component: Some(r("component/openai-realtime-adapter")),
    });
    catalog.add_provider(ContractProvider::available(
        r("contract:realtime-speech"),
        r("provider:openai"),
    ));
    catalog
}

fn realtime_request() -> HarnessCompositionRequest {
    HarnessCompositionRequest {
        harness: r("harness/realvoice"),
        project: Some(r("project/voice-fixtures")),
        agent: Some(r("agent/parasakti")),
        agency: Some(r("agency/nara")),
        session: Some("agent-session-77".into()),
        model: Some(r("model:gpt-realtime")),
        selections: vec![selection("component/openai-realtime-adapter")],
        target_revision: Some("target-1".into()),
        generation: Some("generation-1".into()),
    }
}

/// The realtime surface contract, as an OpenAI Realtime adapter would declare
/// it from its frozen session fixture (see the adapter's own conformance
/// tests). Provider vocabulary stays out of the generic fields.
fn realtime_modality(credential: CredentialCondition) -> ModelModalityContract {
    let mut modality =
        ModelModalityContract::new(provider("provider:openai"), "gpt-realtime");
    modality.input_modalities = BTreeSet::from([
        ModelModality::Audio,
        ModelModality::Speech,
        ModelModality::Text,
    ]);
    modality.output_modalities = BTreeSet::from([
        ModelModality::Audio,
        ModelModality::Speech,
        ModelModality::Text,
    ]);
    modality.transforms = BTreeMap::from([
        (
            TransformCapability::SpeechToSpeech,
            DeclaredSupport::Supported,
        ),
        (
            TransformCapability::AudioUnderstanding,
            DeclaredSupport::Supported,
        ),
        (
            TransformCapability::MultimodalTextAudio,
            DeclaredSupport::Supported,
        ),
    ]);
    modality.interaction = BTreeSet::from([
        InteractionCapability::StreamingInput,
        InteractionCapability::StreamingOutput,
        InteractionCapability::FullDuplexRealtime,
        InteractionCapability::StructuredEvents,
        InteractionCapability::ToolRequests,
        InteractionCapability::VadTurnDetection,
        InteractionCapability::BargeIn,
    ]);
    modality.transport = TransportKind::WebSocket;
    modality.connection = aikit_core::model_modality::ConnectionSemantics::Connected {
        reconnect: aikit_core::model_modality::ReconnectSupport::ReconnectWithoutSession,
    };
    modality.credential_scope = CredentialScope::EphemeralSurfaceToken;
    modality.availability = SurfaceAvailability::Available;
    modality.credential = credential;
    modality.provenance = vec!["fixture:openai-realtime-session/2026-09-17".into()];
    modality
}

fn realtime_relation(credential: CredentialCondition) -> ModelRuntimeRelation {
    ModelRuntimeRelation {
        model: ModelVariantReading {
            model: r("model:gpt-realtime"),
            variant: "gpt-realtime".into(),
        },
        engine: InferenceEngineReading {
            engine: r("engine/openai-realtime"),
            provider: provider("provider:openai"),
            form: InferenceEngineForm::ManagedService,
            revision: Some("fixture-2026-09-17".into()),
            provider_native: BTreeMap::new(),
        },
        materialisation: ModelMaterialisationReading {
            binding_ref: "binding/realtime-1".into(),
            workcell_ref: None,
            placement: PlacementObservation::Remote,
            endpoint: Some("wss://realtime.example.invalid/v1/realtime".into()),
            provider_native: BTreeMap::new(),
            resources: MaterialResourceReading::default(),
            lifetime_owner: "agent-session".into(),
            retraction: RetractionMode::Live,
        },
        model_surface: ModelSurfaceReading {
            contract: Some(r("contract:realtime-speech")),
            protocol: "openai-realtime-websocket".into(),
            capabilities: BTreeSet::new(),
            access: ModelAccessReading {
                inference: AccessFieldReading::available(["invoke", "stream", "interrupt"]),
                material_control: AccessFieldReading::unavailable("provider owns lifecycle"),
                interior: AccessFieldReading::unavailable("no model-interior seam"),
            },
            modality: Some(realtime_modality(credential)),
        },
        change_application: RuntimeChangeApplication::Live,
    }
}

fn cascade_request() -> HarnessCompositionRequest {
    HarnessCompositionRequest {
        harness: r("harness/cascaded-voice"),
        project: Some(r("project/voice-fixtures")),
        agent: Some(r("agent/parasakti")),
        agency: Some(r("agency/nara")),
        session: Some("agent-session-77".into()),
        model: None,
        selections: vec![
            selection("component/stt-stage"),
            selection("component/text-harness"),
            selection("component/tts-stage"),
        ],
        target_revision: Some("target-1".into()),
        generation: Some("generation-1".into()),
    }
}

/// A cascade body: STT surface -> text reasoning harness -> TTS surface,
/// wired through ordinary Contract requirements and providers.
fn cascade_catalog(stt_winner: &str, stt_loser: &str) -> CompositionCatalog {
    let mut catalog = CompositionCatalog::default();

    let mut stt = ComponentDescriptor::new(r("component/stt-stage"));
    stt.implementation = Some(TargetNativeComponentBinding {
        implementation_target: "openai-transcribe".into(),
        native_id: "gpt-4o-transcribe".into(),
        revision: Some("fixture-2026-09-17".into()),
    });
    stt.provisions = vec![r("contract:transcript")];
    stt.supported_surfaces = vec![r("surface/stt")];
    stt.activation_modes = BTreeSet::from([CompositionActivationMode::LiveMounted]);
    catalog.insert_component(stt);

    let mut text = ComponentDescriptor::new(r("component/text-harness"));
    text.requirements = vec![ComponentRequirement::required(r("contract:transcript"))];
    text.provisions = vec![r("contract:reasoned-text")];
    text.activation_modes = BTreeSet::from([CompositionActivationMode::LiveMounted]);
    catalog.insert_component(text);

    let mut tts = ComponentDescriptor::new(r("component/tts-stage"));
    tts.implementation = Some(TargetNativeComponentBinding {
        implementation_target: "openai-tts".into(),
        native_id: "gpt-4o-mini-tts".into(),
        revision: Some("fixture-2026-09-17".into()),
    });
    tts.requirements = vec![ComponentRequirement::required(r("contract:reasoned-text"))];
    tts.supported_surfaces = vec![r("surface/tts")];
    tts.activation_modes = BTreeSet::from([CompositionActivationMode::LiveMounted]);
    catalog.insert_component(tts);

    catalog.insert_surface(SurfaceDescriptor {
        resource: r("surface/stt"),
        kind: SurfaceKind::Api,
        target_native_id: Some("https://api.example.invalid/v1/audio/transcriptions".into()),
        owner_component: Some(r("component/stt-stage")),
    });
    catalog.insert_surface(SurfaceDescriptor {
        resource: r("surface/tts"),
        kind: SurfaceKind::Api,
        target_native_id: Some("https://api.example.invalid/v1/audio/speech".into()),
        owner_component: Some(r("component/tts-stage")),
    });

    let mut winner = ContractProvider::available(r("contract:transcript"), r(stt_winner));
    winner.priority = 10;
    catalog.add_provider(winner);
    let mut loser = ContractProvider::available(r("contract:transcript"), r(stt_loser));
    loser.priority = 5;
    catalog.add_provider(loser);
    catalog.add_provider(ContractProvider::available(
        r("contract:reasoned-text"),
        r("provider:ollama"),
    ));
    catalog
}

fn stt_relation() -> ModelRuntimeRelation {
    let mut modality =
        ModelModalityContract::new(provider("provider:openai"), "gpt-4o-transcribe");
    modality.input_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
    modality.output_modalities = BTreeSet::from([ModelModality::Text]);
    modality.transforms = BTreeMap::from([(
        TransformCapability::SpeechToText,
        DeclaredSupport::Supported,
    )]);
    modality.interaction = BTreeSet::from([
        InteractionCapability::RequestResponse,
        InteractionCapability::Timestamps,
        InteractionCapability::FinalTranscripts,
    ]);
    modality.transport = TransportKind::Http;
    modality.credential = CredentialCondition::Satisfied {
        hint: "openai inference credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    };
    ModelRuntimeRelation {
        model: ModelVariantReading {
            model: r("model:gpt-4o-transcribe"),
            variant: "gpt-4o-transcribe".into(),
        },
        engine: InferenceEngineReading {
            engine: r("engine/openai-transcribe"),
            provider: provider("provider:openai"),
            form: InferenceEngineForm::ManagedService,
            revision: Some("fixture-2026-09-17".into()),
            provider_native: BTreeMap::new(),
        },
        materialisation: ModelMaterialisationReading {
            binding_ref: "binding/stt-1".into(),
            workcell_ref: None,
            placement: PlacementObservation::Remote,
            endpoint: Some("https://api.example.invalid/v1/audio/transcriptions".into()),
            provider_native: BTreeMap::new(),
            resources: MaterialResourceReading::default(),
            lifetime_owner: "agent-session".into(),
            retraction: RetractionMode::Live,
        },
        model_surface: ModelSurfaceReading {
            contract: Some(r("contract:transcript")),
            protocol: "openai-transcribe-http".into(),
            capabilities: BTreeSet::new(),
            access: ModelAccessReading {
                inference: AccessFieldReading::available(["invoke"]),
                material_control: AccessFieldReading::unavailable("provider owns lifecycle"),
                interior: AccessFieldReading::unavailable("no model-interior seam"),
            },
            modality: Some(modality),
        },
        change_application: RuntimeChangeApplication::Live,
    }
}

fn text_relation() -> ModelRuntimeRelation {
    ModelRuntimeRelation {
        model: ModelVariantReading {
            model: r("model:llama3.2"),
            variant: "llama3.2:latest".into(),
        },
        engine: InferenceEngineReading {
            engine: r("engine/ollama"),
            provider: provider("provider:ollama"),
            form: InferenceEngineForm::ManagedService,
            revision: None,
            provider_native: BTreeMap::new(),
        },
        materialisation: ModelMaterialisationReading {
            binding_ref: "binding/text-1".into(),
            workcell_ref: Some(r("workcell/reference-laptop").to_string()),
            placement: PlacementObservation::Local,
            endpoint: Some("http://127.0.0.1:11434".into()),
            provider_native: BTreeMap::new(),
            resources: MaterialResourceReading::default(),
            lifetime_owner: "workcell".into(),
            retraction: RetractionMode::Restart,
        },
        model_surface: ModelSurfaceReading {
            contract: Some(r("contract:reasoned-text")),
            protocol: "openai-compatible-chat".into(),
            capabilities: BTreeSet::new(),
            access: ModelAccessReading {
                inference: AccessFieldReading::available(["invoke", "stream"]),
                material_control: AccessFieldReading::available(["restart", "stop"]),
                interior: AccessFieldReading::unavailable("no model-interior seam"),
            },
            // A plain text harness stage declares no modality contract.
            modality: None,
        },
        change_application: RuntimeChangeApplication::Live,
    }
}

fn tts_relation() -> ModelRuntimeRelation {
    let mut modality = ModelModalityContract::new(provider("provider:openai"), "gpt-4o-mini-tts");
    modality.input_modalities = BTreeSet::from([ModelModality::Text]);
    modality.output_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
    modality.transforms = BTreeMap::from([(
        TransformCapability::TextToSpeech,
        DeclaredSupport::Supported,
    )]);
    modality.interaction = BTreeSet::from([
        InteractionCapability::RequestResponse,
        InteractionCapability::StreamingOutput,
    ]);
    modality.transport = TransportKind::Http;
    modality.credential = CredentialCondition::Satisfied {
        hint: "openai inference credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    };
    ModelRuntimeRelation {
        model: ModelVariantReading {
            model: r("model:gpt-4o-mini-tts"),
            variant: "gpt-4o-mini-tts".into(),
        },
        engine: InferenceEngineReading {
            engine: r("engine/openai-tts"),
            provider: provider("provider:openai"),
            form: InferenceEngineForm::ManagedService,
            revision: Some("fixture-2026-09-17".into()),
            provider_native: BTreeMap::new(),
        },
        materialisation: ModelMaterialisationReading {
            binding_ref: "binding/tts-1".into(),
            workcell_ref: None,
            placement: PlacementObservation::Remote,
            endpoint: Some("https://api.example.invalid/v1/audio/speech".into()),
            provider_native: BTreeMap::new(),
            resources: MaterialResourceReading::default(),
            lifetime_owner: "agent-session".into(),
            retraction: RetractionMode::Live,
        },
        model_surface: ModelSurfaceReading {
            contract: Some(r("contract:reasoned-text")),
            protocol: "openai-tts-http".into(),
            capabilities: BTreeSet::new(),
            access: ModelAccessReading {
                inference: AccessFieldReading::available(["invoke"]),
                material_control: AccessFieldReading::unavailable("provider owns lifecycle"),
                interior: AccessFieldReading::unavailable("no model-interior seam"),
            },
            modality: Some(modality),
        },
        change_application: RuntimeChangeApplication::Live,
    }
}

fn cascade_stages() -> Vec<ModelStageRelation> {
    vec![
        ModelStageRelation {
            component: r("component/stt-stage"),
            relation: stt_relation(),
        },
        ModelStageRelation {
            component: r("component/text-harness"),
            relation: text_relation(),
        },
        ModelStageRelation {
            component: r("component/tts-stage"),
            relation: tts_relation(),
        },
    ]
}

#[test]
fn a_native_realtime_body_resolves_with_its_declared_modality_set() {
    let composition = resolve_harness_composition(&realtime_catalog(), realtime_request()).unwrap();
    // The body carries the realtime conversation surface and the provider
    // binding resolved through the ordinary contract path.
    assert_eq!(composition.surfaces.len(), 1);
    assert_eq!(composition.surfaces[0].resource, r("surface/realtime-conversation"));
    assert_eq!(composition.contract_bindings.len(), 0,
        "the adapter component requires no upstream contract");
    assert!(composition.model.as_ref().unwrap() == &r("model:gpt-realtime"));

    let read = disclose_model_runtime(&composition, realtime_relation(CredentialCondition::Satisfied {
        hint: "openai inference credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    }))
    .unwrap();

    // The question a consumer asks: is this session speech-capable,
    // full-duplex, barge-in-capable?
    assert_eq!(read.speech_capable(), Some(true));
    assert!(read.interaction_support(InteractionCapability::FullDuplexRealtime).is_supported());
    assert!(read.interaction_support(InteractionCapability::BargeIn).is_supported());
    assert!(read.interaction_support(InteractionCapability::VadTurnDetection).is_supported());
    assert!(read.modality_support(ModalityDirection::Input, ModelModality::Speech).is_supported());
    assert!(read.modality_support(ModalityDirection::Output, ModelModality::Speech).is_supported());
    assert!(read.modality_support(ModalityDirection::Input, ModelModality::Text).is_supported());
    // What the provider does not offer is explicitly unsupported, with the
    // reason naming the surface that withheld it.
    match read.interaction_support(InteractionCapability::PartialTranscripts) {
        ModalitySupport::Unsupported { reason } => assert!(reason.contains("gpt-realtime")),
        other => panic!("partial transcripts must not read as {other:?}"),
    }
    match read.transform_support(TransformCapability::TextToSpeech) {
        ModalitySupport::Unsupported { reason } => assert!(reason.contains("gpt-realtime")),
        other => panic!("text-to-speech must not read as {other:?}"),
    }
    // Transport and connection facts are readable as facts.
    let modality = read.modality().unwrap();
    assert_eq!(modality.transport, TransportKind::WebSocket);
    assert_eq!(
        modality.connection,
        aikit_core::model_modality::ConnectionSemantics::Connected {
            reconnect: aikit_core::model_modality::ReconnectSupport::ReconnectWithoutSession,
        }
    );
    assert_eq!(modality.credential_scope, CredentialScope::EphemeralSurfaceToken);
    // A connected surface that cannot restore session state says so.
    assert!(!matches!(
        modality.connection,
        aikit_core::model_modality::ConnectionSemantics::Connected {
            reconnect: aikit_core::model_modality::ReconnectSupport::Resumable
        }
    ));
}

#[test]
fn realtime_resolution_is_deterministic_and_tool_requests_stay_non_action() {
    let first = resolve_harness_composition(&realtime_catalog(), realtime_request()).unwrap();
    let second = resolve_harness_composition(&realtime_catalog(), realtime_request()).unwrap();
    assert_eq!(first.fingerprint, second.fingerprint);

    let read = disclose_model_runtime(&first, realtime_relation(CredentialCondition::Satisfied {
        hint: "openai inference credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    }))
    .unwrap();

    // The structured tool-request channel is a capability the model may
    // propose through; it is not, and cannot become, a projected native
    // Action.
    let surface: &RuntimeSurfaceReading = &read.surfaces[0];
    assert!(surface.action_refs.is_empty());
    assert!(surface.non_action_refs.contains(&r("capability/realtime-tool-request-channel")));
    assert_eq!(surface.kind, SurfaceKind::Conversation);
    // Declaring the ToolRequests interaction capability grants nothing
    // beyond the channel: the read model's Action surface stays empty even
    // though the contract declares it.
    assert!(read
        .modality()
        .unwrap()
        .interaction
        .contains(&InteractionCapability::ToolRequests));
    assert!(read.surfaces.iter().all(|surface| surface.action_refs.is_empty()));
}

#[test]
fn a_missing_credential_degrades_the_realtime_body_explicitly() {
    let composition = resolve_harness_composition(&realtime_catalog(), realtime_request()).unwrap();
    let read = disclose_model_runtime(
        &composition,
        realtime_relation(CredentialCondition::Required {
            hint: "openai realtime credential".into(),
        }),
    )
    .unwrap();
    let credential = read
        .unavailable
        .iter()
        .find(|unavailability| unavailability.field == "modality-credential")
        .expect("a required-but-missing credential must be an explicit unavailability");
    assert!(credential.reason.contains("openai realtime credential"));
}

#[test]
fn a_cascade_body_resolves_with_per_stage_provider_model_materialisation_relations() {
    let composition = resolve_harness_composition(&cascade_catalog("provider:openai", "provider:deepseek"), cascade_request()).unwrap();
    // The resolver wired the cascade through ordinary contract bindings.
    let pairs: Vec<(&ResourceRef, &ResourceRef)> = composition
        .contract_bindings
        .iter()
        .map(|binding| (&binding.contract, &binding.provider))
        .collect();
    assert!(pairs.contains(&(&r("contract:transcript"), &r("provider:openai"))));
    assert!(pairs.contains(&(&r("contract:reasoned-text"), &r("provider:ollama"))));
    let consumers: Vec<&ResourceRef> = composition
        .contract_bindings
        .iter()
        .map(|binding| &binding.consumer_component)
        .collect();
    assert!(consumers.contains(&&r("component/text-harness")));
    assert!(consumers.contains(&&r("component/tts-stage")));

    let read = disclose_staged_model_runtime(&composition, cascade_stages()).unwrap();
    assert_eq!(read.stages.len(), 3);
    // Each stage keeps its own provider/model/materialisation relation.
    let stt = read.stage(&r("component/stt-stage")).unwrap();
    assert_eq!(stt.relation.model.model, r("model:gpt-4o-transcribe"));
    assert_eq!(stt.relation.engine.provider, provider("provider:openai"));
    assert_eq!(stt.relation.materialisation.placement, PlacementObservation::Remote);
    let text = read.stage(&r("component/text-harness")).unwrap();
    assert_eq!(text.relation.model.model, r("model:llama3.2"));
    assert_eq!(text.relation.materialisation.placement, PlacementObservation::Local);
    let tts = read.stage(&r("component/tts-stage")).unwrap();
    assert_eq!(tts.relation.model.model, r("model:gpt-4o-mini-tts"));

    // The composed body view: speech enters at the first stage and leaves
    // at the last.
    assert!(read.speech_capable());
    let (body_in, body_out) = read.composed_modality.pipeline_modalities();
    assert!(body_in.contains(&ModelModality::Speech));
    assert!(body_out.contains(&ModelModality::Speech));
    assert!(!body_in.contains(&ModelModality::Text), "body input is the first stage's input");

    // Strict derivation: both acoustic stages declare request-response but
    // the text harness declared no contract, so the body-level claim is
    // unknown, not granted.
    match read.interaction_support(InteractionCapability::RequestResponse) {
        ModalitySupport::Unknown { reason } => {
            assert!(reason.contains("no modality contract"));
        }
        other => panic!("body-level request-response must not read as {other:?}"),
    }
    // Streaming output is explicitly withheld by the STT stage, so the body
    // cannot claim it — and the reason names who withheld.
    match read.interaction_support(InteractionCapability::StreamingOutput) {
        ModalitySupport::Unsupported { reason } => {
            assert!(reason.contains("component/stt-stage"));
        }
        other => panic!("body-level streaming-output must not read as {other:?}"),
    }
}

#[test]
fn the_cascade_explains_why_it_is_speech_capable_with_stage_provenance() {
    let composition = resolve_harness_composition(&cascade_catalog("provider:openai", "provider:deepseek"), cascade_request()).unwrap();
    let read = disclose_staged_model_runtime(&composition, cascade_stages()).unwrap();
    let evidence = explain_staged_model_runtime(&read);

    let stage_facts: Vec<&aikit_core::explain_history::ExplainFact> = evidence
        .facts
        .iter()
        .filter(|fact| fact.relation == "stage-model-relation")
        .collect();
    assert_eq!(stage_facts.len(), 3);
    assert!(stage_facts.iter().any(|fact| {
        fact.summary.contains("component/stt-stage")
            && fact.summary.contains("gpt-4o-transcribe")
            && fact.summary.contains("provider:openai")
    }));
    // Provenance carries the provider-native spellings, never as identity.
    let stt_fact = stage_facts
        .iter()
        .find(|fact| fact.summary.contains("component/stt-stage"))
        .unwrap();
    assert_eq!(stt_fact.provenance[0].native_id.as_deref(), Some("gpt-4o-transcribe"));
    assert_eq!(stt_fact.provenance[0].provider.as_ref().unwrap().as_str(), "provider:openai");
    assert!(stt_fact.canonical_refs.contains(&r("model:gpt-4o-transcribe")));

    assert!(evidence.facts.iter().any(|fact| {
        fact.relation == "body-speech-capability"
            && fact.summary.contains("carries speech in and out")
    }));
    assert!(evidence
        .facts
        .iter()
        .any(|fact| fact.relation == "body-modality-basis"
            && fact.summary.contains("component/tts-stage")));
}

#[test]
fn body_provider_replacement_changes_facts_never_agent_identity() {
    let before =
        resolve_harness_composition(&cascade_catalog("provider:openai", "provider:deepseek"), cascade_request()).unwrap();
    // The alternative provider wins the transcript contract: a provider
    // replacement through ordinary resolution, nothing else changed.
    let after =
        resolve_harness_composition(&cascade_catalog("provider:deepseek", "provider:openai"), cascade_request()).unwrap();

    assert_eq!(before.harness, after.harness);
    assert_eq!(before.project, after.project);
    assert_eq!(before.agent, after.agent);
    assert_eq!(before.agency, after.agency);
    assert_eq!(before.session, after.session);
    assert_ne!(before.fingerprint, after.fingerprint, "the body facts did change");

    // The diff is explainable as a provider rebind.
    let diff = diff_harness_compositions(&before, &after).unwrap();
    let rebind = diff
        .rebound_contracts
        .iter()
        .find(|binding| binding.contract == r("contract:transcript"))
        .expect("the transcript contract must show a rebind");
    assert_eq!(rebind.before_provider, r("provider:openai"));
    assert_eq!(rebind.after_provider, r("provider:deepseek"));

    // The replaced stage keeps the same component identity: the stage is a
    // binding, not an Agent.
    let before_read = disclose_staged_model_runtime(&before, cascade_stages()).unwrap();
    let after_read = disclose_staged_model_runtime(&after, cascade_stages()).unwrap();
    assert_eq!(
        before_read.stage(&r("component/stt-stage")).unwrap().component,
        after_read.stage(&r("component/stt-stage")).unwrap().component
    );
    assert_eq!(before_read.agent_session, after_read.agent_session);
}

#[test]
fn a_text_only_surface_reports_absent_speech_honestly() {
    // A declared text-only contract: speech is proven-absent, with the
    // reason naming the surface.
    let mut text_only = ModelModalityContract::new(provider("provider:ollama"), "llama3.2:latest");
    text_only.input_modalities = BTreeSet::from([ModelModality::Text]);
    text_only.output_modalities = BTreeSet::from([ModelModality::Text]);
    text_only.transport = TransportKind::Http;
    text_only.validate().unwrap();
    assert!(!text_only.is_speech_capable());
    match text_only.input_support(ModelModality::Speech) {
        ModalitySupport::Unsupported { reason } => {
            assert!(reason.contains("llama3.2:latest"));
        }
        other => panic!("declared text-only input must not read as {other:?}"),
    }
    match text_only.transform_support(TransformCapability::SpeechToText) {
        ModalitySupport::Unsupported { .. } => {}
        other => panic!("speech-to-text must not read as {other:?}"),
    }

    // A surface that declared no contract at all is unproven, not refuted.
    let composition = resolve_harness_composition(&cascade_catalog("provider:openai", "provider:deepseek"), cascade_request()).unwrap();
    let read = disclose_staged_model_runtime(&composition, cascade_stages()).unwrap();
    let text_stage = read.stage(&r("component/text-harness")).unwrap();
    assert!(text_stage.relation.model_surface.modality.is_none());
    assert!(matches!(
        aikit_core::model_modality::surface_interaction_support(
            text_stage.relation.model_surface.modality.as_ref(),
            InteractionCapability::BargeIn
        ),
        ModalitySupport::Unknown { .. }
    ));
}

#[test]
fn the_realtime_explanation_carries_provider_native_provenance() {
    let composition = resolve_harness_composition(&realtime_catalog(), realtime_request()).unwrap();
    let read = disclose_model_runtime(&composition, realtime_relation(CredentialCondition::Satisfied {
        hint: "openai inference credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    }))
    .unwrap();
    let evidence = explain_model_modality(&read);
    assert_eq!(evidence.subject, r("harness/realvoice"));
    let full_duplex = evidence
        .facts
        .iter()
        .find(|fact| fact.summary.contains("full-duplex realtime"))
        .expect("the explanation must answer the full-duplex question");
    assert!(full_duplex.summary.contains("is supported"));
    assert_eq!(full_duplex.provenance[0].native_id.as_deref(), Some("gpt-realtime"));
    assert!(evidence
        .facts
        .iter()
        .any(|fact| fact.summary.contains("barge-in is supported")));
    assert!(evidence
        .facts
        .iter()
        .any(|fact| fact.summary.contains("input declared as") && fact.summary.contains("speech")));
    assert!(evidence
        .facts
        .iter()
        .any(|fact| fact.summary.contains("websocket")));
}

#[test]
fn roster_gates_refuse_a_speech_demand_against_a_text_only_candidate() {
    // The typed contract feeds the existing roster seam through flat tags.
    let mut text_only = ModelModalityContract::new(provider("provider:ollama"), "llama3.2:latest");
    text_only.input_modalities = BTreeSet::from([ModelModality::Text]);
    text_only.output_modalities = BTreeSet::from([ModelModality::Text]);
    let speech = realtime_modality(CredentialCondition::NotRequired);

    let demand = ModelRosterDemand {
        project: None,
        profile: None,
        agency: None,
        use_type: "voice".into(),
        required_capabilities: BTreeSet::new(),
        required_modalities: BTreeSet::from(["speech".into()]),
        required_tools: BTreeSet::new(),
        required_contracts: BTreeSet::new(),
        context_characteristics: BTreeSet::new(),
        independence_from: BTreeSet::new(),
        estimated_input_tokens: None,
        estimated_output_tokens: None,
        cost_ceiling_usd: None,
    };
    let candidate = |model: &str, modalities: BTreeSet<String>, capabilities: BTreeSet<String>| {
        ModelRosterCandidate {
            model: r(model),
            variant: model.to_string(),
            provider: provider("provider:example"),
            provider_revision: None,
            available: true,
            authorised: true,
            provider_usable: true,
            policy_allowed: true,
            contract_compatible: true,
            harness_compatible: true,
            harness_composition: None,
            native_capabilities: capabilities,
            harness_capabilities: BTreeSet::new(),
            profile_skills: BTreeSet::new(),
            modalities,
            tool_support: BTreeSet::new(),
            contracts: BTreeSet::new(),
            task_fitness: BTreeMap::new(),
            role_fitness: BTreeMap::new(),
            profile_fit: None,
            authored_preference: None,
            frecency: None,
            latency_ms: None,
            reliability: None,
            context_window_tokens: None,
            price: None,
            exact_spend: Vec::new(),
            observed_fitness: Vec::new(),
            access: Default::default(),
            provenance: Vec::new(),
        }
    };
    let roster = rank_model_roster(
        demand,
        ModelRankingPolicy::TaskFit,
        vec![
            candidate("model:llama3.2", text_only.modality_tags(), text_only.capability_tags()),
            candidate("model:gpt-realtime", speech.modality_tags(), speech.capability_tags()),
        ],
    );
    assert_eq!(roster.entries[0].model, r("model:gpt-realtime"));
    let text_entry = roster
        .entries
        .iter()
        .find(|entry| entry.model == r("model:llama3.2"))
        .unwrap();
    assert!(!text_entry.explanation.eligible);
    assert!(text_entry
        .explanation
        .failed_gates
        .contains(&"modality:speech".to_string()));
    // Silence about the requested term is not allowed either way: the
    // unsupported candidate states the failed gate, the supported one the
    // passed gate.
    let speech_entry = roster.entries.first().unwrap();
    assert!(speech_entry.explanation.hard_gates.contains(&"modality:speech".to_string()));
    assert!(speech
        .capability_tags()
        .contains(&"full-duplex-realtime".to_string()));
    assert!(RequirementStrength::Required.is_required());
}
