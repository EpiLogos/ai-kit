//! CLI document-in/document-out disclosure of a resolved body's modality,
//! interaction and transport facts.
//!
//! The read models this command consumes are produced by
//! `aikit-core::model_runtime` (`disclose_model_runtime` for a single-model
//! body, `disclose_staged_model_runtime` for a staged cascade). Both are
//! schema-stamped, serialisable documents; this module turns one of them
//! into the disclosure document a machine user asks for: what resolved,
//! the four-state answer for **every** vocabulary member (so absence is
//! visible, never flattened away), and the explanation evidence behind the
//! answers. No network, no resolution re-run, no provider calls — the
//! document in is the resolved truth, the document out reads it back.

use std::collections::BTreeMap;

use aikit_core::model_modality::{
    InteractionCapability, ModalityDirection, ModalitySupport, ModelModality, TransformCapability,
};
use aikit_core::model_runtime::{ModelRuntimeReadModel, StagedModelRuntimeReadModel};
use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

pub const MODEL_MODALITY_DISCLOSURE_VERSION: &str = "aikit.model-modality-disclosure/v1";

const MODEL_RUNTIME_VERSION: &str = "aikit.model-runtime/v1";
const MODEL_STAGE_RUNTIME_VERSION: &str = "aikit.model-stage-runtime/v1";

/// Disclose a resolved read-model document. The input is one of:
///
/// * a `aikit.model-runtime/v1` [`ModelRuntimeReadModel`] (single-model
///   body), or
/// * a `aikit.model-stage-runtime/v1` [`StagedModelRuntimeReadModel`]
///   (staged cascade body).
///
/// Anything else is refused with a stable error naming the two documents
/// this disclosure reads — the file is never guessed at.
pub fn disclose_document(raw: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(raw).map_err(|error| {
        AikitError::new(
            "model_modality_disclosure.unparsable_document",
            format!("disclosure input is not valid JSON: {error}"),
        )
    })?;
    match value.get("version").and_then(Value::as_str) {
        Some(MODEL_RUNTIME_VERSION) => {
            let read: ModelRuntimeReadModel = serde_json::from_value(value).map_err(|error| {
                AikitError::new(
                    "model_modality_disclosure.unparsable_document",
                    format!(
                        "document names {MODEL_RUNTIME_VERSION} but does not load as one: {error}"
                    ),
                )
            })?;
            disclose_single(&read)
        }
        Some(MODEL_STAGE_RUNTIME_VERSION) => {
            let read: StagedModelRuntimeReadModel =
                serde_json::from_value(value).map_err(|error| {
                    AikitError::new(
                        "model_modality_disclosure.unparsable_document",
                        format!("document names {MODEL_STAGE_RUNTIME_VERSION} but does not load as one: {error}"),
                    )
                })?;
            disclose_staged(&read)
        }
        other => Err(AikitError::new(
            "model_modality_disclosure.unknown_document",
            format!(
                "document carries version {other:?}; this disclosure reads a \
                 {MODEL_RUNTIME_VERSION} or {MODEL_STAGE_RUNTIME_VERSION} read model"
            ),
        )),
    }
}

/// The full four-state answer for every vocabulary member. Rendering the
/// whole matrix is the point: a capability the body does not carry appears
/// as an explicit unsupported (or unknown) answer with its reason, not as
/// a missing key.
fn answers_for(
    interaction: impl Fn(InteractionCapability) -> ModalitySupport,
    transform: impl Fn(TransformCapability) -> ModalitySupport,
    modality: impl Fn(ModalityDirection, ModelModality) -> ModalitySupport,
) -> Value {
    let interaction_map: BTreeMap<String, ModalitySupport> = [
        InteractionCapability::RequestResponse,
        InteractionCapability::StreamingInput,
        InteractionCapability::StreamingOutput,
        InteractionCapability::FullDuplexRealtime,
        InteractionCapability::StructuredEvents,
        InteractionCapability::ToolRequests,
        InteractionCapability::Timestamps,
        InteractionCapability::PartialTranscripts,
        InteractionCapability::FinalTranscripts,
        InteractionCapability::VadTurnDetection,
        InteractionCapability::BargeIn,
    ]
    .into_iter()
    .map(|capability| (capability.as_str().to_string(), interaction(capability)))
    .collect();
    let transform_map: BTreeMap<String, ModalitySupport> = [
        TransformCapability::SpeechToText,
        TransformCapability::TextToSpeech,
        TransformCapability::SpeechToSpeech,
        TransformCapability::AudioUnderstanding,
        TransformCapability::MultimodalTextAudio,
    ]
    .into_iter()
    .map(|capability| (capability.as_str().to_string(), transform(capability)))
    .collect();
    let modality_map = |direction: ModalityDirection| -> Value {
        let entries: BTreeMap<String, ModalitySupport> = [
            ModelModality::Text,
            ModelModality::Audio,
            ModelModality::Speech,
        ]
        .into_iter()
        .map(|m| (m.as_str().to_string(), modality(direction, m)))
        .collect();
        json!(entries)
    };
    json!({
        "input_modalities": modality_map(ModalityDirection::Input),
        "output_modalities": modality_map(ModalityDirection::Output),
        "transforms": json!(transform_map),
        "interaction": json!(interaction_map),
    })
}

