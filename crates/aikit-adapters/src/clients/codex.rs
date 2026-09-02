//! Codex — and the shared-tree problem, which is the sharpest correctness point
//! in the whole adapter layer.
//!
//! Codex discovers `.agents/skills` by walking from the working directory up to
//! the repository root. There is no per-session flag that changes where it looks.
//! Two Codex sessions in the **same tree** therefore see the **same skills**, and
//! no amount of AIKit bookkeeping can make that untrue.
//!
//! ## What follows from that
//!
//! When the task has its own tree ([`Isolation::is_isolated`]), the projection is
//! genuinely per-task and everything is written, including the V2 managed actor
//! bootstrap when one is present.
//!
//! When the task shares the session's tree — **the default**, because a worktree
//! costs a checkout, a branch, disk and a teardown decision that belongs to the
//! user — writing a per-task `.agents/skills` would change what a *sibling* task
//! sees. The sibling would not be told, and the symptom would be an agent
//! behaving as though it had skills nobody gave it. So this adapter does not do
//! that. It falls back, in this order, and reports which rung it landed on:
//!
//! 1. **project-stable native skills only.** Skills enabled at project scope or
//!    above are the tree's normal contents; every sibling task sees them anyway,
//!    so writing them changes nothing observable. Session-only deltas and the
//!    context-specific actor bootstrap are *not* projected.
//! 2. **brokered.** Nothing is written; capabilities and actor disclosure are
//!    reached through AIKit's broker/application surfaces.
//! 3. **an explicitly accepted shared projection.** Capability skills may be
//!    shared, but the actor bootstrap remains brokered because its Run/session/body
//!    identity is not a project-stable tree property.
//!
//! ## Two things this adapter will never do
//!
//! It never invents a synthetic `HOME` to fake per-task isolation: a fabricated
//! home silently redirects git config, ssh config and stored credentials, and the
//! damage surfaces long after the shortcut was taken.
//!
//! It never reports a fallback as `Immediate` merely because the files on disk
//! did not change. A no-op apply in the shared case still leaves the session
//! deltas or actor bootstrap outside Codex, and saying otherwise would tell the
//! user their projection worked when part of it was withheld.
//!
//! [`Isolation::is_isolated`]: aikit_core::context::Isolation::is_isolated

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::capsule::Kind;
use aikit_core::context::Isolation;
use aikit_core::id::CapsuleId;
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, MaterializationMode, ProjectionItem, ProjectionPlan, ResolvedContext,
    TargetAdapter, TargetCapabilities,
};
use aikit_core::resolve::{ActiveCapability, SelectionOrigin};
use aikit_core::scope::ScopeKind;
use aikit_core::{AikitError, Result};

use super::agent_skills;
use super::bootstrap;
use super::ClientAdapter;

pub const CLIENT: &str = "codex";

/// The events AIKit installs a durable dispatcher entry for.
pub const DISPATCH_EVENTS: [&str; 6] = [
    "PreToolUse",
    "PostToolUse",
    "UserPromptSubmit",
    "SessionStart",
    "Stop",
    "SessionEnd",
];

/// Codex's discovery path, relative to the tree root.
const SKILLS_PREFIX: &str = ".agents/skills";

/// AIKit's own dispatcher file, kept out of the user's `config.toml`.
const INSTALL_FILE: &str = "hooks/aikit.toml";

/// The markers that make a directory a project/repository root — the places
/// Codex's own upward `.agents/skills` discovery walk stops. `.git` is the
/// repository root; `.agents` is the agent-skills root a project may already own.
const PROJECT_MARKERS: [&str; 2] = [".git", ".agents"];

