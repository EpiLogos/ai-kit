//! `aikit gateway modes` and `aikit gateway setup` — the operating modes as data
//! and as an ordinary way to choose one.
//!
//! The crosswalk in `docs/GATEWAY-OPERATING-MODES.md` and this table are one
//! source: the document explains, this is what the commands read. A mode is a
//! choice about **listener binding, transport and lifecycle only**; workcell
//! placement, connector identity and session continuity are read from their
//! owners and never change with it, so no mode can create another agent or
//! another conversation model.
//!
//! `setup` is plan-first. With no `--apply` it changes nothing and prints every
//! step, including the commands it would not run itself. Two separate flags
//! separate two separate kinds of change:
//!
//! * `--apply` changes *aikit's own* posture: owner-only token files, the
//!   service definition, the declared peers.
//! * `--apply-tailscale` additionally runs the one private `tailscale serve`
//!   mapping, and only after checking that the port is unmapped (a Funnel is
//!   the same mapping at another access level) and that the result reads
//!   `(tailnet only)`. It never runs `tailscale funnel`.

use std::path::{Path, PathBuf};

use aikit_adapters::ListenerClass;
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::gateway_contact::three_part;

pub const MODES_SCHEMA: &str = "aikit.gateway-modes/v1";
pub const SETUP_SCHEMA: &str = "aikit.gateway-setup/v1";

/// What the repository can honestly say about a mode today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    /// Exercised by a test or a live probe.
    Shipped,
    /// Composed from shipped parts and planned/guarded by this tooling; the
    /// real transport has not been exercised end to end.
    Composed,
    /// Refused by default.
    Refused,
    /// Never configured or assumed.
    Never,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryMode {
    pub id: &'static str,
    pub title: &'static str,
    pub binding: &'static str,
    pub transport: &'static str,
    pub reach: &'static str,
    pub authenticates: &'static str,
    pub confidentiality: &'static str,
    pub standing: Standing,
    pub how: &'static str,
}

pub const ENTRY_MODES: [EntryMode; 7] = [
    EntryMode {
        id: "local-ipc",
        title: "Local IPC",
        binding: "Unix socket ~/.aikit/state/gateway.sock, mode 0600",
        transport: "unix",
        reach: "local processes the file mode admits",
        authenticates: "file mode: owner scope",
        confidentiality: "none needed; stays in the kernel",
        standing: Standing::Shipped,
        how: "aikit gateway install-service",
    },
    EntryMode {
        id: "loopback-service",
        title: "Loopback service",
        binding: "127.0.0.1:PORT",
        transport: "websocket",
        reach: "any local process of any user, local browsers",
        authenticates: "bearer token (peer) / owner token",
        confidentiality: "none (local plaintext)",
        standing: Standing::Shipped,
        how: "aikit gateway setup --mode loopback-service",
    },
    EntryMode {
        id: "private-tailnet",
        title: "Private tailnet connection",
        binding: "the node's tailnet address (100.x)",
        transport: "websocket",
        reach: "tailnet peers the tailnet policy admits, plus local processes",
        authenticates: "bearer token; WireGuard authenticates the machine",
        confidentiality: "WireGuard node to node; the app sees plaintext ws://",
        standing: Standing::Shipped,
        how: "aikit gateway setup --mode private-tailnet",
    },
    EntryMode {
        id: "tailscale-serve",
        title: "Tailscale Serve front (private)",
        binding: "127.0.0.1:PORT behind `tailscale serve --tcp PORT`",
        transport: "websocket over a tailnet TCP forward",
        reach: "tailnet peers the policy admits on the serve port, plus local processes",
        authenticates: "bearer token; the node key authenticates the machine",
        confidentiality: "WireGuard; the backend hop is loopback",
        standing: Standing::Composed,
        how: "aikit gateway setup --mode tailscale-serve",
    },
    EntryMode {
        id: "ssh-tunnel",
        title: "Supported SSH / tunnel entry",
        binding: "far gateway on 127.0.0.1; near end 127.0.0.1:LOCAL",
        transport: "websocket inside ssh -L",
        reach: "whoever can log in to the far host; every local user at the near end",
        authenticates: "SSH keys (or Tailscale SSH) plus the bearer token",
        confidentiality: "SSH",
        standing: Standing::Composed,
        how: "aikit gateway setup --mode ssh-tunnel --peer WORKCELL=HOST:PORT",
    },
    EntryMode {
        id: "remote-authenticated-endpoint",
        title: "Explicit remote authenticated endpoint",
        binding: "a routable address",
        transport: "websocket (no TLS in the carrier)",
        reach: "anyone who can route to it",
        authenticates: "bearer token only",
        confidentiality: "none unless TLS is terminated in front",
        standing: Standing::Refused,
        how: "refused; terminate TLS in front and pass --allow-wide-bind on install-service if that is what you mean",
    },
    EntryMode {
        id: "tailscale-funnel",
        title: "Tailscale Funnel (public)",
        binding: "—",
        transport: "—",
        reach: "the whole internet",
        authenticates: "nothing from Tailscale",
        confidentiality: "TLS on the node",
        standing: Standing::Never,
        how: "never configured by aikit; `doctor` fails if a gateway port is funnelled",
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct LifecycleMode {
    pub id: &'static str,
    pub kept_by: &'static str,
    pub restarts_itself: bool,
    pub upgrade: &'static str,
}

pub const LIFECYCLES: [LifecycleMode; 4] = [
    LifecycleMode {
        id: "foreground",
        kept_by: "you, in a terminal",
        restarts_itself: false,
        upgrade: "installs; does not stop it; names the command that starts the new build",
    },
    LifecycleMode {
        id: "supervised-launchd",
        kept_by: "a LaunchAgent (KeepAlive)",
        restarts_itself: true,
        upgrade: "drains, exits, the agent starts the new build, verified",
    },
    LifecycleMode {
        id: "supervised-systemd",
        kept_by: "a systemd user unit (Restart=always)",
        restarts_itself: true,
        upgrade: "drains, exits, the unit starts the new build, verified",
    },
    LifecycleMode {
        id: "application",
        kept_by: "O:I or the desktop, which owns the process",
        restarts_itself: false,
        upgrade: "installs and leaves it running; the application restarts it",
    },
];

/// The five questions a mode must not be confused with.
pub const AXES: [(&str, &str, &str); 7] = [
    (
        "listener-binding",
        "where a socket is bound, so who could possibly reach it",
        "moves with the mode",
    ),
    (
        "carrier-scope",
        "what a peer that got through may do (owner or peer)",
        "never moves with the mode",
    ),
    (
        "transport",
        "the bytes: unix, websocket, a tailnet TCP forward, ssh",
        "moves with the mode",
    ),
    (
        "workcell-placement",
        "which machine's Actuation answers who occupies a Position; where a session lives",
        "never moves with the mode",
    ),
    (
        "connector-identity",
        "which external identity a Surface is, and its per-sender admission",
        "never moves with the mode",
    ),
    (
        "session-continuity",
        "which Stream, AgentSession and delivery a turn belongs to",
        "never moves with the mode",
    ),
    (
        "lifecycle",
        "who keeps the process running and restarts it",
        "moves with the mode; decides whether an upgrade can restart it",
    ),
];

fn mode(id: &str) -> Option<&'static EntryMode> {
    ENTRY_MODES.iter().find(|mode| mode.id == id)
}

/// The mode a declared remote endpoint looks like, from its bind alone. A
/// loopback endpoint is reached through something (an ssh -L forward, a local
/// Serve) that this machine cannot see from the address.
pub fn classify_endpoint(endpoint: &str) -> &'static str {
    match ListenerClass::classify_bind(endpoint) {
        ListenerClass::Tailnet => "private-tailnet",
        ListenerClass::Loopback => {
            "ssh-tunnel-or-forward (a loopback endpoint is fronted by something local)"
        }
        ListenerClass::Named if endpoint.contains(".ts.net") => "tailscale-serve (a tailnet name)",
        ListenerClass::Named => "named-host (resolved at connect; reach unknown)",
        ListenerClass::PrivateNetwork => "lan (not a private-tailnet path)",
        ListenerClass::Public | ListenerClass::Wildcard => {
            "remote-authenticated-endpoint (refused by default)"
        }
        ListenerClass::LocalIpc => "local-ipc",
    }
}

