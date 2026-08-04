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
//! genuinely per-task and everything is written.
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
//!    so writing them changes nothing observable. Session-only deltas are *not*
//!    projected.
//! 2. **brokered.** Nothing is written; capabilities are reached through
//!    `aikit capabilities list|read` and `aikit run`.
//! 3. **an explicitly accepted shared projection.** Everything is written into
//!    the shared tree. This requires [`CodexAdapter::accepting_shared_projection`]
//!    and is otherwise refused with `projection.shared_tree_conflict`.
//!
//! ## Two things this adapter will never do
//!
//! It never invents a synthetic `HOME` to fake per-task isolation: a fabricated
//! home silently redirects git config, ssh config and stored credentials, and the
//! damage surfaces long after the shortcut was taken.
//!
//! It never reports a fallback as `Immediate` merely because the files on disk
//! did not change. A no-op apply in the shared case still leaves the session
//! deltas outside Codex, and saying otherwise would tell the user their toggle
//! worked.
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

    /// Accept that a shared-tree projection is visible to sibling tasks.
    ///
    /// The flag exists so that the acceptance is a *decision*, made by somebody
    /// who was shown the consequence, rather than the accidental outcome of a
    /// default.
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
    ///
    /// An isolated task owns its tree, so the root is that tree. A shared task's
    /// skills have to land where every sibling in the same project sees them: the
    /// nearest project root above the working directory, exactly where Codex's own
    /// upward discovery walk stops. When there is no project root above the working
    /// directory, the working directory itself is used and the fallback is stated
    /// rather than hidden.
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

    /// The absolute `.agents/skills` directory Codex would discover under
    /// `isolation`: the task's own tree when isolated, the nearest project root
    /// when shared.
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
            items.extend(exported.project_effective(
                Path::new(SKILLS_PREFIX),
                mode,
                overlays,
            )?);
        }
        Ok(items)
    }
}

