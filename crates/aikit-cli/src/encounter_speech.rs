//! Explicit local speech stages around an admitted native text encounter.
//! Configuration names machine material; live probes never pretend to prove
//! an inference or the microphone. This module does not replace the text body.
use super::{native_admission, read_binding};
use crate::encounter_service::{request, socket_path, EncounterRequest};
use aikit_core::composition::*;
use aikit_core::model_modality::*;
use aikit_core::model_runtime::*;
use aikit_core::resource::{canonical_model_ref, ProviderRef};
use aikit_core::scope::ScopeKind;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Read,
    net::IpAddr,
    process::{Command, Stdio},
};

const SCHEMA: &str = "aikit.local-speech-config/v1";
fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("encounter.local_speech", message.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSpeechStage {
    pub host: IpAddr,
    pub port: u16,
    pub path: String,
    pub health_path: String,
    pub provider_ref: ProviderRef,
    pub model_ref: ResourceRef,
    pub model_id: String,
    pub engine_ref: ResourceRef,
    pub engine_revision: String,
    pub source_ref: String,
    /// A declared local TTS voice, absent for transcription.
    pub voice: Option<String>,
}
impl LocalSpeechStage {
    fn validate(&self) -> Result<()> {
        if !self.host.is_loopback() || self.port == 0 {
            return Err(error(
                "Speech stages require an explicit loopback address and nonzero port",
            ));
        }
        for path in [&self.path, &self.health_path] {
            if !path.starts_with('/')
                || path.len() > 256
                || !path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
            {
                return Err(error("Speech paths must be bounded local HTTP paths without credentials, query or redirection"));
            }
        }
        for value in [&self.model_id, &self.engine_revision, &self.source_ref] {
            if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            {
                return Err(error(
                    "Speech material requires bounded model, revision and source provenance",
                ));
            }
        }
        Ok(())
    }
    pub fn endpoint(&self, health: bool) -> String {
        let host = match self.host {
            IpAddr::V4(v) => v.to_string(),
            IpAddr::V6(v) => format!("[{v}]"),
        };
        format!(
            "http://{host}:{}{}",
            self.port,
            if health {
                &self.health_path
            } else {
                &self.path
            }
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSpeechConfig {
    pub schema: String,
    pub stt: LocalSpeechStage,
    pub tts: LocalSpeechStage,
}
impl LocalSpeechConfig {
    fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return Err(error(format!("Expected {SCHEMA}")));
        }
        self.stt.validate()?;
        self.tts.validate()?;
        if self.stt.voice.is_some()
            || self.tts.voice.as_ref().is_none_or(|v| {
                v.is_empty()
                    || v.len() > 80
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(error("Transcription has no voice; synthesis requires an explicit bounded voice identifier"));
        }
        Ok(())
    }
}

pub fn configure(home: &AikitHome, config: LocalSpeechConfig) -> Result<Value> {
    config.validate()?;
    home.ensure_layout()?;
    let bytes = serde_json::to_vec_pretty(&config).map_err(error)?;
    let path = home.state().join("local-speech.json");
    let mut file = tempfile::NamedTempFile::new_in(home.state()).map_err(error)?;
    use std::io::Write;
    file.write_all(&bytes).map_err(error)?;
    file.as_file().sync_all().map_err(error)?;
    file.persist(&path).map_err(error)?;
    Ok(
        json!({"configured":true,"source":path,"revision":blake3::hash(&bytes).to_hex().to_string(),"standing":"configured-not-probed"}),
    )
}

pub fn read_config(home: &AikitHome) -> Result<(LocalSpeechConfig, String)> {
    let path = home.state().join("local-speech.json");
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| {
        error(format!(
            "Explicit local speech configuration unavailable: {e}"
        ))
    })?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(error(
            "Local speech configuration must be a bounded owner file",
        ));
    }
    let bytes = std::fs::read(path).map_err(error)?;
    let config: LocalSpeechConfig = serde_json::from_slice(&bytes).map_err(error)?;
    config.validate()?;
    Ok((config, blake3::hash(&bytes).to_hex().to_string()))
}

/// Bounded loopback health reading. Redirects and proxy use are disabled.
fn probe(stage: &LocalSpeechStage) -> Result<Value> {
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--noproxy",
            "*",
            "--proto",
            "=http",
            "--max-time",
            "5",
            "--max-filesize",
            "65536",
            "--url",
            &stage.endpoint(true),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(error)?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or_else(|| error("Health pipe unavailable"))
        .and_then(|pipe| pipe.take(65537).read_to_end(&mut bytes).map_err(error));
    if let Err(failure) = read {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failure);
    }
    if bytes.len() > 65536 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error("Local speech health response exceeds its bound"));
    }
    if !child.wait().map_err(error)?.success() {
        return Err(error(format!(
            "Local speech health unavailable at {}",
            stage.endpoint(true)
        )));
    }
    Ok(
        json!({"endpoint":stage.endpoint(true),"response_digest":blake3::hash(&bytes).to_hex().to_string(),"json":serde_json::from_slice::<Value>(&bytes).ok(),"standing":"live-http-health-not-inference-or-loaded-model-proof"}),
    )
}

