//! What a new Routine is made from, through the real CLI in an isolated
//! AIKIT_HOME: `routine options` names the Method's verified runs, authority
//! receipts, admitted triggers, Central's agent profiles, the context scopes
//! and the time policy, each a document `routine create` takes as handed;
//! `routine prepare-schedule` validates a shape with Central and names the
//! ScheduleRecord without keeping anything. A stand-in `ctrl` on PATH answers
//! for Central.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use tempfile::TempDir;

const METHOD: &str = "skill/method/demo";
const POLICY_REF: &str = "central:source:control:root:Control/user/civil-time-policy.json";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with one Method-classified skill capsule, and a stand-in `ctrl`.
fn fixture() -> TempDir {
    let home = TempDir::new().unwrap();
    let base = home
        .path()
        .join("registries/personal/capsules/skill/method/demo");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "skill/method/demo"
kind = "skill"
name = "demo-method"
description = "METHOD: demo method — a proven-once automation surface for tests."

[skill]
export_name = "demo-method"
"#,
    );
    write(&base.join("SKILL.md"), "Demo method body.\n");
    let ctrl = home.path().join("bin/ctrl");
    write(
        &ctrl,
        &format!(
            r#"#!/bin/sh
action="$6"
if [ -n "$FAKE_CTRL_REFUSE" ]; then
  printf '%s\n' '{{"ok":false,"status":"invalid_input","error":{{"code":"central.invalid_input","message":"Central resolves no such schedule"}}}}'
  exit 2
fi
case "$action" in
  central.time.occurrences)
    printf '%s\n' '{{"ok":true,"data":{{"time_policy_ref":"{POLICY_REF}","time_policy_revision":"rev-test-1","occurrences":[{{"occurrence_ref":"occurrence:test:1","due_unix_ms":4102444800000}}]}}}}' ;;
  central.time.policy)
    printf '%s\n' '{{"ok":true,"data":{{"policy":{{"timezone":"Europe/London"}},"revision":"rev-test-1","source_ref":"{POLICY_REF}"}}}}' ;;
  agent-profile.list)
    printf '%s\n' '{{"ok":true,"data":{{"profiles":[{{"profile":{{"ref":"agent-profile:expressed-demo","name":"Demo agent","purpose":"Runs the demo"}}}}]}}}}' ;;
  *)
    printf '%s\n' '{{"ok":false,"error":{{"message":"unknown action"}}}}'
    exit 2 ;;
esac
"#
        ),
    );
    let mut permissions = fs::metadata(&ctrl).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&ctrl, permissions).unwrap();
    home
}

fn path_with_ctrl(home: &Path) -> String {
    format!(
        "{}:{}",
        home.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn run_with(home: &Path, args: &[&str], path: &str, refuse: bool) -> (bool, Value, String) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let mut command = Command::new(&bin);
    command
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env("AIKIT_CENTRAL_ROOT", home.join("Central"))
        .env("PATH", path)
        .env_remove("FAKE_CTRL_REFUSE")
        .current_dir(home);
    if refuse {
        command.env("FAKE_CTRL_REFUSE", "1");
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("aikit {args:?} should run: {error}"));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope, stdout)
}

fn ok(home: &Path, args: &[&str]) -> Value {
    let (success, envelope, stdout) = run_with(home, args, &path_with_ctrl(home), false);
    assert!(success, "aikit {args:?} failed: {stdout}");
    envelope
}

fn refused(home: &Path, args: &[&str], path: &str, refuse: bool) -> Value {
    let (success, envelope, stdout) = run_with(home, args, path, refuse);
    assert!(
        !success,
        "aikit {args:?} should have been refused: {stdout}"
    );
    envelope["error"].clone()
}

fn file(home: &Path, name: &str, value: &Value) -> String {
    let path: PathBuf = home.join(name);
    write(&path, &serde_json::to_string(value).unwrap());
    format!("@{}", path.display())
}

fn proof_input(home: &Path) -> String {
    file(
        home,
        "proof-input.json",
        &json!({
            "proof_ref": "proof:demo:v1",
            "context_resolution_ref": "context-resolution:demo",
            "activity_refs": ["activity:demo:1"],
            "return_refs": ["return:demo:1"],
            "evidence_refs": ["evidence:demo:1"],
            "verification_refs": ["verification:demo:1"],
            "invocation_succeeded": true,
            "verification_passed": true,
        }),
    )
}

