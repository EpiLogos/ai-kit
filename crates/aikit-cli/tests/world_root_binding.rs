//! Binding the World's own ground beneath a bound world root.
//!
//! The World keeps its authored ground (the World record, the Agent profiles,
//! the Agent sets) in the root register, a repository of its own directly
//! beneath the world root, while the member projects live a level further
//! down. Binding the world root must reach the former without swallowing the
//! latter.

use std::path::{Path, PathBuf};
use std::process::Command;

use aikit_cli::projects;
use aikit_store::AikitHome;

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A world root: the directory carries `.central/`, holds the root register in
/// a repository of its own, and indexes its member projects one level down.
fn world_root(tmp: &Path) -> PathBuf {
    let world = tmp.join("world");
    std::fs::create_dir_all(world.join(".central")).unwrap();

    let ground = world.join("Control");
    std::fs::create_dir_all(ground.join("agents/profiles")).unwrap();
    git(&ground, &["init", "--quiet"]);

    let member = world.join("Work/member");
    std::fs::create_dir_all(member.join("ProjectCentral")).unwrap();
    write(
        &member.join("ProjectCentral/project.json"),
        "{\"schema\":1}\n",
    );
    git(&member, &["init", "--quiet"]);

    world
}

#[test]
fn a_bound_world_root_reaches_its_own_ground_without_swallowing_a_member_project() {
    let tmp = tempfile::tempdir().unwrap();
    let world = world_root(tmp.path());
    let home = AikitHome::at(tmp.path().join("home"));
    home.ensure_layout().unwrap();
    projects::bind(&home, "world", std::slice::from_ref(&world), &[], &[], true).unwrap();

    let ground = projects::resolve(&home, &world.join("Control")).unwrap();
    let ground = ground.expect("the World's own ground is inside the bound world root");
    assert_eq!(ground.spec.id, "world");
    assert_eq!(ground.matched_by, "directory");

    // A member project carries its own ground; the bound root must not claim it.
    assert!(
        projects::resolve(&home, &world.join("Work/member"))
            .unwrap()
            .is_none(),
        "a member project must be matched on its own terms, not swallowed by the root"
    );

    // Anything else beneath the root that is not its own repository stays in.
    let plain = world.join("Work/plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(projects::resolve(&home, &plain).unwrap().is_some());
}
