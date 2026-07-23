//! Declared effects.
//!
//! These declarations drive preview text and confirmation prompts. They are
//! *claims*, not enforcement: they do not replace review or sandboxing. The point
//! is that a user can see, before activation, that a capsule says it will write
//! outside the project or touch credentials.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A coarse, machine-readable effect class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectClass {
    ReadProject,
    WriteProject,
    ReadOutsideProject,
    WriteOutsideProject,
    Network,
    Subprocess,
    CredentialAccess,
    ProcessControl,
    GitHistoryRewrite,
    MultiplexerControl,
}

impl EffectClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadProject => "read-project",
            Self::WriteProject => "write-project",
            Self::ReadOutsideProject => "read-outside-project",
            Self::WriteOutsideProject => "write-outside-project",
            Self::Network => "network",
            Self::Subprocess => "subprocess",
            Self::CredentialAccess => "credential-access",
            Self::ProcessControl => "process-control",
            Self::GitHistoryRewrite => "git-history-rewrite",
            Self::MultiplexerControl => "multiplexer-control",
        }
    }

    /// Effects that warrant an explicit confirmation even for a reviewed capsule.
    pub fn is_elevated(self) -> bool {
        matches!(
            self,
            Self::WriteOutsideProject
                | Self::CredentialAccess
                | Self::GitHistoryRewrite
                | Self::ProcessControl
        )
    }

    /// Short human label used in the palette preview pane.
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadProject => "read project",
            Self::WriteProject => "write project",
            Self::ReadOutsideProject => "read outside project",
            Self::WriteOutsideProject => "write outside project",
            Self::Network => "network",
            Self::Subprocess => "subprocess",
            Self::CredentialAccess => "credentials",
            Self::ProcessControl => "process control",
            Self::GitHistoryRewrite => "rewrite git history",
            Self::MultiplexerControl => "multiplexer control",
        }
    }
}

/// The `[effects]` manifest table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    /// Entries of the form `read:project`, `write:home`, `write:outside`.
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub subprocess: bool,
    /// Named credential scopes the capsule expects to read.
    #[serde(default)]
    pub credentials: Vec<String>,
    #[serde(default)]
    pub process_control: bool,
    #[serde(default)]
    pub git_history_rewrite: bool,
    #[serde(default)]
    pub multiplexer_control: bool,
}

impl Effects {
    /// Normalize declarations into the closed set of effect classes.
    ///
    /// Unknown filesystem scopes are treated conservatively: anything that is not
    /// explicitly `project` is assumed to reach outside the project.
    pub fn classes(&self) -> BTreeSet<EffectClass> {
        let mut out = BTreeSet::new();
        for entry in &self.filesystem {
            let (verb, scope) = entry.split_once(':').unwrap_or((entry.as_str(), "outside"));
            let inside = matches!(scope, "project" | "worktree" | "task");
            match (verb, inside) {
                ("read", true) => out.insert(EffectClass::ReadProject),
                ("read", false) => out.insert(EffectClass::ReadOutsideProject),
                ("write", true) => out.insert(EffectClass::WriteProject),
                _ => out.insert(EffectClass::WriteOutsideProject),
            };
        }
        if self.network {
            out.insert(EffectClass::Network);
        }
        if self.subprocess {
            out.insert(EffectClass::Subprocess);
        }
        if !self.credentials.is_empty() {
            out.insert(EffectClass::CredentialAccess);
        }
        if self.process_control {
            out.insert(EffectClass::ProcessControl);
        }
        if self.git_history_rewrite {
            out.insert(EffectClass::GitHistoryRewrite);
        }
        if self.multiplexer_control {
            out.insert(EffectClass::MultiplexerControl);
        }
        out
    }

    pub fn has_elevated(&self) -> bool {
        self.classes().iter().any(|c| c.is_elevated())
    }

    /// Comma-joined labels for the preview pane.
    pub fn summary(&self) -> String {
        let classes = self.classes();
        if classes.is_empty() {
            return "none declared".to_string();
        }
        classes
            .iter()
            .map(|c| c.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effects(fs: &[&str]) -> Effects {
        Effects {
            filesystem: fs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn unqualified_filesystem_scopes_are_treated_as_outside_the_project() {
        let classes = effects(&["write:home"]).classes();
        assert!(classes.contains(&EffectClass::WriteOutsideProject));
        assert!(!classes.contains(&EffectClass::WriteProject));
    }

    #[test]
    fn a_bare_verb_without_a_scope_is_not_silently_treated_as_project_scoped() {
        let classes = effects(&["write"]).classes();
        assert!(classes.contains(&EffectClass::WriteOutsideProject));
    }

    #[test]
    fn worktree_scope_counts_as_inside_the_project() {
        let classes = effects(&["write:worktree"]).classes();
        assert!(classes.contains(&EffectClass::WriteProject));
    }

    #[test]
    fn credentials_imply_credential_access() {
        let e = Effects {
            credentials: vec!["gh-token".into()],
            ..Default::default()
        };
        assert!(e.classes().contains(&EffectClass::CredentialAccess));
        assert!(e.has_elevated());
    }

    #[test]
    fn empty_effects_summarize_readably() {
        assert_eq!(Effects::default().summary(), "none declared");
    }
}
