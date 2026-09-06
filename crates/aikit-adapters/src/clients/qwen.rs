//! Qwen Code — admitted through the harness-adapter contract.
//!
//! Qwen Code (QwenLM/qwen-code, npm `@qwen-code/qwen-code`) is an open-source
//! terminal AI coding agent — a Gemini CLI-lineage fork. Its instruction
//! surface is file-based and discoverable: `QWEN.md` (project root), global
//! `~/.qwen/QWEN.md`, personal `.qwen/QWEN.local.md`, plus `AGENTS.md` are
//! loaded at session start; skills are `SKILL.md` directories; hooks
//! (SessionStart/PreToolUse/PostToolUse/...) are configured in
//! `.qwen/settings.json`; MCP servers and Qwen extensions contribute native
//! tools; SubAgents and Agent Teams delegate work; sessions can be resumed.
//!
//! This adapter projects onto the one surface the harness provably reads from
//! the project root — `QWEN.md` — and records an evidence-backed census of
//! the rest.
//!
//! ## Identity law
//!
//! A model running in Qwen Code is not the Agent identity; Qwen Code is not
//! the World; a Qwen Code process is not an AgentSession. The adapter keeps
//! `target`, `product`, and any `realised_actuation_ref` distinct, and never
//! fabricates a loaded-activation claim (`verify_activation_truth` rejects
//! it).

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

pub const CLIENT: &str = "qwen-code";
pub const PRODUCT: &str = "Qwen Code";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:qwen-adapter";
/// Detection disclosure: Qwen Code is absent from the Actuation harness
/// catalog (r4, observed 2026-09-06), so no `actuation.harness-detection/v1`
/// record exists for it. Under the detection contract's three-state law this
/// is disclosed unavailability, not absence-by-silence; the census evidence
/// below rests on primary docs and local artifacts.

/// Stable evidence refs so conformance can cite exact sources rather than prose.
const EV_MEMORY_DOC: &str =
    "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/memory.md";
const EV_HOOKS_DOC: &str =
    "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/hooks.md";
const EV_SKILLS_DOC: &str =
    "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/skills.md";
const EV_MCP_DOC: &str = "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/mcp.md";
const EV_SUBAGENTS_DOC: &str =
    "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/sub-agents.md";
const EV_MULTIAGENT_DOC: &str =
    "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/multi-agent-coordination.md";
const EV_WORKTREE_DOC: &str =
    "https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/worktree.md";
const EV_REPO_README: &str = "https://github.com/QwenLM/qwen-code";
const EV_NPM_PACKAGE: &str = "https://www.npmjs.com/package/@qwen-code/qwen-code";
const EV_RESUME_COMMAND: &str = "QwenLM/qwen-code:packages/cli/src/ui/commands/resumeCommand.ts";
const EV_SESSION_RESTORE_DESIGN: &str =
    "QwenLM/qwen-code:docs/design/2026-08-07-safe-session-restore-timeout.md";
const EV_MEMORY_FORGET: &str = "QwenLM/qwen-code:packages/core/src/memory/forget.ts";

pub struct QwenAdapter {
    /// Where the projected `QWEN.md` would be written (the project root).
    root: PathBuf,
}

impl QwenAdapter {
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

fn qwen_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_MEMORY_DOC],
            Some("global ~/.qwen/QWEN.md loads at the start of every session"),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_MEMORY_DOC],
            Some(
                "project-root QWEN.md (team-shared), .qwen/QWEN.local.md (personal), \
                 and AGENTS.md are all read at session start",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS_DOC],
            Some("model-invoked SKILL.md directories, personal and project scope"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Supported,
            &[EV_HOOKS_DOC],
            Some(
                "SessionStart/SessionEnd/PreToolUse/PostToolUse hooks configured \
                 in .qwen/settings.json",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Degraded,
            &[],
            Some(
                "no documented mid-session reload of instruction files; hooks fire \
                 on lifecycle/tool events, not on projected-file changes",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_MEMORY_DOC],
            Some("QWEN.md files are re-loaded when a new session starts"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Degraded,
            &[],
            Some(
                "settings/hook configuration edits are picked up on a new session or \
                 restart, but no separate restart-only reload channel is documented",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_REPO_README],
            Some("multi-protocol: OpenAI, Anthropic, Gemini, and Qwen APIs, plus Ollama/vLLM"),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_MCP_DOC, EV_REPO_README],
            Some("MCP servers and Qwen extensions contribute native tools"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_RESUME_COMMAND, EV_SESSION_RESTORE_DESIGN],
            Some("interactive /resume command with safe session restore"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_SUBAGENTS_DOC, EV_MULTIAGENT_DOC],
            Some("SubAgents and Agent Teams, including background subagents"),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_MEMORY_DOC, EV_WORKTREE_DOC],
            Some(
                "project-root discovery drives QWEN.md/AGENTS.md loading; git worktree \
                 workflows documented",
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
            Some(
                "beyond the terminal: IDE plugins, Desktop app, daemon mode, SDKs, \
                 and IM bots (Telegram/DingTalk/WeChat/Feishu)",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unsupported,
            &[EV_MEMORY_FORGET],
            Some(
                "no documented retraction of already-loaded instructions mid-session; \
                 auto-memory has a forget path, which is not instruction retraction",
            ),
        ),
    ]
}

impl TargetAdapter for QwenAdapter {
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
        // Qwen Code provably reads `QWEN.md` from the project root at session
        // start (docs/users/features/memory.md); that is the one surface we
        // project onto. Everything else in the census is native to the harness
        // and reached on its own terms.
        let mut contents = String::from(
            "# QWEN.md\n\nProjected by ai-kit. Qwen Code reads this file at the start of every session.\n",
        );
        for capsule in context.capsule_roots.keys() {
            contents.push_str(&format!("- capsule: {capsule}\n"));
        }
        let item = ProjectionItem::write("QWEN.md", contents)?;
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::next_session_only(
                "Qwen Code loads QWEN.md at session start; mid-session edits are not picked up",
            ),
        )
        .with_item(item)
        .with_note(
            "projected surface is the project-root QWEN.md only; skills, hooks, MCP, \
             and subagents are native faculties recorded in the admission census"
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

impl HarnessAdmissionAdapter for QwenAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Observed 2026-09-06 via `npm view @qwen-code/qwen-code version`
            // (latest published). The binary is not installed on this machine.
            native_version: Some("0.23.0".to_string()),
            source_revision: Some("92a8a8d17957".to_string()),
            realised_actuation_ref: None,
            project_binding_ref: Some(EV_NPM_PACKAGE.to_string()),
            faculties: qwen_faculties(),
        }
    }
}
