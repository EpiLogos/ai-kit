//! The local speech adapter — the second provider wire, and the swap path's
//! keyless worked example.
//!
//! Two services on the development machine make a fully local speech body
//! resolve: whisper.cpp's `whisper-server` for speech-to-text and a
//! Kokoro-82M ONNX wrapper for text-to-speech. Both are OpenAI-shaped,
//! stateless, request/response HTTP, and need no credential. This module is
//! one adapter instance for that wire, following the `openai_realtime`
//! pattern exactly: verbatim captured documents (frozen in
//! `tests/fixtures/local-speech/`, pinned by
//! [`LOCAL_SPEECH_ADAPTER_REVISION`]) are translated into the generic
//! [`ModelModalityContract`]; provider-private configuration is validated
//! and dropped; anything the captures do not prove is not declared.
//!
//! One module, not two: the repo's adapter pattern is one provider wire per
//! module with one [`declared_surfaces`] inventory the catalogue join
//! consumes (the OpenAI module declares three surfaces the same way). The
//! local speech stack is one wire carrying two provider identities, so both
//! surfaces live here behind their own parse function.
//!
//! Route honesty, stated where it belongs: the catalogue and this adapter
//! declare facts, never a live route. Nothing here claims a service is
//! running; observation stays the compose join. And the STT route delta is
//! a declared fact, not a hidden one: this whisper.cpp build serves
//! `POST /inference` (OpenAI-shaped body, non-OpenAI path) and does **not**
//! serve `/v1/audio/transcriptions` — the override travels on the declared
//! route's endpoint and in the surface's material constraint notes.

use std::collections::BTreeSet;

use aikit_core::model_modality::{
    ConnectionSemantics, DeclaredSupport, InteractionCapability, ModelModality,
    ModelModalityContract, SurfaceAvailability, TransformCapability, TransportKind,
};
use aikit_core::resource::{CredentialCondition, DeclaredRoute, ModelRouteKind, ProviderRef};
use aikit_core::{AikitError, Result};

pub const LOCAL_WHISPER_PROVIDER: &str = "provider:local-whisper-cpp";
pub const LOCAL_KOKORO_PROVIDER: &str = "provider:local-kokoro";

/// The provider-native id of the served STT model (the loaded ggml model
/// file's own name). The STT wire does not name its model in the response
/// document — the capture context does — so this id is a constant of this
/// adapter revision, pinned by the frozen provenance.
pub const LOCAL_WHISPER_STT_MODEL: &str = "whisper-large-v3-turbo-q5_0";

pub const LOCAL_WHISPER_INFERENCE_ENDPOINT: &str = "http://127.0.0.1:8080/inference";
pub const LOCAL_KOKORO_SPEECH_ENDPOINT: &str = "http://127.0.0.1:8880/v1/audio/speech";

/// The pinned revision of the frozen captures these mappings were conformed
/// against (capture date 2026-09-19; see the fixtures' PROVENANCE.md). An
/// evidence pin, never provider or Model identity.
pub const LOCAL_SPEECH_ADAPTER_REVISION: &str = "fixture:local-speech-captures/2026-09-19";

/// The frozen verbatim captures. One recording, one truth: the conformance
/// tests, the declared surfaces and the round-trip evidence all read these
/// documents.
pub const FROZEN_STT_RESPONSE: &str =
    include_str!("../tests/fixtures/local-speech/whisper-stt-response.json");
pub const FROZEN_TTS_REQUEST: &str =
    include_str!("../tests/fixtures/local-speech/kokoro-tts-request.json");
pub const FROZEN_ROUNDTRIP_RESPONSE: &str =
    include_str!("../tests/fixtures/local-speech/roundtrip-stt-response.json");

fn whisper_provider() -> ProviderRef {
    ProviderRef::parse(LOCAL_WHISPER_PROVIDER).expect("local whisper provider ref")
}

fn kokoro_provider() -> ProviderRef {
    ProviderRef::parse(LOCAL_KOKORO_PROVIDER).expect("local kokoro provider ref")
}