/// Walk up from `start` (inclusive) to the nearest ancestor that looks like a
/// project/repository root. `None` if the filesystem root is reached without one.
///
/// This is the divider decision #3 turns on: a shared task's skills must land
/// where every sibling in the *project* sees them (Codex walks up to the project
/// root and reads `.agents/skills` there), not merely where the current task
/// happens to be working.
fn nearest_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        if PROJECT_MARKERS.iter().any(|m| current.join(m).exists()) {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

// ---------------------------------------------------------------------------
// Strategy
// ---------------------------------------------------------------------------

/// What to do when the task shares the session's working tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SharedTreeStrategy {
    /// **The default.** Project the skills every sibling task would see anyway;
    /// broker the session-only deltas.
    #[default]
    ProjectStableOnly,
    /// Write nothing into the tree; reach everything through the broker.
    BrokerAll,
    /// Write the whole session projection into the shared tree. Requires
    /// [`CodexAdapter::accepting_shared_projection`].
    SharedProjection,
}

impl SharedTreeStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            SharedTreeStrategy::ProjectStableOnly => "project-stable-only",
            SharedTreeStrategy::BrokerAll => "broker-all",
            SharedTreeStrategy::SharedProjection => "shared-projection",
        }
    }
}

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

pub struct CodexAdapter {
    /// The task's working tree. The projection is a property of *this*, which is
    /// the entire reason isolation matters here.
    tree: PathBuf,
    strategy: SharedTreeStrategy,
    accept_shared_projection: bool,
    materialization: MaterializationMode,
    binary: String,
}

impl CodexAdapter {
    pub fn new(tree: impl Into<PathBuf>) -> Self {
        Self {
            tree: tree.into(),
            strategy: SharedTreeStrategy::default(),
            accept_shared_projection: false,
            materialization: MaterializationMode::default(),
            binary: CLIENT.to_string(),
        }
    }

    #[must_use]
    pub fn with_strategy(mut self, strategy: SharedTreeStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Accept that a shared-tree capability projection is visible to sibling
    /// tasks. Context-specific actor bootstrap identity is still withheld.
    #[must_use]
    pub fn accepting_shared_projection(mut self, accepted: bool) -> Self {
        self.accept_shared_projection = accepted;
        self
    }

    #[must_use]
    pub fn with_materialization(mut self, mode: MaterializationMode) -> Self {
        self.materialization = mode;
        self
    }

    #[must_use]
    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }

    pub fn tree(&self) -> &Path {
        &self.tree
    }

    /// The absolute directory Codex would discover for the task's own tree.
    ///
    /// This is the isolated answer; a shared task uses [`Self::skills_dir_for`]
    /// with the nearest project root.
    pub fn skills_dir(&self) -> PathBuf {
        self.tree.join(SKILLS_PREFIX)
    }

    /// The tree Codex would read `.agents/skills` from under `isolation`, and — for
    /// a shared task with no project root above it — an honest note about the
    /// fallback.
    fn projection_tree(&self, isolation: Isolation) -> (PathBuf, Option<String>) {
        if isolation.is_isolated() {
            return (self.tree.clone(), None);
        }
        match nearest_project_root(&self.tree) {
            Some(root) => (root, None),
            None => (
                self.tree.clone(),
                Some(format!(
                    "no project root (a `.git` or `.agents` directory) was found above {}, so the \
                     working directory itself is used as the `.agents/skills` root",
                    self.tree.display()
                )),
            ),
        }
    }

    pub fn skills_dir_for(&self, isolation: Isolation) -> PathBuf {
        self.projection_tree(isolation).0.join(SKILLS_PREFIX)
    }

