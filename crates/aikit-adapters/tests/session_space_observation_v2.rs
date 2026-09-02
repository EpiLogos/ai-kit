use aikit_adapters::{SessionSpaceFileObservationProvider, SESSION_SPACE_OBSERVATION_FILE_VERSION};
use aikit_core::{
    SessionSpaceDefinition, SessionSpaceLifecycle, SessionSpaceRef, SessionSpaceRuntime,
};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn runtime_publishes_real_read_model_and_external_reader_observes_new_revision() {
    let space = SessionSpaceRef::parse("session-space/local-observation-proof").unwrap();
    let mut runtime =
        SessionSpaceRuntime::open(SessionSpaceDefinition::new(space.clone())).unwrap();
    let path = unique_observation_path();

    let publisher = SessionSpaceFileObservationProvider::publish(&path, &runtime).unwrap();
    let reader = SessionSpaceFileObservationProvider::open(&path).unwrap();
    let first = reader.read().unwrap();
    assert_eq!(first.id, space);
    assert_eq!(first.lifecycle, SessionSpaceLifecycle::Open);

    runtime.close().unwrap();
    let owner_observation = publisher.republish(&runtime).unwrap();
    let second = reader.read().unwrap();
    assert!(second.revision > first.revision);
    assert_eq!(second, owner_observation);
    assert_eq!(second.lifecycle, SessionSpaceLifecycle::Closed);

    let raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        raw.get("schema").and_then(serde_json::Value::as_str),
        Some(SESSION_SPACE_OBSERVATION_FILE_VERSION)
    );
    assert_eq!(
        raw.pointer("/read_model/id")
            .and_then(serde_json::Value::as_str),
        Some("session-space/local-observation-proof")
    );

    fs::remove_file(path).unwrap();
}

fn unique_observation_path() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "aikit-session-space-observation-{}-{nonce}.json",
        std::process::id()
    ))
}
