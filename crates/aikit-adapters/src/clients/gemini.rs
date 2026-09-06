//! Gemini CLI — harness-adapter admission through the harness-adapter contract.
//!
//! Evidence base (primary sources, observed 2026-09-06):
//! - `gemini --version` → 0.29.5 installed at /opt/homebrew/bin/gemini; npm
//!   `@google/gemini-cli` latest is 0.58.0 (`npm view`).
//! - Repo `google-gemini/gemini-cli` (default branch `main`) docs fetched from
//!   raw.githubusercontent.com: docs/cli/gemini-md.md (GEMINI.md hierarchy),
//!   docs/cli/skills.md (~/.gemini/skills, .gemini/skills, SKILL.md),
//!   docs/hooks/reference.md (settings.json hooks incl. SessionStart),
//!   docs/cli/custom-commands.md (~/.gemini/commands, /commands reload),
//!   docs/cli/session-management.md (--resume, /resume),
//!   docs/tools/memory.md, docs/cli/settings.md, docs/core/subagents.md,
//!   docs/extensions/index.md, docs/cli/checkpointing.md.
//! - Local native artifacts: ~/.gemini/GEMINI.md, ~/.gemini/settings.json
//!   (mcpServers, ide), `gemini --help` (mcp/extensions/skills/hooks subcommands,
//!   -p/--output-format, --resume, --include-directories, --experimental-acp).
//!
//! ## Identity law
//!
//! A model running in Gemini CLI is not the Agent identity; Gemini CLI is not
//! the World; a gemini process is not an AgentSession. The adapter keeps
//! `target`, `product`, and any `realised_actuation_ref` distinct, and never
//! fabricates a loaded-activation claim (`verify_activation_truth` rejects it).
//!
//! ## Projection stance
//!
//! Genuine on-disk surfaces exist and are named exactly in the census
//! (`GEMINI.md` read from the project directory upward; `.gemini/skills/` with
//! `SKILL.md`; `.gemini/commands/`; `~/.gemini/settings.json` hooks). This
//! revision still brokers projection: AIKit capability payloads have no
//! adapter-owned representation on those surfaces yet, and `GEMINI.md` is a
//! user-authored convention where an unmanaged write would violate
//! authored-file ownership. Native projection onto the named surfaces is the
//! next slice, not this admission.
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

pub const CLIENT: &str = "gemini-cli";
pub const PRODUCT: &str = "Gemini CLI";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:gemini-adapter";

/// Stable evidence refs pointing at primary sources, not prose.
const EV_GEMINI_MD: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/cli/gemini-md.md";
const EV_GEMINI_MD_NATIVE: &str = "native:/Users/admin/.gemini/GEMINI.md";
const EV_SETTINGS: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/cli/settings.md";
const EV_SETTINGS_NATIVE: &str = "native:/Users/admin/.gemini/settings.json";
const EV_SKILLS: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/cli/skills.md";
const EV_HOOKS: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/hooks/reference.md";
const EV_HOOKS_NATIVE: &str = "native:gemini hooks --help (0.29.5)";
const EV_COMMANDS: &str =
    "doc:github.com/google-gemini/gemini-cli/main/docs/cli/custom-commands.md";
const EV_SESSIONS: &str =
    "doc:github.com/google-gemini/gemini-cli/main/docs/cli/session-management.md";
const EV_SESSIONS_NATIVE: &str = "native:gemini --help --resume";
const EV_MEMORY: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/tools/memory.md";
const EV_SUBAGENTS: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/core/subagents.md";
const EV_EXTENSIONS: &str = "doc:github.com/google-gemini/gemini-cli/main/docs/extensions/index.md";
const EV_NATIVE_VERSION: &str = "native:gemini --version = 0.29.5";
const EV_NPM: &str = "npm:@google/gemini-cli@0.58.0";