/// `aikit gateway modes`: the table, and what this machine actually runs.
pub fn reading(home: &AikitHome) -> Result<Value> {
    let facts = crate::gateway_doctor::gather(home)?;
    let running = facts.running.as_ref();
    let in_use: Vec<String> = {
        let mut used = Vec::new();
        if let Some(running) = running {
            for listener in &running.listeners {
                if listener.carrier == "unix" {
                    used.push("local-ipc".to_owned());
                } else {
                    used.push(
                        match listener.class {
                            ListenerClass::Loopback => {
                                let fronted = facts.tailscale.as_ref().is_some_and(|t| {
                                    t.serve_targets.iter().any(|(_, target)| {
                                        target.rsplit_once(':').map(|(_, p)| p)
                                            == listener.bind.rsplit_once(':').map(|(_, p)| p)
                                    })
                                });
                                if fronted {
                                    "tailscale-serve"
                                } else {
                                    "loopback-service"
                                }
                            }
                            ListenerClass::Tailnet => "private-tailnet",
                            _ => "remote-authenticated-endpoint",
                        }
                        .to_owned(),
                    );
                }
            }
        }
        used.sort();
        used.dedup();
        used
    };
    let lifecycle = running
        .and_then(|r| r.build.as_ref())
        .map(|b| b.lifecycle.as_str());
    Ok(json!({
        "schema": MODES_SCHEMA,
        "axes": AXES.iter().map(|(id, asks, moves)| json!({"axis": id, "asks": asks, "moves_with_mode": moves})).collect::<Vec<_>>(),
        "entry_modes": ENTRY_MODES.iter().map(|mode| {
            let mut value = serde_json::to_value(mode).unwrap_or(Value::Null);
            value["in_use"] = json!(in_use.iter().any(|used| used == mode.id));
            value
        }).collect::<Vec<_>>(),
        "lifecycle_modes": LIFECYCLES.iter().map(|lifecycle_mode| {
            let mut value = serde_json::to_value(lifecycle_mode).unwrap_or(Value::Null);
            value["in_use"] = json!(Some(lifecycle_mode.id) == lifecycle);
            value
        }).collect::<Vec<_>>(),
        "this_machine": {
            "gateway_answering": running.is_some(),
            "lifecycle": lifecycle,
            "listeners": running.map(|r| r.listeners.clone()),
            "peers": facts.remotes.iter().map(|remote| json!({
                "workcell_ref": remote["workcell_ref"],
                "endpoint": remote["endpoint"],
                "looks_like": classify_endpoint(remote["endpoint"].as_str().unwrap_or("")),
                "reachable": remote["reachable"],
                "revision": remote["revision"],
            })).collect::<Vec<_>>(),
            "tailscale": facts.tailscale.as_ref().map(|t| json!({
                "running": t.backend_running,
                "addresses": t.node_addresses,
                "serve_targets": t.serve_targets,
                "funnel_ports": t.funnel_ports,
            })),
        },
        "guide": "docs/GATEWAY-OPERATING-MODES.md",
    }))
}

