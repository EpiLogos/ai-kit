//! Foreign cron reconciliation through the real CLI (parent §2, addendum
//! A-6): import with a proven Method reconciles into a Disabled Routine with
//! the foreign binding; import without proof is refused and the job shows as
//! unreconciled; the harness stores are only ever read.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with one Method capsule whose payload the foreign job names, plus
/// the two live harness cron stores pinned to their real shapes.
fn fixture() -> TempDir {
    let home = TempDir::new().unwrap();

    let base = home
        .path()
        .join("registries/personal/capsules/skill/method/nara");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "skill/method/nara"
kind = "skill"
name = "nara-flow-compose"
description = "METHOD: Nara daily flow compose — compose the daily Nara flow document."

[skill]
export_name = "nara-flow-compose"
"#,
    );
    write(&base.join("SKILL.md"), "Compose the flow.\n");

    // OpenClaw store, pinned to the live shape (2026-09-23).
    write(
        &home.path().join(".openclaw/cron/jobs.json"),
        &json!({
            "version": 1,
            "jobs": [{
                "id": "9efb7069-a72b-4ccc-8b4e-4e9134c58b57",
                "agentId": "main",
                "name": "tmux-completion-watcher",
                "enabled": true,
                "createdAtMs": 1770056357140_u64,
                "updatedAtMs": 1770135495469_u64,
                "schedule": { "kind": "every", "everyMs": 120000 },
                "sessionTarget": "main",
                "wakeMode": "next-heartbeat",
                "payload": {
                    "kind": "systemEvent",
                    "text": "Tmux completion check: notify on new completions."
                },
                "state": { "lastStatus": "ok" }
            }]
        })
        .to_string(),
    );

    // Hermes store, pinned to the live shape: one prompt job naming the
    // Method, one script-only job with no Method in it.
    write(
        &home.path().join(".hermes/cron/jobs.json"),
        &json!({
            "jobs": [
                {
                    "id": "2e45cb4eef8a",
                    "name": "Nara Daily Flow Compose (06:00)",
                    "prompt": "You are Hermes-Nara composing the daily Nara flow document; run nara-flow-compose work.",
                    "skills": [],
                    "skill": null,
                    "model": null,
                    "provider": null,
                    "script": null,
                    "no_agent": false,
                    "schedule": { "kind": "cron", "expr": "0 6 * * *", "display": "0 6 * * *" },
                    "enabled": true,
                    "state": "active",
                    "deliver": "origin",
                    "workdir": "/tmp/nara"
                },
                {
                    "id": "22d0554dfc4c",
                    "name": "Nara Weekly Flow Archive",
                    "prompt": "",
                    "skills": [],
                    "skill": null,
                    "script": "archive-weekly-flows.py",
                    "no_agent": true,
                    "schedule": { "kind": "cron", "expr": "0 7 * * 0", "display": "0 7 * * 0" },
                    "enabled": true,
                    "state": "active"
                }
            ]
        })
        .to_string(),
    );
    home
}

fn run(home: &Path, args: &[&str]) -> (bool, Value, String) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(home)
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

#[test]
fn import_without_proof_is_refused_and_listed_unreconciled() {
    let home = fixture();

    let args = [
        "routine",
        "import-foreign",
        "--provider",
        "hermes-cron",
        "--job-id",
        "2e45cb4eef8a",
        "--method",
        "skill/method/nara",
    ];
    let (success, envelope, _) = run(home.path(), &args);
    assert!(!success, "import without proof must be refused");
    assert_eq!(envelope["error"]["code"], "routine.import_proof_required");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("proven basis"),
        "the refusal must say what is missing: {message}"
    );

    // The refused job is visible in the read-only reconciliation face.
    let listed = {
        let (success, envelope, stdout) = run(home.path(), &["routine", "list"]);
        assert!(success, "{stdout}");
        envelope["data"].clone()
    };
    let hermes = listed["foreign_reconciliation"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "hermes-cron")
        .unwrap()
        .clone();
    let jobs = hermes["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    let nara = jobs
        .iter()
        .find(|job| job["job_id"] == "2e45cb4eef8a")
        .unwrap();
    assert_eq!(nara["reconciled"], false);
    assert_eq!(nara["state"], "unreconciled");
    assert!(nara["reason"]
        .as_str()
        .unwrap()
        .contains("no Routine claims this timer"));

    // The script-only job is unreconciled for a different, honest reason.
    let archive = jobs
        .iter()
        .find(|job| job["job_id"] == "22d0554dfc4c")
        .unwrap();
    assert_eq!(archive["state"], "unreconciled");
    assert!(archive["reason"]
        .as_str()
        .unwrap()
        .contains("runs a script with no agent"));
}

