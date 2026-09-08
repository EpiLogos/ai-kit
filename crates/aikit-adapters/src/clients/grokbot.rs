//! Grok Bot — xAI's agent platform as installed on this machine: a background
//! daemon/service (`~/.grokbot`, keychain item "Grok Bot Safe Storage") plus
//! the `grok-bot`/`gbot` CLI used to manage bots and groups. Admitted through
//! the harness-adapter contract with discovery riding an Actuation
//! `actuation.harness-detection/v1` record (state `detected`, edition
//! `cli+service`, native_owner xAI).
//!
//! Honest scope note: the realised agent runs as a service reached through a
//! gateway (GROK_BOT_GATEWAY_URL/TOKEN, app session, or CURSOR_ACCESS_TOKEN);
//! the CLI installed here (`grok-bot-cli` 0.2.2, a third-party client by
//! ScriptedAlchemy) manages bots/groups and reads threads through that
//! gateway. No on-disk instruction/skill tree the daemon reads is documented
//! or observed — `~/.grokbot` holds daemon state and app settings, not a
//! projection surface. The daemon was NOT running at detection time and the
//! version probe is keychain-gated (catalog summary says so; `grok-bot
//! --version` fails non-interactively on this machine). Projection is
//! therefore brokered, and lifecycle faculties that require a live daemon
//! are recorded as Unknown rather than invented.
//!
//! Edition mapping: the catalog edition `cli+service` has no
//! [`HarnessEditionKind`] variant; this adapter records `Custom` and carries
//! the catalog edition in the Surfaces faculty evidence.
//!
//! ## Identity law
//!
//! A Grok Bot is not the Agent identity; the gateway service is not the
//! World; no `realised_actuation_ref` is fabricated — Actuation declared no
//! capability descriptor for `grok-bot` at admission time
//! (`actuation harness capability grok-bot`: undeclared).

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

pub const CLIENT: &str = "grok-bot";
pub const PRODUCT: &str = "Grok Bot";
/// Adapter identity inside the admission contract; stable, not the harness's own name.
pub const ADAPTER_REF: &str = "aikit:grokbot-adapter";

/// Actuation detection record produced by this admission's own
/// `actuation harness detect --json --versions` run (catalog_revision 4).
const EV_DETECT: &str = "actuation.harness-detection/v1 \
detection:2026-09-06T23:42:55.567Z grok-bot:detected \
exe:/Users/admin/.npm-global/bin/grok-bot \
sha256:37d4ce54ab559bd80b37174258ae3cad232d5e31f6c1beb161c79359b0d2deee \
observed:2026-09-06T23:42:55.567Z";

/// Actuation catalog descriptor for grok-bot, from this admission's own
/// `actuation harness catalog --json` run.
const EV_CATALOG: &str = "actuation.harness-detection/v1 catalog:revision-4 descriptor \
grok-bot edition:cli+service native_owner:xAI aliases:gbot";

/// CLI surface observed directly (`grok-bot --help`, 2026-09-06).
const EV_CLI_HELP: &str =
    "native:grok-bot --help (gbot bots/groups/send/thread surface observed 2026-09-06)";

/// The installed npm client package (third-party; the xAI daemon is separate).
const EV_CLI_PKG: &str = "npm:grok-bot-cli@0.2.2 installed at \
~/.npm-global/lib/node_modules/grok-bot-cli (github.com/ScriptedAlchemy/grok-bot-cli)";

/// App settings observed on disk: MCP box servers, per-server tool policy,
/// local tool permission.
const EV_SETTINGS: &str = "native:~/.grokbot/settings.json (mcpBoxServers, mcpCustomInstructions, \
mcpDisabledToolsByServerId, localToolPermission)";

/// Daemon state files observed on disk; the detection service probe reports
/// the daemon not running.
const EV_DAEMON: &str = "native:~/.grokbot/local-exec-daemon-*.json + local-exec-supervisor.json; \
detection service probe: 'daemon grok-bot not running'";

/// The version probe reaches for the keychain and fails non-interactively.
const EV_KEYCHAIN: &str = "native:grok-bot --version -> security find-generic-password -s \
'Grok Bot Safe Storage' (fails headless); catalog summary: version probe may prompt the keychain";

pub struct GrokbotAdapter {
    /// Projection seam kept for parity with the other admitted adapters; the
    /// brokered plan in this revision writes nothing, so this root is unused.
    root: PathBuf,
}

