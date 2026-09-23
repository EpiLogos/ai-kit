//! Factory Droid — admitted as a docs-level census adapter.
//!
//! Profiles Factory's Droid CLI (binary `droid`) from the 2026-09-22
//! connection truth research pass
//! (docs/plans/2026-09-22-harness-connection-truth-cards.md, expansion
//! shortlist #2).
//!
//! ## Connection posture: the headless runner is the first-party face
//!
//! Droid's first-party connection family is the headless runner
//! `droid exec --output-format text|json|stream-jsonrpc` (with
//! `--input-format stream-jsonrpc` for streaming input) — a
//! prompt-in/prompt-out process face, **not** a spawn-and-speak ACP server.
//! The ACP registry carries an entry `droid exec --output-format acp-daemon`
//! [VR cdn.agentclientprotocol.com/registry/v1/latest], but that argv implies
//! a daemon whose lifetime outlives the exec invocation — unverified, so no
//! ACP door is declared here. The stream-jsonrpc headless family is the named
//! future connection lane: a first-party structured wire that a aikit
//! process-connection can target once its framing is pinned against an
//! installed binary.
//!
//! Honest scope note: Droid is NOT installed on this machine, so every fact
//! below is documentation-verified ([VD], docs.factory.com) and nothing is
//! machine-observed. The MCP client config paths are documented
//! (`~/.factory/mcp.json` global, `.factory/mcp.json` project, key
//! `mcpServers`, `droid mcp add`) and are recorded as tools-layer
//! observations — but observation is disclosure, not projection: AIKit still
//! writes nothing, because no capability descriptor declares a managed seam.
//!
//! Projection is brokered: the verified faces are the harness's own command
//! surface and its config files — this revision writes nothing.
//!
//! ## Identity law
//!
//! Factory Droid is not the Agent identity; the docs are not the running
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

pub const CLIENT: &str = "droid";
pub const PRODUCT: &str = "Factory Droid";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:droid-adapter";

/// Headless runner [VD docs.factory.com]: the first-party connection family —
/// `droid exec --output-format text|json|stream-jsonrpc`, with
/// `--input-format stream-jsonrpc` for streaming input.
const EV_HEADLESS: &str = "docs:docs.factory.com headless runner \
`droid exec --output-format text|json|stream-jsonrpc` \
(+ `--input-format stream-jsonrpc`)";

/// ACP posture [VR cdn.agentclientprotocol.com/registry/v1/latest]: the
/// registry lists `droid exec --output-format acp-daemon` — a daemon whose
/// lifetime outlives exec, unverified, so no ACP door is declared.
const EV_ACP: &str = "docs:cdn.agentclientprotocol.com/registry/v1/latest ACP registry \
entry `droid exec --output-format acp-daemon` (daemon lifetime outlives exec; \
unverified — no ACP door declared)";

/// MCP client config [VD docs.factory.com]: `~/.factory/mcp.json` (global)
/// and `.factory/mcp.json` (project), key `mcpServers`; `droid mcp add`.
const EV_MCP: &str = "docs:docs.factory.com MCP client config ~/.factory/mcp.json \
(global) and .factory/mcp.json (project), key mcpServers, `droid mcp add`";

/// The research pass record this census was written from.
const EV_CARDS: &str = "docs:docs/plans/2026-09-22-harness-connection-truth-cards.md \
expansion shortlist #2 (2026-09-22 research pass; stream-jsonrpc headless family named \
as a future connection lane)";

pub struct DroidAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl DroidAdapter {
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

fn droid_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Unknown,
            &[EV_HEADLESS, EV_CARDS],
            Some(
                "no instruction-file surface is covered by the docs pass, so nothing is \
                 declared",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some("no project-instruction surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Unknown,
            &[EV_HEADLESS, EV_CARDS],
            Some("no skills surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some("no hook surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some(
                "Droid is not installed on this machine; reload behavior is unobservable \
                 and undocumented in the fact base",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Unknown,
            &[EV_MCP],
            Some(
                "config pickup across invocations is implied by a file-based MCP config but \
                 never verified",
            ),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some("restart pickup is not covered by the docs pass"),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Supported,
            &[EV_MCP],
            Some(
                "docs declare the MCP client config paths and key exactly \
                 (~/.factory/mcp.json global, .factory/mcp.json project, key mcpServers, \
                 `droid mcp add`); Droid is not installed here, so the schema was never \
                 verified against a live harness and the layer stays observed-only",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unknown,
            &[EV_MCP],
            Some(
                "the contribution surface beyond the documented MCP client config is not \
                 covered by the docs pass",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Unknown,
            &[EV_HEADLESS, EV_CARDS],
            Some("no resume face in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some("no delegation surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Degraded,
            &[EV_MCP],
            Some(
                "a project-scope `.factory/mcp.json` is documented, which implies project \
                 scoping; the general project-root handling is otherwise not covered by \
                 the docs pass",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some("no component surface in the fact base; not claimed from silence"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_HEADLESS, EV_ACP, EV_MCP, EV_CARDS],
            Some(
                "docs-level census: binary `droid` with a first-party headless runner \
                 (`droid exec --output-format text|json|stream-jsonrpc`) and documented \
                 MCP config paths. NO spawn-and-speak ACP face is declared: the registry's \
                 `acp-daemon` argv implies a daemon whose lifetime outlives exec — \
                 unverified. The stream-jsonrpc headless family is the named future \
                 connection lane. Droid is NOT installed on this machine — nothing here \
                 is machine-observed",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unknown,
            &[EV_HEADLESS],
            Some("retraction behavior is not covered by the docs pass"),
        ),
    ]
}

impl TargetAdapter for DroidAdapter {
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
                "Factory Droid's verified faces are its own headless command surface \
                 (`droid exec --output-format text|json|stream-jsonrpc`) and its documented \
                 MCP config files (~/.factory/mcp.json, .factory/mcp.json); the MCP layer \
                 is observed-only and no capability descriptor declares a managed seam, so \
                 AIKit provisions nothing and the plan stays brokered",
            ),
        )
        .with_note(
            "Census is docs-level [VD docs.factory.com]: Droid is not installed on this \
             machine. No ACP door is declared — the registry's acp-daemon argv implies a \
             daemon whose lifetime outlives exec, unverified. The stream-jsonrpc headless \
             family is the named future connection lane. Faculties the docs pass did not \
             reach are Unknown, never negatives claimed from silence"
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

impl HarnessAdmissionAdapter for DroidAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Docs-level admission: the binary is a plain CLI, but no version
            // is claimed because Droid is not installed here to probe.
            edition: HarnessEditionKind::Cli,
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: droid_faculties(),
        }
    }
}
