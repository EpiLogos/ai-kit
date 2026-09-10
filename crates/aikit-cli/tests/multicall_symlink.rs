//! The multicall shim, end to end through the real binary.
//!
//! A registry, a project, a resolved generation and a `bin/` export are all built
//! on a real filesystem; then a **real symlink** named after the export points at
//! the `aikit` binary and is invoked. The binary must notice it was not called as
//! `aikit`, find the context's current generation, locate the capsule that owns
//! the export at the applied revision, and run its real payload.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;

use aikit_cli::app::{AikitApplication, ApplyRequest, Service};
use aikit_core::scope::ScopeKind;
use aikit_store::home::AikitHome;
use tempfile::TempDir;

const CONTEXT_ID: &str = "ctx_01HZYMULTICALL0000000000";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A personal registry with one script capsule that exports `greet`.
fn seed_registry(home: &Path) {
    let base = home.join("registries/personal/capsules/script/demo/greet");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "Greets the world for the multicall test."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
exports = ["greet"]
"#,
    );
    let run = base.join("payload/run.sh");
    write(&run, "#!/bin/sh\necho \"hello $1\"\n");
    let mut perms = fs::metadata(&run).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&run, perms).unwrap();
}

#[test]
fn a_symlink_named_after_an_export_runs_the_capsule_that_owns_it() {
    let home_dir = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let home_path = home_dir.path();

    seed_registry(home_path);

    // The project enables the capability.
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    // Build and commit a generation for a fixed context, in process, using the
    // real service — this is the same code `aikit apply` runs.
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_path);
    let mut service =
        Service::open(home, project.path(), |k| env.get(k).cloned()).expect("service opens");
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .expect("apply builds a generation");

    // The multicall path gates on trust, so review the capsule before invoking.
    review(home_path, project.path(), "script/demo/greet");

    // Symlink `greet` -> the aikit binary.
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("greet");
    symlink(&bin, &link).unwrap();

    // Invoke through the symlink.
    let output = std::process::Command::new(&link)
        .arg("world")
        .env("AIKIT_HOME", home_path)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .expect("the symlinked binary runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "exit={:?} stdout={stdout:?} stderr={:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("hello world"),
        "the capsule's real payload should have run; got {stdout:?}"
    );
}

/// Review a capsule in the store the running binary will read, so a symlink
/// invocation of it passes the trust gate.
fn review(home_path: &Path, project: &Path, id: &str) {
    use aikit_core::catalog::Catalog;
    use aikit_core::id::CapsuleId;
    use aikit_core::trust::{TrustKey, TrustState};
    use aikit_store::index::Index;
    use aikit_store::trust::TrustStore;

    let home = AikitHome::at(home_path);
    let load = aikit_cli::app::load_catalog(&home, Some(project)).unwrap();
    let cid = CapsuleId::parse(id).unwrap();
    let capsule = Catalog::get(&load.catalog, &cid).expect("capsule is catalogued");
    let key = TrustKey::new(
        capsule.source.clone().unwrap(),
        capsule.id.clone(),
        capsule.revision.clone().unwrap(),
    );
    let index = Index::open(&home.database()).unwrap();
    TrustStore::new(&index)
        .record(&key, TrustState::Reviewed, None)
        .unwrap();
}

#[test]
fn an_unreviewed_export_invoked_by_symlink_refuses_with_trust_required() {
    // The multicall shim is on the PATH, so a symlink named after an export runs
    // the capsule unattended. That must be gated by the same trust check the
    // interactive `aikit run` uses: an unreviewed executable REFUSES rather than
    // silently running whatever a `git pull` left in a bin/ shim.
    let home_dir = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let home_path = home_dir.path();
    seed_registry(home_path);
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    // Apply the generation, but do NOT review the capsule: it stays `Unseen`.
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_path);
    let mut service = Service::open(home, project.path(), |k| env.get(k).cloned()).unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("greet");
    symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .arg("world")
        .env("AIKIT_HOME", home_path)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "an unreviewed export must refuse to run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--confirm") || stderr.contains("review"),
        "the refusal must tell the user how to proceed; got {stderr:?}"
    );
    // It must NOT have run the payload.
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("hello world"),
        "the unreviewed payload must not have executed"
    );
}

#[test]
fn a_reviewed_export_invoked_by_symlink_runs() {
    // The other side of the gate: once the capsule is reviewed, the same symlink
    // invocation runs its real payload.
    let home_dir = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let home_path = home_dir.path();
    seed_registry(home_path);
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_path);
    let mut service = Service::open(home, project.path(), |k| env.get(k).cloned()).unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    review(home_path, project.path(), "script/demo/greet");

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("greet");
    symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .arg("world")
        .env("AIKIT_HOME", home_path)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "a reviewed export runs; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello world"));
}

#[test]
fn an_unknown_export_name_is_reported_not_silently_ignored() {
    let home_dir = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed_registry(home_dir.path());
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_dir.path());
    let mut service = Service::open(home, project.path(), |k| env.get(k).cloned()).unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("nonexistent-export");
    symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .env("AIKIT_HOME", home_dir.path())
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "an unknown export must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonexistent-export"),
        "the error should name the export; got {stderr:?}"
    );
}
