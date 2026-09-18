//! `aikit model-modality show` against the real binary: document in, document
//! out. The single-body fixture document is built from the same frozen recorded
//! OpenAI Realtime session the adapter is conformed against, so the disclosure
//! a machine user reads is the one the recorded session proves.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aikit_adapters::openai_realtime::{
    parse_realtime_session, speech_synthesis_surface, transcription_surface,
};
use aikit_core::composition::{
    ActivationScope, ActivationScopeKind, ComponentBinding, CompositionActivationMode,
    CompositionState, HarnessComposition, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
    RetractionMode,
};
use aikit_core::model_modality::{DeclaredSupport, TransformCapability};
use aikit_core::model_runtime::{
    disclose_model_runtime, disclose_staged_model_runtime, AccessFieldReading, InferenceEngineForm,
    InferenceEngineReading, ModelAccessReading, ModelMaterialisationReading, ModelRuntimeRelation,
    ModelStageRelation, ModelSurfaceReading, ModelVariantReading, PlacementObservation,
    RuntimeChangeApplication,
};
use aikit_core::resource::{CredentialCondition, ProviderRef, ResourceRef};
use aikit_core::scope::ScopeKind;
use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

/// The frozen recorded session, shared with the adapter's own conformance
/// tests — one recording, one truth.
const FROZEN_SESSION: &str =
    include_str!("../../aikit-adapters/tests/fixtures/openai-realtime/session.json");

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn write_document(dir: &TempDir, name: &str, value: &Value) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    path
}

