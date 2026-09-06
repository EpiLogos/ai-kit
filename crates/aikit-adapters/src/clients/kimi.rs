//! Kimi CLI — admitted through the harness-adapter contract.
//!
//! Kimi CLI (MoonshotAI/kimi-cli, `kimi` on PATH, installed as a uv tool) is a
//! Python terminal AI coding agent. Discovery rode Actuation harness detection
//! (`actuation harness detect --json --versions`, run 2026-09-07): the record
//! below is the receipt. Its instruction surface is file-based and
//! discoverable: `AGENTS.md` is merged from the project root down to the
//! working directory (including `.kimi/AGENTS.md`) and injected as
//! `${KIMI_AGENTS_MD}` into agent system prompts; skills are `SKILL.md`
//! directories discovered from layered roots (`~/.kimi/skills`,
//! `~/.config/agents/skills`, project scope, built-ins); hooks cover 13
//! lifecycle events including `SessionStart`; sessions persist under
//! `~/.kimi/sessions` and resume with `--continue`/`--session`; subagents
//! (`coder`/`explore`/`plan`) run via the built-in `Agent` tool; MCP servers
//! and plugins (`plugin.json`, Beta) contribute native tools; surfaces beyond
//! the TUI include `kimi web`, `kimi acp`, and `kimi term`.
//!
//! Version skew is census material: the installed binary is 1.6 (observed via
//! `kimi --version`); the latest GitHub release is 1.50.0 (2026-09-01). The
//! doc URLs below describe the current release line; where behavior could
//! have drifted between 1.6 and 1.50.0 the note says so.
//!
//! This adapter projects onto the one surface the harness provably reads from
//! the project root — `AGENTS.md` — and records an evidence-backed census of
//! the rest. No capability descriptor is declared for slug `kimi` in
//! Actuation (`actuation harness capability kimi` says so explicitly); the
//! census below is authored from the detection record, direct observation of
//! the installed harness, and primary docs.
//!
//! ## Identity law
//!
//! A model running in Kimi CLI is not the Agent identity; Kimi CLI is not the
//! World; a Kimi CLI process is not an AgentSession. The adapter keeps
//! `target`, `product`, and any `realised_actuation_ref` distinct, and never
//! fabricates a loaded-activation claim (`verify_activation_truth` rejects
//! it).
//!
use std::path::{Path, PathBuf};

use aikit_core::harness_admission::{
    FacultySupport, HarnessAdmissionAdapter, HarnessAdmissionDescriptor, HarnessEditionKind,
    HarnessFaculty, HarnessFacultyObservation, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
    TargetCapabilities,
};
use aikit_core::Result;

pub const CLIENT: &str = "kimi";
pub const PRODUCT: &str = "Kimi CLI";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:kimi-adapter";

/// Detection disclosure: the Actuation detection record this admission cites,
/// produced by `actuation harness detect --json --versions` run on this
/// machine. Executable and config-dir probes both passed; facet
/// `~/.kimi/config.toml` exists.
const EV_DETECTION: &str = "actuation.harness-detection/v1 \
detection:2026-09-06T23:42:48.747Z kimi:detected exe:/Users/admin/.local/bin/kimi \
sha256:ce20004bf1e26934645fa501705dfaade9a3ad0778e8639dd350681d52f8a515 \
observed:2026-09-06T23:42:48.747Z";

/// Stable evidence refs so conformance can cite exact sources rather than prose.
const EV_AGENTS_MD_DOC: &str = "https://moonshotai.github.io/kimi-cli/en/customization/agents.md";
const EV_CONFIG_FILES_DOC: &str =
    "https://moonshotai.github.io/kimi-cli/en/configuration/config-files.md";
const EV_SKILLS_DOC: &str = "https://moonshotai.github.io/kimi-cli/en/customization/skills.md";
const EV_HOOKS_DOC: &str = "https://moonshotai.github.io/kimi-cli/en/customization/hooks.md";
const EV_SESSIONS_DOC: &str = "https://moonshotai.github.io/kimi-cli/en/guides/sessions.md";
const EV_MCP_DOC: &str = "https://moonshotai.github.io/kimi-cli/en/customization/mcp.md";
const EV_PLUGINS_DOC: &str = "https://moonshotai.github.io/kimi-cli/en/customization/plugins.md";
const EV_DATA_LOCATIONS_DOC: &str =
    "https://moonshotai.github.io/kimi-cli/en/configuration/data-locations.md";
const EV_REPO: &str = "https://github.com/MoonshotAI/kimi-cli";
const EV_RELEASE_1_50_0: &str = "https://github.com/MoonshotAI/kimi-cli/releases/tag/1.50.0";
/// Observed 2026-09-07 via `kimi --help`: subcommands `term`, `web`, `acp`,
/// plus the experimental `--wire` server flag.
const EV_HELP_SURFACES: &str = "native:kimi --help (term/web/acp subcommands, --wire server)";

