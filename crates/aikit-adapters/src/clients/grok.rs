//! Grok Build — xAI's coding CLI (`grok`), admitted as a docs-level census.
//!
//! Owner decision 2026-09-22: the `grok-bot` adapter was a misidentification —
//! the grok-bot CLI (grok-bot-cli 0.2.2) manages bots/groups through a gateway
//! service, not a coding harness. This module now profiles xAI's actual coding
//! CLI, **Grok Build** (binary `grok`), from the 2026-09-22 connection truth
//! research pass (docs/plans/2026-09-22-harness-connection-truth-cards.md,
//! "ROSTER CORRECTION" card).
//!
//! Honest scope note: Grok Build is NOT installed on this machine, so every
//! fact below is documentation-verified ([VD], docs.x.ai/build/overview) and
//! nothing is machine-observed. The census records the docs-declared faces and
//! refuses to invent what the docs pass did not cover: the ACP argv is
//! unverified (ACP is supported per docs, no connect face declared here),
//! resume flags are undocumented upstream (nothing declared), and the MCP
//! config path is undocumented (docs say MCP "works out of the box"; no seam
//! is named). Faculties the docs pass did not reach are recorded as Unknown
//! rather than claimed from silence.
//!
//! Projection is brokered: the verified faces are the harness's own command
//! surface and its config file — no on-disk instruction/skill tree that AIKit
//! could project into is documented, so this revision writes nothing.
//!
//! ## Identity law
//!
//! Grok Build is not the Agent identity; the docs are not the running
//! product; no `realised_actuation_ref` is fabricated. The Actuation catalog
//! still carries a grok-bot descriptor describing the wrong product — a
//! catalog correction is owed to Actuation (returned via NOW, not fixable in
//! this repo), until which no capability descriptor exists for `grok` here.

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

pub const CLIENT: &str = "grok";
pub const PRODUCT: &str = "Grok Build";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:grok-adapter";

/// Product, binary and auth face [VD docs.x.ai/build/overview].
const EV_DOCS: &str = "docs:docs.x.ai/build/overview Grok Build CLI, binary `grok`; \
first launch opens browser auth, headless falls back to XAI_API_KEY";

/// Headless face [VD docs.x.ai/build/overview]; the output-format value is
/// literally `streaming-json`.
const EV_HEADLESS: &str = "docs:docs.x.ai/build/overview headless face \
`grok -p \"...\" --output-format streaming-json`";

/// Config surface [VD docs.x.ai/build/overview]; custom providers are
/// declared as `[model.*]` tables carrying `env_key`.
const EV_CONFIG: &str = "docs:docs.x.ai/build/overview config `~/.grok/config.toml`; \
custom providers declare `[model.*]` tables carrying `env_key`";

/// MCP posture [VD docs.x.ai/build/overview]: docs say MCP "works out of the
/// box"; no config path is documented and none was verified.
const EV_MCP: &str = "docs:docs.x.ai/build/overview MCP \"works out of the box\" \
(docs-declared; no config path documented, none verified)";

/// ACP posture [VD docs.x.ai/build/overview]: docs-declared supported; the
/// exact connect argv is NOT verified, so no acp connect is declared.
const EV_ACP: &str = "docs:docs.x.ai/build/overview ACP supported per docs \
(connect argv unverified — not declared here)";

/// The research pass record this census was written from.
const EV_CARDS: &str = "docs:docs/plans/2026-09-22-harness-connection-truth-cards.md \
grokbot roster-correction card (2026-09-22 research pass)";

/// Sessions truth [VD docs.x.ai/build/overview]: resume flags are not
/// documented upstream, so no resume face is declared.
const EV_SESSIONS: &str = "docs:docs.x.ai/build/overview resume flags undocumented \
upstream (nothing declared)";

pub struct GrokAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl GrokAdapter {
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

fn grok_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Unknown,
            &[EV_CONFIG, EV_DOCS],
            Some(
                "the docs pass verified the config file and its model tables only; whether \
                 Grok Build reads a global or per-project instruction file was not covered, \
                 so nothing is declared",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("no project-instruction surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Unknown,
            &[EV_DOCS, EV_CARDS],
            Some("no skills surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("no hook surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some(
                "Grok Build is not installed on this machine; reload behavior is \
                 unobservable and undocumented in the fact base",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("pickup timing across sessions is undocumented in the fact base"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some(
                "the headless face is process-per-invocation per docs, but restart pickup \
                 of config or projected material is unverified",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Degraded,
            &[EV_MCP, EV_CONFIG],
            Some(
                "docs declare MCP working out of the box; the config path is undocumented \
                 and none was verified, so no seam is named and the schema is unverified",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unknown,
            &[EV_MCP],
            Some("the contribution surface is not documented beyond the out-of-the-box claim"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Unknown,
            &[EV_SESSIONS],
            Some("resume flags are undocumented upstream; nothing is declared"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("no delegation surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("project-root handling is not covered by the docs pass"),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("no component surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_DOCS, EV_HEADLESS, EV_CONFIG, EV_ACP, EV_CARDS],
            Some(
                "docs-level census: CLI (`grok`, no other executable documented) with a \
                 headless streaming-json face and a config.toml provider surface; ACP is \
                 docs-declared with unverified argv. Grok Build is NOT installed on this \
                 machine — nothing here is machine-observed",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unknown,
            &[EV_DOCS],
            Some("retraction behavior is not covered by the docs pass"),
        ),
    ]
}

impl TargetAdapter for GrokAdapter {
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
                "Grok Build's verified faces are its own command surface (headless \
                 `grok -p --output-format streaming-json`) and its config file \
                 (~/.grok/config.toml); no on-disk instruction/skill tree the CLI reads is \
                 documented, so AIKit provisions nothing and the plan stays brokered",
            ),
        )
        .with_note(
            "Census is docs-level [VD docs.x.ai/build/overview]: Grok Build is not installed \
             on this machine. MCP is docs-declared out of the box with no documented config \
             path; ACP is docs-declared with unverified argv; resume flags are undocumented. \
             Faculties the docs pass did not reach are Unknown, never negatives claimed from \
             silence"
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

impl HarnessAdmissionAdapter for GrokAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Docs-level admission: the binary is a plain CLI, but no version
            // is claimed because Grok Build is not installed here to probe.
            edition: HarnessEditionKind::Cli,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: grok_faculties(),
        }
    }
}
