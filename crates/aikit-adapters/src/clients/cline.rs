//! Cline — admitted as a docs-level census adapter.
//!
//! Profiles the Cline CLI (binary `cline`) from the 2026-09-22 connection
//! truth research pass
//! (docs/plans/2026-09-22-harness-connection-truth-cards.md, expansion
//! shortlist #3). The ACP face is first-party: `cline --acp`, with the
//! documented optional `--auto-approve true` flag (auto-approval is the
//! harness's own permission bypass — a launch fact, never AIKit's default
//! door). Auth is `cline auth` or `CLINE_API_KEY`; provider and model are
//! selected through `CLINE_PROVIDER` / `CLINE_MODEL`.
//!
//! Honest scope note: Cline is NOT installed on this machine, so every fact
//! below is documentation-verified ([VD], docs.cline.bot) and nothing is
//! machine-observed. No batch headless JSON mode exists in the fact base —
//! the ACP face is the only declared connection door, and no MCP surface is
//! covered by the docs pass (nothing is claimed from silence).
//!
//! Projection is brokered: the verified face is the harness's own ACP command
//! surface; no on-disk instruction/skill/config tree that AIKit could project
//! into is documented, so this revision writes nothing.
//!
//! ## Identity law
//!
//! Cline is not the Agent identity; the docs are not the running product; no
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

pub const CLIENT: &str = "cline";
pub const PRODUCT: &str = "Cline";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:cline-adapter";

/// ACP face [VD docs.cline.bot]: first-party `cline --acp`; the documented
/// optional `--auto-approve true` flag bypasses the permission flow and is
/// deliberately NOT part of the declared door.
const EV_ACP: &str = "docs:docs.cline.bot first-party ACP face `cline --acp` \
(optional `--auto-approve true` documented; not part of the declared door)";

/// Auth and model selection [VD docs.cline.bot]: `cline auth` or
/// `CLINE_API_KEY`; provider/model via `CLINE_PROVIDER` / `CLINE_MODEL`.
const EV_AUTH: &str = "docs:docs.cline.bot auth `cline auth` or CLINE_API_KEY; \
provider/model selection via CLINE_PROVIDER / CLINE_MODEL";

/// Headless posture [VD docs.cline.bot]: no batch headless JSON mode is
/// documented — the ACP face is the only declared connection door.
const EV_HEADLESS: &str = "docs:docs.cline.bot no batch headless JSON mode documented";

/// The research pass record this census was written from.
const EV_CARDS: &str = "docs:docs/plans/2026-09-22-harness-connection-truth-cards.md \
expansion shortlist #3 (2026-09-22 research pass)";

pub struct ClineAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl ClineAdapter {
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

fn cline_faculties() -> Vec<HarnessFacultyObservation> {
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
                "Cline is not installed on this machine; reload behavior is unobservable \
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
                "docs-level census: binary `cline` with a first-party ACP face \
                 (`cline --acp`) and no batch headless JSON mode — the ACP face is the \
                 only declared connection door. Cline is NOT installed on this machine — \
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

impl TargetAdapter for ClineAdapter {
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
                "Cline's documented face is its own ACP command surface (`cline --acp`); no \
                 on-disk instruction/skill/config tree the CLI reads is documented, so AIKit \
                 provisions nothing and the plan stays brokered. The documented \
                 `--auto-approve true` flag is the harness's own permission bypass and is \
                 never part of AIKit's declared door",
            ),
        )
        .with_note(
            "Census is docs-level [VD docs.cline.bot]: Cline is not installed on this \
             machine. Auth is `cline auth` or CLINE_API_KEY with provider/model via \
             CLINE_PROVIDER/CLINE_MODEL; no batch headless JSON mode is documented. \
             Faculties the docs pass did not reach are Unknown, never negatives claimed \
             from silence"
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

impl HarnessAdmissionAdapter for ClineAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Docs-level admission: the binary is a plain CLI, but no version
            // is claimed because Cline is not installed here to probe.
            edition: HarnessEditionKind::Cli,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: cline_faculties(),
        }
    }
}
