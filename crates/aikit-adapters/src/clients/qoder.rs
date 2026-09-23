//! Qoder CLI — admitted as a docs-level census adapter.
//!
//! Profiles the Qoder CLI (binary `qoder`) from the 2026-09-22 connection
//! truth research pass
//! (docs/plans/2026-09-22-harness-connection-truth-cards.md, expansion
//! shortlist #9). The ACP face is first-party: `qoder --acp`
//! (docs.qoder.com/cli/acp). Auth is `qoder login` or the
//! `QODER_PERSONAL_ACCESS_TOKEN` environment variable.
//!
//! Honest scope note: Qoder is NOT installed on this machine, so every fact
//! below is documentation-verified ([VD]) and nothing is machine-observed.
//! Headless is undocumented in the fact base — the ACP face is the only
//! declared connection door — and no MCP surface is covered by the docs pass
//! (nothing is claimed from silence).
//!
//! Projection is brokered: the verified face is the harness's own ACP command
//! surface; no on-disk instruction/skill/config tree that AIKit could project
//! into is documented, so this revision writes nothing.
//!
//! ## Identity law
//!
//! Qoder is not the Agent identity; the docs are not the running product; no
//! `realised_actuation_ref` is fabricated (the Actuation catalog declares no
//! descriptor for this slug — the roster row appears when that descriptor
//! lands, never before).

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

pub const CLIENT: &str = "qoder";
pub const PRODUCT: &str = "Qoder CLI";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:qoder-adapter";

/// ACP face [VD docs.qoder.com/cli/acp]: first-party `qoder --acp`.
const EV_ACP: &str = "docs:docs.qoder.com/cli/acp first-party ACP face `qoder --acp`";

/// Auth face [VD docs.qoder.com]: `qoder login` or the
/// `QODER_PERSONAL_ACCESS_TOKEN` environment variable.
const EV_AUTH: &str = "docs:docs.qoder.com auth `qoder login` or \
QODER_PERSONAL_ACCESS_TOKEN";

/// Headless posture: headless is undocumented in the fact base — the ACP face
/// is the only declared connection door.
const EV_HEADLESS: &str = "docs:docs.qoder.com headless undocumented (nothing declared)";

/// The research pass record this census was written from.
const EV_CARDS: &str = "docs:docs/plans/2026-09-22-harness-connection-truth-cards.md \
expansion shortlist #9 (2026-09-22 research pass)";

pub struct QoderAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl QoderAdapter {
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

fn qoder_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Unknown,
            &[EV_ACP, EV_CARDS],
            Some(
                "no instruction-file surface is covered by the docs pass, so nothing is \
                 declared",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("no project-instruction surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Unknown,
            &[EV_ACP, EV_CARDS],
            Some("no skills surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("no hook surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some(
                "Qoder is not installed on this machine; reload behavior is unobservable \
                 and undocumented in the fact base",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("pickup timing across sessions is undocumented in the fact base"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some(
                "the ACP face is a client-spawned process per the protocol spec, but config \
                 pickup across restarts is unverified",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Unknown,
            &[EV_ACP, EV_CARDS],
            Some("no MCP surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("no tool-contribution surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Unknown,
            &[EV_ACP, EV_CARDS],
            Some("no session-resume face in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("no delegation surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("project-root handling is not covered by the docs pass"),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("no component surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_ACP, EV_AUTH, EV_HEADLESS, EV_CARDS],
            Some(
                "docs-level census: binary `qoder` with a first-party ACP face \
                 (`qoder --acp`) and no documented headless face — the ACP face is the \
                 only declared connection door. Qoder is NOT installed on this machine — \
                 nothing here is machine-observed",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some("retraction behavior is not covered by the docs pass"),
        ),
    ]
}

impl TargetAdapter for QoderAdapter {
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
                "Qoder CLI's documented face is its own ACP command surface (`qoder --acp`); \
                 no on-disk instruction/skill/config tree the CLI reads is documented, so \
                 AIKit provisions nothing and the plan stays brokered",
            ),
        )
        .with_note(
            "Census is docs-level [VD docs.qoder.com/cli/acp]: Qoder is not installed on \
             this machine. Auth is `qoder login` or QODER_PERSONAL_ACCESS_TOKEN; headless \
             is undocumented. Faculties the docs pass did not reach are Unknown, never \
             negatives claimed from silence"
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

impl HarnessAdmissionAdapter for QoderAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Docs-level admission: the binary is a plain CLI, but no version
            // is claimed because Qoder is not installed here to probe.
            edition: HarnessEditionKind::Cli,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: qoder_faculties(),
        }
    }
}
