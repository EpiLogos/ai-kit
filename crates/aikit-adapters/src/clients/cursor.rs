//! Cursor CLI — harness-adapter admission for Cursor's terminal agent
//! (invoked as `agent`, https://cursor.com/docs/cli/overview).
//!
//! Cursor CLI is a process-per-invocation agent with a genuine, documented,
//! auto-discovered on-disk surface: project rules in `.cursor/rules/*.mdc`
//! (frontmatter: `description`, `globs`, `alwaysApply`), `AGENTS.md` in the
//! project root and subdirectories, skills under `.cursor/skills/` and
//! `~/.cursor/skills/`, hooks in `.cursor/hooks.json` and `~/.cursor/hooks.json`,
//! MCP servers in `.cursor/mcp.json` and `~/.cursor/mcp.json`, and CLI config in
//! `<project>/.cursor/cli.json` / `~/.cursor/cli-config.json`. All paths and
//! behaviours below were verified against cursor.com/docs on 2026-09-06.
//!
//! This adapter plans a real projection — one always-applied project rule —
//! because the surface is additive (a new file, no user config to merge) and is
//! read by the agent at session start. Mid-session pickup of a newly written
//! rule is not documented, so the effect is honestly `NextSessionOnly`.
//!
//! ## Identity law
//!
//! A model running under Cursor CLI is not the Agent identity; Cursor CLI is
//! not the World; an agent process is not an AgentSession. The adapter keeps
//! `target`, `product`, and any `realised_actuation_ref` distinct, and never
//! fabricates a loaded-activation claim (`verify_activation_truth` rejects it).

use std::path::{Path, PathBuf};

use aikit_core::Result;
use aikit_core::harness_admission::{
    FacultySupport, HARNESS_ADAPTER_SDK_VERSION, HarnessAdmissionAdapter,
    HarnessAdmissionDescriptor, HarnessEditionKind, HarnessFaculty, HarnessFacultyObservation,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
    TargetCapabilities,
};

pub const CLIENT: &str = "cursor-cli";
pub const PRODUCT: &str = "Cursor CLI";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:cursor-adapter";
/// Detection disclosure: Cursor CLI is absent from the Actuation harness
/// catalog (r4, observed 2026-09-06), so no `actuation.harness-detection/v1`
/// record exists for it. Under the detection contract's three-state law this
/// is disclosed unavailability, not absence-by-silence; the census evidence
/// below rests on primary docs and local artifacts.
/// Primary sources, fetched 2026-09-06. The CLI is not installed on this
/// machine (`which cursor-agent`, `which agent` empty; `actuation harness
/// detect` catalog r4 does not cover it), so the census cites cursor.com/docs.
/// Note today's docs invoke the binary as `agent` (Cursor Agent), not
/// `cursor-agent`, and no longer document a `~/.cursor/rules` user-rules path.
const EV_RULES: &str = "https://cursor.com/docs/rules";
const EV_SKILLS: &str = "https://cursor.com/docs/skills";
const EV_HOOKS: &str = "https://cursor.com/docs/hooks";
const EV_SUBAGENTS: &str = "https://cursor.com/docs/subagents";
const EV_CLI_OVERVIEW: &str = "https://cursor.com/docs/cli/overview";
const EV_CLI_PARAMS: &str = "https://cursor.com/docs/cli/reference/parameters";
const EV_CLI_CONFIG: &str = "https://cursor.com/docs/cli/reference/configuration";

/// Project rule this adapter projects: an always-applied `.mdc` rule, the exact
/// on-disk surface documented at cursor.com/docs/rules.
pub const RULE_PATH: &str = ".cursor/rules/aikit-harness.mdc";

/// Frontmatter + body of the projected rule. `alwaysApply: true` makes the
/// rule included in every session, per the documented rule anatomy.
const RULE_CONTENTS: &str = "---\nalwaysApply: true\n---\n\n- Projected by AIKit's Cursor CLI \
adapter (`aikit:cursor-adapter`). Treat AIKit-managed capability material as \
read-only harness context, not as user-authored project guidance.\n";

pub struct CursorAdapter {
    /// Projection root the relative `.cursor/rules/...` destination is scoped to.
    root: PathBuf,
}

