//! Claude Code.
//!
//! Claude Code accepts an arbitrary extra directory (`--add-dir`), which is the
//! whole reason it can give two sessions in one checkout different skills: the
//! skill surface does not have to live in the working tree, so nothing is shared
//! between siblings by construction.
//!
//! The projection therefore lives inside the generation:
//!
//! ```text
//! <generation>/projections/claude/.claude/skills/<export-name>
//! ```
//!
//! ## What this adapter will never do
//!
//! It never emits an item touching `~/.claude/skills` or the project's own
//! `.claude/skills`. Those belong to the user and to the repository. Writing into
//! either would reintroduce exactly the global mutable active set the whole
//! design exists to avoid — and it would do so invisibly, since both directories
//! keep working afterwards.
//!
//! ## Why a changed generation requires `RestartClient`
//!
//! Claude can watch changes inside one fixed extra directory, but AIKit publishes
//! immutable generations by replacing the stable `current` pointer. AIKit does
//! not observe Claude retargeting that pointer, so a changed plan requires a
//! restart against the new `--add-dir`. When the projection has not changed,
//! there is nothing to reload and the honest answer is `Immediate`.

use std::path::{Path, PathBuf};

use aikit_core::capsule::Kind;
use aikit_core::hooks::HookEventKind;
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, MaterializationMode, ProjectionItem, ProjectionPlan, ResolvedContext,
    TargetAdapter, TargetCapabilities,
};
use aikit_core::{AikitError, Result};

use crate::actuation_harness_capability::{CapabilityOutcome, HarnessCapability};

use super::agent_skills;
use super::bootstrap;
use super::ClientAdapter;

/// The client's own name for itself in a hook command.
pub const CLIENT: &str = "claude";

/// The events AIKit installs are read from Actuation's capability descriptor,
/// never hard-coded here. A descriptor event maps to a dispatch boundary by
/// its native name, which is the exact key the harness spells in its settings.
pub type DescriptorEvents = Vec<(HookEventKind, String)>;

/// The projection subdirectory Claude looks in, relative to `--add-dir`.
const SKILLS_PREFIX: &str = ".claude/skills";

pub struct ClaudeAdapter {
    generation_root: PathBuf,
    materialization: MaterializationMode,
    binary: String,
    capability: Option<HarnessCapability>,
}

impl ClaudeAdapter {
    pub fn new(generation_root: impl Into<PathBuf>) -> Self {
        Self {
            generation_root: generation_root.into(),
            materialization: MaterializationMode::default(),
            binary: CLIENT.to_string(),
            capability: None,
        }
    }

    /// Install derives its events and seams from Actuation's capability
    /// descriptor. Without one the adapter refuses: hard-coding harness facts
    /// here is exactly the rediscovery the ownership split forbids.
    #[must_use]
    pub fn with_capability(mut self, capability: HarnessCapability) -> Self {
        self.capability = Some(capability);
        self
    }

