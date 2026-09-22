//! Native Git provider for the versioned-World resource contract.
//!
//! Agents remain free to use ordinary Git CLI under their normal capability and
//! authority. This adapter gives AIKit a structured, reconciliable view of the
//! resulting repository/worktree state and a small high-value mutation floor.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use aikit_core::project::ProjectRef;
use aikit_core::resource::{
    decide, surface_reason, CreateWorktreeRequest, DevelopmentFieldCurrentDiff,
    DevelopmentFieldGitBasis, Divergence, GitRepositoryRelation, GitWorkingState,
    GitWorktreeRelation, ProjectionAction, ProjectionDecision, ProjectionTarget, ProviderRef,
    RepoProjection, SuiteProjection, VersionDiff, VersionDiffRequest, VersionHistoryEntry,
    VersionHistoryRequest, VersionRevision, VersionedProjectWorld, VersionedWorldCapability,
    VersionedWorldProvider, VersionedWorldProviderDescriptor, VersionedWorldProviderStatus,
    VERSIONED_WORLD_VERSION,
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

    /// Build the exact Git/VersionedWorld portion of a Development Field reading.
    ///
    /// The optional base is caller-owned Run/plan evidence. `git diff <base> --`
    /// compares that base against the current index + working tree, while untracked
    /// paths remain separately disclosed rather than having their contents silently
    /// promoted into tracked source.
    pub fn development_field_basis(
        &self,
        project: &ProjectRef,
        locator: &str,
        base_revision: Option<VersionRevision>,
        max_bytes: usize,
    ) -> Result<DevelopmentFieldGitBasis> {
        let world = self.inspect(project, locator)?;
        let current_diff_from_base = match base_revision.as_ref() {
            None => None,
            Some(base) => {
                let args = vec![
                    "diff".to_string(),
                    "--no-ext-diff".to_string(),
                    "--binary".to_string(),
                    base.as_str().to_string(),
                    "--".to_string(),
                ];
                let output = self.output(locator, args)?;
                if !output.status.success() {
                    return Err(git_failure(output));
                }
                let max = max_bytes.max(1);
                let truncated = output.stdout.len() > max;
                let bytes = &output.stdout[..output.stdout.len().min(max)];
                Some(DevelopmentFieldCurrentDiff {
                    base_revision: base.clone(),
                    observed_head: world.repository.head.clone(),
                    patch: String::from_utf8_lossy(bytes).to_string(),
                    truncated,
                    untracked_paths: world.working.untracked.clone(),
                })
            }
        };
        DevelopmentFieldGitBasis::new(world, base_revision, current_diff_from_base)
    }

    /// Fetch a remote so the projection target reflects canonical state. A
    /// failure here is surfaced by `project_worktree`, never fatal to the
    /// whole-suite reading — an offline machine still gets a stale-but-honest
    /// projection against whatever it last fetched.
    pub fn fetch(&self, locator: &str, remote: &str) -> Result<()> {
        let output = self.output(locator, ["fetch", "--quiet", remote])?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        Ok(())
    }

    /// Resolve the projection target to a commit, or `None` when the ref is not
    /// present here (unknown remote/branch, or never fetched).
    fn resolve_target(
        &self,
        locator: &str,
        target: &ProjectionTarget,
    ) -> Result<Option<VersionRevision>> {
        let commit = format!("{}^{{commit}}", target.qualified());
        Ok(self
            .optional(locator, ["rev-parse", "--verify", "--quiet", &commit])?
            .map(VersionRevision::new))
    }

    /// `HEAD`'s (ahead, behind) counts relative to the target: `git rev-list
    /// --left-right --count HEAD...<target>` reports left = commits on HEAD only
    /// (ahead), right = commits on the target only (behind).
    fn ahead_behind(&self, locator: &str, target_qualified: &str) -> Result<(u64, u64)> {
        let spec = format!("HEAD...{target_qualified}");
        let value = self.checked(locator, ["rev-list", "--left-right", "--count", &spec])?;
        Ok(parse_ahead_behind(&value).unwrap_or((0, 0)))
    }

    /// The one allowed mutation: fast-forward HEAD to the target. `--ff-only`
    /// changes nothing and returns non-zero unless the move is a true
    /// fast-forward, so a change that landed since the read cannot cause a
    /// clobber. Works on a detached HEAD (the whole-suite worktree case),
    /// advancing it without leaving detached state.
    fn fast_forward(&self, locator: &str, target_qualified: &str) -> Result<()> {
        let output = self.output(locator, ["merge", "--ff-only", target_qualified])?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        Ok(())
    }

    /// Project one checkout onto the target, safely.
    ///
    /// Read-only unless `apply`; even then the only mutation is a fast-forward
    /// of a **clean**, strictly-behind checkout. A dirty, ahead, or diverged
    /// checkout is always surfaced untouched — projection never discards
    /// uncommitted or unmerged work to force the target.
    pub fn project_worktree(
        &self,
        project: &ProjectRef,
        key: &str,
        locator: &str,
        target: &ProjectionTarget,
        apply: bool,
        fetch: bool,
    ) -> Result<RepoProjection> {
        let fetch_error = if fetch {
            self.fetch(locator, &target.remote).err()
        } else {
            None
        };
        let mut head = VersionRevision::new(self.checked(locator, ["rev-parse", "HEAD"])?);
        let branch = self.optional(locator, ["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        let detached = branch.is_none();
        let clean = self.working_state(locator)?.is_clean();
        let qualified = target.qualified();
        let target_revision = self.resolve_target(locator, target)?;

        let (divergence, action) = match &target_revision {
            None => {
                let mut reason = surface_reason(&Divergence::TargetMissing, clean, &qualified);
                if let Some(error) = &fetch_error {
                    reason = format!("{reason} (git fetch failed: {})", error.message());
                }
                (
                    Divergence::TargetMissing,
                    ProjectionAction::Surfaced { reason },
                )
            }
            Some(_) => {
                let (ahead, behind) = self.ahead_behind(locator, &qualified)?;
                let divergence = Divergence::classify(true, ahead, behind);
                let action = match decide(&divergence, clean) {
                    ProjectionDecision::AlreadyProjected => ProjectionAction::AlreadyProjected,
                    ProjectionDecision::Surface => ProjectionAction::Surfaced {
                        reason: surface_reason(&divergence, clean, &qualified),
                    },
                    ProjectionDecision::FastForward if apply => {
                        let from = head.clone();
                        match self.fast_forward(locator, &qualified) {
                            Ok(()) => {
                                head = VersionRevision::new(
                                    self.checked(locator, ["rev-parse", "HEAD"])?,
                                );
                                ProjectionAction::FastForwarded {
                                    from,
                                    to: head.clone(),
                                }
                            }
                            Err(error) => ProjectionAction::Failed {
                                reason: format!(
                                    "fast-forward to {qualified} failed: {}",
                                    error.message()
                                ),
                            },
                        }
                    }
                    ProjectionDecision::FastForward => ProjectionAction::WouldFastForward {
                        to: target_revision.clone().unwrap_or_else(|| head.clone()),
                    },
                };
                (divergence, action)
            }
        };

        Ok(RepoProjection {
            key: key.to_string(),
            project: project.clone(),
            locator: locator.to_string(),
            target: qualified,
            head,
            target_revision,
            branch,
            detached,
            clean,
            divergence,
            action,
        })
    }

    /// Project a whole set of checkouts onto the target in one pass.
    ///
    /// Each entry is `(project, key, checkout_root)`. One checkout that cannot
    /// even be read (not a git repository, empty, unreadable) is recorded as a
    /// `Failed` entry with the reason attached — it never aborts the projection
    /// of the others.
    pub fn project_suite(
        &self,
        repos: &[(ProjectRef, String, String)],
        target: &ProjectionTarget,
        apply: bool,
        fetch: bool,
    ) -> SuiteProjection {
        let entries = repos
            .iter()
            .map(|(project, key, locator)| {
                self.project_worktree(project, key, locator, target, apply, fetch)
                    .unwrap_or_else(|error| RepoProjection {
                        key: key.clone(),
                        project: project.clone(),
                        locator: locator.clone(),
                        target: target.qualified(),
                        head: VersionRevision::new("unknown"),
                        target_revision: None,
                        branch: None,
                        detached: false,
                        clean: false,
                        divergence: Divergence::Unknown,
                        action: ProjectionAction::Failed {
                            reason: error.message().to_string(),
                        },
                    })
            })
            .collect();
        SuiteProjection::new(target.qualified(), apply, entries)
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

    #[test]
    fn development_field_basis_includes_current_tracked_difference_and_names_untracked_paths() {
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
        let root = std::env::temp_dir().join(format!(
            "aikit-development-field-git-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run(&root, ["init", "-q"]);
        run(&root, ["config", "user.name", "AIKit Test"]);
        run(&root, ["config", "user.email", "aikit@example.invalid"]);
        fs::write(root.join("README.md"), "one\n").unwrap();
        run(&root, ["add", "README.md"]);
        run(&root, ["commit", "-qm", "initial"]);

        let project = ProjectRef::parse("project:development-field").unwrap();
        let locator = root.to_string_lossy().to_string();
        let base = provider
            .inspect(&project, &locator)
            .unwrap()
            .repository
            .head;
        fs::write(root.join("README.md"), "two\n").unwrap();
        fs::write(root.join("untracked.txt"), "not source until Git says so\n").unwrap();

        let basis = provider
            .development_field_basis(&project, &locator, Some(base.clone()), 64 * 1024)
            .unwrap();
        let diff = basis.current_diff_from_base.unwrap();
        assert_eq!(diff.base_revision, base);
        assert_eq!(diff.observed_head, basis.world.repository.head);
        assert!(diff.patch.contains("+two"), "{}", diff.patch);
        assert_eq!(diff.untracked_paths, vec!["untracked.txt"]);

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

    // ---- Worktree projection (project-to-origin/main) integration ----

    fn git_available() -> bool {
        matches!(
            NativeGitProvider::new().unwrap().descriptor().status,
            VersionedWorldProviderStatus::Available
        )
    }

    fn projection_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("aikit-projection-{}-{unique}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn config_identity(repo: &Path) {
        run(repo, ["config", "user.name", "AIKit Test"]);
        run(repo, ["config", "user.email", "aikit@example.invalid"]);
    }

    /// A bare `origin` seeded with one `main` commit, plus a working clone whose
    /// HEAD sits exactly on `origin/main`.
    fn origin_with_clone(root: &Path) -> (PathBuf, PathBuf) {
        fs::create_dir_all(root).unwrap();
        let origin = root.join("origin.git");
        run(root, ["init", "-q", "-b", "main", "--bare", "origin.git"]);
        let seed = root.join("seed");
        run(
            root,
            [
                "clone",
                "-q",
                origin.to_str().unwrap(),
                seed.to_str().unwrap(),
            ],
        );
        config_identity(&seed);
        fs::write(seed.join("README.md"), "one\n").unwrap();
        run(&seed, ["add", "README.md"]);
        run(&seed, ["commit", "-qm", "c1"]);
        run(&seed, ["push", "-q", "-u", "origin", "main"]);

        let work = root.join("work");
        run(
            root,
            [
                "clone",
                "-q",
                origin.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        config_identity(&work);
        (origin, work)
    }

    /// Advance `origin/main` by one commit, via a throwaway clone.
    fn advance_origin(root: &Path, origin: &Path) {
        let mover = root.join(format!("mover-{}", std::process::id()));
        run(
            root,
            [
                "clone",
                "-q",
                origin.to_str().unwrap(),
                mover.to_str().unwrap(),
            ],
        );
        config_identity(&mover);
        fs::write(mover.join("README.md"), "one\ntwo\n").unwrap();
        run(&mover, ["commit", "-qam", "c2"]);
        run(&mover, ["push", "-q", "origin", "main"]);
        fs::remove_dir_all(&mover).unwrap();
    }

    fn project(work: &Path, apply: bool) -> RepoProjection {
        let provider = NativeGitProvider::new().unwrap();
        provider
            .project_worktree(
                &ProjectRef::parse("project:test").unwrap(),
                "test",
                &work.to_string_lossy(),
                &ProjectionTarget::default(),
                apply,
                true,
            )
            .unwrap()
    }

    fn head_of(repo: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn observe_reports_a_behind_checkout_without_touching_it() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (origin, work) = origin_with_clone(&root);
        let before = head_of(&work);
        advance_origin(&root, &origin);

        let projection = project(&work, false);
        assert_eq!(projection.divergence, Divergence::Behind { by: 1 });
        assert!(matches!(
            projection.action,
            ProjectionAction::WouldFastForward { .. }
        ));
        // Observe mode never moves HEAD.
        assert_eq!(head_of(&work), before, "observe must not move HEAD");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_fast_forwards_a_clean_behind_checkout() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (origin, work) = origin_with_clone(&root);
        advance_origin(&root, &origin);

        let projection = project(&work, true);
        assert_eq!(projection.divergence, Divergence::Behind { by: 1 });
        match &projection.action {
            ProjectionAction::FastForwarded { to, .. } => {
                assert_eq!(head_of(&work), to.as_str(), "HEAD must land on the target");
            }
            other => panic!("expected fast-forward, got {other:?}"),
        }
        // The projected HEAD is exactly origin/main.
        let origin_main = {
            let output = Command::new("git")
                .arg("-C")
                .arg(&work)
                .args(["rev-parse", "origin/main"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        assert_eq!(head_of(&work), origin_main);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_fast_forwards_a_detached_head_and_stays_detached() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (origin, work) = origin_with_clone(&root);
        // Detach HEAD, exactly as the whole-suite dev worktrees are.
        run(&work, ["checkout", "-q", "--detach", "HEAD"]);
        advance_origin(&root, &origin);

        let projection = project(&work, true);
        assert!(projection.detached, "the checkout began detached");
        assert!(matches!(
            projection.action,
            ProjectionAction::FastForwarded { .. }
        ));
        // Still detached after the fast-forward (no branch was created).
        let symbolic = Command::new("git")
            .arg("-C")
            .arg(&work)
            .args(["symbolic-ref", "--quiet", "HEAD"])
            .status()
            .unwrap();
        assert!(
            !symbolic.success(),
            "HEAD must remain detached after projection"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_dirty_behind_checkout_is_surfaced_and_never_reset() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (origin, work) = origin_with_clone(&root);
        advance_origin(&root, &origin);
        // Uncommitted local edit: the case a `reset --hard` would destroy.
        fs::write(work.join("README.md"), "one\nLOCAL WORK\n").unwrap();
        let before = head_of(&work);

        let projection = project(&work, true);
        assert!(!projection.clean);
        match &projection.action {
            ProjectionAction::Surfaced { reason } => {
                assert!(reason.contains("uncommitted"), "reason: {reason}");
            }
            other => panic!("dirty-behind must be surfaced, got {other:?}"),
        }
        // The work is untouched: HEAD unmoved and the local edit intact.
        assert_eq!(head_of(&work), before, "a dirty tree must never be moved");
        assert_eq!(
            fs::read_to_string(work.join("README.md")).unwrap(),
            "one\nLOCAL WORK\n",
            "uncommitted work must be preserved byte for byte"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_checkout_with_local_commits_is_surfaced_not_discarded() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (_origin, work) = origin_with_clone(&root);
        // A committed local advance that is not on origin/main.
        fs::write(work.join("feature.rs").as_path(), "// local\n").unwrap();
        run(&work, ["add", "feature.rs"]);
        run(&work, ["commit", "-qm", "local feature"]);
        let before = head_of(&work);

        let projection = project(&work, true);
        assert_eq!(projection.divergence, Divergence::Ahead { by: 1 });
        assert!(matches!(
            projection.action,
            ProjectionAction::Surfaced { .. }
        ));
        assert_eq!(
            head_of(&work),
            before,
            "a local commit must not be discarded"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_diverged_checkout_is_surfaced_not_rewritten() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (origin, work) = origin_with_clone(&root);
        // Local history and remote history both move: divergence.
        fs::write(work.join("local.rs"), "// local\n").unwrap();
        run(&work, ["add", "local.rs"]);
        run(&work, ["commit", "-qm", "local"]);
        advance_origin(&root, &origin);
        let before = head_of(&work);

        let projection = project(&work, true);
        assert!(matches!(projection.divergence, Divergence::Diverged { .. }));
        assert!(matches!(
            projection.action,
            ProjectionAction::Surfaced { .. }
        ));
        assert_eq!(
            head_of(&work),
            before,
            "diverged history must not be rewritten"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_up_to_date_checkout_needs_no_action() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (_origin, work) = origin_with_clone(&root);
        let projection = project(&work, true);
        assert_eq!(projection.divergence, Divergence::UpToDate);
        assert_eq!(projection.action, ProjectionAction::AlreadyProjected);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn project_suite_reports_each_repo_and_never_aborts_on_a_bad_root() {
        if !git_available() {
            return;
        }
        let root = projection_root();
        let (origin_a, behind) = origin_with_clone(&root.join("a"));
        advance_origin(&root.join("a"), &origin_a);
        let (_origin_b, current) = origin_with_clone(&root.join("b"));
        // A path that is not a git checkout at all.
        let not_a_repo = root.join("not-a-repo");
        fs::create_dir_all(&not_a_repo).unwrap();

        let provider = NativeGitProvider::new().unwrap();
        let repos = vec![
            (
                ProjectRef::parse("project:behind").unwrap(),
                "behind".to_string(),
                behind.to_string_lossy().to_string(),
            ),
            (
                ProjectRef::parse("project:current").unwrap(),
                "current".to_string(),
                current.to_string_lossy().to_string(),
            ),
            (
                ProjectRef::parse("project:broken").unwrap(),
                "broken".to_string(),
                not_a_repo.to_string_lossy().to_string(),
            ),
        ];
        let suite = provider.project_suite(&repos, &ProjectionTarget::default(), true, true);

        assert_eq!(suite.entries.len(), 3);
        let behind_entry = suite.entries.iter().find(|e| e.key == "behind").unwrap();
        assert!(matches!(
            behind_entry.action,
            ProjectionAction::FastForwarded { .. }
        ));
        let current_entry = suite.entries.iter().find(|e| e.key == "current").unwrap();
        assert_eq!(current_entry.action, ProjectionAction::AlreadyProjected);
        let broken_entry = suite.entries.iter().find(|e| e.key == "broken").unwrap();
        assert!(matches!(
            broken_entry.action,
            ProjectionAction::Failed { .. }
        ));
        assert_eq!(broken_entry.divergence, Divergence::Unknown);

        // Two of three now sit on the target; the broken one still needs a human.
        assert_eq!(suite.projected_count(), 2);
        assert_eq!(suite.attention().len(), 1);
        assert!(!suite.all_projected());

        let _ = fs::remove_dir_all(&root);
    }
}