#[test]
fn import_report_reads_without_creating_anything() {
    let home = fixture();
    let (success, envelope, _) = run(
        home.path(),
        &[
            "routine",
            "import-foreign",
            "--provider",
            "openclaw-cron",
            "--job-id",
            "9efb7069-a72b-4ccc-8b4e-4e9134c58b57",
            "--report",
        ],
    );
    assert!(success);
    let report = &envelope["data"];
    assert_eq!(report["report"], true);
    assert_eq!(
        report["job"]["job_id"],
        "9efb7069-a72b-4ccc-8b4e-4e9134c58b57"
    );
    assert_eq!(report["job"]["schedule"]["kind"], "every");
    assert_eq!(report["job"]["schedule"]["interval_ms"], 120_000);
    // Nothing was created.
    let (_, store_check, _) = run(home.path(), &["routine", "list"]);
    assert_eq!(store_check["data"]["routines"].as_array().unwrap().len(), 0);
}

#[test]
fn unknown_jobs_and_providers_are_refused_in_plain_words() {
    let home = fixture();
    let (success, envelope, _) = run(
        home.path(),
        &[
            "routine",
            "import-foreign",
            "--provider",
            "hermes-cron",
            "--job-id",
            "does-not-exist",
        ],
    );
    assert!(!success);
    assert_eq!(envelope["error"]["code"], "routine.import_job_not_found");

    let (success, envelope, _) = run(
        home.path(),
        &[
            "routine",
            "import-foreign",
            "--provider",
            "crontab",
            "--job-id",
            "2e45cb4eef8a",
        ],
    );
    assert!(!success);
    assert_eq!(envelope["error"]["code"], "routine.import_unknown_provider");
}

#[test]
fn import_with_proven_method_creates_a_disabled_routine_with_the_foreign_binding() {
    let home = fixture();

    // Prove the Method the Hermes job's payload runs.
    let proof_path = home.path().join("proof.json");
    write(
        &proof_path,
        &serde_json::to_string(&json!({
            "proof_ref": "proof:nara:v1",
            "context_resolution_ref": "context-resolution:nara",
            "activity_refs": ["activity:nara:1"],
            "return_refs": ["return:nara:1"],
            "evidence_refs": ["evidence:nara:1"],
            "verification_refs": ["verification:nara:1"],
            "invocation_succeeded": true,
            "verification_passed": true
        }))
        .unwrap(),
    );
    let prove = run(
        home.path(),
        &[
            "method",
            "prove",
            "--method",
            "skill/method/nara",
            "--proof-json",
            &format!("@{}", proof_path.display()),
        ],
    );
    assert!(prove.0, "{}", prove.2);
    let basis_file = home.path().join("basis.json");
    write(
        &basis_file,
        &serde_json::to_string_pretty(&prove.1["data"]).unwrap(),
    );

    // Import by Method inference (the payload names the Method's capsule)...
    let import = run(
        home.path(),
        &[
            "routine",
            "import-foreign",
            "--provider",
            "hermes-cron",
            "--job-id",
            "2e45cb4eef8a",
            "--proof-json",
            &format!("@{}", basis_file.display()),
            "--adopt",
        ],
    );
    assert!(import.0, "{}", import.2);
    let receipt = &import.1["data"];
    assert_eq!(receipt["state"], "disabled");
    assert_eq!(receipt["binding"], "provider:hermes-cron");
    assert_eq!(receipt["adopted"], true);
    let routine_ref = receipt["routine"].as_str().unwrap().to_string();

    // The Routine exists, Disabled, with the foreign binding carrying the
    // harness job id.
    let (_, listed, _) = run(home.path(), &["routine", "list"]);
    let routines = listed["data"]["routines"].as_array().unwrap();
    assert_eq!(routines.len(), 1);
    let record = &routines[0];
    assert_eq!(record["routine"], routine_ref);
    assert_eq!(record["state"], "disabled");
    assert_eq!(record["scheduler"]["provider"], "provider:hermes-cron");
    assert_eq!(record["scheduler"]["provider_job_id"], "2e45cb4eef8a");
    assert_eq!(record["foreign_adoption"]["provider"], "hermes-cron");
    assert_eq!(
        record["foreign_adoption"]["provider_job_id"],
        "2e45cb4eef8a"
    );

    // The reconciled job now shows as claimed.
    let (_, listed, _) = run(home.path(), &["routine", "list"]);
    let hermes = listed["data"]["foreign_reconciliation"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "hermes-cron")
        .unwrap()
        .clone();
    let nara = hermes["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["job_id"] == "2e45cb4eef8a")
        .unwrap();
    assert_eq!(nara["reconciled"], true);

    // The harness store is untouched: the import is read-only, and the
    // retirement of the timer is the owner's explicit act in the harness.
    let hermes_store = fs::read_to_string(home.path().join(".hermes/cron/jobs.json")).unwrap();
    assert!(hermes_store.contains("\"2e45cb4eef8a\""));
    assert!(hermes_store.contains("Nara Daily Flow Compose"));
    let openclaw_store = fs::read_to_string(home.path().join(".openclaw/cron/jobs.json")).unwrap();
    assert!(openclaw_store.contains("tmux-completion-watcher"));

    // The gateway dispatcher must not fire the foreign-bound Routine: its
    // binding names hermes-cron, not the gateway.
    let (_, tick, _) = run(home.path(), &["gateway", "tick"]);
    assert!(tick["ok"] == true, "{tick}");
    assert_eq!(tick["data"]["considered"].as_array().unwrap().len(), 0);
}
