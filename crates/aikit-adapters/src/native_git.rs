//! Native Git provider for the versioned-World resource contract.
//!
//! Agents remain free to use ordinary Git CLI under their normal capability and
//! authority. This adapter gives AIKit a structured, reconciliable view of the
//! resulting repository/worktree state and a small high-value mutation floor.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use aikit_core::project::ProjectRef;
use aikit_core::resource::{
    CreateWorktreeRequest, GitRepositoryRelation, GitWorkingState, GitWorktreeRelation,
    ProviderRef, VersionDiff, VersionDiffRequest, VersionHistoryEntry, VersionHistoryRequest,
    VersionRevision, VersionedProjectWorld, VersionedWorldCapability, VersionedWorldProvider,
    VersionedWorldProviderDescriptor, VersionedWorldProviderStatus, VERSIONED_WORLD_VERSION,
};
use aikit_core::{AikitError, Result};

pub const NATIVE_GIT_PROVIDER_REF: &str = "aikit:provider:native-git";
pub const NATIVE_GIT_PROVIDER_VERSION: &str = "aikit.native-git/v1";

#[derive(Debug, Clone)]
pub struct NativeGitProvider {
    git: PathBuf,
    provider: ProviderRef,
}

impl NativeGitProvider {
    pub fn new() -> Result<Self> {
        Ok(Self {
            git: PathBuf::from("git"),
            provider: ProviderRef::parse(NATIVE_GIT_PROVIDER_REF)?,
        })
    }

