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
            let out = git(
                repo,
                &["worktree", "add", "-b", &branch, &path.to_string_lossy()],
            )?;
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

/// How a task's tree is arranged, discovered from the repository at close time.
///
/// Close is only given a task name, so it cannot assume the isolation the task
/// was spawned with. Assuming a worktree — as an earlier version did — means
/// `git worktree remove` is aimed at a path git has never heard of the moment
/// the task was the default, shared kind. The safe thing is to look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTree {
    /// No dedicated tree: the task used the session's working tree.
    Shared,
    /// A plain directory under `.aikit/tasks/`, not under version control.
    Directory(PathBuf),
    /// A registered git worktree.
    Worktree(Worktree),
}

/// Determine how a task's tree is arranged, by asking git and the filesystem.
pub fn detect_task(repo: &Path, name: &str) -> Result<TaskTree> {
    let path = task_path(repo, name);
    if !path.exists() {
        // Nothing was created for this task, so it shared the working tree.
        return Ok(TaskTree::Shared);
    }
    // A registered worktree is listed by git; a plain directory is not.
    // git reports canonical paths (on macOS the tempdir's `/var/...` becomes
    // `/private/var/...`), so both sides are canonicalized before comparison.
    let listed = git(repo, &["worktree", "list", "--porcelain"])?;
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let target = canonical(&path);
    let is_worktree = listed
        .stdout
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .any(|w| canonical(Path::new(w)) == target);
    if is_worktree {
        Ok(TaskTree::Worktree(Worktree {
            path,
            branch: branch_name(name),
        }))
    } else {
        Ok(TaskTree::Directory(path))
    }
}

/// Close a task by name, reckoning with the isolation it actually has.
///
/// Shared tasks touch nothing; directory tasks have their directory removed;
/// worktree tasks go through the worktree teardown, refusing an unclean one
/// unless `force`. This is the entry point the CLI uses.
pub fn close_task(repo: &Path, name: &str, force: bool) -> Result<()> {
    match detect_task(repo, name)? {
        TaskTree::Shared => Ok(()),
        TaskTree::Directory(dir) => std::fs::remove_dir_all(&dir).map_err(|e| {
            AikitError::new(
                "task.close_failed",
                format!("could not remove task directory {}: {e}", dir.display()),
            )
        }),
        TaskTree::Worktree(worktree) => close(repo, &worktree, force),
    }
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
        .map_err(|e| AikitError::new("task.git_unavailable", format!("could not run git: {e}")))?;
    Ok(GitOutput {
        status: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

/// One task as it exists on disk right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    pub name: String,
    pub isolation: Isolation,
    pub path: PathBuf,
    pub branch: Option<String>,
    /// Whether an isolated tree has uncommitted work. `false` for a shared task,
    /// which has no tree of its own to be dirty.
    pub dirty: bool,
}

/// Every task with a tree of its own under `repo`.
///
/// A **shared** task — the default — leaves no directory behind by design, so it
/// cannot be listed from the filesystem and does not appear here. Reporting only
/// what can actually be observed is the honest answer; inventing rows for tasks
/// that left no trace would be worse than a short list.
pub fn list(repo: &Path) -> Result<Vec<TaskSummary>> {
    let root = repo.join(".aikit").join("tasks");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let isolation = if path.join(".git").exists() {
            Isolation::Worktree
        } else {
            Isolation::Directory
        };
        let dirty = isolation == Isolation::Worktree
            && !worktree_blockers(&path).unwrap_or_default().is_empty();
        out.push(TaskSummary {
            name: name.to_string(),
            isolation,
            path: path.clone(),
            branch: (isolation == Isolation::Worktree).then(|| branch_name(name)),
            dirty,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
