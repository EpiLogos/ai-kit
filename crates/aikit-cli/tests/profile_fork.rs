//! A project fork is a local delta over a base profile, not a copied snapshot.
//!
//! These tests use the real binary and real registry loader. They prove the fork
//! remains attached to its evolving base and that `profile diff` reports only
//! the project's delta.

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

fn fixture() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    script(home.path(), "base/check");
    script(home.path(), "project/extra");
    write(
        &home
            .path()
            .join("registries/personal/profiles/code/rust.toml"),
        "schema = 1\nid = \"profile/code/rust\"\nenable = [\"script/base/check\"]\n",
    );
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    (home, project)
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

#[test]
fn a_profile_fork_is_diff_first_and_writes_only_a_project_delta() {
    let (home, project) = fixture();
    let fork_file = project.path().join(".aikit/profiles/project/rust.toml");

    let preview = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--scope",
            "project",
            "--name",
            "profile/project/rust",
        ],
    );
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    assert_eq!(envelope(&preview)["data"]["applied"], false);
    assert!(!fork_file.exists());
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();

    let unreviewed = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--scope",
            "project",
            "--name",
            "profile/project/rust",
            "--yes",
        ],
    );
    assert!(!unreviewed.status.success());
    assert_eq!(
        envelope(&unreviewed)["error"]["code"],
        "procedure.review_required"
    );
    assert!(!fork_file.exists());

    let applied = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--scope",
            "project",
            "--name",
            "profile/project/rust",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(applied.status.success(), "{:?}", envelope(&applied));
    let body = envelope(&applied);
    assert_eq!(body["data"]["applied"], true);
    assert_eq!(body["data"]["base"], "profile/code/rust");
    assert_eq!(body["data"]["fork"], "profile/project/rust");
    assert!(fork_file.is_file());

    let fork_text = fs::read_to_string(&fork_file).unwrap();
    assert!(fork_text.contains("id = \"profile/project/rust\""));
    assert!(fork_text.contains("extends = [\"profile/code/rust\"]"));
    assert!(
        !fork_text.contains("script/base/check"),
        "the fork stores a delta, not a snapshot of the base"
    );
    let project_text = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert!(project_text.contains("profile/project/rust"));

    let status = run(home.path(), project.path(), &["status"]);
    assert!(status.status.success(), "{:?}", envelope(&status));
    let active = envelope(&status)["data"]["active"].clone();
    assert!(
        active
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "script/base/check"),
        "the fork continues to inherit its base: {active}"
    );
}

#[test]
fn profile_diff_reports_the_project_delta_without_repeating_the_base() {
    let (home, project) = fixture();
    let preview = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--scope",
            "project",
            "--name",
            "profile/project/rust",
        ],
    );
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let applied = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--scope",
            "project",
            "--name",
            "profile/project/rust",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(applied.status.success(), "{:?}", envelope(&applied));

    write(
        &project.path().join(".aikit/profiles/project/rust.toml"),
        r#"schema = 1
id = "profile/project/rust"
description = "Use the extra project check."
extends = ["profile/code/rust"]
enable = ["script/project/extra"]

[config."script/project/extra"]
retries = 3
required = true
"#,
    );

    let diff = run(
        home.path(),
        project.path(),
        &["profile", "diff", "profile/project/rust"],
    );
    assert!(diff.status.success(), "{:?}", envelope(&diff));
    let body = envelope(&diff);
    assert_eq!(body["data"]["base"], "profile/code/rust");
    assert_eq!(
        body["data"]["enable"],
        serde_json::json!(["script/project/extra"])
    );
    assert_eq!(body["data"]["disable"], serde_json::json!([]));
    assert_eq!(
        body["data"]["config"]["script/project/extra"],
        serde_json::json!({"retries": 3, "required": true}),
        "the diff carries the typed authored values, not only a list of keys"
    );
    assert_eq!(body["data"]["reason"], "Use the extra project check.");
    assert!(
        !body["data"]["enable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "script/base/check"),
        "the inherited base is not repeated in the delta"
    );
}