    fn descriptor_events(&self) -> Result<DescriptorEvents> {
        match &self.capability {
            Some(capability) => {
                let (mut mapped, unrouted) =
                    CapabilityOutcome::Descriptor(Box::new(capability.clone())).dispatch_events();
                mapped.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
                if mapped.is_empty() {
                    return Err(AikitError::new(
                        "client.capability_without_dispatch_events",
                        format!(
                            "the {} capability descriptor maps none of its native events onto AIKit's dispatch boundaries (unrouted: {unrouted:?})",
                            capability.harness_slug
                        ),
                    ));
                }
                Ok(mapped)
            }
            None => Err(AikitError::new(
                "client.capability_unavailable",
                "no capability descriptor was supplied: AIKit installs only what Actuation                  declares the harness to be, and guessing is not installation",
            )),
        }
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

    /// The directory Claude is pointed at.
    pub fn projection_root(&self) -> PathBuf {
        self.generation_root.join("projections/claude")
    }

    /// The export name for a capability: its `export_name` config override, or
    /// the capsule's leaf.
    ///
    /// Not the `name` from `SKILL.md`: two registries can each ship a
    /// `code-review`, and the export name is how that collision is resolved
    /// without editing anybody's payload.
    fn export_name(capability: &aikit_core::resolve::ActiveCapability) -> String {
        capability
            .config
            .get("export_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| capability.id.leaf())
            .to_string()
    }

    /// Where a skill capsule's Agent Skill tree lives inside its capsule.
    fn payload_root(capability: &aikit_core::resolve::ActiveCapability) -> String {
        capability
            .config
            .get("root")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("payload")
            .to_string()
    }
}

impl TargetAdapter for ClaudeAdapter {
    fn target(&self) -> TargetId {
        TargetId::claude_code()
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: true,
            symlinks: true,
            isolated_per_context: true,
            // The skill directory is not in the working tree, so two shared-tree
            // tasks can still have different skills. This is the one line that
            // separates Claude's story from Codex's.
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: true,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        let mode = self.materialization.resolve_for(&self.capabilities());
        let mut plan =
            ProjectionPlan::new(self.target(), ActivationEffect::restart_client("Claude"));

        if self.materialization.degrades_for(&self.capabilities()) {
            plan = plan.with_note(
                "links were asked for but this target cannot use them, so the skills were copied"
                    .to_string(),
            );
        }

        for capability in context.view.active_of_kind(Kind::Skill) {
            let Some(root) = context.root_of(&capability.id) else {
                // The store did not say where this capsule lives. Skipping is
                // right — a projection cannot be invented — but silence is not.
                plan = plan.with_note(format!(
                    "{} was not projected: the registry did not supply a path for it",
                    capability.id
                ));
                continue;
            };

            let payload = root.join(Self::payload_root(capability));
            let skill = agent_skills::validate(&payload)
                .map_err(|e| e.with("capability", capability.id.to_string()))?;

            // The export name replaces the skill's own, so a collision between
            // two registries is resolved in the projection rather than on disk.
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
            plan = plan.with_items(exported.project_effective(
                Path::new(SKILLS_PREFIX),
                mode,
                overlays,
            )?);
        }

        if let Some(actor) = context.actor_bootstrap.as_ref() {
            plan = plan.with_item(bootstrap::managed_bootstrap_item(
                Path::new(SKILLS_PREFIX),
                actor,
            )?);
            plan = plan.with_note(
                "the managed `aikit-context` Agent Skill carries the resolved actor seed in this generation; richer AIKit state remains on-demand"
                    .to_string(),
            );
        }

        Ok(plan)
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if new.is_noop_against(old) {
            // Nothing changed, so nothing has to be reloaded; claiming a reload
            // would put a "live" badge next to a toggle that did nothing.
            ActivationEffect::immediate("already projected")
        } else {
            ActivationEffect::restart_client("Claude")
        }
    }
}

impl ClientAdapter for ClaudeAdapter {
    fn launch_command(&self, _context: &ResolvedContext) -> Vec<String> {
        vec![
            self.binary.clone(),
            "--add-dir".to_string(),
            self.projection_root().display().to_string(),
        ]
    }

    fn install(&self, config_dir: &Path) -> Result<Vec<ProjectionItem>> {
        let events = self.descriptor_events()?;
        let file_name = self
            .capability
            .as_ref()
            .map(|capability| {
                Path::new(&capability.install_seam.config_path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "settings.json".to_string())
            })
            .unwrap_or_else(|| "settings.json".to_string());
        let path = config_dir.join(&file_name);
        let existing = match std::fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(AikitError::new(
                    "client.settings_unreadable",
                    format!("could not read {}: {e}", path.display()),
                )
                .with("path", path.display().to_string()))
            }
        };

        let merged = merge_dispatcher_entries(existing.as_deref(), &events)?;
        Ok(vec![ProjectionItem::write(&file_name, merged)?])
    }
}

// ---------------------------------------------------------------------------
// Settings merging
// ---------------------------------------------------------------------------

/// Merge AIKit's dispatcher entries into a settings document.
///
/// Delegates to the shared Claude-grammar merge with claude-code's matcher
/// policy: tool names match against a glob, and `*` is its match-all.
pub fn merge_dispatcher_entries(existing: Option<&str>, events: &DescriptorEvents) -> Result<String> {
    super::hook_map::merge_hook_map_entries(
        existing,
        events,
        CLIENT,
        super::hook_map::MatcherPolicy::StarForTools,
    )
}
