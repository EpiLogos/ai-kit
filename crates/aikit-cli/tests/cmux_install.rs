//! The cmux integration is exercised through the real AIKit binary and real
//! filesystem Procedures. The only thing not required here is a running cmux GUI:
//! cmux 0.63 watches its global command file, while newer builds can additionally
//! expose richer action bindings.

use std::fs;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

fn run(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home.join(".aikit-state"))
        .env("HOME", home)
        .current_dir(home)
        .output()
        .expect("the real aikit binary runs")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not JSON ({error}): {:?}; stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn install(home: &std::path::Path, extra: &[&str]) -> Output {
    let mut args = vec!["--json", "mux", "install", "cmux"];
    args.extend_from_slice(extra);
    run(home, &args)
}

#[test]
fn cmux_installs_a_real_global_command_in_the_canonical_json_file() {
    let home = tempfile::tempdir().unwrap();
    let output = install(home.path(), &[]);
    assert!(
        output.status.success(),
        "install failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let reply = json(&output);
    assert_eq!(reply["data"]["mux"], "cmux");
    assert_eq!(
        reply["data"]["path"],
        home.path()
            .join(".config/cmux/cmux.json")
            .display()
            .to_string()
    );
    assert_eq!(reply["data"]["verified"], true);
    assert_eq!(reply["data"]["binding"], "command-palette:AIKit");

    let config: Value =
        serde_json::from_slice(&fs::read(home.path().join(".config/cmux/cmux.json")).unwrap())
            .unwrap();
    assert_eq!(
        config["commands"][0],
        serde_json::json!({
            "name": "AIKit",
            "description": "Open AIKit's unified palette and tree",
            "keywords": ["aikit", "skills", "capabilities"],
            "command": "aikit ui",
            "confirm": false
        })
    );
}

#[test]
fn cmux_merge_preserves_unrelated_json_and_jsonc_comments_byte_for_byte() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".config/cmux/cmux.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = "{\n  // keep this explanation\n  \"schemaVersion\": 1,\n  \"terminal\": { \"autoResumeAgentSessions\": false },\n  \"commands\": [\n    { \"name\": \"Tests\", \"command\": \"cargo test\", \"confirm\": true },\n  ],\n}\n";
    fs::write(&path, original).unwrap();

    let output = install(home.path(), &[]);
    assert!(
        output.status.success(),
        "JSONC merge failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let changed = fs::read_to_string(&path).unwrap();
    assert!(changed.contains("// keep this explanation"));
    assert!(changed.contains("\"terminal\": { \"autoResumeAgentSessions\": false }"));
    assert!(
        changed.contains("{ \"name\": \"Tests\", \"command\": \"cargo test\", \"confirm\": true }")
    );
    assert_eq!(changed.matches("// keep this explanation").count(), 1);
    assert_eq!(changed.matches("\"name\": \"AIKit\"").count(), 1);
}

#[test]
fn cmux_install_is_idempotent_without_reformatting_the_file() {
    let home = tempfile::tempdir().unwrap();
    let first = install(home.path(), &[]);
    assert!(first.status.success());
    let path = home.path().join(".config/cmux/cmux.json");
    let before = fs::read(&path).unwrap();

    let second = install(home.path(), &[]);
    assert!(
        second.status.success(),
        "reinstall failed: stdout={} stderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(json(&second)["data"]["edits"], 0);
}

#[test]
fn cmux_refuses_a_foreign_aikit_command_unless_replacement_is_explicit() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".config/cmux/cmux.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original =
        "{\n  \"commands\": [{ \"name\": \"AIKit\", \"command\": \"dangerous-other-command\" }]\n}\n";
    fs::write(&path, original).unwrap();

    let refused = install(home.path(), &[]);
    assert!(!refused.status.success());
    let error = json(&refused);
    assert_eq!(error["error"]["code"], "mux.key_conflict");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let replaced = install(home.path(), &["--replace-key"]);
    assert!(
        replaced.status.success(),
        "explicit replacement failed: stdout={} stderr={}",
        String::from_utf8_lossy(&replaced.stdout),
        String::from_utf8_lossy(&replaced.stderr)
    );
    let changed = fs::read_to_string(path).unwrap();
    assert!(!changed.contains("dangerous-other-command"));
    assert_eq!(changed.matches("\"name\": \"AIKit\"").count(), 1);
    assert_eq!(changed.matches("\"command\": \"aikit ui\"").count(), 1);
}

#[test]
fn cmux_refuses_duplicate_command_authorities_instead_of_verifying_the_wrong_one() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".config/cmux/cmux.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let duplicate_keys = r#"{
  "commands": [{ "name": "AIKit", "command": "aikit ui" }],
  "commands": [{ "name": "Other", "command": "echo other" }]
}
"#;
    fs::write(&path, duplicate_keys).unwrap();

    let refused = install(home.path(), &[]);
    assert!(!refused.status.success());
    assert_eq!(json(&refused)["error"]["code"], "mux.cmux_config_invalid");
    assert_eq!(fs::read_to_string(&path).unwrap(), duplicate_keys);

    let duplicate_entries = r#"{
  "commands": [
    { "name": "AIKit", "command": "aikit ui" },
    { "name": "AIKit", "command": "something else" }
  ]
}
"#;
    fs::write(&path, duplicate_entries).unwrap();
    let refused = install(home.path(), &["--replace-key"]);
    assert!(!refused.status.success());
    assert_eq!(json(&refused)["error"]["code"], "mux.cmux_config_invalid");
    assert_eq!(fs::read_to_string(&path).unwrap(), duplicate_entries);

    let duplicate_inner_command = r#"{
  "commands": [
    { "name": "AIKit", "command": "aikit ui", "command": "dangerous" }
  ]
}
"#;
    fs::write(&path, duplicate_inner_command).unwrap();
    let refused = install(home.path(), &[]);
    assert!(!refused.status.success());
    assert_eq!(json(&refused)["error"]["code"], "mux.cmux_config_invalid");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        duplicate_inner_command
    );
}

#[test]
fn cmux_procedure_undo_restores_the_exact_original_bytes() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".config/cmux/cmux.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original =
        b"{\n\t\"commands\": [ { \"name\": \"Mine\", \"command\": \"echo mine\" } ]\n}\n";
    fs::write(&path, original).unwrap();

    let installed = install(home.path(), &[]);
    assert!(installed.status.success());
    let procedure = json(&installed)["data"]["procedure"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(fs::read(&path).unwrap(), original);

    let undone = run(home.path(), &["--json", "procedure", "undo", &procedure]);
    assert!(
        undone.status.success(),
        "undo failed: stdout={} stderr={}",
        String::from_utf8_lossy(&undone.stdout),
        String::from_utf8_lossy(&undone.stderr)
    );
    assert_eq!(fs::read(path).unwrap(), original);
}