impl CursorAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn faculty(
    faculty: HarnessFaculty,
    support: FacultySupport,
    evidence: &[&str],
    note: Option<&str>,
) -> HarnessFacultyObservation {
    HarnessFacultyObservation {
        faculty,
        support,
        evidence_refs: evidence.iter().map(|s| (*s).to_string()).collect(),
        note: note.map(|s| s.to_string()),
    }
}

fn cursor_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_RULES, EV_HOOKS, EV_SKILLS],
            Some(
                "User Rules are global preferences managed in Customize → Rules (no on-disk \
                 path documented today); user-level `~/.cursor/hooks.json` and \
                 `~/.cursor/skills/` are documented on-disk global surfaces",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_RULES],
            Some(
                "`.cursor/rules/*.mdc` with frontmatter (`description`, `globs`, \
                 `alwaysApply`), or plain `AGENTS.md` in the project root and \
                 subdirectories",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS],
            Some(
                "skills are folders with `SKILL.md`, discovered from `.cursor/skills/`, \
                 `.agents/skills/`, `~/.cursor/skills/`, `~/.agents/skills/`",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Degraded,
            &[EV_HOOKS],
            Some(
                "`sessionStart`/`sessionEnd` hook events are documented for agent sessions \
                 (editor and cloud/self-hosted); CLI-native loading of `hooks.json` is not \
                 explicitly documented",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Degraded,
            &[EV_SKILLS, EV_RULES],
            Some(
                "skills are discovered \"when Cursor starts\" and rules are read per \
                 session; mid-session pickup of changed files is not documented",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_SKILLS, EV_RULES],
            Some("a new agent invocation discovers project rules and skills from the repo"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[EV_CLI_OVERVIEW],
            Some("process-per-invocation CLI; there is no persistent client to restart"),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_CLI_PARAMS],
            Some(
                "MCP servers configured in `.cursor/mcp.json` / `~/.cursor/mcp.json`, \
                 managed via `agent mcp` subcommands and `--approve-mcps`",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_CLI_PARAMS],
            Some(
                "MCP servers contribute tools to the agent (`agent mcp list-tools`); local \
                 plugin directories load via `--plugin-dir`",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_CLI_PARAMS, EV_CLI_OVERVIEW],
            Some("`--resume [chatId]`, `--continue`, and `agent ls` / `agent resume`"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_SUBAGENTS, EV_CLI_OVERVIEW],
            Some(
                "subagents are documented for \"the editor, CLI, and Cloud Agents\"; the \
                 CLI also documents Cloud Agent handoff by prefixing `&` to a message",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_CLI_PARAMS],
            Some(
                "`--workspace <path>` selects the workspace; `-w/--worktree` runs in a git \
                 worktree under `~/.cursor/worktrees/<reponame>`",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Degraded,
            &[EV_CLI_PARAMS],
            Some(
                "a plugin system exists and the CLI loads local plugin directories via \
                 `--plugin-dir`, but component composition is plugin-managed rather than a \
                 plain project tree",
            ),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_RULES, EV_SKILLS, EV_HOOKS, EV_CLI_PARAMS, EV_CLI_CONFIG],
            Some(
                "`.cursor/rules/*.mdc`, `AGENTS.md`, `.cursor/hooks.json`, \
                 `.cursor/skills/`, `.cursor/mcp.json`, `.cursor/cli.json`, and \
                 `~/.cursor/cli-config.json`",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Degraded,
            &[EV_SKILLS, EV_RULES],
            Some(
                "no live retraction is documented; a removed rule or skill stops being \
                 discovered at the next session",
            ),
        ),
    ]
}

impl TargetAdapter for CursorAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: true,
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        let item = ProjectionItem::write(RULE_PATH, RULE_CONTENTS)?;
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::next_session_only(
                "Cursor CLI discovers `.cursor/rules/*.mdc` when an agent session starts; \
                 mid-session pickup of a newly written rule is not documented",
            ),
        )
        .with_item(item)
        .with_note(
            "projects one always-applied project rule (`.cursor/rules/aikit-harness.mdc`), \
             the exact surface documented at cursor.com/docs/rules; skills, hooks, and MCP \
             surfaces are admitted in the census and brokered separately"
                .to_string(),
        ))
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

impl HarnessAdmissionAdapter for CursorAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Not installed on this machine (`which cursor-agent` / `which agent`
            // empty) and absent from `actuation harness detect` catalog r4, so
            // there is no observed native version to record.
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: cursor_faculties(),
        }
    }
}