// ---------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SetupInputs {
    pub mode: String,
    pub port: u16,
    /// Bind override (`HOST:PORT`); otherwise derived from the mode.
    pub bind: Option<String>,
    pub gateway_ref: Option<String>,
    pub workcell_ref: Option<String>,
    /// `WORKCELL=HOST:PORT` peers to declare.
    pub peers: Vec<(String, String)>,
    pub peer_token_location: Option<String>,
    pub owner_token_location: Option<String>,
    /// The token a declared peer expects of us (by location).
    pub peer_remote_token_location: Option<String>,
    pub allow_wide_bind: bool,
    /// The node's tailnet address, when `tailscale ip -4` answered.
    pub tailnet_address: Option<String>,
    /// Whether a service definition already exists.
    pub service_installed: bool,
    /// The `AIKIT_HOME` that existing definition serves, when it names one.
    pub service_home: Option<String>,
    /// Serve mappings already on this node: `(tailnet port, target)`.
    pub serve_targets: Vec<(String, String)>,
    pub funnel_ports: Vec<String>,
    pub home_dir: PathBuf,
    pub aikit_home: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SetupStep {
    /// Create an owner-only random token file if it does not exist.
    Token { path: String, purpose: String },
    /// (Re)write the service definition and start it.
    InstallService {
        websocket_bind: Option<String>,
        token_location: Option<String>,
        owner_token_location: Option<String>,
        gateway_ref: Option<String>,
        workcell_ref: Option<String>,
        allow_wide_bind: bool,
        replace: bool,
    },
    /// Declare a peer gateway.
    RemoteAdd {
        workcell_ref: String,
        endpoint: String,
        token_location: String,
    },
    /// A private Tailscale Serve mapping (never Funnel): run only with
    /// `--apply-tailscale`.
    TailscaleServe { port: u16 },
    /// A command this tooling prints and does not run.
    OperatorCommand { command: String, why: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupPlan {
    pub schema: &'static str,
    pub mode: String,
    pub steps: Vec<SetupStep>,
    pub warnings: Vec<String>,
    pub standing: String,
}

fn default_token_path(inputs: &SetupInputs, name: &str) -> String {
    format!(
        "file:{}",
        inputs.aikit_home.join("credentials").join(name).display()
    )
}

/// The setup plan for a mode. Pure: nothing is read or changed here.
pub fn plan_setup(inputs: &SetupInputs) -> Result<SetupPlan> {
    let Some(entry) = mode(&inputs.mode) else {
        return Err(three_part(
            "gateway.setup_mode_unknown",
            format!("`{}` is not an operating mode.", inputs.mode),
            "Nothing was planned.",
            format!(
                "Choose one of: {}. `aikit gateway modes` explains each.",
                ENTRY_MODES
                    .iter()
                    .map(|m| m.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    };
    // The service definition is per user, not per AIKIT_HOME: installing from
    // another home would replace the service of the home it really serves. Refused
    // in the plan, before anything is touched; a separate instance is the way.
    if inputs.service_installed {
        if let Some(theirs) = &inputs.service_home {
            let ours = inputs.aikit_home.display().to_string();
            let canonical = |path: &str| {
                std::fs::canonicalize(path)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| path.trim_end_matches('/').to_owned())
            };
            if canonical(&ours) != canonical(theirs) {
                return Err(three_part(
                    "gateway.setup_other_homes_service",
                    format!(
                        "The installed gateway service serves AIKIT_HOME={theirs}, and this is {ours}: setting up here would replace that service."
                    ),
                    "Nothing was planned or changed.",
                    "Run setup against the service's own home, or name a separate instance first (`AIKIT_GATEWAY_SERVICE_INSTANCE=<name>`), which manages its own definition.",
                ));
            }
        }
    }
    let port = if inputs.port == 0 { 7788 } else { inputs.port };
    let mut steps: Vec<SetupStep> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let peer_token = inputs
        .peer_token_location
        .clone()
        .unwrap_or_else(|| default_token_path(inputs, "gateway-ws.token"));
    let owner_token = inputs
        .owner_token_location
        .clone()
        .unwrap_or_else(|| default_token_path(inputs, "gateway-owner.token"));

    let ws_steps = |bind: String, steps: &mut Vec<SetupStep>| {
        steps.push(SetupStep::Token {
            path: peer_token.clone(),
            purpose: "the peer token: what another Workcell presents to relay and ask".into(),
        });
        steps.push(SetupStep::Token {
            path: owner_token.clone(),
            purpose: "the owner token: what drains, restores and upgrades over the network (kept separate)".into(),
        });
        steps.push(SetupStep::InstallService {
            websocket_bind: Some(bind),
            token_location: Some(peer_token.clone()),
            owner_token_location: Some(owner_token.clone()),
            gateway_ref: inputs.gateway_ref.clone(),
            workcell_ref: inputs.workcell_ref.clone(),
            allow_wide_bind: inputs.allow_wide_bind,
            replace: inputs.service_installed,
        });
    };

    match entry.id {
        "local-ipc" => steps.push(SetupStep::InstallService {
            websocket_bind: None,
            token_location: None,
            owner_token_location: None,
            gateway_ref: inputs.gateway_ref.clone(),
            workcell_ref: inputs.workcell_ref.clone(),
            allow_wide_bind: false,
            replace: inputs.service_installed,
        }),
        "loopback-service" => {
            let bind = inputs.bind.clone().unwrap_or_else(|| format!("127.0.0.1:{port}"));
            if ListenerClass::classify_bind(&bind) != ListenerClass::Loopback {
                return Err(three_part(
                    "gateway.setup_bind_not_loopback",
                    format!("--mode loopback-service binds loopback, and {bind} is {}.", ListenerClass::classify_bind(&bind).as_str()),
                    "Nothing was planned.",
                    "Use --mode private-tailnet for a tailnet address, or a 127.0.0.1 bind.",
                ));
            }
            ws_steps(bind, &mut steps);
        }
        "private-tailnet" => {
            let bind = match (&inputs.bind, &inputs.tailnet_address) {
                (Some(bind), _) => bind.clone(),
                (None, Some(address)) => format!("{address}:{port}"),
                (None, None) => {
                    return Err(three_part(
                        "gateway.setup_tailnet_address_unknown",
                        "No tailnet address could be read (`tailscale ip -4` did not answer).",
                        "Nothing was planned.",
                        "Start Tailscale, or name the bind with --bind 100.x.y.z:PORT.",
                    ))
                }
            };
            if ListenerClass::classify_bind(&bind) != ListenerClass::Tailnet {
                return Err(three_part(
                    "gateway.setup_bind_not_tailnet",
                    format!("--mode private-tailnet binds a tailnet address, and {bind} is {}.", ListenerClass::classify_bind(&bind).as_str()),
                    "Nothing was planned.",
                    "Use the address from `tailscale ip -4`, or choose --mode tailscale-serve to keep the listener on loopback.",
                ));
            }
            ws_steps(bind, &mut steps);
            warnings.push(
                "macOS: the application firewall queues inbound connections to a newly installed, \
                 ad-hoc-signed aikit until the owner allows it, and an update installs a new one. \
                 `aikit gateway doctor` names the command; --mode tailscale-serve avoids it."
                    .into(),
            );
        }
        "tailscale-serve" => {
            let bind = inputs.bind.clone().unwrap_or_else(|| format!("127.0.0.1:{port}"));
            if ListenerClass::classify_bind(&bind) != ListenerClass::Loopback {
                return Err(three_part(
                    "gateway.setup_bind_not_loopback",
                    format!("--mode tailscale-serve keeps the listener on loopback, and {bind} is {}.", ListenerClass::classify_bind(&bind).as_str()),
                    "Nothing was planned.",
                    "Use a 127.0.0.1 bind; Tailscale Serve is what reaches it.",
                ));
            }
            let serve_port: u16 = bind
                .rsplit_once(':')
                .and_then(|(_, p)| p.parse().ok())
                .unwrap_or(port);
            if inputs.funnel_ports.iter().any(|p| p == &serve_port.to_string()) {
                return Err(three_part(
                    "gateway.setup_port_funnelled",
                    format!("Port {serve_port} is published to the internet by Tailscale Funnel on this node."),
                    "Nothing was planned: a Serve mapping and a Funnel are two access levels of the same port mapping.",
                    "Turn the Funnel off yourself (`tailscale funnel --https=… off`), or choose another port.",
                ));
            }
            if let Some((_, target)) = inputs.serve_targets.iter().find(|(p, _)| p == &serve_port.to_string()) {
                return Err(three_part(
                    "gateway.setup_port_mapped",
                    format!("Tailscale Serve already maps port {serve_port} on this node (to {target})."),
                    "Nothing was planned: replacing it would rewrite a handler this tooling does not own, and a later `tailscale funnel` would publish whatever it becomes.",
                    "Choose another port (--port), or remove the existing mapping yourself.",
                ));
            }
            ws_steps(bind, &mut steps);
            steps.push(SetupStep::TailscaleServe { port: serve_port });
            warnings.push(
                "The Serve mapping is private (tailnet only). Nothing here runs `tailscale funnel`, and \
                 `aikit gateway doctor` fails if a funnelled port ever points at a gateway."
                    .into(),
            );
            warnings.push(
                "A WebSocket upgrade through a raw `--tcp` forward is expected to ride unchanged; it \
                 has not been exercised against a real peer by this tooling. Verify with \
                 `aikit gateway --at <peer> status` from another machine."
                    .into(),
            );
        }
        "ssh-tunnel" => {
            let bind = inputs.bind.clone().unwrap_or_else(|| format!("127.0.0.1:{port}"));
            if ListenerClass::classify_bind(&bind) != ListenerClass::Loopback {
                return Err(three_part(
                    "gateway.setup_bind_not_loopback",
                    format!("--mode ssh-tunnel keeps the far gateway on loopback, and {bind} is {}.", ListenerClass::classify_bind(&bind).as_str()),
                    "Nothing was planned.",
                    "Use a 127.0.0.1 bind; the ssh forward is what reaches it.",
                ));
            }
            ws_steps(bind, &mut steps);
            for (workcell, endpoint) in &inputs.peers {
                let local_port = endpoint.rsplit_once(':').map(|(_, p)| p).unwrap_or("7788");
                steps.push(SetupStep::OperatorCommand {
                    command: format!(
                        "ssh -N -L 127.0.0.1:{local_port}:127.0.0.1:{port} USER@HOST_OF_{workcell}"
                    ),
                    why: format!(
                        "the tunnel that makes {workcell}'s gateway reachable at {endpoint}; keep it \
                         running under your own supervisor (autossh, a LaunchAgent, a systemd unit)"
                    ),
                });
            }
            warnings.push(
                "The tunnel dies with its ssh session and does not survive a reboot unless something \
                 supervises it. Every local user can use the near end of the forward."
                    .into(),
            );
        }
        "remote-authenticated-endpoint" => {
            return Err(three_part(
                "gateway.setup_mode_refused",
                "A routable WebSocket endpoint carries the bearer token with no TLS: the carrier does not speak wss://.",
                "Nothing was planned.",
                "Use --mode private-tailnet, --mode tailscale-serve or --mode ssh-tunnel. If you terminate TLS in front yourself, `aikit gateway install-service --allow-wide-bind` is the explicit way.",
            ))
        }
        "tailscale-funnel" => {
            return Err(three_part(
                "gateway.setup_mode_refused",
                "Tailscale Funnel publishes a service to the whole internet; it is never configured, and a gateway's bearer token is not an internet-facing defence.",
                "Nothing was planned.",
                "Keep the gateway on the tailnet. If you truly intend a public endpoint, that is an owner decision made with `tailscale funnel` by hand, with application authentication the gateway does not have.",
            ))
        }
        _ => unreachable!("every mode is handled"),
    }

    // Peers (every mode that speaks over a network).
    if entry.id != "local-ipc" {
        for (workcell, endpoint) in &inputs.peers {
            let class = ListenerClass::classify_bind(endpoint);
            if matches!(class, ListenerClass::Wildcard | ListenerClass::Public) {
                warnings.push(format!(
                    "peer {workcell} at {endpoint} is {}: a bearer token over ws:// to a routable address is sent in the clear",
                    class.as_str()
                ));
            }
            steps.push(SetupStep::RemoteAdd {
                workcell_ref: workcell.clone(),
                endpoint: endpoint.clone(),
                token_location: inputs
                    .peer_remote_token_location
                    .clone()
                    .unwrap_or_else(|| {
                        default_token_path(
                            inputs,
                            &format!("{}-gateway.token", workcell.replace("workcell:", "")),
                        )
                    }),
            });
        }
    }
    Ok(SetupPlan {
        schema: SETUP_SCHEMA,
        mode: entry.id.to_owned(),
        steps,
        warnings,
        standing: format!("{:?}", entry.standing).to_lowercase(),
    })
}

/// A fresh random token: 32 bytes, hex, from the operating system.
#[cfg(unix)]
pub fn generate_token() -> Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| {
            AikitError::new(
                "gateway.setup_random",
                format!("could not read the operating system's random source: {error}"),
            )
        })?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Create an owner-only (0600) token file. An existing file is kept: a setup
/// run twice must not rotate a secret another machine already holds.
#[cfg(unix)]
pub fn ensure_token(location: &str) -> Result<bool> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let Some(path) = location.strip_prefix("file:") else {
        // A keychain/pass/op ref is the operator's to provision.
        return Ok(false);
    };
    let path = Path::new(path);
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "gateway.setup_io",
                format!("create {}: {error}", parent.display()),
            )
        })?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            AikitError::new(
                "gateway.setup_io",
                format!("create {}: {error}", path.display()),
            )
        })?;
    file.write_all(generate_token()?.as_bytes())
        .map_err(|error| {
            AikitError::new(
                "gateway.setup_io",
                format!("write {}: {error}", path.display()),
            )
        })?;
    Ok(true)
}