fn identity_fields(
    harness: &aikit_core::resource::ResourceRef,
    project: &Option<aikit_core::resource::ResourceRef>,
    agent: &Option<aikit_core::resource::ResourceRef>,
    agency: &Option<aikit_core::resource::ResourceRef>,
    session: &Option<String>,
    fingerprint: &str,
) -> Value {
    json!({
        "harness": harness.to_string(),
        "project": project.as_ref().map(|r| r.to_string()),
        "agent": agent.as_ref().map(|r| r.to_string()),
        "agency": agency.as_ref().map(|r| r.to_string()),
        "agent_session": session,
        "harness_composition_fingerprint": fingerprint,
    })
}

fn disclose_single(read: &ModelRuntimeReadModel) -> Result<Value> {
    let modality = read.modality();
    let answers = answers_for(
        |capability| read.interaction_support(capability),
        |capability| read.transform_support(capability),
        |direction, m| read.modality_support(direction, m),
    );
    Ok(json!({
        "schema": MODEL_MODALITY_DISCLOSURE_VERSION,
        "body_kind": "single",
        "identity": identity_fields(
            &read.harness,
            &read.project,
            &read.agent,
            &read.agency,
            &read.agent_session,
            &read.harness_composition_fingerprint,
        ),
        "model": {
            "model": read.relation.model.model.to_string(),
            "variant": read.relation.model.variant,
        },
        "speech_capable": read.speech_capable(),
        "surface": modality.map(surface_summary),
        "answers": answers,
        "unavailable": read.unavailable,
        "explanation": aikit_core::model_modality::explain_model_modality(read),
    }))
}

fn surface_summary(contract: &aikit_core::model_modality::ModelModalityContract) -> Value {
    json!({
        "input_modalities": contract.input_modalities.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
        "output_modalities": contract.output_modalities.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
        "transforms_declared": contract.transforms.keys().map(|t| t.as_str()).collect::<Vec<_>>(),
        "interaction_declared": contract.interaction.iter().map(|i| i.as_str()).collect::<Vec<_>>(),
        "interaction_degraded": contract.degraded_interaction,
        "transport": contract.transport.as_str(),
        "connection": contract.connection,
        "credential_scope": contract.credential_scope,
        "availability": contract.availability,
        "provider": contract.provider.to_string(),
        "provider_native_surface": contract.provider_native_surface,
        "provider_revision": contract.provider_revision,
        "credential": contract.credential,
        "constraints": contract.constraints,
        "provenance": contract.provenance,
    })
}

