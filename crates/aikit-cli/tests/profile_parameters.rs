//! Typed profile parameters are bound by a project before ordinary resolution.
//!
//! The binary loads real profile and capsule files here. The assertions are on
//! the effective view, so a parser that merely accepts the syntax without
//! changing selection cannot satisfy them.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn script(home: &Path, id_path: &str) {
    let dir = home
        .join("registries/personal/capsules/script")
        .join(id_path);
    let id = format!("script/{id_path}");
    write(
        &dir.join("manifest.toml"),
        &format!(
            "schema = 1\nid = \"{id}\"\nkind = \"script\"\nname = \"{leaf}\"\n\
             description = \"Run {leaf}.\"\n\n[script]\nentry = \"payload/run.sh\"\n",
            leaf = id_path.rsplit('/').next().unwrap()
        ),
    );
    write(&dir.join("payload/run.sh"), "#!/bin/sh\nexit 0\n");
}

fn fixture() -> (TempDir, TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let default_project = TempDir::new().unwrap();
    let bound_project = TempDir::new().unwrap();
    script(home.path(), "test/cargo-test");
    script(home.path(), "test/cargo-nextest");
    write(
        &home
            .path()
            .join("registries/personal/profiles/code/rust.toml"),
        r#"schema = 1
id = "profile/code/rust"
description = "Rust toolchain lens."
enable = ["script/test/{{test_runner}}"]

[params.test_runner]
type = "enum"
choices = ["cargo-test", "cargo-nextest"]
default = "cargo-nextest"
"#,
    );
    write(
        &default_project.path().join(".aikit/profile.toml"),
        "schema = 1\nprofiles = [\"profile/code/rust\"]\n",
    );
    write(
        &bound_project.path().join(".aikit/profile.toml"),
        r#"schema = 1

[[use]]
profile = "profile/code/rust"
params = { test_runner = "cargo-test" }
"#,
    );
    (home, default_project, bound_project)
}

fn run(home: &Path, project: &Path, args: &[&str]) -> Output {
    Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap()
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON ({error}); stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn active_ids(body: &Value) -> Vec<&str> {
    body["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect()
}

#[test]
fn a_default_and_an_explicit_binding_resolve_to_explicit_capsule_ids() {
    let (home, default_project, bound_project) = fixture();

    let default = run(home.path(), default_project.path(), &["status"]);
    assert!(default.status.success(), "{:?}", envelope(&default));
    let default_body = envelope(&default);
    assert_eq!(
        active_ids(&default_body),
        vec!["script/test/cargo-nextest"]
    );

    let bound = run(home.path(), bound_project.path(), &["status"]);
    assert!(bound.status.success(), "{:?}", envelope(&bound));
    let bound_body = envelope(&bound);
    assert_eq!(active_ids(&bound_body), vec!["script/test/cargo-test"]);
}

#[test]
fn an_invalid_binding_is_rejected_before_layering_with_a_stable_error() {
    let (home, _default_project, bound_project) = fixture();
    write(
        &bound_project.path().join(".aikit/profile.toml"),
        r#"schema = 1

[[use]]
profile = "profile/code/rust"
params = { test_runner = "made-up-runner" }
"#,
    );

    let output = run(home.path(), bound_project.path(), &["status"]);
    assert!(!output.status.success());
    let body = envelope(&output);
    assert_eq!(body["error"]["code"], "profile.invalid_parameter");
    assert_eq!(body["error"]["details"]["parameter"], "test_runner");
    assert_eq!(body["error"]["details"]["profile"], "profile/code/rust");
}

#[test]
fn a_committed_profile_binding_cannot_declare_a_secret_parameter() {
    let (home, _default_project, bound_project) = fixture();
    write(
        &home
            .path()
            .join("registries/personal/profiles/code/secret.toml"),
        r#"schema = 1
id = "profile/code/secret"
enable = ["script/test/cargo-test"]

[params.token]
type = "secret"
"#,
    );
    write(
        &bound_project.path().join(".aikit/profile.toml"),
        r#"schema = 1

[[use]]
profile = "profile/code/secret"
params = { token = "must-not-be-committed" }
"#,
    );

    let output = run(home.path(), bound_project.path(), &["status"]);
    assert!(!output.status.success());
    let body = envelope(&output);
    assert_eq!(body["error"]["code"], "profile.secret_parameter_forbidden");
    assert!(
        !body.to_string().contains("must-not-be-committed"),
        "the rejected value must not be reflected into diagnostics: {body}"
    );
}
