//! GitHub Copilot CLI — admitted as a docs-level census adapter.
//!
//! Profiles GitHub's Copilot CLI (binary `copilot`) from the 2026-09-22
//! connection truth research pass
//! (docs/plans/2026-09-22-harness-connection-truth-cards.md, expansion
//! shortlist #1). The product is in **public preview** (announced 2026-01-28,
//! github.blog changelog), and the ACP face is first-party:
//! `copilot --acp` (docs.github.com/en/copilot/reference/acp-server).
//!
//! Honest scope note: the Copilot CLI is NOT installed on this machine, so
//! every fact below is documentation-verified ([VD]) and nothing is
//! machine-observed. MCP is documented as delivered **per session** through
//! the ACP `session/new` `mcpServers` wire field — no file-based MCP config
//! is documented — so there is no config seam for AIKit to observe or project
//! into. The model/provider surface (BYOK, auth split) is undocumented at
//! preview; the profile declares `dispatch = none` with that reason rather
//! than inventing a roster. Faculties the docs pass did not reach are
//! recorded as Unknown rather than claimed from silence.
//!
//! Projection is brokered: the verified face is the harness's own ACP command
//! surface; no on-disk instruction/skill/config tree that AIKit could project
//! into is documented, so this revision writes nothing.
//!
//! ## Identity law
//!
//! GitHub Copilot CLI is not the Agent identity; the docs are not the running
//! product; no `realised_actuation_ref` is fabricated (the Actuation catalog
//! declares no descriptor for this slug — the roster row appears when that
//! descriptor lands, never before).

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

pub const CLIENT: &str = "copilot";
pub const PRODUCT: &str = "GitHub Copilot CLI";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:copilot-adapter";

/// Product and ACP face [VD docs.github.com/en/copilot/reference/acp-server]:
/// first-party ACP server mode via `copilot --acp`.
const EV_ACP: &str = "docs:docs.github.com/en/copilot/reference/acp-server first-party \
ACP face `copilot --acp` (JSON-RPC 2.0 over stdio)";

/// Preview status [VD github.blog/changelog]: Copilot CLI public preview
/// announced 2026-01-28.
const EV_PREVIEW: &str = "docs:github.blog changelog Copilot CLI public preview \
(announced 2026-01-28)";

/// MCP posture [VD docs.github.com/en/copilot/reference/acp-server]: MCP
/// servers are delivered per session through the ACP `session/new`
/// `mcpServers` wire field; no file-based MCP config is documented.
const EV_MCP: &str = "docs:docs.github.com/en/copilot/reference/acp-server MCP via \
session/new mcpServers (per-session wire delivery; no file-based config documented)";

/// The research pass record this census was written from.
const EV_CARDS: &str = "docs:docs/plans/2026-09-22-harness-connection-truth-cards.md \
expansion shortlist #1 (2026-09-22 research pass)";

pub struct CopilotAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl CopilotAdapter {
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

fn copilot_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Unknown,
            &[EV_ACP, EV_CARDS],
            Some(
                "no instruction-file surface is covered by the docs pass; the preview docs \
                 describe the ACP reference only, so nothing is declared",
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
            &[EV_PREVIEW],
            Some(
                "the CLI is not installed on this machine; reload behavior is unobservable \
                 and undocumented in the fact base",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Unknown,
            &[EV_PREVIEW],
            Some("pickup timing across sessions is undocumented in the fact base"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unknown,
            &[EV_ACP],
            Some(
                "the ACP face is a client-spawned process per the protocol spec, but config \
                 or projected-material pickup across restarts is unverified",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Degraded,
            &[EV_MCP],
            Some(
                "MCP is documented as per-session delivery through the ACP session/new \
                 mcpServers wire field; no file-based MCP config is documented, so no \
                 config seam is named and nothing was verified against a live harness",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unknown,
            &[EV_MCP],
            Some(
                "the contribution surface beyond session/new mcpServers delivery is not \
                 covered by the docs pass",
            ),
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
            &[EV_ACP, EV_PREVIEW, EV_CARDS],
            Some(
                "docs-level census: binary `copilot` with a first-party ACP face \
                 (`copilot --acp`), public preview 2026-01-28. No headless face and no \
                 file-based config are documented. The Copilot CLI is NOT installed on \
                 this machine — nothing here is machine-observed",
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

impl TargetAdapter for CopilotAdapter {
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
                "GitHub Copilot CLI's documented face is its own ACP command surface \
                 (`copilot --acp`); MCP arrives per session through the session/new wire \
                 field and no on-disk instruction/skill/config tree the CLI reads is \
                 documented, so AIKit provisions nothing and the plan stays brokered",
            ),
        )
        .with_note(
            "Census is docs-level [VD docs.github.com/en/copilot/reference/acp-server]: \
             public preview 2026-01-28; the CLI is not installed on this machine. Model \
             dispatch is undeclared (preview; BYOK/auth split undocumented). Faculties \
             the docs pass did not reach are Unknown, never negatives claimed from silence"
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

impl HarnessAdmissionAdapter for CopilotAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Docs-level admission: the binary is a plain CLI, but no version
            // is claimed because the Copilot CLI is not installed here to probe.
            edition: HarnessEditionKind::Cli,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: copilot_faculties(),
        }
    }
}
