//! The OpenAI Realtime provider adapter — the first realtime adapter
//! instance, deliberately an adapter and not an ontology.
//!
//! Its whole job is one direction of translation: a recorded/observed OpenAI
//! Realtime session document (frozen fixture in `tests/fixtures/
//! openai-realtime/`) becomes a generic
//! [`ModelModalityContract`] on the existing Model/provider/Contract seams.
//! Everything provider-specific — voice names, audio format spellings,
//! tool definitions, session instructions — is validated and then dropped:
//! the generic types have no seam for it, so none of it can leak into a
//! resolution read model, a log or a history entry.
//!
//! Facts this adapter declares are declared, not observed: a session
//! document proves what the provider's session shape supports, never that a
//! route is presently usable. Route observation stays Actuation's join;
//! credential usability stays the credential condition on the route. The
//! reconnection fact is the provider's own documented behaviour (a dropped
//! WebSocket cannot resume the prior session), so the honest generic
//! statement is [`ReconnectSupport::ReconnectWithoutSession`] — caller-owned
//! state is what survives, and the contract says so rather than promising
//! conversational continuity the provider does not restore.

use std::collections::BTreeSet;

use aikit_core::model_modality::{
    ConnectionSemantics, CredentialScope, DeclaredSupport, InteractionCapability, ModelModality,
    ModelModalityContract, ReconnectSupport, SurfaceAvailability, TransformCapability,
    TransportKind,
};
use aikit_core::resource::{CredentialCondition, DeclaredRoute, ModelRouteKind, ProviderRef};
use aikit_core::{AikitError, Result};

pub const OPENAI_REALTIME_PROVIDER: &str = "provider:openai";
pub const OPENAI_REALTIME_ROUTE_MODEL: &str = "gpt-realtime";
pub const OPENAI_REALTIME_WS_ENDPOINT: &str = "wss://api.openai.com/v1/realtime";

/// The pinned revision of the frozen session fixture these mappings were
/// conformed against. Like the source-pinned conformance revisions in
/// `model_runtime`, it is an evidence pin, never provider or Model identity.
pub const OPENAI_REALTIME_ADAPTER_REVISION: &str = "fixture:openai-realtime-session/2026-09-17";

fn openai_provider() -> ProviderRef {
    ProviderRef::parse(OPENAI_REALTIME_PROVIDER).expect("openai provider ref")
}

/// The declared route this adapter serves: the provider's own realtime
/// model, addressed natively, needing a credential. Declared is not
/// available — the catalogue-vs-detection join decides that.
pub fn declared_realtime_route() -> DeclaredRoute {
    DeclaredRoute {
        provider: openai_provider(),
        kind: ModelRouteKind::ProviderNative,
        provider_native_ids: vec![OPENAI_REALTIME_ROUTE_MODEL.to_string()],
        endpoint: Some(OPENAI_REALTIME_WS_ENDPOINT.to_string()),
        credential: CredentialCondition::Required {
            hint: "openai realtime credential".into(),
        },
    }
}

