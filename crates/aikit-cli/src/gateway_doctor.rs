//! `aikit gateway doctor` — what is actually true of this gateway, each finding
//! with the command that fixes it.
//!
//! Two halves, kept apart so the second is testable without a machine:
//!
//! * [`gather`] reads [`Facts`] from the running process, the service
//!   definition, the declared peers, the platform firewall and Tailscale —
//!   read-only, every probe bounded.
//! * [`diagnose`] is a pure function from facts to a [`Report`].
//!
//! What it will not do: change anything, run `sudo`, or configure Tailscale.
//! Where a fix needs the owner's authority (the macOS application firewall,
//! a public Funnel) the finding names the exact command and stops.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{
    gateway_command_within, CarrierScope, GatewayBuildIdentity, GatewayCarrierTarget,
    GatewayCommand, GatewayListenerReading, GatewayResponse, ImageMatch, ListenerClass,
    ListenerState,
};
use aikit_core::Result;
use aikit_store::home::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::gateway_upgrade::{Identity, Store};

pub const DOCTOR_SCHEMA: &str = "aikit.gateway-doctor/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Ok,
    Info,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub what: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// The command (or the owner action) that fixes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub verdict: Severity,
    pub findings: Vec<Finding>,
}

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

/// What a running gateway says about itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunningFacts {
    pub build: Option<GatewayBuildIdentity>,
    pub features: Vec<String>,
    pub gateway_ref: Option<String>,
    pub listeners: Vec<GatewayListenerReading>,
    pub connector_count: usize,
    pub binding_count: usize,
    pub pending_operations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceFacts {
    pub platform: Option<String>,
    pub installed: bool,
    pub definition_path: Option<String>,
    pub executable: Option<String>,
    pub executable_exists: bool,
    pub declared_lifecycle: Option<String>,
    pub websocket_bind: Option<String>,
    pub token_location: Option<String>,
    pub configured_gateway_ref: Option<String>,
    /// The `AIKIT_HOME` the installed service serves (from its definition).
    pub configured_home: Option<String>,
    /// `launchctl print` / `systemctl is-active`: whether the manager has it
    /// loaded and running. `None` when the manager could not be asked.
    pub manager_running: Option<bool>,
}

/// Free space on the volume that holds the gateway's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiskFacts {
    pub path: String,
    pub free_kib: u64,
}

