//! OpenClaw — harness-adapter admission through the harness-adapter contract.
//!
//! OpenClaw is a genuine agent harness: a CLI + gateway ("Ship fast, log
//! faster", 🦞) that runs agent turns through a long-lived gateway service with
//! persistent agents, workspaces, skills, hooks, plugins, MCP servers, stored
//! sessions, subagents, cron and channel surfaces. Actuation's catalog
//! (catalog_revision 4) classifies it `native_kind: "harness"`, edition `cli`,
//! with facets agents:~/.openclaw/agents and config:~/.openclaw/openclaw.json.
//!
//! Evidence base (primary sources, observed 2026-09-06/07):
//! - Actuation detection record (my own run): `actuation harness detect
//!   --json --versions` → openclaw detected at /Users/admin/Library/pnpm/openclaw
//!   (pnpm shim), version 2026.1.30, agents facet count 1, config present.
//!   `actuation harness capability openclaw` → undeclared (only claude-code,
//!   codex, zcode are declared), noted honestly in the census.
//! - `openclaw --version` → 2026.1.30 (76b5208); repo openclaw/openclaw,
//!   docs at docs.openclaw.ai (cli/skills, cli/hooks, cli/agents verified live).
//! - Local native artifacts: ~/.openclaw/openclaw.json (agents.defaults.workspace
//!   = ~/.openclaw/workspace, subagents.maxConcurrent 8, model + auth profiles),
//!   ~/.openclaw/mcp.json (bimba-mcp, linear-server MCP servers),
//!   ~/.openclaw/workspace/AGENTS.md ("Every Session" reads SOUL.md, USER.md,
//!   MISSION.md, NEXT-SESSION-STARTUP.md, PARADIGM.md, memory/YYYY-MM-DD.md),
//!   ~/.openclaw/agents/main/sessions/*.jsonl + sessions.json, memory store
//!   (main.sqlite), cron/jobs.json, telegram channel dir, ~/.openclaw/subagents.
//! - `openclaw skills list` → 54 skills (17 ready, source openclaw-bundled,
//!   ClawHub install path); `openclaw hooks list` → boot-md (BOOT.md on gateway
//!   startup), command-logger, session-memory (on /new), enable/disable/install;
//!   `openclaw agents --help` → isolated agents (workspaces + auth + routing);
//!   `openclaw plugins --help` → list/info/enable/disable in config;
//!   `openclaw sessions --help` → stored conversation sessions.
//!
//! ## Identity law
//!
//! A model running in OpenClaw is not the Agent identity; OpenClaw is not the
//! World; a gateway process is not an AgentSession. The adapter keeps `target`,
//! `product`, and any `realised_actuation_ref` distinct, and never fabricates a
//! loaded-activation claim (`verify_activation_truth` rejects it).
//!
//! ## Projection stance
//!
//! A genuine on-disk instruction surface exists and is named exactly: the
//! per-agent workspace (~/.openclaw/workspace, governed by AGENTS.md and read
//! every session). Those files are user-authored identity/memory (SOUL.md,
//! MEMORY.md, IDENTITY.md), so an unmanaged projection write would violate
//! authored-file ownership. This revision therefore brokers projection, naming
//! the surfaces; native projection onto the workspace is the next slice.
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

pub const CLIENT: &str = "openclaw";
pub const PRODUCT: &str = "OpenClaw";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:openclaw-adapter";

/// Stable evidence refs pointing at primary sources, not prose.
const EV_VERSION: &str = "native:openclaw --version = 2026.1.30 (76b5208)";
const EV_REPO: &str = "doc:github.com/openclaw/openclaw";
const EV_CONFIG: &str = "native:/Users/admin/.openclaw/openclaw.json";
const EV_AGENTS_MD: &str = "native:/Users/admin/.openclaw/workspace/AGENTS.md";
const EV_WORKSPACE: &str =
    "native:/Users/admin/.openclaw/workspace (IDENTITY.md, hooks/, knowledge/, memory/)";
