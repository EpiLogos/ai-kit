//! Real end-to-end tests for `scripts/build-hygiene.sh`.
//!
//! Each test builds a real git repository (with real worktrees, branches,
//! merges and dirty files) and drives the script against it. Assertions are on
//! actual filesystem and git state, not on the script's own output. `HOME` is
//! redirected to a temp directory so the shared-target detection can never
//! touch a real user-level cargo dir.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn script_path() -> PathBuf {
    repo_root().join("scripts/build-hygiene.sh")
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to start: {e}"))
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit(dir: &Path, msg: &str) {
    git_ok(dir, &["add", "-A"]);
    git_ok(
        dir,
        &[
            "-c",
            "user.name=hygiene test",
            "-c",
            "user.email=hygiene@test.invalid",
            "commit",
            "-m",
            msg,
        ],
    );
}

/// A real repository on `main` with one committed file.
fn init_repo(dir: &Path) {
    git_ok(dir, &["init", "-b", "main"]);
    fs::write(dir.join("README"), "base\n").unwrap();
    commit(dir, "init");
}

/// Add a linked worktree on a new branch.
fn add_worktree(repo: &Path, worktree: &Path, branch: &str) {
    git_ok(
        repo,
        &["worktree", "add", "-b", branch, worktree.to_str().unwrap()],
    );
}

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Run the script from `dir` with an isolated HOME and no CARGO_TARGET_DIR.
fn run_script(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new("bash")
        .arg(script_path())
        .args(args)
        .current_dir(dir)
        .env("HOME", home)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap_or_else(|e| panic!("script failed to start: {e}"))
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn has_worktree(repo: &Path, worktree: &Path) -> bool {
    let out = git(repo, &["worktree", "list", "--porcelain"]);
    String::from_utf8_lossy(&out.stdout).contains(worktree.to_str().unwrap())
}

fn has_branch(repo: &Path, branch: &str) -> bool {
    let out = git(repo, &["branch", "--list", branch]);
    !String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

#[test]
fn report_lists_every_target_and_clean_removes_worktree_targets_only() {
    let repo_dir = TempDir::new().unwrap();
    let wt_dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    init_repo(repo_dir.path());
    add_worktree(repo_dir.path(), wt_dir.path(), "feature/unmerged");

    write_file(&repo_dir.path().join("target/debug/artifact.bin"), "main");
    write_file(&wt_dir.path().join("target/debug/artifact.bin"), "wt");

    let report = run_script(repo_dir.path(), home.path(), &["report"]);
    assert!(report.status.success(), "{}", stderr(&report));
    let text = stdout(&report);
    assert!(
        text.contains("main"),
        "report should name the main checkout: {text}"
    );
    assert!(
        text.contains("feature/unmerged"),
        "report should name the branch: {text}"
    );
    assert!(
        text.contains("unmerged"),
        "branch is not merged yet: {text}"
    );

    let clean = run_script(
        repo_dir.path(),
        home.path(),
        &["clean", "--worktrees", "--yes"],
    );
    assert!(clean.status.success(), "{}", stderr(&clean));
    assert!(
        !wt_dir.path().join("target").exists(),
        "worktree target removed"
    );
    assert!(
        repo_dir.path().join("target").exists(),
        "main target untouched by --worktrees"
    );
    assert!(wt_dir.path().exists(), "worktree itself survives a clean");
    assert!(has_branch(repo_dir.path(), "feature/unmerged"));
    assert!(has_worktree(repo_dir.path(), wt_dir.path()));
}

#[test]
fn clean_without_yes_refuses_non_interactive_runs() {
    let repo_dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    init_repo(repo_dir.path());
    write_file(&repo_dir.path().join("target/debug/artifact.bin"), "main");

    let out = run_script(repo_dir.path(), home.path(), &["clean", "--repo"]);
    assert!(!out.status.success());
    let text = stderr(&out);
    assert!(
        text.contains("--yes"),
        "non-interactive refusal should name --yes: {text}"
    );
    assert!(
        repo_dir.path().join("target").exists(),
        "nothing was removed"
    );
}

#[test]
fn prune_removes_a_merged_worktree_and_its_branch() {
    let repo_dir = TempDir::new().unwrap();
    let wt_dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    init_repo(repo_dir.path());
    add_worktree(repo_dir.path(), wt_dir.path(), "feature/merged");

    write_file(&wt_dir.path().join("feature.txt"), "feature\n");
    commit(wt_dir.path(), "feature work");
    git_ok(
        repo_dir.path(),
        &[
            "-c",
            "user.name=hygiene test",
            "-c",
            "user.email=hygiene@test.invalid",
            "merge",
            "--no-ff",
            "feature/merged",
            "-m",
            "merge feature",
        ],
    );

    let prune = run_script(repo_dir.path(), home.path(), &["prune", "--yes"]);
    assert!(prune.status.success(), "{}", stderr(&prune));
    assert!(
        !wt_dir.path().exists(),
        "merged worktree directory removed: {}",
        stdout(&prune)
    );
    assert!(!has_worktree(repo_dir.path(), wt_dir.path()));
    assert!(!has_branch(repo_dir.path(), "feature/merged"));
}

#[test]
fn prune_keeps_a_dirty_merged_worktree_unless_forced() {
    let repo_dir = TempDir::new().unwrap();
    let wt_dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    init_repo(repo_dir.path());
    add_worktree(repo_dir.path(), wt_dir.path(), "feature/dirty-merged");

    write_file(&wt_dir.path().join("feature.txt"), "feature\n");
    commit(wt_dir.path(), "feature work");
    git_ok(
        repo_dir.path(),
        &[
            "-c",
            "user.name=hygiene test",
            "-c",
            "user.email=hygiene@test.invalid",
            "merge",
            "--no-ff",
            "feature/dirty-merged",
            "-m",
            "merge feature",
        ],
    );
    write_file(&wt_dir.path().join("uncommitted.txt"), "keep me\n");

    let cautious = run_script(repo_dir.path(), home.path(), &["prune", "--yes"]);
    assert!(cautious.status.success(), "{}", stderr(&cautious));
    assert!(
        stdout(&cautious).contains("dirty"),
        "script should report the dirty worktree: {}",
        stdout(&cautious)
    );
    assert!(
        wt_dir.path().exists(),
        "dirty worktree survives without --force"
    );
    assert!(has_branch(repo_dir.path(), "feature/dirty-merged"));

    let forced = run_script(repo_dir.path(), home.path(), &["prune", "--yes", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert!(
        !wt_dir.path().exists(),
        "--force discards the dirty worktree"
    );
    assert!(!has_branch(repo_dir.path(), "feature/dirty-merged"));
}

#[test]
fn report_and_prune_handle_stale_prunable_registrations() {
    let repo_dir = TempDir::new().unwrap();
    let wt_dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    init_repo(repo_dir.path());
    add_worktree(repo_dir.path(), wt_dir.path(), "feature/stale");

    fs::remove_dir_all(wt_dir.path()).unwrap();
    let listed = git(repo_dir.path(), &["worktree", "list"]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("prunable"),
        "precondition: git marks the removed worktree prunable"
    );

    let report = run_script(repo_dir.path(), home.path(), &["report"]);
    assert!(report.status.success(), "{}", stderr(&report));
    assert!(
        stdout(&report).contains("stale"),
        "report should call out the stale registration: {}",
        stdout(&report)
    );

    let prune = run_script(repo_dir.path(), home.path(), &["prune", "--yes"]);
    assert!(prune.status.success(), "{}", stderr(&prune));
    assert!(
        !has_worktree(repo_dir.path(), wt_dir.path()),
        "registration pruned"
    );
}