/// The declared routes of the local wire: OpenAI-shaped bodies over local
/// HTTP, no credential. The STT endpoint carries the `/inference` path
/// override in plain sight. Declared is not running — nothing here claims
/// the services are up; that observation stays the compose join.
pub fn local_speech_routes() -> Vec<DeclaredRoute> {
    vec![
        DeclaredRoute {
            provider: whisper_provider(),
            kind: ModelRouteKind::LocalServing,
            provider_native_ids: vec![LOCAL_WHISPER_STT_MODEL.to_string()],
            endpoint: Some(LOCAL_WHISPER_INFERENCE_ENDPOINT.to_string()),
            credential: CredentialCondition::NotRequired,
        },
        DeclaredRoute {
            provider: kokoro_provider(),
            kind: ModelRouteKind::LocalServing,
            provider_native_ids: vec!["kokoro-82m".to_string()],
            endpoint: Some(LOCAL_KOKORO_SPEECH_ENDPOINT.to_string()),
            credential: CredentialCondition::NotRequired,
        },
    ]
}

/// The facts both local surfaces share: plain HTTP, one complete response
/// per request, no credential, available as declared, pinned to the frozen
/// captures.
fn stateless_http_facts(contract: &mut ModelModalityContract) {
    contract.transport = TransportKind::Http;
    contract.connection = ConnectionSemantics::Stateless;
    contract.credential = CredentialCondition::NotRequired;
    contract.availability = SurfaceAvailability::Available;
    contract.provenance = vec![LOCAL_SPEECH_ADAPTER_REVISION.to_string()];
}

/// The streaming refusal both local surfaces share, said in the facts:
/// these services return one complete response per request and do not
/// stream, so no streaming interaction form is declared. Absence from the
/// declared set is the explicit unsupported answer (queries widen it with
/// the surface's name); the reason why travels as material constraint
/// notes, which the read models and the disclosure render verbatim.
fn say_no_streaming(contract: &mut ModelModalityContract, detail: &str) {
    contract
        .constraints
        .notes
        .insert("streaming".into(), detail.into());
}

/// Parse a verbatim whisper.cpp transcription reply (`{"text": ...}`) into
/// the generic STT surface contract.
///
/// The capture proves: a request produced one committed final transcript —
/// request/response over HTTP, final transcripts, speech-to-text. It
/// proves no partials, no timestamps, no streaming, so none of those are
/// declared. The transcript's own words are content, not capability: they
/// are read and dropped, never carried on the contract.
pub fn parse_transcription_capture(stt_response_json: &str) -> Result<ModelModalityContract> {
    let invalid =
        |message: &str| AikitError::new("local_speech.unparsable_stt_capture", message.to_string());
    let value: serde_json::Value = serde_json::from_str(stt_response_json)
        .map_err(|error| invalid(&format!("STT capture is not valid JSON: {error}")))?;
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| invalid("STT capture carries no usable `text` transcript"))?;
    let _ = text.len(); // content is proven to exist, then dropped

    let mut contract = ModelModalityContract::new(whisper_provider(), LOCAL_WHISPER_STT_MODEL);
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
    stateless_http_facts(&mut contract);
    say_no_streaming(
        &mut contract,
        "one complete transcript per request; no partial delivery",
    );
    contract.constraints.notes.insert(
        "request-shape".into(),
        "multipart/form-data (file=<16 kHz mono WAV>, response_format=json)".into(),
    );
    contract
        .constraints
        .notes
        .insert("route-path".into(), "/inference — OpenAI-shaped body; this whisper.cpp build does not serve /v1/audio/transcriptions".into());
    contract.validate()?;
    Ok(contract)
}