/// Parse an OpenAI Realtime session document into the generic modality
/// contract.
///
/// Recognised and translated: the session's model, output modalities, audio
/// input format, input transcription, turn detection (type and
/// `interrupt_response`) and tools. Recognised and deliberately dropped:
/// voice, instructions, format spellings, noise reduction and tool
/// definitions — provider-private session configuration with no generic
/// seam. Anything outside both lists is a refusal, never a coercion.
pub fn parse_realtime_session(
    session_json: &str,
    credential: CredentialCondition,
) -> Result<ModelModalityContract> {
    let invalid =
        |message: &str| AikitError::new("openai_realtime.unparsable_session", message.to_string());
    let value: serde_json::Value = serde_json::from_str(session_json)
        .map_err(|error| invalid(&format!("session document is not valid JSON: {error}")))?;
    let session = value
        .get("session")
        .ok_or_else(|| invalid("session document carries no `session` object"))?;

    let model = session
        .get("model")
        .and_then(serde_json::Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| invalid("session carries no usable `model`"))?;

    let mut contract = ModelModalityContract::new(openai_provider(), model);
    contract.schema = aikit_core::model_modality::MODEL_MODALITY_VERSION.to_string();
    contract.provenance = vec![OPENAI_REALTIME_ADAPTER_REVISION.to_string()];
    contract.transport = TransportKind::WebSocket;
    contract.connection = ConnectionSemantics::Connected {
        reconnect: ReconnectSupport::ReconnectWithoutSession,
    };
    contract.credential_scope = CredentialScope::EphemeralSurfaceToken;
    contract.availability = SurfaceAvailability::Available;
    contract.credential = credential;

    contract.input_modalities.insert(ModelModality::Text);
    if session.get("input_audio_format").is_some() {
        contract.input_modalities.insert(ModelModality::Audio);
        contract.input_modalities.insert(ModelModality::Speech);
    }

    let output_modalities = session
        .get("output_modalities")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid("session carries no `output_modalities` array"))?;
    if output_modalities.is_empty() {
        return Err(invalid("session declares no output modalities"));
    }
    for modality in output_modalities {
        match modality.as_str() {
            Some("audio") => {
                contract.output_modalities.insert(ModelModality::Audio);
                contract.output_modalities.insert(ModelModality::Speech);
            }
            Some("text") => {
                contract.output_modalities.insert(ModelModality::Text);
            }
            other => {
                return Err(invalid(&format!(
                    "session declares output modality {other:?}, which this adapter revision does not understand"
                )))
            }
        }
    }

    // Interaction facts inherent to the realtime protocol.
    contract.interaction = BTreeSet::from([
        InteractionCapability::StreamingInput,
        InteractionCapability::StreamingOutput,
        InteractionCapability::FullDuplexRealtime,
        InteractionCapability::StructuredEvents,
    ]);

    // Turn detection and interruption, exactly as the session declares them.
    let turn_detection = session.get("turn_detection");
    let vad = turn_detection
        .and_then(|node| node.get("type"))
        .and_then(serde_json::Value::as_str);
    let interrupts = turn_detection
        .and_then(|node| node.get("interrupt_response"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    match vad {
        Some("semantic_vad") | Some("server_vad") => {
            contract.interaction.insert(InteractionCapability::VadTurnDetection);
            if interrupts {
                contract.interaction.insert(InteractionCapability::BargeIn);
            }
        }
        Some("none") | None => {
            if interrupts {
                return Err(invalid(
                    "session declares interrupt_response without any turn detection to interrupt against",
                ));
            }
        }
        Some(other) => {
            return Err(invalid(&format!(
                "session declares turn detection {other:?}, which this adapter revision does not understand"
            )))
        }
    }

    // Input transcription: a committed final transcript per utterance. The
    // contract does not claim partials or timestamps the fixture does not
    // prove.
    if session
        .get("input_audio_transcription")
        .and_then(|node| node.get("model"))
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        contract
            .interaction
            .insert(InteractionCapability::FinalTranscripts);
    }

    // Structured tool requests: the session may carry tool definitions, and
    // that makes the channel exist — it grants no authority, which is a
    // property of the generic vocabulary, not of this provider.
    let has_tools = session
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if has_tools {
        contract
            .interaction
            .insert(InteractionCapability::ToolRequests);
    }

    // Transform capabilities derived from the declared modalities.
    let acoustic_in = contract
        .input_modalities
        .iter()
        .any(|modality| modality.is_acoustic());
    let acoustic_out = contract
        .output_modalities
        .iter()
        .any(|modality| modality.is_acoustic());
    if acoustic_in {
        contract.transforms.insert(
            TransformCapability::AudioUnderstanding,
            DeclaredSupport::Supported,
        );
    }
    if acoustic_in && acoustic_out {
        contract.transforms.insert(
            TransformCapability::SpeechToSpeech,
            DeclaredSupport::Supported,
        );
    }
    if contract.input_modalities.contains(&ModelModality::Text)
        && contract.output_modalities.contains(&ModelModality::Text)
        && acoustic_in
        && acoustic_out
    {
        contract.transforms.insert(
            TransformCapability::MultimodalTextAudio,
            DeclaredSupport::Supported,
        );
    }

    contract.validate()?;
    Ok(contract)
}

/// The declared speech-to-text surface: the transcription API over HTTP,
/// request/response, audio and speech in, text out, committed finals.
/// Declared facts pinned to the adapter revision, not observed availability.
pub fn transcription_surface(credential: CredentialCondition) -> ModelModalityContract {
    let mut contract = ModelModalityContract::new(openai_provider(), "gpt-4o-transcribe");
    contract.input_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
    contract.output_modalities = BTreeSet::from([ModelModality::Text]);
    contract.transforms.insert(
        TransformCapability::SpeechToText,
        DeclaredSupport::Supported,
    );
    contract.transforms.insert(
        TransformCapability::AudioUnderstanding,
        DeclaredSupport::Supported,
    );
    contract.interaction = BTreeSet::from([
        InteractionCapability::RequestResponse,
        InteractionCapability::FinalTranscripts,
    ]);
    contract.transport = TransportKind::Http;
    contract.connection = ConnectionSemantics::Stateless;
    contract.credential = credential;
    contract.provenance = vec![OPENAI_REALTIME_ADAPTER_REVISION.to_string()];
    contract
}

