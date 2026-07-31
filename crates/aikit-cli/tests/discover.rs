//! Context discovery on a real filesystem: walking up for `.aikit/`, building the
//! nested profile chain from the repo root down to the cwd, and assembling a
//! `ContextDescriptor` from the `AIKIT_*` environment.
//!
//! The environment is injected as a closure rather than read from the real
//! process, both so the tests are hermetic under a parallel runner and because
//! mutating `std::env` is a global side effect the CLI itself never performs.

use std::collections::BTreeMap;
use std::fs;

use aikit_cli::discover;
use aikit_core::context::Isolation;
use tempfile::TempDir;

fn touch_aikit(dir: &std::path::Path) {
    let a = dir.join(".aikit");
    fs::create_dir_all(&a).unwrap();
    fs::write(a.join("profile.toml"), "schema = 1\n").unwrap();
}

#[test]
fn discovery_walks_up_to_the_topmost_aikit_marker() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    let pkg = root.join("crates").join("payments");
    fs::create_dir_all(&pkg).unwrap();
    touch_aikit(&root);
    touch_aikit(&pkg);

    let project = discover::discover_project(&pkg).expect("a project should be found");
    assert_eq!(project.root, root, "the repo root owns the highest .aikit");

    // The chain runs root -> cwd, depth increasing, so the deeper package layer
    // wins ties per the resolver's precedence rules.
    let depths: Vec<u32> = project.chain.iter().map(|l| l.depth).collect();
    assert_eq!(depths, vec![0, 1]);
    assert_eq!(project.chain[0].dir, root);
    assert_eq!(project.chain[1].dir, pkg);
}

#[test]
fn a_single_marker_yields_a_one_element_chain_at_depth_zero() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    let deep = root.join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    touch_aikit(&root);

    let project = discover::discover_project(&deep).expect("found");
    assert_eq!(project.root, root);
    assert_eq!(project.chain.len(), 1);
    assert_eq!(project.chain[0].depth, 0);
}

#[test]
fn no_marker_means_no_project() {
    let tmp = TempDir::new().unwrap();
    let deep = tmp.path().join("x").join("y");
    fs::create_dir_all(&deep).unwrap();
    assert!(discover::discover_project(&deep).is_none());
}

#[test]
fn the_global_aikit_store_is_not_a_project_marker() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let project = home.join("work/project");
    fs::create_dir_all(home.join(".aikit/state")).unwrap();
    let custom_store = tmp.path().join("custom-aikit-home");
    fs::create_dir_all(&custom_store).unwrap();
    fs::create_dir_all(project.join(".aikit")).unwrap();

    let default_store = home.join(".aikit");
    let discovered = discover::discover_project_excluding_many(
        &project,
        &[custom_store.as_path(), default_store.as_path()],
    )
    .unwrap();
    assert_eq!(discovered.root, project);
    assert_eq!(discovered.chain.len(), 1);

    let unrelated = home.join("Documents/unrelated");
    fs::create_dir_all(&unrelated).unwrap();
    assert!(
        discover::discover_project_excluding_many(
            &unrelated,
            &[custom_store.as_path(), default_store.as_path()],
        )
        .is_none(),
        "the operational store must not turn the whole home directory into one project"
    );
}

#[test]
fn the_descriptor_defaults_isolation_to_shared() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    touch_aikit(&root);

    let env: BTreeMap<String, String> = BTreeMap::new();
    let d = discover::descriptor_from(&root, |k| env.get(k).cloned());
    assert_eq!(d.isolation, Isolation::Shared);
    assert!(!d.isolation.is_isolated());
    assert_eq!(d.project_root.as_deref(), Some(root.as_path()));
    assert!(d.session_id.is_none());
    assert!(d.task.is_none());
}

#[test]
fn the_descriptor_honours_aikit_environment_variables() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    touch_aikit(&root);

    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(
        "AIKIT_SESSION_ID".into(),
        "ses_01HZYSESSION0000000000000".into(),
    );
    env.insert("AIKIT_TASK".into(), "migration-review".into());
    env.insert("AIKIT_ISOLATION".into(), "worktree".into());

    let d = discover::descriptor_from(&root, |k| env.get(k).cloned());
    assert_eq!(d.isolation, Isolation::Worktree);
    assert!(d.isolation.is_isolated());
    assert!(d.isolation.owns_a_git_worktree());
    assert_eq!(d.task.as_deref(), Some("migration-review"));
    assert!(d.session_id.is_some());
    assert!(d.is_task());
}

#[test]
fn an_unparseable_isolation_value_falls_back_to_shared_honestly() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    touch_aikit(&root);

    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert("AIKIT_ISOLATION".into(), "nonsense".into());
    let d = discover::descriptor_from(&root, |k| env.get(k).cloned());
    assert_eq!(d.isolation, Isolation::Shared);
}
