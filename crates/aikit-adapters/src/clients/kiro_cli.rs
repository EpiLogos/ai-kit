//! Kiro CLI — admitted as a docs-level census adapter.
//!
//! Profiles AWS's Kiro CLI (binary `kiro-cli`) from the 2026-09-22 connection
//! truth research pass
//! (docs/plans/2026-09-22-harness-connection-truth-cards.md, expansion
//! shortlist #4). The ACP face is first-party: `kiro-cli acp [--agent <name>]`
//! — JSON-RPC 2.0 over stdio (kiro.dev/docs/cli/acp). Headless docs exist but
//! were nav-verified only in the research pass, so only the ACP door is
//! declared.
//!
//! ## The q/kiro binary-identity split (census hazard)
//!
//! Kiro CLI is the renamed Amazon Q CLI: older installs carry the binary `q`,
//! current installs carry `kiro-cli`. The two names describe the same product
//! line at different generations, and launch facts MUST NOT be derived from a
//! PATH name — the same identity hazard the kimi census records (a PATH
//! lookup can land on a different product than the docs describe). This
//! adapter pins the current-generation binary `kiro-cli` and declares no
//! fallback argv for `q`: an old-generation install joins nothing here until
//! its own census is taken.
//!
//! Honest scope note: Kiro CLI is NOT installed on this machine, so every
//! fact below is documentation-verified ([VD]) and nothing is
//! machine-observed. No MCP surface is covered by the docs pass; faculties
//! the docs pass did not reach are recorded as Unknown rather than claimed
//! from silence.
//!
//! Projection is brokered: the verified face is the harness's own ACP command
//! surface; no on-disk instruction/skill/config tree that AIKit could project
//! into is documented, so this revision writes nothing.

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

pub const CLIENT: &str = "kiro-cli";
pub const PRODUCT: &str = "Kiro CLI";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:kiro-cli-adapter";

/// ACP face [VD kiro.dev/docs/cli/acp]: first-party `kiro-cli acp
/// [--agent <name>]`, JSON-RPC 2.0 over stdio.
const EV_ACP: &str = "docs:kiro.dev/docs/cli/acp first-party ACP face \
`kiro-cli acp [--agent <name>]` (JSON-RPC 2.0 over stdio)";

/// Identity [VD kiro.dev/docs/cli]: Kiro CLI is the renamed Amazon Q CLI —
/// the older generation installs as `q`, the current generation as
/// `kiro-cli`. The split is the census hazard, so no `q` fallback argv is
/// declared.
const EV_IDENTITY: &str = "docs:kiro.dev/docs/cli kiro-cli is the renamed Amazon Q CLI \
(older generation binary `q`); launch facts pinned to `kiro-cli`, no `q` fallback declared";

/// Headless posture [VD kiro.dev/docs/cli]: headless docs exist but were
/// nav-verified only in the research pass — details unverified, nothing
/// declared beyond that.
const EV_HEADLESS: &str = "docs:kiro.dev/docs/cli headless docs exist \
(nav-verified only in the 2026-09-22 pass; details unverified)";

/// The research pass record this census was written from.
const EV_CARDS: &str = "docs:docs/plans/2026-09-22-harness-connection-truth-cards.md \
expansion shortlist #4 (2026-09-22 research pass; q/kiro binary-identity split named)";

pub struct KiroCliAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl KiroCliAdapter {
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

fn kiro_cli_faculties() -> Vec<HarnessFacultyObservation> {
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
                "Kiro CLI is not installed on this machine; reload behavior is \
                 unobservable and undocumented in the fact base",
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
            FacultySupport::Degraded,
            &[EV_ACP],
            Some(
                "the ACP face accepts `--agent <name>` per docs — an agent-selection flag; \
                 whether it names delegable subagents or profiles is undocumented, so the \
                 faculty is degraded rather than supported",
            ),
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
            &[EV_ACP, EV_IDENTITY, EV_HEADLESS, EV_CARDS],
            Some(
                "docs-level census: binary `kiro-cli` (renamed Amazon Q CLI; older \
                 generation installs as `q` — no `q` fallback declared) with a first-party \
                 ACP face (`kiro-cli acp`) and nav-verified-only headless docs. Kiro CLI \
                 is NOT installed on this machine — nothing here is machine-observed",
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

impl TargetAdapter for KiroCliAdapter {
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
                "Kiro CLI's documented face is its own ACP command surface (`kiro-cli acp`); \
                 no on-disk instruction/skill/config tree the CLI reads is documented, so \
                 AIKit provisions nothing and the plan stays brokered",
            ),
        )
        .with_note(
            "Census is docs-level [VD kiro.dev/docs/cli]: Kiro CLI is not installed on this \
             machine. Identity is pinned to the current-generation `kiro-cli` binary — the \
             renamed Amazon Q CLI (`q`) joins nothing here until its own census is taken. \
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

impl HarnessAdmissionAdapter for KiroCliAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Docs-level admission: the binary is a plain CLI, but no version
            // is claimed because Kiro CLI is not installed here to probe.
            edition: HarnessEditionKind::Cli,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: kiro_cli_faculties(),
        }
    }
}