/// The declared text-to-speech surface: the speech API over HTTP,
/// request/response, text in, audio and speech out.
pub fn speech_synthesis_surface(credential: CredentialCondition) -> ModelModalityContract {
    let mut contract = ModelModalityContract::new(openai_provider(), "gpt-4o-mini-tts");
    contract.input_modalities = BTreeSet::from([ModelModality::Text]);
    contract.output_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
    contract.transforms.insert(
        TransformCapability::TextToSpeech,
        DeclaredSupport::Supported,
    );
    contract.interaction = BTreeSet::from([
        InteractionCapability::RequestResponse,
        InteractionCapability::StreamingOutput,
    ]);
    contract.transport = TransportKind::Http;
    contract.connection = ConnectionSemantics::Stateless;
    contract.credential = credential;
    contract.provenance = vec![OPENAI_REALTIME_ADAPTER_REVISION.to_string()];
    contract
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::composition::{
        resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentDescriptor,
        ComponentSelection, CompositionActivationMode, CompositionCatalog,
        HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
        SurfaceDescriptor, SurfaceKind, TargetNativeComponentBinding,
    };
    use aikit_core::composition::{
        ComponentContribution, ContributionKind, RetractionMode,
    };
    use aikit_core::model_runtime::{
        disclose_model_runtime, InferenceEngineForm, InferenceEngineReading, ModelRuntimeRelation,
        ModelSurfaceReading, ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
    };

    const FROZEN_SESSION: &str = include_str!("../tests/fixtures/openai-realtime/session.json");

    fn bound_credential() -> CredentialCondition {
        CredentialCondition::Satisfied {
            hint: "openai realtime credential".into(),
            binding_ref: "credential-binding/openai-1".into(),
        }
    }

    #[test]
    fn the_frozen_session_resolves_to_the_generic_contract_without_provider_leakage() {
        let contract = parse_realtime_session(FROZEN_SESSION, bound_credential()).unwrap();
        assert_eq!(contract.provider_native_surface, "gpt-realtime");
        assert!(contract.input_support(ModelModality::Speech).is_supported());
        assert!(contract
            .output_support(ModelModality::Speech)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::FullDuplexRealtime)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::VadTurnDetection)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::BargeIn)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::FinalTranscripts)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::ToolRequests)
            .is_supported());
        assert!(contract
            .transform_support(TransformCapability::SpeechToSpeech)
            .is_supported());
        assert_eq!(contract.transport, TransportKind::WebSocket);
        assert_eq!(
            contract.connection,
            ConnectionSemantics::Connected {
                reconnect: ReconnectSupport::ReconnectWithoutSession
            }
        );
        assert_eq!(
            contract.credential_scope,
            CredentialScope::EphemeralSurfaceToken
        );
        assert!(contract.is_speech_capable());

        // No provider-private vocabulary survives the translation: the
        // voice, the format spellings and the tool name are session
        // configuration, not generic capability facts.
        let rendered = serde_json::to_string(&contract).unwrap();
        for leaked in [
            "marin",
            "pcm16",
            "look_up_schedule",
            "near_field",
            "helpful voice assistant",
        ] {
            assert!(
                !rendered.contains(leaked),
                "provider-specific `{leaked}` must not leak into the generic contract"
            );
        }
        // The evidence pin does travel, as provenance.
        assert!(contract
            .provenance
            .contains(&OPENAI_REALTIME_ADAPTER_REVISION.to_string()));
    }

    #[test]
    fn interruption_requires_declared_turn_detection_and_absence_is_explicit() {
        let without_interrupt = r#"{"session":{
            "model":"gpt-realtime","output_modalities":["audio","text"],
            "input_audio_format":"pcm16",
            "turn_detection":{"type":"semantic_vad","interrupt_response":false}
        }}"#;
        let contract =
            parse_realtime_session(without_interrupt, CredentialCondition::NotRequired).unwrap();
        assert!(contract
            .interaction_support(InteractionCapability::VadTurnDetection)
            .is_supported());
        match contract.interaction_support(InteractionCapability::BargeIn) {
            aikit_core::model_modality::ModalitySupport::Unsupported { .. } => {}
            other => panic!("uninterruptible session must read as unsupported, got {other:?}"),
        }

        let no_vad = r#"{"session":{
            "model":"gpt-realtime","output_modalities":["text"],
            "turn_detection":{"type":"none"}
        }}"#;
        let contract = parse_realtime_session(no_vad, CredentialCondition::NotRequired).unwrap();
        assert!(matches!(
            contract.interaction_support(InteractionCapability::VadTurnDetection),
            aikit_core::model_modality::ModalitySupport::Unsupported { .. }
        ));

        let inconsistent = r#"{"session":{
            "model":"gpt-realtime","output_modalities":["text"],
            "turn_detection":{"type":"none","interrupt_response":true}
        }}"#;
        let error =
            parse_realtime_session(inconsistent, CredentialCondition::NotRequired).unwrap_err();
        assert_eq!(error.code(), "openai_realtime.unparsable_session");
    }

    #[test]
    fn an_unrecognised_output_modality_is_refused_not_coerced() {
        let future = r#"{"session":{
            "model":"gpt-realtime","output_modalities":["audio","video"]
        }}"#;
        let error = parse_realtime_session(future, CredentialCondition::NotRequired).unwrap_err();
        assert!(error.to_string().contains("video"));
    }

    #[test]
    fn a_session_without_audio_input_declares_no_acoustic_input_or_speech_transforms() {
        let text_only_realtime = r#"{"session":{
            "model":"gpt-realtime","output_modalities":["text"]
        }}"#;
        let contract =
            parse_realtime_session(text_only_realtime, CredentialCondition::NotRequired).unwrap();
        assert!(matches!(
            contract.input_support(ModelModality::Speech),
            aikit_core::model_modality::ModalitySupport::Unsupported { .. }
        ));
        assert!(!contract.is_speech_capable());
        assert!(matches!(
            contract.transform_support(TransformCapability::SpeechToSpeech),
            aikit_core::model_modality::ModalitySupport::Unsupported { .. }
        ));
    }

    #[test]
    fn the_adapters_contract_attaches_to_resolution_and_reads_back_speech_capable() {
        // Build the realtime body through the ordinary resolver, exactly as
        // a caller would: adapter contract in, read model out.
        let mut catalog = CompositionCatalog::default();
        let mut adapter = ComponentDescriptor::new(
            aikit_core::resource::ResourceRef::parse("component/openai-realtime-adapter").unwrap(),
        );
        adapter.implementation = Some(TargetNativeComponentBinding {
            implementation_target: "openai-realtime".into(),
            native_id: "gpt-realtime".into(),
            revision: Some(OPENAI_REALTIME_ADAPTER_REVISION.into()),
        });
        adapter.activation_modes = BTreeSet::from([CompositionActivationMode::LiveMounted]);
        adapter.contributions = vec![ComponentContribution {
            id: aikit_core::resource::ResourceRef::parse("contribution/realtime-surface").unwrap(),
            component: adapter.resource.clone(),
            kind: ContributionKind::ModelAdapter,
            target_contract: None,
            exposed_ref: None,
            exposed_kind: None,
            surface: Some(
                aikit_core::resource::ResourceRef::parse("surface/realtime-conversation").unwrap(),
            ),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
            activation_mode: CompositionActivationMode::LiveMounted,
            retraction_mode: RetractionMode::Live,
            provenance: vec![OPENAI_REALTIME_ADAPTER_REVISION.into()],
        }];
        catalog.insert_component(adapter);
        catalog.insert_surface(SurfaceDescriptor {
            resource: aikit_core::resource::ResourceRef::parse("surface/realtime-conversation")
                .unwrap(),
            kind: SurfaceKind::Conversation,
            target_native_id: Some(OPENAI_REALTIME_WS_ENDPOINT.into()),
            owner_component: Some(
                aikit_core::resource::ResourceRef::parse("component/openai-realtime-adapter")
                    .unwrap(),
            ),
        });

        let composition = resolve_harness_composition(
            &catalog,
            HarnessCompositionRequest {
                harness: aikit_core::resource::ResourceRef::parse("harness/realvoice").unwrap(),
                project: None,
                agent: None,
                agency: None,
                session: Some("agent-session-9".into()),
                model: Some(
                    aikit_core::resource::ResourceRef::parse("model:gpt-realtime").unwrap(),
                ),
                selections: vec![ComponentSelection {
                    component: aikit_core::resource::ResourceRef::parse(
                        "component/openai-realtime-adapter",
                    )
                    .unwrap(),
                    resolution_scope: ResolutionScope::new(
                        aikit_core::scope::ScopeKind::Project,
                        "project/voice",
                    ),
                    activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
                    lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
                    activation_mode: CompositionActivationMode::LiveMounted,
                }],
                target_revision: None,
                generation: None,
            },
        )
        .unwrap();

        let relation = ModelRuntimeRelation {
            model: ModelVariantReading {
                model: aikit_core::resource::ResourceRef::parse("model:gpt-realtime").unwrap(),
                variant: "gpt-realtime".into(),
            },
            engine: InferenceEngineReading {
                engine: aikit_core::resource::ResourceRef::parse("engine/openai-realtime").unwrap(),
                provider: openai_provider(),
                form: InferenceEngineForm::ManagedService,
                revision: Some(OPENAI_REALTIME_ADAPTER_REVISION.into()),
                provider_native: Default::default(),
            },
            materialisation: aikit_core::model_runtime::ModelMaterialisationReading {
                binding_ref: "binding/realtime-9".into(),
                workcell_ref: None,
                placement: PlacementObservation::Remote,
                endpoint: Some(OPENAI_REALTIME_WS_ENDPOINT.into()),
                provider_native: Default::default(),
                resources: Default::default(),
                lifetime_owner: "agent-session".into(),
                retraction: RetractionMode::Live,
            },
            model_surface: ModelSurfaceReading {
                contract: None,
                protocol: "openai-realtime-websocket".into(),
                capabilities: Default::default(),
                access: aikit_core::model_runtime::ModelAccessReading {
                    inference: aikit_core::model_runtime::AccessFieldReading::available([
                        "invoke",
                        "stream",
                        "interrupt",
                    ]),
                    material_control: aikit_core::model_runtime::AccessFieldReading::unavailable(
                        "provider owns lifecycle",
                    ),
                    interior: aikit_core::model_runtime::AccessFieldReading::unavailable(
                        "no model-interior seam",
                    ),
                },
                modality: Some(parse_realtime_session(FROZEN_SESSION, bound_credential()).unwrap()),
            },
            change_application: RuntimeChangeApplication::Live,
        };
        let read = disclose_model_runtime(&composition, relation).unwrap();
        assert_eq!(read.speech_capable(), Some(true));
        assert!(read
            .interaction_support(InteractionCapability::BargeIn)
            .is_supported());
        assert!(read
            .interaction_support(InteractionCapability::FullDuplexRealtime)
            .is_supported());
    }

    #[test]
    fn stt_and_tts_declare_independent_surfaces_with_honest_gaps() {
        let stt = transcription_surface(CredentialCondition::NotRequired);
        assert!(stt
            .transform_support(TransformCapability::SpeechToText)
            .is_supported());
        assert!(stt
            .interaction_support(InteractionCapability::FinalTranscripts)
            .is_supported());
        // The transcription surface proves no timestamps here, so none are
        // claimed.
        assert!(matches!(
            stt.interaction_support(InteractionCapability::Timestamps),
            aikit_core::model_modality::ModalitySupport::Unsupported { .. }
        ));
        assert!(!stt.is_speech_capable(), "STT listens; it does not speak");

        let tts = speech_synthesis_surface(CredentialCondition::NotRequired);
        assert!(tts
            .transform_support(TransformCapability::TextToSpeech)
            .is_supported());
        assert!(tts
            .interaction_support(InteractionCapability::StreamingOutput)
            .is_supported());
        assert!(!tts.is_speech_capable(), "TTS speaks; it does not listen");

        // Together they compose the cascade's acoustic endpoints; the
        // realtime route stays a separate declaration.
        let route = declared_realtime_route();
        assert_eq!(route.provider_native_ids, ["gpt-realtime"]);
        assert_eq!(route.kind, ModelRouteKind::ProviderNative);
        assert!(route.credential.requires_credential());
    }

    #[test]
    fn unparsable_session_documents_are_refused_with_a_reason() {
        for broken in ["", "not json", "{}", r#"{"session":{}}"#] {
            let error = parse_realtime_session(broken, CredentialCondition::NotRequired);
            assert_eq!(
                error.unwrap_err().code(),
                "openai_realtime.unparsable_session",
                "session document {broken:?} must be refused with the adapter's code"
            );
        }
    }
}
