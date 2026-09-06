//! Aider — harness-adapter admission for the aider CLI (AI pair programming
//! in your terminal, https://github.com/Aider-AI/aider).
//!
//! Aider is a process-per-invocation CLI, not a long-running client: it reads
//! `.aider.conf.yml` (home dir or git repo root) and `--read` files at startup,
//! edits files with lint/test hooks, and exits. There is no skill tree, tool
//! protocol, or delegation surface. This adapter records the evidence-backed
//! faculty census from aider's own docs (fetched 2026-09-06) and brokers
//! projection: the genuine on-disk surfaces (`.aider.conf.yml`, `CONVENTIONS.md`)
//! require either per-invocation CLI flags or merging into the user's existing
//! config, which this adapter revision does not do unilaterally.
//!
//! ## Identity law
//!
//! A model running under aider is not the Agent identity; aider is not the
//! World; an aider process is not an AgentSession. The adapter keeps `target`,
//! `product`, and any `realised_actuation_ref` distinct, and never fabricates a
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

pub const CLIENT: &str = "aider";
pub const PRODUCT: &str = "Aider";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:aider-adapter";

/// Primary sources, fetched 2026-09-06. Aider is not installed on this machine
/// (`which aider`, `pip show aider-chat` both empty; `actuation harness detect`
/// catalog r4 does not cover it), so the census cites aider's own docs/repo.
const EV_REPO: &str = "https://github.com/Aider-AI/aider";
const EV_CONF: &str = "https://aider.chat/docs/config/aider_conf.html";
const EV_OPTIONS: &str = "https://aider.chat/docs/config/options.html";
const EV_CONVENTIONS: &str = "https://aider.chat/docs/usage/conventions.html";
const EV_MODES: &str = "https://aider.chat/docs/usage/modes.html";

pub struct AiderAdapter {
    /// Where a future native config/conventions projection would be written.
    /// Unused by the brokered plan in this revision; kept as the explicit
    /// projection-root seam so the next slice does not re-derive it.
    root: PathBuf,
}

impl AiderAdapter {
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

fn aider_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_CONF],
            Some("user-level `.aider.conf.yml` in the home dir is read at startup"),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Supported,
            &[EV_CONVENTIONS, EV_CONF, EV_OPTIONS],
            Some(
                "CONVENTIONS.md convention from aider's docs, wired via `--read` or the \
                 `read:` key of the project `.aider.conf.yml`",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some("no skill or plugin system in aider"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some("no hook mechanism; aider runs one chat turn per invocation"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Degraded,
            &[EV_OPTIONS],
            Some(
                "`--watch-files` watches and re-reads *edited source files* into the chat; \
                 it does not reload instruction/config surfaces",
            ),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Supported,
            &[EV_CONF, EV_OPTIONS],
            Some("each new aider invocation re-reads `.aider.conf.yml` and `--read` files"),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some("process-per-invocation CLI; there is no persistent client to restart"),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some("no MCP or equivalent tool protocol in aider"),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some("no mechanism for third parties to contribute tools"),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Degraded,
            &[EV_OPTIONS],
            Some(
                "`--load <file>` replays a prepared command file; the cited sources do not \
                 document cross-run interactive session resume",
            ),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Degraded,
            &[EV_MODES],
            Some(
                "architect mode sends each request to a second in-process editor model; \
                 there is no subagent delegation",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Supported,
            &[EV_OPTIONS],
            Some("operates on the git repo; `--subtree-only` scopes to the current subtree"),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some("no component model"),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_CONF, EV_OPTIONS, EV_CONVENTIONS],
            Some("`.aider.conf.yml`, `CONVENTIONS.md` (via `read:`/`--read`), `.aiderignore`"),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Unsupported,
            &[EV_REPO],
            Some(
                "nothing loads live; each invocation re-reads from disk, so removal is \
                  per-invocation, not a retraction faculty",
            ),
        ),
    ]
}

impl TargetAdapter for AiderAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: true,
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: true,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(
                "aider surfaces are reached through AIKit's broker in this adapter revision",
            ),
        )
        .with_note(
            "aider's genuine on-disk surfaces (`.aider.conf.yml`, `CONVENTIONS.md`) are \
             verified in aider's docs, but activating them requires per-invocation flags \
             (`--read`) or merging into the user's existing YAML config; wiring that \
             without clobbering user config is a subsequent slice"
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

impl HarnessAdmissionAdapter for AiderAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            edition: HarnessEditionKind::Cli,
            // Not installed on this machine (`which aider`, `pip show aider-chat`
            // empty) and absent from `actuation harness detect` catalog r4, so
            // there is no observed native version to record.
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: aider_faculties(),
        }
    }
}