    pub fn with_binary(binary: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            git: binary.into(),
            provider: ProviderRef::parse(NATIVE_GIT_PROVIDER_REF)?,
        })
    }

    fn output<I, S>(&self, locator: &str, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        Command::new(&self.git)
            .arg("-C")
            .arg(locator)
            .args(args)
            .output()
            .map_err(|error| {
                AikitError::new(
                    "versioned_world.git_spawn_failed",
                    format!("failed to invoke {}: {error}", self.git.display()),
                )
            })
    }

    fn checked<I, S>(&self, locator: &str, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = self.output(locator, args)?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn optional<I, S>(&self, locator: &str, args: I) -> Result<Option<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = self.output(locator, args)?;
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok((!value.is_empty()).then_some(value))
        } else {
            Ok(None)
        }
    }

    fn git_version(&self) -> Option<String> {
        Command::new(&self.git)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn repository_root(&self, locator: &str, worktree_root: &str) -> Result<String> {
        let common = self.checked(
            locator,
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = Path::new(&common);
        if common.file_name().and_then(|name| name.to_str()) == Some(".git") {
            if let Some(parent) = common.parent() {
                return Ok(parent.to_string_lossy().to_string());
            }
        }
        Ok(worktree_root.to_string())
    }

    fn working_state(&self, locator: &str) -> Result<GitWorkingState> {
        let output = self.output(locator, ["status", "--porcelain=v1", "-z"])?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        Ok(parse_porcelain_v1_z(&output.stdout))
    }

    fn worktrees(&self, locator: &str) -> Result<Vec<GitWorktreeRelation>> {
        let raw = self.checked(locator, ["worktree", "list", "--porcelain"])?;
        parse_worktrees(&raw)
    }
}

impl VersionedWorldProvider for NativeGitProvider {
    fn descriptor(&self) -> VersionedWorldProviderDescriptor {
        let version = self.git_version();
        VersionedWorldProviderDescriptor {
            provider: self.provider.clone(),
            status: if version.is_some() {
                VersionedWorldProviderStatus::Available
            } else {
                VersionedWorldProviderStatus::Unavailable {
                    reason: format!("{} is unavailable", self.git.display()),
                }
            },
            capabilities: vec![
                VersionedWorldCapability::Inspect,
                VersionedWorldCapability::Reconcile,
                VersionedWorldCapability::Diff,
                VersionedWorldCapability::History,
                VersionedWorldCapability::Worktrees,
                VersionedWorldCapability::CreateWorktree,
                VersionedWorldCapability::RemoveWorktree,
            ],
            implementation_version: version,
        }
    }

    fn inspect(&self, project: &ProjectRef, locator: &str) -> Result<VersionedProjectWorld> {
        let worktree_root = self.checked(locator, ["rev-parse", "--show-toplevel"])?;
        let repository_root = self.repository_root(locator, &worktree_root)?;
        let head = VersionRevision::new(self.checked(locator, ["rev-parse", "HEAD"])?);
        let branch = self.optional(locator, ["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        let detached = branch.is_none();
        let upstream = self.optional(
            locator,
            [
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        )?;
        let (ahead, behind) = if upstream.is_some() {
            self.optional(
                locator,
                ["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
            )?
            .and_then(|value| parse_ahead_behind(&value))
            .unwrap_or((0, 0))
        } else {
            (0, 0)
        };

        Ok(VersionedProjectWorld {
            version: VERSIONED_WORLD_VERSION.to_string(),
            project: project.clone(),
            provider: self.descriptor(),
            repository: GitRepositoryRelation {
                repository_root,
                worktree_root,
                head,
                branch,
                detached,
                upstream,
                ahead,
                behind,
            },
            working: self.working_state(locator)?,
            worktrees: self.worktrees(locator)?,
        })
    }

    fn diff(&self, locator: &str, request: &VersionDiffRequest) -> Result<VersionDiff> {
        let mut args = vec![
            "diff".to_string(),
            "--no-ext-diff".to_string(),
            "--binary".to_string(),
            request.from.as_str().to_string(),
            request.to.as_str().to_string(),
        ];
        if let Some(path) = &request.path {
            args.push("--".to_string());
            args.push(path.clone());
        }
        let output = self.output(locator, args)?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        let max = request.max_bytes.max(1);
        let truncated = output.stdout.len() > max;
        let bytes = &output.stdout[..output.stdout.len().min(max)];
        Ok(VersionDiff {
            from: request.from.clone(),
            to: request.to.clone(),
            path: request.path.clone(),
            patch: String::from_utf8_lossy(bytes).to_string(),
            truncated,
        })
    }

    fn history(
        &self,
        locator: &str,
        request: &VersionHistoryRequest,
    ) -> Result<Vec<VersionHistoryEntry>> {
        if request.limit == 0 {
            return Ok(Vec::new());
        }
        let mut args = vec![
            "log".to_string(),
            format!("-n{}", request.limit),
            "--format=%H%x1f%P%x1f%s%x1f%an%x1f%aI%x1e".to_string(),
        ];
        if let Some(path) = &request.path {
            args.push("--".to_string());
            args.push(path.clone());
        }
        let raw = self.checked(locator, args)?;
        Ok(parse_history(&raw))
    }

    fn create_worktree(
        &self,
        project: &ProjectRef,
        locator: &str,
        request: &CreateWorktreeRequest,
    ) -> Result<VersionedProjectWorld> {
        let args = if let Some(branch) = &request.branch {
            vec![
                "worktree".to_string(),
                "add".to_string(),
                "-b".to_string(),
                branch.clone(),
                request.path.clone(),
                request.base.as_str().to_string(),
            ]
        } else {
            vec![
                "worktree".to_string(),
                "add".to_string(),
                "--detach".to_string(),
                request.path.clone(),
                request.base.as_str().to_string(),
            ]
        };
        self.checked(locator, args)?;
        self.inspect(project, &request.path)
    }

    fn remove_worktree(
        &self,
        project: &ProjectRef,
        locator: &str,
        worktree_path: &str,
    ) -> Result<VersionedProjectWorld> {
        self.checked(locator, ["worktree", "remove", worktree_path])?;
        self.inspect(project, locator)
    }
}

fn git_failure(output: Output) -> AikitError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    AikitError::new(
        "versioned_world.git_failed",
        if stderr.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            stderr
        },
    )
}

fn parse_ahead_behind(value: &str) -> Option<(u64, u64)> {
    let mut parts = value.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn parse_porcelain_v1_z(raw: &[u8]) -> GitWorkingState {
    let mut state = GitWorkingState::default();
    let mut records = raw
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        if record.len() < 3 {
            continue;
        }
        let x = record[0] as char;
        let y = record[1] as char;
        let path = String::from_utf8_lossy(&record[3..]).to_string();
        if x == '?' && y == '?' {
            state.untracked.push(path);
            continue;
        }
        let conflicted = x == 'U' || y == 'U' || matches!((x, y), ('A', 'A') | ('D', 'D'));
        if conflicted {
            state.conflicted.push(path.clone());
        } else {
            if x != ' ' {
                state.staged.push(path.clone());
            }
            if y != ' ' {
                state.unstaged.push(path.clone());
            }
        }
        if x == 'R' || x == 'C' {
            let _ = records.next();
        }
    }
    state
}

fn parse_worktrees(raw: &str) -> Result<Vec<GitWorktreeRelation>> {
    let mut result = Vec::new();
    for block in raw.split("\n\n").filter(|block| !block.trim().is_empty()) {
        let mut path = None;
        let mut head = None;
        let mut branch = None;
        let mut detached = false;
        let mut locked = false;
        let mut prunable = false;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                head = Some(VersionRevision::new(value));
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_string(),
                );
            } else if line == "detached" {
                detached = true;
            } else if line.starts_with("locked") {
                locked = true;
            } else if line.starts_with("prunable") {
                prunable = true;
            }
        }
        let (Some(path), Some(head)) = (path, head) else {
            return Err(AikitError::new(
                "versioned_world.invalid_worktree_record",
                "git worktree record is missing path or HEAD",
            ));
        };
        result.push(GitWorktreeRelation {
            path,
            head,
            branch,
            detached,
            locked,
            prunable,
        });
    }
    Ok(result)
}

