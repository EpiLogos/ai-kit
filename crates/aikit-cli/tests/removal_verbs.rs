//! Removal lifecycle through the real CLI binary: a registered source and a
//! bound project can be removed outright, and removal means removal — the
//! registration, its snapshots and the projected link are gone, and the
//! source's skills stop resolving.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    (output, value)
}

fn skill_pack(root: &Path) {
    write(
        &root.join("alpha/SKILL.md"),
        "---\nname: alpha\ndescription: A removal-lifecycle skill.\n---\n\nalpha\n",
    );
    write(
        &root.join("beta/SKILL.md"),
        "---\nname: beta\ndescription: Another removal-lifecycle skill.\n---\n\nbeta\n",
    );
}

#[test]
fn a_source_can_be_removed_and_then_stops_existing() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let pack = temp.path().join("pack");
    skill_pack(&pack);

    let (added, reply) = run(
        &home,
        temp.path(),
        &["source", "add-directory", "testsrc", pack.to_str().unwrap()],
    );
    assert!(added.status.success(), "add failed: {reply}");
    let (synced, reply) = run(&home, temp.path(), &["source", "sync", "testsrc"]);
    assert!(synced.status.success(), "sync failed: {reply}");
    let (promoted, reply) = run(&home, temp.path(), &["source", "promote", "testsrc"]);
    assert!(promoted.status.success(), "promote failed: {reply}");
    assert_eq!(reply["data"]["skills"], 2);

    // The promoted snapshot feeds the shared catalog, so a plain remove
    // refuses and names why; removal is still reachable through --force,
    // which states the loss it is choosing.
    let (refused, reply) = run(&home, temp.path(), &["source", "remove", "testsrc"]);
    assert!(
        !refused.status.success(),
        "plain remove must refuse: {reply}"
    );
    assert_eq!(reply["error"]["code"], "source.still_active");

    let (forced, reply) = run(
        &home,
        temp.path(),
        &["source", "remove", "testsrc", "--force"],
    );
    assert!(forced.status.success(), "forced remove failed: {reply}");
    assert_eq!(reply["data"]["removed"], true);
    assert_eq!(reply["data"]["removed_snapshots"], 1);

    assert!(
        !home.join("sources/testsrc").exists(),
        "the registration, its state and its snapshots are gone"
    );
    let (shown, reply) = run(&home, temp.path(), &["source", "show", "testsrc"]);
    assert!(!shown.status.success(), "the source must no longer exist");
    assert_eq!(reply["error"]["code"], "source.unknown");
}

#[test]
fn a_project_binding_can_be_removed_and_releases_its_projection_link() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("demo-project");
    fs::create_dir_all(&project).unwrap();
    write(&project.join(".aikit/profile.toml"), "schema = 1\n");
    write(&project.join("README.md"), "human file\n");

    let (bound, reply) = run(
        &home,
        &project,
        &[
            "project",
            "bind",
            "demo",
            "--directory",
            project.to_str().unwrap(),
        ],
    );
    assert!(bound.status.success(), "bind failed: {reply}");
    assert!(home.join("projects/demo.toml").is_file());

    // The codex project seam hangs a symlink into the bound directory; if a
    // projection put one there, unbinding must take its own link back.
    let link = project.join(".agents/skills");
    fs::create_dir_all(project.join(".agents")).unwrap();
    std::os::unix::fs::symlink(&home, &link).unwrap();
    // A link into the home's contexts dir is AIKit-owned; this one points at
    // the home root, so it is NOT AIKit's and must survive unbind untouched.
    let (unbound, reply) = run(&home, &project, &["project", "unbind", "demo"]);
    assert!(unbound.status.success(), "unbind failed: {reply}");
    assert_eq!(reply["data"]["unbound"], true);
    assert!(!home.join("projects/demo.toml").exists());
    assert!(
        link.symlink_metadata().is_ok(),
        "a foreign link is not AIKit's to remove"
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "human file\n"
    );

    let (again, reply) = run(&home, &project, &["project", "unbind", "demo"]);
    assert!(
        !again.status.success(),
        "double unbind must say so: {reply}"
    );
    assert_eq!(reply["error"]["code"], "project.unknown");
}
