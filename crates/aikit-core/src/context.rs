//! Contexts.
//!
//! A context is the unit AIKit resolves for. One session space owns one context
//! per project/worktree/task overlay, so a session with three agent tasks has one
//! session id and four context ids.
//!
//! ## Isolation is a choice, not a default
//!
//! An agent task shares the session's working tree unless the user asks for
//! something else. Isolation buys a clean per-task client skill surface, and it
//! costs a checkout, a branch, disk, and a teardown decision. That trade is the
//! user's to make per task, so [`Isolation::Shared`] is the default and the more
//! isolated modes are opt-in. What AIKit owes the user in the shared case is
//! honesty: the client adapters must say plainly that a shared tree cannot give
//! two sibling tasks different native skill surfaces, and fall back rather than
//! pretend.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::id::{ContextId, ProjectId, SessionId};
use crate::platform::{MuxKind, Platform, TargetId};

/// How a task context relates to the session's working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum Isolation {
    /// The default. The context uses the session's working tree as-is.
    #[default]
    Shared,
    /// A dedicated directory that is not a git worktree (a scratch checkout, a
    /// copied tree, a subdirectory). Gives filesystem isolation without touching
    /// git state.
    Directory,
    /// A git worktree with its own branch. Opt-in.
    Worktree,
}

impl Isolation {
    pub fn as_str(self) -> &'static str {
        match self {
            Isolation::Shared => "shared",
            Isolation::Directory => "directory",
            Isolation::Worktree => "worktree",
        }
    }

    /// Does this context have a working tree that siblings cannot see?
    ///
    /// This is the question client adapters actually care about: it decides
    /// whether a project-local native skill directory can be per-task.
    pub fn is_isolated(self) -> bool {
        !matches!(self, Isolation::Shared)
    }

    /// Does teardown need to consider git state (dirty tree, unpushed commits)?
    pub fn owns_a_git_worktree(self) -> bool {
        matches!(self, Isolation::Worktree)
    }
}

/// Everything the resolver needs to know about *where* it is resolving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextDescriptor {
    pub context_id: ContextId,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    #[serde(default)]
    pub project_id: Option<ProjectId>,
    #[serde(default)]
    pub project_root: Option<PathBuf>,
    /// Set when this context is an agent task within a session.
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub isolation: Isolation,
    pub platform: Platform,
    /// The clients and surfaces this context projects into.
    pub targets: Vec<TargetId>,
    #[serde(default)]
    pub mux: Option<MuxKind>,
    /// Host identity, shown prominently when a session is presented remotely.
    #[serde(default)]
    pub host: String,
}

impl ContextDescriptor {
    /// A fresh context for a plain terminal in a project.
    pub fn for_project(project_root: impl Into<PathBuf>) -> Self {
        Self {
            context_id: ContextId::generate(),
            session_id: None,
            project_id: None,
            project_root: Some(project_root.into()),
            task: None,
            isolation: Isolation::default(),
            platform: Platform::current(),
            targets: vec![
                TargetId::shell(),
                TargetId::claude_code(),
                TargetId::codex(),
            ],
            mux: None,
            host: hostname(),
        }
    }

    pub fn is_task(&self) -> bool {
        self.task.is_some()
    }

    /// The scope AIKit should write to by default when the user toggles something.
    ///
    /// Inside an AIKit-bound session: the session overlay. Inside a project with
    /// no session: the private project-local file. Outside a project: global.
    pub fn default_mutation_scope(&self) -> crate::scope::ScopeKind {
        use crate::scope::ScopeKind;
        if self.task.is_some() {
            ScopeKind::Task
        } else if self.session_id.is_some() {
            ScopeKind::Session
        } else if self.project_root.is_some() {
            ScopeKind::ProjectLocal
        } else {
            ScopeKind::Global
        }
    }

    /// The scopes the palette's `Tab` key may cycle through here.
    pub fn permitted_scopes(&self) -> Vec<crate::scope::ScopeKind> {
        use crate::scope::ScopeKind;
        let mut scopes = vec![ScopeKind::Global];
        if !self.host.is_empty() {
            scopes.push(ScopeKind::Host);
        }
        if self.project_root.is_some() {
            scopes.push(ScopeKind::Project);
            scopes.push(ScopeKind::ProjectLocal);
        }
        if self.session_id.is_some() {
            scopes.push(ScopeKind::Session);
        }
        if self.task.is_some() {
            scopes.push(ScopeKind::Task);
        }
        scopes
    }

    /// A short label for the palette title bar.
    pub fn label(&self) -> String {
        let project = self
            .project_root
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "no project".to_string());
        match (&self.task, &self.session_id) {
            (Some(task), _) => format!("{project} · task: {task}"),
            (None, Some(_)) => format!("{project} · session"),
            (None, None) => project,
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "localhost".to_string())
}

/// The runtime binding between an AIKit context and a multiplexer location.
///
/// Multiplexer ids are useful bindings, not durable identity: a restored cmux
/// workspace or a restarted tmux server may hand back different ids for the same
/// human-meaningful session. AIKit's own ids remain authoritative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextBinding {
    pub context_id: ContextId,
    pub session_id: SessionId,
    pub mux: MuxKind,
    /// tmux session name, cmux workspace or workspace-group id.
    pub mux_session: Option<String>,
    /// tmux pane id, cmux surface id.
    pub mux_surface: Option<String>,
    pub project_root: Option<PathBuf>,
    pub isolation: Isolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_context_shares_the_working_tree_unless_asked_otherwise() {
        let d = ContextDescriptor::for_project("/work/payments");
        assert_eq!(d.isolation, Isolation::Shared);
        assert!(!d.isolation.is_isolated());
        assert!(!d.isolation.owns_a_git_worktree());
    }

    #[test]
    fn only_worktree_isolation_implies_git_teardown_checks() {
        assert!(Isolation::Worktree.owns_a_git_worktree());
        assert!(!Isolation::Directory.owns_a_git_worktree());
        assert!(Isolation::Directory.is_isolated());
    }

    #[test]
    fn the_default_mutation_scope_follows_where_the_user_is() {
        let mut d = ContextDescriptor::for_project("/work/payments");
        assert_eq!(
            d.default_mutation_scope(),
            crate::scope::ScopeKind::ProjectLocal
        );

        d.session_id = Some(SessionId::generate());
        assert_eq!(d.default_mutation_scope(), crate::scope::ScopeKind::Session);

        d.task = Some("migration-review".into());
        assert_eq!(d.default_mutation_scope(), crate::scope::ScopeKind::Task);

        let outside = ContextDescriptor {
            project_root: None,
            ..ContextDescriptor::for_project("/tmp")
        };
        assert_eq!(
            outside.default_mutation_scope(),
            crate::scope::ScopeKind::Global
        );
    }

    #[test]
    fn permitted_scopes_never_offer_a_scope_that_does_not_exist_here() {
        let outside = ContextDescriptor {
            project_root: None,
            session_id: None,
            task: None,
            host: String::new(),
            ..ContextDescriptor::for_project("/tmp")
        };
        assert_eq!(
            outside.permitted_scopes(),
            vec![crate::scope::ScopeKind::Global]
        );
    }

    #[test]
    fn the_palette_label_names_the_task_when_there_is_one() {
        let mut d = ContextDescriptor::for_project("/work/payments");
        assert_eq!(d.label(), "payments");
        d.session_id = Some(SessionId::generate());
        assert_eq!(d.label(), "payments · session");
        d.task = Some("migration-review".into());
        assert_eq!(d.label(), "payments · task: migration-review");
    }
}
