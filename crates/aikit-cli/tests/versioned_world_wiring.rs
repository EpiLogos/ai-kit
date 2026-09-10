//! The wiring proof: the CLI backend observes real git material for a real
//! Project, and the reading the Compose preview renders carries it.
//!
//! This drives the production `PaletteBackend` implementation against an
//! actual repository created by actual `git` — not a fixture — because the
//! whole point of the slice is that a socket nothing plugged into is now
//! plugged in.

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

/// A real repository with a real ProjectCentral identity, because
/// `versioned_world` refuses to observe without a bound Project.
fn project(root: &std::path::Path) {
    // `.aikit` is what makes the service recognise a Project at all
    // (`discover::MARKER`); `ProjectCentral/project.json` is what gives it a
    // native owner identity. Both are needed before material can be observed
    // *for* anything.
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
fn the_cli_backend_observes_real_git_material_for_a_real_project() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);

    let service = service(tmp.path(), &root);
    let observed = service
        .versioned_world()
        .expect("observation does not fail on a real worktree");

    let versioned = observed.expect("a real git worktree is observed, not reported absent");
    assert_eq!(versioned.repository.branch.as_deref(), Some("trunk"));
    assert!(!versioned.repository.head.as_str().is_empty());
    assert!(
        versioned.working.is_clean(),
        "a fresh commit leaves a clean tree"
    );
    // The observation names the Project it was made for; it cannot rebind it.
    assert_eq!(versioned.project.as_str(), "project:probe");
}

#[test]
fn a_project_that_is_not_a_worktree_reads_as_absent_not_as_a_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("plain");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join(".aikit")).unwrap();
    std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
    std::fs::write(
        root.join("ProjectCentral/project.json"),
        r#"{"schema":"central.project/v1","project_id":"project:plain","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
    )
    .unwrap();

    let service = service(tmp.path(), &root);
    assert!(
        service.versioned_world().unwrap().is_none(),
        "a directory under no version control is an absence, not an error"
    );
}

#[test]
fn a_directory_with_no_project_identity_is_not_observed_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("unbound");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);
    std::fs::remove_file(root.join("ProjectCentral/project.json")).unwrap();

    let service = service(tmp.path(), &root);
    assert!(
        service.versioned_world().unwrap().is_none(),
        "no bound Project means there is nothing to observe material *for*"
    );
}
