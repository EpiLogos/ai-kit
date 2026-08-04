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

use super::agent_skills;
use super::ClientAdapter;

/// The client's own name for itself in a hook command.
pub const CLIENT: &str = "claude";

/// The events AIKit installs a durable dispatcher entry for.
///
/// One entry per event, forever: the chain that runs behind it is rebuilt from
/// the current generation on every dispatch, so the client's configuration never
/// has to change again when capabilities do.
pub const DISPATCH_EVENTS: [HookEventKind; 6] = [
    HookEventKind::PreToolUse,
    HookEventKind::PostToolUse,
    HookEventKind::UserPromptSubmit,
    HookEventKind::SessionStart,
    HookEventKind::Stop,
    HookEventKind::SessionEnd,
];

/// The projection subdirectory Claude looks in, relative to `--add-dir`.
const SKILLS_PREFIX: &str = ".claude/skills";

pub struct ClaudeAdapter {
    generation_root: PathBuf,
    materialization: MaterializationMode,
    binary: String,
}

impl ClaudeAdapter {
    pub fn new(generation_root: impl Into<PathBuf>) -> Self {
        Self {
            generation_root: generation_root.into(),
            materialization: MaterializationMode::default(),
            binary: CLIENT.to_string(),
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
        let path = config_dir.join("settings.json");
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

        let merged = merge_dispatcher_entries(existing.as_deref())?;
        Ok(vec![ProjectionItem::write("settings.json", merged)?])
    }
}

// ---------------------------------------------------------------------------
// Settings merging
// ---------------------------------------------------------------------------

/// The command AIKit installs for one event.
pub fn dispatch_command(event: &HookEventKind) -> String {
    format!("aikit hook dispatch {CLIENT} {event}")
}

/// Is this an AIKit dispatcher entry — including a stale one from an older
/// install that spelled the event differently?
fn is_aikit_entry(command: &str) -> bool {
    command
        .trim()
        .starts_with(&format!("aikit hook dispatch {CLIENT}"))
}

/// Merge AIKit's dispatcher entries into a settings document.
///
/// Everything that is not AIKit's is preserved: unrelated top-level keys,
/// unrelated events, and the user's own hooks inside events AIKit also uses.
/// What is *not* preserved is a previous AIKit entry, because leaving one behind
/// next to a new one would fire the whole chain twice.
pub fn merge_dispatcher_entries(existing: Option<&str>) -> Result<String> {
    let mut document: serde_json::Value = match existing {
        None => serde_json::json!({}),
        Some(raw) if raw.trim().is_empty() => serde_json::json!({}),
        Some(raw) => serde_json::from_str(raw).map_err(|e| {
            AikitError::new(
                "client.settings_unreadable",
                format!(
                    "the existing Claude settings are not valid JSON ({e}); AIKit will not \
                     overwrite a file it cannot read"
                ),
            )
        })?,
    };

    if !document.is_object() {
        return Err(AikitError::new(
            "client.settings_unreadable",
            "the existing Claude settings are not a JSON object",
        ));
    }

    let hooks = document
        .as_object_mut()
        .and_then(|o| {
            o.entry("hooks")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .ok_or_else(|| {
            AikitError::new(
                "client.settings_unreadable",
                "the existing `hooks` value is not an object",
            )
        })?;

    // A previous install may have written an entry under an event AIKit no longer
    // dispatches, or under a misspelling. Sweep those first, everywhere.
    for entries in hooks.values_mut() {
        if let Some(matchers) = entries.as_array_mut() {
            for matcher in matchers.iter_mut() {
                if let Some(list) = matcher.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    list.retain(|hook| {
                        !hook
                            .get("command")
                            .and_then(|c| c.as_str())
                            .is_some_and(is_aikit_entry)
                    });
                }
            }
            matchers.retain(|matcher| {
                matcher
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .is_none_or(|list| !list.is_empty())
            });
        }
    }

    for event in DISPATCH_EVENTS {
        let mut entry = serde_json::Map::new();
        // A matcher is only meaningful where the event carries a tool name;
        // elsewhere it is noise that invites people to edit it.
        if event.carries_tool_name() {
            entry.insert("matcher".to_string(), serde_json::json!("*"));
        }
        entry.insert(
            "hooks".to_string(),
            serde_json::json!([{ "type": "command", "command": dispatch_command(&event) }]),
        );

        let list = hooks
            .entry(event.as_str().to_string())
            .or_insert_with(|| serde_json::json!([]));
        match list.as_array_mut() {
            Some(array) => array.push(serde_json::Value::Object(entry)),
            None => {
                return Err(AikitError::new(
                    "client.settings_unreadable",
                    format!("the existing `hooks.{event}` value is not an array"),
                ))
            }
        }
    }

    // Remove any event key that ended up empty after the sweep, so an old install
    // does not leave `"PreCompact": []` behind forever.
    hooks.retain(|_, entries| entries.as_array().is_none_or(|a| !a.is_empty()));

    let mut rendered = serde_json::to_string_pretty(&document).map_err(|e| {
        AikitError::new(
            "client.settings_unreadable",
            format!("could not render the merged settings: {e}"),
        )
    })?;
    rendered.push('\n');
    Ok(rendered)
}