fn stage_relation(
    stage: &LocalSpeechStage,
    revision: &str,
    input: ModelModality,
    output: ModelModality,
    transform: TransformCapability,
) -> Result<ModelRuntimeRelation> {
    let mut modality =
        ModelModalityContract::new(stage.provider_ref.clone(), stage.endpoint(false));
    modality.input_modalities.insert(input);
    modality.output_modalities.insert(output);
    modality
        .transforms
        .insert(transform, DeclaredSupport::Supported);
    modality
        .interaction
        .insert(InteractionCapability::RequestResponse);
    if transform == TransformCapability::SpeechToText {
        modality
            .interaction
            .insert(InteractionCapability::FinalTranscripts);
    }
    modality.transport = TransportKind::Http;
    modality.provider_revision = Some(stage.engine_revision.clone());
    modality.provenance = vec![
        stage.source_ref.clone(),
        format!("aikit:local-speech-config@{revision}"),
        "route-declaration-with-live-health; actual-inference-evidence-is-per-turn".into(),
    ];
    modality.validate()?;
    Ok(ModelRuntimeRelation {
        model: ModelVariantReading {
            model: stage.model_ref.clone(),
            variant: stage.model_id.clone(),
        },
        engine: InferenceEngineReading {
            engine: stage.engine_ref.clone(),
            provider: stage.provider_ref.clone(),
            form: InferenceEngineForm::LightweightServer,
            revision: Some(stage.engine_revision.clone()),
            provider_native: BTreeMap::new(),
        },
        materialisation: ModelMaterialisationReading {
            binding_ref: format!("local-speech:{revision}:{}", stage.model_id),
            workcell_ref: None,
            placement: PlacementObservation::Local,
            endpoint: Some(stage.endpoint(false)),
            provider_native: BTreeMap::from([("declared_model_id".into(), stage.model_id.clone())]),
            resources: MaterialResourceReading::default(),
            lifetime_owner: "machine-local-speech-service".into(),
            retraction: RetractionMode::Unsupported,
        },
        model_surface: ModelSurfaceReading {
            contract: None,
            protocol: "http-request-response".into(),
            capabilities: Default::default(),
            access: ModelAccessReading {
                inference: AccessFieldReading::available(["configured-local-inference-route"]),
                material_control: AccessFieldReading::unavailable(
                    "This route grants no server lifecycle control",
                ),
                interior: AccessFieldReading::unavailable(
                    "This route grants no model interior access",
                ),
            },
            modality: Some(modality),
        },
        change_application: RuntimeChangeApplication::NextSession,
    })
}

fn native_reply(home: &AikitHome, operation: EncounterRequest) -> Result<Value> {
    let envelope = request(&socket_path(home), &operation)?;
    if envelope["ok"] != true {
        return Err(error(format!(
            "Native encounter refused speech disclosure: {}",
            envelope["error"]
        )));
    }
    envelope
        .get("data")
        .cloned()
        .ok_or_else(|| error("Native encounter reply omitted its data"))
}

