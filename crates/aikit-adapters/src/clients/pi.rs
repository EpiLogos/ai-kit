//! Pi — Mario Zechner's AI coding agent (earendil-works/pi), admitted through
//! the harness-adapter contract.
//!
//! Pi is a per-invocation CLI/TUI agent with a documented, discoverable
//! on-disk surface: global `~/.pi/agent/` (AGENTS.md instructions, skills with
//! SKILL.md frontmatter, extensions, prompt templates, themes) and a
//! trust-gated project-local `.pi/` tree (`.pi/skills/`, `.pi/extensions/`,
//! `.pi/prompts/`, `.pi/settings.json`). Discovery rides an Actuation
//! `actuation.harness-detection/v1` record (state `detected`, config dir
//! `~/.pi/agent`, skills facet count 14) supplemented by direct observation of
//! the installed binary (`pi --version` = 0.84.4) and the upstream docs at
//! `github.com/earendil-works/pi` (repo moved from `badlogic/pi-mono`).
//!
//! Because pi provably discovers skills from a project-relative `.pi/skills/`
//! directory (packages/coding-agent/docs/skills.md), this adapter projects
//! skill capsules there with `ProjectionItem::write`/`link` — a real plan, not
//! a brokered placeholder. Activation is `NextSessionOnly`: pi reads the
//! surface at session start; a running TUI can pick changes up with `/reload`
//! (usage.md), but a fresh invocation is the guaranteed pickup, so `Loaded`
//! overclaims (see `verify_activation_truth`).
//!
//! ## Identity law
//!
//! A model running in pi is not the Agent identity; pi is not the World; a pi
//! process is not an AgentSession. No `realised_actuation_ref` is fabricated —
//! Actuation declared no capability descriptor for `pi` at admission time
//! (`actuation harness capability pi`: undeclared), which the census records
//! honestly.

use std::path::{Path, PathBuf};

use aikit_core::capsule::Kind;
use aikit_core::harness_admission::{
    FacultySupport, HarnessAdmissionAdapter, HarnessAdmissionDescriptor, HarnessEditionKind,
    HarnessFaculty, HarnessFacultyObservation, HARNESS_ADAPTER_SDK_VERSION,
};
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, MaterializationMode, ProjectionPlan, ResolvedContext, TargetAdapter,
    TargetCapabilities,
};
use aikit_core::Result;

use super::agent_skills;

pub const CLIENT: &str = "pi";
pub const PRODUCT: &str = "Pi";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:pi-adapter";

/// Actuation detection record produced by this admission's own
/// `actuation harness detect --json --versions` run (catalog_revision 4).
const EV_DETECT: &str = "actuation.harness-detection/v1 \
detection:2026-09-06T23:42:55.567Z pi:detected exe:/Users/admin/.local/bin/pi \
sha256:5406c369954516fb56879d685e082ff9095cd6e06e41af406f394942377fd4bf \
observed:2026-09-06T23:42:55.567Z";

/// Version observed from the installed binary; matches the detection record.
const EV_VERSION: &str = "native:pi --version = 0.84.4 (matches detection record version)";

/// Upstream source tree observed 2026-09-06 (repo moved from badlogic/pi-mono).
const EV_REPO: &str =
    "source:github.com/earendil-works/pi @ 9767ba275f3e9a5ee0f5c5342249b629ab1b2282";

/// Skills documentation: discovery locations and SKILL.md frontmatter rules.
const EV_SKILLS_DOC: &str =
    "doc:https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md";

/// Extensions documentation: locations, ExtensionAPI, session hooks, tools.
const EV_EXTENSIONS_DOC: &str =
    "doc:https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md";

/// Usage documentation: context files, /reload, trust gating.
const EV_USAGE_DOC: &str =
    "doc:https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/usage.md";

/// Sessions documentation: resume/fork/tree, session storage.
const EV_SESSIONS_DOC: &str =
    "doc:https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sessions.md";

/// Themes documentation: global/project theme directories.
const EV_THEMES_DOC: &str =
    "doc:https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/themes.md";

/// Global skills directory observed on disk (14 entries, per detection facet).
const EV_SKILLS_DIR: &str = "facet:~/.pi/agent/skills count:14 (detection record)";

/// Global config observed on disk (detection config facet).
const EV_SETTINGS: &str = "native:~/.pi/agent/settings.json (detection config facet, exists)";

/// CLI surface observed directly (`pi --help`, 2026-09-06).
const EV_HELP: &str = "native:pi --help (options surface observed 2026-09-06)";

/// Official subagent extension example (planner/reviewer/scout/worker agents).
const EV_SUBAGENT_EXAMPLE: &str = "source:packages/coding-agent/examples/extensions/subagent/ \
(planner.md, reviewer.md, scout.md, worker.md)";

/// The projection subdirectory pi discovers, relative to the project root
/// (skills.md: project-local skills live in `.pi/skills/`).
const SKILLS_PREFIX: &str = ".pi/skills";

