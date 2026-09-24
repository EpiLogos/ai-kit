//! A native Routine invokes the real Factory CLI, stores its owner field in
//! real Redis, and leaves its collection in Factory's durable state.

use aikit_cli::routine_cli::{factory_change_pass, observe_factory_change};
use aikit_cli::routine_dispatch::{
    CatalogMethodResolver, MethodResolver, OccurrenceReading, OccurrenceSource, RoutineDispatcher,
};
use aikit_cli::routine_dispatch::{RoutineRunRequest, RoutineRunner, RunStatus};
use aikit_cli::routine_native::{
    FactoryMethodBinding, NativeActionRunner, NativeBody, NativeMethod,
};
use aikit_core::resource::routine::{
    ProvenMethodBasis, Routine, RoutineAuthority, RoutineSchedulerBinding, RoutineSchedulerState,
    RoutineTrigger, METHOD_PROOF_VERSION,
};
use aikit_core::resource::{ProviderRef, ResourceRef, SourceRef, SourceRevision};
use aikit_core::schedule::{ScheduleRecord, ScheduleShape};
use aikit_store::now_context::{RedisNowConfig, RedisNowStore, NOW_REDIS_CONFIG_SCHEMA};
use aikit_store::{AikitHome, RoutineStore, StoredRoutine};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

struct UnusedOccurrences;

impl OccurrenceSource for UnusedOccurrences {
    fn occurrences(
        &self,
        _schedule: &Value,
        _from: i64,
        _to: i64,
    ) -> aikit_core::Result<OccurrenceReading> {
        panic!("an owner-change event must not ask the calendar for a due schedule")
    }
}

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
    assert!(!owner.field["coverage"].as_array().unwrap().is_empty());
    let durable: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert!(!durable["state"]["sensing"]["collections"]
        .as_array()
        .unwrap()
        .is_empty());

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

