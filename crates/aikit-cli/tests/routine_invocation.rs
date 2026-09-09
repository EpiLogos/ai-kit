//! Public Routine invocation contract through the real AIKit binary and store.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/routine-invocation")
        .join(name)
}

fn request() -> Value {
    serde_json::from_slice(&std::fs::read(fixture("authorised-scheduled-request.json")).unwrap())
        .unwrap()
}

fn expected() -> Value {
    serde_json::from_slice(&std::fs::read(fixture("authorised-scheduled-evidence.json")).unwrap())
        .unwrap()
}

fn run(home: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(bin())
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(home)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "aikit {args:?} must emit JSON ({error}); stdout={stdout:?}, stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output, value)
}

fn write_request(home: &Path, name: &str, value: &Value) -> PathBuf {
    let path = home.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

fn authorise(home: &Path, name: &str, value: &Value) -> (Output, Value) {
    let path = write_request(home, name, value);
    run(
        home,
        &[
            "routine",
            "authorise-invocation",
            "--request-json",
            &format!("@{}", path.display()),
        ],
    )
}

#[test]
fn public_cli_admits_reads_and_idempotently_replays_exact_owner_evidence() {
    let home = TempDir::new().unwrap();
    let request = request();

    let (first_output, first) = authorise(home.path(), "request.json", &request);
    assert!(first_output.status.success(), "{first}");
    assert_eq!(first["data"]["status"], "applied");
    assert_eq!(first["data"]["evidence"], expected());

    let (replay_output, replay) = authorise(home.path(), "replay.json", &request);
    assert!(replay_output.status.success(), "{replay}");
    assert_eq!(replay["data"]["status"], "already-applied");
    assert_eq!(replay["data"]["evidence"], expected());

    let (read_output, read) = run(
        home.path(),
        &[
            "routine",
            "invocation",
            "routine-invocation:2026-09-09:daily-research:1",
        ],
    );
    assert!(read_output.status.success(), "{read}");
    assert_eq!(read["data"], expected());

    let (_, list) = run(home.path(), &["routine", "invocations"]);
    assert_eq!(list["data"].as_array().unwrap(), &[expected()]);
}

#[test]
fn conflicting_reuse_of_invocation_or_delivery_identity_fails_without_mutation() {
    let home = TempDir::new().unwrap();
    let request = request();
    assert!(authorise(home.path(), "first.json", &request)
        .0
        .status
        .success());

    let mut occurrence_conflict = request.clone();
    occurrence_conflict["occurrence"]["observed_at"] = Value::from("2026-09-09T09:30:00.500Z");
    let (output, conflict) = authorise(
        home.path(),
        "occurrence-conflict.json",
        &occurrence_conflict,
    );
    assert!(!output.status.success());
    assert_eq!(
        conflict["error"]["code"],
        "routine.invocation_identity_conflict"
    );

    let mut delivery_conflict = request.clone();
    delivery_conflict["provider_delivery"]["provider_job_id"] = Value::from("changed-job");
    delivery_conflict["routine"]["scheduler"]["provider_job_id"] = Value::from("changed-job");
    let (output, conflict) = authorise(home.path(), "delivery-conflict.json", &delivery_conflict);
    assert!(!output.status.success());
    assert_eq!(
        conflict["error"]["code"],
        "routine.provider_delivery_identity_conflict"
    );

    let (_, list) = run(home.path(), &["routine", "invocations"]);
    assert_eq!(list["data"].as_array().unwrap(), &[expected()]);
}

#[test]
fn provider_retry_and_restart_provenance_accumulates_without_identity_drift() {
    let home = TempDir::new().unwrap();
    let request = request();
    assert!(authorise(home.path(), "first.json", &request)
        .0
        .status
        .success());

    let mut retry = request.clone();
    retry["provider_delivery"]["delivery_ref"] =
        Value::from("provider-delivery:2026-09-09:retry-2");
    retry["provider_delivery"]["provider_job_id"] = Value::from("job-43");
    retry["routine"]["scheduler"]["provider_job_id"] = Value::from("job-43");
    retry["provider_delivery"]["restart_ref"] = Value::from("provider-restart:cron:9");
    let (output, admission) = authorise(home.path(), "retry.json", &retry);
    assert!(output.status.success(), "{admission}");
    assert_eq!(admission["data"]["status"], "delivery-recorded");
    assert_eq!(
        admission["data"]["evidence"]["routine_ref"],
        "routine:daily-research"
    );
    assert_eq!(
        admission["data"]["evidence"]["invocation_ref"],
        "routine-invocation:2026-09-09:daily-research:1"
    );
    assert_eq!(
        admission["data"]["evidence"]["provider_deliveries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn disabled_stale_and_revoked_routines_produce_no_authorised_envelope() {
    for (name, mutate, expected_code) in [
        (
            "disabled",
            mutate_disabled as fn(&mut Value),
            "routine.not_enabled",
        ),
        ("stale", mutate_stale, "routine.proof_method_mismatch"),
        ("revoked", mutate_revoked, "routine.authority_revoked"),
    ] {
        let home = TempDir::new().unwrap();
        let mut value = request();
        mutate(&mut value);
        let (output, failure) = authorise(home.path(), &format!("{name}.json"), &value);
        assert!(!output.status.success(), "{name}: {failure}");
        assert_eq!(failure["error"]["code"], expected_code, "{name}");
        let (_, list) = run(home.path(), &["routine", "invocations"]);
        assert!(list["data"].as_array().unwrap().is_empty(), "{name}");
    }
}

#[test]
fn inconsistent_owner_supplied_authority_provenance_fails_without_write() {
    for (name, mutate, expected_code) in [
        (
            "authority-ref",
            mismatch_authority_ref as fn(&mut Value),
            "routine.authority_validation_mismatch",
        ),
        (
            "authority-revision",
            mismatch_authority_revision,
            "routine.authority_validation_revision_mismatch",
        ),
        (
            "authority-time",
            authority_precedes_trigger,
            "routine.authority_validation_precedes_trigger",
        ),
    ] {
        let home = TempDir::new().unwrap();
        let mut value = request();
        mutate(&mut value);
        let (output, failure) = authorise(home.path(), &format!("{name}.json"), &value);
        assert!(!output.status.success(), "{name}: {failure}");
        assert_eq!(failure["error"]["code"], expected_code, "{name}");
        let (_, list) = run(home.path(), &["routine", "invocations"]);
        assert!(list["data"].as_array().unwrap().is_empty(), "{name}");
    }
}

#[test]
fn deserialised_bodies_must_satisfy_constructor_invariants_and_reject_unknown_fields() {
    for (name, mutate, expected_code) in [
        (
            "empty-method-name",
            empty_method_name as fn(&mut Value),
            "method.name_empty",
        ),
        (
            "incomplete-proof",
            incomplete_proof,
            "routine.proof_evidence_incomplete",
        ),
        (
            "invalid-scheduler",
            invalid_scheduler,
            "routine.provider_job_id_empty",
        ),
    ] {
        let home = TempDir::new().unwrap();
        let mut value = request();
        mutate(&mut value);
        let (output, failure) = authorise(home.path(), &format!("{name}.json"), &value);
        assert!(!output.status.success(), "{name}: {failure}");
        assert_eq!(failure["error"]["code"], expected_code, "{name}");
        let (_, list) = run(home.path(), &["routine", "invocations"]);
        assert!(list["data"].as_array().unwrap().is_empty(), "{name}");
    }

    let home = TempDir::new().unwrap();
    let mut unknown = request();
    unknown["authority_validation"]["trusted_by_cli"] = Value::Bool(true);
    let (output, failure) = authorise(home.path(), "unknown.json", &unknown);
    assert!(!output.status.success(), "{failure}");
    assert_eq!(failure["error"]["code"], "cli.structured_json_invalid");
    let (_, list) = run(home.path(), &["routine", "invocations"]);
    assert!(list["data"].as_array().unwrap().is_empty());
}

fn empty_method_name(value: &mut Value) {
    value["method"]["name"] = Value::from(" ");
}

fn incomplete_proof(value: &mut Value) {
    value["routine"]["proof"]["return_refs"] = serde_json::json!([]);
}

fn invalid_scheduler(value: &mut Value) {
    value["routine"]["scheduler"]["provider_job_id"] = Value::from(" ");
    value["provider_delivery"]["provider_job_id"] = Value::from(" ");
}

#[test]
fn trigger_and_delivery_receipts_are_globally_bound_to_one_invocation() {
    let home = TempDir::new().unwrap();
    let first = request();
    assert!(authorise(home.path(), "first.json", &first)
        .0
        .status
        .success());

    let mut reused_trigger = request();
    reused_trigger["occurrence"]["invocation_ref"] = Value::from("routine-invocation:other");
    reused_trigger["provider_delivery"]["delivery_ref"] = Value::from("provider-delivery:other");
    let (output, failure) = authorise(home.path(), "reused-trigger.json", &reused_trigger);
    assert!(!output.status.success(), "{failure}");
    assert_eq!(
        failure["error"]["code"],
        "routine.trigger_observation_identity_conflict"
    );

    let mut reused_delivery = request();
    reused_delivery["occurrence"]["invocation_ref"] =
        Value::from("routine-invocation:other-delivery");
    reused_delivery["occurrence"]["trigger_observation"]["observation_ref"] =
        Value::from("trigger-observation:other-delivery");
    let (output, failure) = authorise(home.path(), "reused-delivery.json", &reused_delivery);
    assert!(!output.status.success(), "{failure}");
    assert_eq!(
        failure["error"]["code"],
        "routine.provider_delivery_identity_conflict"
    );

    let (_, list) = run(home.path(), &["routine", "invocations"]);
    assert_eq!(list["data"].as_array().unwrap(), &[expected()]);
}

#[test]
fn bound_provider_job_identity_cannot_be_silently_unavailable() {
    let home = TempDir::new().unwrap();
    let mut value = request();
    value["provider_delivery"]
        .as_object_mut()
        .unwrap()
        .remove("provider_job_id");
    let (output, failure) = authorise(home.path(), "missing-job.json", &value);
    assert!(!output.status.success(), "{failure}");
    assert_eq!(
        failure["error"]["code"],
        "routine.provider_delivery_job_unavailable"
    );
    let (_, list) = run(home.path(), &["routine", "invocations"]);
    assert!(list["data"].as_array().unwrap().is_empty());
}

#[test]
fn persisted_ledger_rejects_unknown_security_fields() {
    let home = TempDir::new().unwrap();
    let value = request();
    assert!(authorise(home.path(), "first.json", &value)
        .0
        .status
        .success());

    let ledger_path = home.path().join("state/routine-invocations.json");
    let mut ledger: Value = serde_json::from_slice(&std::fs::read(&ledger_path).unwrap()).unwrap();
    ledger["trusted"] = Value::Bool(true);
    std::fs::write(&ledger_path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();

    let (output, failure) = run(home.path(), &["routine", "invocations"]);
    assert!(!output.status.success(), "{failure}");
    assert_eq!(
        failure["error"]["code"],
        "routine.invalid_invocation_ledger"
    );
}

fn mismatch_authority_ref(value: &mut Value) {
    value["authority_validation"]["authority_ref"] = Value::from("authority:unrelated");
}

fn mismatch_authority_revision(value: &mut Value) {
    value["authority_validation"]["authority_revision"] = Value::from("authority-rev-old");
}

fn authority_precedes_trigger(value: &mut Value) {
    value["authority_validation"]["validated_at"] = Value::from("2026-09-09T09:29:59Z");
}

fn mutate_disabled(value: &mut Value) {
    value["routine"]["state"] = Value::from("disabled");
}

fn mutate_stale(value: &mut Value) {
    value["method"]["revision"] = Value::from("method-rev-2");
}

fn mutate_revoked(value: &mut Value) {
    value["routine"]["authority"]["granted"] = Value::Bool(false);
    value["authority_validation"]["granted"] = Value::Bool(false);
}

#[test]
fn two_occurrences_are_distinct_and_manual_invocation_needs_no_scheduler() {
    let home = TempDir::new().unwrap();
    let first = request();
    assert!(authorise(home.path(), "first.json", &first)
        .0
        .status
        .success());

    let mut second = request();
    second["occurrence"]["invocation_ref"] =
        Value::from("routine-invocation:2026-09-09:daily-research:2");
    second["occurrence"]["trigger_observation"]["observation_ref"] =
        Value::from("trigger-observation:2026-09-09:2");
    second["provider_delivery"]["delivery_ref"] = Value::from("provider-delivery:2026-09-09:2");
    assert!(authorise(home.path(), "second.json", &second)
        .0
        .status
        .success());

    let manual_home = TempDir::new().unwrap();
    let mut manual = request();
    manual["routine"]["id"] = Value::from("routine:manual-research");
    manual["routine"]["revision"] = Value::from("routine-rev-manual");
    manual["routine"]["trigger"] = serde_json::json!({"kind": "manual"});
    manual["routine"]["authority"]["unattended"] = Value::Bool(false);
    manual["routine"]
        .as_object_mut()
        .unwrap()
        .remove("scheduler");
    manual["occurrence"]["invocation_ref"] = Value::from("routine-invocation:manual:1");
    manual["occurrence"]["trigger_observation"]["routine"] = Value::from("routine:manual-research");
    manual["occurrence"]["trigger_observation"]["observation_ref"] =
        Value::from("trigger-observation:manual:1");
    manual["occurrence"]["trigger_observation"]["trigger"] = serde_json::json!({"kind": "manual"});
    manual["authority_validation"]["unattended"] = Value::Bool(false);
    manual.as_object_mut().unwrap().remove("provider_delivery");
    let (output, admission) = authorise(manual_home.path(), "manual.json", &manual);
    assert!(output.status.success(), "{admission}");
    assert_eq!(admission["data"]["evidence"]["trigger"]["kind"], "manual");
    assert!(admission["data"]["evidence"]["provider_deliveries"]
        .as_array()
        .unwrap()
        .is_empty());
}