    fn export_name(capability: &ActiveCapability) -> String {
        capability
            .config
            .get("export_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| capability.id.leaf())
            .to_string()
    }

    fn payload_root(capability: &ActiveCapability) -> String {
        capability
            .config
            .get("root")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("payload")
            .to_string()
    }

    fn items_for(
        &self,
        context: &ResolvedContext,
        capabilities: &[&ActiveCapability],
        plan: &mut ProjectionPlan,
    ) -> Result<Vec<ProjectionItem>> {
        let mode = self.materialization.resolve_for(&self.capabilities());
        let mut items = Vec::new();
        for capability in capabilities {
            let Some(root) = context.root_of(&capability.id) else {
                plan.notes.push(format!(
                    "{} was not projected: the registry did not supply a path for it",
                    capability.id
                ));
                continue;
            };
            let payload = root.join(Self::payload_root(capability));
            let skill = agent_skills::validate(&payload)
                .map_err(|e| e.with("capability", capability.id.to_string()))?;
            let exported = agent_skills::AgentSkill {
                name: Self::export_name(capability),
                ..skill
            };
            let overlays = context
                .view
                .skill_usage_overlays
                .get(&capability.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            items.extend(exported.project_effective(Path::new(SKILLS_PREFIX), mode, overlays)?);
        }
        Ok(items)
    }

    fn append_isolated_bootstrap(
        &self,
        context: &ResolvedContext,
        plan: &mut ProjectionPlan,
        items: &mut Vec<ProjectionItem>,
    ) -> Result<()> {
        if let Some(actor) = context.actor_bootstrap.as_ref() {
            items.push(bootstrap::managed_bootstrap_item(
                Path::new(SKILLS_PREFIX),
                actor,
            )?);
            plan.notes.push(
                "the managed `aikit-context` Agent Skill is private to this task worktree; richer AIKit state remains on-demand"
                    .to_string(),
            );
        }
        Ok(())
    }
}

fn is_project_stable(
    capability: &ActiveCapability,
    all: &BTreeMap<CapsuleId, ActiveCapability>,
) -> bool {
    match &capability.origin {
        SelectionOrigin::Layer { scope, .. } => scope.rank() <= ScopeKind::ProjectLocal.rank(),
        SelectionOrigin::Policy { .. } => true,
        SelectionOrigin::Dependency { required_by } => match all.get(required_by) {
            Some(requirer) => is_project_stable(requirer, all),
            None => false,
        },
    }
}

impl TargetAdapter for CodexAdapter {
    fn target(&self) -> TargetId {
        TargetId::codex()
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: true,
            symlinks: true,
            isolated_per_context: true,
            requires_isolated_tree_for_isolation: true,
            brokered_fallback: true,
            watches_for_changes: true,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        let isolation = context.isolation();
        let skills: Vec<&ActiveCapability> = context.view.active_of_kind(Kind::Skill);

        if self.strategy == SharedTreeStrategy::BrokerAll {
            return Ok(ProjectionPlan::new(
                self.target(),
                ActivationEffect::brokered("this projection was configured to broker everything"),
            )
            .with_note(
                "nothing was written into the working tree; Codex reaches capabilities and actor disclosure through AIKit's broker/application surfaces"
                    .to_string(),
            ));
        }

        if isolation.is_isolated() {
            let mut plan = ProjectionPlan::new(
                self.target(),
                ActivationEffect::next_session_only(
                    "plain Codex discovers project skills at task start",
                ),
            )
            .with_note(format!(
                "this task has its own working tree ({}), so its skills are written to {} and no sibling task can see them",
                isolation.as_str(),
                self.skills_dir().display()
            ));
            let mut items = self.items_for(context, &skills, &mut plan)?;
            self.append_isolated_bootstrap(context, &mut plan, &mut items)?;
            return Ok(plan.with_items(items));
        }

        let (shared_root, root_fallback) = self.projection_tree(isolation);
        let shared_skills = shared_root.join(SKILLS_PREFIX);

        let stable: Vec<&ActiveCapability> = skills
            .iter()
            .copied()
            .filter(|c| is_project_stable(c, &context.view.active))
            .collect();
        let deltas: Vec<&ActiveCapability> = skills
            .iter()
            .copied()
            .filter(|c| !is_project_stable(c, &context.view.active))
            .collect();

        if self.strategy == SharedTreeStrategy::SharedProjection {
            if !self.accept_shared_projection {
                return Err(AikitError::new(
                    "projection.shared_tree_conflict",
                    format!(
                        "a shared-tree projection was asked for, but this task uses the session's working tree: writing {} would change what sibling tasks in the same tree see, without telling them. Accept the shared projection explicitly, or give the task its own tree with `--worktree`.",
                        shared_skills.display()
                    ),
                )
                .with("isolation", isolation.as_str())
                .with("tree", self.tree.display().to_string())
                .with("target", TargetId::CODEX));
            }

            let effect = if context.actor_bootstrap.is_some() {
                ActivationEffect::brokered(
                    "the shared capability projection was accepted, but the Run/session-specific actor bootstrap is withheld from the shared tree",
                )
            } else {
                ActivationEffect::next_session_only(
                    "plain Codex discovers project skills at task start",
                )
            };
            let mut plan = ProjectionPlan::new(self.target(), effect).with_note(format!(
                "a shared-tree capability projection was explicitly accepted: projected skills are written to {}, and sibling tasks working in the same tree will see them too",
                shared_skills.display()
            ));
            if context.actor_bootstrap.is_some() {
                plan = plan.with_note(
                    "the managed actor bootstrap was not written: its Run/session/runtime-body identity is context-specific and would leak to sibling tasks; use an isolated worktree for native projection"
                        .to_string(),
                );
            }
            if let Some(note) = &root_fallback {
                plan = plan.with_note(note.clone());
            }
            let items = self.items_for(context, &skills, &mut plan)?;
            return Ok(plan.with_items(items));
        }

        let withheld_bootstrap = context.actor_bootstrap.is_some();
        let effect = if deltas.is_empty() && !withheld_bootstrap {
            ActivationEffect::next_session_only(
                "plain Codex discovers project-stable skills at task start",
            )
        } else {
            let skill_part = if deltas.is_empty() {
                "no session-only capability skills are withheld".to_string()
            } else {
                format!(
                    "{} session-only skill{} {} withheld",
                    deltas.len(),
                    if deltas.len() == 1 { "" } else { "s" },
                    if deltas.len() == 1 { "is" } else { "are" }
                )
            };
            let bootstrap_part = if withheld_bootstrap {
                "the context-specific actor bootstrap is also withheld"
            } else {
                "no actor bootstrap is present"
            };
            ActivationEffect::brokered(format!(
                "this task uses the session's shared working tree: {skill_part}; {bootstrap_part}"
            ))
        };

        let mut plan = ProjectionPlan::new(self.target(), effect);
        if let Some(note) = &root_fallback {
            plan = plan.with_note(note.clone());
        }
        if deltas.is_empty() {
            plan = plan.with_note(format!(
                "this task uses the session's shared working tree; every active capability skill is project-stable, so {} holds exactly what every task in this tree already sees",
                shared_skills.display()
            ));
        } else {
            let names: Vec<String> = deltas.iter().map(|c| Self::export_name(c)).collect();
            plan = plan.with_note(format!(
                "Codex reads `.agents/skills` from the shared tree, so session-only skills ({}) remain available through the broker. Give the task its own tree with `--worktree` for native projection.",
                names.join(", "),
            ));
        }
        if withheld_bootstrap {
            plan = plan.with_note(
                "the managed `aikit-context` bootstrap is not projected into a shared tree because it contains Run/session/runtime-body identity; use an isolated worktree for a native seed"
                    .to_string(),
            );
        }

        let items = self.items_for(context, &stable, &mut plan)?;
        Ok(plan.with_items(items))
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if matches!(
            new.effect,
            ActivationEffect::Brokered { .. } | ActivationEffect::Unsupported { .. }
        ) {
            return new.effect.clone();
        }
        if new.is_noop_against(old) {
            ActivationEffect::immediate("already projected")
        } else {
            new.effect.clone()
        }
    }
}

impl ClientAdapter for CodexAdapter {
    fn launch_command(&self, _context: &ResolvedContext) -> Vec<String> {
        vec![self.binary.clone()]
    }

    fn install(&self, _config_dir: &Path) -> Result<Vec<ProjectionItem>> {
        let mut contents = String::from(
            "# >>> aikit >>>\n\
             # Managed by AIKit. One durable dispatcher entry per Codex event; the chain each\n\
             # one runs is rebuilt from the current generation on every dispatch, so this file\n\
             # never has to change when capabilities do.\n",
        );
        for event in DISPATCH_EVENTS {
            contents.push_str(&format!(
                "\n[[hooks]]\nevent = \"{event}\"\ncommand = \"aikit hook dispatch {CLIENT} {event}\"\n"
            ));
        }
        contents.push_str("\n# <<< aikit <<<\n");

        Ok(vec![ProjectionItem::write(INSTALL_FILE, contents)?])
    }
}