pub struct KimiAdapter {
    /// Where the projected `AGENTS.md` would be written (the project root).
    root: PathBuf,
}

impl KimiAdapter {
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

fn kimi_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_DETECTION, EV_CONFIG_FILES_DOC, EV_RELEASE_1_50_0],
            Some(
                "user-level ~/.kimi/config.toml exists on disk (detection facet); it carries \
                 default model, providers, loop control, services, MCP client settings, and \
                 [[hooks]]. Installed 1.6 lags release 1.50.0; config claims cite the \
                 current docs",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_AGENTS_MD_DOC],
            Some(
                "AGENTS.md is merged from the project root down to the working directory \
                 (including .kimi/AGENTS.md) and exposed as ${KIMI_AGENTS_MD} in system \
                 prompts",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS_DOC],
            Some(
                "SKILL.md directories discovered from layered roots (project > user > extra \
                 > built-in), incl. ~/.kimi/skills and ~/.config/agents/skills; --skills-dir \
                 overrides discovery",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_HOOKS_DOC],
            Some(
                "SessionStart hook fires when a session is created/resumed (source: \
                 startup|resume) among 13 lifecycle events configured in \
                 ~/.kimi/config.toml",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Degraded,
            &[],
            Some(
                "no documented mid-session re-read of AGENTS.md/skills; skills are injected \
                 at startup and hooks fire on lifecycle/tool events, not projected-file \
                 changes",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_AGENTS_MD_DOC, EV_SKILLS_DOC],
            Some("each new invocation re-merges AGENTS.md and re-discovers skills from disk"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[],
            Some(
                "process-per-invocation CLI: there is no persistent client process to \
                 restart; a fresh invocation is the reload channel",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_MCP_DOC, EV_HELP_SURFACES],
            Some(
                "built-in tool modules (kimi_cli.tools.*) plus MCP client: ~/.kimi/mcp.json, \
                 --mcp-config/--mcp-config-file",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_PLUGINS_DOC, EV_MCP_DOC],
            Some(
                "plugins (Beta) declare executable tools via plugin.json; MCP servers \
                 contribute tools natively",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_SESSIONS_DOC, EV_DETECTION],
            Some(
                "--continue/--session resume replays history and restores approval, plan \
                 mode, subagent, and added-directory state; ~/.kimi/sessions observed on \
                 disk",
            ),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_AGENTS_MD_DOC],
            Some(
                "built-in Agent tool with coder/explore/plan subagent types; custom \
                 subagents definable in agent YAML files",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_DATA_LOCATIONS_DOC, EV_HELP_SURFACES],
            Some(
                "sessions grouped by working-directory MD5 under ~/.kimi/sessions; --work-dir \
                 selects the root and --add-dir adds workspace roots",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unknown,
            &[],
            Some("no component/slots-style composition surface observed in primary sources"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_HELP_SURFACES],
            Some(
                "beyond the TUI: `kimi web` browser UI, `kimi acp` (deprecated flag form too), \
                 `kimi term` TUI, experimental --wire server",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unsupported,
            &[],
            Some(
                "removal of a skill/instruction file takes effect on the next invocation; \
                 nothing is retracted from a running session",
            ),
        ),
    ]
}

impl TargetAdapter for KimiAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: true,
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: false,
            watches_for_changes: false,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        // Kimi provably merges `AGENTS.md` from the project root into its
        // system prompt (${KIMI_AGENTS_MD}); that is the one surface we
        // project onto. Everything else in the census is native to the harness
        // and reached on its own terms.
        let mut contents = String::from(
            "# AGENTS.md\n\nProjected by ai-kit. Kimi CLI merges this file from the project root into its system prompt.\n",
        );
        for capsule in context.capsule_roots.keys() {
            contents.push_str(&format!("- capsule: {capsule}\n"));
        }
        let item = ProjectionItem::write("AGENTS.md", contents)?;
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::next_session_only(
                "Kimi CLI merges AGENTS.md when a session starts; mid-session edits are not picked up",
            ),
        )
        .with_item(item)
        .with_note(
            "projected surface is the project-root AGENTS.md only; skills, hooks, MCP, \
             plugins, and subagents are native faculties recorded in the admission census"
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

impl HarnessAdmissionAdapter for KimiAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Observed 2026-09-07 via `kimi --version` (installed uv tool
            // kimi-cli 1.6 at ~/.local/share/uv/tools/kimi-cli). Latest GitHub
            // release is 1.50.0 (2026-09-01); the skew is noted in the census.
            native_version: Some("1.6".to_string()),
            // MoonshotAI/kimi-cli main HEAD observed 2026-09-07 via
            // `gh api repos/MoonshotAI/kimi-cli/commits/main`.
            source_revision: Some("86f136422a0a".to_string()),
            realised_actuation_ref: None,
            project_binding_ref: Some(EV_REPO.to_string()),
            faculties: kimi_faculties(),
        }
    }
}