/// What a step did when applied.
#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub step: SetupStep,
    pub outcome: String,
}

/// The effects of a setup, behind a seam the tests replace.
pub trait SetupEffects {
    fn ensure_token(&self, location: &str) -> Result<bool>;
    fn install_service(&self, step: &SetupStep) -> Result<Value>;
    fn remote_add(&self, workcell: &str, endpoint: &str, token_location: &str) -> Result<Value>;
    /// Run `tailscale serve --bg --tcp PORT tcp://127.0.0.1:PORT`.
    fn tailscale_serve(&self, port: u16) -> Result<String>;
    /// The node's serve status after the mapping (`(tailnet only)` or not).
    fn tailscale_serve_is_private(&self, port: u16) -> Result<bool>;
}

/// Apply a plan in order; the first refusal stops it, with what already ran
/// named. `apply_tailscale` is a separate consent from `--apply`.
pub fn apply_setup(
    plan: &SetupPlan,
    effects: &dyn SetupEffects,
    apply_tailscale: bool,
) -> Result<Vec<StepResult>> {
    let mut results = Vec::new();
    for step in &plan.steps {
        let outcome = match step {
            SetupStep::Token { path, .. } => {
                if effects.ensure_token(path)? {
                    format!("created {path} (owner-only)")
                } else {
                    format!("kept {path} (already there, or not a file location)")
                }
            }
            SetupStep::InstallService { .. } => {
                let installed = effects.install_service(step)?;
                format!(
                    "service definition written: {}",
                    installed["unit"].as_str().unwrap_or("?")
                )
            }
            SetupStep::RemoteAdd {
                workcell_ref,
                endpoint,
                token_location,
            } => {
                let added = effects.remote_add(workcell_ref, endpoint, token_location)?;
                let reachable = added["probe"]["reachable"].as_bool().unwrap_or(false);
                format!(
                    "declared {workcell_ref} at {endpoint} ({})",
                    if reachable {
                        "it answers"
                    } else {
                        "it does not answer now"
                    }
                )
            }
            SetupStep::TailscaleServe { port } => {
                if !apply_tailscale {
                    format!(
                        "NOT RUN: add --apply-tailscale to run `tailscale serve --bg --tcp {port} tcp://127.0.0.1:{port}` (private; never Funnel)"
                    )
                } else {
                    let ran = effects.tailscale_serve(*port)?;
                    if !effects.tailscale_serve_is_private(*port)? {
                        return Err(three_part(
                            "gateway.setup_serve_not_private",
                            format!("After mapping port {port}, `tailscale serve status` does not read `(tailnet only)`."),
                            format!("The mapping may be public. Steps already run: {}.", results.len()),
                            format!("Run `tailscale serve status` and `tailscale funnel status` now; remove the mapping with `tailscale serve --tcp={port} off` if it is not private."),
                        ));
                    }
                    format!("{ran}; verified tailnet only")
                }
            }
            SetupStep::OperatorCommand { command, why } => {
                format!("NOT RUN (yours to run): {command} — {why}")
            }
        };
        results.push(StepResult {
            step: step.clone(),
            outcome,
        });
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// The real effects
// ---------------------------------------------------------------------------

fn run_capture(program: &str, arguments: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub struct SystemSetupEffects {
    pub home: AikitHome,
    pub home_dir: PathBuf,
}

impl SetupEffects for SystemSetupEffects {
    #[cfg(unix)]
    fn ensure_token(&self, location: &str) -> Result<bool> {
        ensure_token(location)
    }

    #[cfg(not(unix))]
    fn ensure_token(&self, _location: &str) -> Result<bool> {
        Ok(false)
    }

    fn install_service(&self, step: &SetupStep) -> Result<Value> {
        let SetupStep::InstallService {
            websocket_bind,
            token_location,
            owner_token_location,
            gateway_ref,
            workcell_ref,
            allow_wide_bind,
            replace,
        } = step
        else {
            return Err(AikitError::new("gateway.setup_step", "not an install step"));
        };
        let options = crate::gateway_install::ServiceOptions {
            websocket_bind: websocket_bind.clone(),
            token_location: token_location.clone(),
            gateway_ref: gateway_ref.clone(),
            workcell_ref: workcell_ref.clone(),
            owner_token_location: owner_token_location.clone(),
            allow_wide_bind: *allow_wide_bind,
        };
        // Validate before anything is stopped: a bad option must not cost the
        // running gateway.
        options.validate()?;
        if *replace && crate::gateway_install::is_installed(&self.home_dir) {
            crate::gateway_install::uninstall(&self.home_dir)?;
            // The bootout sends SIGTERM and the gateway drains; the new
            // definition is written once nothing answers on the socket.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while std::time::Instant::now() < deadline {
                let answering = aikit_adapters::gateway_command_within(
                    &aikit_adapters::GatewayCarrierTarget::UnixSocket(self.home.gateway_socket()),
                    aikit_adapters::GatewayCommand::Protocol,
                    None,
                    std::time::Duration::from_secs(1),
                )
                .is_ok();
                if !answering {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
        crate::gateway_install::install(&self.home_dir, &self.home, &options)
    }

    fn remote_add(&self, workcell: &str, endpoint: &str, token_location: &str) -> Result<Value> {
        crate::gateway_contact::remote_add_probed(
            &self.home,
            workcell,
            endpoint,
            "/",
            token_location,
            true,
        )
    }

    fn tailscale_serve(&self, port: u16) -> Result<String> {
        let target = format!("tcp://127.0.0.1:{port}");
        let output = std::process::Command::new("tailscale")
            .args(["serve", "--bg", "--tcp", &port.to_string(), &target])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| {
                AikitError::new(
                    "gateway.setup_tailscale",
                    format!("could not run tailscale: {error}"),
                )
            })?;
        if !output.status.success() {
            return Err(three_part(
                "gateway.setup_tailscale_refused",
                format!("`tailscale serve` refused: {}", String::from_utf8_lossy(&output.stderr).trim()),
                "The gateway definition was written; no Serve mapping exists.",
                "Enable Serve for this node (the error names how), then re-run with --apply-tailscale.",
            ));
        }
        Ok(format!("tailscale serve --bg --tcp {port} {target}"))
    }

    fn tailscale_serve_is_private(&self, port: u16) -> Result<bool> {
        let status = run_capture("tailscale", &["serve", "status", "--json"]).unwrap_or_default();
        let parsed: Value = serde_json::from_str(&status).unwrap_or(Value::Null);
        // No `AllowFunnel` entry set for this port.
        let funnelled = parsed["AllowFunnel"].as_object().is_some_and(|allow| {
            allow.iter().any(|(host_port, on)| {
                on == &json!(true) && host_port.ends_with(&format!(":{port}"))
            })
        });
        let text = run_capture("tailscale", &["serve", "status"]).unwrap_or_default();
        Ok(!funnelled && !text.to_lowercase().contains("funnel on"))
    }
}

/// Everything `setup` reads from the machine to plan: the node's tailnet
/// address, the existing service, and what Serve/Funnel already map.
pub fn gather_inputs(home: &AikitHome, mode: &str) -> SetupInputs {
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let facts = crate::gateway_doctor::gather(home).ok();
    SetupInputs {
        mode: mode.to_owned(),
        tailnet_address: run_capture("tailscale", &["ip", "-4"])
            .and_then(|text| text.lines().next().map(|line| line.trim().to_owned()))
            .filter(|line| !line.is_empty()),
        service_installed: crate::gateway_install::is_installed(&home_dir),
        service_home: facts
            .as_ref()
            .and_then(|f| f.service.configured_home.clone()),
        serve_targets: facts
            .as_ref()
            .and_then(|f| f.tailscale.as_ref())
            .map(|t| t.serve_targets.clone())
            .unwrap_or_default(),
        funnel_ports: facts
            .as_ref()
            .and_then(|f| f.tailscale.as_ref())
            .map(|t| t.funnel_ports.clone())
            .unwrap_or_default(),
        aikit_home: home.root().to_path_buf(),
        home_dir,
        ..SetupInputs::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn inputs(mode: &str) -> SetupInputs {
        SetupInputs {
            mode: mode.into(),
            port: 7788,
            tailnet_address: Some("100.109.102.82".into()),
            aikit_home: PathBuf::from("/h/.aikit"),
            home_dir: PathBuf::from("/h"),
            gateway_ref: Some("agency-gateway/mac".into()),
            workcell_ref: Some("workcell:mac".into()),
            ..SetupInputs::default()
        }
    }

    fn kinds(plan: &SetupPlan) -> Vec<&'static str> {
        plan.steps
            .iter()
            .map(|step| match step {
                SetupStep::Token { .. } => "token",
                SetupStep::InstallService { .. } => "install",
                SetupStep::RemoteAdd { .. } => "remote",
                SetupStep::TailscaleServe { .. } => "serve",
                SetupStep::OperatorCommand { .. } => "operator",
            })
            .collect()
    }

    #[test]
    fn every_mode_the_docs_name_is_a_mode_the_command_knows_and_the_table_is_complete() {
        let ids: Vec<&str> = ENTRY_MODES.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            [
                "local-ipc",
                "loopback-service",
                "private-tailnet",
                "tailscale-serve",
                "ssh-tunnel",
                "remote-authenticated-endpoint",
                "tailscale-funnel"
            ]
        );
        let doc = include_str!("../../../docs/GATEWAY-OPERATING-MODES.md");
        for id in ids {
            assert!(
                doc.contains(id),
                "docs/GATEWAY-OPERATING-MODES.md must name `{id}`"
            );
        }
        for lifecycle in &LIFECYCLES {
            assert!(
                doc.contains(lifecycle.id.trim_start_matches("supervised-"))
                    || doc.contains(lifecycle.id)
            );
        }
    }

    #[test]
    fn private_tailnet_binds_the_tailnet_address_with_two_distinct_tokens() {
        let plan = plan_setup(&inputs("private-tailnet")).unwrap();
        assert_eq!(kinds(&plan), ["token", "token", "install"]);
        let SetupStep::InstallService {
            websocket_bind,
            token_location,
            owner_token_location,
            replace,
            ..
        } = &plan.steps[2]
        else {
            panic!()
        };
        assert_eq!(websocket_bind.as_deref(), Some("100.109.102.82:7788"));
        assert_ne!(
            token_location, owner_token_location,
            "one token cannot grant two scopes"
        );
        assert!(!replace);
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("application firewall")));
        // A second run over an installed service replaces the definition.
        let mut installed = inputs("private-tailnet");
        installed.service_installed = true;
        let again = plan_setup(&installed).unwrap();
        assert!(matches!(
            &again.steps[2],
            SetupStep::InstallService { replace: true, .. }
        ));
    }

    #[test]
    fn a_setup_from_another_home_refuses_instead_of_planning_to_replace_the_service() {
        // The service of /h/.aikit; this setup is run from a throwaway home.
        let mut other = inputs("tailscale-serve");
        other.service_installed = true;
        other.service_home = Some("/h/.aikit".into());
        other.aikit_home = PathBuf::from("/tmp/throwaway-home");
        let error = plan_setup(&other).unwrap_err();
        assert_eq!(error.code(), "gateway.setup_other_homes_service");
        // From the service's own home it is the ordinary replacement.
        other.aikit_home = PathBuf::from("/h/.aikit");
        assert!(plan_setup(&other).is_ok());
        // With no service installed there is nothing to replace.
        other.service_installed = false;
        other.aikit_home = PathBuf::from("/tmp/throwaway-home");
        assert!(plan_setup(&other).is_ok());
    }

    #[test]
    fn a_tailscale_serve_setup_keeps_the_listener_on_loopback_and_adds_one_private_mapping() {
        let plan = plan_setup(&inputs("tailscale-serve")).unwrap();
        assert_eq!(kinds(&plan), ["token", "token", "install", "serve"]);
        let SetupStep::InstallService { websocket_bind, .. } = &plan.steps[2] else {
            panic!()
        };
        assert_eq!(websocket_bind.as_deref(), Some("127.0.0.1:7788"));
        assert!(matches!(
            plan.steps[3],
            SetupStep::TailscaleServe { port: 7788 }
        ));
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("never") || w.contains("Nothing here runs")));
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("has not been exercised")));
    }

    #[test]
    fn a_port_that_already_has_a_serve_mapping_or_a_funnel_is_refused_not_overwritten() {
        let mut mapped = inputs("tailscale-serve");
        mapped.serve_targets = vec![("7788".into(), "http://127.0.0.1:18790".into())];
        let error = plan_setup(&mapped).unwrap_err();
        assert_eq!(error.code(), "gateway.setup_port_mapped");
        let mut funnelled = inputs("tailscale-serve");
        funnelled.funnel_ports = vec!["7788".into()];
        assert_eq!(
            plan_setup(&funnelled).unwrap_err().code(),
            "gateway.setup_port_funnelled"
        );
        // Another port is fine.
        let mut other = mapped.clone();
        other.port = 7790;
        assert!(plan_setup(&other).is_ok());
    }

    #[test]
    fn public_and_routable_modes_are_refused_with_the_reason_and_the_honest_alternative() {
        for mode in ["remote-authenticated-endpoint", "tailscale-funnel"] {
            let error = plan_setup(&inputs(mode)).unwrap_err();
            assert_eq!(error.code(), "gateway.setup_mode_refused", "{mode}");
        }
        let mut wide = inputs("private-tailnet");
        wide.bind = Some("0.0.0.0:7788".into());
        assert_eq!(
            plan_setup(&wide).unwrap_err().code(),
            "gateway.setup_bind_not_tailnet"
        );
        let mut lan = inputs("loopback-service");
        lan.bind = Some("192.168.4.90:7788".into());
        assert_eq!(
            plan_setup(&lan).unwrap_err().code(),
            "gateway.setup_bind_not_loopback"
        );
        assert_eq!(
            plan_setup(&inputs("nonsense")).unwrap_err().code(),
            "gateway.setup_mode_unknown"
        );
    }

    #[test]
    fn an_ssh_tunnel_plan_prints_the_forward_it_will_not_run_and_declares_the_peer_at_the_near_end()
    {
        let mut tunnel = inputs("ssh-tunnel");
        tunnel.peers = vec![("workcell:omarchy".into(), "127.0.0.1:17788".into())];
        let plan = plan_setup(&tunnel).unwrap();
        assert_eq!(
            kinds(&plan),
            ["token", "token", "install", "operator", "remote"]
        );
        let SetupStep::OperatorCommand { command, .. } = &plan.steps[3] else {
            panic!()
        };
        assert!(
            command.starts_with("ssh -N -L 127.0.0.1:17788:127.0.0.1:7788"),
            "{command}"
        );
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("does not survive a reboot")));
    }

    #[test]
    fn a_tailnet_address_that_cannot_be_read_is_asked_for_not_guessed() {
        let mut none = inputs("private-tailnet");
        none.tailnet_address = None;
        assert_eq!(
            plan_setup(&none).unwrap_err().code(),
            "gateway.setup_tailnet_address_unknown"
        );
    }

    #[test]
    fn endpoints_are_classified_by_what_could_reach_them_not_by_the_word_a_person_used() {
        assert_eq!(classify_endpoint("100.92.62.101:7788"), "private-tailnet");
        assert!(classify_endpoint("127.0.0.1:17788").starts_with("ssh-tunnel"));
        assert!(classify_endpoint("frank.tail7e55a2.ts.net:7788").starts_with("tailscale-serve"));
        assert!(classify_endpoint("203.0.113.9:7788").starts_with("remote-authenticated-endpoint"));
        assert!(classify_endpoint("192.168.4.90:7788").starts_with("lan"));
    }

    struct Recorded {
        log: RefCell<Vec<String>>,
        private: bool,
    }

    impl SetupEffects for Recorded {
        fn ensure_token(&self, location: &str) -> Result<bool> {
            self.log.borrow_mut().push(format!("token {location}"));
            Ok(true)
        }
        fn install_service(&self, _step: &SetupStep) -> Result<Value> {
            self.log.borrow_mut().push("install".into());
            Ok(json!({"unit": "/u/plist"}))
        }
        fn remote_add(&self, workcell: &str, _e: &str, _t: &str) -> Result<Value> {
            self.log.borrow_mut().push(format!("remote {workcell}"));
            Ok(json!({"probe": {"reachable": true}}))
        }
        fn tailscale_serve(&self, port: u16) -> Result<String> {
            self.log.borrow_mut().push(format!("serve {port}"));
            Ok(format!("tailscale serve --tcp {port}"))
        }
        fn tailscale_serve_is_private(&self, _port: u16) -> Result<bool> {
            Ok(self.private)
        }
    }

    #[test]
    fn apply_changes_aikit_and_leaves_tailscale_alone_until_asked_separately() {
        let plan = plan_setup(&inputs("tailscale-serve")).unwrap();
        let effects = Recorded {
            log: RefCell::new(vec![]),
            private: true,
        };
        let results = apply_setup(&plan, &effects, false).unwrap();
        assert!(results
            .last()
            .unwrap()
            .outcome
            .starts_with("NOT RUN: add --apply-tailscale"));
        assert!(!effects.log.borrow().iter().any(|l| l.starts_with("serve")));
        assert_eq!(
            effects
                .log
                .borrow()
                .iter()
                .filter(|l| l.starts_with("token"))
                .count(),
            2
        );
        // With the second consent it runs, and only after checking it reads private.
        let effects = Recorded {
            log: RefCell::new(vec![]),
            private: true,
        };
        let results = apply_setup(&plan, &effects, true).unwrap();
        assert!(results
            .last()
            .unwrap()
            .outcome
            .contains("verified tailnet only"));
        // A mapping that does not read private is an error naming how to remove it.
        let effects = Recorded {
            log: RefCell::new(vec![]),
            private: false,
        };
        let error = apply_setup(&plan, &effects, true).unwrap_err();
        assert_eq!(error.code(), "gateway.setup_serve_not_private");
        assert!(error.to_string().contains("off"));
    }

    #[cfg(unix)]
    #[test]
    fn a_token_file_is_created_owner_only_once_and_never_rotated() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials/gateway.token");
        let location = format!("file:{}", path.display());
        assert!(ensure_token(&location).unwrap());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let first = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first.len(), 64);
        assert!(
            !ensure_token(&location).unwrap(),
            "an existing token is kept"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        // A secret-manager ref is the operator's to provision.
        assert!(!ensure_token("keychain://aikit/gateway").unwrap());
    }
}