fn parse_history(raw: &str) -> Vec<VersionHistoryEntry> {
    raw.split('\u{1e}')
        .filter_map(|record| {
            let record = record.trim();
            if record.is_empty() {
                return None;
            }
            let mut fields = record.split('\u{1f}');
            let revision = VersionRevision::new(fields.next()?);
            let parents = fields
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .map(VersionRevision::new)
                .collect();
            let subject = fields.next().unwrap_or_default().to_string();
            let author = fields
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let authored_at = fields
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Some(VersionHistoryEntry {
                revision,
                parents,
                subject,
                author,
                authored_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn porcelain_parser_separates_working_states() {
        let state =
            parse_porcelain_v1_z(b"M  staged.rs\0 M unstaged.rs\0?? new.rs\0UU conflict.rs\0");
        assert_eq!(state.staged, vec!["staged.rs"]);
        assert_eq!(state.unstaged, vec!["unstaged.rs"]);
        assert_eq!(state.untracked, vec!["new.rs"]);
        assert_eq!(state.conflicted, vec!["conflict.rs"]);
    }

    #[test]
    fn native_git_reconciles_external_cli_and_manages_isolated_worktree() {
        let provider = NativeGitProvider::new().unwrap();
        if !matches!(
            provider.descriptor().status,
            VersionedWorldProviderStatus::Available
        ) {
            return;
        }
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("aikit-native-git-{}-{unique}", std::process::id()));
        let worktree = root.with_extension("worktree");
        fs::create_dir_all(&root).unwrap();
        run(&root, ["init", "-q"]);
        run(&root, ["config", "user.name", "AIKit Test"]);
        run(&root, ["config", "user.email", "aikit@example.invalid"]);
        fs::write(root.join("README.md"), "one\n").unwrap();
        run(&root, ["add", "README.md"]);
        run(&root, ["commit", "-qm", "initial"]);

        let project = ProjectRef::parse("project:test").unwrap();
        let root_str = root.to_string_lossy().to_string();
        let initial = provider.inspect(&project, &root_str).unwrap();
        assert!(initial.working.is_clean());
        let base = initial.repository.head.clone();

        fs::write(root.join("README.md"), "two\n").unwrap();
        let changed = provider.reconcile(&project, &root_str).unwrap();
        assert_eq!(changed.repository.head, base);
        assert_eq!(changed.working.unstaged, vec!["README.md"]);

        run(&root, ["add", "README.md"]);
        run(&root, ["commit", "-qm", "external change"]);
        let reconciled = provider.reconcile(&project, &root_str).unwrap();
        assert_ne!(reconciled.repository.head, base);
        assert!(reconciled.working.is_clean());

        let request = CreateWorktreeRequest {
            path: worktree.to_string_lossy().to_string(),
            base: reconciled.repository.head.clone(),
            branch: Some("agent/test-worktree".into()),
        };
        let isolated = provider
            .create_worktree(&project, &root_str, &request)
            .unwrap();
        assert_eq!(
            isolated.repository.branch.as_deref(),
            Some("agent/test-worktree")
        );
        assert_eq!(isolated.project, project);
        provider
            .remove_worktree(&project, &root_str, &request.path)
            .unwrap();
        assert!(!worktree.exists());

        let _ = fs::remove_dir_all(&root);
    }

    fn run<const N: usize>(cwd: &Path, args: [&str; N]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
