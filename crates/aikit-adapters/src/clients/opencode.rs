//! OpenCode — harness-adapter admission through the harness-adapter contract.
//!
//! Evidence base (primary sources, observed 2026-09-06):
//! - Repo `sst/opencode` (default branch `dev`, pushed 2026-09-06); latest
//!   release `v1.18.29` via `gh release view`.
//! - Docs fetched from opencode.ai: /docs/rules (AGENTS.md global + project,
//!   precedence, CLAUDE.md compatibility), /docs/config (global
//!   ~/.config/opencode/opencode.json, project opencode.json, plural
//!   subdirectories agents/ commands/ modes/ plugins/ skills/ tools/ themes/),
//!   /docs/plugins (JS/TS plugins, custom tools, event list incl.
//!   session.created and file.watcher.updated), /docs/agents (primary agents
//!   and subagents, @ mention), /docs/mcp-servers (local and remote MCP),
//!   /docs/commands (.opencode/commands/*.md, ~/.config/opencode/commands/),
//!   /docs/cli (tui, run, serve, web, attach, --continue, --session).
//! - OpenCode is not installed on this machine (`which opencode` → not
//!   found), so there is no local native artifact layer for this census.
//!
//! ## Identity law
//!
//! A model running in OpenCode is not the Agent identity; OpenCode is not the
//! World; an opencode process is not an AgentSession. The adapter keeps
//! `target`, `product`, and any `realised_actuation_ref` distinct, and never
//! fabricates a loaded-activation claim (`verify_activation_truth` rejects it).
//!
//! ## Projection stance
//!
//! Genuine on-disk surfaces exist and are named exactly in the census
//! (project `AGENTS.md`; `.opencode/` skills/commands/agents/plugins trees;
//! `opencode.json`). This revision still brokers projection: AGENTS.md is a
//! user-authored file whose documented flow is `/init`-managed, the config
//! precedence chain (project over global, CLAUDE.md as fallback) makes an
//! unmanaged adapter write an authored-file conflict, and AIKit owns no
//! representation on the `.opencode/` trees yet. Native projection is the next
//! slice, not this admission.
use std::path::{Path, PathBuf};

use aikit_core::harness_admission::{
    FacultySupport, HarnessAdmissionAdapter, HarnessAdmissionDescriptor, HarnessEditionKind,
    HarnessFaculty, HarnessFacultyObservation, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionPlan, ResolvedContext, TargetAdapter, TargetCapabilities,
};
use aikit_core::Result;

pub const CLIENT: &str = "opencode";
pub const PRODUCT: &str = "OpenCode";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:opencode-adapter";

/// Stable evidence refs pointing at primary sources, not prose.
const EV_RULES: &str = "doc:opencode.ai/docs/rules";
const EV_CONFIG: &str = "doc:opencode.ai/docs/config";
const EV_PLUGINS: &str = "doc:opencode.ai/docs/plugins";
const EV_AGENTS: &str = "doc:opencode.ai/docs/agents";
const EV_MCP: &str = "doc:opencode.ai/docs/mcp-servers";
const EV_CLI: &str = "doc:opencode.ai/docs/cli";
const EV_RELEASE: &str = "gh:sst/opencode release v1.18.29";

pub struct OpencodeAdapter {
    /// Where a future native .opencode/ projection would be written.
    /// Unused by the brokered plan in this revision; kept as the explicit
    /// projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl OpencodeAdapter {
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

fn opencode_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_RULES],
            Some("global rules live in ~/.config/opencode/AGENTS.md"),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_RULES],
            Some(
                "project AGENTS.md is the primary project rules file; CLAUDE.md is a \
                 documented fallback when no AGENTS.md exists",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_CONFIG, EV_RULES],
            Some(
                "skills/ is a documented config subdirectory (global and project scope, \
                 plural name with singular back-compat); Claude Code ~/.claude/skills \
                 compatibility is documented",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_PLUGINS],
            Some("plugins subscribe to events; session.created is a documented event type"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Degraded,
            &[EV_PLUGINS],
            Some(
                "no documented live reload of rules/config; plugins only receive \
                 file.watcher.updated file-watch events",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_RULES, EV_CONFIG],
            Some("rules and config are session-context inputs re-resolved for each session"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Degraded,
            &[EV_CONFIG],
            Some(
                "config precedence (remote < global < project < managed) is resolved at \
                 startup; no explicit restart-reload contract is documented",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_MCP],
            Some("built-in tools plus local and remote MCP servers, enabled via config"),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_PLUGINS, EV_CONFIG],
            Some(
                "plugins define custom tools in JS/TS; tools/ is a documented config \
                 subdirectory",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_CLI, EV_RELEASE],
            Some(
                "opencode --continue/-c resumes the last session; --session continues by ID \
                 (census pinned to release v1.18.29)",
            ),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_AGENTS],
            Some(
                "primary agents (Build, Plan) and subagents configurable with custom \
                 prompts, models, and tool access; subagents are @-mentionable",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_CONFIG, EV_PLUGINS],
            Some(
                "project config is opencode.json in the project plus the .opencode/ tree; \
                 plugins receive the project directory and git worktree path",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Degraded,
            &[EV_CONFIG],
            Some(
                "themes/ is a documented config subdirectory, but the TUI chrome is fixed; \
                 there is no user-authored component tree",
            ),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_CLI],
            Some(
                "terminal UI, opencode run (non-interactive), opencode serve (headless \
                 server), opencode web/attach surfaces",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Degraded,
            &[EV_PLUGINS],
            Some(
                "plugins observe message.removed/message.updated and session.deleted, but \
                 no documented contract unloads rules or skills from a live session",
            ),
        ),
    ]
}

impl TargetAdapter for OpencodeAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            // No documented live reload of rules/config; sessions re-resolve
            // context at start.
            live_reload: false,
            symlinks: true,
            // Config homes are fixed (~/.config/opencode and the project's
            // .opencode/); there is no per-context config-dir flag.
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(
                "OpenCode projection is brokered in this adapter revision; the census \
                 names the genuine surfaces (project AGENTS.md, .opencode/ skills, \
                 commands, agents, plugins trees, opencode.json) but AIKit owns no \
                 representation on them yet",
            ),
        )
        .with_note(
            "project AGENTS.md is a user-authored, /init-managed rules file and the \
             config precedence chain makes an unmanaged adapter write an authored-file \
             conflict; native projection onto the .opencode/ trees with an ownership \
             rule is the next slice"
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

impl HarnessAdmissionAdapter for OpencodeAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Not installed on this machine; nothing observed locally.
            native_version: None,
            source_revision: Some("v1.18.29".to_string()),
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: opencode_faculties(),
        }
    }
}
