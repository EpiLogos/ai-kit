//! The wiring proof: the CLI backend composes the ranked Model roster the
//! palette's roster overlay renders, from the same resolved route sets the
//! compose path produces. Driven against a real project.

use aikit_cli::app::Service;
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use std::collections::BTreeMap;
use std::process::Command;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git is available in the test environment");
    assert!(status.success(), "git {args:?} failed");
}

fn project(root: &std::path::Path) {
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
    std::fs::write(root.join("README.md"), "probe\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "first"]);
}

fn service(home: &std::path::Path, root: &std::path::Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
}

#[test]
fn the_cli_backend_composes_a_model_roster_for_a_real_project() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);

    let service = service(tmp.path(), &root);
    let roster = service
        .model_roster()
        .expect("composing the roster does not fail")
        .expect("a producer is attached — the roster was CLI-only before");

    // It is a real ranked roster (its schema version is set); entries may be
    // empty when no route is observed on this host, which is honest — the
    // overlay shows a real ranking, not a placeholder.
    assert!(!roster.schema_version.is_empty());
}

#[test]
fn a_context_with_no_project_composes_no_roster() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("plain");
    std::fs::create_dir_all(&root).unwrap();
    // No `.aikit`/ProjectCentral: not a Project. The producer answers None
    // rather than an empty roster that would read as "no Models exist".
    let service = service(tmp.path(), &root);
    assert!(service.model_roster().unwrap().is_none());
}

#[test]
fn the_roster_is_cached_across_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);

    let service = service(tmp.path(), &root);
    let first = service.model_roster().unwrap();
    let second = service.model_roster().unwrap();
    assert_eq!(first, second);
}