/// Is this capability part of the tree's durable contents, or a delta this
/// context alone has?
///
/// Project scope and above are durable: every sibling task in the tree resolves
/// them too, so writing them changes nothing anyone can observe. Session, task
/// and one-shot selections are the ones that would leak.
///
/// A dependency inherits the answer from whatever pulled it in — expanding a
/// session-only skill's requirement into the shared tree would leak just as
/// surely as the skill itself.
fn is_project_stable(
    capability: &ActiveCapability,
    all: &BTreeMap<CapsuleId, ActiveCapability>,
) -> bool {
    match &capability.origin {
        SelectionOrigin::Layer { scope, .. } => scope.rank() <= ScopeKind::ProjectLocal.rank(),
        // Managed policy is durable by definition: it is not a per-session choice.
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
            // The line that matters: Codex's skill directory lives in the tree.
            requires_isolated_tree_for_isolation: true,
            brokered_fallback: true,
            watches_for_changes: true,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        let isolation = context.isolation();
        let skills: Vec<&ActiveCapability> = context.view.active_of_kind(Kind::Skill);

        // --- Brokering is a choice available in either case ------------------
        if self.strategy == SharedTreeStrategy::BrokerAll {
            return Ok(ProjectionPlan::new(
                self.target(),
                ActivationEffect::brokered("this projection was configured to broker everything"),
            )
            .with_note(
                "nothing was written into the working tree; Codex reaches these capabilities \
                 through `aikit capabilities list --context current --agent-index`, \
                 `aikit capabilities read <id>` and `aikit run <id>`"
                    .to_string(),
            ));
        }

        // --- The task has its own tree ---------------------------------------
        if isolation.is_isolated() {
            let mut plan = ProjectionPlan::new(
                self.target(),
                ActivationEffect::next_session_only(
                    "plain Codex discovers project skills at task start",
                ),
            )
            .with_note(format!(
                "this task has its own working tree ({}), so its skills are written to \
                         {} and no sibling task can see them",
                isolation.as_str(),
                self.skills_dir().display()
            ));
            let items = self.items_for(context, &skills, &mut plan)?;
            return Ok(plan.with_items(items));
        }

        // --- The task shares the session's tree ------------------------------
        // A shared projection has to be visible to every sibling in the project,
        // so it targets the nearest project root above the working directory, not
        // merely where this task happens to be working (decision #3).
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
                        "a shared-tree projection was asked for, but this task uses the \
                         session's working tree: writing {} would change what sibling tasks in \
                         the same tree see, without telling them. Accept the shared projection \
                         explicitly, or give the task its own tree with `--worktree`.",
                        shared_skills.display()
                    ),
                )
                .with("isolation", isolation.as_str())
                .with("tree", self.tree.display().to_string())
                .with("target", TargetId::CODEX));
            }

            let mut plan = ProjectionPlan::new(
                self.target(),
                ActivationEffect::next_session_only(
                    "plain Codex discovers project skills at task start",
                ),
            )
            .with_note(format!(
                "a shared-tree projection was explicitly accepted: every skill in this \
                     context was written to {}, and sibling tasks working in the same tree will \
                     see them too",
                shared_skills.display()
            ));
            if let Some(note) = &root_fallback {
                plan = plan.with_note(note.clone());
            }
            let items = self.items_for(context, &skills, &mut plan)?;
            return Ok(plan.with_items(items));
        }

        // Fallback 1: project-stable only.
        let effect = if deltas.is_empty() {
            ActivationEffect::next_session_only(
                "plain Codex discovers project-stable skills at task start",
            )
        } else {
            ActivationEffect::brokered(format!(
                "this task uses the session's shared working tree, so {} session-only {} not \
                 written into it",
                deltas.len(),
                if deltas.len() == 1 {
                    "skill was"
                } else {
                    "skills were"
                }
            ))
        };

        let mut plan = ProjectionPlan::new(self.target(), effect);
        if let Some(note) = &root_fallback {
            plan = plan.with_note(note.clone());
        }
        if deltas.is_empty() {
            plan = plan.with_note(format!(
                "this task uses the session's shared working tree; every active skill is \
                 project-stable, so {} holds exactly what every task in this tree already sees",
                shared_skills.display()
            ));
        } else {
            let names: Vec<String> = deltas.iter().map(|c| Self::export_name(c)).collect();
            let (noun, verb, pronoun) = if names.len() == 1 {
                ("skill", "is", "it")
            } else {
                ("skills", "are", "them")
            };
            plan = plan.with_note(format!(
                "this task uses the session's shared working tree, and Codex reads \
                 `.agents/skills` from the tree — so a per-task projection would change what \
                 sibling tasks see. Only the project-stable skills were written. The \
                 session-only {noun} ({}) {verb} available through the broker \
                 (`aikit capabilities list --context current --agent-index`). Give the task its \
                 own tree with `--worktree` to have {pronoun} projected natively.",
                names.join(", "),
            ));
        }

        let items = self.items_for(context, &stable, &mut plan)?;
        Ok(plan.with_items(items))
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        // A fallback effect survives a no-op apply. The files did not change, but
        // the session deltas are still not in Codex, and "immediate" would say
        // the opposite.
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
        // Codex finds `.agents/skills` by walking up from the working directory.
        // There is no directory flag to pass, and inventing one — or a synthetic
        // `HOME` — would produce a command that either fails outright or quietly
        // redirects the user's credentials.
        vec![self.binary.clone()]
    }

    fn install(&self, _config_dir: &Path) -> Result<Vec<ProjectionItem>> {
        // AIKit's entries live in their own file rather than merged into the
        // user's `config.toml`. A round-trip through a TOML parser would strip
        // the comments out of a hand-written config, and no amount of tidiness is
        // worth that.
        let mut contents = String::from(
            "# >>> aikit >>>\n\
             # Managed by AIKit. One durable dispatcher entry per Codex event; the chain each\n\
             # one runs is rebuilt from the current generation on every dispatch, so this file\n\
             # never has to change when capabilities do.\n",
        );
        for event in DISPATCH_EVENTS {
            contents.push_str(&format!(
                "\n[[hooks]]\nevent = \"{event}\"\ncommand = \"aikit hook dispatch {CLIENT} \
                 {event}\"\n"
            ));
        }
        contents.push_str("\n# <<< aikit <<<\n");

        Ok(vec![ProjectionItem::write(INSTALL_FILE, contents)?])
    }
}
