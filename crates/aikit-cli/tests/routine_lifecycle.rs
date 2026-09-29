//! The Routine lifecycle through the real CLI in an isolated AIKIT_HOME
//! (parent Acceptance): proof gate, authoring, Draft→enable requiring granted
//! plus unattended authority, run-now, reprove returning to Disabled, and
//! delete refusing while Enabled.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with one Method-classified skill capsule in the personal registry.
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

fn ok(home: &Path, args: &[&str]) -> Value {
    let (success, envelope, stdout) = run(home, args);
    assert!(
        success,
        "aikit {args:?} failed: {stdout} {:?}",
        envelope["error"]
    );
    envelope["data"].clone()
}

fn refused(home: &Path, args: &[&str], code: &str) -> String {
    let (success, envelope, _) = run(home, args);
    assert!(
        !success,
        "aikit {args:?} should have been refused with {code}"
    );
    assert_eq!(envelope["error"]["code"], code, "{envelope}");
    envelope["error"]["message"].as_str().unwrap().to_string()
}

fn proof_json(home: &Path, verification_passed: bool) -> String {
    let path = home.join("proof.json");
    write(
        &path,
        &serde_json::to_string(&json!({
            "proof_ref": "proof:demo:v1",
            "context_resolution_ref": "context-resolution:demo",
            "activity_refs": ["activity:demo:1"],
            "return_refs": ["return:demo:1"],
            "evidence_refs": ["evidence:demo:1"],
            "verification_refs": ["verification:demo:1"],
            "invocation_succeeded": true,
            "verification_passed": verification_passed,
        }))
        .unwrap(),
    );
    format!("@{}", path.display())
}

fn schedule_json(home: &Path) -> String {
    let path = home.join("trigger.json");
    write(
        &path,
        &serde_json::to_string(&json!({
            "schema": "aikit.time-schedule/v1",
            "schedule_ref": "schedule/daily-demo",
            "schedule": { "kind": "daily", "time": "06:00" }
        }))
        .unwrap(),
    );
    format!("@{}", path.display())
}

fn authority_json(home: &Path, granted: bool, unattended: bool) -> String {
    let path = home.join(format!("authority-{granted}-{unattended}.json"));
    write(
        &path,
        &serde_json::to_string(&json!({
            "authority_ref": "authority:routine:demo",
            "revision": "authority-rev-1",
            "action_refs": ["action/capability/run"],
            "granted": granted,
            "unattended": unattended,
        }))
        .unwrap(),
    );
    format!("@{}", path.display())
}

const PROVE: [&str; 4] = ["method", "prove", "--method", "skill/method/demo"];

