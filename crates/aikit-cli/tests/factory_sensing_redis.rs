//! The public CLI reads exactly one Project's hot Factory field from real
//! Redis. It does not infer empty signals when that Project has no projection.

use aikit_store::now_context::{
    FactorySensingProjection, RedisNowConfig, RedisNowStore, FACTORY_SENSING_PROJECTION_SCHEMA,
    NOW_REDIS_CONFIG_SCHEMA,
};
use serde_json::{json, Value};
use std::process::Command;

#[test]
fn factory_sensing_cli_is_project_scoped_and_reports_a_missing_hot_field() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("AIKIT_TEST_REDIS_ADDR absent; real Redis CLI integration is exercised by the dedicated workflow");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let config = RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: format!("aikit-factory-cli-test-{}", ulid::Ulid::generate()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    };
    let config_path = dir.path().join("redis-now.json");
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let store = RedisNowStore::new(config).unwrap();
    let read = |project: &str| -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .args(["now-context", "factory-sensing", "--config-file"])
            .arg(&config_path)
            .args(["--project-world-ref", project, "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["ok"], true, "{envelope}");
        envelope["data"].clone()
    };
    let a = "project:Alpha";
    let b = "project:Beta";
    let absent = read(b);
    assert_eq!(absent["available"], false);
    assert!(absent["projection"].is_null());
    let projection = FactorySensingProjection {
        schema: FACTORY_SENSING_PROJECTION_SCHEMA.into(),
        project_world_ref: a.into(),
        version: 1,
        source_revision: "blake3:factory-a1".into(),
        field: json!({"schema":"factory.telemetry-field/v1", "project_world_ref":a,
            "source_revision":"blake3:factory-a1", "source_sequence":1, "observed_at_unix_ms":10,
            "signals":[{"signal_ref":"factory:signal:s1","source_refs":["factory:attempt:a1"],
                "summary":"Attempt failed", "classification":"verified-defect","disposition":"investigate"}],
            "coverage":[],"owner_basis":{},"absences":[],"cursor":"blake3:factory-a1",
            "counts":{"signals":1},"truncated":false}),
        published_at_unix_ms: 11,
    };
    store.publish_factory_sensing(&projection, 0, None).unwrap();
    let present = read(a);
    assert_eq!(present["available"], true);
    assert_eq!(
        present["projection"]["field"]["signals"][0]["signal_ref"],
        "factory:signal:s1"
    );
    assert_eq!(read(b)["available"], false);
    store.delete_factory_sensing(a, None).unwrap();
    assert_eq!(read(a)["available"], false);
}
