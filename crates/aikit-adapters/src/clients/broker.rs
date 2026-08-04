//! The broker: one small generic skill, for clients that cannot be handed a
//! directory of their own.
//!
//! Instead of projecting every capability into a client, the broker projects a
//! single skill that teaches the model three commands:
//!
//! ```text
//! aikit capabilities list --context current --agent-index
//! aikit capabilities read <id>
//! aikit run <id>
//! ```
//!
//! ## Why the index is metadata and nothing else
//!
//! The broker exists to *save* context, so an index that inlined instructions
//! would be strictly worse than the native projection it stands in for — the
//! model would pay for every capability's full text whether or not it used one.
//! What it gets instead is enough to choose by: id, name, one line of
//! description, and the tags that hint at when a capability applies. The
//! instructions stay where they are until `aikit capabilities read` asks for
//! them.
//!
//! ## Why the index is bounded
//!
//! A catalogue grows; a context window does not. [`IndexBudget`] caps the entry
//! count, the bytes and the description length, and a truncated index says how
//! many were left out and how to see them. A silently truncated list is worse
//! than a short one, because a model will treat it as complete.

use std::path::Path;

use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
    TargetCapabilities,
};
use aikit_core::Result;

use super::agent_skills::first_sentence;
use super::ClientAdapter;

/// The target id for the brokered surface.
pub const TARGET: &str = "broker";

/// The export name of the generated skill.
pub const EXPORT_NAME: &str = "aikit";

/// How much of the model's context the index is allowed to spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexBudget {
    pub max_entries: usize,
    pub max_bytes: usize,
    pub max_description_chars: usize,
}

impl Default for IndexBudget {
    fn default() -> Self {
        Self {
            // Enough to cover a well-curated context without becoming a document
            // in its own right.
            max_entries: 60,
            max_bytes: 8 * 1024,
            max_description_chars: 120,
        }
    }
}

pub struct BrokerAdapter {
    prefix: String,
    budget: IndexBudget,
}

impl Default for BrokerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl BrokerAdapter {
    pub fn new() -> Self {
        Self {
            // Codex's discovery path, which is also a perfectly good place for a
            // client that reads skills from the tree.
            prefix: ".agents/skills".to_string(),
            budget: IndexBudget::default(),
        }
    }

    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    #[must_use]
    pub fn with_budget(mut self, budget: IndexBudget) -> Self {
        self.budget = budget;
        self
    }

    pub fn budget(&self) -> IndexBudget {
        self.budget
    }

    /// The generated `SKILL.md`.
    pub fn skill_markdown(&self) -> String {
        format!(
            "---\n\
             name: {EXPORT_NAME}\n\
             description: Lists and runs the capabilities AIKit has resolved for this context. \
             Use it to find a script, skill or tool that is available here, to read one's full \
             instructions before following them, and to run one.\n\
             ---\n\
             \n\
             # AIKit capabilities\n\
             \n\
             The capabilities available in this context are not all projected into your\n\
             skill directory. `INDEX.md` next to this file lists what is available, one\n\
             line each. Read it first.\n\
             \n\
             ## Commands\n\
             \n\
             List everything available here, as a compact index:\n\
             \n\
             ```sh\n\
             aikit capabilities list --context current --agent-index\n\
             ```\n\
             \n\
             Read one capability's full instructions, only when you have decided to use it:\n\
             \n\
             ```sh\n\
             aikit capabilities read <id>\n\
             ```\n\
             \n\
             Run one:\n\
             \n\
             ```sh\n\
             aikit run <id>\n\
             ```\n\
             \n\
             ## Notes\n\
             \n\
             - Ids look like `skill/rust/code-review` or `script/test/cargo-nextest`.\n\
             - The index is deliberately short. Do not guess at a capability's behaviour\n\
               from its one-line description; read it first.\n\
             - A capability that is not in the index is not available in this context,\n\
               whatever you may know about it from elsewhere.\n"
        )
    }

    /// The compact metadata index.
    ///
    /// Built entirely from the resolved view. It has no access to a payload and
    /// therefore cannot leak one — the property is structural, not a rule someone
    /// has to remember.
    pub fn index(&self, context: &ResolvedContext) -> String {
        let mut out = String::from(
            "# Capabilities available in this context\n\
             \n\
             One line each: `id — name: what it is [when to use]`.\n\
             Run `aikit capabilities read <id>` for the full instructions before using one.\n\
             \n",
        );

        let mut rendered = 0usize;
        let mut skipped = 0usize;

        for (id, capability) in &context.view.active {
            let entry = context.view.catalog_index.get(id);
            let mut description = entry.map(|e| e.description.clone()).unwrap_or_default();
            if let Some(overlays) = context.view.skill_usage_overlays.get(id) {
                for addition in overlays.iter().filter_map(|overlay| overlay.description.as_ref()) {
                    if !addition.trim().is_empty() {
                        if !description.is_empty() {
                            description.push(' ');
                        }
                        description.push_str(addition.trim());
                    }
                }
            }
            let tags = entry.map(|e| e.tags.as_slice()).unwrap_or(&[]);

            let line = format!(
                "- `{id}` — {}: {}{}\n",
                capability.name,
                truncate(&first_sentence(&description), self.budget.max_description_chars),
                if tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", tags.join(", "))
                }
            );

            if rendered >= self.budget.max_entries || out.len() + line.len() > self.budget.max_bytes
            {
                skipped += 1;
                continue;
            }
            out.push_str(&line);
            rendered += 1;
        }

        if rendered == 0 {
            out.push_str("- (nothing is active in this context)\n");
        }
        if skipped > 0 {
            // Naming the number is the difference between a short list and a
            // list a model will treat as complete.
            out.push_str(&format!(
                "\n… and {skipped} more. This index is capped so it does not fill your context; \
                 run `aikit capabilities list --context current --agent-index` to page through \
                 the rest.\n"
            ));
        }
        out
    }
}

/// Truncate on a character boundary, marking that it happened.
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

impl TargetAdapter for BrokerAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(TARGET)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: true,
            symlinks: false,
            isolated_per_context: true,
            // One generic skill is the same in every task, so there is nothing
            // for two sibling tasks to overwrite.
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: true,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        let root = format!("{}/{EXPORT_NAME}", self.prefix);
        Ok(
            ProjectionPlan::new(self.target(), ActivationEffect::LiveReloadExpected)
                .with_note(
                    "capabilities are reachable through `aikit capabilities list|read` and \
                     `aikit run` rather than projected individually"
                        .to_string(),
                )
                .with_item(ProjectionItem::write(
                    format!("{root}/SKILL.md"),
                    self.skill_markdown(),
                )?)
                .with_item(ProjectionItem::write(
                    format!("{root}/INDEX.md"),
                    self.index(context),
                )?),
        )
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if new.is_noop_against(old) {
            ActivationEffect::immediate("already projected")
        } else {
            ActivationEffect::LiveReloadExpected
        }
    }
}

impl ClientAdapter for BrokerAdapter {
    fn launch_command(&self, _context: &ResolvedContext) -> Vec<String> {
        // The broker is a skill inside somebody else's client. It has nothing of
        // its own to start, and returning a plausible-looking command would
        // invite a caller to run one.
        Vec::new()
    }

    fn install(&self, _config_dir: &Path) -> Result<Vec<ProjectionItem>> {
        // No client, no client configuration. The dispatcher entries belong to
        // whichever client is hosting the broker skill.
        Ok(Vec::new())
    }
}
