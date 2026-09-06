//! Google Antigravity — admitted through the harness-adapter contract.
//!
//! Google Antigravity is Google's agentic development platform (native owner
//! Google; catalog edition `ide`). Discovery rode Actuation harness detection
//! (`actuation harness detect --json --versions`, run 2026-09-07): the
//! record below is the receipt. Unlike the process-per-invocation CLIs, the
//! detection for slug `gemini-antigravity` is config-dir-only — the receipt's
//! `executable` IS `~/.gemini/antigravity` (`executable_is: config-dir`), no
//! sha256 is recorded, and no version probe exists.
//!
//! Direct observation of the receipt path shows an IDE-managed lifecycle:
//! `~/.gemini/antigravity` holds protobuf state (`antigravity_state.pbtxt`),
//! 20 conversation stores (`conversations/*.db`), per-conversation `brain/`
//! transcript dirs, browser recordings, and html artifacts; the sibling
//! `~/.gemini/antigravity-ide/` holds the IDE's builtin `SKILL.md` skills
//! (`agy-customizations`, `antigravity_guide`, `permissioned-github`) and an
//! `agentapi` shim pointing at `/Applications/Antigravity IDE.app/...` — which
//! is currently ABSENT (the shim dangles; last IDE logs are 2026-06-24). So
//! the state tree is detected, but the IDE executable itself is not installed
//! on this machine right now.
//!
//! Consequence for honesty: faculties backed by on-disk artifacts (skills,
//! session stores, MCP config) are Supported; faculties that only the docs
//! describe (rules, hooks, subagent behavior) are marked Unknown or Degraded
//! with the absent-IDE reason — Supported must not rest on docs alone, and no
//! detection record covers those surfaces. No capability descriptor is
//! declared for slug `gemini-antigravity` in Actuation (`actuation harness
//! capability gemini-antigravity` says so explicitly).
//!
//! The projection plan is brokered: Antigravity's rules/skills/agent lifecycle
//! is owned by the IDE (and its 2.0 command center), not by a fixed tree a
//! projection can write into the project — and with the IDE executable
//! absent there is no client to activate at all.
//!
//! ## Identity law
//!
//! A model running in Antigravity is not the Agent identity; Antigravity is
//! not the World; the IDE is not an AgentSession. The adapter keeps `target`,
//! `product`, and any `realised_actuation_ref` distinct, and never fabricates
//! a loaded-activation claim (`verify_activation_truth` rejects it).
//!
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

pub const CLIENT: &str = "gemini-antigravity";
pub const PRODUCT: &str = "Google Antigravity";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:antigravity-adapter";

/// Detection disclosure: the Actuation detection record this admission cites,
/// produced by `actuation harness detect --json --versions` run on this
/// machine. Config-dir probe only; no executable probe or version for this
/// slug, so no sha256 exists on the receipt — recorded honestly rather than
/// invented.
const EV_DETECTION: &str = "actuation.harness-detection/v1 \
detection:2026-09-06T23:42:48.747Z gemini-antigravity:detected \
exe:/Users/admin/.gemini/antigravity (config-dir receipt; no sha256 recorded) \
observed:2026-09-06T23:42:48.747Z";

/// Stable evidence refs so conformance can cite exact sources rather than prose.
const EV_SKILLS_DOC: &str = "https://antigravity.google/docs/skills";
const EV_RULES_DOC: &str = "https://antigravity.google/docs/rules-workflows";
const EV_HOOKS_DOC: &str = "https://antigravity.google/docs/hooks";
const EV_MCP_DOC: &str = "https://antigravity.google/docs/mcp";
const EV_SUBAGENTS_DOC: &str = "https://antigravity.google/docs/subagents";
const EV_ARTIFACTS_DOC: &str = "https://antigravity.google/docs/artifact-review";
const EV_PRODUCT_IDE: &str = "https://antigravity.google/product/antigravity-ide";
/// Observed 2026-09-07: three builtin SKILL.md skill directories on disk.
const EV_BUILTIN_SKILLS: &str =
    "local:/Users/admin/.gemini/antigravity-ide/builtin/skills (agy-customizations, \
antigravity_guide, permissioned-github)";
/// Observed 2026-09-07: 20 conversation stores under the detection receipt path.
const EV_CONVERSATIONS: &str =
    "local:/Users/admin/.gemini/antigravity/conversations (20 *.db conversation stores)";