/// Parse a verbatim Kokoro wrapper speech request (OpenAI-shaped body) into
/// the generic TTS surface contract.
///
/// Recognised and translated: the `model` id (the wire's own name for the
/// surface), the text `input` (text in), and `response_format` (audio out —
/// this wrapper supports only `wav`, and anything else is refused, never
/// coerced). Recognised and deliberately dropped: `voice` and `speed` —
/// provider-private request configuration with no generic seam. The reply
/// is one complete WAV document, so no streaming form is declared.
pub fn parse_synthesis_capture(tts_request_json: &str) -> Result<ModelModalityContract> {
    let invalid =
        |message: &str| AikitError::new("local_speech.unparsable_tts_capture", message.to_string());
    let value: serde_json::Value = serde_json::from_str(tts_request_json)
        .map_err(|error| invalid(&format!("TTS capture is not valid JSON: {error}")))?;

    let model = value
        .get("model")
        .and_then(serde_json::Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| invalid("TTS capture carries no usable `model`"))?;
    let input = value
        .get("input")
        .and_then(serde_json::Value::as_str)
        .filter(|input| !input.trim().is_empty())
        .ok_or_else(|| invalid("TTS capture carries no usable `input` text"))?;
    let _ = input.len(); // content is proven to exist, then dropped
    let response_format = value
        .get("response_format")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            invalid("TTS capture carries no `response_format`; this adapter revision only understands `wav`")
        })?;
    if response_format != "wav" {
        return Err(invalid(&format!(
            "TTS capture declares response_format {response_format:?}, which this adapter revision does not understand (the local wrapper serves `wav` only)"
        )));
    }
    let voice_is_present = value.get("voice").is_some();
    let speed_is_present = value.get("speed").is_some();
    let _ = (voice_is_present, speed_is_present); // validated, then dropped

    let mut contract = ModelModalityContract::new(kokoro_provider(), model);
    contract.input_modalities = BTreeSet::from([ModelModality::Text]);
    contract.output_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
    contract.transforms.insert(
        TransformCapability::TextToSpeech,
        DeclaredSupport::Supported,
    );
    contract.interaction = BTreeSet::from([InteractionCapability::RequestResponse]);
    stateless_http_facts(&mut contract);
    say_no_streaming(
        &mut contract,
        "the complete WAV is returned in one response; only response_format wav is served",
    );
    contract
        .constraints
        .notes
        .insert("response-format".into(), "wav (24 kHz Int16 mono)".into());
    contract.validate()?;
    Ok(contract)
}