const EV_SKILLS: &str =
    "native:openclaw skills list (54 skills, 17 ready, openclaw-bundled; ClawHub) + doc:docs.openclaw.ai/cli/skills";
const EV_HOOKS: &str =
    "native:openclaw hooks list (boot-md, command-logger, session-memory; enable/disable/install) + doc:docs.openclaw.ai/cli/hooks";
const EV_MCP: &str =
    "native:/Users/admin/.openclaw/mcp.json (mcpServers: bimba-mcp, linear-server)";
const EV_PLUGINS: &str = "native:openclaw plugins --help (list/info/enable/disable in config)";
const EV_SESSIONS: &str =
    "native:/Users/admin/.openclaw/agents/main/sessions (*.jsonl + sessions.json) + openclaw sessions --help";
const EV_AGENTS: &str =
    "native:openclaw agents --help (isolated agents: workspaces + auth + routing) + doc:docs.openclaw.ai/cli/agents";
const EV_SUBAGENTS: &str =
    "native:openclaw.json agents.defaults.subagents.maxConcurrent=8 + /Users/admin/.openclaw/subagents";
const EV_SURFACES: &str =
    "native:openclaw --help (tui, dashboard, message, channels, acp) + /Users/admin/.openclaw/telegram";
const EV_MEMORY: &str = "native:/Users/admin/.openclaw/memory/main.sqlite (session-memory hook)";
const EV_CAPABILITY_UNDECLARED: &str =
    "native:actuation harness capability openclaw -> undeclared (declared: claude-code, codex, zcode)";
/// Actuation owns detection; AIKit consumes the record. Cited per the
/// `actuation.harness-detection/v1` schema (catalog r4); this is the record my
/// own `actuation harness detect --json --versions` run produced.
const EV_DETECTION: &str = "actuation.harness-detection/v1 detection:2026-09-06T23:42:43.943Z openclaw:detected exe:/Users/admin/Library/pnpm/openclaw sha256:bf7273d8f96922a7129b4db26fb8f17d56e032ce0033ed9a7d7f45b1b6b1a648 observed:2026-09-06T23:42:43.943Z version:2026.1.30 facets:agents(1),config";

