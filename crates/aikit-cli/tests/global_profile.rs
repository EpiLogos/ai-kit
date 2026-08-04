//! Persistent User Baseline Profile behavior through the real CLI binary.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

const CAPABILITY: &str = "script/test/global-guidance";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn scene(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let home = root.join("home");
    let first = root.join("first");
    let sibling = root.join("sibling");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    write(&first.join(".aikit/profile.toml"), "schema = 1\n");
    write(&sibling.join(".aikit/profile.toml"), "schema = 1\n");
    write(
        &home.join("registries/personal/capsules/script/test/global-guidance/manifest.toml"),
        r#"schema = 1
id = "script/test/global-guidance"
kind = "script"
name = "global-guidance"
description = "A real script used to prove global profile persistence."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
"#,
    );
    write(
        &home.join("registries/personal/capsules/script/test/global-guidance/payload/run.sh"),
        "#!/bin/sh\necho global\n",
    );
    (home, first, sibling)
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    (output, value)
}

fn active(home: &Path, cwd: &Path) -> bool {
    let (output, value) = run(home, cwd, &["status", "--all"]);
    assert!(output.status.success(), "status failed: {value}");
    value["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == CAPABILITY)
}

#[test]
fn a_user_baseline_persists_and_a_project_can_override_it() {
    let temp = tempfile::tempdir().unwrap();
    let (home, first, sibling) = scene(temp.path());

    let (enabled, reply) = run(
        &home,
        temp.path(),
        &["enable", CAPABILITY, "--scope", "user"],
    );
    assert!(enabled.status.success(), "global enable failed: {reply}");
    assert_eq!(reply["data"]["scope"], "global");
    assert!(home.join("scopes/global/profile.toml").is_file());

    assert!(active(&home, &first), "a fresh process must load the baseline");
    assert!(active(&home, &sibling));

    let (disabled, reply) = run(
        &home,
        &first,
        &["disable", CAPABILITY, "--scope", "project"],
    );
    assert!(disabled.status.success(), "project disable failed: {reply}");
    assert!(!active(&home, &first));
    assert!(
        active(&home, &sibling),
        "one project's higher-precedence override must not change its sibling"
    );

    let (_output, explanation) = run(&home, &sibling, &["explain", CAPABILITY]);
    let selected = explanation["data"]["selected_by"].as_array().unwrap();
    assert!(selected
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("scopes/global/profile.toml")));
}

#[test]
fn a_named_profile_can_be_used_from_the_user_baseline() {
    let temp = tempfile::tempdir().unwrap();
    let (home, first, _) = scene(temp.path());
    write(
        &home.join("registries/personal/profiles/personal/foundation.toml"),
        &format!(
            "schema = 1\nid = \"profile/personal/foundation\"\nenable = [\"{CAPABILITY}\"]\n"
        ),
    );

    let (used, reply) = run(
        &home,
        temp.path(),
        &["use", "profile/personal/foundation", "--scope", "global"],
    );
    assert!(used.status.success(), "global profile use failed: {reply}");
    assert!(active(&home, &first));
}
