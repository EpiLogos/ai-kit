//! `aikit z` end to end through the real binary.
//!
//! The acceptance criterion is precise: **`z` acts, and never activates.** Running
//! a capability and making it active are different acts; a fuzzy match is consent
//! to the first and never to the second.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A registry with two scripts, neither enabled anywhere.
fn seed(home: &Path) {
    for (id, leaf) in [
        ("script/test/cargo-nextest", "cargo-nextest"),
        ("script/test/cargo-nextest-helper", "cargo-nextest-helper"),
    ] {
        let base = home.join(format!("registries/personal/capsules/{id}"));
        write(
            &base.join("manifest.toml"),
            &format!(
                r#"schema = 1
id = "{id}"
kind = "script"
name = "{leaf}"
description = "Runs tests for the z test."

[script]
entry = "payload/run.sh"
exports = ["{leaf}"]
"#
            ),
        );
        let run = base.join("payload/run.sh");
        write(&run, "#!/bin/sh\necho ran-$0\n");
        let mut perms = fs::metadata(&run).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&run, perms).unwrap();
    }
}

fn z(home: &Path, project: &Path, args: &[&str]) -> Value {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .arg("z")
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(project)
        .output()
        .expect("aikit z runs");
    assert!(
        output.status.success(),
        "z failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("z emits JSON")
}

#[test]
fn z_finds_a_capability_by_a_fragment_of_its_name_and_never_activates_it() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    // A project that declares NOTHING: nothing is active here.
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");

    let envelope = z(home.path(), project.path(), &["nextest"]);
    let data = &envelope["data"];

    assert_eq!(envelope["ok"], Value::Bool(true));
    assert_eq!(data["decision"], "act", "one clear winner: {data}");
    assert_eq!(
        data["capability"], "script/test/cargo-nextest",
        "the exact leaf beats the longer one: {data}"
    );
    assert_eq!(
        data["action"], "run",
        "a script's natural action is to run it"
    );
    assert_eq!(
        data["activated"],
        Value::Bool(false),
        "z proposes running; it never makes anything active"
    );

    // And nothing on disk changed: no profile was written, nothing was enabled.
    let profile = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert_eq!(
        profile.trim(),
        "schema = 1",
        "z must not have written a declaration: {profile}"
    );
}

#[test]
fn z_returns_ranked_candidates_for_an_agent_to_decide_from() {
    // SPEC-III §3.3: an agent gets the same affordance a human does, headless.
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");

    let data = z(home.path(), project.path(), &["cargo"])["data"].clone();
    let candidates = data["candidates"].as_array().expect("candidates array");

    assert!(candidates.len() >= 2, "both matched: {data}");
    let first = candidates[0]["score"].as_f64().unwrap();
    let second = candidates[1]["score"].as_f64().unwrap();
    assert!(first >= second, "candidates arrive ranked best first");
    assert!(
        candidates.iter().all(|c| c["active"] == Value::Bool(false)),
        "nothing was activated by asking"
    );
}

#[test]
fn z_reports_nothing_rather_than_running_something_unrelated() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");

    let data = z(home.path(), project.path(), &["zzzz-no-such-thing"])["data"].clone();
    assert_eq!(data["decision"], "nothing");
    assert!(data["candidates"].as_array().unwrap().is_empty());
}