fn options(home: &Path) -> Value {
    ok(home, &["routine", "options", "--method", METHOD])["data"].clone()
}

#[test]
fn options_name_each_document_create_takes_and_the_prepared_schedule_makes_a_routine() {
    let home = fixture();
    let home = home.path();

    // Nothing proven yet: the Method, the pending authority, Manual, and
    // Central's profiles and time policy.
    let first = options(home);
    assert_eq!(first["schema"], "aikit.routine-options/v1");
    assert_eq!(first["method"]["ref"], METHOD);
    let revision = first["method"]["revision"].as_str().unwrap().to_owned();
    assert!(!revision.is_empty());
    let actions = first["method"]["actions"].clone();
    assert!(!actions.as_array().unwrap().is_empty(), "{first}");
    assert_eq!(first["proofs"], json!([]));
    assert_eq!(first["stale_proofs"], 0);
    let authorities = first["authorities"].as_array().unwrap();
    assert_eq!(authorities.len(), 1, "{first}");
    let pending = authorities[0]["document"].clone();
    assert_eq!(
        pending,
        json!({"authority_ref": "authority:routine:pending-skill-method-demo",
               "action_refs": actions, "granted": false, "unattended": false})
    );
    assert_eq!(first["triggers"].as_array().unwrap().len(), 1);
    assert_eq!(first["triggers"][0]["document"], json!({"kind": "manual"}));
    assert_eq!(
        first["profiles"],
        json!([{"title": "Demo agent", "ref": "agent-profile:expressed-demo"}])
    );
    assert_eq!(first["context_scopes"], json!([]));
    assert_eq!(
        first["time_policy"],
        json!({"ref": POLICY_REF, "revision": "rev-test-1", "timezone": "Europe/London"})
    );
    let absences = first["absences"].to_string();
    assert!(absences.contains("no verified run"), "{absences}");
    assert!(
        absences.contains("no granted authority receipt"),
        "{absences}"
    );

    // `method prove` answers the basis exactly as before, and keeps it.
    let input = proof_input(home);
    let proved = ok(
        home,
        &[
            "method",
            "prove",
            "--method",
            METHOD,
            "--proof-json",
            &input,
        ],
    );
    assert_eq!(proved["warnings"], json!([]), "{proved}");
    let basis = proved["data"].clone();
    let mut keys: Vec<&str> = basis
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "activity_refs",
            "context_resolution_ref",
            "evidence_refs",
            "method",
            "method_revision",
            "proof_ref",
            "return_refs",
            "verification_refs",
            "version"
        ]
    );
    // Proving the same run again keeps one.
    ok(
        home,
        &[
            "method",
            "prove",
            "--method",
            METHOD,
            "--proof-json",
            &input,
        ],
    );
    let proven = options(home);
    assert_eq!(proven["proofs"].as_array().unwrap().len(), 1, "{proven}");
    assert_eq!(proven["proofs"][0]["source"], "kept");
    assert_eq!(proven["proofs"][0]["title"], "proof:demo:v1");
    assert_eq!(proven["proofs"][0]["document"], basis);
    assert!(!proven["absences"].to_string().contains("no verified run"));

    // A prepared schedule is the trigger `create` takes; with the kept proof
    // and the pending authority it makes a Draft.
    let schedule = file(
        home,
        "shape.json",
        &json!({"kind": "daily", "time": "07:30"}),
    );
    let prepared = ok(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &schedule],
    )["data"]
        .clone();
    let trigger = prepared["trigger"].clone();
    let created = ok(
        home,
        &[
            "routine",
            "create",
            "--name",
            "Morning demo",
            "--method",
            METHOD,
            "--proof-json",
            &file(home, "proof.json", &proven["proofs"][0]["document"]),
            "--trigger-json",
            &file(home, "trigger.json", &trigger),
            "--authority-json",
            &file(home, "authority.json", &pending),
            "--agent-profile",
            "agent-profile:expressed-demo",
            "--context-scope",
            "project:Demo",
        ],
    );
    assert_eq!(created["data"]["state"], "draft", "{created}");
    // Enable stays refused: the pending authority grants nothing.
    let refusal = refused(
        home,
        &[
            "routine",
            "enable",
            "routine/morning-demo",
            "--authority-json",
            &file(home, "authority.json", &pending),
        ],
        &path_with_ctrl(home),
        false,
    );
    assert_eq!(
        refusal["code"], "routine.authority_not_granted",
        "{refusal}"
    );

    // The Routine's trigger and context scope are now offered; its proof and
    // authority are the ones already offered, so nothing doubles.
    let after = options(home);
    assert_eq!(after["proofs"].as_array().unwrap().len(), 1, "{after}");
    assert_eq!(after["authorities"].as_array().unwrap().len(), 1, "{after}");
    let triggers = after["triggers"].as_array().unwrap();
    assert_eq!(triggers.len(), 2, "{after}");
    assert_eq!(triggers[1]["document"], trigger);
    assert_eq!(triggers[1]["source"], "routine/morning-demo");
    assert!(triggers[1]["title"]
        .as_str()
        .unwrap()
        .ends_with(": daily at 07:30"));
    assert_eq!(
        after["context_scopes"],
        json!([{"title": "project:Demo", "ref": "project:Demo"}])
    );

    // A Method nobody can resolve is refused, not answered empty.
    refused(
        home,
        &["routine", "options", "--method", "skill/method/absent"],
        &path_with_ctrl(home),
        false,
    );
}

