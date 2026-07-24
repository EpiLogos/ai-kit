//! Task isolation, against a real git repository.
//!
//! The load-bearing rule from ARCHITECTURE.md §3: a worktree is opt-in, and
//! teardown refuses to discard a worktree that is not clean unless forced. These
//! tests use real `git`, a real worktree and a real dirty file — a mock of git
//! would only prove this crate calls itself the way it expects to.

use std::fs;
use std::path::Path;
use std::process::Command;

use aikit_cli::task;
use aikit_core::context::Isolation;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .expect("git should run");
    assert!(status.success(), "git {args:?} failed");
}

fn repo_with_commit() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    git(repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "initial"]);
    tmp
}

#[test]
fn spawning_a_shared_task_creates_no_worktree() {
    let tmp = repo_with_commit();
    let outcome = task::spawn(tmp.path(), "review", Isolation::Shared).unwrap();
    assert_eq!(outcome.isolation, Isolation::Shared);
    assert!(
        outcome.worktree.is_none(),
        "a shared task must not cut a worktree"
    );
    assert!(outcome.note.is_some(), "the shared fallback is stated, not hidden");
}

#[test]
fn spawning_a_worktree_task_creates_a_real_worktree_on_its_own_branch() {
    let tmp = repo_with_commit();
    let outcome = task::spawn(tmp.path(), "migration", Isolation::Worktree).unwrap();
    let wt = outcome.worktree.expect("a worktree task cuts a worktree");
    assert!(wt.path.exists(), "the worktree directory should exist");
    assert!(wt.path.join(".git").exists(), "it should be a real git worktree");

    // git knows about it.
    let list = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&list.stdout);
    assert!(text.contains(&wt.path.to_string_lossy().to_string()));
}

#[test]
fn closing_a_dirty_worktree_is_refused_without_force() {
    let tmp = repo_with_commit();
    let outcome = task::spawn(tmp.path(), "wip", Isolation::Worktree).unwrap();
    let wt = outcome.worktree.unwrap();

    // Make it unclean with an untracked file.
    fs::write(wt.path.join("scratch.txt"), "unsaved work\n").unwrap();

    let blockers = task::worktree_blockers(&wt.path).unwrap();
    assert!(!blockers.is_empty(), "an untracked file is a blocker");

    let refused = task::close(tmp.path(), &wt, false).unwrap_err();
    assert_eq!(refused.code(), "task.worktree_dirty");
    assert!(
        wt.path.exists(),
        "a refused close must leave the worktree intact"
    );
}

#[test]
fn a_clean_worktree_closes() {
    let tmp = repo_with_commit();
    let outcome = task::spawn(tmp.path(), "clean", Isolation::Worktree).unwrap();
    let wt = outcome.worktree.unwrap();

    assert!(task::worktree_blockers(&wt.path).unwrap().is_empty());
    task::close(tmp.path(), &wt, false).unwrap();
    assert!(!wt.path.exists(), "a clean worktree is removed");
}

#[test]
fn force_discards_even_a_dirty_worktree() {
    let tmp = repo_with_commit();
    let outcome = task::spawn(tmp.path(), "force", Isolation::Worktree).unwrap();
    let wt = outcome.worktree.unwrap();
    fs::write(wt.path.join("scratch.txt"), "unsaved\n").unwrap();

    task::close(tmp.path(), &wt, true).unwrap();
    assert!(!wt.path.exists(), "force removes the worktree regardless");
}
