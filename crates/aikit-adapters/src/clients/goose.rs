//! Goose — admitted through the harness-adapter contract.
//!
//! Goose (aaif-goose/goose, formerly block/goose) is an open-source,
//! Rust-built general-purpose AI agent: desktop app, CLI, and API. Its
//! file surface is documented and discoverable: user config at
//! `~/.config/goose/config.yaml` (plus permission/secrets files); project
//! instructions via `.goosehints` files discovered at session start (with
//! nested expansion); persistent instructions injected every turn through the
//! MOIM working memory (`GOOSE_MOIM_MESSAGE_TEXT`/`_FILE`, e.g.
//! `~/.goose/guardrails.md`); skills as SKILL.md directories (Claude-compatible
//! Agent Skills); lifecycle hooks shipped as plugin `hooks/hooks.json`; recipes
//! (parameterized session templates); subagents for delegation; sessions that
//! can be resumed; extensions via MCP (70+ documented).
//!
//! This adapter projects onto the one project-root surface the harness
//! provably reads — `.goosehints` — and records an evidence-backed census of
//! the rest.
//!
//! ## Identity law
//!
//! A model running in Goose is not the Agent identity; Goose is not the World;
//! a Goose process is not an AgentSession. The adapter keeps `target`,
//! `product`, and any `realised_actuation_ref` distinct, and never fabricates
//! a loaded-activation claim (`verify_activation_truth` rejects it).

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

pub const CLIENT: &str = "goose";
pub const PRODUCT: &str = "Goose";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:goose-adapter";
/// Detection disclosure: Goose is absent from the Actuation harness catalog
/// (r4, observed 2026-09-06), so no `actuation.harness-detection/v1` record
/// exists for it. Under the detection contract's three-state law this is
/// disclosed unavailability, not absence-by-silence; the census evidence
/// below rests on primary docs and local artifacts.
/// Stable evidence refs so conformance can cite exact sources rather than prose.
const EV_CONFIG_DOC: &str =
    "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/config-files.md";
const EV_PERSISTENT_INSTRUCTIONS: &str = "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/using-persistent-instructions.md";
const EV_GOOSEHINTS_DOC: &str = "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/using-goosehints.md";
const EV_SKILLS_DOC: &str = "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/using-skills.md";
const EV_HOOKS_DOC: &str = "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/hooks.md";
const EV_SUBAGENTS_DOC: &str = "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/subagents.mdx";
const EV_SESSIONS_DOC: &str = "https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/sessions/session-management.md";
const EV_REPO_README: &str = "https://github.com/aaif-goose/goose";
const EV_MEMORY_MCP_DOC: &str =
    "https://github.com/aaif-goose/goose/blob/main/documentation/docs/mcp/memory-mcp.md";

pub struct GooseAdapter {
    /// Where the projected `.goosehints` would be written (the project root).
    root: PathBuf,
}

impl GooseAdapter {
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

fn goose_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_PERSISTENT_INSTRUCTIONS],
            Some(
                "persistent instructions are injected into MOIM working memory every turn; \
                 file-based via GOOSE_MOIM_MESSAGE_FILE (e.g. ~/.goose/guardrails.md)",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_GOOSEHINTS_DOC],
            Some(
                ".goosehints files load at session start and expand as nested hint \
                 files are discovered",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS_DOC],
            Some("Claude-compatible Agent Skills (SKILL.md) via the built-in skills extension"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_HOOKS_DOC],
            Some(
                "lifecycle hooks (Open Plugins spec) ship as plugin hooks/hooks.json and run \
                 as shell commands when session/tool events fire",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Supported,
            &[EV_PERSISTENT_INSTRUCTIONS],
            Some(
                "the MOIM instruction file is re-read fresh every turn, so edits take \
                 effect immediately without restarting; .goosehints themselves are \
                 session-start only",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_GOOSEHINTS_DOC],
            Some("project hints and project-scope plugins load when a session starts"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Degraded,
            &[],
            Some(
                "config.yaml edits are picked up on a new session/restart; no separate \
                 restart-only reload channel is documented",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_CONFIG_DOC],
            Some("15+ providers (Anthropic, OpenAI, Google, Ollama, OpenRouter, ...) via config"),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_REPO_README, EV_CONFIG_DOC],
            Some("70+ extensions via the Model Context Protocol, plus built-in extensions"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_SESSIONS_DOC],
            Some("sessions persist and can be resumed; chat history search available"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_SUBAGENTS_DOC],
            Some("subagents spawn as isolated instances, sequentially or in parallel"),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_HOOKS_DOC, EV_GOOSEHINTS_DOC],
            Some(
                "project scope: <project>/.agents/plugins and project .goosehints load when \
                 goose starts from that project; working directory switchable per session",
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
            &[EV_REPO_README],
            Some("native desktop app (macOS/Linux/Windows), full CLI, and an embeddable API"),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unsupported,
            &[EV_MEMORY_MCP_DOC],
            Some(
                "no documented mid-session retraction of already-loaded hints/instructions; \
                 memory MCP manages memories, not retraction of loaded context",
            ),
        ),
    ]
}

impl TargetAdapter for GooseAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: true,
            symlinks: true,
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: false,
            watches_for_changes: false,
        }
    }

    fn plan(&self, context: &ResolvedContext) -> Result<ProjectionPlan> {
        // Goose provably reads `.goosehints` from the project tree at session
        // start (docs/guides/context-engineering/using-goosehints.md); that is
        // the one project-root surface we project onto. User-scope surfaces
        // (~/.config/goose/config.yaml, MOIM env files) live outside the
        // projection root and are recorded in the census instead.
        let mut contents = String::from(
            "# goosehints\n\nProjected by ai-kit. Goose loads this file when a session starts.\n",
        );
        for capsule in context.capsule_roots.keys() {
            contents.push_str(&format!("- capsule: {capsule}\n"));
        }
        let item = ProjectionItem::write(".goosehints", contents)?;
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::next_session_only(
                "Goose loads .goosehints at session start; persistent-instruction live \
                 injection is a separate native env-file path",
            ),
        )
        .with_item(item)
        .with_note(
            "projected surface is the project-root .goosehints only; config.yaml, MOIM \
             persistent instructions, skills, hooks, recipes, and subagents are native \
             faculties recorded in the admission census"
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

impl HarnessAdmissionAdapter for GooseAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // The goose binary is not installed on this machine (`which goose`
            // empty); version observed from source instead.
            native_version: None,
            source_revision: Some("5e90925962f0".to_string()),
            realised_actuation_ref: None,
            project_binding_ref: Some(EV_REPO_README.to_string()),
            faculties: goose_faculties(),
        }
    }
}