pub struct OpenclawAdapter {
    /// Where a future native workspace projection would be written. Unused by
    /// the brokered plan in this revision; kept as the explicit
    /// projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl OpenclawAdapter {
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

fn openclaw_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_AGENTS_MD, EV_WORKSPACE, EV_AGENTS, EV_MEMORY],
            Some(
                "the per-agent workspace is governed by AGENTS.md, which directs every \
                 session to read SOUL.md (identity), USER.md, PARADIGM.md, MEMORY.md and \
                 memory/YYYY-MM-DD.md — a real standing-instruction surface on disk",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_CONFIG, EV_AGENTS_MD],
            Some(
                "agents.defaults.workspace in openclaw.json pins the workspace root and \
                 AGENTS.md governs it per agent; workspaces are per-agent isolated trees",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS],
            Some(
                "54 bundled skills (17 ready at observation) with `openclaw skills \
                 list/info/check` and ClawHub install/publish",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_HOOKS, EV_REPO],
            Some(
                "hook packs install via `openclaw hooks install` and toggle via \
                 enable/disable; boot-md runs BOOT.md on gateway startup and \
                 session-memory fires on /new",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unknown,
            &[EV_AGENTS_MD],
            Some(
                "every-session re-read of workspace files is documented in AGENTS.md, but \
                 no evidence was observed that a running gateway re-reads them mid-session",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_AGENTS_MD, EV_SESSIONS, EV_MEMORY],
            Some(
                "AGENTS.md's \"Every Session\" contract re-reads the workspace files and \
                 today's/yesterday's memory notes on each new session; sessions are \
                 persisted as jsonl stores",
            ),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Degraded,
            &[EV_HOOKS, EV_PLUGINS, EV_CONFIG],
            Some(
                "hooks/plugins/config are toggled through CLI helpers, but whether the \
                 running gateway applies each change without a restart is not documented \
                 in the observed 2026.1.30 surface",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_MCP, EV_CONFIG],
            Some(
                "MCP servers are declared in ~/.openclaw/mcp.json (bimba-mcp, \
                 linear-server) and executed with exec-approval gating \
                 (exec-approvals.json/sock observed)",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_PLUGINS, EV_MCP],
            Some(
                "plugins/extensions are discovered and toggled in config \
                 (`openclaw plugins list/enable/disable`); MCP servers contribute tools \
                 to agent turns",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_SESSIONS],
            Some(
                "stored conversation sessions are listed via `openclaw sessions` and \
                 persisted under agents/main/sessions as jsonl transcripts with a \
                 sessions.json index",
            ),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_AGENTS, EV_SUBAGENTS, EV_CAPABILITY_UNDECLARED],
            Some(
                "`openclaw agents` manages isolated agents (workspaces + auth + routing), \
                 subagents run with maxConcurrent 8; Actuation has no capability \
                 descriptor for openclaw yet (`actuation harness capability openclaw` is \
                 undeclared) so this census rests on native observation",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_CONFIG, EV_AGENTS],
            Some(
                "agents.defaults.workspace is explicit config, and each isolated agent \
                 gets its own workspace/state root",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Degraded,
            &[EV_SURFACES],
            Some(
                "no user-authored UI-component tree; the dashboard/Control UI and TUI are \
                 built-in surfaces rather than composable components",
            ),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_SURFACES, EV_VERSION, EV_DETECTION],
            Some(
                "terminal UI (`openclaw tui`), dashboard Control UI, message/channel \
                 surfaces (telegram channel dir present) and ACP; detection confirmed \
                 the CLI+config on this machine",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Degraded,
            &[EV_HOOKS, EV_SESSIONS],
            Some(
                "skills/hooks/plugins can be disabled via CLI and deleted sessions are \
                 tombstoned (*.jsonl.deleted.* observed), but live retraction from an \
                 already-running session is unverified — a restart/new session is the \
                 honest boundary",
            ),
        ),
    ]
}

impl TargetAdapter for OpenclawAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            // No evidence of a file watch; reload is per-session, not live.
            live_reload: false,
            symlinks: true,
            // Named profiles (`--profile <name>`) and isolated agents each get
            // their own state dir under ~/.openclaw[-<name>].
            isolated_per_context: true,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(
                "OpenClaw projection is brokered in this adapter revision; the census \
                 names the genuine surface (the per-agent workspace governed by \
                 AGENTS.md) but AIKit owns no representation on it yet",
            ),
        )
        .with_note(
            "the workspace files OpenClaw reads every session (SOUL.md, USER.md, \
             MISSION.md, MEMORY.md, memory/YYYY-MM-DD.md) are user-authored identity \
             and memory; writing them from a projection without an ownership rule \
             would conflict with authored files, so native projection onto the \
             workspace is the next slice"
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

impl HarnessAdmissionAdapter for OpenclawAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Observed locally: `openclaw --version` = 2026.1.30 (76b5208).
            native_version: Some("2026.1.30 (76b5208)".to_string()),
            source_revision: Some("76b5208".to_string()),
            // Bound to Actuation's detection identity for this harness
            // (`actuation harness detect`, catalog r4); AIKit consumes the
            // ref, it does not mint it. `actuation harness capability openclaw`
            // is undeclared (only claude-code, codex, zcode are) — noted in the
            // DelegatedAgents census rather than fabricated.
            realised_actuation_ref: Some("harness/openclaw".to_string()),
            project_binding_ref: None,
            faculties: openclaw_faculties(),
        }
    }
}
