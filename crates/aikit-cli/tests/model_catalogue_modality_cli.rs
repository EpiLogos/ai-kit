//! `aikit model-catalogue show` against the real binary, on the speech class:
//! a machine with no provider key sees speech as a class of model — visible
//! options whose absent credential is named — and nothing silently drops.
//! Seed and fixture backed: no network, no keys, no provider calls.

use std::collections::BTreeMap;
use std::path::Path;

use aikit_core::credential::{
    CredentialBindingState, CredentialRef, SecretMaterialisationClass, SecretProviderRef,
    SecretProviderTier,
};
use aikit_store::credentials::CredentialBindingStore;
use aikit_store::home::AikitHome;
use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

const SPEECH_MODELS: [&str; 3] = [
    "model:gpt-realtime",
    "model:gpt-4o-transcribe",
    "model:gpt-4o-mini-tts",
];

fn run_show(home: &Path) -> Value {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(["model-catalogue", "show", "--json"])
        .env("AIKIT_HOME", home)
        .current_dir(home)
        .output()
        .expect("aikit model-catalogue show runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "the command must speak the stable envelope; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(
        output.status.success(),
        "the listing must succeed: {envelope}"
    );
    envelope["data"].clone()
}

fn entry<'a>(data: &'a Value, model: &str) -> &'a Value {
    data["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["model"] == model)
        .unwrap_or_else(|| panic!("the listing must show {model}"))
}

fn bind_credential(home: &Path, credential_ref: &str, revoked: bool) {
    let aikit_home = AikitHome::at(home.to_path_buf());
    let state = CredentialBindingState {
        credential_ref: CredentialRef::new(credential_ref).unwrap(),
        provider_ref: SecretProviderRef::new("keychain:test").unwrap(),
        provider_tier: SecretProviderTier::OsSecureStore,
        materialisation: SecretMaterialisationClass::ProviderNativeLease,
        binding_provenance: "binding:test".into(),
        revision_or_lease_class: None,
        expires_at: None,
        revoked,
        metadata: BTreeMap::new(),
    };
    CredentialBindingStore::new(&aikit_home)
        .save(&state)
        .expect("binding saves");
}

#[test]
fn a_keyless_machine_sees_speech_as_a_class_with_each_gap_named() {
    let dir = TempDir::new().unwrap();
    let data = run_show(dir.path());

    // Nothing silently drops: every catalogued model is in the listing.
    let entries = data["entries"].as_array().unwrap().len();
    assert_eq!(
        entries,
        data["catalogued"].as_u64().unwrap() as usize,
        "the listing shows the whole catalogue, gated entries included"
    );

    // Speech is visible as a class, derived from declared facts.
    let speech = data["classes"]["speech"].as_array().unwrap();
    for model in SPEECH_MODELS {
        assert!(
            speech.iter().any(|listed| listed == model),
            "{model} must appear in the speech class: {speech:?}"
        );
    }

    // The realtime option carries its declared class facts and names its gap.
    let realtime = entry(&data, "model:gpt-realtime");
    assert_eq!(realtime["availability"]["state"], "credential-gated");
    assert_eq!(
        realtime["availability"]["missing"], "openai realtime credential",
        "the gap is named: which credential is absent"
    );
    let class = &realtime["modality_classes"][0];
    assert_eq!(class["speech_capable"], true);
    assert_eq!(class["transport"], "websocket");
    for field in ["input_modalities", "output_modalities"] {
        assert!(
            class[field]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m == "speech"),
            "{field} must declare speech"
        );
    }
    assert!(
        class["transforms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "speech-to-speech")
    );
    assert_eq!(
        class["credential"]["condition"], "required",
        "presence stays ref-only: required, never a secret"
    );

    // The cascade endpoints are visible options with their own gaps named.
    let transcribe = entry(&data, "model:gpt-4o-transcribe");
    assert_eq!(transcribe["availability"]["state"], "credential-gated");
    assert_eq!(
        transcribe["availability"]["missing"],
        "openai transcription credential"
    );
    let synthesis = entry(&data, "model:gpt-4o-mini-tts");
    assert_eq!(synthesis["availability"]["state"], "credential-gated");
    assert_eq!(
        synthesis["availability"]["missing"],
        "openai speech credential"
    );

    // A plain text model stays catalogued with nothing claimed about speech.
    let text_model = entry(&data, "model:llama3.2");
    assert_eq!(text_model["availability"]["state"], "catalogued");
    assert_eq!(
        text_model["modality_classes"].as_array().unwrap().len(),
        0,
        "no declared surface joined: nothing is claimed"
    );
    assert!(
        !speech.iter().any(|listed| listed == "model:llama3.2"),
        "a text-only model is not in the speech class"
    );
}

#[test]
fn a_bound_credential_resolves_the_same_option_and_a_revoked_one_does_not() {
    let dir = TempDir::new().unwrap();
    bind_credential(dir.path(), "credential:openai", false);
    let data = run_show(dir.path());

    let realtime = entry(&data, "model:gpt-realtime");
    assert_eq!(
        realtime["availability"]["state"], "catalogued",
        "with the credential bound, the option is no longer gated"
    );
    let class = &realtime["modality_classes"][0];
    assert_eq!(class["credential"]["condition"], "satisfied");
    assert_eq!(class["credential"]["binding_ref"], "credential:openai");
    // Visibility never depended on the key: the class is the same class.
    let speech = data["classes"]["speech"].as_array().unwrap();
    for model in SPEECH_MODELS {
        assert!(speech.iter().any(|listed| listed == model));
    }

    // A revoked binding is not a present credential: the gap is named again.
    let revoked_dir = TempDir::new().unwrap();
    bind_credential(revoked_dir.path(), "credential:openai", true);
    let data = run_show(revoked_dir.path());
    let realtime = entry(&data, "model:gpt-realtime");
    assert_eq!(realtime["availability"]["state"], "credential-gated");
    assert_eq!(
        realtime["availability"]["missing"],
        "openai realtime credential"
    );
}
