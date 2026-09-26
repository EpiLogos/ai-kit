//! Gateway coexistence: awareness of OTHER harness-owned gateway services on
//! the same machine, and an inspectable, editable policy between them.
//!
//! A machine can carry more than one harness gateway — Hermes, OpenClaw, this
//! gateway — each with its own LaunchAgent/service and its own conversation
//! bots. The gateway's law is: never silent competition, never silent
//! conquest. So coexistence has three parts, every one of them visible:
//!
//! 1. **Detection** ([`detect`]) reads only what this machine volunteers: the
//!    user's service-manager listing (launchd on macOS, systemd --user
//!    elsewhere), well-known harness binaries on PATH, and their well-known
//!    state directories. Detection is INSPECT ONLY — aikit never starts,
//!    stops or reconfigures a foreign service, and never reads its state.
//! 2. **Policy** ([`CoexistencePolicy`], the
//!    `aikit.gateway-coexistence/v1` document at
//!    `state/gateway-coexistence.json`): `exclusive` (the default) or
//!    `coexist`, editable with `aikit gateway coexistence --policy <p>`. The
//!    document also records foreign bot identities the owner observed (which
//!    platform a foreign harness's bot serves, by id) — the one concrete
//!    ownership fact a conflict can be named with.
//! 3. **The gate** ([`GatewayCoexistenceGate`], enforced at serve startup):
//!    with `exclusive` and a foreign gateway detected, a connector whose
//!    platform + bot identity is recorded to a *detected* foreign harness is
//!    refused with a named error. Everything else starts, but the decision —
//!    policy, every sighting, every refusal — is printed at serve startup and
//!    in `aikit gateway coexistence`. Nothing is silent, in either direction.

use std::process::Command;

use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The coexistence policy document's schema.
pub const GATEWAY_COEXISTENCE_SCHEMA: &str = "aikit.gateway-coexistence/v1";

/// The well-known file name inside an AIKit home's `state/` directory.
pub const GATEWAY_COEXISTENCE_FILE_NAME: &str = "gateway-coexistence.json";

/// How this gateway behaves toward a foreign harness gateway on the same
/// machine. `exclusive` is the default: this gateway's connectors do not run
/// beside a foreign gateway that is recorded to own the same platform + bot
/// identity. `coexist` starts connectors alongside any detected foreign
/// gateway, with the sighting disclosed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoexistencePolicy {
    #[default]
    Exclusive,
    Coexist,
}

impl CoexistencePolicy {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "exclusive" => Ok(Self::Exclusive),
            "coexist" => Ok(Self::Coexist),
            other => Err(AikitError::new(
                "gateway_coexistence.policy_unknown",
                format!(
                    "{other:?} is not a coexistence policy; use `exclusive` or `coexist`"
                ),
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Exclusive => "exclusive",
            Self::Coexist => "coexist",
        }
    }
}

/// A foreign gateway's bot identity the owner observed and recorded: which
/// harness owns it, which platform it serves, and the bot id itself. This is
/// the concrete ownership fact the exclusive policy names a conflict with —
/// detection cannot read a foreign service's bot identity without touching it,
/// so the identity is recorded here when observed, never invented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignBotIdentity {
    pub harness: String,
    pub platform: String,
    pub bot_id: String,
}

/// The editable, inspectable coexistence document. A missing file is the
/// default posture: exclusive, nothing recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoexistenceDocument {
    pub schema: String,
    #[serde(default)]
    pub policy: CoexistencePolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_bot_identities: Vec<ForeignBotIdentity>,
}

impl Default for CoexistenceDocument {
    fn default() -> Self {
        Self {
            schema: GATEWAY_COEXISTENCE_SCHEMA.into(),
            policy: CoexistencePolicy::Exclusive,
            foreign_bot_identities: Vec::new(),
        }
    }
}

/// Load the coexistence document at `path`; a missing file means the default
/// posture (exclusive, nothing recorded). A wrong schema is a named error.
pub fn load_coexistence(path: &std::path::Path) -> Result<CoexistenceDocument> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CoexistenceDocument::default());
        }
        Err(error) => {
            return Err(AikitError::new(
                "gateway_coexistence.unreadable",
                format!("read {}: {error}", path.display()),
            ));
        }
    };
    let document: CoexistenceDocument = serde_json::from_slice(&bytes).map_err(|error| {
        AikitError::new(
            "gateway_coexistence.invalid",
            format!(
                "{} is not a valid {GATEWAY_COEXISTENCE_SCHEMA} document: {error}",
                path.display()
            ),
        )
    })?;
    if document.schema != GATEWAY_COEXISTENCE_SCHEMA {
        return Err(AikitError::new(
            "gateway_coexistence.invalid",
            format!("{} has schema {}", path.display(), document.schema),
        ));
    }
    Ok(document)
}