#[test]
fn a_required_base_parameter_must_be_bound_and_remains_active_through_the_fork() {
    let (home, project) = fixture();
    script(home.path(), "tool/cargo-test");
    write(
        &home
            .path()
            .join("registries/personal/profiles/code/runner.toml"),
        r#"schema = 1
id = "profile/code/runner"
enable = ["script/tool/{{runner}}"]

[params.runner]
type = "enum"
choices = ["cargo-test", "nextest"]
"#,
    );

    let missing = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/runner",
            "--name",
            "profile/project/runner",
            "--yes",
        ],
    );
    assert!(!missing.status.success());
    assert_eq!(
        envelope(&missing)["error"]["code"],
        "profile.missing_parameter"
    );
    assert!(!project
        .path()
        .join(".aikit/profiles/project/runner.toml")
        .exists());

    let preview = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/runner",
            "--name",
            "profile/project/runner",
            "--param",
            "runner=cargo-test",
        ],
    );
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let applied = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/runner",
            "--name",
            "profile/project/runner",
            "--param",
            "runner=cargo-test",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(applied.status.success(), "{:?}", envelope(&applied));
    let fork_text =
        fs::read_to_string(project.path().join(".aikit/profiles/project/runner.toml")).unwrap();
    assert!(fork_text.contains("[[extends_use]]"), "{fork_text}");
    assert!(fork_text.contains("runner = \"cargo-test\""), "{fork_text}");

    let status = run(home.path(), project.path(), &["status"]);
    assert!(status.status.success(), "{:?}", envelope(&status));
    assert!(
        envelope(&status)["data"]["active"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "script/tool/cargo-test"),
        "the fork resolves its base with the committed binding"
    );

    let diff = run(
        home.path(),
        project.path(),
        &["profile", "diff", "profile/project/runner"],
    );
    assert!(diff.status.success(), "{:?}", envelope(&diff));
    assert_eq!(
        envelope(&diff)["data"]["base_params"]["runner"],
        "cargo-test",
        "the exact committed parent binding is part of the fork delta"
    );
}

#[test]
fn a_profile_fork_refuses_a_stale_review_when_the_base_changes() {
    let (home, project) = fixture();
    let preview = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--name",
            "profile/project/rust",
        ],
    );
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();

    write(
        &home
            .path()
            .join("registries/personal/profiles/code/rust.toml"),
        "schema = 1\nid = \"profile/code/rust\"\nenable = [\"script/project/extra\"]\n",
    );

    let stale = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--name",
            "profile/project/rust",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(!stale.status.success());
    assert_eq!(
        envelope(&stale)["error"]["code"],
        "procedure.review_mismatch"
    );
    assert!(!project
        .path()
        .join(".aikit/profiles/project/rust.toml")
        .exists());
}

#[test]
fn a_saved_profile_fork_rechecks_the_base_declaration_at_apply_time() {
    let (home, project) = fixture();
    let preview = run(
        home.path(),
        project.path(),
        &[
            "profile",
            "fork",
            "profile/code/rust",
            "--name",
            "profile/project/rust",
        ],
    );
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    let body = envelope(&preview);
    let procedure = body["data"]["procedure"].as_str().unwrap();
    let digest = body["data"]["digest"].as_str().unwrap();

    write(
        &home
            .path()
            .join("registries/personal/profiles/code/rust.toml"),
        "schema = 1\nid = \"profile/code/rust\"\nenable = [\"script/project/extra\"]\n",
    );
    let applied = run(
        home.path(),
        project.path(),
        &["procedure", "run", procedure, "--expect-digest", digest],
    );

    assert!(!applied.status.success());
    assert_eq!(
        envelope(&applied)["error"]["code"],
        "procedure.precondition_failed"
    );
    assert!(!project
        .path()
        .join(".aikit/profiles/project/rust.toml")
        .exists());
}

#[test]
fn cli_fork_bindings_preserve_boolean_integer_and_list_types() {
    let (home, project) = fixture();
    write(
        &home
            .path()
            .join("registries/personal/profiles/code/typed.toml"),
        r#"schema = 1
id = "profile/code/typed"
enable = ["script/base/check"]

[params.strict]
type = "bool"

[params.retries]
type = "integer"
min = 1

[params.features]
type = "multiselect"
choices = ["lint", "audit"]
"#,
    );
    let args = [
        "profile",
        "fork",
        "profile/code/typed",
        "--name",
        "profile/project/typed",
        "--param",
        "strict=true",
        "--param",
        "retries=3",
        "--param",
        "features=lint,audit",
    ];
    let preview = run(home.path(), project.path(), &args);
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let mut apply_args = args.to_vec();
    apply_args.extend(["--yes", "--expect-digest", &digest]);
    let applied = run(home.path(), project.path(), &apply_args);
    assert!(applied.status.success(), "{:?}", envelope(&applied));

    let fork =
        fs::read_to_string(project.path().join(".aikit/profiles/project/typed.toml")).unwrap();
    let document: toml::Value = toml::from_str(&fork).unwrap();
    let params = &document["extends_use"][0]["params"];
    assert_eq!(params["strict"].as_bool(), Some(true), "{fork}");
    assert_eq!(params["retries"].as_integer(), Some(3), "{fork}");
    assert_eq!(
        params["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(toml::Value::as_str)
            .collect::<Vec<_>>(),
        vec!["lint", "audit"],
        "{fork}"
    );

    let diff = run(
        home.path(),
        project.path(),
        &["profile", "diff", "profile/project/typed"],
    );
    assert!(diff.status.success(), "{:?}", envelope(&diff));
    let body = envelope(&diff);
    let params = &body["data"]["base_params"];
    assert_eq!(params["strict"], true);
    assert_eq!(params["retries"], 3);
    assert_eq!(params["features"], serde_json::json!(["lint", "audit"]));
}