pub struct PiAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// plan writes project-relative `.pi/skills/...` items, so this root is not
    /// consulted in this revision.
    root: PathBuf,
    materialization: MaterializationMode,
}

impl PiAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            materialization: MaterializationMode::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn with_materialization(mut self, mode: MaterializationMode) -> Self {
        self.materialization = mode;
        self
    }

    /// The export name for a capability: its `export_name` config override, or
    /// the capsule's leaf — the same collision-resolution rule the Claude
    /// adapter uses, so two registries can each ship a `code-review`.
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

fn pi_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_USAGE_DOC, EV_SETTINGS, EV_DETECT],
            Some("global ~/.pi/agent/AGENTS.md is loaded at startup alongside settings.json"),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_USAGE_DOC],
            Some(
                "AGENTS.md / CLAUDE.md / AGENTS.override.md layer per directory at startup; \
                 --no-context-files disables discovery",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS_DOC, EV_SKILLS_DIR],
            Some(
                "~/.pi/agent/skills (14 entries observed) plus project-local .pi/skills; \
                 direct .md files with valid frontmatter are discovered as skills",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_EXTENSIONS_DOC],
            Some("extensions receive session-scoped events incl. session_start/session teardown"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Supported,
            &[EV_USAGE_DOC, EV_HELP],
            Some(
                "/reload in the running TUI reloads extensions, skills, prompts, themes, \
                 and context files — command-mediated, not a file watcher",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_HELP, EV_USAGE_DOC],
            Some("pi is process-per-invocation; every new session reads disk surfaces at startup"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[],
            Some("no persistent client or daemon to restart; a restart is a new process"),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_EXTENSIONS_DOC, EV_HELP],
            Some(
                "extensions register tools through ExtensionAPI; built-in/extension tools can be \
                 allow/deny-listed per invocation via --tools/--exclude-tools",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_EXTENSIONS_DOC],
            Some("extensions contribute native tools (examples: tools.ts, dynamic-tools.ts)"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_SESSIONS_DOC, EV_HELP],
            Some("--continue/--resume/--fork/--session plus /tree and /resume inside the TUI"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_SUBAGENT_EXAMPLE],
            Some(
                "official subagent extension ships planner/reviewer/scout/worker agents; \
                 extension-mediated, not a first-party command",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_USAGE_DOC, EV_SKILLS_DOC, EV_EXTENSIONS_DOC],
            Some(
                "project-local .pi/ surfaces (skills, extensions, prompts, settings) load per \
                 cwd after the project-trust decision; sessions are per-project",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Supported,
            &[EV_THEMES_DOC, EV_USAGE_DOC],
            Some(
                "theme system (~/.pi/agent/themes, --theme, /reload) and extension UI components \
                 (header/footer renderers, widgets)",
            ),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_HELP, EV_USAGE_DOC, EV_REPO, EV_VERSION],
            Some("interactive TUI, --print non-interactive, --mode text|json|rpc, HTML session export"),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Degraded,
            &[EV_HELP],
            Some(
                "pi remove/update rewrites settings and /reload or the next session drops the \
                 material; no push retraction of already-loaded context is documented",
            ),
        ),
    ]
}

impl TargetAdapter for PiAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            // The TUI's /reload picks up changed skills/extensions/prompts.
            live_reload: true,
            symlinks: true,
            // The projected surface is project-relative `.pi/skills/`, shared by
            // every session rooted at that project — the Codex shape.
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: true,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        // Pi provably discovers `.pi/skills/` from the project tree
        // (packages/coding-agent/docs/skills.md), so skills are projected
        // there as real write/link items — not brokered. Project-local .pi/
        // surfaces load only after pi's project-trust decision, which the note
        // states so the palette can print it.
        let mode = self.materialization.resolve_for(&self.capabilities());
        let mut plan = ProjectionPlan::new(
            self.target(),
            ActivationEffect::next_session_only(
                "pi discovers .pi/skills at session start; a running TUI can /reload \
                 skills, extensions, prompts, themes, and context files",
            ),
        );

        for capability in context.view.active_of_kind(Kind::Skill) {
            let Some(root) = context.root_of(&capability.id) else {
                plan = plan.with_note(format!(
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
            plan = plan.with_items(exported.project_effective(
                Path::new(SKILLS_PREFIX),
                mode,
                overlays,
            )?);
        }

        if plan.items.is_empty() {
            plan = plan.with_note(
                "no active skill capsules; pi's instruction/extension surfaces are recorded \
                 in the admission census, not projected here"
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

impl HarnessAdmissionAdapter for PiAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            native_version: Some("0.84.4".to_string()),
            source_revision: Some(
                "earendil-works/pi@9767ba275f3e9a5ee0f5c5342249b629ab1b2282".to_string(),
            ),
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: pi_faculties(),
        }
    }
}
