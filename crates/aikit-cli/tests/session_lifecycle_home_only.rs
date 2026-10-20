//! `aikit session lifecycle …` works from the AIKit home and the context descriptor, without building the Service.
//!
//! The lifecycle store is built from the home alone, and a reply only says where the call stood (its context descriptor). Building the
//! Service for it hashed every capsule of the registry, resolved trust and the view and opened the sqlite index — most of the ~140 ms
//! each `session lifecycle history` cost the O:I kernel, ten times in the first seconds after launch (r28 trace, 20 Oct), and a
//! process per hook turn. The proof is a home whose index cannot be opened: the Service cannot be built over it, so a verb that
//! answers there did not build one. The reply still carries the same context the Service-built path gave it.
use std::path::Path;

use serde_json::Value;

fn call(home: &Path, project: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aikit"))
        .arg("-C")
        .arg(project)
        .args(args)
        .env("AIKIT_HOME", home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {stdout:?}"));
    (output, value)
}

/// A home whose index file is not a database, and a project directory with a profile.
fn unopenable_index() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(home.join("state")).unwrap();
    std::fs::create_dir_all(project.join(".aikit")).unwrap();
    std::fs::write(project.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::write(home.join("state/aikit.sqlite3"), [0x5a_u8; 4096]).unwrap();
    (temp, home, project)
}

#[test]
fn the_session_lifecycle_runs_without_the_service() {
    let (_temp, home, project) = unopenable_index();
    let session = "ses_homeonly";

    let (output, started) = call(
        &home,
        &project,
        &["session", "lifecycle", "start", session, "--json"],
    );
    assert!(output.status.success(), "{started}");
    assert_eq!(started["data"]["kind"], "session-started");

    let (output, thinking) = call(
        &home,
        &project,
        &[
            "session",
            "lifecycle",
            "thinking",
            session,
            "--state",
            "started",
            "--json",
        ],
    );
    assert!(output.status.success(), "{thinking}");

    let (output, history) = call(
        &home,
        &project,
        &["session", "lifecycle", "history", session, "--json"],
    );
    assert!(output.status.success(), "{history}");
    assert_eq!(history["data"]["count"], 2);
    assert_eq!(history["data"]["events"][0]["kind"], "session-started");

    let (output, listed) = call(&home, &project, &["session", "lifecycle", "list", "--json"]);
    assert!(output.status.success(), "{listed}");
    assert_eq!(listed["data"]["sessions"], serde_json::json!([session]));

    let (output, model) = call(
        &home,
        &project,
        &["session", "lifecycle", "show", session, "--json"],
    );
    assert!(output.status.success(), "{model}");

    // The reply says where the call stood, as it did when the Service built it.
    let context = &history["context"];
    assert!(
        context["context_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("ctx_")),
        "{history}"
    );
    assert_eq!(
        Path::new(context["project_root"].as_str().expect("a project root"))
            .canonicalize()
            .unwrap(),
        project.canonicalize().unwrap(),
        "{history}"
    );
}

#[test]
fn the_other_session_verbs_still_build_the_service() {
    let (_temp, home, project) = unopenable_index();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aikit"))
        .arg("-C")
        .arg(&project)
        .args(["--json", "session", "list"])
        .env("AIKIT_HOME", &home)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "session list reads the Service's index and must fail over an unopenable one"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("index.open_failed")
            || String::from_utf8_lossy(&output.stderr).contains("index.open_failed"),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