/// Store the coexistence document atomically.
pub fn store_coexistence(path: &std::path::Path, document: &CoexistenceDocument) -> Result<()> {
    let write = |error: std::io::Error| {
        AikitError::new(
            "gateway_coexistence.write",
            format!("write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(write)?;
    }
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| AikitError::new("gateway_coexistence.write", error.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(write)?;
    std::fs::rename(&tmp, path).map_err(write)
}

/// What observation of this machine found. Pure data, so detection is proven
/// against fixtures and the live collectors only fill it in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoexistenceProbe {
    /// Service labels observed in the user's service manager (launchd on
    /// macOS, systemd --user elsewhere).
    pub service_labels: Vec<String>,
    /// Well-known harness binaries found on PATH.
    pub path_binaries: Vec<String>,
    /// Well-known harness state directories that exist (absolute paths).
    pub state_dirs: Vec<String>,
}

/// One foreign harness's observable footprint on this machine.
pub struct ForeignGatewaySpec {
    pub harness: &'static str,
    pub service_labels: &'static [&'static str],
    pub binaries: &'static [&'static str],
    pub state_dir_names: &'static [&'static str],
}

/// The foreign harness gateways this build knows. A sighting names the
/// harness and carries every piece of evidence observed — what was seen, and
/// where.
pub const FOREIGN_GATEWAYS: &[ForeignGatewaySpec] = &[
    ForeignGatewaySpec {
        harness: "hermes",
        service_labels: &["ai.hermes.gateway"],
        binaries: &["hermes"],
        state_dir_names: &[".hermes"],
    },
    ForeignGatewaySpec {
        harness: "openclaw",
        service_labels: &["ai.openclaw.gateway", "openclaw.gateway"],
        binaries: &["openclaw"],
        state_dir_names: &[".openclaw"],
    },
];

/// One detected foreign harness gateway, with the evidence for the sighting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignGateway {
    pub harness: String,
    pub evidence: Vec<String>,
}

/// Read a probe against the known foreign gateways. Pure: the same probe
/// always yields the same sightings.
pub fn detect(probe: &CoexistenceProbe) -> Vec<ForeignGateway> {
    FOREIGN_GATEWAYS
        .iter()
        .filter_map(|spec| {
            let mut evidence = Vec::new();
            for label in spec.service_labels {
                if probe.service_labels.iter().any(|seen| seen == label) {
                    evidence.push(format!(
                        "service {label} is loaded in the user's service manager"
                    ));
                }
            }
            for binary in spec.binaries {
                if probe.path_binaries.iter().any(|seen| seen == binary) {
                    evidence.push(format!("the {binary:?} binary is on PATH"));
                }
            }
            for dir in spec.state_dir_names {
                if probe
                    .state_dirs
                    .iter()
                    .any(|seen| seen.ends_with(dir))
                {
                    evidence.push(format!(
                        "the harness state directory {dir} exists in the home"
                    ));
                }
            }
            if evidence.is_empty() {
                None
            } else {
                Some(ForeignGateway {
                    harness: spec.harness.to_owned(),
                    evidence,
                })
            }
        })
        .collect()
}

/// The coexistence decision: the policy read against the sightings. Every
/// variant is disclosable in plain words, and none of them is ever implied —
/// serve prints the decision it reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum CoexistenceDecision {
    /// No foreign harness gateway observed: the machine is ours alone, as far
    /// as observation can say.
    ExclusivelyOurs,
    /// `exclusive` policy with a foreign gateway detected: connectors whose
    /// platform + bot identity is recorded to a detected foreign harness are
    /// refused at start.
    ExclusiveHold { foreign: Vec<ForeignGateway> },
    /// `coexist` policy with a foreign gateway detected: connectors start
    /// alongside the sighting, which is disclosed.
    Coexisting { foreign: Vec<ForeignGateway> },
}

impl CoexistenceDecision {
    /// The plain-words summary serve startup and the CLI verb print.
    pub fn summary(&self) -> String {
        match self {
            Self::ExclusivelyOurs => {
                "no foreign harness gateway observed; the machine is ours alone as far as \
                 observation can say"
                    .into()
            }
            Self::ExclusiveHold { foreign } => format!(
                "exclusive: a foreign harness gateway is present ({}); connectors whose \
                 recorded platform + bot identity it owns are refused at start",
                harnesses(foreign),
            ),
            Self::Coexisting { foreign } => format!(
                "coexisting: connectors start alongside the detected foreign gateway(s) ({})",
                harnesses(foreign),
            ),
        }
    }
}

