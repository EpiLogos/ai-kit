//! Durable Procedure CLI: plan, inspect and run are separate real invocations.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
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

fn body(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}); stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn fixture() -> (TempDir, TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let foreign = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    write(
        &foreign.path().join("review/SKILL.md"),
        "---\nname: review\ndescription: Review carefully.\n---\n",
    );
    (home, project, foreign)
}

#[test]
fn a_saved_adoption_plan_can_be_diffed_and_applied_by_exact_id_and_digest() {
    let (home, project, foreign) = fixture();
    let planned = run(
        home.path(),
        project.path(),
        &[
            "procedure",
            "plan",
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "durable",
        ],
    );
    assert!(planned.status.success(), "{:?}", body(&planned));
    let planned_body = body(&planned);
    let id = planned_body["data"]["procedure"].as_str().unwrap();
    let digest = planned_body["data"]["digest"].as_str().unwrap();
    assert!(!foreign.path().join("review/SKILL.md").is_symlink());

    let diff = run(home.path(), project.path(), &["procedure", "diff", id]);
    assert!(diff.status.success(), "{:?}", body(&diff));
    assert_eq!(body(&diff)["data"]["digest"], digest);

    let applied = run(
        home.path(),
        project.path(),
        &["procedure", "run", id, "--expect-digest", digest],
    );
    assert!(applied.status.success(), "{:?}", body(&applied));
    assert!(foreign.path().join("review/SKILL.md").is_symlink());
    assert!(home
        .path()
        .join("registries/personal/capsules/skill/durable/review/manifest.toml")
        .is_file());
}

#[test]
fn a_saved_plan_refuses_source_drift_and_a_wrong_digest() {
    let (home, project, foreign) = fixture();
    let planned = run(
        home.path(),
        project.path(),
        &[
            "procedure",
            "plan",
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "durable",
        ],
    );
    let planned_body = body(&planned);
    let id = planned_body["data"]["procedure"].as_str().unwrap();
    let digest = planned_body["data"]["digest"].as_str().unwrap();

    let wrong = run(
        home.path(),
        project.path(),
        &["procedure", "run", id, "--expect-digest", "wrong"],
    );
    assert!(!wrong.status.success());
    assert_eq!(body(&wrong)["error"]["code"], "procedure.review_mismatch");

    write(
        &foreign.path().join("arrived-after-review/SKILL.md"),
        "---\nname: arrived-after-review\ndescription: Added concurrently.\n---\n",
    );
    let drifted = run(
        home.path(),
        project.path(),
        &["procedure", "run", id, "--expect-digest", digest],
    );
    assert!(!drifted.status.success());
    assert_eq!(
        body(&drifted)["error"]["code"],
        "procedure.precondition_failed"
    );
    assert!(!foreign.path().join("review/SKILL.md").is_symlink());
}