impl GrokbotAdapter {
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

fn grokbot_faculties() -> Vec<HarnessFacultyObservation> {
    vec![
        faculty(
            HarnessFaculty::StandingInstructions,
            FacultySupport::Supported,
            &[EV_CLI_HELP, EV_DETECT],
            Some(
                "bots and groups carry a persistent UI Instructions field settable via \
                 'gbot bots|groups update --instructions' through the gateway",
            ),
        ),
        faculty(
            HarnessFaculty::ProjectInstructions,
            FacultySupport::Unsupported,
            &[EV_CLI_HELP],
            Some(
                "no project-root instruction surface observed; instructions are per-bot/per-group \
                 on the service, not per working tree",
            ),
        ),
        faculty(
            HarnessFaculty::NativeSkills,
            FacultySupport::Unsupported,
            &[EV_CATALOG, EV_DAEMON],
            Some("no skills facet in the catalog descriptor and no skill tree in ~/.grokbot"),
        ),
        faculty(
            HarnessFaculty::SessionStartHook,
            FacultySupport::Unsupported,
            &[],
            Some("no session-start hook surface observed in the CLI or daemon state"),
        ),
        faculty(
            HarnessFaculty::LiveReload,
            FacultySupport::Unknown,
            &[EV_DAEMON],
            Some("daemon was not running at detection; live reload behavior is unobservable"),
        ),
        faculty(
            HarnessFaculty::NextSessionReload,
            FacultySupport::Unknown,
            &[EV_DAEMON, EV_KEYCHAIN],
            Some(
                "instruction pickup timing through the gateway/daemon is unverified without \
                 docs or a running daemon",
            ),
        ),
        faculty(
            HarnessFaculty::RestartReload,
            FacultySupport::Unsupported,
            &[EV_CLI_HELP],
            Some(
                "gbot is process-per-invocation; the daemon is a service, not a restartable \
                 client of projected material",
            ),
        ),
        faculty(
            HarnessFaculty::ToolProtocol,
            FacultySupport::Degraded,
            &[EV_SETTINGS],
            Some(
                "app settings expose MCP box servers, per-server custom instructions and \
                 disabled tools, and a local-tool permission — a native tool protocol exists, \
                 but its schema is unverified and the daemon is down",
            ),
        ),
        faculty(
            HarnessFaculty::NativeToolContribution,
            FacultySupport::Unknown,
            &[EV_SETTINGS, EV_DAEMON],
            Some(
                "local-exec daemon files suggest local tool execution, but the contribution \
                 surface is not inspectable from here",
            ),
        ),
        faculty(
            HarnessFaculty::SessionResume,
            FacultySupport::Supported,
            &[EV_CLI_HELP],
            Some("gbot thread/chat with --root MESSAGE_ID reads thread history per bot or group"),
        ),
        faculty(
            HarnessFaculty::DelegatedAgents,
            FacultySupport::Supported,
            &[EV_CLI_HELP],
            Some("groups compose up to 6 member bots (create/add/remove/set); multi-agent surface"),
        ),
        faculty(
            HarnessFaculty::ProjectRoots,
            FacultySupport::Degraded,
            &[EV_CLI_HELP],
            Some(
                "--dir DIR / GROK_BOT_AGENTS_DIR scope the CLI's data directory; that is a \
                 data-root switch, not a project-instruction surface",
            ),
        ),
        faculty(
            HarnessFaculty::Components,
            FacultySupport::Unsupported,
            &[EV_CLI_HELP],
            Some(
                "avatar shapes/colors and titles are bot profile metadata, not reusable UI \
                 components",
            ),
        ),
        faculty(
            HarnessFaculty::Surfaces,
            FacultySupport::Supported,
            &[EV_DETECT, EV_CATALOG, EV_CLI_HELP, EV_CLI_PKG],
            Some(
                "catalog edition cli+service: management CLI (binaries grok-bot/gbot) plus the \
                 background daemon service and Grok Bot app session auth",
            ),
        ),
        faculty(
            HarnessFaculty::LiveRetraction,
            FacultySupport::Degraded,
            &[EV_CLI_HELP, EV_DAEMON],
            Some(
                "bots delete / groups delete / groups remove exist over the gateway; retraction \
                 from a running daemon is unverifiable while the daemon is down",
            ),
        ),
    ]
}

impl TargetAdapter for GrokbotAdapter {
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
                "Grok Bot's realised surface is the xAI daemon/gateway plus app session; no \
                 on-disk instruction tree the daemon reads is documented or observed, so \
                 provisioning flows through the gateway management API rather than a \
                 projected file tree",
            ),
        )
        .with_note(
            "~/.grokbot holds daemon state and app settings (MCP config, tool permissions), \
             not a projection surface; bots/groups and their instructions are managed through \
             the grok-bot CLI against the gateway. The daemon was not running at detection \
             (2026-09-06) and the version probe is keychain-gated, so daemon-side lifecycle \
             faculties are recorded as Unknown in the census"
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

impl HarnessAdmissionAdapter for GrokbotAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: ADAPTER_REF.to_string(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            target: self.target(),
            product: PRODUCT.to_string(),
            // Catalog edition is "cli+service"; no HarnessEditionKind variant
            // covers it, so the admission records Custom and cites the catalog
            // descriptor as evidence on the Surfaces faculty.
            edition: HarnessEditionKind::Custom,
            // The version probe is keychain-gated and fails headless; the
            // installed management CLI is grok-bot-cli 0.2.2 (cited on
            // Surfaces). No honest native_version is available.
            native_version: None,
            source_revision: None,
            realised_actuation_ref: None,
            project_binding_ref: None,
            faculties: grokbot_faculties(),
        }
    }
}
