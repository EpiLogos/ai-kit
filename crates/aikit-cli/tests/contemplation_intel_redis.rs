//! `aikit now-context publish-intelligence` against a real disposable Redis
//! (loopback, `AIKIT_TEST_REDIS_ADDR` — see `redis_now_context.rs` in
//! aikit-store for the pre-existing warm/cold/CAS/revocation/restart/
//! disclosure-isolation coverage this reuses rather than repeats). New
//! coverage here: the field+decision+test-selection items actually publish
//! with correct source refs/revisions, the decision's `jev_invocation_ref`
//! threads onto the prepared view, and a fresh participant replays the
//! appended changes from cursor zero.

use aikit_cli::cli::NowPublishIntelligenceArgs;
use aikit_cli::contemplation_field::FIELD_SCHEMA;
use aikit_cli::contemplation_intel::{now_publish_intelligence, DECISION_SCHEMA};
use aikit_core::ResourceRef;
use aikit_store::now_context::{RedisNowConfig, RedisNowStore, NOW_REDIS_CONFIG_SCHEMA};
use serde_json::json;

fn config(address: String) -> RedisNowConfig {
    RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: format!("aikit-intel-test-{}", ulid::Ulid::generate()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    }
}

#[test]
fn field_decision_and_test_selection_publish_and_replay_as_participant_changes() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("skip: AIKIT_TEST_REDIS_ADDR is not set; real Redis integration is exercised by the dedicated workflow/local runner");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("redis-now.json");
    std::fs::write(&config_path, serde_json::to_vec(&config(address)).unwrap()).unwrap();

    let field_path = dir.path().join("field.json");
    std::fs::write(
        &field_path,
        serde_json::to_vec(&json!({"schema": FIELD_SCHEMA, "pass": "prospective"})).unwrap(),
    )
    .unwrap();
    let decision_path = dir.path().join("decision.json");
    let invocation_ref = ResourceRef::parse("invocation/jev-contemplate/deadbeef").unwrap();
    std::fs::write(
        &decision_path,
        serde_json::to_vec(&json!({
            "schema": DECISION_SCHEMA,
            "invocation_ref": invocation_ref,
            "pass": "prospective",
        }))
        .unwrap(),
    )
    .unwrap();
    let test_selection_path = dir.path().join("test-selection.json");
    std::fs::write(
        &test_selection_path,
        serde_json::to_vec(
            &json!({"schema": "aikit.test-selection/v1", "recommended_disposition": "Direct"}),
        )
        .unwrap(),
    )
    .unwrap();

    let participant = "participant/intelligence-arm/implementer".to_string();
    let result = now_publish_intelligence(NowPublishIntelligenceArgs {
        config_file: config_path.clone(),
        participant_ref: participant.clone(),
        project_ref: "project/intelligence-arm".into(),
        now_ref: "now/intelligence-arm".into(),
        agent_session: "agent-session/intelligence-arm".into(),
        concern: "publish contemplation intelligence".into(),
        disclosure_revision: "disclosure-1".into(),
        expected_version: 0,
        field_file: field_path,
        decision_file: Some(decision_path),
        test_selection_file: Some(test_selection_path),
        central_root: None,
        ctrl_bin: None,
        now_basis_refs: vec![
            "workcell:now:root=root-rev-1".into(),
            "workcell:now:child=child-rev-1".into(),
        ],
        allow_env_import: false,
    })
    .unwrap();

    assert_eq!(result["publishedVersion"], 1);
    assert_eq!(result["jevInvocationRef"], invocation_ref.to_string());
    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "field + decision + test-selection");
    let appended = result["appendedChanges"].as_array().unwrap();
    assert_eq!(appended.len(), 3, "one change appended per published item");

    // A fresh participant session replays every appended change from cursor 0,
    // re-opening under the exact same key prefix the CLI call used.
    let config: RedisNowConfig =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    let store = RedisNowStore::new(config).unwrap();
    let participant_ref = ResourceRef::parse(&participant).unwrap();
    let replayed = store.read_changes(&participant_ref, 0, 10, None).unwrap();
    assert_eq!(replayed.len(), 3);
    let kinds: Vec<&str> = replayed.iter().map(|c| c.change.kind.as_str()).collect();
    assert!(kinds
        .iter()
        .all(|k| *k == "contemplation-intelligence-published"));

    let prepared = store
        .read_prepared(&participant_ref, false, None)
        .unwrap()
        .expect("published view must be readable");
    assert!(
        prepared
            .basis
            .source_revisions
            .keys()
            .any(|k| k == "workcell:now:root"),
        "supplied now-basis-ref must fold into the prepared basis"
    );
    assert_eq!(
        prepared.jev_invocation_ref.as_ref().unwrap().to_string(),
        invocation_ref.to_string()
    );
}
