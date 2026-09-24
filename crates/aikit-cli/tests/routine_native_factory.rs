//! A native Routine invokes the real Factory CLI, stores its owner field in
//! real Redis, and leaves its collection in Factory's durable state.

use aikit_cli::routine_dispatch::{RoutineRunRequest, RoutineRunner, RunStatus};
use aikit_cli::routine_native::{
    FactoryMethodBinding, NativeActionRunner, NativeBody, NativeMethod,
};
use aikit_core::resource::{ResourceRef, SourceRevision};
use aikit_core::schedule::{ScheduleRecord, ScheduleShape};
use aikit_store::now_context::{RedisNowConfig, RedisNowStore, NOW_REDIS_CONFIG_SCHEMA};
use aikit_store::AikitHome;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

fn reference(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn admitted_factory_collect_runs_natively_and_refresh_rebuilds_the_hot_field() {
    let Ok(factory) = std::env::var("AIKIT_TEST_FACTORY") else {
        eprintln!("AIKIT_TEST_FACTORY absent; real Factory Routine integration is exercised by the dedicated workflow");
        return;
    };
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("AIKIT_TEST_REDIS_ADDR absent; real Redis Routine integration is exercised by the dedicated workflow");
        return;
    };
    assert!(
        Path::new(&factory).is_file(),
        "AIKIT_TEST_FACTORY must name the real Factory executable"
    );
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("development-state.json");
    let output = Command::new(&factory)
        .args(["conformance", "developmental-state"])
        .arg(&state)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: Value = serde_json::from_slice(&output.stdout).unwrap();
    let project = manifest["projectRef"].as_str().unwrap();
    let policy = dir.path().join("factory-policy.json");
    std::fs::write(
        &policy,
        serde_json::to_vec(&json!({
            "schema":"factory.sensing-policy/v1", "version":1,"project_world_ref":project,
            "sources":[{"id":"factory-native","provider":"factory","scope":project,
                "source_ref":format!("factory:internal:{project}"),"arguments":{"kind":"all"}}],
            "workflows":{"collect":{"enabled":true,"sources":["factory-native"],"schedule":"every:3600000"},
                "field-refresh":{"enabled":true,"sources":[],"schedule":"every:300000"}}
        }))
        .unwrap(),
    )
    .unwrap();
    let config = RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: format!("aikit-factory-routine-test-{}", ulid::Ulid::generate()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    };
    let home = AikitHome::at(dir.path().join("aikit-home"));
    std::fs::create_dir_all(home.root()).unwrap();
    std::fs::write(
        home.root().join("redis-now.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let store = RedisNowStore::new(config).unwrap();
    let runner = NativeActionRunner {
        home,
        central_root: dir.path().into(),
        ctrl: "ctrl".into(),
        factory,
    };
    let binding = FactoryMethodBinding {
        state: state.clone(),
        policy: policy.clone(),
        project_world_ref: project.into(),
    };
    let request = |body, invocation: &str, actions: Vec<ResourceRef>| RoutineRunRequest {
        routine_ref: reference("routine/factory-test"),
        invocation_ref: reference(invocation),
        trigger_observation_ref: reference("trigger-observation/factory-test"),
        method_ref: reference("skill/test/factory"),
        method_revision: SourceRevision::parse("factory-method-rev-1").unwrap(),
        prompt: "factory native sensing".into(),
        observation_payload: None,
        time_schedule: Some(
            ScheduleRecord::new(
                reference(if body == NativeBody::FactoryCollect {
                    "schedule/factory-collect"
                } else {
                    "schedule/factory-field-refresh"
                }),
                ScheduleShape::Every {
                    interval_ms: if body == NativeBody::FactoryCollect {
                        3_600_000
                    } else {
                        300_000
                    },
                },
                None,
            )
            .unwrap(),
        ),
        native: Some(NativeMethod {
            body,
            actions: actions.clone(),
            credentials: vec![],
            factory: Some(binding.clone()),
        }),
        authorised_actions: actions,
    };
    let collect = reference("factory:action/telemetry.collect");
    let field = reference("factory:action/telemetry.field");
    let first = runner.run(request(
        NativeBody::FactoryCollect,
        "routine-invocation/factory-collect-1",
        vec![collect, field.clone()],
    ));
    assert_eq!(first.status, RunStatus::Completed, "{}", first.detail);
    let owner = store.read_factory_sensing(project, None).unwrap().unwrap();
    assert_eq!(owner.version, 1);
    assert_eq!(owner.field["project_world_ref"], project);
    assert!(owner.field["coverage"].as_array().unwrap().len() > 0);
    let durable: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert!(
        durable["state"]["sensing"]["collections"]
            .as_array()
            .unwrap()
            .len()
            > 0
    );

    let second = runner.run(request(
        NativeBody::FactoryFieldRefresh,
        "routine-invocation/factory-refresh-1",
        vec![field],
    ));
    assert_eq!(second.status, RunStatus::Completed, "{}", second.detail);
    let refreshed = store.read_factory_sensing(project, None).unwrap().unwrap();
    assert_eq!(refreshed.version, 2);
    assert_eq!(refreshed.source_revision, owner.source_revision);
    let mut changed_policy: Value =
        serde_json::from_slice(&std::fs::read(&policy).unwrap()).unwrap();
    changed_policy["workflows"]["field-refresh"]["schedule"] = json!("every:600000");
    std::fs::write(&policy, serde_json::to_vec(&changed_policy).unwrap()).unwrap();
    let refused = runner.run(request(
        NativeBody::FactoryFieldRefresh,
        "routine-invocation/factory-refresh-stale",
        vec![reference("factory:action/telemetry.field")],
    ));
    assert_eq!(refused.status, RunStatus::Failed);
    assert!(
        refused.detail.contains("policy-cadence"),
        "{}",
        refused.detail
    );
    assert_eq!(store.factory_sensing_version(project, None).unwrap(), 2);
    changed_policy["workflows"]["field-refresh"]["schedule"] = json!("every:300000");
    std::fs::write(&policy, serde_json::to_vec(&changed_policy).unwrap()).unwrap();
    store.delete_factory_sensing(project, None).unwrap();
    assert!(store.read_factory_sensing(project, None).unwrap().is_none());
    let rebuilt = runner.run(request(
        NativeBody::FactoryFieldRefresh,
        "routine-invocation/factory-refresh-2",
        vec![reference("factory:action/telemetry.field")],
    ));
    assert_eq!(rebuilt.status, RunStatus::Completed, "{}", rebuilt.detail);
    assert_eq!(
        store
            .read_factory_sensing(project, None)
            .unwrap()
            .unwrap()
            .source_revision,
        owner.source_revision
    );
    store.delete_factory_sensing(project, None).unwrap();
}
