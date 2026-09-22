//! `aikit worktree project` — project repository checkouts onto origin/main.
//!
//! The git-repository projection reconciliation the owner otherwise runs by
//! hand (`git fetch` + `git reset --hard origin/main` + `git clean`, per repo,
//! per machine). AIKit owns the repository/worktree side of the suite, so this
//! is its command. The checkout roots are supplied by the caller — the O-I
//! dev-world resolver, or explicit `--repo` flags — never guessed here.
//!
//! Read-only by default; `--apply` performs only the one safe mutation
//! (fast-forwarding a clean, strictly-behind checkout). Dirty, ahead, and
//! diverged checkouts are surfaced untouched.

use aikit_adapters::NativeGitProvider;
use aikit_core::project::ProjectRef;
use aikit_core::resource::{
    ProjectionTarget, SuiteProjection, VersionedWorldProvider, VersionedWorldProviderStatus,
};
use aikit_core::{AikitError, Result};

use crate::cli::WorktreeProjectArgs;

/// Parse one `--repo` value into `(project, key, checkout_root)`.
///
/// `KEY=PATH` names both; a bare `PATH` derives the key from the directory name.
fn parse_repo(raw: &str) -> Result<(ProjectRef, String, String)> {
    let (key, path) = match raw.split_once('=') {
        Some((key, path)) => (key.trim().to_string(), path.trim().to_string()),
        None => {
            let path = raw.trim().to_string();
            let key = std::path::Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| path.clone());
            (key, path)
        }
    };
    if key.is_empty() || path.is_empty() {
        return Err(AikitError::new(
            "worktree.invalid_repo",
            format!("--repo expects KEY=PATH or a non-empty PATH; got {raw:?}"),
        ));
    }
    let project = ProjectRef::parse(&format!("project:{key}"))?;
    Ok((project, key, path))
}

/// Run the projection described by the CLI arguments and return the reading.
pub fn run(args: &WorktreeProjectArgs) -> Result<SuiteProjection> {
    let repos = args
        .repos
        .iter()
        .map(|raw| parse_repo(raw))
        .collect::<Result<Vec<_>>>()?;
    let target = ProjectionTarget::parse(&args.target);

    let provider = NativeGitProvider::new()?;
    if let VersionedWorldProviderStatus::Unavailable { reason } = provider.descriptor().status {
        return Err(AikitError::new(
            "worktree.git_unavailable",
            format!("git is required to project checkouts but is unavailable: {reason}"),
        ));
    }

    Ok(provider.project_suite(&repos, &target, args.apply, !args.no_fetch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repo_reads_key_and_path() {
        let (project, key, path) = parse_repo("ai-kit=/work/ai-kit").unwrap();
        assert_eq!(project.as_str(), "project:ai-kit");
        assert_eq!(key, "ai-kit");
        assert_eq!(path, "/work/ai-kit");
    }

    #[test]
    fn parse_repo_derives_key_from_a_bare_path() {
        let (project, key, path) = parse_repo("/Users/me/Central/Work/O-I").unwrap();
        assert_eq!(key, "O-I");
        assert_eq!(project.as_str(), "project:O-I");
        assert_eq!(path, "/Users/me/Central/Work/O-I");
    }

    #[test]
    fn parse_repo_rejects_an_empty_path() {
        assert!(parse_repo("key=").is_err());
        assert!(parse_repo("").is_err());
    }
}
