//! Development Field S3 acceptance through the native application/CLI boundary.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

use aikit_cli::app::{DevelopmentFieldApplicationRequest, Service};
use aikit_core::resource::{
    DevelopmentFieldAvailabilityState, ResourceRef, VersionRevision,
    DEVELOPMENT_FIELD_READING_VERSION,
};
use aikit_store::AikitHome;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git is available in the test environment");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn project(root: &Path) -> String {
    std::fs::create_dir_all(root.join(".aikit")).unwrap();
    std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
    std::fs::write(
        root.join("ProjectCentral/project.json"),
        r#"{"schema":"central.project/v1","project_id":"project:probe","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
    )
    .unwrap();
    git(root, &["init", "--initial-branch=trunk"]);
    git(root, &["config", "user.email", "probe@example.invalid"]);
    git(root, &["config", "user.name", "probe"]);
    std::fs::write(root.join("README.md"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "first"]);
    git(root, &["rev-parse", "HEAD"])
}

fn service(home: &Path, root: &Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
}

fn aikit(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aikit"))
        .current_dir(root)
        .env("AIKIT_HOME", home)
        .args(args)
        .output()
        .expect("run the native aikit binary")
}

#[test]
fn application_read_composes_existing_resource_field_and_exact_worktree_basis() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    let base = project(&root);
    std::fs::write(root.join("README.md"), "two\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "not yet Git source\n").unwrap();

    let service = service(&home, &root);
    let reading = service
        .development_field_read(DevelopmentFieldApplicationRequest {
            subjects: vec![ResourceRef::parse("source:missing").unwrap()],
            base_revision: Some(VersionRevision::new(base)),
            ..DevelopmentFieldApplicationRequest::default()
        })
        .unwrap();

    assert_eq!(reading.version, DEVELOPMENT_FIELD_READING_VERSION);
    assert_eq!(
        reading.subjects[0].availability.state,
        DevelopmentFieldAvailabilityState::Unknown
    );
    assert_eq!(
        reading.central_self_description.availability.state,
        DevelopmentFieldAvailabilityState::Unknown,
        "Central S1 has not supplied a public self carrier here; AIKit must not infer a path"
    );
    let git = reading
        .git
        .expect("a real worktree supplies exact Git basis");
    let diff = git
        .current_diff_from_base
        .expect("an explicit base requests current difference");
    assert!(diff.patch.contains("+two"), "{}", diff.patch);
    assert_eq!(diff.untracked_paths, vec!["untracked.txt"]);
}

#[test]
fn native_cli_returns_the_same_bounded_packet_and_rejects_a_stale_expected_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    let base = project(&root);
    std::fs::write(root.join("README.md"), "two\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "not yet Git source\n").unwrap();

    let output = aikit(
        &root,
        &home,
        &[
            "--json",
            "development-field",
            "--ref",
            "source:missing",
            "--base",
            &base,
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        envelope
            .pointer("/data/version")
            .and_then(serde_json::Value::as_str),
        Some(DEVELOPMENT_FIELD_READING_VERSION)
    );
    assert_eq!(
        envelope
            .pointer("/data/git_basis/state")
            .and_then(serde_json::Value::as_str),
        Some("available")
    );
    assert!(envelope
        .pointer("/data/git/current_diff_from_base/patch")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|patch| patch.contains("+two")));
    assert!(envelope
        .pointer("/data/executable_basis/package_version")
        .and_then(serde_json::Value::as_str)
        .is_some());

    let stale = aikit(
        &root,
        &home,
        &[
            "--json",
            "development-field",
            "--expect-aikit-revision",
            "definitely-not-this-build",
        ],
    );
    assert!(!stale.status.success());
    assert!(
        String::from_utf8_lossy(&stale.stdout)
            .contains("resource.development_field_executable_revision_mismatch"),
        "{}",
        String::from_utf8_lossy(&stale.stdout)
    );
}
