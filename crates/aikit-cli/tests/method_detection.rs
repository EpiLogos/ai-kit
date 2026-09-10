//! Methods are skills whose description carries the `METHOD:` prefix
//! (`aikit_core::method::METHOD_DESCRIPTION_PREFIX`). These tests drive the
//! real binary: detection is prefix-only, the payload is surfaced, the
//! effective state (active/declared) is reported, and non-method skills
//! never appear.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with one Method capsule, one ordinary skill, and a project.
fn fixture() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let base = home
        .path()
        .join("registries/personal/capsules/script/wiki-inhabitation/inhabit");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "script/wiki-inhabitation/inhabit"
kind = "script"
name = "inhabit"
description = "METHOD: inhabit a project wiki from Control state — generate the agent cognition relative to the live human project docs."

[script]
entry = "payload/run.sh"
"#,
    );
    write(&base.join("payload/run.sh"), "#!/bin/sh\necho situating\n");
    let plain = home
        .path()
        .join("registries/personal/capsules/script/demo/greet");
    write(
        &plain.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "A plain skill: no METHOD here."

[script]
entry = "payload/run.sh"
"#,
    );
    write(&plain.join("payload/run.sh"), "#!/bin/sh\necho hi\n");
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    (home, project)
}

fn run(home: &Path, project: &Path, args: &[&str]) -> Value {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?} should run: {e}"));
    assert!(
        output.status.success(),
        "aikit {args:?} failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("aikit method list must emit a JSON envelope")
}

#[test]
fn method_list_detects_prefixed_skills_and_only_prefixed_skills() {
    let (home, project) = fixture();
    let envelope = run(home.path(), project.path(), &["method", "list"]);

    assert_eq!(envelope["data"]["method_prefix"], "METHOD:");
    assert_eq!(envelope["data"]["count"], 1, "only the prefixed skill is a Method");
    let method = &envelope["data"]["methods"][0];
    assert_eq!(method["id"], "script/wiki-inhabitation/inhabit");
    assert_eq!(method["name"], "inhabit");
    assert!(
        method["payload"]
            .as_str()
            .unwrap()
            .starts_with("inhabit a project wiki"),
        "payload should carry the text after the prefix, got {method}"
    );
}

#[test]
fn method_list_filter_matches_name_and_payload() {
    let (home, project) = fixture();

    let hit = run(home.path(), project.path(), &["method", "list", "inhabit"]);
    assert_eq!(hit["data"]["count"], 1);

    let payload_hit = run(home.path(), project.path(), &["method", "list", "cognition"]);
    assert_eq!(payload_hit["data"]["count"], 1, "filter reaches the payload");

    let miss = run(home.path(), project.path(), &["method", "list", "greet"]);
    assert_eq!(miss["data"]["count"], 0, "an ordinary skill is not a Method");
}