fn disclose_staged(read: &StagedModelRuntimeReadModel) -> Result<Value> {
    let stages: Vec<Value> = read
        .stages
        .iter()
        .map(|stage| {
            json!({
                "component": stage.component.to_string(),
                "model": {
                    "model": stage.relation.model.model.to_string(),
                    "variant": stage.relation.model.variant,
                },
                "engine": {
                    "engine": stage.relation.engine.engine.to_string(),
                    "provider": stage.relation.engine.provider.to_string(),
                    "form": stage.relation.engine.form,
                },
                "surface": stage
                    .relation
                    .model_surface
                    .modality
                    .as_ref()
                    .map(surface_summary),
                "protocol": stage.relation.model_surface.protocol,
                "change_application": stage.relation.change_application,
            })
        })
        .collect();
    let composed = &read.composed_modality;
    let answers = answers_for(
        |capability| composed.interaction_support(capability),
        |capability| composed.transform_support(capability),
        |direction, m| composed.modality_support(direction, m),
    );
    Ok(json!({
        "schema": MODEL_MODALITY_DISCLOSURE_VERSION,
        "body_kind": "staged",
        "identity": identity_fields(
            &read.harness,
            &read.project,
            &read.agent,
            &read.agency,
            &read.agent_session,
            &read.harness_composition_fingerprint,
        ),
        "speech_capable": read.speech_capable(),
        "complete": composed.complete,
        "stages": stages,
        "answers": answers,
        "basis": composed.basis,
        "unavailable": read.unavailable,
        "explanation": aikit_core::model_modality::explain_staged_model_runtime(read),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::composition::{
        ActivationScope, ActivationScopeKind, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
    };
    use aikit_core::composition::{
        ComponentBinding, CompositionActivationMode, CompositionState, HarnessComposition,
    };
    use aikit_core::model_modality::{
        ConnectionSemantics, CredentialScope, DeclaredSupport, ReconnectSupport,
        SurfaceAvailability, TransportKind,
    };
    use aikit_core::model_runtime::{
        AccessFieldReading, InferenceEngineForm, InferenceEngineReading, ModelAccessReading,
        ModelMaterialisationReading, ModelRuntimeRelation, ModelStageRelation, ModelSurfaceReading,
        ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
    };
    use aikit_core::resource::{CredentialCondition, ProviderRef, ResourceRef};
    use aikit_core::scope::ScopeKind;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn binding(component: &str) -> ComponentBinding {
        ComponentBinding {
            component: r(component),
            resolution_scope: ResolutionScope::new(ScopeKind::Project, "project/voice"),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
            activation_mode: CompositionActivationMode::LiveMounted,
            implementation: None,
        }
    }

    fn composition(model: Option<ResourceRef>, components: &[&str]) -> HarnessComposition {
        HarnessComposition {
            version: "aikit.harness-composition/v2".into(),
            harness: r("harness/voice"),
            project: Some(r("project/voice")),
            agent: None,
            agency: None,
            session: Some("agent-session-1".into()),
            model,
            component_bindings: components.iter().map(|c| binding(c)).collect(),
            contract_bindings: vec![],
            contributions: vec![],
            surfaces: vec![],
            projections: vec![],
            absences: vec![],
            state: CompositionState::Resolved,
            target_revision: Some("target-1".into()),
            generation: Some("generation-1".into()),
            fingerprint: "fp-1".into(),
        }
    }

    fn relation(
        model: &str,
        variant: &str,
        provider: &str,
        modality: Option<aikit_core::model_modality::ModelModalityContract>,
    ) -> ModelRuntimeRelation {
        ModelRuntimeRelation {
            model: ModelVariantReading {
                model: r(model),
                variant: variant.into(),
            },
            engine: InferenceEngineReading {
                engine: r(&format!("engine/{variant}")),
                provider: ProviderRef::parse(provider).unwrap(),
                form: InferenceEngineForm::ManagedService,
                revision: None,
                provider_native: BTreeMap::new(),
            },
            materialisation: ModelMaterialisationReading {
                binding_ref: "binding/1".into(),
                workcell_ref: None,
                placement: PlacementObservation::Remote,
                endpoint: None,
                provider_native: BTreeMap::new(),
                resources: Default::default(),
                lifetime_owner: "agent-session".into(),
                retraction: aikit_core::composition::RetractionMode::Live,
            },
            model_surface: ModelSurfaceReading {
                contract: None,
                protocol: "fixture-protocol".into(),
                capabilities: Default::default(),
                access: ModelAccessReading {
                    inference: AccessFieldReading::available(["invoke"]),
                    material_control: AccessFieldReading::unavailable("provider owns lifecycle"),
                    interior: AccessFieldReading::unavailable("no model-interior seam"),
                },
                modality,
            },
            change_application: RuntimeChangeApplication::Live,
        }
    }

    fn realtime_contract() -> aikit_core::model_modality::ModelModalityContract {
        let mut contract = aikit_core::model_modality::ModelModalityContract::new(
            ProviderRef::parse("provider:openai").unwrap(),
            "gpt-realtime",
        );
        use aikit_core::model_modality::{InteractionCapability, ModelModality};
        contract.input_modalities = [
            ModelModality::Audio,
            ModelModality::Speech,
            ModelModality::Text,
        ]
        .into_iter()
        .collect();
        contract.output_modalities = contract.input_modalities.clone();
        contract.transforms.insert(
            TransformCapability::SpeechToSpeech,
            DeclaredSupport::Supported,
        );
        contract.interaction = [
            InteractionCapability::StreamingInput,
            InteractionCapability::StreamingOutput,
            InteractionCapability::FullDuplexRealtime,
            InteractionCapability::VadTurnDetection,
            InteractionCapability::BargeIn,
        ]
        .into_iter()
        .collect();
        contract.transport = TransportKind::WebSocket;
        contract.connection = ConnectionSemantics::Connected {
            reconnect: ReconnectSupport::ReconnectWithoutSession,
        };
        contract.credential_scope = CredentialScope::EphemeralSurfaceToken;
        contract.availability = SurfaceAvailability::Available;
        contract.credential = CredentialCondition::Satisfied {
            hint: "openai realtime credential".into(),
            binding_ref: "credential-binding/openai-1".into(),
        };
        contract
    }

    #[test]
    fn a_single_body_discloses_the_full_four_state_matrix() {
        let composition = composition(Some(r("model:gpt-realtime")), &["component/voice"]);
        let read = aikit_core::model_runtime::disclose_model_runtime(
            &composition,
            relation(
                "model:gpt-realtime",
                "gpt-realtime",
                "provider:openai",
                Some(realtime_contract()),
            ),
        )
        .unwrap();
        let document = serde_json::to_string(&read).unwrap();
        let disclosure = disclose_document(&document).unwrap();

        assert_eq!(disclosure["schema"], MODEL_MODALITY_DISCLOSURE_VERSION);
        assert_eq!(disclosure["body_kind"], "single");
        assert_eq!(disclosure["speech_capable"], true);
        assert_eq!(
            disclosure["answers"]["interaction"]["barge-in"]["state"],
            "supported"
        );
        // The honest absences are rendered answers, not missing keys.
        assert_eq!(
            disclosure["answers"]["interaction"]["partial-transcripts"]["state"],
            "unsupported"
        );
        assert!(
            disclosure["answers"]["interaction"]["partial-transcripts"]["reason"]
                .as_str()
                .unwrap()
                .contains("gpt-realtime")
        );
        assert_eq!(
            disclosure["answers"]["interaction"]["timestamps"]["state"],
            "unsupported"
        );
        assert_eq!(
            disclosure["answers"]["transforms"]["speech-to-speech"]["state"],
            "supported"
        );
        assert_eq!(
            disclosure["answers"]["input_modalities"]["speech"]["state"],
            "supported"
        );
        assert_eq!(
            disclosure["answers"]["output_modalities"]["text"]["state"],
            "supported"
        );
        assert_eq!(disclosure["surface"]["transport"], "websocket");
        // The explanation evidence travels in the same document.
        assert!(disclosure["explanation"]["facts"].as_array().unwrap().len() > 1);
    }

    #[test]
    fn a_contractless_body_answers_unknown_everywhere_and_says_why() {
        let composition = composition(Some(r("model:llama3.2")), &["component/text"]);
        let read = aikit_core::model_runtime::disclose_model_runtime(
            &composition,
            relation("model:llama3.2", "llama3.2:latest", "provider:ollama", None),
        )
        .unwrap();
        let document = serde_json::to_string(&read).unwrap();
        let disclosure = disclose_document(&document).unwrap();

        assert_eq!(disclosure["speech_capable"], Value::Null);
        assert_eq!(disclosure["surface"], Value::Null);
        assert_eq!(
            disclosure["answers"]["interaction"]["barge-in"]["state"],
            "unknown"
        );
        assert_eq!(
            disclosure["answers"]["transforms"]["speech-to-text"]["state"],
            "unknown"
        );
        assert_eq!(
            disclosure["answers"]["input_modalities"]["speech"]["state"],
            "unknown"
        );
        assert!(disclosure["explanation"]["facts"][0]["summary"]
            .as_str()
            .unwrap()
            .contains("no modality contract"));
    }

    #[test]
    fn a_staged_cascade_discloses_per_stage_facts_and_the_strict_body_view() {
        let mut stt = aikit_core::model_modality::ModelModalityContract::new(
            ProviderRef::parse("provider:openai").unwrap(),
            "gpt-4o-transcribe",
        );
        use aikit_core::model_modality::ModelModality;
        stt.input_modalities = [ModelModality::Audio, ModelModality::Speech]
            .into_iter()
            .collect();
        stt.output_modalities = [ModelModality::Text].into_iter().collect();
        stt.transforms.insert(
            TransformCapability::SpeechToText,
            DeclaredSupport::Supported,
        );
        stt.interaction = [
            InteractionCapability::RequestResponse,
            InteractionCapability::FinalTranscripts,
        ]
        .into_iter()
        .collect();
        let mut tts = aikit_core::model_modality::ModelModalityContract::new(
            ProviderRef::parse("provider:openai").unwrap(),
            "gpt-4o-mini-tts",
        );
        tts.input_modalities = [ModelModality::Text].into_iter().collect();
        tts.output_modalities = [ModelModality::Audio, ModelModality::Speech]
            .into_iter()
            .collect();
        tts.transforms.insert(
            TransformCapability::TextToSpeech,
            DeclaredSupport::Supported,
        );
        tts.interaction = [
            InteractionCapability::RequestResponse,
            InteractionCapability::StreamingOutput,
        ]
        .into_iter()
        .collect();

        let composition = composition(
            None,
            &["component/stt", "component/text-harness", "component/tts"],
        );
        let read = aikit_core::model_runtime::disclose_staged_model_runtime(
            &composition,
            vec![
                ModelStageRelation {
                    component: r("component/stt"),
                    relation: relation(
                        "model:gpt-4o-transcribe",
                        "gpt-4o-transcribe",
                        "provider:openai",
                        Some(stt),
                    ),
                },
                // A plain text harness stage declares no modality contract.
                ModelStageRelation {
                    component: r("component/text-harness"),
                    relation: relation(
                        "model:llama3.2",
                        "llama3.2:latest",
                        "provider:ollama",
                        None,
                    ),
                },
                ModelStageRelation {
                    component: r("component/tts"),
                    relation: relation(
                        "model:gpt-4o-mini-tts",
                        "gpt-4o-mini-tts",
                        "provider:openai",
                        Some(tts),
                    ),
                },
            ],
        )
        .unwrap();
        let document = serde_json::to_string(&read).unwrap();
        let disclosure = disclose_document(&document).unwrap();

        assert_eq!(disclosure["body_kind"], "staged");
        assert_eq!(disclosure["complete"], false);
        assert_eq!(disclosure["speech_capable"], true);
        assert_eq!(disclosure["stages"].as_array().unwrap().len(), 3);
        assert_eq!(
            disclosure["stages"][0]["surface"]["provider_native_surface"],
            "gpt-4o-transcribe"
        );
        assert_eq!(disclosure["stages"][1]["surface"], Value::Null);

        // Body-level answers stay strict: the opaque text stage leaves the
        // shared request-response claim unproven, and streaming-output is
        // explicitly withheld by the STT stage.
        assert_eq!(
            disclosure["answers"]["interaction"]["request-response"]["state"],
            "unknown"
        );
        assert_eq!(
            disclosure["answers"]["interaction"]["streaming-output"]["state"],
            "unsupported"
        );
        assert!(
            disclosure["answers"]["interaction"]["streaming-output"]["reason"]
                .as_str()
                .unwrap()
                .contains("component/stt")
        );
        // Speech enters and leaves the body; text is interior.
        assert_eq!(
            disclosure["answers"]["input_modalities"]["speech"]["state"],
            "supported"
        );
        assert_eq!(
            disclosure["answers"]["output_modalities"]["text"]["state"],
            "unknown"
        );
        // The stage-named basis and the explanation evidence are in the document.
        assert!(disclosure["basis"].as_array().unwrap().len() >= 3);
        assert!(disclosure["explanation"]["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact["relation"] == "stage-model-relation"));
    }

    #[test]
    fn an_unrelated_document_is_refused_with_a_stable_code() {
        let error = disclose_document("{\"version\":\"aikit.some-other/v1\"}").unwrap_err();
        assert_eq!(error.code(), "model_modality_disclosure.unknown_document");
        let error = disclose_document("not json").unwrap_err();
        assert_eq!(
            error.code(),
            "model_modality_disclosure.unparsable_document"
        );
        // A document with the right version name but wrong shape is refused too.
        let error = disclose_document("{\"version\":\"aikit.model-runtime/v1\"}").unwrap_err();
        assert_eq!(
            error.code(),
            "model_modality_disclosure.unparsable_document"
        );
    }
}