/// Every model surface this adapter instance declares, from the frozen
/// captures alone. This inventory is the adapter-instance half of the
/// catalogue join: a consumer asks what surfaces exist and joins them to
/// catalogue entries by (provider, provider-native surface). No network,
/// no live services — the gate passes on the frozen files.
pub fn declared_surfaces() -> Vec<ModelModalityContract> {
    vec![
        parse_transcription_capture(FROZEN_STT_RESPONSE)
            .expect("frozen STT capture must conform to this adapter's own parser"),
        parse_synthesis_capture(FROZEN_TTS_REQUEST)
            .expect("frozen TTS capture must conform to this adapter's own parser"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::composition::{
        ComponentBinding, CompositionActivationMode, CompositionState, HarnessComposition,
        RetractionMode,
    };
    use aikit_core::model_modality::{ModalityDirection, ModalitySupport};
    use aikit_core::model_runtime::{
        disclose_model_runtime, disclose_staged_model_runtime, InferenceEngineForm,
        InferenceEngineReading, ModelRuntimeRelation, ModelStageRelation, ModelSurfaceReading,
        ModelVariantReading, PlacementObservation,
    };
    use aikit_core::resource::ResourceRef;

    fn stt() -> ModelModalityContract {
        parse_transcription_capture(FROZEN_STT_RESPONSE).unwrap()
    }

    fn tts() -> ModelModalityContract {
        parse_synthesis_capture(FROZEN_TTS_REQUEST).unwrap()
    }

    #[test]
    fn the_frozen_stt_capture_resolves_to_an_honest_keyless_listening_surface() {
        let contract = stt();
        assert_eq!(
            contract.provider_native_surface,
            "whisper-large-v3-turbo-q5_0"
        );
        assert_eq!(contract.provider.as_str(), LOCAL_WHISPER_PROVIDER);
        assert!(contract.input_support(ModelModality::Speech).is_supported());
        assert!(contract.input_support(ModelModality::Audio).is_supported());
        assert!(contract.output_support(ModelModality::Text).is_supported());
        assert!(
            !contract
                .output_support(ModelModality::Speech)
                .is_supported(),
            "STT listens; it does not speak"
        );
        assert!(contract
            .transform_support(TransformCapability::SpeechToText)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::RequestResponse)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::FinalTranscripts)
            .is_supported());
        // The capture proves no partials, timestamps or streaming input, so
        // each absence reads as an explicit unsupported naming the surface.
        for unproven in [
            InteractionCapability::PartialTranscripts,
            InteractionCapability::Timestamps,
            InteractionCapability::StreamingInput,
            InteractionCapability::FullDuplexRealtime,
        ] {
            match contract.interaction_support(unproven) {
                ModalitySupport::Unsupported { reason } => {
                    assert!(
                        reason.contains("whisper-large-v3-turbo-q5_0"),
                        "the reason names the surface: {reason}"
                    );
                }
                other => panic!(
                    "the capture proves no `{}`; it must not read as {other:?}",
                    unproven.as_str()
                ),
            }
        }
        assert_eq!(contract.transport, TransportKind::Http);
        assert_eq!(contract.connection, ConnectionSemantics::Stateless);
        assert_eq!(contract.credential, CredentialCondition::NotRequired);
        assert!(!contract.is_speech_capable());

        // The route delta is a declared fact in the open: the /inference
        // override travels in the constraint notes; the OpenAI path is not
        // claimed anywhere.
        let route_note = contract.constraints.notes.get("route-path").unwrap();
        assert!(route_note.contains("/inference"));
        assert!(route_note.contains("/v1/audio/transcriptions"));
        let rendered = serde_json::to_string(&contract).unwrap();
        assert!(
            rendered.contains("/inference"),
            "the override is not hidden"
        );
    }

    #[test]
    fn the_frozen_tts_capture_resolves_and_provider_private_facts_are_dropped() {
        let contract = tts();
        assert_eq!(contract.provider_native_surface, "kokoro-82m");
        assert_eq!(contract.provider.as_str(), LOCAL_KOKORO_PROVIDER);
        assert!(contract.input_support(ModelModality::Text).is_supported());
        assert!(contract.output_support(ModelModality::Audio).is_supported());
        assert!(contract
            .output_support(ModelModality::Speech)
            .is_supported());
        assert!(
            !contract.input_support(ModelModality::Speech).is_supported(),
            "TTS speaks; it does not listen"
        );
        assert!(contract
            .transform_support(TransformCapability::TextToSpeech)
            .is_supported());
        assert!(contract
            .interaction_support(InteractionCapability::RequestResponse)
            .is_supported());
        for unproven in [
            InteractionCapability::StreamingOutput,
            InteractionCapability::FullDuplexRealtime,
            InteractionCapability::PartialTranscripts,
        ] {
            match contract.interaction_support(unproven) {
                ModalitySupport::Unsupported { reason } => {
                    assert!(
                        reason.contains("kokoro-82m"),
                        "the reason names the surface: {reason}"
                    );
                }
                other => panic!(
                    "the capture proves no `{}`; it must not read as {other:?}",
                    unproven.as_str()
                ),
            }
        }
        assert_eq!(contract.credential, CredentialCondition::NotRequired);
        assert!(!contract.is_speech_capable());

        // No provider-private vocabulary survives the translation: the voice
        // name and the request's words are session configuration, not
        // generic capability facts.
        let rendered = serde_json::to_string(&contract).unwrap();
        for leaked in ["af_heart", "Nara here", "speed"] {
            assert!(
                !rendered.contains(leaked),
                "provider-specific `{leaked}` must not leak into the generic contract"
            );
        }
        assert!(contract
            .provenance
            .contains(&LOCAL_SPEECH_ADAPTER_REVISION.to_string()));
    }

    #[test]
    fn the_no_streaming_refusal_is_said_in_the_facts_not_only_by_absence() {
        for contract in [stt(), tts()] {
            match contract.interaction_support(InteractionCapability::FullDuplexRealtime) {
                ModalitySupport::Unsupported { .. } => {}
                other => panic!("full-duplex must read unsupported, got {other:?}"),
            }
            let streaming_note = contract.constraints.notes.get("streaming").unwrap();
            assert!(
                streaming_note.contains("one complete") || streaming_note.contains("one response"),
                "the why travels with the surface: {streaming_note}"
            );
        }
    }

    #[test]
    fn captures_that_do_not_match_the_wire_are_refused_never_coerced() {
        for broken in ["", "not json", "{}", r#"{"text":""}"#, r#"{"other":1}"#] {
            let error = parse_transcription_capture(broken).unwrap_err();
            assert_eq!(error.code(), "local_speech.unparsable_stt_capture");
        }
        for broken in [
            "",
            "not json",
            "{}",
            r#"{"model":"kokoro-82m"}"#,
            r#"{"model":"kokoro-82m","input":"  "}"#,
            r#"{"model":"kokoro-82m","input":"Hello"}"#,
            r#"{"model":"kokoro-82m","input":"Hello","response_format":"mp3"}"#,
        ] {
            let error = parse_synthesis_capture(broken).unwrap_err();
            assert_eq!(
                error.code(),
                "local_speech.unparsable_tts_capture",
                "capture {broken:?} must be refused with the adapter's code"
            );
        }
        let error = parse_synthesis_capture(
            r#"{"model":"kokoro-82m","input":"Hello","response_format":"mp3"}"#,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("mp3"),
            "the refusal names what it does not understand"
        );
    }

    #[test]
    fn the_roundtrip_capture_proves_the_two_local_services_compose() {
        let roundtrip: serde_json::Value = serde_json::from_str(FROZEN_ROUNDTRIP_RESPONSE).unwrap();
        let transcript = roundtrip["text"].as_str().unwrap();
        let tts_request: serde_json::Value = serde_json::from_str(FROZEN_TTS_REQUEST).unwrap();
        let spoken = tts_request["input"].as_str().unwrap();
        // The TTS output fed back through the local STT service is heard
        // again: the round-trip transcript shares the spoken sentence.
        assert!(
            transcript
                .to_lowercase()
                .contains("the local voice is online"),
            "round-trip transcript: {transcript:?}"
        );
        assert!(spoken.contains("the local voice is online"));
        // And it parses through the same STT parser: one wire, one shape.
        let contract = parse_transcription_capture(FROZEN_ROUNDTRIP_RESPONSE).unwrap();
        assert_eq!(contract.provider_native_surface, LOCAL_WHISPER_STT_MODEL);
    }

    #[test]
    fn the_declared_routes_carry_the_path_override_and_no_credential() {
        let routes = local_speech_routes();
        assert_eq!(routes.len(), 2);
        let stt_route = &routes[0];
        assert_eq!(stt_route.provider.as_str(), LOCAL_WHISPER_PROVIDER);
        assert_eq!(stt_route.kind, ModelRouteKind::LocalServing);
        assert_eq!(
            stt_route.endpoint.as_deref(),
            Some("http://127.0.0.1:8080/inference"),
            "the non-OpenAI path is the declared route, in plain sight"
        );
        assert!(!stt_route.credential.requires_credential());
        assert!(stt_route.claims(LOCAL_WHISPER_STT_MODEL));
        let tts_route = &routes[1];
        assert_eq!(
            tts_route.endpoint.as_deref(),
            Some("http://127.0.0.1:8880/v1/audio/speech")
        );
        assert!(!tts_route.credential.requires_credential());
    }

    #[test]
    fn every_declared_surface_joins_to_a_catalogued_model() {
        let catalogue = aikit_core::resource::ModelCatalogue::first_party_seed();
        let surfaces = declared_surfaces();
        assert_eq!(surfaces.len(), 2, "STT, TTS");
        for surface in &surfaces {
            let (entry, route) = catalogue
                .claiming(&surface.provider, &surface.provider_native_surface)
                .unwrap_or_else(|| {
                    panic!(
                        "catalogue must claim declared surface {}",
                        surface.provider_native_surface
                    )
                });
            assert_eq!(route.provider, surface.provider);
            assert!(entry.model.as_str().starts_with("model:"));
            assert!(!route.credential.requires_credential());
        }
    }

    // -- resolution proofs --------------------------------------------------

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn binding(component: &str) -> ComponentBinding {
        use aikit_core::composition::{
            ActivationScope, ActivationScopeKind, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
        };
        use aikit_core::scope::ScopeKind;
        ComponentBinding {
            component: r(component),
            resolution_scope: ResolutionScope::new(ScopeKind::Project, "project/voice"),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
            activation_mode: CompositionActivationMode::LiveMounted,
            implementation: None,
        }
    }

    fn composition(components: &[&str]) -> HarnessComposition {
        HarnessComposition {
            version: "aikit.harness-composition/v2".into(),
            harness: r("harness/local-voice"),
            project: Some(r("project/voice")),
            agent: None,
            agency: None,
            session: Some("agent-session-local-1".into()),
            model: None,
            component_bindings: components.iter().map(|c| binding(c)).collect(),
            contract_bindings: vec![],
            contributions: vec![],
            surfaces: vec![],
            projections: vec![],
            absences: vec![],
            state: CompositionState::Resolved,
            target_revision: Some("target-local-1".into()),
            generation: Some("generation-local-1".into()),
            fingerprint: "fp-local-1".into(),
        }
    }

    fn relation(
        model: &str,
        variant: &str,
        provider: &str,
        engine: &str,
        endpoint: Option<&str>,
        modality: Option<ModelModalityContract>,
    ) -> ModelRuntimeRelation {
        let mut materialisation_provider_native = std::collections::BTreeMap::new();
        if let Some(endpoint) = endpoint {
            materialisation_provider_native.insert("endpoint".to_string(), endpoint.to_string());
        }
        ModelRuntimeRelation {
            model: ModelVariantReading {
                model: r(model),
                variant: variant.into(),
            },
            engine: InferenceEngineReading {
                engine: r(engine),
                provider: ProviderRef::parse(provider).unwrap(),
                form: InferenceEngineForm::LightweightServer,
                revision: Some(LOCAL_SPEECH_ADAPTER_REVISION.into()),
                provider_native: std::collections::BTreeMap::new(),
            },
            materialisation: aikit_core::model_runtime::ModelMaterialisationReading {
                binding_ref: "binding/local-1".into(),
                workcell_ref: None,
                placement: PlacementObservation::Local,
                endpoint: endpoint.map(str::to_string),
                provider_native: materialisation_provider_native,
                resources: Default::default(),
                lifetime_owner: "agent-session".into(),
                retraction: RetractionMode::Live,
            },
            model_surface: ModelSurfaceReading {
                contract: None,
                protocol: format!("{provider}-http"),
                capabilities: Default::default(),
                access: aikit_core::model_runtime::ModelAccessReading {
                    inference: aikit_core::model_runtime::AccessFieldReading::available(["invoke"]),
                    material_control: aikit_core::model_runtime::AccessFieldReading::unavailable(
                        "the service owns its own process lifecycle",
                    ),
                    interior: aikit_core::model_runtime::AccessFieldReading::unavailable(
                        "no model-interior seam",
                    ),
                },
                modality,
            },
            change_application: aikit_core::model_runtime::RuntimeChangeApplication::Restart,
        }
    }

    /// A plain local text harness stage (the machine's local Ollama text
    /// model): it declares a contract too, so the cascade body is complete
    /// and body-level request/response rests on every stage.
    fn text_stage_contract() -> ModelModalityContract {
        let mut contract = ModelModalityContract::new(
            ProviderRef::parse("provider:ollama").unwrap(),
            "llama3.2:latest",
        );
        contract.input_modalities = BTreeSet::from([ModelModality::Text]);
        contract.output_modalities = BTreeSet::from([ModelModality::Text]);
        contract.interaction = BTreeSet::from([InteractionCapability::RequestResponse]);
        contract.transport = TransportKind::Http;
        contract.connection = ConnectionSemantics::Stateless;
        contract.credential = CredentialCondition::NotRequired;
        contract
            .provenance
            .push("fixture:test/local-text-stage".into());
        contract
    }

    #[test]
    fn each_local_surface_resolves_as_a_single_body_with_honest_half_capability() {
        for (model, provider, engine, endpoint, modality) in [
            (
                "model:local-whisper-large-v3-turbo",
                LOCAL_WHISPER_PROVIDER,
                "engine/whisper-server",
                LOCAL_WHISPER_INFERENCE_ENDPOINT,
                stt(),
            ),
            (
                "model:kokoro-82m",
                LOCAL_KOKORO_PROVIDER,
                "engine/kokoro-onnx",
                LOCAL_KOKORO_SPEECH_ENDPOINT,
                tts(),
            ),
        ] {
            let composition = composition(&["component/voice"]);
            let read = disclose_model_runtime(
                &composition,
                relation(
                    model,
                    modality.provider_native_surface.as_str(),
                    provider,
                    engine,
                    Some(endpoint),
                    Some(modality.clone()),
                ),
            )
            .unwrap();
            // Neither half is speech-capable alone: STT listens, TTS speaks.
            assert_eq!(read.speech_capable(), Some(false));
            // Nothing keyless gates these surfaces: no credential or
            // availability entry. The disclosed gaps are exactly the honest
            // service-owned ones (lifecycle, interior, thin-target contract).
            for absence in &read.unavailable {
                assert!(
                    !absence.field.contains("modality"),
                    "a keyless local surface must carry no modality gating: {absence:?}"
                );
            }
            for expected in [
                "material-control-access",
                "model-interior-access",
                "inference-contract",
            ] {
                assert!(
                    read.unavailable
                        .iter()
                        .any(|absence| absence.field == expected),
                    "the honest {expected} gap must be disclosed"
                );
            }
            assert_eq!(
                read.relation.materialisation.placement,
                PlacementObservation::Local
            );
        }
    }

    #[test]
    fn the_fully_local_cascade_resolves_speech_capable_with_per_stage_relations() {
        let composition = composition(&["component/stt", "component/text", "component/tts"]);
        let stt_relation = relation(
            "model:local-whisper-large-v3-turbo",
            LOCAL_WHISPER_STT_MODEL,
            LOCAL_WHISPER_PROVIDER,
            "engine/whisper-server",
            Some(LOCAL_WHISPER_INFERENCE_ENDPOINT),
            Some(stt()),
        );
        let text_relation = relation(
            "model:llama3.2",
            "llama3.2:latest",
            "provider:ollama",
            "engine/ollama",
            Some("http://127.0.0.1:11434"),
            Some(text_stage_contract()),
        );
        let tts_relation = relation(
            "model:kokoro-82m",
            "kokoro-82m",
            LOCAL_KOKORO_PROVIDER,
            "engine/kokoro-onnx",
            Some(LOCAL_KOKORO_SPEECH_ENDPOINT),
            Some(tts()),
        );
        let read = disclose_staged_model_runtime(
            &composition,
            vec![
                ModelStageRelation {
                    component: r("component/stt"),
                    relation: stt_relation,
                },
                ModelStageRelation {
                    component: r("component/text"),
                    relation: text_relation,
                },
                ModelStageRelation {
                    component: r("component/tts"),
                    relation: tts_relation,
                },
            ],
        )
        .unwrap();

        // Speech enters and leaves the body, through three local stages.
        assert!(read.speech_capable());
        assert!(read.composed_modality.complete);
        // Nothing keyless gates any stage; the disclosed gaps are the
        // service-owned access facts, never a credential or availability.
        assert!(read
            .unavailable
            .iter()
            .all(|absence| !absence.field.contains("modality")));
        assert_eq!(read.stages.len(), 3);
        assert_eq!(
            read.stage(&r("component/stt"))
                .unwrap()
                .relation
                .model
                .model,
            r("model:local-whisper-large-v3-turbo")
        );
        assert_eq!(
            read.stage(&r("component/tts"))
                .unwrap()
                .relation
                .model
                .model,
            r("model:kokoro-82m")
        );

        // Pipeline modalities: speech in at the first stage, speech out at
        // the last; text is interior and must not leak to the body edges.
        assert!(read
            .composed_modality
            .modality_support(
                aikit_core::model_modality::ModalityDirection::Input,
                ModelModality::Speech
            )
            .is_supported());
        assert!(read
            .composed_modality
            .modality_support(
                aikit_core::model_modality::ModalityDirection::Output,
                ModelModality::Speech
            )
            .is_supported());

        // Request/response rests on every stage. Full-duplex realtime is
        // explicitly refused at the body level — no stage of this body
        // declares it, because these servers do not stream — and each
        // stage's own answer names that stage's surface.
        assert!(read
            .interaction_support(InteractionCapability::RequestResponse)
            .is_supported());
        match read.interaction_support(InteractionCapability::FullDuplexRealtime) {
            ModalitySupport::Unsupported { reason } => {
                assert!(
                    reason.contains("no stage of this body declares"),
                    "the body-level refusal is explicit: {reason}"
                );
            }
            other => panic!("full-duplex must read unsupported, got {other:?}"),
        }
        for (component, surface) in [
            ("component/stt", "whisper-large-v3-turbo-q5_0"),
            ("component/tts", "kokoro-82m"),
        ] {
            let stage = read.stage(&r(component)).unwrap();
            match stage
                .relation
                .model_surface
                .modality
                .as_ref()
                .unwrap()
                .interaction_support(InteractionCapability::FullDuplexRealtime)
            {
                ModalitySupport::Unsupported { reason } => {
                    assert!(
                        reason.contains(surface),
                        "the stage refusal names its surface: {reason}"
                    );
                }
                other => panic!("stage {component} full-duplex must be unsupported, got {other:?}"),
            }
        }
        match read.interaction_support(InteractionCapability::StreamingOutput) {
            ModalitySupport::Unsupported { .. } => {}
            other => panic!("streaming-output must read unsupported, got {other:?}"),
        }

        // Body-level transforms obey the strict pipeline law: the body does
        // speech-to-speech-by-way-of-text, not STT or TTS alone, so the
        // single-stage transforms stay per-stage facts.
        match read
            .composed_modality
            .transform_support(TransformCapability::SpeechToText)
        {
            ModalitySupport::Unsupported { reason } => {
                assert!(reason.contains("withheld"), "{reason}");
            }
            other => panic!("body-level STT must not be claimed, got {other:?}"),
        }
        let stt_stage = &read.stage(&r("component/stt")).unwrap().relation;
        assert!(stt_stage
            .model_surface
            .modality
            .as_ref()
            .unwrap()
            .transform_support(TransformCapability::SpeechToText)
            .is_supported());
        let tts_stage = &read.stage(&r("component/tts")).unwrap().relation;
        assert!(tts_stage
            .model_surface
            .modality
            .as_ref()
            .unwrap()
            .transform_support(TransformCapability::TextToSpeech)
            .is_supported());

        // The basis names the local stages, and every materialisation is
        // local: nothing left the machine.
        assert!(read.composed_modality.basis.iter().any(|line| {
            line.contains("component/stt") && line.contains("whisper-large-v3-turbo-q5_0")
        }));
        assert!(read
            .composed_modality
            .basis
            .iter()
            .any(|line| { line.contains("component/tts") && line.contains("kokoro-82m") }));
        for stage in &read.stages {
            assert_eq!(
                stage.relation.materialisation.placement,
                PlacementObservation::Local,
                "stage {} is local",
                stage.component
            );
        }
    }

    // -- the optional live check -------------------------------------------

    /// The declared endpoints are adapter data; the live check derives its
    /// origins from them and adds no URL of its own.
    fn origin_of(endpoint: &str) -> &str {
        endpoint
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .expect("declared local endpoints are http URLs with an authority")
    }

    /// One raw HTTP/1.0 GET; no HTTP client dependency exists in this crate
    /// and the check adds none.
    fn http_get_status(origin: &str, path: &str) -> std::io::Result<u16> {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;
        let mut stream = TcpStream::connect(origin)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let request = format!("GET {path} HTTP/1.0\r\nHost: {origin}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes())?;
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer)?;
        let head = String::from_utf8_lossy(&buffer);
        head.split_whitespace()
            .nth(1)
            .and_then(|status| status.parse::<u16>().ok())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "no HTTP status line in response",
                )
            })
    }

    /// The clearly-skippable live check. The gate runs on the frozen
    /// captures alone; this test observes the real services only when the
    /// operator asks — `AIKIT_LOCAL_SPEECH_LIVE=1` with the stack up (see
    /// `/Users/admin/.local-speech/README.md`). Any other state prints its
    /// skip honestly and passes. Observation lives here and nowhere else:
    /// the declared facts never claim a live route.
    #[test]
    fn when_the_operator_asks_the_live_services_answer_their_declared_origins() {
        if std::env::var("AIKIT_LOCAL_SPEECH_LIVE").as_deref() != Ok("1") {
            eprintln!(
                "skip: AIKIT_LOCAL_SPEECH_LIVE is not 1; the frozen captures are the test truth"
            );
            return;
        }
        for (endpoint, path, what) in [
            (
                LOCAL_WHISPER_INFERENCE_ENDPOINT,
                "/",
                "whisper-server web UI",
            ),
            (
                LOCAL_KOKORO_SPEECH_ENDPOINT,
                "/health",
                "kokoro wrapper health",
            ),
        ] {
            let origin = origin_of(endpoint);
            match http_get_status(origin, path) {
                Ok(status) => assert_eq!(
                    status, 200,
                    "{what} at {origin}{path} answered {status}"
                ),
                Err(error) => panic!(
                    "{what} at {origin}{path} did not answer (is the local speech stack up?): {error}"
                ),
            }
        }
    }
}
