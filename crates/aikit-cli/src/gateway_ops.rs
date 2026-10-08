//! Default carrier resolution for the Agency Gateway commands.
//!
//! The gateway becomes part of the bootstrap/config posture by having one
//! well-known endpoint: `~/.aikit/state/gateway.sock` with semantic state in
//! `~/.aikit/state/gateway.json`. A bare `aikit gateway serve` is a complete
//! default posture, a bare `aikit gateway status` finds it, and `doctor`
//! reports it — so the terminal lane, the agent lane and the O:I desktop all
//! agree on where the gateway lives without a flag. Explicit carriers still
//! win over every default.

use aikit_adapters::{
    coexistence_report, decide, detect, exclusive_gate, load_coexistence, probe_live,
    store_coexistence, CarrierScope, CoexistenceDecision, CoexistencePolicy, GatewayBuildIdentity,
    GatewayCarrierTarget, GatewayCoexistenceGate, GatewayConversationOperation,
    GatewayListenerReading, GatewayProcessRecord, GatewayServiceConfig, ListenerClass,
    ListenerState, DEFAULT_GATEWAY_MAX_FRAME_BYTES, GATEWAY_COEXISTENCE_FILE_NAME,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::Value;

use crate::cli::{
    GatewayAgentArgs, GatewayCoexistenceArgs, GatewayQueryArgs, GatewayServeArgs, GatewaySub,
};

/// Resolve the serve carriers. The state file always defaults to the home
/// file; the Unix carrier defaults to the home socket when no carrier is
/// named, so `serve` with no flags is same-host and discoverable. `--unix`
/// with no path is that same home socket, so `--ws ADDR --unix` serves both
/// carriers. Naming `--ws` alone is a network-only posture and binds no
/// socket (the service says so on stderr).
///
/// The WebSocket token comes from `--ws-token-location` (read once, here;
/// a group- or world-readable file is refused), else `--ws-token`, else
/// `AIKIT_GATEWAY_TOKEN`.
pub fn serve_config(home: &AikitHome, args: &GatewayServeArgs) -> Result<GatewayServiceConfig> {
    let websocket_bearer_token = match &args.websocket_token_location {
        Some(location) => Some(token_from_location(location)?),
        None => args.websocket_token.clone().or_else(gateway_token_from_env),
    };
    let named_unix = args
        .unix_socket
        .clone()
        .map(|path| path.unwrap_or_else(|| home.gateway_socket()));
    #[cfg(unix)]
    let unix_socket = named_unix.or(match &args.websocket_bind {
        Some(_) => None,
        None => Some(home.gateway_socket()),
    });
    #[cfg(not(unix))]
    let unix_socket = named_unix;
    let config = GatewayServiceConfig {
        websocket_bind: args.websocket_bind.clone(),
        websocket_bearer_token,
        unix_socket,
        state_file: args
            .state_file
            .clone()
            .or_else(|| Some(home.gateway_state())),
        max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
    };
    config.validate()?;
    Ok(config)
}

/// Resolve a query carrier: explicit `--unix`/`--ws` wins; with neither, the
/// well-known home socket.
pub fn carrier_target(home: &AikitHome, args: &GatewayQueryArgs) -> Result<GatewayCarrierTarget> {
    if args.unix_socket.is_some() && args.websocket_bind.is_some() {
        return Err(AikitError::new(
            "cli.gateway_carrier_conflict",
            "address one carrier: --unix PATH or --ws HOST:PORT, not both",
        ));
    }
    if let Some(bind) = &args.websocket_bind {
        let bearer_token = args
            .websocket_token
            .clone()
            .or_else(gateway_token_from_env)
            .ok_or_else(|| {
                AikitError::new(
                    "cli.gateway_token_required",
                    "WebSocket queries need --ws-token or AIKIT_GATEWAY_TOKEN",
                )
            })?;
        return Ok(GatewayCarrierTarget::WebSocket {
            bind: bind.clone(),
            path: args.websocket_path.clone(),
            bearer_token,
        });
    }
    #[cfg(unix)]
    {
        let path = args
            .unix_socket
            .clone()
            .unwrap_or_else(|| home.gateway_socket());
        Ok(GatewayCarrierTarget::UnixSocket(path))
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        Err(AikitError::new(
            "cli.gateway_carrier_required",
            "this platform has no default Unix carrier; address a gateway with --ws HOST:PORT",
        ))
    }
}

/// Reframe a failed default-endpoint query so the common bootstrap case says
/// what to do instead of only naming the socket.
pub fn unreachable_hint(error: &AikitError) -> Option<AikitError> {
    if error.code() == "agency_gateway_client.unix_connect" {
        Some(
            AikitError::new(
                "cli.gateway_unreachable",
                format!("{error}; start one with `aikit gateway serve`"),
            )
            .with("hint", "aikit gateway serve"),
        )
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// `--at WORKCELL_REF`: route a gateway verb through a declared remote.
// ---------------------------------------------------------------------------

/// The gateway verb's carrier, resolved from the endpoint declared for a
/// remote Workcell (`aikit gateway remote add`). The token is resolved from
/// its declared location at call time and lives only in the one request.
pub fn at_carrier(home: &AikitHome, workcell_ref: &str) -> Result<GatewayQueryArgs> {
    let declared = crate::gateway_contact::load_remotes(home)?
        .remotes
        .into_iter()
        .find(|remote| remote.workcell_ref == workcell_ref);
    let Some(remote) = declared else {
        return Err(crate::gateway_contact::three_part(
            "gateway.remote_undeclared",
            format!(
                "No gateway endpoint is declared for {workcell_ref}, so --at cannot route there."
            ),
            "Nothing was run.",
            format!(
                "Declare the endpoint first: aikit gateway remote add --workcell {workcell_ref} \
                 --ws HOST:PORT --token-location file:/ABSOLUTE/PATH"
            ),
        ));
    };
    let token = crate::secret_location::SecretLocation::parse(&remote.token_location)
        .and_then(|location| location.resolve())
        .map_err(|error| {
            crate::gateway_contact::three_part(
                "gateway.at_token_unusable",
                format!(
                    "The token declared for {workcell_ref} at {} cannot be used: {error}.",
                    remote.token_location
                ),
                "Nothing was run.",
                format!(
                    "Make it an owner-only, non-empty file (chmod 600 {}) or re-declare the \
                     endpoint with `aikit gateway remote add`.",
                    remote.token_location.trim_start_matches("file:")
                ),
            )
        })?;
    Ok(GatewayQueryArgs {
        unix_socket: None,
        websocket_bind: Some(remote.websocket_bind.clone()),
        websocket_path: remote.websocket_path.clone(),
        websocket_token: Some(token.expose().to_owned()),
    })
}

/// Whether this verb addresses a gateway carrier at all. The local-file and
/// service-management verbs do not; `--at` on them is a refusal, not a silent
/// no-op.
pub fn takes_carrier(command: &GatewaySub) -> bool {
    matches!(
        command,
        GatewaySub::Protocol(_)
            | GatewaySub::Discover(_)
            | GatewaySub::Status(_)
            | GatewaySub::Ecology(_)
            | GatewaySub::Snapshot(_)
            | GatewaySub::Who(_)
            | GatewaySub::Send(_)
            | GatewaySub::Inbox(_)
            | GatewaySub::Conversation(_)
            | GatewaySub::Delegate(_)
            | GatewaySub::Forward(_)
            | GatewaySub::Agent(_)
            | GatewaySub::NativeOwner(_)
    )
}

/// Replace every flattened carrier in the command with `carrier`. `--at` is
/// one fact about the whole invocation; the walk keeps it out of every verb's
/// dispatch branch.
pub fn override_carriers(command: &mut crate::cli::GatewayCmd, carrier: GatewayQueryArgs) {
    use GatewaySub as G;
    match &mut command.command {
        G::Protocol(a)
        | G::Discover(a)
        | G::Status(a)
        | G::Ecology(a)
        | G::Snapshot(a)
        | G::Forward(a) => *a = carrier,
        G::Who(a) => a.carrier = carrier,
        G::Send(a) => a.carrier = carrier,
        G::Inbox(a) => a.carrier = carrier,
        G::Conversation(a) => a.carrier = carrier,
        G::Delegate(a) => a.carrier = carrier,
        G::Agent(a) => a.carrier = carrier,
        G::NativeOwner(a) => a.carrier = carrier,
        G::Handoff { .. }
        | G::Message { .. }
        | G::Team { .. }
        | G::Serve(_)
        | G::Tick
        | G::InstallService(_)
        | G::UninstallService
        | G::Remote(_)
        | G::Connector(_)
        | G::Coexistence(_)
        | G::Hoist(_)
        | G::Upgrade(_)
        | G::Doctor
        | G::Modes
        | G::Setup(_)
        | G::Recover(_) => {}
    }
}

/// The hoist verb's own arguments carry no carrier: hoisting plans from this
/// home and stages to the target; it never routes through `--at`.
pub fn hoist_args(command: &crate::cli::GatewayCmd) -> Option<&crate::cli::GatewayHoistArgs> {
    match &command.command {
        GatewaySub::Hoist(args) => Some(args),
        _ => None,
    }
}

/// Read the WebSocket bearer token from its declared location, refusing a
/// location that does not parse and a `file:` that is not owner-only.
fn token_from_location(location: &str) -> Result<String> {
    let parsed = crate::secret_location::SecretLocation::parse(location)?;
    let token = parsed.resolve().map_err(|error| {
        crate::gateway_contact::three_part(
            "gateway.serve_token_unusable",
            format!("The WebSocket token at {location} cannot be used: {error}"),
            "The gateway was not started; no carrier was bound.",
            format!(
                "Make it an owner-only, non-empty file (chmod 600 {}) or name another location.",
                location.trim_start_matches("file:")
            ),
        )
        .with("source_code", error.code().to_owned())
    })?;
    Ok(token.expose().to_owned())
}

fn gateway_token_from_env() -> Option<String> {
    std::env::var("AIKIT_GATEWAY_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Coexistence
// ---------------------------------------------------------------------------

/// The coexistence document of this AIKit home.
pub fn coexistence_path(home: &AikitHome) -> std::path::PathBuf {
    home.state().join(GATEWAY_COEXISTENCE_FILE_NAME)
}

/// What coexistence detection observes right now, one line per foreign
/// harness gateway: the same reading `aikit gateway coexistence` reports,
/// for a caller that only wants the observations (the doctor's neighbours).
pub fn foreign_gateway_lines() -> Vec<String> {
    let foreign = detect(&probe_live());
    foreign
        .iter()
        .map(|gateway| format!("{}: {}", gateway.harness, gateway.evidence.join("; ")))
        .collect()
}

/// What `aikit gateway coexistence` answers with. Human output is plain
/// lines; `--json` keeps the reading for the envelope.
pub enum CoexistenceOutput {
    Text(String),
    Data(Value),
}

/// `aikit gateway coexistence`: report the policy, what foreign harness
/// gateways were observed on this machine, and the decision that follows —
/// and, with `--policy`, set the policy. Detection is inspect-only; the
/// command never touches a foreign service.
pub fn coexistence_command(
    home: &AikitHome,
    args: &GatewayCoexistenceArgs,
) -> Result<CoexistenceOutput> {
    let path = coexistence_path(home);
    let mut document = load_coexistence(&path)?;
    let mut policy_line = format!(
        "coexistence policy: {} ({})",
        document.policy.as_str(),
        path.display()
    );
    if let Some(raw) = &args.policy {
        let policy = CoexistencePolicy::parse(raw)?;
        let previous = document.policy;
        if policy != previous {
            document.policy = policy;
            store_coexistence(&path, &document)?;
        }
        policy_line = format!(
            "coexistence policy: {} ({}) — {}",
            document.policy.as_str(),
            path.display(),
            if policy != previous {
                format!("changed from {}", previous.as_str())
            } else {
                "unchanged".into()
            }
        );
    }

    let probe = probe_live();
    let foreign = detect(&probe);
    let decision = decide(document.policy, &foreign);

    let mut lines = vec![
        policy_line,
        format!("decision: {}", decision.summary()),
        format!(
            "recorded foreign bot identities: {}",
            if document.foreign_bot_identities.is_empty() {
                "none".into()
            } else {
                document
                    .foreign_bot_identities
                    .iter()
                    .map(|identity| {
                        format!(
                            "{} on {} (bot {})",
                            identity.harness, identity.platform, identity.bot_id
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        ),
    ];
    if foreign.is_empty() {
        lines.push(
            "foreign harness gateways: none observed (launchd/systemd labels, well-known \
             binaries on PATH, and state directories)"
                .into(),
        );
    } else {
        for gateway in &foreign {
            lines.push(format!(
                "foreign harness gateway {}: {}",
                gateway.harness,
                gateway.evidence.join("; ")
            ));
        }
    }
    lines.push(
        "detection is inspect-only: aikit never starts, stops or reconfigures a foreign \
         harness gateway"
            .into(),
    );

    let report = coexistence_report(&document, &path, &foreign, &decision);
    if args.json {
        Ok(CoexistenceOutput::Data(report))
    } else {
        Ok(CoexistenceOutput::Text(lines.join("\n")))
    }
}

/// What `serve` discloses about coexistence before any carrier binds, and the
/// gate (when the exclusive policy holds against a detected foreign gateway)
/// that refuses a connector whose recorded platform + bot identity the
/// foreign gateway owns.
pub struct ServeCoexistence {
    pub gate: Option<std::sync::Arc<dyn GatewayCoexistenceGate>>,
    /// Plain-words disclosure lines, printed on serve startup.
    pub lines: Vec<String>,
}

/// Read the coexistence posture for a serving gateway: policy from this
/// home's document, sightings from a live probe, decision from both.
pub fn serve_coexistence(home: &AikitHome) -> Result<ServeCoexistence> {
    let path = coexistence_path(home);
    let document = load_coexistence(&path)?;
    let foreign = detect(&probe_live());
    let decision = decide(document.policy, &foreign);
    let mut lines = vec![format!(
        "policy {} ({}): {}",
        document.policy.as_str(),
        path.display(),
        decision.summary()
    )];
    for gateway in &foreign {
        lines.push(format!(
            "foreign gateway {}: {}",
            gateway.harness,
            gateway.evidence.join("; ")
        ));
    }
    let gate = match &decision {
        CoexistenceDecision::ExclusiveHold { foreign } => {
            lines.push(
                "connector start is gated: a connector whose platform + bot identity a \
                 detected foreign gateway owns is refused"
                    .into(),
            );
            if document.foreign_bot_identities.is_empty() {
                lines.push(
                    "no foreign bot identity is recorded, so no connector is refused today; \
                     record one in the coexistence document when a foreign gateway is \
                     observed to own a bot identity"
                        .into(),
                );
            }
            Some(std::sync::Arc::new(exclusive_gate(
                foreign.clone(),
                document.foreign_bot_identities.clone(),
            )) as std::sync::Arc<dyn GatewayCoexistenceGate>)
        }
        CoexistenceDecision::Coexisting { .. } => {
            lines.push("connectors start alongside the detected foreign gateways".into());
            None
        }
        CoexistenceDecision::ExclusivelyOurs => None,
    };
    Ok(ServeCoexistence { gate, lines })
}

/// Resolve `aikit gateway agent <op>` into the canonical operation the
/// protocol carries. The names here mirror the connector-edge slash commands;
/// both spellings resolve to one `GatewayConversationOperation`.
pub fn conversation_operation(args: &GatewayAgentArgs) -> Result<GatewayConversationOperation> {
    let connector_ref = match &args.connector_ref {
        Some(raw) => Some(ResourceRef::parse(raw).map_err(|error| {
            AikitError::new(
                "cli.gateway_connector_ref_invalid",
                format!("parse connector ref {raw}: {error}"),
            )
        })?),
        None => None,
    };
    match args.operation.as_str() {
        "status" => Ok(GatewayConversationOperation::Status),
        "stop" => Ok(GatewayConversationOperation::Stop),
        "new" => Ok(GatewayConversationOperation::New),
        "sessions" => Ok(GatewayConversationOperation::Sessions),
        "restart" => Ok(GatewayConversationOperation::Restart),
        "pause" => Ok(GatewayConversationOperation::PauseConnector { connector_ref }),
        "resume" => Ok(GatewayConversationOperation::ResumeConnector { connector_ref }),
        "model" => Ok(GatewayConversationOperation::Model {
            model: args.model.clone(),
        }),
        "harness" => Ok(GatewayConversationOperation::Harness),
        "skills" => Ok(GatewayConversationOperation::Skills),
        other => Err(AikitError::new(
            "cli.gateway_agent_operation_unknown",
            format!(
                "unknown conversation operation {other:?}; use status, stop, new, sessions, \
                 restart, pause, resume, model, harness or skills"
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// The running process's posture record
// ---------------------------------------------------------------------------

/// The running gateway publishes what it is — the build it executes, the
/// carriers it serves — as a small record beside its state file, written at
/// service start and refreshed once the executable's digest has been read in
/// the background. The readers (`gateway doctor`, the upgrade surface) take
/// the running identity from the process's own record: the wire protocol on
/// the current internals carries no build identity, and a process is the only
/// honest witness of what it executes (`oi update` flips a symlink; a
/// resident keeps executing its old inode).
pub const PROCESS_RECORD_SCHEMA: &str = "aikit.gateway-process/v1";
pub const PROCESS_RECORD_FILE_NAME: &str = "gateway-process.json";

/// One published posture reading: the running build and the carriers.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GatewayProcessPosture {
    pub schema: String,
    #[serde(flatten)]
    pub build: GatewayBuildIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<GatewayListenerReading>,
}

pub fn process_record_path(home: &AikitHome) -> std::path::PathBuf {
    home.state().join(PROCESS_RECORD_FILE_NAME)
}

/// What this process is, stamped the way its build was: the exact source
/// revision when the build could read one (inside a checkout, or stamped by
/// the caller), otherwise the short one the managed updater stamps. A process
/// that cannot name its build is a finding, never a guess.
pub fn this_process_identity() -> GatewayBuildIdentity {
    GatewayBuildIdentity::of_this_process(
        option_env!("AIKIT_BUILD_SOURCE_REVISION")
            .filter(|revision| !revision.is_empty())
            .or(option_env!("SUITE_BUILD_REVISION"))
            .unwrap_or("unknown"),
        option_env!("AIKIT_BUILD_SOURCE_DIRTY") == Some("1"),
        std::env::var(crate::gateway_contact::WORKCELL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty()),
    )
}

/// The carriers this service is about to serve, as listener readings: the
/// facts the record can state before any bind (the service refuses to run
/// with a carrier it cannot bind, so a serving gateway's carriers are bound).
pub fn configured_listeners(config: &GatewayServiceConfig) -> Vec<GatewayListenerReading> {
    let mut listeners = Vec::new();
    #[cfg(unix)]
    if let Some(path) = &config.unix_socket {
        listeners.push(GatewayListenerReading {
            carrier: "unix".into(),
            bind: path.display().to_string(),
            class: ListenerClass::LocalIpc,
            scope: CarrierScope::Owner,
            state: ListenerState::Bound,
            detail: None,
        });
    }
    if let Some(bind) = &config.websocket_bind {
        listeners.push(GatewayListenerReading {
            carrier: "websocket".into(),
            bind: bind.clone(),
            class: ListenerClass::classify_bind(bind),
            scope: CarrierScope::Peer,
            state: ListenerState::Bound,
            detail: None,
        });
    }
    listeners
}

/// Publish the posture record and keep it fresh until the executable digest
/// has been read. Called by the serve arm before the carriers start; the
/// digest thread ends on its own once the digest is written (or after a
/// generous bound — a record without a digest is a named doctor finding, not
/// a hang).
pub fn publish_process_record(
    home: &AikitHome,
    listeners: Vec<GatewayListenerReading>,
) -> Result<()> {
    // The record is published before the carriers start — frequently before
    // anything else has created the state directory on a fresh home.
    std::fs::create_dir_all(home.state()).map_err(|error| {
        AikitError::new(
            "gateway.process_record_directory",
            format!(
                "create the gateway state directory {}: {error}",
                home.state().display()
            ),
        )
        .with_io_source(error)
    })?;
    let record = GatewayProcessRecord::new(this_process_identity());
    let path = process_record_path(home);
    let write = move |build: &GatewayBuildIdentity| -> Result<()> {
        let posture = GatewayProcessPosture {
            schema: PROCESS_RECORD_SCHEMA.into(),
            build: build.clone(),
            listeners: listeners.clone(),
        };
        crate::gateway_upgrade::write_atomic(
            &path,
            &serde_json::to_vec_pretty(&posture).map_err(|error| {
                AikitError::new(
                    "gateway.process_record_encode",
                    format!("encode the gateway posture record: {error}"),
                )
            })?,
        )
    };
    write(&record.build())?;
    let writer = std::thread::Builder::new()
        .name("gateway-posture-digest".into())
        .spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
            while std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let build = record.build();
                if build.executable_sha256.is_some() {
                    let _ = write(&build);
                    return;
                }
            }
        });
    match writer {
        Ok(_) => Ok(()),
        Err(error) => Err(AikitError::new(
            "gateway.process_record_thread",
            format!("start the posture digest reader: {error}"),
        )),
    }
}

/// The published posture of the gateway answering on this home, when one has
/// published it. A record is trusted only beside a gateway that answers (the
/// callers ask the socket first): a record without a live gateway is stale
/// and never read.
pub fn read_process_record(home: &AikitHome) -> Option<GatewayProcessPosture> {
    let bytes = std::fs::read(process_record_path(home)).ok()?;
    serde_json::from_slice(&bytes).ok()
}