#[test]
fn a_prepared_schedule_is_validated_by_central_allocated_here_and_kept_nowhere() {
    let home = fixture();
    let home = home.path();
    let daily = file(
        home,
        "daily.json",
        &json!({"kind": "daily", "time": "07:30"}),
    );
    let first = ok(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &daily],
    )["data"]
        .clone();
    assert_eq!(first["schema"], "aikit.prepared-schedule/v1");
    assert_eq!(first["kept"], false);
    let trigger = &first["trigger"];
    assert_eq!(trigger["schema"], "aikit.time-schedule/v1");
    assert_eq!(
        trigger["schedule"],
        json!({"kind": "daily", "time": "07:30"})
    );
    let reference = trigger["schedule_ref"].as_str().unwrap();
    assert!(reference.starts_with("schedule/daily-"), "{reference}");
    assert_eq!(
        trigger.as_object().unwrap().len(),
        3,
        "only what create takes: {trigger}"
    );
    assert_eq!(
        first["time_policy"],
        json!({"ref": POLICY_REF, "revision": "rev-test-1"})
    );
    assert_eq!(
        first["next_occurrences"],
        json!([{"occurrence_ref": "occurrence:test:1", "due_unix_ms": 4102444800000i64}])
    );
    // Each preparation allocates its own identity, and nothing is stored.
    let second = ok(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &daily],
    )["data"]
        .clone();
    assert_ne!(second["trigger"]["schedule_ref"], trigger["schedule_ref"]);
    assert_eq!(
        ok(home, &["routine", "list"])["data"]["routines"],
        json!([])
    );

    // AIKit's grammar refuses first, in its own code.
    let late = file(
        home,
        "late.json",
        &json!({"kind": "daily", "time": "25:00"}),
    );
    let refusal = refused(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &late],
        &path_with_ctrl(home),
        false,
    );
    assert_eq!(refusal["code"], "schedule.invalid_daily_time", "{refusal}");
    let unknown = file(home, "unknown.json", &json!({"kind": "fortnightly"}));
    let refusal = refused(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &unknown],
        &path_with_ctrl(home),
        false,
    );
    assert_eq!(refusal["code"], "routine.trigger_invalid", "{refusal}");

    // Central's refusal is given in Central's words; with no Central nothing
    // is prepared.
    let refusal = refused(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &daily],
        &path_with_ctrl(home),
        true,
    );
    assert_eq!(refusal["code"], "central.action_failed", "{refusal}");
    assert!(refusal["message"]
        .as_str()
        .unwrap()
        .contains("Central resolves no such schedule"));
    refused(
        home,
        &["routine", "prepare-schedule", "--schedule-json", &daily],
        "/nonexistent",
        false,
    );
}

#[test]
fn options_name_central_absent_instead_of_answering_empty() {
    let home = fixture();
    let home = home.path();
    let (success, envelope, stdout) = run_with(
        home,
        &["routine", "options", "--method", METHOD],
        "/nonexistent",
        false,
    );
    assert!(success, "{stdout}");
    let data = &envelope["data"];
    assert_eq!(data["time_policy"], Value::Null);
    assert_eq!(data["profiles"], json!([]));
    let absences = data["absences"].to_string();
    assert!(absences.contains("agent profiles:"), "{absences}");
    assert!(absences.contains("time policy:"), "{absences}");
}
