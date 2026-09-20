//! Hermes Agent — harness-adapter admission through the harness-adapter
//! contract, for both catalog slugs Actuation declares: `hermes` (the CLI
//! agent) and `hermes-acp` (the Agent Client Protocol bridge over the same
//! agent and config tree).
//!
//! Evidence base (primary sources, observed live 2026-09-18 on the Omarchy
//! machine by this adapter's authoring session):
//! - `hermes --version` → "Hermes Agent v0.19.0 (2026.7.20)", installed via
//!   pip under mise (`pipx-hermes-agent/0.19.0`); `hermes-acp --version` →
//!   0.19.0 (the bridge ships with the agent, sharing `~/.hermes`).
//! - `hermes --help` → first-class harness surface: `skills`, `hooks`, `mcp`,
//!   `sessions`, `memory`, `project`, `acp`, `serve`, `gateway`, `dashboard`;
//!   flags `--provider`, `-m MODEL`, `--resume SESSION`,
//!   `--continue [NAME]`, `--skills SKILLS` (session preload), `--tui`/`--cli`,
//!   `--ignore-user-config`, `--ignore-rules`.
//! - `hermes skills list` → "0 hub-installed, 0 builtin, 4 local — 4 enabled":
//!   SKILL.md-frontmatter directories dropped into `~/.hermes/skills` are
//!   discovered as local skills (observed: `omarchy`, `diagnose-crash`,
//!   `devops/actuation-gateway-resident-carrier`, `devops/remote-host-bootstrap`).
//! - `hermes acp --help` → "Start Hermes Agent in ACP mode for editor
//!   integration (VS Code, Zed, JetBrains)".
//! - `~/.hermes` tree: `SOUL.md` (global personal instructions), `config.yaml`
//!   (model defaults glm-5.3-flash @ provider zai; `platform_toolsets.cli`
//!   incl. `delegation`, `skills`, `terminal`, `memory`; `plugins.enabled`),
//!   `memories/`, `sessions/`, `hooks/`, `state.db`.
//!
//! ## Projection stance
//!
//! The skills tree is a real, live-proven discovery surface — but it is
//! hermes-authored and -managed ground (`hermes skills` installs, audits and
//! tracks it via `.hub/` and `state.db`), and AIKit has no declared seam on
//! it: Actuation's catalog carries capability-gap documents for both slugs,
//! no capability descriptor. A plan item cannot even name the destination —
//! projection items are relative by construction and the materialisation
//! base for a user-home tree rides a capability descriptor's install seam.
//! So this revision brokers projection, exactly like the gemini admission,
//! and records the observed discovery seam in the census so the native
//! projection slice is mechanical once a descriptor declares the seam.

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

pub const CLIENT: &str = "hermes";
pub const CLIENT_ACP: &str = "hermes-acp";
pub const PRODUCT: &str = "Hermes Agent";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:hermes-adapter";

/// Stable evidence refs pointing at primary sources captured live, not prose.
const EV_SOUL: &str = "native:~/.hermes/SOUL.md (authored personal-instruction surface)";
const EV_CONFIG: &str =
    "native:~/.hermes/config.yaml (model defaults glm-5.3-flash @ zai; platform_toolsets.cli; plugins.enabled)";
const EV_HELP: &str =
    "native:hermes --help (subcommands skills/hooks/mcp/sessions/memory/project/acp; \
 --provider/-m/--resume/--continue/--skills/--tui/--cli/--ignore-user-config)";
const EV_SKILLS_LIST: &str =
    "native:hermes skills list = 4 local skills enabled, 0 hub-installed (SKILL.md dirs in ~/.hermes/skills)";
const EV_SKILLS_TREE: &str =
    "native:~/.hermes/skills (local SKILL.md dirs discovered; .hub/ manager state)";
const EV_SKILLS_CMD: &str =
    "native:hermes skills --help (browse/search/install/inspect/list/update/audit/uninstall/snapshot)";
const EV_HOOKS: &str =
    "native:hermes --help (subcommand hooks; --accept-hooks gates unseen shell hooks)";
const EV_MCP: &str = "native:hermes mcp --help = \"Manage MCP server connections and run Hermes \
 as an MCP server\" (add/list/test client side, serve server side)";
const EV_ACP: &str =
    "native:hermes acp --help = \"Start Hermes Agent in ACP mode for editor integration \
 (VS Code, Zed, JetBrains)\"; hermes-acp --version = 0.19.0";
const EV_VERSION: &str = "native:hermes --version = Hermes Agent v0.19.0 (2026.7.20), pip via mise";

/// Which catalog slug this adapter answers for: the agent itself or its ACP
/// bridge. The bridge shares `~/.hermes` and the agent process; it differs in
/// surface (editor integration over Agent Client Protocol) and therefore in
/// census, not in identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Cli,
    Acp,
}

