//! DeepSeek Harness (DSH) — a composable harness admitted through the
//! harness-adapter contract rather than modeled as a fixed-tree client.
//!
//! DSH's native faculties are composition-managed (Cordis Host/Client plugins,
//! agent presets, skills, model tools, subagents, session persistence, and
//! Slots/theme UI). There is no single fixed `.agents/skills` tree to write the
//! way Codex or Claude Code read one; the projection surface is the session's
//! composition and preset. This adapter therefore records the evidence-backed
//! faculty census up front and, for this revision, brokers projection rather
//! than inventing a tree DSH does not read.
//!
//! ## Identity law
//!
//! A model running in DSH is not the Agent identity; DSH is not the World; a
//! DSH process is not an AgentSession. The adapter keeps `target`, `product`,
//! and any `realised_actuation_ref` distinct, and never fabricates a
//! loaded-activation claim (`verify_activation_truth` rejects it).

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

pub const CLIENT: &str = "deepseek-harness";
pub const PRODUCT: &str = "DeepSeek Harness";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:dsh-adapter";

/// Stable evidence refs so conformance can cite exact sources rather than prose.
const EV_PRESET: &str = "dsh:faculty:agent-preset";
const EV_SKILLS: &str = "dsh:faculty:skill-catalog";
const EV_LIVE_RELOAD: &str = "dsh:faculty:client-hmr";
const EV_NEXT_SESSION: &str = "dsh:faculty:composition-lifecycle:next-session";
const EV_RESTART: &str = "dsh:faculty:composition-lifecycle:restart";
const EV_TOOL_PROTOCOL: &str = "dsh:faculty:model-tools";
const EV_NATIVE_TOOL: &str = "dsh:faculty:dynamic-plugins";
const EV_SESSION_RESUME: &str = "dsh:faculty:session-persistence-jsonl";
const EV_DELEGATED: &str = "dsh:faculty:subagents";
const EV_PROJECT_ROOTS: &str = "dsh:faculty:session-workspace";
const EV_COMPONENTS: &str = "dsh:faculty:slots:components";
const EV_SURFACES: &str = "dsh:faculty:slots:surfaces";
const EV_LIVE_RETRACTION: &str = "dsh:faculty:plugin-lifecycle:retraction";

pub struct DshAdapter {
    /// Where a future native composition/preset projection would be written.
    /// Unused by the brokered plan in this revision; kept as the explicit
    /// projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl DshAdapter {
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

fn dsh_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_PRESET],
            None,
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_PRESET],
            None,
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Supported,
            &[EV_SKILLS],
            None,
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Degraded,
            &[],
            Some("no separate session-start hook faculty; composition apply() is the mount point"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Supported,
            &[EV_LIVE_RELOAD],
            None,
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_NEXT_SESSION],
            None,
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Supported,
            &[EV_RESTART],
            None,
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_TOOL_PROTOCOL],
            None,
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Supported,
            &[EV_NATIVE_TOOL],
            None,
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_SESSION_RESUME],
            None,
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_DELEGATED],
            None,
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_PROJECT_ROOTS],
            None,
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Supported,
            &[EV_COMPONENTS],
            None,
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_SURFACES],
            None,
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Supported,
            &[EV_LIVE_RETRACTION],
            None,
        ),
    ]
}

impl TargetAdapter for DshAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: true,
            symlinks: true,
            isolated_per_context: true,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: true,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(
                "DSH composition/preset projection is brokered in this adapter revision; \
                 capabilities are reached through AIKit's broker rather than a fixed tree",
            ),
        )
        .with_note(
            "DSH exposes its skills, tools, plugins and session surface through its own \
             composition and preset mechanism, not a fixed project tree; native preset \
             projection is a subsequent slice"
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

impl HarnessAdmissionAdapter for DshAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Custom,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: dsh_faculties(),
        }
    }
}
