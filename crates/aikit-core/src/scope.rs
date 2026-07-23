//! Scope layers.
//!
//! The precedence order is the specification's, and it is the reason AIKit can
//! offer a low-friction session overlay without endangering the project baseline:
//!
//! ```text
//! managed policy                      (not a normal layer — see `policy`)
//!   user / global profile
//!   host-local profile
//!   project shared profile            (repository root → working directory)
//!   project-local private profile
//!   session overlay
//!   task / pane overlay
//!   one-shot invocation override
//! ```

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{err, AikitError, Result};
use crate::profile::PoolPatch;

/// A normal (overridable) scope layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeKind {
    Global,
    Host,
    Project,
    ProjectLocal,
    Session,
    Task,
    OneShot,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 7] = [
        ScopeKind::Global,
        ScopeKind::Host,
        ScopeKind::Project,
        ScopeKind::ProjectLocal,
        ScopeKind::Session,
        ScopeKind::Task,
        ScopeKind::OneShot,
    ];

    /// Lower rank applies first and is overridable by higher ranks.
    pub fn rank(self) -> u8 {
        match self {
            ScopeKind::Global => 0,
            ScopeKind::Host => 1,
            ScopeKind::Project => 2,
            ScopeKind::ProjectLocal => 3,
            ScopeKind::Session => 4,
            ScopeKind::Task => 5,
            ScopeKind::OneShot => 6,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Global => "global",
            ScopeKind::Host => "host",
            ScopeKind::Project => "project",
            ScopeKind::ProjectLocal => "project-local",
            ScopeKind::Session => "session",
            ScopeKind::Task => "task",
            ScopeKind::OneShot => "one-shot",
        }
    }

    /// The single-letter badge shown in the palette's scope column.
    pub fn badge(self) -> char {
        match self {
            ScopeKind::Global => 'G',
            ScopeKind::Host => 'H',
            ScopeKind::Project => 'P',
            ScopeKind::ProjectLocal => 'L',
            ScopeKind::Session => 'S',
            ScopeKind::Task => 'T',
            ScopeKind::OneShot => '1',
        }
    }

    /// Scopes a user may pick as the mutation target with `Tab`.
    ///
    /// One-shot is excluded: it is an invocation flag, not a place to write.
    pub fn is_mutation_target(self) -> bool {
        !matches!(self, ScopeKind::OneShot)
    }

    /// Writing a committed project profile is a shared, reviewable act and must
    /// therefore be confirmed. Session and project-local are deliberately cheap.
    pub fn requires_confirmation_to_write(self) -> bool {
        matches!(self, ScopeKind::Project | ScopeKind::Global)
    }

    /// Whether a change at this scope can outlive the current session.
    pub fn is_durable(self) -> bool {
        matches!(
            self,
            ScopeKind::Global | ScopeKind::Host | ScopeKind::Project | ScopeKind::ProjectLocal
        )
    }
}

impl fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScopeKind {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "global" | "user" => ScopeKind::Global,
            "host" => ScopeKind::Host,
            "project" => ScopeKind::Project,
            "project-local" | "local" => ScopeKind::ProjectLocal,
            "session" => ScopeKind::Session,
            "task" => ScopeKind::Task,
            "one-shot" | "once" => ScopeKind::OneShot,
            other => return err("scope.unknown", format!("`{other}` is not a scope")),
        })
    }
}

/// Where a layer came from, precise enough to point a user at a file and line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerOrigin {
    pub label: String,
    #[serde(default)]
    pub line: Option<u32>,
}

impl LayerOrigin {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            line: None,
        }
    }

    pub fn at_line(label: impl Into<String>, line: u32) -> Self {
        Self {
            label: label.into(),
            line: Some(line),
        }
    }
}

impl fmt::Display for LayerOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{}", self.label, line),
            None => f.write_str(&self.label),
        }
    }
}

/// One layer of the scope chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeLayer {
    pub kind: ScopeKind,
    /// Position within a kind. Used for nested project profiles: the repository
    /// root is depth 0 and each directory towards the working directory increases
    /// it, so a package profile overrides the repository baseline.
    #[serde(default)]
    pub depth: u16,
    pub origin: LayerOrigin,
    pub patch: PoolPatch,
}

impl ScopeLayer {
    pub fn new(kind: ScopeKind, origin: LayerOrigin, patch: PoolPatch) -> Self {
        Self {
            kind,
            depth: 0,
            origin,
            patch,
        }
    }

    /// The key layers are sorted by before resolution, so that supplying them in
    /// any order produces the same effective view.
    pub fn precedence_key(&self, insertion_index: usize) -> (u8, u16, usize) {
        (self.kind.rank(), self.depth, insertion_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_runs_from_global_up_to_one_shot() {
        let mut kinds = ScopeKind::ALL;
        kinds.sort_by_key(|k| k.rank());
        assert_eq!(kinds[0], ScopeKind::Global);
        assert_eq!(kinds[kinds.len() - 1], ScopeKind::OneShot);
    }

    #[test]
    fn session_and_project_local_are_low_friction_but_project_is_not() {
        assert!(!ScopeKind::Session.requires_confirmation_to_write());
        assert!(!ScopeKind::ProjectLocal.requires_confirmation_to_write());
        assert!(ScopeKind::Project.requires_confirmation_to_write());
        assert!(ScopeKind::Global.requires_confirmation_to_write());
    }

    #[test]
    fn one_shot_is_not_a_place_to_write_changes() {
        assert!(!ScopeKind::OneShot.is_mutation_target());
        assert!(ScopeKind::Session.is_mutation_target());
    }

    #[test]
    fn badges_are_unique_so_the_palette_column_is_unambiguous() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in ScopeKind::ALL {
            assert!(seen.insert(kind.badge()), "duplicate badge for {kind}");
        }
    }

    #[test]
    fn scope_names_round_trip() {
        for kind in ScopeKind::ALL {
            assert_eq!(kind.as_str().parse::<ScopeKind>().unwrap(), kind);
        }
    }

    #[test]
    fn origins_render_with_a_line_when_one_is_known() {
        assert_eq!(
            LayerOrigin::at_line("~/.aikit/state/sessions/ses_42/overlay.toml", 9).to_string(),
            "~/.aikit/state/sessions/ses_42/overlay.toml:9"
        );
    }
}