fn harnesses(foreign: &[ForeignGateway]) -> String {
    foreign
        .iter()
        .map(|gateway| gateway.harness.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read the policy against the sightings.
pub fn decide(policy: CoexistencePolicy, foreign: &[ForeignGateway]) -> CoexistenceDecision {
    if foreign.is_empty() {
        CoexistenceDecision::ExclusivelyOurs
    } else {
        match policy {
            CoexistencePolicy::Exclusive => CoexistenceDecision::ExclusiveHold {
                foreign: foreign.to_vec(),
            },
            CoexistencePolicy::Coexist => CoexistenceDecision::Coexisting {
                foreign: foreign.to_vec(),
            },
        }
    }
}

/// The serve-startup seam: every enabled connector passes the gate before its
/// pump spawns. `Err` names the conflict and the connector is not started;
/// the named error is printed, never swallowed.
pub trait GatewayCoexistenceGate: Send + Sync {
    fn admit_connector(&self, connector_ref: &str, platform: &str) -> Result<()>;
}

/// The `exclusive` gate: refuse a connector exactly when a detected foreign
/// harness is recorded to own a bot identity on the connector's platform.
/// Mere co-detection refuses nothing by itself — a different bot on the same
/// platform is real coexistence, and the sighting is disclosed instead.
pub struct ExclusiveCoexistenceGate {
    foreign: Vec<ForeignGateway>,
    recorded: Vec<ForeignBotIdentity>,
}

impl ExclusiveCoexistenceGate {
    pub fn new(foreign: Vec<ForeignGateway>, recorded: Vec<ForeignBotIdentity>) -> Self {
        Self { foreign, recorded }
    }
}

/// Build the exclusive gate from a decision's sightings and the document's
/// recorded identities.
pub fn exclusive_gate(
    foreign: Vec<ForeignGateway>,
    recorded: Vec<ForeignBotIdentity>,
) -> ExclusiveCoexistenceGate {
    ExclusiveCoexistenceGate::new(foreign, recorded)
}

impl GatewayCoexistenceGate for ExclusiveCoexistenceGate {
    fn admit_connector(&self, connector_ref: &str, platform: &str) -> Result<()> {
        let conflicts: Vec<&ForeignBotIdentity> = self
            .recorded
            .iter()
            .filter(|identity| identity.platform.eq_ignore_ascii_case(platform))
            .filter(|identity| {
                self.foreign
                    .iter()
                    .any(|gateway| gateway.harness == identity.harness)
            })
            .collect();
        if conflicts.is_empty() {
            return Ok(());
        }
        let named = conflicts
            .iter()
            .map(|identity| {
                format!(
                    "{} on {} (bot {})",
                    identity.harness, identity.platform, identity.bot_id
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        Err(AikitError::new(
            "gateway_coexistence.exclusive_conflict",
            format!(
                "connector {connector_ref} serves {platform}, whose bot identity a foreign \
                 harness gateway detected on this machine is recorded to own ({named}); the \
                 exclusive coexistence policy refuses to start it. Switch with `aikit gateway \
                 coexistence --policy coexist`, or remove the recorded identity from the \
                 coexistence document."
            ),
        ))
    }
}

/// Collect what this machine volunteers, read-only. Service-manager listing,
/// PATH scan, home state directories — never a foreign service is started,
/// stopped, configured or read.
pub fn probe_live() -> CoexistenceProbe {
    CoexistenceProbe {
        service_labels: service_manager_labels(),
        path_binaries: FOREIGN_GATEWAYS
            .iter()
            .flat_map(|spec| spec.binaries.iter())
            .filter(|binary| which(binary))
            .map(|binary| (*binary).to_owned())
            .collect(),
        state_dirs: FOREIGN_GATEWAYS
            .iter()
            .flat_map(|spec| spec.state_dir_names.iter())
            .filter_map(|dir| {
                let home = std::env::var_os("HOME")?;
                let path = std::path::PathBuf::from(home).join(dir);
                path.is_dir().then(|| path.display().to_string())
            })
            .collect(),
    }
}

fn which(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(binary);
        candidate.is_file()
            && candidate
                .metadata()
                .map(|metadata| {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
    })
}

fn service_manager_labels() -> Vec<String> {
    let output = service_manager_listing();
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // launchd: PID Status Label. systemd --user --no-legend: UNIT …
            fields.last().map(|label| (*label).to_owned())
        })
        .filter(|label| {
            FOREIGN_GATEWAYS
                .iter()
                .any(|spec| spec.service_labels.iter().any(|known| known == label))
        })
        .collect()
}

fn service_manager_listing() -> String {
    #[cfg(target_os = "macos")]
    {
        Command::new("launchctl")
            .arg("list")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("systemctl")
            .args(["--user", "list-units", "--all", "--no-legend"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default()
    }
    #[cfg(not(unix))]
    {
        String::new()
    }
}

/// The JSON shape `aikit gateway coexistence --json` prints: the document,
/// the probe-derived sightings and the decision, all inspectable.
pub fn coexistence_report(
    document: &CoexistenceDocument,
    path: &std::path::Path,
    foreign: &[ForeignGateway],
    decision: &CoexistenceDecision,
) -> Value {
    serde_json::json!({
        "schema": document.schema,
        "policy": document.policy,
        "path": path.display().to_string(),
        "foreign_bot_identities": document.foreign_bot_identities,
        "foreign_gateways": foreign,
        "decision": decision,
        "law": "detection is inspect-only: aikit never starts, stops or reconfigures a \
                foreign harness gateway",
    })
}
