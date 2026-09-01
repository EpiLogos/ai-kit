//! Versioned-World contracts for native revision/difference/history providers.
//!
//! The semantic subject remains owned by its native product. This module only
//! describes an optional temporal/provider relation around a Project World; a
//! repository, worktree, branch or commit never becomes Project/World identity.

use serde::{Deserialize, Serialize};

use crate::project::ProjectRef;
use crate::resource::ProviderRef;

pub const VERSIONED_WORLD_VERSION: &str = "aikit.versioned-world/v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionRevision(String);

impl VersionRevision {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionedWorldCapability {
    Inspect,
    Reconcile,
    Diff,
    History,
    Worktrees,
    CreateWorktree,
    RemoveWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum VersionedWorldProviderStatus {
    Available,
    Degraded { reason: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedWorldProviderDescriptor {
    pub provider: ProviderRef,
    pub status: VersionedWorldProviderStatus,
    pub capabilities: Vec<VersionedWorldCapability>,
    #[serde(default)]
    pub implementation_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRepositoryRelation {
    pub repository_root: String,
    pub worktree_root: String,
    pub head: VersionRevision,
    #[serde(default)]
    pub branch: Option<String>,
    pub detached: bool,
    #[serde(default)]
    pub upstream: Option<String>,
    #[serde(default)]
    pub ahead: u64,
    #[serde(default)]
    pub behind: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GitWorkingState {
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
    pub conflicted: Vec<String>,
}

impl GitWorkingState {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.unstaged.is_empty()
            && self.untracked.is_empty()
            && self.conflicted.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeRelation {
    pub path: String,
    pub head: VersionRevision,
    #[serde(default)]
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedProjectWorld {
    pub version: String,
    pub project: ProjectRef,
    pub provider: VersionedWorldProviderDescriptor,
    pub repository: GitRepositoryRelation,
    pub working: GitWorkingState,
    #[serde(default)]
    pub worktrees: Vec<GitWorktreeRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionDiffRequest {
    pub from: VersionRevision,
    pub to: VersionRevision,
    #[serde(default)]
    pub path: Option<String>,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionDiff {
    pub from: VersionRevision,
    pub to: VersionRevision,
    #[serde(default)]
    pub path: Option<String>,
    pub patch: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionHistoryEntry {
    pub revision: VersionRevision,
    pub parents: Vec<VersionRevision>,
    pub subject: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub authored_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionHistoryRequest {
    pub limit: usize,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorktreeRequest {
    pub path: String,
    pub base: VersionRevision,
    #[serde(default)]
    pub branch: Option<String>,
}

/// I/O-free application contract. Native adapters own process/filesystem mechanics.
pub trait VersionedWorldProvider {
    fn descriptor(&self) -> VersionedWorldProviderDescriptor;

    fn inspect(&self, project: &ProjectRef, locator: &str) -> crate::Result<VersionedProjectWorld>;

    fn reconcile(
        &self,
        project: &ProjectRef,
        locator: &str,
    ) -> crate::Result<VersionedProjectWorld> {
        self.inspect(project, locator)
    }

    fn diff(&self, locator: &str, request: &VersionDiffRequest) -> crate::Result<VersionDiff>;

    fn history(
        &self,
        locator: &str,
        request: &VersionHistoryRequest,
    ) -> crate::Result<Vec<VersionHistoryEntry>>;

    fn create_worktree(
        &self,
        project: &ProjectRef,
        locator: &str,
        request: &CreateWorktreeRequest,
    ) -> crate::Result<VersionedProjectWorld>;

    fn remove_worktree(
        &self,
        project: &ProjectRef,
        locator: &str,
        worktree_path: &str,
    ) -> crate::Result<VersionedProjectWorld>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_state_cleanliness_is_explicit() {
        let mut state = GitWorkingState::default();
        assert!(state.is_clean());
        state.untracked.push("notes.md".into());
        assert!(!state.is_clean());
    }
}