/// Observed 2026-09-07: MCP wiring in the gemini config tree the harness shares.
const EV_MCP_STATE: &str =
    "local:~/.gemini/antigravity/mcp_config.json (symlink) + ~/.gemini/settings.json \
mcpServers + ~/.gemini/policies (tool permission rules)";
/// Observed 2026-09-07: the IDE app bundle is gone, so no native version is
/// observable and no runtime faculty can be exercised.
const EV_IDE_ABSENT: &str =
    "local:/Applications/Antigravity IDE.app absent (agentapi shim dangling); last IDE logs \
2026-06-24";

pub struct AntigravityAdapter {
    /// Where a future IDE-managed projection would be anchored (the project
    /// root). Unused by the brokered plan in this revision; kept as the
    /// explicit projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl AntigravityAdapter {
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

fn antigravity_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Unknown,
            &[EV_RULES_DOC, EV_IDE_ABSENT],
            Some(
                "global Rules are documented for the platform, but the IDE executable is \
                 absent, so nothing can be observed loading them; ~/.gemini/GEMINI.md in the \
                 shared gemini tree is Gemini CLI's memory surface, not claimed here",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Unknown,
            &[EV_RULES_DOC, EV_IDE_ABSENT],
            Some(
                "workspace Rules are documented; with the IDE uninstalled no workspace rule \
                 file can be verified on disk",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_BUILTIN_SKILLS, EV_SKILLS_DOC],
            Some(
                "SKILL.md-format skills are a documented platform surface and three builtin \
                 skills are present on disk in the antigravity-ide state tree",
            ),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unknown,
            &[EV_HOOKS_DOC, EV_IDE_ABSENT],
            Some(
                "hooks are documented (PreToolUse/PostToolUse/PreInvocation/PostInvocation/ \
                 Stop, ...); no session-start event or live IDE exists to verify one",
            ),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unknown,
            &[EV_IDE_ABSENT],
            Some("no live-reload evidence in the state tree; the IDE that would reload is absent"),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Degraded,
            &[EV_SKILLS_DOC, EV_IDE_ABSENT],
            Some(
                "docs say new agent invocations pick up skills/rules; unverifiable while the \
                 IDE is uninstalled, so documented-but-unconfirmed",
            ),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unknown,
            &[EV_IDE_ABSENT],
            Some("no restart-reload channel verifiable without the IDE executable"),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_MCP_DOC, EV_MCP_STATE],
            Some(
                "MCP is a documented platform surface and MCP wiring exists on disk (mcp \
                 config symlink, mcpServers in settings.json, tool permission policies)",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_MCP_DOC, EV_MCP_STATE],
            Some(
                "MCP servers (e.g. the configured `pencil` server) and plugins contribute \
                 tools; MCP wiring observed on disk, plugins documented",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_CONVERSATIONS],
            Some(
                "20 conversation stores persist under ~/.gemini/antigravity/conversations; \
                 resuming them needs the IDE, but the storage faculty itself is observed",
            ),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_SUBAGENTS_DOC],
            Some(
                "subagents and agent teams are a documented platform surface; no detection \
                 record covers this surface, so docs are the cited evidence",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Unknown,
            &[EV_DETECTION],
            Some(
                "projects/workspaces are referenced in state (projects.json in the shared \
                 gemini tree), but the receipt path exposes no workspace-root surface to \
                 verify",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unknown,
            &[],
            Some("no component/slots-style composition surface observed in state or docs"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_ARTIFACTS_DOC, EV_DETECTION],
            Some(
                "artifact review and walkthroughs are documented platform surfaces; html \
                 artifacts and browser recordings directories exist under the receipt path",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unknown,
            &[EV_IDE_ABSENT],
            Some("no retraction semantics observable without the running IDE"),
        ),
    ]
}

impl TargetAdapter for AntigravityAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: false,
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
                "Antigravity's rules/skills/agent lifecycle is owned by the IDE and its 2.0 \
                 command center, not by a fixed project tree a projection can write; with the \
                 IDE app absent there is no client to activate",
            ),
        )
        .with_note(
            "detection is config-dir-only (receipt executable is ~/.gemini/antigravity \
             itself) and /Applications/Antigravity IDE.app is not installed, so projection \
             is brokered: capabilities are reached through AIKit's broker or the IDE's own \
             settings once reinstalled, not through authored files in the working tree"
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

impl HarnessAdmissionAdapter for AntigravityAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Ide,
            // Not observable: the IDE app bundle is absent and the detection
            // record carries no version probe for this slug. The docs site
            // shows an "Antigravity for IDEs v2.5.5" nav label, but that is
            // the docs revision, not the (uninstalled) local IDE.
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: Some(EV_PRODUCT_IDE.to_string()),
            faculties: antigravity_faculties(),
        }
    }
}
