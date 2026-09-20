use serde_json::{json, Value};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PROVIDER: &str = "aikit.git-cli/v1";
const VERSION: &str = "aikit.git-project-observation/v1";

fn git(path: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|error| format!("failed to execute git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn optional_git(path: &Path, args: &[&str]) -> Option<String> {
    git(path, args).ok().filter(|value| !value.is_empty())
}

fn parse_status(text: &str) -> Value {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicts = Vec::new();

    for line in text.lines() {
        if line.len() < 3 {
            continue;
        }
        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let path = line[3..].to_string();

        if x == '?' && y == '?' {
            untracked.push(path);
            continue;
        }

        let unmerged = matches!(
            (x, y),
            ('D', 'D')
                | ('A', 'U')
                | ('U', 'D')
                | ('U', 'A')
                | ('D', 'U')
                | ('A', 'A')
                | ('U', 'U')
        );
        if unmerged {
            conflicts.push(path);
            continue;
        }

        if x != ' ' {
            staged.push(path.clone());
        }
        if y != ' ' {
            unstaged.push(path);
        }
    }

    json!({
        "clean": staged.is_empty() && unstaged.is_empty() && untracked.is_empty() && conflicts.is_empty(),
        "staged": staged,
        "unstaged": unstaged,
        "untracked": untracked,
        "conflicts": conflicts,
    })
}

fn parse_worktrees(text: &str) -> Vec<Value> {
    let mut result = Vec::new();
    let mut path: Option<String> = None;
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut detached = false;

    let flush = |result: &mut Vec<Value>,
                 path: &mut Option<String>,
                 head: &mut Option<String>,
                 branch: &mut Option<String>,
                 detached: &mut bool| {
        if let Some(worktree_path) = path.take() {
            result.push(json!({
                "path": worktree_path,
                "head": head.take(),
                "branch": branch.take(),
                "detached": *detached,
            }));
        }
        *detached = false;
    };

    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            flush(
                &mut result,
                &mut path,
                &mut head,
                &mut branch,
                &mut detached,
            );
        } else if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("HEAD ") {
            head = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("branch ") {
            branch = Some(
                value
                    .strip_prefix("refs/heads/")
                    .unwrap_or(value)
                    .to_string(),
            );
        } else if line == "detached" {
            detached = true;
        }
    }
    result
}

fn observe(path: &Path) -> Result<Value, String> {
    let root = git(path, &["rev-parse", "--show-toplevel"])?;
    let root_path = PathBuf::from(&root);
    let head = optional_git(&root_path, &["rev-parse", "HEAD"]);
    let branch = optional_git(&root_path, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let detached = head.is_some() && branch.is_none();
    let upstream = optional_git(
        &root_path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );
    let status = git(
        &root_path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    let worktrees = git(&root_path, &["worktree", "list", "--porcelain"])?;

    Ok(json!({
        "version": VERSION,
        "provider": PROVIDER,
        "repository_root": root,
        "worktree": root_path.to_string_lossy(),
        "head": head,
        "branch": branch,
        "detached": detached,
        "upstream": upstream,
        "difference": parse_status(&status),
        "worktrees": parse_worktrees(&worktrees),
        "identity_law": {
            "repository_is_project_identity": false,
            "worktree_is_world_identity": false,
            "head_is_current_material_state": false
        }
    }))
}

fn main() -> ExitCode {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    match observe(&path) {
        Ok(observation) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&observation).expect("serialize observation")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aikit-git-project: {error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn status_preserves_dirty_material_beside_head() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init"]).unwrap();
        git(
            dir.path(),
            &["config", "user.email", "test@example.invalid"],
        )
        .unwrap();
        git(dir.path(), &["config", "user.name", "AIKit Test"]).unwrap();
        fs::write(dir.path().join("Ground.md"), "one\n").unwrap();
        git(dir.path(), &["add", "Ground.md"]).unwrap();
        git(dir.path(), &["commit", "-m", "ground"]).unwrap();
        fs::write(dir.path().join("Ground.md"), "two\n").unwrap();

        let observation = observe(dir.path()).unwrap();
        assert!(observation["head"].is_string());
        assert_eq!(observation["difference"]["clean"], false);
        assert_eq!(
            observation["identity_law"]["head_is_current_material_state"],
            false
        );
    }

    #[test]
    fn worktree_parser_keeps_branch_and_material_path_separate() {
        let parsed = parse_worktrees("worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\n");
        assert_eq!(parsed[0]["path"], "/tmp/repo");
        assert_eq!(parsed[0]["branch"], "main");
    }
}
