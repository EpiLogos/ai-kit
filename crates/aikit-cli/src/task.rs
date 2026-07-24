//! Agent tasks and their isolation.
//!
//! The whole of ARCHITECTURE.md §3 lives here in executable form. A task shares
//! the session's working tree by default; [`Isolation::Worktree`] is the only mode
//! that cuts a git worktree, and it is the only mode whose teardown has to reckon
//! with git state. The shared case is not a degraded worktree — it is the normal
//! case, and [`spawn`] says so out loud rather than pretending a shared task has a
//! private tree it does not have.
//!
//! Teardown ([`close`]) refuses to discard a worktree that is dirty, carries
//! untracked files, or has unpushed commits, unless `--force` is given. The check
//! is [`worktree_blockers`], and it asks real `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use aikit_core::context::Isolation;
use aikit_core::{AikitError, Result};

/// A git worktree cut for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

/// What spawning a task produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnOutcome {
    pub name: String,
    pub isolation: Isolation,
    /// Present only for [`Isolation::Worktree`].
    pub worktree: Option<Worktree>,
    /// Present for a shared task: the honest statement that it did not get a
    /// private tree, and what that means for sibling tasks' client skills.
    pub note: Option<String>,
    /// The directory the task's agent should run in.
    pub directory: PathBuf,
}

/// The branch a task worktree is created on.
pub fn branch_name(name: &str) -> String {
    format!("aikit/{name}")
}

/// Where a task's worktree or directory is placed: under the repo's `.aikit/`, so
/// it is namespaced and easy to find, and out of the way of the source tree.
pub fn task_path(repo: &Path, name: &str) -> PathBuf {
    repo.join(".aikit").join("tasks").join(name)
}

/// Spawn a task with the requested isolation.
///
/// This does the filesystem/git part only; binding the task's context and
/// launching an agent are the caller's job. The point enforced here is that
/// [`Isolation::Shared`] and [`Isolation::Directory`] never touch git, and only
/// [`Isolation::Worktree`] creates a worktree.
pub fn spawn(repo: &Path, name: &str, isolation: Isolation) -> Result<SpawnOutcome> {
    match isolation {
        Isolation::Shared => Ok(SpawnOutcome {
            name: name.to_string(),
            isolation,
            worktree: None,
            note: Some(
                "shared tree: sibling tasks see the same files, so per-task native client \
                 skill surfaces fall back to the project-stable set"
                    .to_string(),
            ),
            directory: repo.to_path_buf(),
        }),
        Isolation::Directory => {
            let dir = task_path(repo, name);
            std::fs::create_dir_all(&dir).map_err(|e| {
                AikitError::new(
                    "task.directory_failed",
                    format!("could not create {}: {e}", dir.display()),
                )
            })?;
            Ok(SpawnOutcome {
                name: name.to_string(),
                isolation,
                worktree: None,
                note: None,
                directory: dir,
            })
        }
        Isolation::Worktree => {
            let path = task_path(repo, name);
            let branch = branch_name(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let out = git(repo, &[
                "worktree",
                "add",
                "-b",
                &branch,
                &path.to_string_lossy(),
            ])?;
            if !out.status {
                return Err(AikitError::new(
                    "task.worktree_failed",
                    format!("git worktree add failed: {}", out.stderr.trim()),
                )
                .with("name", name.to_string()));
            }
            Ok(SpawnOutcome {
                name: name.to_string(),
                isolation,
                worktree: Some(Worktree {
                    path: path.clone(),
                    branch,
                }),
                note: None,
                directory: path,
            })
        }
    }
}

/// The reasons a worktree is not safe to discard.
///
/// Empty means clean. Each entry is a human-readable reason, in a stable order so
/// the refusal message is deterministic.
pub fn worktree_blockers(worktree: &Path) -> Result<Vec<String>> {
    let mut blockers = Vec::new();

    let status = git(worktree, &["status", "--porcelain"])?;
    if !status.status {
        return Err(AikitError::new(
            "task.not_a_worktree",
            format!("`git status` failed in {}", worktree.display()),
        ));
    }
    let mut modified = false;
    let mut untracked = false;
    for line in status.stdout.lines() {
        if line.starts_with("??") {
            untracked = true;
        } else if !line.trim().is_empty() {
            modified = true;
        }
    }
    if modified {
        blockers.push("uncommitted changes".to_string());
    }
    if untracked {
        blockers.push("untracked files".to_string());
    }

    // Unpushed commits, but only when there is an upstream to be unpushed *from*.
    // Without an upstream, "unpushed" is not a meaningful teardown hazard — the
    // branch was never meant to have a remote — so we do not block on it.
    let upstream = git(worktree, &["rev-parse", "--abbrev-ref", "@{upstream}"])?;
    if upstream.status {
        let ahead = git(worktree, &["rev-list", "--count", "@{upstream}..HEAD"])?;
        if ahead.status && ahead.stdout.trim() != "0" && !ahead.stdout.trim().is_empty() {
            blockers.push(format!("{} unpushed commit(s)", ahead.stdout.trim()));
        }
    }

    Ok(blockers)
}

/// Close a task worktree, refusing an unclean one unless `force`.
///
/// The refusal is the whole point: `git worktree remove` will itself refuse a
/// dirty worktree, but AIKit refuses *first*, with a specific code and the list of
/// reasons, so the user is told what they would lose before anything is attempted.
pub fn close(repo: &Path, worktree: &Worktree, force: bool) -> Result<()> {
    if !force {
        let blockers = worktree_blockers(&worktree.path)?;
        if !blockers.is_empty() {
            return Err(AikitError::new(
                "task.worktree_dirty",
                format!(
                    "refusing to discard the worktree for `{}`: {}. Re-run with --force to \
                     discard it anyway",
                    worktree.branch,
                    blockers.join(", ")
                ),
            )
            .with("worktree", worktree.path.display().to_string())
            .with("blockers", blockers.join(", ")));
        }
    }

    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    let path = worktree.path.to_string_lossy().to_string();
    args.push(&path);
    let out = git(repo, &args)?;
    if !out.status {
        return Err(AikitError::new(
            "task.close_failed",
            format!("git worktree remove failed: {}", out.stderr.trim()),
        )
        .with("worktree", worktree.path.display().to_string()));
    }
    Ok(())
}

/// A captured git invocation.
struct GitOutput {
    status: bool,
    stdout: String,
    stderr: String,
}

fn git(cwd: &Path, args: &[&str]) -> Result<GitOutput> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            AikitError::new(
                "task.git_unavailable",
                format!("could not run git: {e}"),
            )
        })?;
    Ok(GitOutput {
        status: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}
