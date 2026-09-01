//! First-class Git disclosure for Git-shaped Project Worlds.
//!
//! These are observed material/version-control facts, not Project or World
//! identity. The I/O-free core owns the read model; adapters own observation.

use serde::{Deserialize, Serialize};

pub const GIT_PROJECT_OBSERVATION_VERSION: &str = "aikit.git-project-observation/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeDisclosure {
    pub path: String,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GitWorkingTreeDifference {
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
    pub conflicts: Vec<String>,
}

impl GitWorkingTreeDifference {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.unstaged.is_empty()
            && self.untracked.is_empty()
            && self.conflicts.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitProjectObservation {
    pub version: String,
    pub provider: String,
    pub repository_root: String,
    pub worktree: String,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    pub detached: bool,
    #[serde(default)]
    pub upstream: Option<String>,
    pub difference: GitWorkingTreeDifference,
    pub worktrees: Vec<GitWorktreeDisclosure>,
}

impl GitProjectObservation {
    pub fn is_clean(&self) -> bool {
        self.difference.is_clean()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_working_tree_is_not_collapsed_into_head_identity() {
        let observation = GitProjectObservation {
            version: GIT_PROJECT_OBSERVATION_VERSION.to_string(),
            provider: "git-cli/v1".to_string(),
            repository_root: "/work/project".to_string(),
            worktree: "/work/project".to_string(),
            head: Some("abc123".to_string()),
            branch: Some("main".to_string()),
            detached: false,
            upstream: None,
            difference: GitWorkingTreeDifference {
                unstaged: vec!["Ground.md".to_string()],
                ..GitWorkingTreeDifference::default()
            },
            worktrees: vec![],
        };

        assert_eq!(observation.head.as_deref(), Some("abc123"));
        assert!(!observation.is_clean());
    }
}