#[test]
fn native_owner_change_dispatches_one_project_event_and_recovers_after_redis_loss() {
    let (Ok(factory), Ok(address)) = (
        std::env::var("AIKIT_TEST_FACTORY"),
        std::env::var("AIKIT_TEST_REDIS_ADDR"),
    ) else {
        eprintln!("real Factory and Redis integration is exercised by the dedicated workflow");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("development-state.json");
    let created = Command::new(&factory)
        .args(["conformance", "developmental-state"])
        .arg(&state)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let manifest: Value = serde_json::from_slice(&created.stdout).unwrap();
    let project = manifest["projectRef"].as_str().unwrap();
    let policy = dir.path().join("factory-policy.json");
    std::fs::write(&policy, serde_json::to_vec(&json!({
        "schema":"factory.sensing-policy/v1", "version":1, "project_world_ref":project,
        "sources":[{"id":"factory-native","provider":"factory","scope":project,
            "source_ref":format!("factory:internal:{project}"),"arguments":{"kind":"all"}}],
        "workflows":{"collect":{"enabled":true,"sources":["factory-native"],"schedule":"every:3600000"},
            "field-refresh":{"enabled":true,"sources":[],"schedule":"every:300000"}}
    })).unwrap()).unwrap();
    let home = AikitHome::at(dir.path().join("aikit-home"));
    std::fs::create_dir_all(home.root()).unwrap();
    let config = RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: format!("aikit-factory-change-test-{}", ulid::Ulid::generate()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    };
    std::fs::write(
        home.root().join("redis-now.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let store = RedisNowStore::new(config).unwrap();
    let skill = home
        .root()
        .join("registries/personal/capsules/skill/aikit/factory-field-change");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("manifest.toml"),
        format!(
            r#"schema = 1
id = "skill/aikit/factory-field-change"
kind = "skill"
name = "Factory field change"
description = "METHOD: Refresh one bound Factory project field through the native owner."
[skill]
export_name = "factory-field-change"
[metadata.native-method]
schema = "aikit.native-method/v1"
body = "factory-field-refresh"
actions = ["factory:action/telemetry.field"]
[metadata.native-method.factory]
state = "{}"
policy = "{}"
project_world_ref = "{}"
"#,
            state.display(),
            policy.display(),
            project
        ),
    )
    .unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "# Factory field change\n\nRead the bound Factory owner field.\n",
    )
    .unwrap();
    let resolver = CatalogMethodResolver { home: home.clone() };
    let method_ref = reference("skill/aikit/factory-field-change");
    let method = resolver.resolve(&method_ref).unwrap();
    let native = resolver.native_method(&method_ref).unwrap().unwrap();
    let action = reference("factory:action/telemetry.field");
    let proof = ProvenMethodBasis {
        version: METHOD_PROOF_VERSION.into(),
        method: method.id.clone(),
        method_revision: method.revision.clone().unwrap(),
        proof_ref: reference("proof:test:factory-field-change"),
        context_resolution_ref: reference("context-resolution:test:factory-field-change"),
        activity_refs: vec![reference("activity:test:factory-field-change")],
        return_refs: vec![reference("return:test:factory-field-change")],
        evidence_refs: vec![reference("evidence:test:factory-field-change")],
        verification_refs: vec![reference("verification:test:factory-field-change")],
    };
    let authority = RoutineAuthority {
        authority_ref: reference("authority:test:factory-field-change"),
        revision: Some(SourceRevision::parse("authority-rev-1").unwrap()),
        action_refs: vec![action],
        granted: true,
        unattended: true,
    };
    let mut scheduled = Routine::new(
        reference("routine/factory-field-scheduled"),
        SourceRef::parse("source:aikit:routines/routine/factory-field-scheduled").unwrap(),
        None,
        "Factory field scheduled",
        "Scheduled recovery refresh",
        &method,
        proof.clone(),
        RoutineTrigger::Schedule {
            schedule_ref: "schedule/factory-field-scheduled".into(),
        },
        authority.clone(),
        None,
        vec![],
    )
    .unwrap();
    scheduled
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
            provider_job_id: None,
            observed_state: RoutineSchedulerState::Planned,
        })
        .unwrap();
    scheduled.enable(&method).unwrap();
    RoutineStore::new(home.clone())
        .put(
            StoredRoutine::new(
                scheduled,
                Some(
                    ScheduleRecord::new(
                        reference("schedule/factory-field-scheduled"),
                        ScheduleShape::Every {
                            interval_ms: 300_000,
                        },
                        None,
                    )
                    .unwrap(),
                ),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let mut routine = Routine::new(
        reference("routine/factory-field-change"),
        SourceRef::parse("source:aikit:routines/routine/factory-field-change").unwrap(),
        None,
        "Factory field change",
        "Refresh changed Factory owner field",
        &method,
        proof,
        RoutineTrigger::Event {
            event_ref: format!("aikit.routine-event/v1:factory:field-changed:{project}"),
        },
        authority,
        None,
        vec![],
    )
    .unwrap();
    routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
            provider_job_id: None,
            observed_state: RoutineSchedulerState::Planned,
        })
        .unwrap();
    routine.enable(&method).unwrap();
    let routine_ref = routine.id.clone();
    RoutineStore::new(home.clone())
        .put(StoredRoutine::new(routine, None, None).unwrap())
        .unwrap();
    assert_eq!(native.body, NativeBody::FactoryFieldRefresh);
    let runner = NativeActionRunner {
        home: home.clone(),
        central_root: dir.path().into(),
        ctrl: "ctrl".into(),
        factory: factory.clone(),
    };
    let dispatcher = RoutineDispatcher::new(
        home.clone(),
        UnusedOccurrences,
        CatalogMethodResolver { home: home.clone() },
        NativeActionRunner {
            home: home.clone(),
            central_root: dir.path().into(),
            ctrl: "ctrl".into(),
            factory: factory.clone(),
        },
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let pass = |at| {
        factory_change_pass(&dispatcher, &resolver, at, |binding| {
            observe_factory_change(&home, &runner, binding)
        })
    };
    let first = pass(now);
    assert_eq!(first["changed"], json!([project]), "{first}");
    assert_eq!(
        first["dispatched"][0]["outcome"]["status"], "completed",
        "{first}"
    );
    let first_hot = store.read_factory_sensing(project, None).unwrap().unwrap();
    let unchanged = pass(now + 1);
    assert_eq!(unchanged["changed"], json!([]), "{unchanged}");
    assert_eq!(unchanged["dispatched"], json!([]), "{unchanged}");

    let collected = Command::new(&factory)
        .args(["telemetry", "collect"])
        .arg(&state)
        .arg("--policy")
        .arg(&policy)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        collected.status.success(),
        "{}",
        String::from_utf8_lossy(&collected.stderr)
    );
    let changed = pass(now + 30_000);
    assert_eq!(changed["changed"], json!([project]), "{changed}");
    assert_eq!(
        changed["dispatched"][0]["outcome"]["status"], "completed",
        "{changed}"
    );
    let new_hot = store.read_factory_sensing(project, None).unwrap().unwrap();
    assert_ne!(new_hot.field["cursor"], first_hot.field["cursor"]);

    store.delete_factory_sensing(project, None).unwrap();
    let rebuilt = pass(now + 60_000);
    assert_eq!(rebuilt["changed"], json!([project]), "{rebuilt}");
    assert_eq!(
        rebuilt["dispatched"][0]["outcome"]["status"], "completed",
        "{rebuilt}"
    );
    assert_eq!(
        store
            .read_factory_sensing(project, None)
            .unwrap()
            .unwrap()
            .field["cursor"],
        new_hot.field["cursor"]
    );

    let mut changed_policy: Value =
        serde_json::from_slice(&std::fs::read(&policy).unwrap()).unwrap();
    changed_policy["workflows"]["field-refresh"]["schedule"] = json!("every:600000");
    std::fs::write(&policy, serde_json::to_vec(&changed_policy).unwrap()).unwrap();
    store.delete_factory_sensing(project, None).unwrap();
    let stale_cadence = pass(now + 90_000);
    assert_eq!(
        stale_cadence["dispatched"][0]["outcome"]["status"], "failed",
        "{stale_cadence}"
    );
    assert_eq!(store.read_factory_sensing(project, None).unwrap(), None);

    changed_policy["workflows"]["field-refresh"]["schedule"] = json!("every:300000");
    std::fs::write(&policy, serde_json::to_vec(&changed_policy).unwrap()).unwrap();
    let retried = pass(now + 120_000);
    assert_eq!(retried["changed"], json!([project]), "{retried}");
    assert_eq!(
        retried["dispatched"][0]["outcome"]["status"], "completed",
        "{retried}"
    );
    assert_eq!(
        store
            .read_factory_sensing(project, None)
            .unwrap()
            .unwrap()
            .field["cursor"],
        new_hot.field["cursor"]
    );

    changed_policy["workflows"]["field-refresh"]["enabled"] = json!(false);
    std::fs::write(&policy, serde_json::to_vec(&changed_policy).unwrap()).unwrap();
    store.delete_factory_sensing(project, None).unwrap();
    let refused = pass(now + 150_000);
    assert_eq!(refused["dispatched"], json!([]), "{refused}");
    assert!(
        !refused["failures"].as_array().unwrap().is_empty(),
        "{refused}"
    );
    assert_eq!(store.read_factory_sensing(project, None).unwrap(), None);

    changed_policy["workflows"]["field-refresh"]["enabled"] = json!(true);
    std::fs::write(&policy, serde_json::to_vec(&changed_policy).unwrap()).unwrap();
    let mut scheduled_record = RoutineStore::new(home.clone())
        .get(&reference("routine/factory-field-scheduled"))
        .unwrap();
    scheduled_record.routine.disable();
    RoutineStore::new(home.clone())
        .put(scheduled_record)
        .unwrap();
    let no_fallback = pass(now + 180_000);
    assert_eq!(
        no_fallback["dispatched"][0]["outcome"]["status"], "failed",
        "{no_fallback}"
    );
    assert_eq!(store.read_factory_sensing(project, None).unwrap(), None);

    let mut record = RoutineStore::new(home.clone()).get(&routine_ref).unwrap();
    record.routine.disable();
    RoutineStore::new(home.clone()).put(record).unwrap();
    let disabled = pass(now + 210_000);
    assert_eq!(disabled["considered"], json!([]), "{disabled}");
}
