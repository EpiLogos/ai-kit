//! Every command is wired, and none of them answer `command.not_implemented`.
//!
//! This is the test that keeps Phase D honest. It drives the **real binary** for
//! every read-only command in the surface and asserts two things: a valid JSON
//! envelope comes back, and the error code is never `command.not_implemented`.
//!
//! Commands that change the world (`apply`, `enable`, `client install`,
//! `mux install`, `promote`) are exercised by their own tests against real
//! fixtures rather than here — running them in a loop would be a test that
//! mutates a machine to prove a string is absent.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with one capsule, and a project that enables nothing.
fn fixture() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let base = home
        .path()
        .join("registries/personal/capsules/script/demo/greet");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "A capsule for the command-surface test."

[script]
entry = "payload/run.sh"
"#,
    );
    write(&base.join("payload/run.sh"), "#!/bin/sh\necho hi\n");
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    (home, project)
}

/// Run `aikit <args> --json` and return (exit_ok, parsed envelope).
fn run(home: &Path, project: &Path, args: &[&str]) -> (bool, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?} should run: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; got stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

/// The read-only surface. Every one of these must be real.
const READ_ONLY: &[&[&str]] = &[
    &["status"],
    &["search", "greet"],
    &["explain", "script/demo/greet"],
    &["diff"],
    &["doctor"],
    &["inbox"],
    &["recent"],
    &["failures"],
    &["stats"],
    &["unused"],
    &["jobs"],
    &["bypasses"],
    &["log", "export"],
    &["context", "current"],
    &["context", "list"],
    &["task", "list"],
    &["capabilities", "list"],
    &["client", "status"],
    &["mux", "detect"],
    &["session", "list"],
    &["init"],
    &["collate"],
    &["z", "greet"],
    &["prune"],
];

#[test]
fn no_command_answers_not_implemented() {
    let (home, project) = fixture();
    let mut checked = 0;

    for args in READ_ONLY {
        let (_ok, envelope) = run(home.path(), project.path(), args);
        assert_eq!(
            envelope["schema"],
            Value::from(1),
            "aikit {args:?} must speak the stable envelope: {envelope}"
        );

        if let Some(error) = envelope.get("error") {
            let code = error["code"].as_str().unwrap_or_default();
            assert_ne!(
                code, "command.not_implemented",
                "aikit {args:?} is still a stub: {envelope}"
            );
        }
        checked += 1;
    }

    assert_eq!(checked, READ_ONLY.len());
}

#[test]
fn every_read_only_command_succeeds_on_a_fresh_machine() {
    // A brand-new home with one capsule is the least interesting possible state,
    // and every read-only command must still answer rather than erroring. This is
    // what catches a command that only works once something has been applied.
    let (home, project) = fixture();

    for args in READ_ONLY {
        let (ok, envelope) = run(home.path(), project.path(), args);
        assert!(ok, "aikit {args:?} failed on a fresh machine: {envelope}");
        assert_eq!(envelope["ok"], Value::Bool(true), "{args:?}: {envelope}");
    }
}

#[test]
fn an_unknown_capability_reports_a_stable_error_code_not_a_stub() {
    let (home, project) = fixture();
    let (ok, envelope) = run(
        home.path(),
        project.path(),
        &["explain", "script/nope/missing"],
    );
    assert!(!ok);
    assert_eq!(
        envelope["error"]["code"], "resolution.unknown_capability",
        "errors carry their documented machine code: {envelope}"
    );
}
