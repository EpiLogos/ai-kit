//! `aikit worktree project` against the real binary and real git repositories.
//!
//! The projection is exercised end to end: observe reports drift without
//! touching the checkout, `--apply` fast-forwards a clean behind checkout, and a
//! dirty checkout is surfaced with its uncommitted work preserved byte for byte.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_AUTHOR_NAME", "AIKit Test")
        .env("GIT_AUTHOR_EMAIL", "aikit@example.invalid")
        .env("GIT_COMMITTER_NAME", "AIKit Test")
        .env("GIT_COMMITTER_EMAIL", "aikit@example.invalid")
        .status()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed");
}

fn head(cwd: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A bare origin with one `main` commit and a working clone sitting on it.
fn origin_with_clone(root: &Path) -> (PathBuf, PathBuf) {
    let origin = root.join("origin.git");
    git(root, &["init", "-q", "-b", "main", "--bare", "origin.git"]);
    let seed = root.join("seed");
    git(
        root,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    );
    fs::write(seed.join("README.md"), "one\n").unwrap();
    git(&seed, &["add", "README.md"]);
    git(&seed, &["commit", "-qm", "c1"]);
    git(&seed, &["push", "-q", "-u", "origin", "main"]);
    let work = root.join("work");
    git(
        root,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    (origin, work)
}

fn advance_origin(root: &Path, origin: &Path) {
    let mover = root.join("mover");
    git(
        root,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            mover.to_str().unwrap(),
        ],
    );
    fs::write(mover.join("README.md"), "one\ntwo\n").unwrap();
    git(&mover, &["commit", "-qam", "c2"]);
    git(&mover, &["push", "-q", "origin", "main"]);
    fs::remove_dir_all(&mover).unwrap();
}

/// A minimal AIKit home + a project cwd (so `Service::discover` resolves for the
/// reply envelope), fully isolated from the real machine.
fn isolated_home() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join(".aikit")).unwrap();
    fs::write(project.path().join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    (home, project)
}

fn run_aikit(home: &Path, cwd: &Path, args: &[&str]) -> (bool, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?}: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

/// The single entry from a one-repo projection envelope.
fn only_entry(envelope: &Value) -> &Value {
    &envelope["data"]["entries"][0]
}

#[test]
fn observe_reports_drift_without_touching_the_checkout() {
    if !git_available() {
        eprintln!("skipped: git is not available");
        return;
    }
    let root = TempDir::new().unwrap();
    let (origin, work) = origin_with_clone(root.path());
    advance_origin(root.path(), &origin);
    let before = head(&work);

    let (home, project) = isolated_home();
    let repo = format!("demo={}", work.display());
    let (ok, envelope) = run_aikit(
        home.path(),
        project.path(),
        &["worktree", "project", "--repo", &repo],
    );
    assert!(ok, "observe must succeed: {envelope}");
    assert_eq!(envelope["ok"], Value::Bool(true));
    let entry = only_entry(&envelope);
    assert_eq!(entry["divergence"]["relation"], "behind");
    assert_eq!(entry["action"]["action"], "would-fast-forward");
    assert_eq!(envelope["data"]["applied"], Value::Bool(false));
    // Observe never moves HEAD.
    assert_eq!(head(&work), before, "observe must not move HEAD");
}

#[test]
fn apply_fast_forwards_a_clean_behind_checkout() {
    if !git_available() {
        eprintln!("skipped: git is not available");
        return;
    }
    let root = TempDir::new().unwrap();
    let (origin, work) = origin_with_clone(root.path());
    advance_origin(root.path(), &origin);

    let (home, project) = isolated_home();
    let repo = format!("demo={}", work.display());
    let (ok, envelope) = run_aikit(
        home.path(),
        project.path(),
        &["worktree", "project", "--apply", "--repo", &repo],
    );
    assert!(ok, "apply must succeed: {envelope}");
    let entry = only_entry(&envelope);
    assert_eq!(entry["action"]["action"], "fast-forwarded");
    // HEAD now sits exactly on origin/main.
    let origin_main = {
        let out = Command::new("git")
            .arg("-C")
            .arg(&work)
            .args(["rev-parse", "origin/main"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(head(&work), origin_main, "HEAD must land on origin/main");
}

#[test]
fn apply_surfaces_and_preserves_a_dirty_checkout() {
    if !git_available() {
        eprintln!("skipped: git is not available");
        return;
    }
    let root = TempDir::new().unwrap();
    let (origin, work) = origin_with_clone(root.path());
    advance_origin(root.path(), &origin);
    // Uncommitted local work — the case a reset --hard would destroy.
    fs::write(work.join("README.md"), "one\nLOCAL WORK\n").unwrap();
    let before = head(&work);

    let (home, project) = isolated_home();
    let repo = format!("demo={}", work.display());
    let (ok, envelope) = run_aikit(
        home.path(),
        project.path(),
        &["worktree", "project", "--apply", "--repo", &repo],
    );
    assert!(
        ok,
        "the command itself succeeds even when a repo is surfaced: {envelope}"
    );
    let entry = only_entry(&envelope);
    assert_eq!(entry["action"]["action"], "surfaced");
    assert_eq!(entry["clean"], Value::Bool(false));
    // The surfaced repo is a warning, and the work is untouched.
    assert_eq!(head(&work), before, "a dirty tree must never be moved");
    assert_eq!(
        fs::read_to_string(work.join("README.md")).unwrap(),
        "one\nLOCAL WORK\n",
        "uncommitted work must be preserved byte for byte"
    );
}