fn run_show(home: &Path, document: &Path) -> (bool, Value) {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(["model-modality", "show"])
        .arg("--document")
        .arg(document)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(home)
        .output()
        .expect("aikit model-modality show runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "the command must speak the stable envelope; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

fn composition(model: Option<ResourceRef>, components: &[&str]) -> HarnessComposition {
    let bindings: Vec<ComponentBinding> = components
        .iter()
        .map(|component| ComponentBinding {
            component: r(component),
            resolution_scope: ResolutionScope::new(ScopeKind::Project, "project/voice"),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
            activation_mode: CompositionActivationMode::LiveMounted,
            implementation: None,
        })
        .collect();
    HarnessComposition {
        version: "aikit.harness-composition/v2".into(),
        harness: r("harness/voice"),
        project: Some(r("project/voice")),
        agent: None,
        agency: None,
        session: Some("agent-session-9".into()),
        model,
        component_bindings: bindings,
        contract_bindings: vec![],
        contributions: vec![],
        surfaces: vec![],
        projections: vec![],
        absences: vec![],
        state: CompositionState::Resolved,
        target_revision: Some("target-1".into()),
        generation: Some("generation-1".into()),
        fingerprint: "fp-cli-1".into(),
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
            retraction: RetractionMode::Live,
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

#[test]
fn the_recorded_realtime_session_discloses_through_the_real_binary() {
    let home = TempDir::new().unwrap();
    let bound = CredentialCondition::Satisfied {
        hint: "openai realtime credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    };
    let contract = parse_realtime_session(FROZEN_SESSION, bound.clone()).unwrap();
    let composition = composition(Some(r("model:gpt-realtime")), &["component/voice"]);
    let read = disclose_model_runtime(
        &composition,
        relation(
            "model:gpt-realtime",
            "gpt-realtime",
            "provider:openai",
            Some(contract),
        ),
    )
    .unwrap();
    let document = write_document(
        &home,
        "realtime-read-model.json",
        &serde_json::to_value(&read).unwrap(),
    );

    let (ok, envelope) = run_show(home.path(), &document);
    assert!(ok, "envelope: {envelope}");
    assert_eq!(envelope["schema"], 1);
    let data = &envelope["data"];
    assert_eq!(data["schema"], "aikit.model-modality-disclosure/v1");
    assert_eq!(data["body_kind"], "single");
    assert_eq!(data["identity"]["harness"], "harness/voice");
    assert_eq!(data["speech_capable"], true);

    // The four answers a voice consumer asks first.
    assert_eq!(
        data["answers"]["interaction"]["full-duplex-realtime"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["interaction"]["barge-in"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["interaction"]["vad-turn-detection"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["input_modalities"]["speech"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["output_modalities"]["audio"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["transforms"]["speech-to-speech"]["state"],
        "supported"
    );

    // What the recording does not prove is unsupported with a reason, never
    // silently absent and never claimed.
    assert_eq!(
        data["answers"]["interaction"]["partial-transcripts"]["state"],
        "unsupported"
    );
    assert_eq!(
        data["answers"]["interaction"]["timestamps"]["state"],
        "unsupported"
    );
    assert!(
        data["answers"]["interaction"]["partial-transcripts"]["reason"]
            .as_str()
            .unwrap()
            .contains("gpt-realtime")
    );

    // Transport facts and explanation evidence travel in the same document.
    assert_eq!(data["surface"]["transport"], "websocket");
    assert_eq!(
        data["surface"]["credential_scope"],
        "ephemeral-surface-token"
    );
    assert!(data["explanation"]["facts"].as_array().unwrap().len() > 1);
}

#[test]
fn a_staged_cascade_document_discloses_the_strict_body_view_through_the_binary() {
    let home = TempDir::new().unwrap();
    let bound = CredentialCondition::Satisfied {
        hint: "openai inference credential".into(),
        binding_ref: "credential-binding/openai-1".into(),
    };
    let mut stt = transcription_surface(bound.clone());
    stt.transforms.insert(
        TransformCapability::SpeechToText,
        DeclaredSupport::Supported,
    );
    let tts = speech_synthesis_surface(bound);

    let composition = composition(
        None,
        &["component/stt", "component/text-harness", "component/tts"],
    );
    let read = disclose_staged_model_runtime(
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
            ModelStageRelation {
                component: r("component/text-harness"),
                relation: relation("model:llama3.2", "llama3.2:latest", "provider:ollama", None),
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
    let document = write_document(
        &home,
        "cascade-read-model.json",
        &serde_json::to_value(&read).unwrap(),
    );

    let (ok, envelope) = run_show(home.path(), &document);
    assert!(ok, "envelope: {envelope}");
    let data = &envelope["data"];
    assert_eq!(data["body_kind"], "staged");
    assert_eq!(data["complete"], false);
    assert_eq!(data["speech_capable"], true);
    assert_eq!(data["stages"].as_array().unwrap().len(), 3);
    // Speech enters and leaves the body; interior text is not a body fact
    // while a stage is opaque.
    assert_eq!(
        data["answers"]["input_modalities"]["speech"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["output_modalities"]["speech"]["state"],
        "supported"
    );
    assert_eq!(
        data["answers"]["output_modalities"]["text"]["state"],
        "unknown"
    );
    // Streaming output is withheld by the STT stage and the reason names it.
    assert_eq!(
        data["answers"]["interaction"]["streaming-output"]["state"],
        "unsupported"
    );
    assert!(data["answers"]["interaction"]["streaming-output"]["reason"]
        .as_str()
        .unwrap()
        .contains("component/stt"));
    // The stage-named basis and explanation evidence are present.
    assert!(data["basis"].as_array().unwrap().len() >= 3);
    assert!(data["explanation"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|fact| fact["relation"] == "stage-model-relation"));
}

#[test]
fn an_unrelated_document_fails_with_a_stable_error_code() {
    let home = TempDir::new().unwrap();
    let document = write_document(
        &home,
        "wrong.json",
        &serde_json::json!({"version": "aikit.some-other/v1", "note": "not a read model"}),
    );
    let (ok, envelope) = run_show(home.path(), &document);
    assert!(!ok);
    assert_eq!(envelope["schema"], 1);
    assert_eq!(
        envelope["error"]["code"],
        "model_modality_disclosure.unknown_document"
    );
}