/// Below this the gateway's whole-file state writes (and anything else on the
/// volume) start to fail halfway.
pub const DISK_FAIL_KIB: u64 = 512 * 1024;
/// Below this a managed install (which builds the suite) will not fit.
pub const DISK_WARN_KIB: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FirewallFacts {
    pub enabled: bool,
    /// Whether the running executable is on the firewall's allow list.
    pub running_listed: Option<bool>,
    /// Whether the installed executable is on it.
    pub installed_listed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TailscaleFacts {
    pub backend_running: bool,
    pub node_addresses: Vec<String>,
    /// Serve mappings that forward to a local port: `(tailnet port, target)`.
    pub serve_targets: Vec<(String, String)>,
    /// Funnelled ports (public internet) on this node.
    pub funnel_ports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpgradeFacts {
    pub in_flight: Option<(String, String)>,
    pub undelivered_receipts: Vec<String>,
    pub leftover_workers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateFacts {
    pub path: String,
    pub exists: bool,
    pub parses: Option<bool>,
    pub bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    /// The AIKit home these facts were gathered for.
    pub home: Option<String>,
    pub running: Option<RunningFacts>,
    pub installed: Option<Identity>,
    pub service: ServiceFacts,
    /// Token files that exist but are readable beyond their owner.
    pub token_problems: Vec<String>,
    pub remotes: Vec<Value>,
    pub firewall: Option<FirewallFacts>,
    pub tailscale: Option<TailscaleFacts>,
    pub upgrade: UpgradeFacts,
    pub state: StateFacts,
    pub disk: Option<DiskFacts>,
    pub foreign_gateways: Vec<String>,
}

// ---------------------------------------------------------------------------
// Diagnosis (pure)
// ---------------------------------------------------------------------------

fn finding(
    id: &str,
    severity: Severity,
    what: impl Into<String>,
    evidence: Vec<String>,
    remedy: Option<&str>,
) -> Finding {
    Finding {
        id: id.to_owned(),
        severity,
        what: what.into(),
        evidence,
        remedy: remedy.map(str::to_owned),
    }
}

/// Whether the installed service's definition names a different `AIKIT_HOME` than
/// the one these facts are about. Paths are compared as written and as resolved.
pub fn service_serves_another_home(facts: &Facts) -> bool {
    let (Some(ours), Some(theirs)) = (&facts.home, &facts.service.configured_home) else {
        return false;
    };
    let canonical = |path: &str| {
        std::fs::canonicalize(path)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.trim_end_matches('/').to_owned())
    };
    canonical(ours) != canonical(theirs)
}

/// The port of a `HOST:PORT` bind.
fn port_of(bind: &str) -> Option<&str> {
    bind.rsplit_once(':').map(|(_, port)| port)
}

pub fn diagnose(facts: &Facts) -> Report {
    let mut findings: Vec<Finding> = Vec::new();

    // -- service and process ------------------------------------------------
    match (&facts.running, facts.service.installed) {
        (None, false) => findings.push(finding(
            "gateway.not_running",
            Severity::Fail,
            "no gateway answers on this home's socket and no service is installed",
            vec![],
            Some("aikit gateway install-service   (or: aikit gateway serve --unix)"),
        )),
        // The installed service belongs to another AIKIT_HOME: it is not this
        // home's gateway, so "not answering on this socket" says nothing about it.
        (None, true) if service_serves_another_home(facts) => findings.push(finding(
            "service.serves_other_home",
            Severity::Warn,
            "no gateway answers on this home's socket, and the installed service serves a \
             different AIKIT_HOME: it is not this home's gateway",
            vec![
                format!("this home:    {}", facts.home.clone().unwrap_or_default()),
                format!(
                    "the service:  {}",
                    facts.service.configured_home.clone().unwrap_or_default()
                ),
            ],
            Some("run against the service's home (AIKIT_HOME=…), or name a separate instance with AIKIT_GATEWAY_SERVICE_INSTANCE=<name> before `install-service`"),
        )),
        (None, true) => findings.push(finding(
            "gateway.service_not_answering",
            Severity::Fail,
            "a gateway service is installed but nothing answers on its socket",
            vec![format!(
                "manager reports running: {:?}",
                facts.service.manager_running
            )],
            Some(
                "aikit gateway upgrade apply   (starts the installed build); read the service log",
            ),
        )),
        (Some(running), _) => {
            let detail = running
                .build
                .as_ref()
                .map(|b| {
                    format!(
                        "revision {} pid {} lifecycle {}",
                        b.revision,
                        b.pid,
                        b.lifecycle.as_str()
                    )
                })
                .unwrap_or_else(|| {
                    "it has published no posture record, so it cannot say which build it                      executes (it predates the posture record)".into()
                });
            findings.push(finding(
                "gateway.running",
                Severity::Ok,
                "a gateway answers on this home's socket",
                vec![detail],
                None,
            ));
        }
    }
    if facts.service.installed {
        if !facts.service.executable_exists {
            findings.push(finding(
                "service.executable_missing",
                Severity::Fail,
                "the service definition names an executable that does not exist",
                vec![format!("{:?}", facts.service.executable)],
                Some("aikit gateway install-service   (rewrites the definition with the current aikit)"),
            ));
        }
        if facts.service.declared_lifecycle.is_none() {
            findings.push(finding(
                "service.lifecycle_undeclared",
                Severity::Info,
                "the service definition does not declare its lifecycle (written before it was)",
                vec![],
                Some("aikit gateway install-service   (rewrites it with AIKIT_GATEWAY_LIFECYCLE)"),
            ));
        }
    }

    // -- the running process versus what is installed -----------------------
    if let (Some(running), Some(installed)) = (&facts.running, &facts.installed) {
        match &running.build {
            None => findings.push(finding(
                "gateway.stale_unknown",
                Severity::Warn,
                "the running gateway has published no posture record, so it cannot prove \
                 which build it executes against the installed one",
                vec![format!("installed: {}", installed.revision)],
                Some("aikit gateway upgrade apply"),
            )),
            Some(build) => {
                match build.image_match(
                    installed.executable_sha256.as_deref(),
                    Some(installed.revision.as_str()),
                ) {
                    ImageMatch::Same => findings.push(finding(
                        "gateway.current",
                        Severity::Ok,
                        "the running gateway is the installed build",
                        vec![format!("revision {}", build.revision)],
                        None,
                    )),
                    ImageMatch::Different => findings.push(finding(
                        "gateway.stale",
                        Severity::Fail,
                        "the running gateway is not the installed build: an update changed the \
                         file but the process still executes the old image",
                        vec![
                            format!("running:   {} (pid {})", build.revision, build.pid),
                            format!("installed: {}", installed.revision),
                        ],
                        Some("aikit gateway upgrade apply"),
                    )),
                    ImageMatch::Unknown => findings.push(finding(
                        "gateway.identity_pending",
                        Severity::Info,
                        "the running gateway's executable digest is not read yet, and a build \
                         with local edits cannot be told from the installed one by revision alone",
                        vec![
                            format!("running:   {} (pid {})", build.revision, build.pid),
                            format!("installed: {}", installed.revision),
                        ],
                        Some("aikit gateway doctor   (again, in a moment: the process reads its own digest after it starts)"),
                    )),
                }
            }
        }
    }

    // -- identity -----------------------------------------------------------
    if let (Some(running), Some(configured)) =
        (&facts.running, &facts.service.configured_gateway_ref)
    {
        if running
            .gateway_ref
            .as_deref()
            .is_some_and(|actual| actual != configured)
        {
            findings.push(finding(
                "gateway.identity_drift",
                Severity::Warn,
                "the running gateway answers under a different ref than its service names",
                vec![
                    format!("service: {configured}"),
                    format!("running: {}", running.gateway_ref.clone().unwrap_or_default()),
                ],
                Some("aikit gateway upgrade apply   (the restarted gateway honours the configured ref)"),
            ));
        }
    }
    if let Some(running) = &facts.running {
        if running.gateway_ref.as_deref() == Some("agency-gateway/local")
            && !facts.remotes.is_empty()
        {
            findings.push(finding(
                "gateway.identity_default",
                Severity::Warn,
                "this gateway relays to other Workcells under the first-run default name \
                 `agency-gateway/local`; two homes can carry it",
                vec![],
                Some("aikit gateway install-service --gateway-ref agency-gateway/<name> --workcell-ref workcell:<name>"),
            ));
        }
    }

    // -- listeners -----------------------------------------------------------
    if let Some(running) = &facts.running {
        for listener in &running.listeners {
            if listener.state == ListenerState::Waiting {
                findings.push(finding(
                    "listener.waiting",
                    Severity::Warn,
                    format!(
                        "the {} carrier at {} is not bound yet",
                        listener.carrier, listener.bind
                    ),
                    listener.detail.clone().into_iter().collect(),
                    Some("start Tailscale; the carrier binds itself when the address exists"),
                ));
            }
            if listener.carrier == "websocket" {
                match listener.class {
                    ListenerClass::Wildcard | ListenerClass::Public => findings.push(finding(
                        "listener.wide",
                        Severity::Fail,
                        format!(
                            "the WebSocket carrier is bound on {} ({}): reachable beyond the \
                             tailnet with only a bearer token and no TLS",
                            listener.bind,
                            listener.class.as_str()
                        ),
                        vec![],
                        Some("aikit gateway setup --mode private-tailnet   (or bind 127.0.0.1 behind tailscale serve)"),
                    )),
                    ListenerClass::PrivateNetwork => findings.push(finding(
                        "listener.lan",
                        Severity::Warn,
                        format!(
                            "the WebSocket carrier is bound on the local network ({}); the \
                             tailnet is the private path",
                            listener.bind
                        ),
                        vec![],
                        Some("aikit gateway setup --mode private-tailnet"),
                    )),
                    _ => findings.push(finding(
                        "listener.private",
                        Severity::Ok,
                        format!(
                            "the WebSocket carrier at {} is {} ({} scope)",
                            listener.bind,
                            listener.class.as_str(),
                            match listener.scope {
                                CarrierScope::Owner => "owner",
                                CarrierScope::Peer => "peer",
                            }
                        ),
                        vec![],
                        None,
                    )),
                }
            }
        }
    }

    // -- tokens ---------------------------------------------------------------
    for problem in &facts.token_problems {
        findings.push(finding(
            "token.permissions",
            Severity::Fail,
            problem.clone(),
            vec![],
            Some("chmod 600 <token file>; the gateway refuses a token other users can read"),
        ));
    }

    // -- peers ---------------------------------------------------------------
    let our_revision = facts
        .running
        .as_ref()
        .and_then(|r| r.build.as_ref())
        .map(|b| b.revision.clone());
    for remote in &facts.remotes {
        let workcell = remote["workcell_ref"].as_str().unwrap_or("?");
        if remote["reachable"] != json!(true) {
            findings.push(finding(
                "peer.unreachable",
                Severity::Warn,
                format!("declared peer {workcell} does not answer"),
                remote["detail"]
                    .as_str()
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                Some("aikit gateway remote list; check the peer's gateway and Tailscale"),
            ));
            continue;
        }
        if let Some(answers) = remote["answers_as_workcell"].as_str() {
            if answers != workcell {
                findings.push(finding(
                    "peer.identity_mismatch",
                    Severity::Fail,
                    format!(
                        "the gateway declared as {workcell} says it serves {answers}: claims \
                         and relays would be recorded against the wrong Workcell"
                    ),
                    vec![],
                    Some("aikit gateway remote remove … / remote add … with the right endpoint"),
                ));
            }
        }
        let missing: Vec<String> = remote["missing_features"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let their_revision = remote["revision"].as_str();
        if !missing.is_empty() {
            findings.push(finding(
                "peer.features_missing",
                Severity::Warn,
                format!(
                    "peer {workcell} (build {}) lacks features this gateway supports: {}",
                    their_revision.unwrap_or("unknown"),
                    missing.join(", ")
                ),
                vec![],
                Some("upgrade the peer: aikit gateway --at <workcell> upgrade plan (or run the upgrade on it)"),
            ));
        } else if their_revision.is_some() && their_revision != our_revision.as_deref() {
            findings.push(finding(
                "peer.revision_differs",
                Severity::Info,
                format!(
                    "peer {workcell} runs {} and this gateway runs {}; every feature this \
                     gateway advertises is present on the peer",
                    their_revision.unwrap_or("?"),
                    our_revision.clone().unwrap_or_default()
                ),
                vec![],
                None,
            ));
        } else if their_revision.is_some() {
            findings.push(finding(
                "peer.ok",
                Severity::Ok,
                format!("peer {workcell} answers and runs the same build"),
                vec![],
                None,
            ));
        }
    }

    // -- firewall -------------------------------------------------------------
    let exposes_network = facts.running.as_ref().is_some_and(|r| {
        r.listeners
            .iter()
            .any(|l| l.carrier == "websocket" && l.class.is_network_reachable())
    });
    if let Some(firewall) = &facts.firewall {
        if firewall.enabled && exposes_network {
            match (firewall.running_listed, firewall.installed_listed) {
                (Some(true), Some(false)) => findings.push(finding(
                    "firewall.installed_not_allowed",
                    Severity::Info,
                    "the macOS application firewall's list names the running gateway binary but \
                     not the installed one. macOS also admits signed binaries it never lists \
                     (it did for the last upgrades), so this may be nothing: after the restart, \
                     ask a peer — `aikit gateway --at workcell:<this> protocol` — and only if \
                     that queues or times out does the owner need to allow the new binary",
                    vec![],
                    Some(&firewall_remedy(
                        facts
                            .installed
                            .as_ref()
                            .and_then(|installed| installed.executable_path.as_deref()),
                        "<installed aikit>",
                        true,
                    )),
                )),
                (Some(false), _) => findings.push(finding(
                    "firewall.running_not_allowed",
                    Severity::Info,
                    "the macOS application firewall's list does not name the running gateway \
                     binary. macOS also admits signed binaries it never lists, and the list \
                     cannot say which: `socketfilterfw --getappblocked` answers \"permitted\" for \
                     any path, listed or not. The test is a peer asking this gateway — \
                     `aikit gateway --at workcell:<this> protocol` from another Workcell; only \
                     if that queues or times out does the owner need to allow the binary",
                    vec![],
                    Some(&firewall_remedy(
                        facts
                            .running
                            .as_ref()
                            .and_then(|running| running.build.as_ref())
                            .and_then(|build| build.executable_path.as_deref()),
                        "<running aikit>",
                        false,
                    )),
                )),
                _ => {}
            }
        }
    }

    // -- tailscale ------------------------------------------------------------
    if let Some(tailscale) = &facts.tailscale {
        if !tailscale.backend_running && exposes_network {
            findings.push(finding(
                "tailscale.down",
                Severity::Warn,
                "Tailscale is not running: a tailnet carrier cannot be reached (and cannot bind \
                 until the address exists)",
                vec![],
                Some("start Tailscale"),
            ));
        }
        let gateway_ports: Vec<String> = facts
            .running
            .as_ref()
            .map(|r| {
                r.listeners
                    .iter()
                    .filter_map(|l| port_of(&l.bind).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        for port in &tailscale.funnel_ports {
            let funnelled_target = tailscale
                .serve_targets
                .iter()
                .find(|(p, _)| p == port)
                .map(|(_, target)| target.clone());
            let points_at_gateway = funnelled_target
                .as_deref()
                .and_then(port_of)
                .is_some_and(|target_port| gateway_ports.iter().any(|p| p == target_port));
            if points_at_gateway {
                findings.push(finding(
                    "tailscale.funnel_exposes_gateway",
                    Severity::Fail,
                    format!(
                        "Tailscale Funnel publishes port {port} to the internet and it forwards \
                         to this gateway: anyone can reach it with only a bearer token"
                    ),
                    funnelled_target.into_iter().collect(),
                    Some("tailscale funnel --https=<port> off   (owner action; aikit never configures Funnel)"),
                ));
            } else {
                findings.push(finding(
                    "tailscale.funnel_on",
                    Severity::Info,
                    format!("Tailscale Funnel is on for port {port}; it does not forward to this gateway"),
                    vec![],
                    None,
                ));
            }
        }
        for (port, target) in &tailscale.serve_targets {
            if let Some(target_port) = port_of(target) {
                if gateway_ports.iter().any(|p| p == target_port)
                    && !tailscale.funnel_ports.contains(port)
                {
                    findings.push(finding(
                        "tailscale.serve_front",
                        Severity::Ok,
                        format!(
                            "a private Tailscale Serve mapping on {port} forwards to this \
                             gateway ({target}); tailnet only"
                        ),
                        vec![],
                        None,
                    ));
                }
            }
        }
    }

    // -- disk ------------------------------------------------------------------
    if let Some(disk) = &facts.disk {
        let mib = disk.free_kib / 1024;
        if disk.free_kib < DISK_FAIL_KIB {
            findings.push(finding(
                "disk.low",
                Severity::Fail,
                format!(
                    "{mib} MiB are free where the gateway keeps its state: it rewrites its whole \
                     state file on every command, and on a full disk that write can tear"
                ),
                vec![format!("volume of {}", disk.path)],
                Some("free space on that volume (build caches of retired work are the usual cause); `aikit gateway recover` repairs a state file that did tear"),
            ));
        } else if disk.free_kib < DISK_WARN_KIB {
            findings.push(finding(
                "disk.low",
                Severity::Warn,
                format!(
                    "{mib} MiB are free where the gateway keeps its state: a managed install \
                     (`upgrade apply --install`) builds the suite and needs at least {} MiB",
                    crate::gateway_upgrade_system::install_min_free_kib() / 1024
                ),
                vec![format!("volume of {}", disk.path)],
                Some("free space on that volume before installing; restarting onto an already-installed build (`aikit gateway upgrade apply`) needs none"),
            ));
        }
    }

    // -- state ----------------------------------------------------------------
    if facts.state.exists && facts.state.parses == Some(false) {
        findings.push(finding(
            "state.damaged",
            Severity::Fail,
            "the gateway state file does not parse: the service will not start from it",
            facts.state.error.clone().into_iter().collect(),
            Some("aikit gateway recover   (quarantines it and restores the last upgrade's copy)"),
        ));
    }
    if let Some(running) = &facts.running {
        if running.pending_operations > 0 {
            findings.push(finding(
                "delivery.pending_operations",
                Severity::Warn,
                format!(
                    "{} outbound operation(s) are prepared and unreceipted; a restart retains \
                     them and never blindly re-sends",
                    running.pending_operations
                ),
                vec![],
                None,
            ));
        }
    }

    // -- upgrade --------------------------------------------------------------
    if let Some((id, phase)) = &facts.upgrade.in_flight {
        findings.push(finding(
            "upgrade.in_flight",
            Severity::Info,
            format!("upgrade {id} is in flight ({phase})"),
            vec![],
            Some("aikit gateway upgrade status"),
        ));
    }
    for id in &facts.upgrade.undelivered_receipts {
        findings.push(finding(
            "upgrade.receipt_undelivered",
            Severity::Warn,
            format!(
                "upgrade {id} finished but its receipt has not reached the conversation that asked"
            ),
            vec![],
            Some("aikit gateway upgrade resume <id>"),
        ));
    }
    for worker in &facts.upgrade.leftover_workers {
        findings.push(finding(
            "upgrade.leftover_worker",
            Severity::Info,
            format!("a finished upgrade worker definition remains: {worker}"),
            vec![],
            Some("remove the file; it is one-shot and harmless"),
        ));
    }

    // -- neighbours -----------------------------------------------------------
    for foreign in &facts.foreign_gateways {
        findings.push(finding(
            "coexistence.foreign_gateway",
            Severity::Info,
            format!("another harness gateway runs on this machine: {foreign}"),
            vec![],
            Some(
                "aikit gateway coexistence   (policy and ownership of shared connector identities)",
            ),
        ));
    }

    let verdict = findings
        .iter()
        .map(|f| f.severity)
        .max()
        .unwrap_or(Severity::Ok);
    findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
    Report {
        schema: DOCTOR_SCHEMA.into(),
        verdict,
        findings,
    }
}

// ---------------------------------------------------------------------------
// Gathering (system)
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

fn file_mode_problem(label: &str, location: &str) -> Option<String> {
    let path = location.strip_prefix("file:")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path).ok()?.permissions().mode();
        if mode & 0o077 != 0 {
            return Some(format!(
                "{label} token {path} is readable beyond its owner (mode {:o})",
                mode & 0o777
            ));
        }
    }
    let _ = (label, path);
    None
}

/// Arguments of a service definition's `serve` command, read back from the
/// definition text: `--ws`, `--ws-token-location`.
fn definition_argument(definition: &str, flag: &str) -> Option<String> {
    if definition.contains("<string>") {
        // launchd lists each argument as its own <string>.
        let mut previous_was_flag = false;
        for piece in definition.split("<string>").skip(1) {
            let value = piece.split("</string>").next().unwrap_or("").trim();
            if previous_was_flag {
                return Some(value.to_owned());
            }
            previous_was_flag = value == flag;
        }
        return None;
    }
    // systemd: one ExecStart line of words.
    let mut words = definition.split_whitespace();
    while let Some(word) = words.next() {
        if word == flag {
            return words.next().map(|w| w.trim_matches('"').to_owned());
        }
    }
    None
}

fn environment_value(definition: &str, name: &str) -> Option<String> {
    // systemd: Environment="NAME=value"; launchd: <key>NAME</key><string>value</string>
    for line in definition.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Environment=") {
            let rest = rest.trim_matches('"');
            if let Some(value) = rest.strip_prefix(&format!("{name}=")) {
                return Some(value.to_owned());
            }
        }
    }
    let key = format!("<key>{name}</key>");
    let after = definition.split(&key).nth(1)?;
    after
        .split("<string>")
        .nth(1)?
        .split("</string>")
        .next()
        .map(|v| v.trim().to_owned())
}

pub fn gather(home: &AikitHome) -> Result<Facts> {
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut facts = Facts::default();

    // The running process.
    let target = GatewayCarrierTarget::UnixSocket(home.gateway_socket());
    let asked = |command: GatewayCommand| {
        gateway_command_within(&target, command, None, Duration::from_secs(3)).ok()
    };
    if let Some(GatewayResponse::Protocol { features, .. }) = asked(GatewayCommand::Protocol) {
        // The running identity comes from the process's own posture record
        // (the wire protocol carries no build identity); the record's
        // listeners are the carriers the process was configured to serve.
        let posture = crate::gateway_ops::read_process_record(home);
        let mut running = RunningFacts {
            build: posture.as_ref().map(|posture| posture.build.clone()),
            listeners: posture
                .map(|posture| posture.listeners)
                .unwrap_or_default(),
            features,
            ..RunningFacts::default()
        };
        if let Some(GatewayResponse::Status { status }) = asked(GatewayCommand::Status) {
            running.gateway_ref = Some(status.gateway_ref.to_string());
            running.connector_count = status.connector_count;
            running.binding_count = status.binding_count;
            running.pending_operations = status.pending_delivery_count;
        }
        facts.running = Some(running);
    }

    // The service definition.
    if let Ok(platform) = crate::gateway_install::ServicePlatform::current() {
        let path = platform.unit_path(&home_dir);
        facts.service.platform = Some(platform.as_str().to_owned());
        if let Ok(definition) = std::fs::read_to_string(&path) {
            facts.service.installed = true;
            facts.service.definition_path = Some(path.display().to_string());
            let executable =
                crate::gateway_upgrade_system::executable_named_by(&definition, platform);
            facts.service.executable_exists = executable.as_ref().is_some_and(|e| e.exists());
            facts.service.executable = executable.map(|e| e.display().to_string());
            facts.service.declared_lifecycle =
                environment_value(&definition, "AIKIT_GATEWAY_LIFECYCLE");
            facts.service.configured_gateway_ref =
                environment_value(&definition, "AIKIT_GATEWAY_REF");
            facts.service.configured_home = environment_value(&definition, "AIKIT_HOME");
            facts.service.websocket_bind = definition_argument(&definition, "--ws");
            facts.service.token_location = definition_argument(&definition, "--ws-token-location");
            facts.service.manager_running = match platform {
                crate::gateway_install::ServicePlatform::LaunchAgent => run_capture(
                    "launchctl",
                    &[
                        "print",
                        &format!(
                            "gui/{}/{}",
                            run_capture("id", &["-u"])
                                .unwrap_or_else(|| "501".into())
                                .trim(),
                            crate::gateway_install::service_label()
                        ),
                    ],
                )
                .map(|text| text.contains("state = running")),
                crate::gateway_install::ServicePlatform::SystemdUser => run_capture(
                    "systemctl",
                    &[
                        "--user",
                        "is-active",
                        &crate::gateway_install::systemd_unit_name(),
                    ],
                )
                .map(|text| text.trim() == "active"),
            };
        }
        facts.installed = facts
            .service
            .executable
            .as_deref()
            .and_then(|path| crate::gateway_upgrade_system::identify_executable(Path::new(path)))
            .or_else(|| {
                crate::probe::which("aikit")
                    .and_then(|path| crate::gateway_upgrade_system::identify_executable(&path))
            });
    }

    // Tokens.
    for (label, location) in [("peer", facts.service.token_location.clone())] {
        if let Some(problem) = location
            .as_deref()
            .and_then(|location| file_mode_problem(label, location))
        {
            facts.token_problems.push(problem);
        }
    }

    // Peers.
    facts.remotes = crate::gateway_upgrade_system::declared_remote_readings(home);

    // The state file.
    let state = home.gateway_state();
    facts.disk = crate::gateway_upgrade_system::free_kib(&home.state()).map(|free_kib| DiskFacts {
        path: home.state().display().to_string(),
        free_kib,
    });
    facts.home = Some(home.root().display().to_string());
    facts.state.path = state.display().to_string();
    if let Ok(bytes) = std::fs::read(&state) {
        facts.state.exists = true;
        facts.state.bytes = bytes.len() as u64;
        match serde_json::from_slice::<aikit_adapters::GatewaySnapshot>(&bytes) {
            Ok(_) => facts.state.parses = Some(true),
            Err(error) => {
                facts.state.parses = Some(false);
                facts.state.error = Some(error.to_string());
            }
        }
    }

    // macOS application firewall (read-only).
    #[cfg(target_os = "macos")]
    {
        let socketfilterfw = "/usr/libexec/ApplicationFirewall/socketfilterfw";
        if Path::new(socketfilterfw).exists() {
            let enabled = run_capture(socketfilterfw, &["--getglobalstate"])
                .is_some_and(|text| text.contains("enabled"));
            let listed = run_capture(socketfilterfw, &["--listapps"]).unwrap_or_default();
            let is_listed = |path: Option<&str>| {
                path.map(|path| listed.lines().any(|line| line.contains(path)))
            };
            let running_exe = facts
                .running
                .as_ref()
                .and_then(|r| r.build.as_ref())
                .and_then(|b| b.executable_path.clone());
            facts.firewall = Some(FirewallFacts {
                enabled,
                running_listed: is_listed(running_exe.as_deref()),
                installed_listed: is_listed(
                    facts
                        .installed
                        .as_ref()
                        .and_then(|i| i.executable_path.as_deref()),
                ),
            });
        }
    }

    // Tailscale (read-only).
    if let Some(text) = run_capture("tailscale", &["status", "--json"]) {
        if let Ok(status) = serde_json::from_str::<Value>(&text) {
            let mut tailscale = TailscaleFacts {
                backend_running: status["BackendState"] == "Running",
                node_addresses: status["Self"]["TailscaleIPs"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                ..TailscaleFacts::default()
            };
            if let Some(text) = run_capture("tailscale", &["serve", "status", "--json"]) {
                if let Ok(serve) = serde_json::from_str::<Value>(&text) {
                    if let Some(web) = serve["Web"].as_object() {
                        for (host_port, config) in web {
                            if let Some(handlers) = config["Handlers"].as_object() {
                                for handler in handlers.values() {
                                    if let Some(proxy) = handler["Proxy"].as_str() {
                                        let port =
                                            port_of(host_port).unwrap_or(host_port).to_owned();
                                        tailscale.serve_targets.push((port, proxy.to_owned()));
                                    }
                                }
                            }
                        }
                    }
                    if let Some(tcp) = serve["TCP"].as_object() {
                        for (port, config) in tcp {
                            if let Some(forward) = config["TCPForward"].as_str() {
                                tailscale
                                    .serve_targets
                                    .push((port.clone(), forward.to_owned()));
                            }
                        }
                    }
                    if let Some(allow) = serve["AllowFunnel"].as_object() {
                        for (host_port, on) in allow {
                            if on == &json!(true) {
                                tailscale
                                    .funnel_ports
                                    .push(port_of(host_port).unwrap_or(host_port).to_owned());
                            }
                        }
                    }
                }
            }
            facts.tailscale = Some(tailscale);
        }
    }

    // Upgrades.
    let store = Store::new(&home.state());
    facts.upgrade.in_flight = store
        .in_flight()
        .map(|t| (t.id, format!("{:?}", t.phase).to_lowercase()));
    facts.upgrade.undelivered_receipts = store
        .list()
        .into_iter()
        .filter(|t| t.phase.is_terminal() && !t.receipt_delivered && t.origin.is_some())
        .map(|t| t.id)
        .collect();
    if let Ok(entries) = std::fs::read_dir(home_dir.join("Library/LaunchAgents")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("ai.aikit.gateway-upgrade.") {
                facts
                    .upgrade
                    .leftover_workers
                    .push(entry.path().display().to_string());
            }
        }
    }

    // Neighbours.
    facts.foreign_gateways = crate::gateway_ops::foreign_gateway_lines();
    Ok(facts)
}

/// Run the doctor against this machine.
pub fn run(home: &AikitHome) -> Result<Value> {
    let facts = gather(home)?;
    let report = diagnose(&facts);
    Ok(serde_json::to_value(&report).unwrap_or(Value::Null))
}

/// The owner's command for the application firewall, with the real path (quoted:
/// managed paths contain spaces). aikit never runs it: it needs `sudo`.
fn firewall_remedy(path: Option<&str>, placeholder: &str, also_unblock: bool) -> String {
    let quoted = path
        .map(|path| format!("'{}'", path.replace('\'', "'\\''")))
        .unwrap_or_else(|| placeholder.to_owned());
    let socketfilter = "sudo /usr/libexec/ApplicationFirewall/socketfilterfw";
    let mut command =
        format!("owner action (needs sudo; not run by aikit): {socketfilter} --add {quoted}");
    if also_unblock {
        command.push_str(&format!(" && {socketfilter} --unblockapp {quoted}"));
        command.push_str(
            " — or bind 127.0.0.1 behind `tailscale serve` so the allowance belongs to Tailscale",
        );
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::GatewayLifecycle;

    fn build(revision: &str, sha: &str) -> GatewayBuildIdentity {
        GatewayBuildIdentity {
            revision: revision.into(),
            dirty: false,
            pid: 42,
            started_at_unix_ms: 1,
            executable_path: Some("/managed/aikit".into()),
            executable_sha256: Some(sha.into()),
            workcell_ref: Some("workcell:mac".into()),
            lifecycle: GatewayLifecycle::SupervisedLaunchd,
        }
    }

    fn listener(bind: &str, class: ListenerClass) -> GatewayListenerReading {
        GatewayListenerReading {
            carrier: "websocket".into(),
            bind: bind.into(),
            class,
            scope: CarrierScope::Peer,
            state: ListenerState::Bound,
            detail: None,
        }
    }

    fn healthy() -> Facts {
        Facts {
            running: Some(RunningFacts {
                build: Some(build("aaaa", &"a".repeat(64))),
                gateway_ref: Some("agency-gateway/mac".into()),
                listeners: vec![listener("100.109.102.82:7788", ListenerClass::Tailnet)],
                ..RunningFacts::default()
            }),
            installed: Some(Identity {
                revision: "aaaa".into(),
                executable_sha256: Some("a".repeat(64)),
                executable_path: Some("/managed/aikit".into()),
            }),
            service: ServiceFacts {
                installed: true,
                executable_exists: true,
                declared_lifecycle: Some("supervised-launchd".into()),
                configured_gateway_ref: Some("agency-gateway/mac".into()),
                manager_running: Some(true),
                ..ServiceFacts::default()
            },
            ..Facts::default()
        }
    }

    fn ids(report: &Report) -> Vec<&str> {
        report.findings.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn a_healthy_gateway_is_ok_and_says_what_it_checked() {
        let report = diagnose(&healthy());
        assert_eq!(report.verdict, Severity::Ok, "{:#?}", report.findings);
        assert!(ids(&report).contains(&"gateway.current"));
        assert!(ids(&report).contains(&"listener.private"));
    }

    #[test]
    fn a_resident_running_an_older_image_than_is_installed_is_a_failure_with_the_fix() {
        let mut facts = healthy();
        facts.installed.as_mut().unwrap().executable_sha256 = Some("b".repeat(64));
        facts.installed.as_mut().unwrap().revision = "bbbb".into();
        let report = diagnose(&facts);
        assert_eq!(report.verdict, Severity::Fail);
        let stale = report
            .findings
            .iter()
            .find(|f| f.id == "gateway.stale")
            .unwrap();
        assert!(stale.remedy.as_deref().unwrap().contains("upgrade apply"));
        assert!(
            stale.evidence.iter().any(|e| e.contains("aaaa"))
                && stale.evidence.iter().any(|e| e.contains("bbbb"))
        );
    }

    #[test]
    fn a_dirty_build_whose_digest_is_not_read_yet_is_pending_not_stale_and_not_current() {
        let mut facts = healthy();
        let build = facts.running.as_mut().unwrap().build.as_mut().unwrap();
        build.dirty = true;
        build.executable_sha256 = None;
        let report = diagnose(&facts);
        assert!(
            ids(&report).contains(&"gateway.identity_pending"),
            "{:#?}",
            report.findings
        );
        assert!(!ids(&report).contains(&"gateway.stale"));
        assert!(!ids(&report).contains(&"gateway.current"));
        assert_ne!(
            report.verdict,
            Severity::Fail,
            "a build that is only unproven is not a failure"
        );
        // A different revision is stale whatever the digest says.
        facts.installed.as_mut().unwrap().revision = "bbbb".into();
        facts.installed.as_mut().unwrap().executable_sha256 = Some("b".repeat(64));
        assert!(ids(&diagnose(&facts)).contains(&"gateway.stale"));
    }

    #[test]
    fn a_nearly_full_disk_is_a_failure_and_a_tight_one_a_warning_with_the_install_floor() {
        let mut facts = healthy();
        facts.disk = Some(DiskFacts {
            path: "/home/state".into(),
            free_kib: 100 * 1024,
        });
        let report = diagnose(&facts);
        let finding = report.findings.iter().find(|f| f.id == "disk.low").unwrap();
        assert_eq!(finding.severity, Severity::Fail);
        assert!(finding.what.contains("100 MiB"), "{}", finding.what);
        assert_eq!(report.verdict, Severity::Fail);
        facts.disk = Some(DiskFacts {
            path: "/home/state".into(),
            free_kib: 2 * 1024 * 1024,
        });
        let report = diagnose(&facts);
        let finding = report.findings.iter().find(|f| f.id == "disk.low").unwrap();
        assert_eq!(finding.severity, Severity::Warn);
        assert!(
            finding.what.contains("3072 MiB"),
            "the install floor is named: {}",
            finding.what
        );
        facts.disk = Some(DiskFacts {
            path: "/home/state".into(),
            free_kib: 50 * 1024 * 1024,
        });
        assert!(!ids(&diagnose(&facts)).contains(&"disk.low"));
    }

    #[test]
    fn a_service_that_serves_another_home_is_named_not_called_not_answering() {
        let mut facts = healthy();
        facts.running = None;
        facts.home = Some("/tmp/throwaway-home".into());
        facts.service.configured_home = Some("/h/.aikit".into());
        let report = diagnose(&facts);
        assert!(
            ids(&report).contains(&"service.serves_other_home"),
            "{:#?}",
            report.findings
        );
        assert!(!ids(&report).contains(&"gateway.service_not_answering"));
        // The same home: the ordinary finding.
        facts.home = Some("/h/.aikit".into());
        let report = diagnose(&facts);
        assert!(ids(&report).contains(&"gateway.service_not_answering"));
        assert!(!ids(&report).contains(&"service.serves_other_home"));
    }

    #[test]
    fn a_gateway_that_cannot_report_its_build_is_called_stale_not_current() {
        let mut facts = healthy();
        facts.running.as_mut().unwrap().build = None;
        let report = diagnose(&facts);
        assert!(ids(&report).contains(&"gateway.stale_unknown"));
        assert!(!ids(&report).contains(&"gateway.current"));
    }

    #[test]
    fn a_configured_ref_that_the_running_gateway_does_not_answer_to_is_identity_drift() {
        let mut facts = healthy();
        facts.running.as_mut().unwrap().gateway_ref = Some("agency-gateway/local".into());
        facts.remotes = vec![
            json!({"workcell_ref":"workcell:omarchy","reachable":true,"revision":"aaaa","missing_features":[]}),
        ];
        let report = diagnose(&facts);
        assert!(ids(&report).contains(&"gateway.identity_drift"));
        assert!(ids(&report).contains(&"gateway.identity_default"));
    }

    #[test]
    fn a_wide_bind_is_a_failure_and_a_lan_bind_a_warning() {
        let mut facts = healthy();
        facts.running.as_mut().unwrap().listeners =
            vec![listener("0.0.0.0:7788", ListenerClass::Wildcard)];
        assert!(ids(&diagnose(&facts)).contains(&"listener.wide"));
        facts.running.as_mut().unwrap().listeners =
            vec![listener("192.168.4.90:7788", ListenerClass::PrivateNetwork)];
        let report = diagnose(&facts);
        assert_eq!(
            report
                .findings
                .iter()
                .find(|f| f.id == "listener.lan")
                .unwrap()
                .severity,
            Severity::Warn
        );
    }

    #[test]
    fn a_waiting_tailnet_carrier_is_named_with_why_and_what_starts_it() {
        let mut facts = healthy();
        let mut waiting = listener("100.109.102.82:7788", ListenerClass::Tailnet);
        waiting.state = ListenerState::Waiting;
        waiting.detail = Some("not an address of this machine yet".into());
        facts.running.as_mut().unwrap().listeners = vec![waiting];
        let report = diagnose(&facts);
        let finding = report
            .findings
            .iter()
            .find(|f| f.id == "listener.waiting")
            .unwrap();
        assert!(finding.remedy.as_deref().unwrap().contains("Tailscale"));
    }

    #[test]
    fn peers_are_checked_for_reachability_declared_identity_and_missing_features() {
        let mut facts = healthy();
        facts.remotes = vec![
            json!({"workcell_ref":"workcell:down","reachable":false,"detail":"connection refused"}),
            json!({"workcell_ref":"workcell:omarchy","reachable":true,"answers_as_workcell":"workcell:elsewhere","revision":"aaaa","missing_features":[]}),
            json!({"workcell_ref":"workcell:old","reachable":true,"revision":"eeaab031","missing_features":["gateway-drain","gateway-build-identity"]}),
            json!({"workcell_ref":"workcell:same","reachable":true,"revision":"aaaa","missing_features":[]}),
        ];
        let report = diagnose(&facts);
        let got = ids(&report);
        assert!(got.contains(&"peer.unreachable"));
        assert!(got.contains(&"peer.identity_mismatch"));
        assert!(got.contains(&"peer.features_missing"));
        assert!(got.contains(&"peer.ok"));
        let missing = report
            .findings
            .iter()
            .find(|f| f.id == "peer.features_missing")
            .unwrap();
        assert!(missing.what.contains("gateway-drain") && missing.what.contains("eeaab031"));
    }

    #[test]
    fn the_firewall_finding_names_the_owner_command_and_the_loopback_alternative_and_runs_nothing()
    {
        let mut facts = healthy();
        facts.firewall = Some(FirewallFacts {
            enabled: true,
            running_listed: Some(true),
            installed_listed: Some(false),
        });
        let report = diagnose(&facts);
        let finding = report
            .findings
            .iter()
            .find(|f| f.id == "firewall.installed_not_allowed")
            .unwrap();
        let remedy = finding.remedy.as_deref().unwrap();
        assert!(remedy.contains("sudo"));
        assert!(remedy.contains("tailscale serve"));
        // The real path, quoted (managed paths contain spaces), not a placeholder.
        assert!(remedy.contains("--add '/managed/aikit'"), "{remedy}");
        assert!(!remedy.contains("<installed aikit>"));
        // A loopback-only gateway exposes nothing to the firewall.
        facts.running.as_mut().unwrap().listeners =
            vec![listener("127.0.0.1:7788", ListenerClass::Loopback)];
        assert!(!ids(&diagnose(&facts)).contains(&"firewall.installed_not_allowed"));
    }

    #[test]
    fn a_funnel_that_forwards_to_the_gateway_is_a_failure_and_a_private_serve_front_is_ok() {
        let mut facts = healthy();
        facts.running.as_mut().unwrap().listeners =
            vec![listener("127.0.0.1:7788", ListenerClass::Loopback)];
        facts.tailscale = Some(TailscaleFacts {
            backend_running: true,
            node_addresses: vec!["100.109.102.82".into()],
            serve_targets: vec![("7788".into(), "tcp://127.0.0.1:7788".into())],
            funnel_ports: vec![],
        });
        let report = diagnose(&facts);
        assert!(ids(&report).contains(&"tailscale.serve_front"));
        assert!(!ids(&report).contains(&"tailscale.funnel_exposes_gateway"));
        facts.tailscale.as_mut().unwrap().funnel_ports = vec!["7788".into()];
        let report = diagnose(&facts);
        assert_eq!(report.verdict, Severity::Fail);
        assert!(ids(&report).contains(&"tailscale.funnel_exposes_gateway"));
        // A funnel on an unrelated port is noted, not alarmed.
        facts.tailscale.as_mut().unwrap().serve_targets =
            vec![("443".into(), "http://127.0.0.1:18790".into())];
        facts.tailscale.as_mut().unwrap().funnel_ports = vec!["443".into()];
        let report = diagnose(&facts);
        assert!(ids(&report).contains(&"tailscale.funnel_on"));
        assert!(!ids(&report).contains(&"tailscale.funnel_exposes_gateway"));
    }

    #[test]
    fn a_damaged_state_file_a_missing_service_executable_and_an_undelivered_receipt_are_each_found()
    {
        let mut facts = healthy();
        facts.state = StateFacts {
            path: "/x/gateway.json".into(),
            exists: true,
            parses: Some(false),
            bytes: 10,
            error: Some("expected value".into()),
        };
        facts.service.executable_exists = false;
        facts.upgrade.undelivered_receipts = vec!["upg-1".into()];
        let report = diagnose(&facts);
        let got = ids(&report);
        assert!(got.contains(&"state.damaged"));
        assert!(got.contains(&"service.executable_missing"));
        assert!(got.contains(&"upgrade.receipt_undelivered"));
        assert!(
            report
                .findings
                .windows(2)
                .all(|w| w[0].severity >= w[1].severity),
            "worst first"
        );
    }

    #[test]
    fn a_missing_gateway_is_a_failure_whether_or_not_a_service_is_installed() {
        let mut facts = Facts::default();
        assert!(ids(&diagnose(&facts)).contains(&"gateway.not_running"));
        facts.service.installed = true;
        assert!(ids(&diagnose(&facts)).contains(&"gateway.service_not_answering"));
    }

    #[test]
    fn definition_arguments_are_read_back_from_both_platforms_text() {
        let unit = "ExecStart=/x/aikit gateway serve --unix --ws 100.1.2.3:7788 --ws-token-location file:/t --ws-owner-token-location file:/o\nEnvironment=\"AIKIT_GATEWAY_REF=agency-gateway/mac\"\n";
        assert_eq!(
            definition_argument(unit, "--ws").as_deref(),
            Some("100.1.2.3:7788")
        );
        assert_eq!(
            definition_argument(unit, "--ws-owner-token-location").as_deref(),
            Some("file:/o")
        );
        assert_eq!(
            environment_value(unit, "AIKIT_GATEWAY_REF").as_deref(),
            Some("agency-gateway/mac")
        );
        let plist = "<string>--ws</string>\n<string>100.1.2.3:7788</string>\n<key>AIKIT_GATEWAY_LIFECYCLE</key>\n<string>supervised-launchd</string>";
        assert_eq!(
            definition_argument(plist, "--ws").as_deref(),
            Some("100.1.2.3:7788")
        );
        assert_eq!(
            environment_value(plist, "AIKIT_GATEWAY_LIFECYCLE").as_deref(),
            Some("supervised-launchd")
        );
    }
}
