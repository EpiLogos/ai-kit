//! `aikit status` speaks both dialects: a readable summary for the person at
//! the terminal, and the exact JSON envelope machines already script against.
//!
//! The audit that produced this test found plain `aikit status` printing raw
//! JSON — the "human text" the `--json` flag's help contrasted against simply
//! did not exist for this command.

use std::fs;
use std::path::Path;

fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::TempDir::new().unwrap();
    let project = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(project.path().join(".aikit")).unwrap();
    fs::write(project.path().join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    (home, project)
}

fn run(home: &Path, project: &Path, json: bool) -> std::process::Output {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let mut command = std::process::Command::new(&bin);
    command
        .args(["status"])
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project);
    if json {
        command.arg("--json");
    }
    command
        .output()
        .unwrap_or_else(|e| panic!("aikit status should run: {e}"))
}

#[test]
fn plain_status_is_a_human_summary_not_json() {
    let (home, project) = fixture();
    let output = run(home.path(), project.path(), false);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim_start().starts_with('{'),
        "human status must not be JSON: {stdout}"
    );
    assert!(stdout.contains("Active capabilities:"), "{stdout}");
    assert!(stdout.contains("Catalogued but inactive:"), "{stdout}");
    assert!(stdout.contains("Health findings:"), "{stdout}");
    assert!(
        output.stderr.is_empty(),
        "findings belong in the summary, not stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn json_status_keeps_the_machine_envelope() {
    let (home, project) = fixture();
    let output = run(home.path(), project.path(), true);
    assert!(output.status.success());
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("status --json must emit a JSON envelope");
    assert_eq!(envelope["ok"], true);
    assert!(envelope["data"]["active_count"].is_u64());
    assert!(envelope["data"]["hash"].is_string());
    assert!(envelope["data"]["bypasses"].is_array());
}