#[test]
fn the_full_lifecycle_runs_through_the_real_cli() {
    let home = fixture();

    // Prove: no verification, no proof.
    let unverified = proof_json(home.path(), false);
    let mut args: Vec<&str> = PROVE.to_vec();
    args.extend(["--proof-json", &unverified]);
    refused(home.path(), &args, "routine.verification_required");

    // Prove with verification: a ProvenMethodBasis at the capsule's exact
    // revision.
    let verified = proof_json(home.path(), true);
    let mut args: Vec<&str> = PROVE.to_vec();
    args.extend(["--proof-json", &verified]);
    let basis = ok(home.path(), &args);
    assert_eq!(basis["version"], "aikit.method-proof/v1");
    assert_eq!(basis["method"], "skill/method/demo");
    let basis_file = home.path().join("basis.json");
    write(&basis_file, &serde_json::to_string_pretty(&basis).unwrap());
    let basis_ref = format!("@{}", basis_file.display());

    // Create without proof is refused.
    let args = [
        "routine",
        "create",
        "--name",
        "Daily demo",
        "--method",
        "skill/method/demo",
        "--proof-json",
        "@/nonexistent/proof.json",
        "--trigger-json",
        &schedule_json(home.path()),
        "--authority-json",
        &authority_json(home.path(), true, true),
    ];
    let (success, envelope, _) = run(home.path(), &args);
    assert!(!success);
    assert_eq!(envelope["error"]["code"], "cli.structured_json_unreadable");

    // Create: the Routine sits in Draft, bound to the gateway dispatcher.
    let created = ok(
        home.path(),
        &[
            "routine",
            "create",
            "--name",
            "Daily demo",
            "--method",
            "skill/method/demo",
            "--proof-json",
            &basis_ref,
            "--trigger-json",
            &schedule_json(home.path()),
            "--authority-json",
            &authority_json(home.path(), true, true),
        ],
    );
    assert_eq!(created["state"], "draft");
    let routine_ref = created["routine"].as_str().unwrap().to_string();

    // A second Routine with the same name is refused.
    let args = [
        "routine",
        "create",
        "--name",
        "Daily demo",
        "--method",
        "skill/method/demo",
        "--proof-json",
        &basis_ref,
        "--trigger-json",
        &schedule_json(home.path()),
        "--authority-json",
        &authority_json(home.path(), true, true),
    ];
    refused(home.path(), &args, "routine.already_exists");

    // list shows it, with the foreign reconciliation face.
    let listed = ok(home.path(), &["routine", "list"]);
    assert_eq!(listed["routines"].as_array().unwrap().len(), 1);
    assert_eq!(listed["routines"][0]["state"], "draft");
    assert_eq!(
        listed["routines"][0]["scheduler"]["provider"],
        "provider:aikit-gateway"
    );
    assert!(listed["foreign_reconciliation"]["providers"].is_array());

    // Enable without granted authority is refused.
    let args = [
        "routine",
        "enable",
        &routine_ref,
        "--authority-json",
        &authority_json(home.path(), false, true),
    ];
    refused(home.path(), &args, "routine.authority_not_granted");

    // An unattended (schedule) trigger without unattended authority is refused.
    let args = [
        "routine",
        "enable",
        &routine_ref,
        "--authority-json",
        &authority_json(home.path(), true, false),
    ];
    refused(home.path(), &args, "routine.unattended_not_authorised");

    // Enable with a full receipt.
    let enabled = ok(
        home.path(),
        &[
            "routine",
            "enable",
            &routine_ref,
            "--authority-json",
            &authority_json(home.path(), true, true),
        ],
    );
    assert_eq!(enabled["state"], "enabled");

    // run-now: admitted and dispatched through the same gate. The fixture home
    // has no resident encounter owner, so the run is honestly unreturned —
    // and the invocation plus its outcome are in the ledger.
    let dispatched = ok(home.path(), &["routine", "run-now", &routine_ref]);
    assert_eq!(dispatched["admission"], "applied");
    let invocations = ok(home.path(), &["routine", "invocations"]);
    assert_eq!(invocations.as_array().unwrap().len(), 1);
    // A manual run-now carries no scheduled-occurrence delivery, so the
    // ledger holds exactly the run-outcome delivery.
    let deliveries = invocations[0]["provider_deliveries"].as_array().unwrap();
    assert_eq!(deliveries.len(), 1);

    // Delete while Enabled is refused.
    let args = ["routine", "delete", &routine_ref];
    refused(home.path(), &args, "routine.delete_enabled");

    // Reprove returns the Routine to Disabled (never silently resumed).
    let reproved = ok(
        home.path(),
        &[
            "routine",
            "reprove",
            &routine_ref,
            "--proof-json",
            &basis_ref,
        ],
    );
    assert_eq!(reproved["state"], "disabled");

    // Delete is now possible.
    let deleted = ok(home.path(), &["routine", "delete", &routine_ref]);
    assert_eq!(deleted["deleted"], true);
    let listed = ok(home.path(), &["routine", "list"]);
    assert_eq!(listed["routines"].as_array().unwrap().len(), 0);
}

#[test]
fn show_reports_the_stored_record_and_honest_time_ground() {
    let home = fixture();

    let verified = proof_json(home.path(), true);
    let mut args: Vec<&str> = PROVE.to_vec();
    args.extend(["--proof-json", &verified]);
    let basis = ok(home.path(), &args);
    let basis_file = home.path().join("basis.json");
    write(&basis_file, &serde_json::to_string_pretty(&basis).unwrap());
    let basis_ref = format!("@{}", basis_file.display());

    ok(
        home.path(),
        &[
            "routine",
            "create",
            "--name",
            "Shown demo",
            "--method",
            "skill/method/demo",
            "--proof-json",
            &basis_ref,
            "--trigger-json",
            &schedule_json(home.path()),
            "--authority-json",
            &authority_json(home.path(), true, true),
        ],
    );

    let shown = ok(home.path(), &["routine", "show", "routine/shown-demo"]);
    assert_eq!(shown["state"], "draft");
    assert_eq!(
        shown["authority"]["authority_ref"],
        "authority:routine:demo"
    );
    assert!(shown["time_schedule"].is_object());
    // Without a reachable Central root the next occurrences are an honest
    // error field, never an invented schedule.
    if shown.get("occurrence_error").is_none() {
        assert!(shown["next_occurrences"].is_object() || shown["next_occurrences"].is_array());
    }
}
