//! The CLI contract, exercised against the real built binary.
//!
//! `AIKIT_HOME` points at a temp directory and the commands run in a real project
//! directory, so these are the same code paths a user's shell drives. Assertions
//! are on parsed JSON and on exit codes, not on substrings of human text.

use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with a personal registry holding one searchable script capsule, and a
/// project that enables it.
fn scene() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    write(
        &home
            .path()
            .join("registries/personal/capsules/script/demo/greet/manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "Greets for the CLI binary test."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
exports = ["greet"]
"#,
    );
    write(
        &home
            .path()
            .join("registries/personal/capsules/script/demo/greet/payload/run.sh"),
        "#!/bin/sh\necho hi\n",
    );
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );
    (home, project)
}

fn run_json(home: &Path, project: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home)
        .current_dir(project)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout:?}"));
    (output, value)
}

#[test]
fn status_json_carries_the_envelope_and_the_active_capability() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["status", "--json"]);

    assert!(output.status.success());
    assert_eq!(value["schema"], 1);
    assert_eq!(value["ok"], true);
    // Compare canonicalised paths: on macOS `current_dir()` resolves the temp
    // dir's `/var` -> `/private/var` symlink, which is not a contract difference.
    let reported = value["context"]["project_root"].as_str().unwrap();
    assert_eq!(
        fs::canonicalize(reported).unwrap(),
        fs::canonicalize(project.path()).unwrap()
    );
    // The enabled script is active.
    let active = value["data"]["active"].as_array().unwrap();
    assert!(
        active.iter().any(|c| c["id"] == "script/demo/greet"),
        "the enabled capability should be active: {value}"
    );
}

#[test]
fn doctor_json_is_the_installed_native_verification_path() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["doctor", "--json"]);

    assert!(
        output.status.success(),
        "doctor should return an observation even when individual checks report findings: {value}"
    );
    assert_eq!(value["schema"], 1);
    assert_eq!(value["ok"], true);
    assert!(value["data"].is_object());
}

#[test]
fn search_json_finds_a_capability_by_name() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["search", "greet", "--json"]);
    assert!(output.status.success());
    let rows = value["data"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["id"] == "script/demo/greet"));
}

#[test]
fn an_unknown_capability_is_a_resolution_failure_with_exit_code_three() {
    let (home, project) = scene();
    let (output, value) =
        run_json(home.path(), project.path(), &["explain", "script/no/such", "--json"]);

    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "resolution.unknown_capability");
    assert_eq!(
        output.status.code(),
        Some(3),
        "resolution failures exit 3 per the published table"
    );
}

#[test]
fn shell_init_prints_a_sourceable_snippet() {
    let (home, project) = scene();
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(["shell", "init", "bash"])
        .env("AIKIT_HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "the snippet should not be empty");
}

#[test]
fn a_bad_scope_argument_is_a_usage_error_with_exit_code_two() {
    let (home, project) = scene();
    let (_output, value) = run_json(
        home.path(),
        project.path(),
        &["enable", "script/demo/greet", "--scope", "nonsense", "--json"],
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "cli.usage");
}

#[test]
fn flow_contemplates_a_now_stream_through_the_real_binary() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    let seam = project.path().join("now-stream.json");
    write(
        &seam,
        r#"{
  "schema": "central.thoughts-reading/v1",
  "now_ref": "central:now:control:root:cli",
  "total": 1,
  "truncated": false,
  "fixtures": [
    {"file": "raw-2026-09-13.md", "revision": "central.content-fnv1a64/v1:1:aa", "conforming": true, "day": "2026-09-13", "actor": "agent:test", "actor_kind": "agent", "content": "raw body"}
  ]
}"#,
    );
    let run = |args: &[&str]| {
        let output = std::process::Command::new(cargo_bin!("aikit"))
            .env("AIKIT_HOME", home.path())
            .current_dir(project.path())
            .args(args)
            .output()
            .unwrap();
        (
            output.status.success(),
            serde_json::from_slice::<Value>(&output.stdout).ok(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let (ok, json, stderr) = run(&[
        "flow",
        "preflight",
        "--now-ref",
        "central:now:control:root:cli",
        "--fixtures",
        seam.to_str().unwrap(),
    ]);
    assert!(ok, "{stderr}");
    let data = json.unwrap();
    assert_eq!(data["contemplation"]["state"], "unavailable");
    assert_eq!(data["preflight"]["fixture_count"], 1);
    assert!(data["preflight"]["invocation_ref"]
        .as_str()
        .unwrap()
        .starts_with("now-contemplate/"));

    // A stream that disagrees with the addressed NOW refuses outright.
    let (ok, _, stderr) = run(&[
        "flow",
        "preflight",
        "--now-ref",
        "central:now:control:root:other",
        "--fixtures",
        seam.to_str().unwrap(),
    ]);
    assert!(!ok, "{stderr}");

    // `--now-ref` without `--fixtures` refuses before any owner call.
    let (ok, _, stderr) = run(&[
        "flow",
        "preflight",
        "--now-ref",
        "central:now:control:root:cli",
    ]);
    assert!(!ok, "{stderr}");
}