pub struct GeminiAdapter {
    /// Where a future native GEMINI.md/skills projection would be written.
    /// Unused by the brokered plan in this revision; kept as the explicit
    /// projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl GeminiAdapter {
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

fn gemini_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_GEMINI_MD, EV_GEMINI_MD_NATIVE, EV_MEMORY],
            Some("global personal instructions and memory live in ~/.gemini/GEMINI.md"),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_GEMINI_MD],
            Some(
                "the CLI searches for GEMINI.md from the workspace directory upward \
                 (hierarchical context); filename is configurable via context.fileName",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS, EV_NATIVE_VERSION],
            Some(
                "SKILL.md skills discovered from built-in, extension, user \
                 (~/.gemini/skills, ~/.agents/skills) and workspace (.gemini/skills) tiers",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_HOOKS, EV_HOOKS_NATIVE],
            Some(
                "hooks are declared in settings.json; SessionStart is a documented \
                 firing event; installed 0.29.5 already exposes the `gemini hooks` command",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Supported,
            &[EV_GEMINI_MD, EV_SKILLS, EV_COMMANDS],
            Some(
                "explicit slash-command reload: /memory reload re-scans GEMINI.md, \
                 /skills reload re-discovers skills, /commands reload re-reads commands; \
                 not an automatic file watch",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_GEMINI_MD, EV_SETTINGS],
            Some("context files and settings are re-read at session start by construction"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Degraded,
            &[EV_SETTINGS],
            Some(
                "no documented restart-specific reload contract; /settings applies many \
                 changes live and some settings only take effect on a fresh start",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_SETTINGS_NATIVE, EV_NATIVE_VERSION],
            Some(
                "built-in tools plus MCP servers declared under mcpServers in \
                 settings.json and managed via `gemini mcp`",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_SETTINGS_NATIVE, EV_EXTENSIONS],
            Some("MCP servers contribute native tools; extensions can bundle MCP servers"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_SESSIONS, EV_SESSIONS_NATIVE],
            Some("gemini --resume (latest, index, or UUID) and the in-session /resume command"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_SUBAGENTS, EV_EXTENSIONS, EV_NPM],
            Some(
                "sub-agents are a documented (preview) feature and can ship via extensions; \
                 availability varies by version (installed 0.29.5, npm latest 0.58.0)",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_GEMINI_MD, EV_SESSIONS_NATIVE],
            Some(
                "context discovery walks up from the current working directory; \
                 --include-directories extends the workspace",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Degraded,
            &[EV_EXTENSIONS],
            Some(
                "extensions bundle prompts, commands, themes, hooks, sub-agents and skills; \
                 there is no user-authored UI component tree",
            ),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_NATIVE_VERSION, EV_SETTINGS_NATIVE],
            Some(
                "interactive TUI, headless mode (-p/--prompt, --output-format json), \
                 IDE integration (ide.enabled in settings), experimental ACP mode",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Supported,
            &[EV_SKILLS, EV_HOOKS, EV_GEMINI_MD],
            Some(
                "/skills disable removes a skill for the session, hooks can deny tool \
                 calls live, and /memory reload picks up removed context files",
            ),
        ),
    ]
}

impl TargetAdapter for GeminiAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            // Reload exists only as explicit slash commands (/memory, /skills,
            // /commands reload); the CLI does not watch the projection.
            live_reload: false,
            symlinks: true,
            // No per-context config-dir surface: global ~/.gemini plus the
            // project's own .gemini tree are the only config homes.
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
                "Gemini CLI projection is brokered in this adapter revision; the census \
                 names the genuine surfaces (GEMINI.md, .gemini/skills, .gemini/commands, \
                 settings.json hooks) but AIKit owns no representation on them yet",
            ),
        )
        .with_note(
            "GEMINI.md is a user-authored context convention the CLI reads from the \
             project directory upward, and .gemini/skills carries SKILL.md trees; writing \
             either from a projection without an ownership rule would conflict with \
             authored files, so native projection onto those surfaces is the next slice"
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

impl HarnessAdmissionAdapter for GeminiAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Observed locally: `gemini --version` = 0.29.5 (npm latest 0.58.0).
            native_version: Some("0.29.5".to_string()),
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: gemini_faculties(),
        }
    }
}