pub struct HermesAdapter {
    variant: Variant,
    /// Where a future native skills projection would be staged. Unused by the
    /// brokered plan in this revision; kept as the explicit projection-root
    /// seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl HermesAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            variant: Variant::Cli,
            root: root.into(),
        }
    }

    /// The `hermes-acp` bridge: same config tree, ACP wire surface.
    pub fn acp(root: impl Into<PathBuf>) -> Self {
        Self {
            variant: Variant::Acp,
            root: root.into(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn target(&self) -> TargetId {
        match self.variant {
            Variant::Cli => TargetId::hermes(),
            Variant::Acp => TargetId::hermes_acp(),
        }
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

fn hermes_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_SOUL, EV_HELP],
            Some("SOUL.md is the global personal-instruction surface; --ignore-user-config runs without it"),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_HELP],
            Some(
                "chat injects project AGENTS.md/rules and memory alongside user config \
                 (--ignore-rules drops the rules leg)",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS_LIST, EV_SKILLS_TREE, EV_SKILLS_CMD],
            Some(
                "SKILL.md-frontmatter directories under ~/.hermes/skills are discovered as \
                 local skills; hermes' own manager tracks the tree, so AIKit projection \
                 awaits a declared seam (capability descriptor) rather than an unmanaged write",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Degraded,
            &[EV_HOOKS],
            Some("hooks exist (~/.hermes/hooks, hermes hooks), but unseen hooks are accept-gated; \
                 unattended AIKit dispatch is not evidenced"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unsupported,
            &[],
            Some("no file-watcher reload is documented; surfaces are read per invocation"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[EV_HELP],
            Some(
                "the chat surface is process-per-invocation (a restart is a new process); \
                 long-running surfaces (serve/gateway) exist but carry no evidenced reload seam",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_MCP],
            Some("mcp subcommand exposes client and server tool-protocol surfaces"),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_CONFIG],
            Some("plugins contribute toolsets (observed enabled: herdr-agent-state; known_plugin_toolsets)"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_HELP],
            Some("--resume SESSION, --continue [NAME], and the sessions subcommand"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_CONFIG],
            Some("the cli platform toolset includes delegation; kanban/portal coordinate multi-agent work"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_HELP, EV_ACP, EV_VERSION],
            Some(
                "chat (--tui/--cli), acp editor bridge (VS Code/Zed/JetBrains), send/gateway \
                 messaging, serve/dashboard",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Degraded,
            &[EV_SKILLS_CMD],
            Some("hermes skills uninstall/reset remove skills for the next session; no push \
                 retraction of already-loaded context is documented"),
        ),
    ]
}

fn hermes_acp_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_SOUL],
            Some("the bridge rides the same ~/.hermes authored ground as the agent"),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Degraded,
            &[EV_SKILLS_TREE],
            Some("shares the hermes skills tree; owns no independent skill surface"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_ACP],
            Some("Agent Client Protocol bridge for editor integration (VS Code, Zed, JetBrains)"),
        ),
    ]
}

impl TargetAdapter for HermesAdapter {
    fn target(&self) -> TargetId {
        self.target()
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: true,
            // One user-level config tree shared by every session; there is no
            // per-context surface to isolate.
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        let (who, surfaces) = match self.variant {
            Variant::Cli => (
                "Hermes Agent",
                "~/.hermes/skills, SOUL.md, AGENTS.md/rules, hooks",
            ),
            Variant::Acp => (
                "The hermes-acp bridge",
                "the shared ~/.hermes tree over the ACP wire",
            ),
        };
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(format!(
                "{who} projection is brokered in this adapter revision; the census names the \
                     genuine surfaces ({surfaces}) but AIKit owns no declared seam on them yet — \
                     the catalog carries capability-gap documents for both slugs, no descriptor"
            )),
        )
        .with_note(
            "~/.hermes/skills is a live discovery surface (SKILL.md dirs are adopted as local \
                 skills) and hermes' own manager tracks it; a capability descriptor declaring that \
                 seam is what turns this plan native, not an unmanaged write"
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

impl HarnessAdmissionAdapter for HermesAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // The catalog declares the bridge's edition `acp-bridge`, which
            // the HarnessEditionKind vocabulary has no member for; `cli` is
            // the least-distorting member (a command-line bridge process) —
            // the same choice the embedded profile makes.
            edition: HarnessEditionKind::Cli,
            native_version: Some("0.19.0".to_string()),
            source_revision: Some("hermes-agent 0.19.0 (2026.7.20), pip via mise".to_string()),
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: match self.variant {
                Variant::Cli => hermes_faculties(),
                Variant::Acp => hermes_acp_faculties(),
            },
        }
    }
}