pub fn disclose(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
    let (config, revision) = read_config(home)?;
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Speech requires an actually admitted native Agency binding"))?;
    native_admission(&binding)?;
    let model = native_reply(
        home,
        EncounterRequest::ModelRead {
            agent_session: session.clone(),
        },
    )?;
    let status = native_reply(
        home,
        EncounterRequest::Status {
            agent_session: session.clone(),
        },
    )?;
    if status["provider"]["body_ref"] != "agent-body/epi-prime-ql"
        || status["provider"]["body_revision"]
            .as_str()
            .is_none_or(str::is_empty)
        || status["error"].as_str().is_some()
    {
        return Err(error(
            "Local Nara speech requires the actual resident Prime–QL body and its revision",
        ));
    }
    let observation = &model["model_observation"];
    let native_provider = observation["native_provider"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error("Native text body has not disclosed its model provider"))?;
    let model_id = observation["current_model_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error("Native text body has not disclosed its model"))?;
    let provider = ProviderRef::parse(format!("provider/{native_provider}"))?;
    let stt_probe = probe(&config.stt)?;
    let tts_probe = probe(&config.tts)?;
    let mut text_modality = ModelModalityContract::new(provider.clone(), "native-prime-rpc");
    text_modality.input_modalities.insert(ModelModality::Text);
    text_modality.output_modalities.insert(ModelModality::Text);
    text_modality
        .interaction
        .insert(InteractionCapability::RequestResponse);
    text_modality.transport = TransportKind::Cli;
    text_modality.connection = ConnectionSemantics::Connected {
        reconnect: ReconnectSupport::Unsupported,
    };
    text_modality.provider_revision = status["provider"]["body_revision"]
        .as_str()
        .map(str::to_owned);
    text_modality.provenance = vec![
        format!("aikit:resident:{}", session),
        "actual-native-session-model-observation".into(),
    ];
    let mut text = stage_relation(
        &config.stt,
        &revision,
        ModelModality::Speech,
        ModelModality::Text,
        TransformCapability::SpeechToText,
    )?;
    text.model = ModelVariantReading {
        model: canonical_model_ref(format!("model:{model_id}"))?,
        variant: model_id.to_owned(),
    };
    text.engine = InferenceEngineReading {
        engine: ResourceRef::parse("engine/prime-rpc")?,
        provider,
        form: InferenceEngineForm::External,
        revision: status["provider"]["body_revision"]
            .as_str()
            .map(str::to_owned),
        provider_native: BTreeMap::new(),
    };
    text.materialisation = ModelMaterialisationReading {
        binding_ref: status["native_session_id"]
            .as_str()
            .ok_or_else(|| error("Native text session identity unavailable"))?
            .to_owned(),
        workcell_ref: None,
        placement: PlacementObservation::Unknown,
        endpoint: None,
        provider_native: BTreeMap::from([("native_provider".into(), native_provider.to_owned())]),
        resources: MaterialResourceReading::default(),
        lifetime_owner: session.to_string(),
        retraction: RetractionMode::NextSession,
    };
    text.model_surface = ModelSurfaceReading {
        contract: None,
        protocol: "prime-rpc".into(),
        capabilities: Default::default(),
        access: ModelAccessReading {
            inference: AccessFieldReading::available(["admitted-native-text-encounter"]),
            material_control: AccessFieldReading::unavailable(
                "This composition grants no provider lifecycle control",
            ),
            interior: AccessFieldReading::unavailable(
                "This composition grants no model interior access",
            ),
        },
        modality: Some(text_modality),
    };
    let mut catalogue = CompositionCatalog::default();
    let mut selections = Vec::new();
    let mut stages = Vec::new();
    for (name, relation) in [
        (
            "stt",
            stage_relation(
                &config.stt,
                &revision,
                ModelModality::Speech,
                ModelModality::Text,
                TransformCapability::SpeechToText,
            )?,
        ),
        ("text", text),
        (
            "tts",
            stage_relation(
                &config.tts,
                &revision,
                ModelModality::Text,
                ModelModality::Speech,
                TransformCapability::TextToSpeech,
            )?,
        ),
    ] {
        let component = ResourceRef::parse(format!("component/local-speech-{name}"))?;
        catalogue.insert_component(ComponentDescriptor::new(component.clone()));
        selections.push(ComponentSelection {
            component: component.clone(),
            resolution_scope: ResolutionScope::new(
                ScopeKind::Host,
                format!("aikit:local-speech-config@{revision}"),
            ),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
                .with_reference(session.to_string()),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession)
                .with_reference(session.to_string()),
            activation_mode: CompositionActivationMode::ProcedureMediated,
        });
        stages.push(ModelStageRelation {
            component,
            relation,
        });
    }
    let composition = resolve_harness_composition(
        &catalogue,
        HarnessCompositionRequest {
            harness: ResourceRef::parse("harness/prime-local-speech-cascade")?,
            project: Some(binding.world_ref.clone()),
            agent: Some(binding.agent_ref.clone()),
            agency: Some(binding.agency_ref.clone()),
            session: Some(session.to_string()),
            model: None,
            selections,
            target_revision: Some(revision.clone()),
            generation: None,
        },
    )?;
    let runtime = disclose_staged_model_runtime(&composition, stages)?;
    Ok(
        json!({"schema":"aikit.local-speech-reading/v1","agent_ref":binding.agent_ref,"agency_ref":binding.agency_ref,"world_ref":binding.world_ref,"world_binding_ref":binding.world_binding_ref,"agent_session_ref":session,"acting_body":status["provider"],"native_session_id":status["native_session_id"],"runtime":runtime,"composition":composition,"configuration":config,"configuration_revision":revision,"probes":{"stt":stt_probe,"tts":tts_probe},"conditions":["Turn-based local speech around the same native Prime text session. Full duplex, barge-in, streaming speech and automatic voice activity detection are not established."],"inference_observed":false}),
    )
}
