//! Agent client adapters.
//!
//! A client adapter is a [`TargetAdapter`] that also knows two things the store
//! cannot: how to *launch* the client against a context's projection, and what
//! durable configuration the client needs so AIKit's hook dispatcher is reached.
//!
//! ## Two projections, two very different shapes
//!
//! Claude Code takes an arbitrary extra directory, so its projection lives in the
//! generation and two sessions in one checkout can differ. Codex discovers
//! `.agents/skills` by walking up from the working directory, so its projection
//! is a property of the *tree* — and two shared-tree tasks would overwrite each
//! other's. That asymmetry is not an implementation detail to paper over; it is
//! the reason [`aikit_core::context::Isolation`] exists, and each adapter answers
//! it explicitly rather than by accident.
//!
//! The V2 managed actor bootstrap follows the same rule. Both clients receive the
//! same generated Agent Skill representation of the already-resolved bootstrap;
//! the adapter is still responsible for proving that its native skill surface is
//! private enough for the context-specific seed.
//!
//! ## Why `install` returns a `Result`
//!
//! Merging into a client's existing settings requires reading them, and reading
//! can fail. Treating an unreadable settings file as "there are no settings"
//! would silently destroy the user's configuration on the next write, so the
//! failure is propagated instead of swallowed.

pub mod agent_skills;
mod bootstrap;
pub mod broker;
pub mod claude;
pub mod codex;

use std::path::Path;

use aikit_core::projection::{ProjectionItem, ResolvedContext, TargetAdapter};
use aikit_core::Result;

/// A projection target that is also a launchable agent client.
pub trait ClientAdapter: TargetAdapter {
    /// The argv that starts this client against the context's projection.
    fn launch_command(&self, context: &ResolvedContext) -> Vec<String>;

    /// The durable configuration entries that route the client's events into
    /// `aikit hook dispatch`.
    ///
    /// Returns projection items rather than writing them: the caller decides when
    /// anything lands on disk, and a plan can be shown before it is applied.
    fn install(&self, config_dir: &Path) -> Result<Vec<ProjectionItem>>;
}
