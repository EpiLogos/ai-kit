//! Gateway contact: `aikit gateway who | send | inbox | conversation |
//! delegate | forward | remote`.
//!
//! The joins behind World-inhabitation contact (O:I #65/#220, Factory #195):
//! Central says which Positions exist, Actuation says who occupies each one,
//! Factory says what each carries, and the gateway's own journal says what
//! was said between them. This module composes those answers; it holds no
//! roster, no occupancy and no custody of its own.
//!
//! Laws kept here:
//!
//! - **Attribution comes from occupancy, never from text.** The sender is the
//!   Position/generation this body was launched into (`OI_POSITION_REF`,
//!   `OI_OCCUPANT_GENERATION`, or `--from-position`), verified current by
//!   `actuation occupancy verify`. An unverifiable claim is labelled
//!   `claimed`; no identity at all is labelled `unknown` and still delivered
//!   (OpenRig P18). A superseded generation is refused: a body that no longer
//!   holds the address may not speak for it. Nothing in a body changes any of
//!   this.
//! - **Contact never blocks.** `send` appends and returns. A vacant recipient
//!   is `held` for its next occupant; an occupant on another Workcell is
//!   relayed through the gateway's authenticated WebSocket carrier, and a
//!   remote that is down leaves the record queued for the next relay pass.
//! - **Agency identity is addressable by default** (the V0 reconciliation).
//!   `who`, `send` and `/ask` resolve the recipient as agency identity first:
//!   every registered agent profile (`agent-profile.list`, the registry the
//!   minted Positions' `eligible_agent_refs` point into) answers at its
//!   identity, joined with the Position that names it when one exists —
//!   occupancy then routes exactly as for any Position. A profile with no
//!   Position is **not currently embodied**, never nonexistent: the Communique
//!   is recorded held for the agency, at the agency's identity. A handle with
//!   neither profile nor Position is refused as unknown, naming the listing
//!   remedy. Position stays what it is: the occupancy projection — tenure,
//!   succession, attribution verification.
//! - **Occupancy is each Workcell's own.** This Workcell's Actuation ledger
//!   is read first; only when it records no current occupant are the declared
//!   remote gateways asked, and each answers from its own Actuation at the
//!   moment of asking. One answer routes; two are refused as ambiguous; none
//!   holds. Nothing is cached and no second occupancy store exists.
//! - **A durable route and an exact route continue differently.** `--to P`
//!   alone is a durable Position route: it follows succession and reaches
//!   whichever generation holds P when it is delivered. `--instance G` binds
//!   it to one occupancy generation (Actuation's `generation_ref`), and
//!   `--require-workcell W` to that generation standing on W. An exact route
//!   is never delivered to a successor, a same-named occupant on another
//!   Workcell, or the right generation on the wrong Workcell: it stays held
//!   with its reason (`instance-absent`, `instance-superseded`,
//!   `workcell-mismatch`, `instance-unverified`) and is relayed only to the
//!   Workcell where that generation is current.
//! - **A Communique never mints obligation.** Only `delegate` crosses into
//!   Factory custody, and the custody ref is Factory's answer.
//! - **Every refusal is three-part**: the fact, what did or did not happen,
//!   and the exact next command.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{
    Communique, CommuniqueDraft, CommuniqueForward, CommuniqueForwardOutcome, CommuniqueInstance,
    CommuniqueInstanceHold, CommuniqueRouting, CommuniqueState, GatewayAskRequest, GatewayAskRoute,
    GatewayCarrierTarget, GatewayCommand, GatewayResponse, SenderAttribution,
    COMMUNIQUE_REF_PREFIX, GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE,
};
use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::gateway_owners::{
    current_tenure, position_record, profile_answers_to, profile_handle, profile_record,
    ContactOwners, CustodyAssign, OccupancyVerdict, OwnerUnavailable, PositionLookup,
};
use crate::secret_location::SecretLocation;

#[path = "gateway_owner_address.rs"]
pub mod owner_address;

pub const POPULATION_READING_SCHEMA: &str = "aikit.population-reading/v1";
pub const GATEWAY_REMOTES_SCHEMA: &str = "aikit.gateway-remotes/v1";

/// The environment a launched body carries (contract §2): identity is
/// recovered from these, never from text.
pub const POSITION_ENV: &str = "OI_POSITION_REF";
pub const GENERATION_ENV: &str = "OI_OCCUPANT_GENERATION";
/// Explicit Workcell identity for this AIKit home, when Central's
/// `central.world.here` is not the answer (a second home on one machine).
pub const WORKCELL_ENV: &str = "AIKIT_WORKCELL_REF";

/// A three-part refusal as an AIKit error: the message reads as one plain
/// sentence and the parts stay structured for a UI.
pub fn three_part(
    code: &'static str,
    fact: impl Into<String>,
    consequence: impl Into<String>,
    action: impl Into<String>,
) -> AikitError {
    let (fact, consequence, action) = (fact.into(), consequence.into(), action.into());
    AikitError::new(code, format!("{fact} {consequence} {action}"))
        .with("fact", fact)
        .with("consequence", consequence)
        .with("action", action)
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The local gateway: its carrier, or its state file when no service runs.
// ---------------------------------------------------------------------------

/// One command against the gateway this AIKit home owns.
pub trait GatewayAccess {
    fn call(&self, command: GatewayCommand) -> Result<GatewayResponse>;
}

/// Production access: the running service's carrier; when no service answers
/// on the home socket, the same kernel command executed against the durable
/// state file under the state lock. A sender is therefore never blocked by a
/// stopped gateway, and a running service is never written around.
pub struct LocalGateway {
    pub target: GatewayCarrierTarget,
    /// The state file an offline command may use; `None` for a remote
    /// (`--ws`) target, which has no local state to fall back to.
    pub state_file: Option<PathBuf>,
    pub gateway_ref: String,
}

impl LocalGateway {
    pub fn for_home(home: &AikitHome, target: GatewayCarrierTarget) -> Self {
        #[cfg(unix)]
        let state_file = match &target {
            GatewayCarrierTarget::UnixSocket(_) => Some(home.gateway_state()),
            GatewayCarrierTarget::WebSocket { .. } => None,
        };
        #[cfg(not(unix))]
        let state_file = None;
        Self {
            target,
            state_file,
            gateway_ref: std::env::var("AIKIT_GATEWAY_REF")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "agency-gateway/local".into()),
        }
    }

    pub fn default_for(home: &AikitHome) -> Self {
        #[cfg(unix)]
        let target = GatewayCarrierTarget::UnixSocket(home.gateway_socket());
        #[cfg(not(unix))]
        let target = GatewayCarrierTarget::websocket("127.0.0.1:0", "");
        Self::for_home(home, target)
    }
}

impl GatewayAccess for LocalGateway {
    fn call(&self, command: GatewayCommand) -> Result<GatewayResponse> {
        match aikit_adapters::gateway_command(&self.target, command.clone(), None) {
            Ok(response) => Ok(response),
            Err(error) if error.code() == "agency_gateway_client.unix_connect" => {
                let Some(state_file) = &self.state_file else {
                    return Err(error);
                };
                let gateway_ref = aikit_core::resource::ResourceRef::parse(&self.gateway_ref)?;
                aikit_adapters::execute_against_state_file(
                    aikit_adapters::AgencyGateway::new(gateway_ref),
                    state_file,
                    command,
                    Duration::from_secs(2),
                )
                .map_err(|offline| {
                    if offline.code() == "agency_gateway_service.state_locked" {
                        three_part(
                            "gateway.state_held_by_unreachable_service",
                            format!(
                                "No gateway answered at {}, yet another process holds its state {} ({}).",
                                match &self.target {
                                    #[cfg(unix)]
                                    GatewayCarrierTarget::UnixSocket(path) => path.display().to_string(),
                                    GatewayCarrierTarget::WebSocket { bind, .. } => bind.clone(),
                                },
                                state_file.display(),
                                offline.message()
                            ),
                            "Nothing was read or written.",
                            "Check `aikit gateway status`; if a service is running on another socket, address it with --unix PATH.",
                        )
                    } else {
                        offline
                    }
                })
            }
            Err(error) => Err(error),
        }
    }
}

fn expect_list(response: GatewayResponse) -> Result<Vec<Communique>> {
    match response {
        GatewayResponse::CommuniqueList { communiques } => Ok(communiques),
        other => Err(unexpected(&other)),
    }
}

fn expect_record(response: GatewayResponse) -> Result<Communique> {
    match response {
        GatewayResponse::CommuniqueRecord { communique } => Ok(communique),
        other => Err(unexpected(&other)),
    }
}

fn expect_accepted(response: GatewayResponse) -> Result<(Communique, bool, String)> {
    match response {
        GatewayResponse::CommuniqueAccepted {
            communique,
            replayed,
            accepted_by,
        } => Ok((communique, replayed, accepted_by)),
        other => Err(unexpected(&other)),
    }
}

fn unexpected(response: &GatewayResponse) -> AikitError {
    AikitError::new(
        "gateway.unexpected_response",
        format!(
            "the gateway answered with {} to a Communique command; it may predate gateway contact — restart it with this aikit",
            serde_json::to_value(response)
                .ok()
                .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| "an unknown response".into())
        ),
    )
}

// ---------------------------------------------------------------------------
// Declared remote Workcells.
// ---------------------------------------------------------------------------

/// One remote Workcell's gateway endpoint. The bearer token is a location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRemote {
    pub workcell_ref: String,
    pub websocket_bind: String,
    #[serde(default = "default_ws_path")]
    pub websocket_path: String,
    pub token_location: String,
}

fn default_ws_path() -> String {
    "/".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRemotes {
    pub schema: String,
    #[serde(default)]
    pub remotes: Vec<GatewayRemote>,
}

impl Default for GatewayRemotes {
    fn default() -> Self {
        Self {
            schema: GATEWAY_REMOTES_SCHEMA.into(),
            remotes: Vec::new(),
        }
    }
}

pub fn remotes_path(home: &AikitHome) -> PathBuf {
    home.state().join("gateway-remotes.json")
}

pub fn load_remotes(home: &AikitHome) -> Result<GatewayRemotes> {
    let path = remotes_path(home);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let remotes: GatewayRemotes = serde_json::from_slice(&bytes).map_err(|error| {
                AikitError::new(
                    "gateway.remotes_invalid",
                    format!(
                        "{} is not a valid {GATEWAY_REMOTES_SCHEMA} document: {error}",
                        path.display()
                    ),
                )
            })?;
            if remotes.schema != GATEWAY_REMOTES_SCHEMA {
                return Err(AikitError::new(
                    "gateway.remotes_invalid",
                    format!("{} has schema {}", path.display(), remotes.schema),
                ));
            }
            Ok(remotes)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(GatewayRemotes::default()),
        Err(error) => Err(AikitError::new(
            "gateway.remotes_unreadable",
            format!("read {}: {error}", path.display()),
        )),
    }
}

fn store_remotes(home: &AikitHome, remotes: &GatewayRemotes) -> Result<()> {
    let path = remotes_path(home);
    let write = |error: std::io::Error| {
        AikitError::new(
            "gateway.remotes_write",
            format!("write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(write)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(remotes)
        .map_err(|error| AikitError::new("gateway.remotes_write", error.to_string()))?;
    std::fs::write(&tmp, bytes).map_err(write)?;
    std::fs::rename(&tmp, &path).map_err(write)
}

pub fn remote_add(
    home: &AikitHome,
    workcell_ref: &str,
    websocket_bind: &str,
    websocket_path: &str,
    token_location: &str,
) -> Result<Value> {
    if workcell_ref.trim().is_empty() || websocket_bind.trim().is_empty() {
        return Err(AikitError::new(
            "cli.usage",
            "a remote needs --workcell and --ws HOST:PORT",
        ));
    }
    let location = SecretLocation::parse(token_location)?;
    let mut remotes = load_remotes(home)?;
    remotes
        .remotes
        .retain(|remote| remote.workcell_ref != workcell_ref);
    let remote = GatewayRemote {
        workcell_ref: workcell_ref.into(),
        websocket_bind: websocket_bind.into(),
        websocket_path: websocket_path.into(),
        token_location: location.render(),
    };
    remotes.remotes.push(remote.clone());
    remotes
        .remotes
        .sort_by(|a, b| a.workcell_ref.cmp(&b.workcell_ref));
    store_remotes(home, &remotes)?;
    Ok(json!({ "declared": remote, "path": remotes_path(home).display().to_string() }))
}

/// `remote add`, and then ask the endpoint what it is. A gateway that answers
/// as a *different* Workcell than the one declared is refused before anything
/// is written (every claim and relay would be recorded against the wrong
/// Workcell); one that does not answer is declared and said not to answer —
/// a peer may simply be down.
pub fn remote_add_probed(
    home: &AikitHome,
    workcell_ref: &str,
    websocket_bind: &str,
    websocket_path: &str,
    token_location: &str,
    probe: bool,
) -> Result<Value> {
    if probe {
        if let Ok(location) = SecretLocation::parse(token_location) {
            let candidate = GatewayRemote {
                workcell_ref: workcell_ref.into(),
                websocket_bind: websocket_bind.into(),
                websocket_path: websocket_path.into(),
                token_location: location.render(),
            };
            let reading = crate::gateway_upgrade_system::probe_remote(&candidate);
            if let Some(answers) = reading["answers_as_workcell"].as_str() {
                if answers != workcell_ref {
                    return Err(three_part(
                        "gateway.remote_identity_mismatch",
                        format!(
                            "The gateway at {websocket_bind} says it serves {answers}, not {workcell_ref}."
                        ),
                        "Nothing was declared: claims and relays would be recorded against the wrong Workcell.",
                        format!("Declare it as {answers}, or check the endpoint you meant."),
                    ));
                }
            }
            let mut declared = remote_add(
                home,
                workcell_ref,
                websocket_bind,
                websocket_path,
                token_location,
            )?;
            declared["probe"] = reading;
            return Ok(declared);
        }
    }
    remote_add(
        home,
        workcell_ref,
        websocket_bind,
        websocket_path,
        token_location,
    )
}

pub fn remote_remove(home: &AikitHome, workcell_ref: &str) -> Result<Value> {
    let mut remotes = load_remotes(home)?;
    let before = remotes.remotes.len();
    remotes
        .remotes
        .retain(|remote| remote.workcell_ref != workcell_ref);
    if remotes.remotes.len() == before {
        return Err(three_part(
            "gateway.remote_not_declared",
            format!("No gateway endpoint is declared for {workcell_ref}."),
            "Nothing was removed.",
            "List the declared endpoints with `aikit gateway remote list`.",
        ));
    }
    store_remotes(home, &remotes)?;
    Ok(json!({ "removed": workcell_ref }))
}

pub fn remote_list(home: &AikitHome) -> Result<Value> {
    serde_json::to_value(load_remotes(home)?)
        .map_err(|error| AikitError::new("gateway.remotes_invalid", error.to_string()))
}

fn remote_command(workcell_ref: &str) -> String {
    format!(
        "aikit gateway remote add --workcell {workcell_ref} --ws HOST:PORT --token-location file:/ABSOLUTE/PATH/TO/TOKEN"
    )
}

// ---------------------------------------------------------------------------
// Occupancy across declared Workcells.
// ---------------------------------------------------------------------------

/// How long one remote gateway may take to say who occupies a Position.
/// Contact asks while the sender waits, so a Workcell that is asleep or gone
/// costs this much at most (remotes are asked in parallel).
pub const REMOTE_OCCUPANCY_TIMEOUT: Duration = Duration::from_secs(2);

/// What one declared remote said when asked about occupancy.
#[derive(Debug, Clone)]
pub enum RemoteOutcome {
    /// Its gateway answered with its own Workcell's Actuation reading (which
    /// may itself record that Actuation could not answer).
    Answered(aikit_adapters::GatewayOccupancyReading),
    /// Its gateway was reached but refused the query (for example a gateway
    /// that predates cross-Workcell occupancy).
    Refused(String),
    /// Its gateway could not be reached.
    Unreachable(String),
}

/// One remote Workcell that reports a current occupant for a Position.
#[derive(Debug, Clone)]
pub struct RemoteClaim {
    pub remote: GatewayRemote,
    pub gateway_ref: String,
    pub generation_ref: Option<String>,
    pub tenure: Value,
}

/// Every declared remote, asked once, in declaration order. The survey holds
/// the remote owners' answers for the length of one operation only; it is
/// never stored.
#[derive(Debug, Clone, Default)]
pub struct RemoteSurvey {
    pub answers: Vec<(GatewayRemote, RemoteOutcome)>,
}

/// The declared remotes other than this home's own Workcell.
fn remotes_elsewhere(home: &AikitHome, local: Option<&str>) -> Result<Vec<GatewayRemote>> {
    Ok(load_remotes(home)?
        .remotes
        .into_iter()
        .filter(|remote| Some(remote.workcell_ref.as_str()) != local)
        .collect())
}

fn ask_remote(remote: &GatewayRemote, command: GatewayCommand) -> RemoteOutcome {
    let attempt = SecretLocation::parse(&remote.token_location)
        .and_then(|location| location.resolve())
        .and_then(|token| {
            let target = GatewayCarrierTarget::WebSocket {
                bind: remote.websocket_bind.clone(),
                path: remote.websocket_path.clone(),
                bearer_token: token.expose().to_owned(),
            };
            aikit_adapters::gateway_command_within(&target, command, None, REMOTE_OCCUPANCY_TIMEOUT)
        });
    match attempt {
        Ok(GatewayResponse::Occupancy { reading }) => RemoteOutcome::Answered(reading),
        Ok(other) => RemoteOutcome::Refused(unexpected(&other).to_string()),
        Err(error) if error.code() == "agency_gateway_client.gateway_refused" => {
            RemoteOutcome::Refused(error.to_string())
        }
        Err(error) => RemoteOutcome::Unreachable(error.to_string()),
    }
}

/// The tenure a reading records as current for `position_ref`, whether the
/// reading is one Position's document or the whole listing.
fn tenure_in<'a>(occupancy: &'a Value, position_ref: &str) -> Option<&'a Value> {
    match occupancy.get("positions").and_then(Value::as_array) {
        Some(rows) => rows
            .iter()
            .find(|row| row.get("position_ref").and_then(Value::as_str) == Some(position_ref))
            .and_then(current_tenure),
        None => (occupancy.get("position_ref").and_then(Value::as_str) == Some(position_ref))
            .then(|| current_tenure(occupancy))
            .flatten(),
    }
}

impl RemoteSurvey {
    /// Ask every remote the same question, in parallel, each bounded by
    /// [`REMOTE_OCCUPANCY_TIMEOUT`].
    pub fn ask(remotes: &[GatewayRemote], command: &GatewayCommand) -> Self {
        let answers = std::thread::scope(|scope| {
            let asks: Vec<_> = remotes
                .iter()
                .map(|remote| {
                    let command = command.clone();
                    scope.spawn(move || ask_remote(remote, command))
                })
                .collect();
            remotes
                .iter()
                .cloned()
                .zip(asks.into_iter().map(|ask| {
                    ask.join().unwrap_or_else(|_| {
                        RemoteOutcome::Unreachable("the query thread panicked".into())
                    })
                }))
                .collect()
        });
        Self { answers }
    }

    /// The remotes that report a current occupant of `position_ref`.
    pub fn claims(&self, position_ref: &str) -> Vec<RemoteClaim> {
        self.answers
            .iter()
            .filter_map(|(remote, outcome)| match outcome {
                RemoteOutcome::Answered(reading) => reading
                    .occupancy
                    .as_ref()
                    .and_then(|occupancy| tenure_in(occupancy, position_ref))
                    .map(|tenure| RemoteClaim {
                        remote: remote.clone(),
                        gateway_ref: reading.gateway_ref.clone(),
                        generation_ref: tenure
                            .get("generation_ref")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        tenure: tenure.clone(),
                    }),
                _ => None,
            })
            .collect()
    }

    /// Plain words for every remote that could not say anything about
    /// occupancy: unreachable, refused, or its Actuation unavailable.
    pub fn unanswered(&self) -> Vec<String> {
        self.answers
            .iter()
            .filter_map(|(remote, outcome)| match outcome {
                RemoteOutcome::Answered(reading) => reading.unavailable.as_ref().map(|owner| {
                    format!(
                        "{} (its Actuation could not answer: `{}` failed: {})",
                        remote.workcell_ref, owner.command, owner.reason
                    )
                }),
                RemoteOutcome::Refused(detail) | RemoteOutcome::Unreachable(detail) => {
                    Some(format!("{} ({detail})", remote.workcell_ref))
                }
            })
            .collect()
    }

    /// The remotes that answered and record no current occupant.
    pub fn answered_vacant(&self, position_ref: &str) -> Vec<String> {
        self.answers
            .iter()
            .filter_map(|(remote, outcome)| match outcome {
                RemoteOutcome::Answered(reading)
                    if reading.unavailable.is_none()
                        && reading.occupancy.as_ref().is_some_and(|occupancy| {
                            tenure_in(occupancy, position_ref).is_none()
                        }) =>
                {
                    Some(remote.workcell_ref.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// `data.remotes` rows of the population reading and send receipts.
    pub fn statuses(&self) -> Vec<Value> {
        self.answers
            .iter()
            .map(|(remote, outcome)| match outcome {
                RemoteOutcome::Answered(reading) => json!({
                    "workcell_ref": remote.workcell_ref,
                    "gateway_ref": reading.gateway_ref,
                    "status": "reachable",
                    "detail": match (&reading.unavailable, &reading.workcell_ref) {
                        (Some(owner), _) => format!(
                            "the gateway answered, but its Actuation could not: `{}` failed: {}",
                            owner.command, owner.reason
                        ),
                        (None, Some(answered)) if answered != &remote.workcell_ref => format!(
                            "the gateway declared for {} says it serves {answered} ({}); check `aikit gateway remote list`",
                            remote.workcell_ref, reading.workcell_basis
                        ),
                        (None, _) => format!("answered from its Workcell's Actuation at {}", remote.websocket_bind),
                    },
                }),
                RemoteOutcome::Refused(detail) => json!({
                    "workcell_ref": remote.workcell_ref,
                    "gateway_ref": Value::Null,
                    "status": "reachable",
                    "detail": format!("the gateway at {} refused the occupancy query (it may predate cross-Workcell occupancy): {detail}", remote.websocket_bind),
                }),
                RemoteOutcome::Unreachable(detail) => json!({
                    "workcell_ref": remote.workcell_ref,
                    "gateway_ref": Value::Null,
                    "status": "unreachable",
                    "detail": detail,
                }),
            })
            .collect()
    }
}

impl RemoteClaim {
    fn routing(&self, position_ref: &str) -> aikit_adapters::CommuniqueRouting {
        aikit_adapters::CommuniqueRouting {
            workcell_ref: self.remote.workcell_ref.clone(),
            gateway_ref: self.gateway_ref.clone(),
            generation_ref: self.generation_ref.clone(),
            basis: format!(
                "{position_ref} has no current occupant on this Workcell; gateway {} of {} reports {} current there",
                self.gateway_ref,
                self.remote.workcell_ref,
                self.generation_ref.as_deref().unwrap_or("an unnamed generation")
            ),
            observed_at_unix_ms: now_unix_ms(),
        }
    }

    fn describe(&self) -> String {
        format!(
            "{} ({} via gateway {})",
            self.remote.workcell_ref,
            self.generation_ref
                .as_deref()
                .unwrap_or("an unnamed generation"),
            self.gateway_ref
        )
    }
}

fn ambiguous_occupancy(position_ref: &str, claims: &[RemoteClaim]) -> AikitError {
    three_part(
        "gateway.occupancy_ambiguous",
        format!(
            "Position {position_ref} has a current occupant on more than one Workcell: {}.",
            claims
                .iter()
                .map(RemoteClaim::describe)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "Nothing was sent; no Communique was recorded. The gateway does not choose between two occupants of one address.",
        format!(
            "Settle the occupancy on those Workcells (`actuation occupancy read --position {position_ref}` on each; release the stale tenure), then send again."
        ),
    )
}

/// The serving gateway's answer to a peer's "who occupies P on your
/// Workcell": this Workcell's Actuation, read when asked, beside this
/// Workcell's ref. Nothing is cached; nothing is stored.
pub struct ServedOccupancy {
    pub cwd: PathBuf,
}

impl aikit_adapters::GatewayOccupancyReader for ServedOccupancy {
    fn read(
        &self,
        gateway_ref: &str,
        position_ref: Option<&str>,
    ) -> aikit_adapters::GatewayOccupancyReading {
        let owners = crate::gateway_owners::ProcessOwners::from_env();
        let (workcell_ref, workcell_basis) = local_workcell(&owners, &self.cwd);
        let answer = match position_ref {
            Some(position_ref) => owners.occupancy_read(position_ref),
            None => owners.occupancy_list(),
        };
        let (occupancy, unavailable) = match answer {
            Ok(reading) => (Some(reading), None),
            Err(owner) => (
                None,
                Some(aikit_adapters::GatewayOwnerUnavailable {
                    command: owner.command,
                    reason: owner.reason,
                }),
            ),
        };
        aikit_adapters::GatewayOccupancyReading {
            schema: aikit_adapters::GATEWAY_OCCUPANCY_READING_SCHEMA.into(),
            position_ref: position_ref.map(str::to_owned),
            gateway_ref: gateway_ref.to_owned(),
            workcell_ref,
            workcell_basis,
            occupancy,
            unavailable,
            read_at_unix_ms: now_unix_ms(),
        }
    }
}

// ---------------------------------------------------------------------------
// Where this home stands: its Workcell, its Project World.
// ---------------------------------------------------------------------------

/// This AIKit home's Workcell: `AIKIT_WORKCELL_REF` when declared, else the
/// Workcell Central's `central.world.here` names as current. `None` means
/// the Workcell is unknown, which is reported, never guessed.
pub fn local_workcell(owners: &dyn ContactOwners, cwd: &Path) -> (Option<String>, String) {
    if let Ok(value) = std::env::var(WORKCELL_ENV) {
        if !value.trim().is_empty() {
            return (Some(value), WORKCELL_ENV.to_owned());
        }
    }
    match owners.world_here(cwd) {
        Ok(here) => {
            let declared: Vec<&Value> = here
                .get("workcells")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .collect();
            let current = declared
                .iter()
                .find(|workcell| workcell.get("role").and_then(Value::as_str) == Some("current"))
                .and_then(|workcell| workcell.get("ref").and_then(Value::as_str));
            match (current, declared.as_slice()) {
                (Some(reference), _) => (
                    Some(reference.to_owned()),
                    "central.world.here: the Workcell declared current".into(),
                ),
                // One declared Workcell and no role marking is still the
                // root's own declaration, not a guess among several.
                (None, [only]) => match only.get("ref").and_then(Value::as_str) {
                    Some(reference) => (
                        Some(reference.to_owned()),
                        "central.world.here: the only Workcell this root declares".into(),
                    ),
                    None => (
                        None,
                        "central.world.here names a Workcell without a ref".into(),
                    ),
                },
                (None, []) => (None, "central.world.here declares no Workcell".into()),
                (None, _) => (
                    None,
                    "central.world.here declares several Workcells and marks none current".into(),
                ),
            }
        }
        Err(unavailable) => (None, unavailable.to_string()),
    }
}

/// The Project World this cwd stands in, per Central (`project_world.name`,
/// the Work member Central's position actions take as `project`).
fn here_project(owners: &dyn ContactOwners, cwd: &Path) -> Option<(String, Option<PathBuf>)> {
    let here = owners.world_here(cwd).ok()?;
    let project = here.get("project_world")?;
    if project.get("state").and_then(Value::as_str) != Some("present") {
        return None;
    }
    let name = project
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            project
                .get("ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix("project:"))
        })?
        .to_owned();
    let path = here
        .pointer("/local_world/root")
        .and_then(Value::as_str)
        .zip(project.get("path").and_then(Value::as_str))
        .map(|(root, path)| Path::new(root).join(path));
    Some((name, path))
}

/// A `--project-world` value as Central's `project` input: `project:O-I` or
/// `O-I` → `O-I`; `control:root` → the root listing.
fn project_input(world: &str) -> Option<String> {
    if world == "control:root" {
        None
    } else {
        Some(world.strip_prefix("project:").unwrap_or(world).to_owned())
    }
}

// ---------------------------------------------------------------------------
// Attribution and recipients.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SenderResolution {
    pub from_position_ref: Option<String>,
    pub from_generation_ref: Option<String>,
    pub attribution: SenderAttribution,
    pub basis: String,
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Resolve the sender from the body's own occupancy. See the module doc.
pub fn resolve_sender(
    owners: &dyn ContactOwners,
    explicit_position: Option<&str>,
) -> Result<SenderResolution> {
    let position = explicit_position
        .map(str::to_owned)
        .or_else(|| env_value(POSITION_ENV));
    let generation = env_value(GENERATION_ENV);
    let Some(position) = position else {
        return Ok(SenderResolution {
            from_position_ref: None,
            from_generation_ref: None,
            attribution: SenderAttribution::Unknown,
            basis: format!(
                "no --from-position and no {POSITION_ENV} in this body's environment; delivered labelled <unknown sender>"
            ),
        });
    };
    let Some(generation) = generation else {
        return Ok(SenderResolution {
            from_position_ref: Some(position),
            from_generation_ref: None,
            attribution: SenderAttribution::Claimed,
            basis: format!(
                "the Position was named but no {GENERATION_ENV} was presented, so occupancy was not verified"
            ),
        });
    };
    match owners.occupancy_verify(&position, &generation) {
        Ok(OccupancyVerdict::Current(_)) => Ok(SenderResolution {
            from_position_ref: Some(position.clone()),
            from_generation_ref: Some(generation.clone()),
            attribution: SenderAttribution::Verified,
            basis: format!("actuation occupancy verify: {generation} is the current occupant of {position}"),
        }),
        Ok(OccupancyVerdict::Refused(refusal)) => Err(three_part(
            "gateway.sender_not_current",
            format!(
                "This body presents generation {generation} for {position}, and Actuation refuses it ({}): {}",
                refusal.code, refusal.fact
            ),
            "Nothing was sent; a body that no longer holds an address may not speak for it.",
            if refusal.action.is_empty() {
                format!("Read the Position's current holder with `actuation occupancy read --position {position}`.")
            } else {
                refusal.action
            },
        )),
        Err(unavailable) => Ok(SenderResolution {
            from_position_ref: Some(position),
            from_generation_ref: Some(generation),
            attribution: SenderAttribution::Claimed,
            basis: format!("occupancy could not be verified: {unavailable}"),
        }),
    }
}

/// The recipient as the contact plane addresses it: a Position (the occupancy
/// projection) or — with no Position naming it — a registered agent profile,
/// held at the agency's own identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recipient {
    pub position_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: String,
    /// The agency identity (`agent/<slug>`) when the recipient is a registered
    /// agent profile with no Position: the record's address and the body its
    /// mail holds for. `None` for Position-resolved recipients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agency_ref: Option<String>,
}

fn nothing_sent() -> &'static str {
    "Nothing was sent; no Communique was recorded."
}

/// The profile rows of Central's `agent-profile.list` answer.
fn profile_entries(listing: &Value) -> impl Iterator<Item = &Value> {
    listing
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// The Positions (present and inherited) whose `eligible_agent_refs` name
/// `agent_ref` — the occupancy seats the agency may be embodied in.
fn positions_naming<'a>(position_listing: &'a Value, agent_ref: &str) -> Vec<&'a Value> {
    ["positions", "inherited"]
        .iter()
        .flat_map(|key| {
            position_listing
                .get(*key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(position_record)
        .filter(|record| {
            record
                .get("eligible_agent_refs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|reference| reference == agent_ref)
        })
        .collect()
}

/// Join one matched profile with the Position that names it. Exactly one
/// naming Position resolves the recipient to that Position — occupancy then
/// routes exactly as for any directly-addressed Position. No naming Position
/// resolves to the agency's own identity: not currently embodied, never
/// nonexistent — the Communique holds for the agency. Several naming
/// Positions are refused: the gateway does not choose between two addresses.
fn resolve_agency(
    position_listing: &Value,
    profile: &Value,
    agent_ref: &str,
    handle: Option<String>,
) -> Result<Recipient> {
    let eligible = positions_naming(position_listing, agent_ref);
    match eligible.as_slice() {
        [record] => Ok(Recipient {
            position_ref: record
                .get("ref")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            handle,
            label: record
                .get("label")
                .and_then(Value::as_str)
                .map(str::to_owned),
            source: "central.position.list+agent-profile.list".into(),
            agency_ref: None,
        }),
        [] => Ok(Recipient {
            position_ref: agent_ref.to_owned(),
            handle,
            label: profile
                .get("role")
                .and_then(Value::as_str)
                .map(str::to_owned),
            source: "agent-profile.list".into(),
            agency_ref: Some(agent_ref.to_owned()),
        }),
        _ => Err(three_part(
            "gateway.ambiguous_position",
            format!(
                "{} Positions ({}) name {agent_ref} as eligible.",
                eligible.len(),
                eligible
                    .iter()
                    .filter_map(|record| record.get("ref").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            nothing_sent(),
            "Address the Position by its full ref from `aikit gateway who --json`.",
        )),
    }
}

pub fn resolve_recipient(
    owners: &dyn ContactOwners,
    to: &str,
    project_world: Option<&str>,
    cwd: &Path,
) -> Result<Recipient> {
    let to = to.trim();
    if let Some(handle) = to.strip_prefix('@') {
        let project = match project_world {
            Some(world) => project_input(world),
            None => here_project(owners, cwd).map(|(name, _)| name),
        };
        let listing = owners
            .position_list(project.as_deref())
            .map_err(|unavailable| {
                owner_unavailable_refusal("Central could not list Positions", &unavailable)
            })?;
        let wanted = format!("@{handle}");
        let matches: Vec<&Value> = ["positions", "inherited"]
            .iter()
            .flat_map(|key| {
                listing
                    .get(*key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .map(position_record)
            .filter(|record| record.get("handle").and_then(Value::as_str) == Some(wanted.as_str()))
            .collect();
        return match matches.as_slice() {
            [record] => Ok(Recipient {
                position_ref: record
                    .get("ref")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                handle: Some(wanted),
                label: record
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source: "central.position.list".into(),
                agency_ref: None,
            }),
            // No Position carries this handle: the agency the system already
            // names may still be addressable through its registered profile.
            [] => {
                let profiles = owners.agent_profiles().map_err(|unavailable| {
                    owner_unavailable_refusal("Central could not list agent profiles", &unavailable)
                })?;
                let matches: Vec<(&Value, &str)> = profile_entries(&profiles)
                    .map(profile_record)
                    .filter_map(|profile| {
                        profile
                            .get("agent_ref")
                            .and_then(Value::as_str)
                            .map(|agent_ref| (profile, agent_ref))
                    })
                    .filter(|(_, agent_ref)| profile_answers_to(agent_ref, handle))
                    .collect();
                match matches.as_slice() {
                    [(profile, agent_ref)] => {
                        resolve_agency(&listing, profile, agent_ref, Some(wanted))
                    }
                    [] => Err(three_part(
                        "gateway.unknown_position",
                        format!(
                            "No Position with handle {wanted} is defined in {}.",
                            listing
                                .get("world_ref")
                                .and_then(Value::as_str)
                                .unwrap_or("this World")
                        ),
                        nothing_sent(),
                        "List the Positions and their handles with `aikit gateway who --json`.",
                    )),
                    _ => Err(three_part(
                        "gateway.ambiguous_recipient",
                        format!("{} agent profiles answer to {wanted}.", matches.len()),
                        nothing_sent(),
                        "List the registered agencies with `aikit gateway who --json`.",
                    )),
                }
            }
            _ => Err(three_part(
                "gateway.ambiguous_position",
                format!("{} Positions answer to {wanted}.", matches.len()),
                nothing_sent(),
                "Address the Position by its full ref from `aikit gateway who --json`.",
            )),
        };
    }
    if !to.starts_with("central:position:") {
        // An agent identity (`agent/<slug>`, the registry's own spelling) is
        // a recipient when the registry names it.
        let profiles = owners.agent_profiles().map_err(|unavailable| {
            owner_unavailable_refusal("Central could not list agent profiles", &unavailable)
        })?;
        let matches: Vec<(&Value, &str)> = profile_entries(&profiles)
            .map(profile_record)
            .filter_map(|profile| {
                profile
                    .get("agent_ref")
                    .and_then(Value::as_str)
                    .map(|agent_ref| (profile, agent_ref))
            })
            .filter(|(_, agent_ref)| *agent_ref == to)
            .collect();
        return match matches.as_slice() {
            [(profile, agent_ref)] => {
                let listing = owners.position_list(None).map_err(|unavailable| {
                    owner_unavailable_refusal("Central could not list Positions", &unavailable)
                })?;
                resolve_agency(&listing, profile, agent_ref, None)
            }
            [] => Err(three_part(
                "gateway.invalid_recipient",
                format!(
                    "{to:?} is neither a Position ref (central:position:<world>:<slug>), a \
                     registered agent ref (agent/<slug>), nor an @handle."
                ),
                nothing_sent(),
                "List the Positions and the registered agencies with `aikit gateway who --json`.",
            )),
            _ => Err(three_part(
                "gateway.ambiguous_recipient",
                format!("{} agent profiles answer to {to}.", matches.len()),
                nothing_sent(),
                "List the registered agencies with `aikit gateway who --json`.",
            )),
        };
    }
    match owners.position_read(to).map_err(|unavailable| {
        owner_unavailable_refusal("Central could not read the Position", &unavailable)
    })? {
        PositionLookup::Found(value) => {
            let record = position_record(&value);
            Ok(Recipient {
                position_ref: record
                    .get("ref")
                    .and_then(Value::as_str)
                    .unwrap_or(to)
                    .to_owned(),
                handle: record
                    .get("handle")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                label: record
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source: "central.position.read".into(),
                agency_ref: None,
            })
        }
        PositionLookup::NotFound(refusal) => Err(three_part(
            "gateway.unknown_position",
            if refusal.fact.is_empty() {
                format!("Central defines no Position {to}.")
            } else {
                refusal.fact
            },
            nothing_sent(),
            "List the Positions with `aikit gateway who --json`.",
        )),
    }
}

fn owner_unavailable_refusal(what: &str, unavailable: &OwnerUnavailable) -> AikitError {
    three_part(
        "gateway.owner_unavailable",
        format!(
            "{what}: `{}` failed ({}).",
            unavailable.command, unavailable.reason
        ),
        "Nothing was sent or recorded; the gateway does not guess an owner's answer.",
        format!(
            "Make the owner answer (re-run `{}`), then retry.",
            unavailable.command
        ),
    )
}

// ---------------------------------------------------------------------------
// send
// ---------------------------------------------------------------------------

pub struct SendRequest<'a> {
    pub to: &'a str,
    pub body: String,
    pub reply_to: Option<String>,
    pub from_position: Option<&'a str>,
    pub project_world: Option<&'a str>,
    /// Exact-instance route: the occupancy generation (`generation_ref`) the
    /// Communique is bound to. `None` is a durable Position route.
    pub instance: Option<&'a str>,
    /// With `instance`: the Workcell that generation must stand on.
    pub require_workcell: Option<&'a str>,
    /// What is asked when the recipient is the owner (`@owner`).
    pub owner: owner_address::OwnerAsk,
}

/// The Workcell a tenure names, when it names one.
pub fn tenure_workcell(tenure: &Value) -> Option<String> {
    tenure
        .get("workcell_ref")
        .and_then(Value::as_str)
        .filter(|workcell| !workcell.trim().is_empty())
        .map(str::to_owned)
}

/// Where an exact instance stands, as the owners answer at the moment of
/// asking.
enum InstancePlacement {
    /// Current, on this Workcell (or where this Workcell cannot tell): it is
    /// this gateway's to deliver at that instance's next turn boundary.
    Here { basis: String },
    /// Current on a declared remote Workcell: relayed there and nowhere else.
    Relay {
        remote: GatewayRemote,
        routing: Option<CommuniqueRouting>,
        basis: String,
    },
    /// Current on a Workcell with no declared endpoint.
    Undeclared { workcell_ref: String, basis: String },
    /// Not deliverable to anyone now; held with the reason.
    Held {
        hold: CommuniqueInstanceHold,
        basis: String,
    },
    /// More than one remote reports the same generation current.
    Ambiguous(AikitError),
}

struct InstanceReading {
    placement: InstancePlacement,
    /// The instance's tenure, when an owner reported it current.
    tenure: Option<Value>,
    /// `remotes` rows for the receipt, when remotes were asked.
    remotes: Vec<Value>,
}

/// Place an exact instance. This Workcell's Actuation is asked first
/// (`occupancy verify`); only a generation this ledger never knew is looked
/// for on declared remotes — and only on the required Workcell's gateway when
/// one is required. A remote occupant of the same Position under another
/// generation is a same-named peer, never a route.
fn place_instance(
    owners: &dyn ContactOwners,
    position_ref: &str,
    instance: &CommuniqueInstance,
    local: Option<&str>,
    declared: &GatewayRemotes,
    survey_of: &mut dyn FnMut(&[GatewayRemote]) -> RemoteSurvey,
) -> InstanceReading {
    let generation = instance.generation_ref.as_str();
    let required = instance.required_workcell_ref.as_deref();
    let held = |hold, basis| InstanceReading {
        placement: InstancePlacement::Held { hold, basis },
        tenure: None,
        remotes: Vec::new(),
    };
    let unknown_here = match owners.occupancy_verify(position_ref, generation) {
        Ok(OccupancyVerdict::Current(tenure)) => {
            let stands = tenure_workcell(&tenure).or_else(|| local.map(str::to_owned));
            if let Some(required) = required {
                if stands.as_deref() != Some(required) {
                    return InstanceReading {
                        placement: InstancePlacement::Held {
                            hold: CommuniqueInstanceHold::WorkcellMismatch,
                            basis: format!(
                                "{generation} is the current occupant of {position_ref}, but it stands on {}, not the required Workcell {required}; held, and delivered to no occupant elsewhere",
                                stands.as_deref().unwrap_or("a Workcell this ledger does not name")
                            ),
                        },
                        tenure: Some(tenure),
                        remotes: Vec::new(),
                    };
                }
            }
            let placement = match (stands.as_deref(), local) {
                (Some(stands), Some(local)) if stands != local => {
                    let basis = format!(
                        "{generation} is the current occupant of {position_ref} on Workcell {stands}; relayed to that Workcell's gateway only"
                    );
                    match declared
                        .remotes
                        .iter()
                        .find(|entry| entry.workcell_ref == stands)
                    {
                        Some(remote) => InstancePlacement::Relay {
                            remote: remote.clone(),
                            routing: None,
                            basis,
                        },
                        None => InstancePlacement::Undeclared {
                            workcell_ref: stands.to_owned(),
                            basis,
                        },
                    }
                }
                _ => InstancePlacement::Here {
                    basis: format!(
                        "actuation occupancy verify: {generation} is the current occupant of {position_ref}; delivered at that instance's next turn boundary and to no other generation"
                    ),
                },
            };
            return InstanceReading {
                placement,
                tenure: Some(tenure),
                remotes: Vec::new(),
            };
        }
        Ok(OccupancyVerdict::Refused(refusal)) if refusal.code == "occupancy.unknown_generation" => {
            format!("this Workcell's Actuation never knew {generation} for {position_ref}")
        }
        Ok(OccupancyVerdict::Refused(refusal)) => {
            return held(
                CommuniqueInstanceHold::InstanceSuperseded,
                format!(
                    "Actuation refuses {generation} for {position_ref} ({}: {}); an exact-instance Communique is never delivered to a successor, so it is held",
                    refusal.code, refusal.fact
                ),
            )
        }
        Err(unavailable) => {
            return held(
                CommuniqueInstanceHold::InstanceUnverified,
                format!(
                    "Actuation could not say whether {generation} is current for {position_ref} ({unavailable}); held, and delivered to no other occupant"
                ),
            )
        }
    };

    let candidates: Vec<GatewayRemote> = declared
        .remotes
        .iter()
        .filter(|remote| Some(remote.workcell_ref.as_str()) != local)
        .filter(|remote| required.is_none_or(|required| remote.workcell_ref == required))
        .cloned()
        .collect();
    if let (Some(required), true) = (required, candidates.is_empty()) {
        return held(
            CommuniqueInstanceHold::InstanceAbsent,
            if Some(required) == local {
                format!("{unknown_here}, and the required Workcell {required} is this one; held")
            } else {
                format!(
                    "{unknown_here}, and no gateway endpoint is declared for the required Workcell {required}, so it could not be asked; held ({})",
                    remote_command(required)
                )
            },
        );
    }
    let survey = survey_of(&candidates);
    let remotes = survey.statuses();
    let (mine, peers): (Vec<RemoteClaim>, Vec<RemoteClaim>) =
        survey.claims(position_ref).into_iter().partition(|claim| {
            claim.generation_ref.as_deref() == Some(generation)
                && required.is_none_or(|required| claim.remote.workcell_ref == required)
        });
    match mine.as_slice() {
        [claim] => InstanceReading {
            placement: InstancePlacement::Relay {
                remote: claim.remote.clone(),
                routing: Some(CommuniqueRouting {
                    workcell_ref: claim.remote.workcell_ref.clone(),
                    gateway_ref: claim.gateway_ref.clone(),
                    generation_ref: claim.generation_ref.clone(),
                    basis: format!(
                        "{unknown_here}; gateway {} of {} reports that exact instance current there",
                        claim.gateway_ref, claim.remote.workcell_ref
                    ),
                    observed_at_unix_ms: now_unix_ms(),
                }),
                basis: format!(
                    "{unknown_here}; {generation} is current on {} and the Communique is relayed to that Workcell's gateway only",
                    claim.remote.workcell_ref
                ),
            },
            tenure: Some(claim.tenure.clone()),
            remotes,
        },
        [] => {
            let mut parts = vec![unknown_here];
            if candidates.is_empty() {
                parts.push("no other Workcell is declared (`aikit gateway remote list`)".into());
            }
            if !peers.is_empty() {
                parts.push(format!(
                    "{position_ref} is held by other generations ({}), which are not the addressed instance and receive nothing",
                    peers.iter().map(RemoteClaim::describe).collect::<Vec<_>>().join(", ")
                ));
            }
            let unanswered = survey.unanswered();
            if !unanswered.is_empty() {
                parts.push(format!("could not ask {}", unanswered.join("; ")));
            }
            InstanceReading {
                placement: InstancePlacement::Held {
                    hold: CommuniqueInstanceHold::InstanceAbsent,
                    basis: format!("{}; held", parts.join("; ")),
                },
                tenure: None,
                remotes,
            }
        }
        _ => InstanceReading {
            placement: InstancePlacement::Ambiguous(ambiguous_occupancy(position_ref, &mine)),
            tenure: None,
            remotes,
        },
    }
}

fn instance_held_notice(
    position_ref: &str,
    instance: &CommuniqueInstance,
    hold: CommuniqueInstanceHold,
    basis: &str,
) -> Value {
    let on = instance
        .required_workcell_ref
        .as_deref()
        .map(|workcell| format!(" on Workcell {workcell}"))
        .unwrap_or_default();
    json!({
        "fact": format!("The exact instance {} of {position_ref} is not deliverable{on} now ({}): {basis}.", instance.generation_ref, hold.as_str()),
        "consequence": format!("The Communique is recorded held ({}) in this gateway's journal; it is delivered to no successor, peer or other Workcell.", hold.as_str()),
        "action": format!(
            "It is delivered only to {}{on}; every relay pass (gateway service tick, or `aikit gateway forward`) re-reads its standing. To reach whoever holds {position_ref} now, send without --instance.",
            instance.generation_ref
        ),
        "instance_hold": hold,
    })
}

/// How a recipient's occupancy routes a Communique: the one decision every
/// sender shares, exactly as the module doc states it. This Workcell's ledger
/// is read first; only a vacant recipient sends the declared remotes to survey.
pub(crate) struct OccupancyRouting {
    pub state: CommuniqueState,
    pub state_basis: String,
    pub occupant_workcell: Option<String>,
    pub delivery_notice: Option<Value>,
    pub remote: Option<GatewayRemote>,
    pub routing: Option<CommuniqueRouting>,
    pub remotes_asked: Vec<Value>,
}

/// The ONE reading of a recipient's occupancy that every durable-route
/// decider shares (#481-1): the local ledger answered with a current tenure,
/// answered vacant, or could not be read at all. What each caller does with
/// the answer differs (refuse before recording, relay now, queue for the
/// pass) — the answer itself does not.
enum TenureReading {
    Tenured(Value),
    Vacant,
    Unreadable(String),
}

fn read_tenure(owners: &dyn ContactOwners, position_ref: &str) -> TenureReading {
    match owners.occupancy_read(position_ref) {
        Ok(reading) => match current_tenure(&reading) {
            Some(tenure) => TenureReading::Tenured(tenure.clone()),
            None => TenureReading::Vacant,
        },
        Err(unavailable) => TenureReading::Unreadable(unavailable.to_string()),
    }
}

/// The ONE resolution of a vacant ledger against one survey of the declared
/// remotes: exactly one claim relays, none holds, several are ambiguous.
/// Send-time routing and the relay pass both resolve through this — never
/// through two private readings of the same claims.
enum VacantResolution {
    Relay {
        remote: GatewayRemote,
        routing: aikit_adapters::CommuniqueRouting,
        basis: String,
    },
    Hold {
        basis: String,
        notice: Value,
    },
    Ambiguous {
        refusal: AikitError,
    },
}

fn resolve_vacant(
    position_ref: &str,
    elsewhere: &[GatewayRemote],
    survey: &RemoteSurvey,
) -> VacantResolution {
    let claims = survey.claims(position_ref);
    match claims.as_slice() {
        [claim] => {
            let route = claim.routing(position_ref);
            let basis = format!(
                "{}; relayed to that Workcell's gateway, delivered at the occupant's next turn \
                 boundary there",
                route.basis
            );
            VacantResolution::Relay {
                remote: claim.remote.clone(),
                routing: route,
                basis,
            }
        }
        [] => {
            let (basis, notice) = vacant_everywhere(position_ref, elsewhere, survey);
            VacantResolution::Hold { basis, notice }
        }
        _ => VacantResolution::Ambiguous {
            refusal: ambiguous_occupancy(position_ref, &claims),
        },
    }
}

/// Route a resolved recipient by occupancy, or refuse before anything is
/// recorded (ambiguous occupancy, an occupant on an undeclared Workcell).
pub(crate) fn route_to_occupancy(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    local_workcell: Option<&str>,
    recipient: &Recipient,
) -> Result<OccupancyRouting> {
    let mut remote = None;
    let mut routing = None;
    let mut remotes_asked = Vec::new();
    let (state, state_basis, occupant_workcell, delivery_notice) =
        match read_tenure(owners, &recipient.position_ref) {
            TenureReading::Tenured(tenure) => {
                let generation = tenure
                    .get("generation_ref")
                    .and_then(Value::as_str)
                    .unwrap_or("an unnamed generation");
                (
                    CommuniqueState::Pending,
                    format!("{} is occupied by {generation}; delivered at its next turn boundary", recipient.position_ref),
                    tenure.get("workcell_ref").and_then(Value::as_str).map(str::to_owned),
                    None,
                )
            }
            TenureReading::Vacant => {
                let elsewhere = remotes_elsewhere(home, local_workcell)?;
                let survey = RemoteSurvey::ask(
                    &elsewhere,
                    &GatewayCommand::OccupancyRead {
                        position_ref: recipient.position_ref.clone(),
                    },
                );
                remotes_asked = survey.statuses();
                match resolve_vacant(&recipient.position_ref, &elsewhere, &survey) {
                    VacantResolution::Relay {
                        remote: entry,
                        routing: route,
                        basis,
                    } => {
                        remote = Some(entry.clone());
                        routing = Some(route.clone());
                        (
                            CommuniqueState::Pending,
                            basis,
                            Some(entry.workcell_ref.clone()),
                            None,
                        )
                    }
                    VacantResolution::Hold { basis, notice } => {
                        (CommuniqueState::Held, basis, None, Some(notice))
                    }
                    VacantResolution::Ambiguous { refusal } => return Err(refusal),
                }
            }
            TenureReading::Unreadable(unavailable) => (
                CommuniqueState::Pending,
                format!("occupancy of {} could not be read ({unavailable}); pending for whichever occupant takes its next turn here", recipient.position_ref),
                None,
                Some(json!({
                    "fact": format!("Actuation could not say who occupies {}: {unavailable}.", recipient.position_ref),
                    "consequence": "The Communique is recorded pending in this gateway's journal; it was not relayed to any other Workcell.",
                    "action": format!("It is delivered at the next turn boundary of an occupant reading this gateway; check occupancy with `actuation occupancy read --position {}`.", recipient.position_ref),
                })),
            ),
        };

    // This Workcell's ledger places the occupant on another Workcell: relay
    // there, or refuse before recording.
    if let (None, Some(occupant), Some(local)) = (&routing, &occupant_workcell, local_workcell) {
        if occupant != local {
            let remotes = load_remotes(home)?;
            match remotes
                .remotes
                .into_iter()
                .find(|entry| &entry.workcell_ref == occupant)
            {
                Some(entry) => remote = Some(entry),
                None => {
                    return Err(three_part(
                        "gateway.remote_undeclared",
                        format!(
                            "The occupant of {} stands on Workcell {occupant}, which is not reachable from here ({local}): no gateway endpoint is declared for it.",
                            recipient.position_ref
                        ),
                        nothing_sent(),
                        format!("Declare the endpoint: {}", remote_command(occupant)),
                    ));
                }
            }
        }
    }

    Ok(OccupancyRouting {
        state,
        state_basis,
        occupant_workcell,
        delivery_notice,
        remote,
        routing,
        remotes_asked,
    })
}

/// The recipient is a registered agent profile with no Position naming it:
/// no Workcell embodies the agency, so there is no occupancy to read and no
/// relay to survey. The record holds for the agency itself, at its identity —
/// the existing held law, minus the false "does not exist" refusal.
pub(crate) fn held_for_agency(recipient: &Recipient) -> OccupancyRouting {
    let agency_ref = recipient
        .agency_ref
        .clone()
        .unwrap_or_else(|| recipient.position_ref.clone());
    OccupancyRouting {
        state: CommuniqueState::Held,
        state_basis: format!(
            "{agency_ref} is a registered agent profile with no Position — not currently \
             embodied; held for the agency"
        ),
        occupant_workcell: None,
        delivery_notice: Some(json!({
            "fact": format!(
                "{agency_ref} is a registered agent profile and no Position names it: the \
                 agency is not currently embodied, and there is no occupancy to read."
            ),
            "consequence": "The Communique is recorded held in this gateway's journal at the agency's identity; nothing has been delivered yet.",
            "action": format!(
                "It stays held for the agency; follow it with `aikit gateway conversation \
                 --with {agency_ref}`, and `aikit gateway who --json` shows its embodiment \
                 state."
            ),
        })),
        remote: None,
        routing: None,
        remotes_asked: Vec::new(),
    }
}

pub fn send(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
    request: SendRequest<'_>,
) -> Result<Value> {
    if request.body.trim().is_empty() {
        return Err(three_part(
            "gateway.empty_body",
            "The Communique body is empty.",
            nothing_sent(),
            "Pass the words with --body TEXT or --body-file PATH.",
        ));
    }
    // The person occupies no Position: addressing them is a request in
    // Central's receiving ledger, which their Inbox reads.
    if owner_address::is_owner_address(request.to) {
        return owner_address::send_to_owner(owners, &request);
    }
    if request.require_workcell.is_some() && request.instance.is_none() {
        return Err(three_part(
            "gateway.require_workcell_without_instance",
            "--require-workcell binds an exact instance, and no --instance was named.",
            nothing_sent(),
            "Name the generation with --instance GENERATION_REF (read it from `aikit gateway who --json`), or drop --require-workcell for a durable Position route.",
        ));
    }
    let sender = resolve_sender(owners, request.from_position)?;
    let recipient = resolve_recipient(owners, request.to, request.project_world, cwd)?;
    let (local_workcell, workcell_basis) = local_workcell(owners, cwd);
    let now = now_unix_ms();

    // An exact-instance route never reaches the durable-occupancy decision
    // below: its placement is asked of Actuation directly (`occupancy verify`
    // here first, then only the required Workcell's remote), and a same-named
    // peer under another generation is never a route.
    if let Some(generation) = request.instance {
        let mut instance = CommuniqueInstance {
            generation_ref: generation.trim().to_owned(),
            required_workcell_ref: request.require_workcell.map(|w| w.trim().to_owned()),
            agent_session_ref: None,
            agency_ref: None,
        };
        if instance.generation_ref.is_empty()
            || instance.required_workcell_ref.as_deref() == Some("")
        {
            return Err(three_part(
                "gateway.invalid_instance",
                "--instance and --require-workcell must name refs, and one of them is empty.",
                nothing_sent(),
                "Read the current instance with `aikit gateway who --json` (occupancy.generation_ref, occupancy.workcell_ref).",
            ));
        }
        if recipient.agency_ref.is_some() {
            return Err(three_part(
                "gateway.instance_without_position",
                "An exact-instance route binds a Position's occupancy generation, and the recipient resolves to an agency with no Position.",
                nothing_sent(),
                "Send a durable route (omit --instance); it holds for the agency.",
            ));
        }
        let declared = load_remotes(home)?;
        let position_ref = recipient.position_ref.clone();
        let reading = place_instance(
            owners,
            &position_ref,
            &instance,
            local_workcell.as_deref(),
            &declared,
            &mut |candidates| {
                RemoteSurvey::ask(
                    candidates,
                    &GatewayCommand::OccupancyRead {
                        position_ref: position_ref.clone(),
                    },
                )
            },
        );
        if let Some(tenure) = &reading.tenure {
            let field = |key: &str| tenure.get(key).and_then(Value::as_str).map(str::to_owned);
            instance.agent_session_ref = field("agent_session_ref");
            instance.agency_ref = field("agency_ref");
        }
        let (state, state_basis, occupant_workcell, delivery, remote, routing, hold) =
            match reading.placement {
                InstancePlacement::Here { basis } => (
                    CommuniqueState::Pending,
                    basis,
                    reading.tenure.as_ref().and_then(tenure_workcell),
                    None,
                    None,
                    None,
                    None,
                ),
                InstancePlacement::Relay {
                    remote,
                    routing,
                    basis,
                } => (
                    CommuniqueState::Pending,
                    basis,
                    Some(remote.workcell_ref.clone()),
                    None,
                    Some(remote),
                    routing,
                    None,
                ),
                InstancePlacement::Held { hold, basis } => (
                    CommuniqueState::Held,
                    basis.clone(),
                    None,
                    Some(instance_held_notice(&position_ref, &instance, hold, &basis)),
                    None,
                    None,
                    Some(hold),
                ),
                InstancePlacement::Undeclared {
                    workcell_ref,
                    basis,
                } => {
                    return Err(three_part(
                        "gateway.remote_undeclared",
                        format!("{basis}, and Workcell {workcell_ref} is not reachable from here: no gateway endpoint is declared for it."),
                        nothing_sent(),
                        format!("Declare the endpoint: {}", remote_command(&workcell_ref)),
                    ))
                }
                InstancePlacement::Ambiguous(error) => return Err(error),
            };
        let draft = CommuniqueDraft {
            communique_ref: new_communique_ref(),
            from_position_ref: sender.from_position_ref.clone(),
            from_generation_ref: sender.from_generation_ref.clone(),
            attribution: sender.attribution,
            attribution_basis: sender.basis.clone(),
            to_position_ref: position_ref,
            to_workcell_ref: occupant_workcell,
            to_instance: Some(instance),
            instance_hold: hold,
            body: request.body,
            sent_at_unix_ms: now,
            state,
            state_basis,
            reply_to: request.reply_to,
            forward_to_workcell_ref: remote.as_ref().map(|entry| entry.workcell_ref.clone()),
            routing,
        };
        return accept_and_relay(
            home,
            gateway,
            draft,
            remote,
            json!({
                "recipient": recipient,
                "sender": sender,
                "route": "exact-instance",
                "local_workcell": { "ref": local_workcell, "basis": workcell_basis },
                "delivery": delivery,
                "remotes": reading.remotes,
            }),
        );
    }

    // Where the occupant stands, and how the record must be routed: the one
    // decision every sender shares (see the module doc). An agency with no
    // Position is not routed by occupancy at all — its mail holds for it.
    let OccupancyRouting {
        state,
        state_basis,
        occupant_workcell,
        delivery_notice,
        remote,
        routing,
        remotes_asked,
    } = match &recipient.agency_ref {
        Some(_) => held_for_agency(&recipient),
        None => route_to_occupancy(home, owners, local_workcell.as_deref(), &recipient)?,
    };

    let draft = CommuniqueDraft {
        communique_ref: new_communique_ref(),
        from_position_ref: sender.from_position_ref.clone(),
        from_generation_ref: sender.from_generation_ref.clone(),
        attribution: sender.attribution,
        attribution_basis: sender.basis.clone(),
        to_position_ref: recipient.position_ref.clone(),
        to_workcell_ref: occupant_workcell.clone(),
        to_instance: None,
        instance_hold: None,
        body: request.body,
        sent_at_unix_ms: now,
        state,
        state_basis,
        reply_to: request.reply_to,
        forward_to_workcell_ref: remote.as_ref().map(|entry| entry.workcell_ref.clone()),
        routing,
    };
    accept_and_relay(
        home,
        gateway,
        draft,
        remote,
        json!({
            "recipient": recipient,
            "sender": sender,
            "route": "position",
            "local_workcell": { "ref": local_workcell, "basis": workcell_basis },
            "delivery": delivery_notice,
            "remotes": remotes_asked,
        }),
    )
}

fn new_communique_ref() -> String {
    format!(
        "{COMMUNIQUE_REF_PREFIX}{}",
        ulid::Ulid::generate().to_string().to_ascii_lowercase()
    )
}

/// Append the draft, relay it when a remote was chosen, and answer the
/// receipt (`context` carries the route's own fields).
fn accept_and_relay(
    home: &AikitHome,
    gateway: &dyn GatewayAccess,
    draft: CommuniqueDraft,
    remote: Option<GatewayRemote>,
    context: Value,
) -> Result<Value> {
    accept_and_relay_via(home, gateway, draft, remote, context, &carrier_call)
}

fn accept_and_relay_via(
    home: &AikitHome,
    gateway: &dyn GatewayAccess,
    draft: CommuniqueDraft,
    remote: Option<GatewayRemote>,
    mut context: Value,
    carrier: RemoteCarrier<'_>,
) -> Result<Value> {
    let expected = draft.to_instance.clone();
    if expected.is_some() {
        // An exact-instance route is handed only to gateways that keep it:
        // this Workcell's (running service or state file) must advertise the
        // feature, and a relay target that answers without it is refused
        // before anything is recorded. A relay target that cannot be asked
        // now is asked again by the relay pass before it is ever handed the
        // record.
        if let ExactSupport::Missing(refusal) | ExactSupport::Unasked(refusal) =
            exact_instance_support(gateway.call(GatewayCommand::Protocol), "on this Workcell")
        {
            return Err(refusal);
        }
        if let Some(entry) = &remote {
            if let ExactSupport::Missing(refusal) = exact_instance_support(
                carrier(entry, GatewayCommand::Protocol),
                &format!("of Workcell {}", entry.workcell_ref),
            ) {
                return Err(refusal);
            }
        }
    }
    let (mut communique, replayed, accepted_by) =
        expect_accepted(gateway.call(GatewayCommand::SendCommunique {
            draft: Box::new(draft),
        })?)?;
    ensure_instance_kept(expected.as_ref(), &communique, "on this Workcell")?;
    let mut forward = Value::Null;
    if let Some(entry) = remote {
        let (record, report) = forward_one_via(
            home,
            gateway,
            &communique,
            &entry,
            &accepted_by,
            None,
            carrier,
        )?;
        communique = record;
        forward = report;
    }
    context["communique"] = serde_json::to_value(&communique).unwrap_or(Value::Null);
    context["replayed"] = json!(replayed);
    context["accepted_by"] = json!(accepted_by);
    context["forward"] = forward;
    Ok(context)
}

/// The recipient is vacant here and no declared Workcell reports an occupant:
/// the record's basis and the sender's three-part notice, naming exactly which
/// Workcells were asked and which could not answer.
fn vacant_everywhere(
    position_ref: &str,
    remotes: &[GatewayRemote],
    survey: &RemoteSurvey,
) -> (String, Value) {
    let vacant = survey.answered_vacant(position_ref);
    let unanswered = survey.unanswered();
    let elsewhere = if remotes.is_empty() {
        "no other Workcell is declared (`aikit gateway remote list`)".to_owned()
    } else {
        let mut parts = Vec::new();
        if !vacant.is_empty() {
            parts.push(format!("vacant on {}", vacant.join(", ")));
        }
        if !unanswered.is_empty() {
            parts.push(format!("could not ask {}", unanswered.join("; ")));
        }
        parts.join("; ")
    };
    let basis = format!(
        "{position_ref} is vacant on this Workcell and no declared Workcell reports an occupant ({elsewhere}); held for the next occupant"
    );
    let relay = if remotes.is_empty() {
        String::new()
    } else {
        " If an occupant appears on a declared Workcell (or one that could not be asked answers with one), the next relay pass (every gateway service tick, or `aikit gateway forward`) relays it there.".to_owned()
    };
    let notice = json!({
        "fact": format!("Position {position_ref} is vacant: Actuation on this Workcell records no current occupant, and {elsewhere}."),
        "consequence": "The Communique is recorded held in this gateway's journal; nothing has been delivered yet.",
        "action": format!(
            "It is delivered to the next occupant that claims {position_ref} at that occupant's first turn boundary.{relay} Follow it with `aikit gateway conversation --with {position_ref}`."
        ),
        "vacant_on": vacant,
        "unanswered": unanswered,
    });
    (basis, notice)
}

/// Relay one record to a declared remote gateway and record the outcome
/// locally. A remote that cannot be reached leaves the record queued; the
/// sender is never blocked on it.
/// One command to a declared remote Workcell's gateway.
type RemoteCarrier<'a> = &'a dyn Fn(&GatewayRemote, GatewayCommand) -> Result<GatewayResponse>;

/// The production remote carrier: the remote gateway's authenticated
/// WebSocket, its bearer token resolved from its declared location.
fn carrier_call(remote: &GatewayRemote, command: GatewayCommand) -> Result<GatewayResponse> {
    let token = SecretLocation::parse(&remote.token_location)?.resolve()?;
    let target = GatewayCarrierTarget::WebSocket {
        bind: remote.websocket_bind.clone(),
        path: remote.websocket_path.clone(),
        bearer_token: token.expose().to_owned(),
    };
    aikit_adapters::gateway_command(&target, command, None)
}

/// Whether a gateway keeps exact-instance bindings, from its `protocol`
/// answer.
enum ExactSupport {
    Advertised,
    /// It answered, and does not advertise the feature (or predates it).
    Missing(AikitError),
    /// It could not be asked.
    Unasked(AikitError),
}

fn exact_instance_support(answer: Result<GatewayResponse>, whose: &str) -> ExactSupport {
    let features = match answer {
        Ok(GatewayResponse::Protocol { features, .. }) => features,
        Ok(_) => Vec::new(),
        Err(error) => return ExactSupport::Unasked(error),
    };
    if features
        .iter()
        .any(|feature| feature == GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE)
    {
        return ExactSupport::Advertised;
    }
    ExactSupport::Missing(three_part(
        "gateway.exact_instance_unsupported",
        format!(
            "The gateway {whose} does not advertise `{GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE}` in its protocol answer: it predates exact-instance Communique routes and would silently drop the instance binding, turning the Communique into a durable Position route."
        ),
        "Nothing was handed to it.",
        format!(
            "Restart that gateway with this aikit (`aikit gateway protocol` must list `{GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE}`), or send a durable Position route by omitting --instance."
        ),
    ))
}

/// A gateway's echoed record must carry exactly the instance binding it was
/// handed; one that lost it is never reported as an exact-instance route.
fn ensure_instance_kept(
    expected: Option<&CommuniqueInstance>,
    echoed: &Communique,
    whose: &str,
) -> Result<()> {
    if echoed.to_instance.as_ref() == expected {
        return Ok(());
    }
    let describe = |instance: Option<&CommuniqueInstance>| {
        instance
            .map(|instance| instance.generation_ref.clone())
            .unwrap_or_else(|| "no instance (a durable Position route)".into())
    };
    Err(three_part(
        "gateway.communique_instance_binding_lost",
        format!(
            "The gateway {whose} answered for {} with {}, but it was handed {}: the exact-instance binding was lost.",
            echoed.communique_ref,
            describe(echoed.to_instance.as_ref()),
            describe(expected)
        ),
        "Its record there must not be taken as an exact-instance route, and no route is reported over it.",
        format!(
            "Upgrade that gateway (`aikit gateway upgrade apply` on its machine; `aikit gateway protocol` must list `{GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE}`), then read its record of {} with `aikit gateway conversation --with <the Position>`.",
            echoed.communique_ref
        ),
    ))
}

/// This gateway's signed sender assertion for one Communique it is about to
/// relay (#481-8): the signing key is generated on first use and stays in
/// this home; a gateway that cannot sign relays as before, named — never
/// silently upgraded or downgraded.
fn sender_attestation_for(
    home: &AikitHome,
    gateway_ref: &str,
    communique: &Communique,
    now_unix_ms: u64,
) -> Option<aikit_adapters::SenderAttestation> {
    match aikit_adapters::gateway_attestation::load_or_create_signing_key(&home.state()) {
        Ok(key) => match aikit_adapters::gateway_attestation::attest(
            &key,
            gateway_ref,
            communique,
            now_unix_ms,
        ) {
            Ok(proof) => Some(proof),
            Err(error) => {
                eprintln!("gateway relay: the sender attestation could not be signed: {error}");
                None
            }
        },
        Err(error) => {
            eprintln!("gateway relay: no signing key, the relay carries no attestation: {error}");
            None
        }
    }
}

fn forward_one(
    home: &AikitHome,
    gateway: &dyn GatewayAccess,
    communique: &Communique,
    remote: &GatewayRemote,
    local_gateway_ref: &str,
    routing: Option<aikit_adapters::CommuniqueRouting>,
    carrier: RemoteCarrier<'_>,
) -> Result<(Communique, Value)> {
    // The SAME carrier the pass was given: the durable route and the
    // exact-instance route are offered through one seam, so a test (or a
    // future caller) can stand in for the wire once, for both.
    forward_one_via(
        home,
        gateway,
        communique,
        remote,
        local_gateway_ref,
        routing,
        carrier,
    )
}

/// `forward_one` through a given remote carrier. An exact-instance record is
/// handed only to a remote that advertises the feature, and the remote's
/// echoed record must still carry the binding; either refusal is recorded as
/// a failed relay (the record stays queued here) and returned as the typed
/// error.
fn forward_one_via(
    home: &AikitHome,
    gateway: &dyn GatewayAccess,
    communique: &Communique,
    remote: &GatewayRemote,
    local_gateway_ref: &str,
    routing: Option<aikit_adapters::CommuniqueRouting>,
    carrier: RemoteCarrier<'_>,
) -> Result<(Communique, Value)> {
    let at = now_unix_ms();
    let whose = format!("of Workcell {}", remote.workcell_ref);
    let mut refusal: Option<AikitError> = None;
    let attempt = (|| {
        if communique.to_instance.is_some() {
            match exact_instance_support(carrier(remote, GatewayCommand::Protocol), &whose) {
                ExactSupport::Advertised => {}
                ExactSupport::Missing(missing) => {
                    refusal = Some(missing.clone());
                    return Err(missing);
                }
                ExactSupport::Unasked(error) => return Err(error),
            }
        }
        let accepted = expect_accepted(carrier(
            remote,
            GatewayCommand::IngestCommunique {
                communique: Box::new(communique.clone()),
                relayed_by: local_gateway_ref.to_owned(),
                attestation: sender_attestation_for(home, local_gateway_ref, communique, at),
            },
        )?)?;
        if let Err(lost) =
            ensure_instance_kept(communique.to_instance.as_ref(), &accepted.0, &whose)
        {
            refusal = Some(lost.clone());
            return Err(lost);
        }
        Ok(accepted)
    })();
    let outcome = match &attempt {
        Ok((_, _, remote_gateway_ref)) => CommuniqueForwardOutcome::Forwarded {
            workcell_ref: remote.workcell_ref.clone(),
            remote_gateway_ref: remote_gateway_ref.clone(),
            at_unix_ms: at,
        },
        Err(error) => CommuniqueForwardOutcome::Failed {
            workcell_ref: remote.workcell_ref.clone(),
            error: error.to_string(),
            at_unix_ms: at,
        },
    };
    let record = expect_record(gateway.call(GatewayCommand::RecordCommuniqueForward {
        communique_ref: communique.communique_ref.clone(),
        outcome,
        routing,
    })?)?;
    ensure_instance_kept(communique.to_instance.as_ref(), &record, "on this Workcell")?;
    if let Some(refusal) = refusal {
        return Err(refusal);
    }
    let report = match attempt {
        Ok((_, replayed, remote_gateway_ref)) => json!({
            "state": "forwarded",
            "workcell_ref": remote.workcell_ref,
            "remote_gateway_ref": remote_gateway_ref,
            "replayed": replayed,
        }),
        Err(error) => json!({
            "state": "queued",
            "workcell_ref": remote.workcell_ref,
            "fact": format!("The gateway of Workcell {} at {} could not take the Communique: {error}.", remote.workcell_ref, remote.websocket_bind),
            "consequence": "It is recorded here, queued for relay; the sender was not blocked and nothing was lost.",
            "action": "It is relayed on the next relay pass (every gateway service tick), or now with `aikit gateway forward`.",
        }),
    };
    Ok((record, report))
}

// ---------------------------------------------------------------------------
// The conversation engine's ask router (the connector edge's /ask)
// ---------------------------------------------------------------------------

/// The inverted hook the gateway service wires behind a connector
/// conversation's `/ask`: it resolves the ask with the exact `gateway send`
/// laws (this module's), and the engine does the appending — kernel work, in
/// process, against the journal the service owns. Nothing here shells out.
pub struct ContactAskRouter {
    pub home: AikitHome,
    pub cwd: PathBuf,
}

impl aikit_adapters::GatewayAskRouter for ContactAskRouter {
    fn route(&self, ask: &GatewayAskRequest) -> Result<GatewayAskRoute> {
        route_ask(&self.home, &self.cwd, ask)
    }

    fn relay(&self, communique: &Communique, relayed_by: &str) -> Result<CommuniqueForwardOutcome> {
        relay_appended(&self.home, communique, relayed_by)
    }
}

/// The asking agency's Position, named by occupancy — never by the chat. A
/// current tenure that carries this conversation's agent session verifies the
/// asker (`verified`); when the ledger cannot say that but the bound agency
/// holds exactly one occupied Position, that Position is named for it
/// (`claimed`); anything else delivers labelled `<unknown sender>`, with the
/// connector provenance carried beside whatever basis resolved.
fn resolve_ask_sender(owners: &dyn ContactOwners, ask: &GatewayAskRequest) -> SenderResolution {
    let unknown = |basis: String| SenderResolution {
        from_position_ref: None,
        from_generation_ref: None,
        attribution: SenderAttribution::Unknown,
        basis,
    };
    let listing = match owners.occupancy_list() {
        Ok(listing) => listing,
        Err(unavailable) => {
            return unknown(format!(
                "Actuation's occupancy ledger could not be read (`{}` failed: {}), so this \
                 conversation's sender could not be attributed from occupancy",
                unavailable.command, unavailable.reason
            ));
        }
    };
    let rows = listing
        .get("positions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    let mut by_session: Vec<(&Value, &Value)> = Vec::new();
    let mut by_agency: Vec<(&Value, &Value)> = Vec::new();
    for row in rows {
        let Some(tenure) = current_tenure(row) else {
            continue;
        };
        if tenure.get("agent_session_ref").and_then(Value::as_str)
            == Some(ask.agent_session_ref.as_str())
        {
            by_session.push((row, tenure));
        }
        if tenure.get("agency_ref").and_then(Value::as_str) == Some(ask.agency_ref.as_str()) {
            by_agency.push((row, tenure));
        }
    }
    let position_ref = |(row, _): &(&Value, &Value)| {
        row.get("position_ref")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let generation_ref = |(_, tenure): &(&Value, &Value)| {
        tenure
            .get("generation_ref")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    // One session match is the verification the send path requires: the
    // ledger itself names this conversation's session as a current occupant.
    if let [match_] = by_session.as_slice() {
        let position = position_ref(match_);
        let generation = generation_ref(match_);
        return SenderResolution {
            from_position_ref: Some(position.clone()),
            from_generation_ref: generation.clone(),
            attribution: SenderAttribution::Verified,
            basis: format!(
                "actuation occupancy list: {} is the current occupant of {position} and its \
                 tenure carries this conversation's agent session {}",
                generation.as_deref().unwrap_or("an unnamed generation"),
                ask.agent_session_ref
            ),
        };
    }
    // The bound agency holds exactly one occupied Position: it is named for
    // the asking agency, claimed rather than verified — this conversation's
    // session is not the tenure the ledger names.
    if by_session.is_empty() {
        if let [match_] = by_agency.as_slice() {
            let position = position_ref(match_);
            let generation = generation_ref(match_);
            return SenderResolution {
                from_position_ref: Some(position.clone()),
                from_generation_ref: generation.clone(),
                attribution: SenderAttribution::Claimed,
                basis: format!(
                    "the asking agency {} holds exactly one occupied Position, {position}; the \
                     conversation is attributed to it as claimed, not verified against this \
                     conversation's agent session {}",
                    ask.agency_ref, ask.agent_session_ref
                ),
            };
        }
        if by_agency.len() > 1 {
            return unknown(format!(
                "the asking agency {} holds several occupied Positions ({}), and a connector \
                 conversation names none of them",
                ask.agency_ref,
                by_agency
                    .iter()
                    .map(position_ref)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    unknown(format!(
        "no current occupant in Actuation's ledger names this conversation's agent session {} \
         or its agency {}",
        ask.agent_session_ref, ask.agency_ref
    ))
}

/// Resolve one connector-originated ask into an accepted-shaped draft: the
/// sender attributed from occupancy, the recipient resolved by Central, the
/// route taken from occupancy (`route_to_occupancy` — the same decision
/// `gateway send` makes), and the origin provenance — asking agent session,
/// bound agency, connector conversation — carried in the attribution basis.
/// Every refusal answers before anything is appended.
///
/// This is the production resolution `ContactAskRouter` serves; the owners are
/// a parameter so the same laws can be proven against fixture owners.
pub fn route_ask_with_owners(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    cwd: &Path,
    ask: &GatewayAskRequest,
) -> Result<GatewayAskRoute> {
    if ask.message.trim().is_empty() {
        return Err(three_part(
            "gateway.empty_body",
            "The ask's message is empty.",
            nothing_sent(),
            "Ask with /ask <position-ref-or-@handle> <message>.",
        ));
    }
    let (local_workcell, _) = local_workcell(owners, cwd);
    let sender = resolve_ask_sender(owners, ask);
    let recipient = resolve_recipient(owners, &ask.recipient, None, cwd)?;
    let OccupancyRouting {
        state,
        state_basis,
        occupant_workcell,
        delivery_notice,
        remote,
        routing,
        remotes_asked: _,
    } = match &recipient.agency_ref {
        Some(_) => held_for_agency(&recipient),
        None => route_to_occupancy(home, owners, local_workcell.as_deref(), &recipient)?,
    };
    let attribution_basis = format!(
        "{}; asked from connector conversation {} on {} (binding {}, agent session {}, agency \
         {})",
        sender.basis,
        ask.conversation_id,
        ask.platform,
        ask.binding_ref,
        ask.agent_session_ref,
        ask.agency_ref
    );
    let delivery = delivery_notice.unwrap_or_else(|| {
        json!({
            "fact": state_basis,
            "consequence": "The Communique is recorded in this gateway's journal; nothing has been delivered yet.",
            "action": format!("It is delivered at the recipient occupant's next turn boundary; follow it with `aikit gateway conversation --with {}`.", recipient.position_ref),
        })
    });
    Ok(GatewayAskRoute {
        draft: CommuniqueDraft {
            communique_ref: format!(
                "{COMMUNIQUE_REF_PREFIX}{}",
                ulid::Ulid::generate().to_string().to_ascii_lowercase()
            ),
            from_position_ref: sender.from_position_ref.clone(),
            from_generation_ref: sender.from_generation_ref.clone(),
            attribution: sender.attribution,
            attribution_basis,
            to_position_ref: recipient.position_ref.clone(),
            to_workcell_ref: occupant_workcell,
            to_instance: None,
            instance_hold: None,
            body: ask.message.clone(),
            sent_at_unix_ms: now_unix_ms(),
            state,
            state_basis,
            reply_to: None,
            forward_to_workcell_ref: remote.as_ref().map(|entry| entry.workcell_ref.clone()),
            routing,
        },
        recipient_position_ref: recipient.position_ref,
        delivery,
    })
}

fn route_ask(home: &AikitHome, cwd: &Path, ask: &GatewayAskRequest) -> Result<GatewayAskRoute> {
    let owners = crate::gateway_owners::ProcessOwners::from_env();
    route_ask_with_owners(home, &owners, cwd, ask)
}

/// One relay attempt of an already-appended ask to the Workcell its route
/// names. A remote that cannot be reached is a `Failed` outcome — the record
/// stays queued for the next relay pass — never an error; a route naming no
/// declared Workcell endpoint is.
/// One relay attempt to a declared remote: resolve its token, offer the
/// Communique to its gateway. Answers `(replayed, remote gateway ref)`. An
/// exact-instance record is handed only to a remote that advertises the
/// feature, and the remote's echo must still carry the binding.
fn relay_attempt(
    home: &AikitHome,
    remote: &GatewayRemote,
    communique: &Communique,
    relayed_by: &str,
) -> Result<(bool, String)> {
    let whose = format!("of Workcell {}", remote.workcell_ref);
    let carrier = |command: GatewayCommand| -> Result<GatewayResponse> {
        let token = SecretLocation::parse(&remote.token_location)?.resolve()?;
        let target = GatewayCarrierTarget::WebSocket {
            bind: remote.websocket_bind.clone(),
            path: remote.websocket_path.clone(),
            bearer_token: token.expose().to_owned(),
        };
        aikit_adapters::gateway_command(&target, command, None)
    };
    if communique.to_instance.is_some() {
        match exact_instance_support(carrier(GatewayCommand::Protocol), &whose) {
            ExactSupport::Advertised => {}
            ExactSupport::Missing(missing) => return Err(missing),
            ExactSupport::Unasked(error) => return Err(error),
        }
    }
    let (communique, replayed, remote_gateway_ref) =
        expect_accepted(carrier(GatewayCommand::IngestCommunique {
            communique: Box::new(communique.clone()),
            relayed_by: relayed_by.to_owned(),
            attestation: sender_attestation_for(home, relayed_by, communique, now_unix_ms()),
        })?)?;
    ensure_instance_kept(communique.to_instance.as_ref(), &communique, &whose)?;
    Ok((replayed, remote_gateway_ref))
}

fn relay_appended(
    home: &AikitHome,
    communique: &Communique,
    relayed_by: &str,
) -> Result<CommuniqueForwardOutcome> {
    let workcell_ref = match &communique.forward {
        Some(CommuniqueForward::Queued { workcell_ref, .. }) => Some(workcell_ref.clone()),
        Some(CommuniqueForward::Forwarded { .. }) => None,
        None => communique
            .routing
            .as_ref()
            .map(|routing| routing.workcell_ref.clone()),
    }
    .ok_or_else(|| {
        AikitError::new(
            "gateway.remote_unnamed",
            format!(
                "{} names no Workcell to relay to; it is recorded here undelivered",
                communique.communique_ref
            ),
        )
    })?;
    let remotes = load_remotes(home)?;
    let entry = remotes
        .remotes
        .iter()
        .find(|entry| entry.workcell_ref == workcell_ref)
        .ok_or_else(|| {
            three_part(
                "gateway.remote_undeclared",
                format!(
                    "The route of {} names Workcell {workcell_ref}, which is not reachable from here: no gateway endpoint is declared for it.",
                    communique.communique_ref
                ),
                "The Communique is recorded and stays queued for relay; nothing was lost.",
                format!("Declare the endpoint: {}", remote_command(&workcell_ref)),
            )
        })?;
    let at = now_unix_ms();
    Ok(match relay_attempt(home, entry, communique, relayed_by) {
        Ok((_, remote_gateway_ref)) => CommuniqueForwardOutcome::Forwarded {
            workcell_ref,
            remote_gateway_ref,
            at_unix_ms: at,
        },
        Err(error) => CommuniqueForwardOutcome::Failed {
            workcell_ref,
            error: error.to_string(),
            at_unix_ms: at,
        },
    })
}

// ---------------------------------------------------------------------------
// forward (the relay pass)
// ---------------------------------------------------------------------------

/// Where this Workcell's own ledger places a Position's occupant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LedgerPlacement {
    /// Occupied here, or occupied with no Workcell named: delivered here.
    Here,
    /// This ledger names another Workcell for the current tenure.
    Elsewhere(String),
    /// No current occupant in this ledger.
    Vacant,
    /// Actuation could not answer.
    Unknown,
}

fn ledger_placement(
    owners: &dyn ContactOwners,
    position_ref: &str,
    local: Option<&str>,
) -> LedgerPlacement {
    // The SAME reading `route_to_occupancy` makes at send time (#481-1): one
    // occupancy answer, one tenure shape — the pass never decides from a
    // second private reading of the ledger.
    match read_tenure(owners, position_ref) {
        TenureReading::Tenured(tenure) => {
            match (tenure.get("workcell_ref").and_then(Value::as_str), local) {
                (Some(workcell), Some(local)) if workcell != local => {
                    LedgerPlacement::Elsewhere(workcell.to_owned())
                }
                _ => LedgerPlacement::Here,
            }
        }
        TenureReading::Vacant => LedgerPlacement::Vacant,
        TenureReading::Unreadable(_) => LedgerPlacement::Unknown,
    }
}

/// Re-evaluate every Communique this gateway still has to deliver, the same
/// way `send` routes a new one: when this Workcell's ledger places the
/// recipient's occupant on a declared remote Workcell, relay there; when this
/// ledger records no occupant, ask the declared remotes (once per pass) and
/// relay to the one that reports a current occupant. A held Communique thus
/// reaches a recipient who occupies later on another machine; one queued for
/// a remote that was down is retried.
pub fn forward_pass(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
) -> Result<Value> {
    forward_pass_via(home, owners, gateway, cwd, &carrier_call)
}

fn forward_pass_via(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
    carrier: RemoteCarrier<'_>,
) -> Result<Value> {
    let queue = expect_list(gateway.call(GatewayCommand::CommuniqueForwardQueue)?)?;
    let (local, basis) = local_workcell(owners, cwd);
    let local_gateway_ref = match gateway.call(GatewayCommand::Status)? {
        GatewayResponse::Status { status } => status.gateway_ref.to_string(),
        other => return Err(unexpected(&other)),
    };
    let remotes = load_remotes(home)?;
    let elsewhere = remotes_elsewhere(home, local.as_deref())?;
    let mut placements: BTreeMap<String, LedgerPlacement> = BTreeMap::new();
    // The remotes are surveyed at most once per pass, and only when some
    // Communique's recipient is vacant here.
    let mut survey: Option<RemoteSurvey> = None;
    let (mut forwarded, mut queued, mut skipped, mut held, mut ambiguous, mut restood) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    for record in &queue {
        if let Some(instance) = &record.to_instance {
            // Each exact-instance Communique stands or falls on its own: a
            // gateway that cannot record one standing (an older service that
            // does not know the command, or a record that stopped being
            // deliverable meanwhile) or a relay refused for it is reported in
            // `skipped` with its reason, and the pass goes on.
            let outcome = (|| -> Result<()> {
                let reading = place_instance(
                    owners,
                    &record.to_position_ref,
                    instance,
                    local.as_deref(),
                    &remotes,
                    &mut |_| {
                        survey
                            .get_or_insert_with(|| {
                                RemoteSurvey::ask(&elsewhere, &GatewayCommand::OccupancyList)
                            })
                            .clone()
                    },
                );
                let mut restand = |state, hold, basis: &str| -> Result<Communique> {
                    let record =
                        expect_record(gateway.call(GatewayCommand::RecordCommuniqueStanding {
                            communique_ref: record.communique_ref.clone(),
                            state,
                            instance_hold: hold,
                            at_unix_ms: now_unix_ms(),
                            basis: basis.to_owned(),
                        })?)?;
                    restood.push(json!({
                        "communique_ref": record.communique_ref,
                        "state": record.state,
                        "instance_hold": record.instance_hold,
                    }));
                    Ok(record)
                };
                let changes = |state, hold: Option<CommuniqueInstanceHold>| {
                    record.state != state || record.instance_hold != hold
                };
                let (entry, routing, current) = match reading.placement {
                    InstancePlacement::Here { basis } => {
                        if changes(CommuniqueState::Pending, None) {
                            restand(CommuniqueState::Pending, None, &basis)?;
                        }
                        return Ok(());
                    }
                    InstancePlacement::Relay {
                        remote,
                        routing,
                        basis,
                    } => {
                        let current = if changes(CommuniqueState::Pending, None) {
                            restand(CommuniqueState::Pending, None, &basis)?
                        } else {
                            record.clone()
                        };
                        (remote, routing, current)
                    }
                    InstancePlacement::Undeclared {
                        workcell_ref,
                        basis,
                    } => {
                        skipped.push(json!({
                            "communique_ref": record.communique_ref,
                            "workcell_ref": workcell_ref,
                            "basis": basis,
                            "action": format!("Declare the endpoint: {}", remote_command(&workcell_ref)),
                        }));
                        return Ok(());
                    }
                    // Not knowing never overwrites what was known.
                    InstancePlacement::Held {
                        hold: CommuniqueInstanceHold::InstanceUnverified,
                        ..
                    } => return Ok(()),
                    InstancePlacement::Held { hold, basis } => {
                        if changes(CommuniqueState::Held, Some(hold)) {
                            restand(CommuniqueState::Held, Some(hold), &basis)?;
                        }
                        held.push(json!({
                            "communique_ref": record.communique_ref,
                            "position_ref": record.to_position_ref,
                            "instance": instance,
                            "instance_hold": hold,
                            "basis": basis,
                        }));
                        return Ok(());
                    }
                    InstancePlacement::Ambiguous(refusal) => {
                        ambiguous.push(json!({
                            "communique_ref": record.communique_ref,
                            "position_ref": record.to_position_ref,
                            "fact": refusal.details().get("fact"),
                            "consequence": "It stays in this gateway's journal, undelivered; nothing was relayed.",
                            "action": refusal.details().get("action"),
                        }));
                        return Ok(());
                    }
                };
                let (relayed, report) = forward_one_via(
                    home,
                    gateway,
                    &current,
                    &entry,
                    &local_gateway_ref,
                    routing,
                    carrier,
                )?;
                if report["state"] == "forwarded" {
                    forwarded.push(json!({
                        "communique_ref": relayed.communique_ref,
                        "workcell_ref": entry.workcell_ref,
                        "routing": relayed.routing,
                        "instance": relayed.to_instance,
                    }));
                } else {
                    queued
                        .push(json!({"communique_ref": relayed.communique_ref, "report": report}));
                }
                Ok(())
            })();
            if let Err(error) = outcome {
                skipped.push(json!({
                    "communique_ref": record.communique_ref,
                    "code": error.code(),
                    "reason": error.to_string(),
                }));
            }
            continue;
        }
        let placement = placements
            .entry(record.to_position_ref.clone())
            .or_insert_with(|| ledger_placement(owners, &record.to_position_ref, local.as_deref()))
            .clone();
        let (entry, routing) = match placement {
            LedgerPlacement::Here | LedgerPlacement::Unknown => continue,
            LedgerPlacement::Elsewhere(workcell) => {
                match remotes
                    .remotes
                    .iter()
                    .find(|entry| entry.workcell_ref == workcell)
                {
                    Some(entry) => (entry.clone(), None),
                    None => {
                        skipped.push(json!({
                            "communique_ref": record.communique_ref,
                            "workcell_ref": workcell,
                            "action": format!("Declare the endpoint: {}", remote_command(&workcell)),
                        }));
                        continue;
                    }
                }
            }
            LedgerPlacement::Vacant => {
                if elsewhere.is_empty() {
                    continue;
                }
                let survey = survey.get_or_insert_with(|| {
                    RemoteSurvey::ask(&elsewhere, &GatewayCommand::OccupancyList)
                });
                // The SAME vacant resolution send-time routing makes (#481-1):
                // one claim relays, none holds, several are ambiguous.
                match resolve_vacant(&record.to_position_ref, &elsewhere, survey) {
                    VacantResolution::Relay {
                        remote, routing, ..
                    } => (remote, Some(routing)),
                    VacantResolution::Hold { .. } => {
                        held.push(json!({
                            "communique_ref": record.communique_ref,
                            "position_ref": record.to_position_ref,
                            "vacant_on": survey.answered_vacant(&record.to_position_ref),
                            "unanswered": survey.unanswered(),
                        }));
                        continue;
                    }
                    VacantResolution::Ambiguous { refusal } => {
                        ambiguous.push(json!({
                            "communique_ref": record.communique_ref,
                            "position_ref": record.to_position_ref,
                            "fact": refusal.details().get("fact"),
                            "consequence": "It stays in this gateway's journal, undelivered; nothing was relayed.",
                            "action": refusal.details().get("action"),
                        }));
                        continue;
                    }
                }
            }
        };
        let (record, report) = forward_one(
            home,
            gateway,
            record,
            &entry,
            &local_gateway_ref,
            routing,
            carrier,
        )?;
        if report["state"] == "forwarded" {
            forwarded.push(json!({
                "communique_ref": record.communique_ref,
                "workcell_ref": entry.workcell_ref,
                "routing": record.routing,
            }));
        } else {
            queued.push(json!({"communique_ref": record.communique_ref, "report": report}));
        }
    }
    let readback = readback_forwarded(gateway, carrier, &remotes, now_unix_ms())?;
    Ok(json!({
        "considered": queue.len(),
        "local_workcell": { "ref": local, "basis": basis },
        "forwarded": forwarded,
        "queued": queued,
        "skipped": skipped,
        "held": held,
        "ambiguous": ambiguous,
        "restood": restood,
        "readback": readback,
        "remotes": survey.map(|survey| survey.statuses()).unwrap_or_default(),
    }))
}

/// The sender side of remote delivery: every Communique this gateway relayed
/// and still counts as pending is read back from the gateway it was relayed
/// to. When that gateway has recorded the delivery, the sender copy learns
/// it (state `delivered`, basis naming the remote and the readback); when it
/// has not, the copy stays exactly as it was and the pass says so. A readback
/// never marks anything delivered on its own authority — only the remote's
/// own record does.
fn readback_forwarded(
    gateway: &dyn GatewayAccess,
    carrier: RemoteCarrier<'_>,
    remotes: &GatewayRemotes,
    at: u64,
) -> Result<Value> {
    let mut learned = Vec::new();
    let mut unresolved = Vec::new();
    // One survey of the queue; the forwarded subset is what a readback means.
    let queue = expect_list(gateway.call(GatewayCommand::CommuniqueForwardQueue)?)?;
    for record in queue.iter().filter(|record| {
        record.state.is_undelivered()
            && matches!(record.forward, Some(CommuniqueForward::Forwarded { .. }))
    }) {
        let Some(CommuniqueForward::Forwarded {
            workcell_ref,
            remote_gateway_ref,
            ..
        }) = &record.forward
        else {
            continue;
        };
        let Some(entry) = remotes
            .remotes
            .iter()
            .find(|entry| &entry.workcell_ref == workcell_ref)
        else {
            unresolved.push(json!({
                "communique_ref": record.communique_ref,
                "workcell_ref": workcell_ref,
                "reason": "the Workcell it was relayed to is no longer declared here",
            }));
            continue;
        };
        let fate = match carrier(
            entry,
            GatewayCommand::CommuniqueFate {
                communique_ref: record.communique_ref.clone(),
            },
        ) {
            Ok(GatewayResponse::CommuniqueFate { fate, .. }) => fate,
            Ok(other) => {
                unresolved.push(json!({
                    "communique_ref": record.communique_ref,
                    "remote_gateway_ref": remote_gateway_ref,
                    "reason": format!("the remote answered the readback unexpectedly: {}", unexpected(&other)),
                }));
                continue;
            }
            Err(error) => {
                unresolved.push(json!({
                    "communique_ref": record.communique_ref,
                    "remote_gateway_ref": remote_gateway_ref,
                    "reason": error.to_string(),
                }));
                continue;
            }
        };
        match fate {
            Some(fate) if fate.state == CommuniqueState::Delivered => {
                let basis = format!(
                    "delivered at Workcell {workcell_ref} through gateway {remote_gateway_ref}; \
                     read back on the relay pass"
                );
                gateway.call(GatewayCommand::RecordRemoteDelivery {
                    communique_ref: record.communique_ref.clone(),
                    at_unix_ms: at,
                    basis,
                    delivered_to_generation_ref: fate.delivered_to_generation_ref.clone(),
                })?;
                learned.push(json!({
                    "communique_ref": record.communique_ref,
                    "workcell_ref": workcell_ref,
                    "remote_gateway_ref": remote_gateway_ref,
                    "delivered_to_generation_ref": fate.delivered_to_generation_ref,
                }));
            }
            Some(fate) => {
                unresolved.push(json!({
                    "communique_ref": record.communique_ref,
                    "remote_gateway_ref": remote_gateway_ref,
                    "reason": format!("the remote records it {}", fate.state.as_str()),
                }));
            }
            None => {
                unresolved.push(json!({
                    "communique_ref": record.communique_ref,
                    "remote_gateway_ref": remote_gateway_ref,
                    "reason": "the remote holds no such record",
                }));
            }
        }
    }
    Ok(json!({
        "learned_delivery": learned,
        "unresolved": unresolved,
    }))
}

// ---------------------------------------------------------------------------
// inbox / conversation
// ---------------------------------------------------------------------------

/// The occupant a read or acknowledgement speaks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occupant {
    pub position_ref: String,
    pub generation_ref: Option<String>,
    pub verified: bool,
    pub basis: String,
    /// Where a verified generation stands: its tenure's Workcell, else this
    /// home's Workcell. `None` when unverified or unknown.
    pub workcell_ref: Option<String>,
}

pub fn resolve_occupant(
    owners: &dyn ContactOwners,
    cwd: &Path,
    explicit: Option<&str>,
) -> Result<Occupant> {
    let position = explicit
        .map(str::to_owned)
        .or_else(|| env_value(POSITION_ENV))
        .ok_or_else(|| {
            three_part(
                "gateway.no_position",
                format!("No Position was named and this body carries no {POSITION_ENV}."),
                "Nothing was read.",
                "Name the Position with --position central:position:<world>:<slug>.",
            )
        })?;
    let from_env = env_value(POSITION_ENV).as_deref() == Some(position.as_str());
    let generation = from_env.then(|| env_value(GENERATION_ENV)).flatten();
    let Some(generation) = generation else {
        return Ok(Occupant {
            position_ref: position,
            generation_ref: None,
            verified: false,
            basis: format!("no {GENERATION_ENV} for this Position; read-only"),
            workcell_ref: None,
        });
    };
    match owners.occupancy_verify(&position, &generation) {
        Ok(OccupancyVerdict::Current(tenure)) => Ok(Occupant {
            position_ref: position,
            generation_ref: Some(generation),
            verified: true,
            basis: "actuation occupancy verify".into(),
            workcell_ref: tenure_workcell(&tenure).or_else(|| local_workcell(owners, cwd).0),
        }),
        Ok(OccupancyVerdict::Refused(refusal)) => Ok(Occupant {
            position_ref: position,
            generation_ref: Some(generation),
            verified: false,
            basis: format!(
                "Actuation refuses this generation ({}): {}",
                refusal.code, refusal.fact
            ),
            workcell_ref: None,
        }),
        Err(unavailable) => Ok(Occupant {
            position_ref: position,
            generation_ref: Some(generation),
            verified: false,
            basis: format!("occupancy could not be verified: {unavailable}"),
            workcell_ref: None,
        }),
    }
}

pub fn inbox(
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
    position: Option<&str>,
    ack: bool,
) -> Result<Value> {
    let occupant = resolve_occupant(owners, cwd, position)?;
    // An exact-instance Communique is this reader's only when the reader is
    // its verified instance on the Workcell it requires.
    let mine = |record: &Communique| match (&occupant.generation_ref, occupant.verified) {
        (Some(generation), true) => {
            record.deliverable_to(generation, occupant.workcell_ref.as_deref())
        }
        _ => record.to_instance.is_none(),
    };
    let records = expect_list(gateway.call(GatewayCommand::CommuniqueInbox {
        position_ref: occupant.position_ref.clone(),
    })?)?;
    let occupied_now = occupant.verified
        || owners
            .occupancy_read(&occupant.position_ref)
            .ok()
            .is_some_and(|reading| current_tenure(&reading).is_some());
    let annotate = |record: &Communique| {
        let mut value = serde_json::to_value(record).unwrap_or(Value::Null);
        value["deliverable"] = json!(match (&record.to_instance, record.state, occupied_now) {
            (Some(_), _, _) if occupant.verified && mine(record) => "pending".to_owned(),
            (Some(_), _, _) => match record.instance_hold {
                Some(hold) => format!("held-{}", hold.as_str()),
                None => "awaiting-instance".to_owned(),
            },
            (None, CommuniqueState::Held, true) => "held-now-deliverable".to_owned(),
            (None, CommuniqueState::Held, false) => "held-awaiting-occupant".to_owned(),
            (None, _, _) => "pending".to_owned(),
        });
        value
    };
    if !ack {
        return Ok(json!({
            "position_ref": occupant.position_ref,
            "occupant": occupant,
            "communiques": records.iter().map(annotate).collect::<Vec<_>>(),
            "acknowledged": false,
        }));
    }
    let Some(generation) = occupant
        .generation_ref
        .clone()
        .filter(|_| occupant.verified)
    else {
        return Err(three_part(
            "gateway.ack_requires_current_occupant",
            format!(
                "Acknowledging marks delivery to a verified current occupant of {}, and this body is not one ({}).",
                occupant.position_ref, occupant.basis
            ),
            "Nothing was marked delivered.",
            format!("Run from the occupying body (with {POSITION_ENV} and {GENERATION_ENV}), or read without --ack."),
        ));
    };
    let (deliverable, withheld): (Vec<&Communique>, Vec<&Communique>) =
        records.iter().partition(|record| mine(record));
    let refs: Vec<String> = deliverable
        .iter()
        .map(|record| record.communique_ref.clone())
        .collect();
    let withheld: Vec<Value> = withheld.into_iter().map(annotate).collect();
    let delivered = if refs.is_empty() {
        Vec::new()
    } else {
        expect_list(gateway.call(GatewayCommand::AcknowledgeCommuniques {
            position_ref: occupant.position_ref.clone(),
            generation_ref: generation,
            workcell_ref: occupant.workcell_ref.clone(),
            communique_refs: refs,
            delivered_at_unix_ms: now_unix_ms(),
            via: "by `aikit gateway inbox --ack`".into(),
        })?)?
    };
    Ok(json!({
        "position_ref": occupant.position_ref,
        "occupant": occupant,
        "communiques": delivered,
        "withheld": withheld,
        "acknowledged": true,
    }))
}

pub fn conversation(
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
    position: Option<&str>,
    with: &str,
    project_world: Option<&str>,
) -> Result<Value> {
    let own = position
        .map(str::to_owned)
        .or_else(|| env_value(POSITION_ENV))
        .ok_or_else(|| {
            three_part(
                "gateway.no_position",
                format!("No Position was named and this body carries no {POSITION_ENV}."),
                "Nothing was read.",
                "Name your side with --position central:position:<world>:<slug>.",
            )
        })?;
    let with_ref = if with.starts_with("central:position:") {
        with.to_owned()
    } else {
        resolve_recipient(owners, with, project_world, cwd)?.position_ref
    };
    let records = expect_list(gateway.call(GatewayCommand::CommuniqueConversation {
        position_ref: own.clone(),
        with_position_ref: with_ref.clone(),
    })?)?;
    Ok(json!({
        "position_ref": own,
        "with_position_ref": with_ref,
        "communiques": records,
    }))
}

// ---------------------------------------------------------------------------
// delegate
// ---------------------------------------------------------------------------

pub struct DelegateRequest<'a> {
    pub communique_ref: &'a str,
    pub work_ref: &'a str,
    pub run_ref: Option<String>,
    pub journey_ref: Option<String>,
    pub workflow_unit_ref: Option<String>,
    pub reason: &'a str,
}

/// The explicit crossing: Factory assigns custody of `work_ref` to the
/// Communique's recipient Position, then the journal records the custody ref
/// and the Communique becomes `escalated`. A Factory refusal leaves the
/// Communique exactly as it was.
pub fn delegate(
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
    request: DelegateRequest<'_>,
) -> Result<Value> {
    let record = expect_record(gateway.call(GatewayCommand::ReadCommunique {
        communique_ref: request.communique_ref.to_owned(),
    })?)?;
    if record.state == CommuniqueState::Escalated {
        return Err(three_part(
            "gateway.already_escalated",
            format!(
                "{} was already escalated into {}.",
                record.communique_ref,
                record.escalated_custody_ref.as_deref().unwrap_or("custody")
            ),
            "No second custody was requested.",
            format!(
                "Read the custody with `factory development custody list --position {}`.",
                record.to_position_ref
            ),
        ));
    }
    let assign = CustodyAssign {
        position_ref: record.to_position_ref.clone(),
        work_ref: request.work_ref.to_owned(),
        reason: request.reason.to_owned(),
        run_ref: request.run_ref,
        journey_ref: request.journey_ref,
        workflow_unit_ref: request.workflow_unit_ref,
        origin_communique_ref: record.communique_ref.clone(),
    };
    let receipt = match owners.custody_assign(&assign, cwd) {
        Ok(Ok(receipt)) => receipt,
        Ok(Err(refusal)) => {
            return Err(three_part(
                "gateway.custody_refused",
                format!("Factory refused the custody ({}): {}", refusal.code, refusal.fact),
                format!(
                    "{} The Communique stays {}.",
                    if refusal.consequence.is_empty() {
                        "No custody was created."
                    } else {
                        refusal.consequence.as_str()
                    },
                    record.state.as_str()
                ),
                if refusal.action.is_empty() {
                    format!("Correct the request and re-run `{}`.", refusal.command)
                } else {
                    refusal.action
                },
            ))
        }
        Err(unavailable) => {
            return Err(three_part(
                "gateway.owner_unavailable",
                format!("Factory could not assign custody: `{}` failed ({}).", unavailable.command, unavailable.reason),
                format!("No custody was created; the Communique stays {}.", record.state.as_str()),
                "Run from inside the Factory project (or make `factory development custody` answer), then retry.",
            ))
        }
    };
    let custody_ref = receipt
        .pointer("/custody/custody_ref")
        .or_else(|| receipt.get("custody_ref"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "gateway.custody_receipt_invalid",
                "Factory answered the assignment without a custody_ref; the Communique was not escalated",
            )
        })?
        .to_owned();
    let (communique, replayed, _) =
        expect_accepted(gateway.call(GatewayCommand::EscalateCommunique {
            communique_ref: record.communique_ref.clone(),
            custody_ref: custody_ref.clone(),
            escalated_at_unix_ms: now_unix_ms(),
            basis: format!(
                "delegated into Factory custody {custody_ref} for {}: {}",
                request.work_ref, request.reason
            ),
        })?)?;
    Ok(json!({
        "communique": communique,
        "replayed": replayed,
        "custody_ref": custody_ref,
        "factory_receipt": receipt,
    }))
}

// ---------------------------------------------------------------------------
// who — aikit.population-reading/v1
// ---------------------------------------------------------------------------

fn absence(facet: &str, reason: impl Into<String>, source: impl Into<String>) -> Value {
    json!({ "facet": facet, "reason": reason.into(), "source": source.into() })
}

pub fn who(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
    project_world: Option<&str>,
) -> Result<Value> {
    let mut absences = Vec::new();
    let here = owners.world_here(cwd);
    let local_world_ref = match &here {
        Ok(here) => here
            .pointer("/local_world/ref")
            .cloned()
            .unwrap_or(Value::Null),
        Err(unavailable) => {
            absences.push(absence(
                "local_world",
                unavailable.reason.clone(),
                unavailable.command.clone(),
            ));
            Value::Null
        }
    };
    let (project, project_dir) = match project_world {
        Some(world) => (project_input(world), None),
        None => match here_project(owners, cwd) {
            Some((name, dir)) => (Some(name), dir),
            None => (None, None),
        },
    };
    let work_dir = project_dir.unwrap_or_else(|| cwd.to_path_buf());

    // Definitions (Central).
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    // Which Position names which agency as eligible: the join that turns the
    // registry's identities into occupancy seats.
    let mut eligibility: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut project_world_ref = project_world
        .map(|world| {
            if world.contains(':') {
                world.to_owned()
            } else {
                format!("project:{world}")
            }
        })
        .map(Value::String)
        .unwrap_or(Value::Null);
    let definitions_available = match owners.position_list(project.as_deref()) {
        Ok(listing) => {
            if let Some(world) = listing.get("world_ref") {
                project_world_ref = world.clone();
            }
            for (key, inherited) in [("positions", false), ("inherited", true)] {
                for entry in listing
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let record = position_record(entry);
                    let Some(reference) = record.get("ref").and_then(Value::as_str) else {
                        continue;
                    };
                    for agent_ref in record
                        .get("eligible_agent_refs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        eligibility
                            .entry(agent_ref.to_owned())
                            .or_default()
                            .push(reference.to_owned());
                    }
                    rows.insert(
                        reference.to_owned(),
                        json!({
                            "position_ref": reference,
                            "handle": record.get("handle").cloned().unwrap_or(Value::Null),
                            "label": record.get("label").cloned().unwrap_or(Value::Null),
                            "role_ref": record.get("role_ref").cloned().unwrap_or(Value::Null),
                            "inherited": inherited,
                            "definition": "present",
                        }),
                    );
                }
            }
            for invalid in listing
                .get("invalid")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                absences.push(absence(
                    "position",
                    format!(
                        "invalid definition at {}: {}",
                        invalid.get("path").and_then(Value::as_str).unwrap_or("?"),
                        invalid.get("error").and_then(Value::as_str).unwrap_or("?")
                    ),
                    "central.position.list",
                ));
            }
            true
        }
        Err(unavailable) => {
            absences.push(absence(
                "positions",
                unavailable.reason.clone(),
                unavailable.command.clone(),
            ));
            false
        }
    };

    // Occupancy (Actuation) — one uncapped listing.
    let occupancy: Option<BTreeMap<String, Value>> = match owners.occupancy_list() {
        Ok(listing) => {
            for invalid in listing
                .get("invalid")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                absences.push(absence(
                    "occupancy",
                    format!(
                        "ledger {} cannot be derived: {}",
                        invalid.get("path").and_then(Value::as_str).unwrap_or("?"),
                        invalid.get("error").and_then(Value::as_str).unwrap_or("?")
                    ),
                    "actuation occupancy list",
                ));
            }
            Some(
                listing
                    .get("positions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| {
                        entry
                            .get("position_ref")
                            .and_then(Value::as_str)
                            .map(|reference| (reference.to_owned(), entry.clone()))
                    })
                    .collect(),
            )
        }
        Err(unavailable) => {
            absences.push(absence(
                "occupancy",
                unavailable.reason.clone(),
                unavailable.command.clone(),
            ));
            None
        }
    };
    // Occupancy on the declared remote Workcells, one listing each, asked only
    // when this Workcell's own ledger could be read (a Position is only
    // "vacant here" when this ledger says so).
    let (local_workcell_ref, _) = local_workcell(owners, cwd);
    let elsewhere = remotes_elsewhere(home, local_workcell_ref.as_deref())?;
    let survey = if occupancy.is_some() && !elsewhere.is_empty() {
        RemoteSurvey::ask(&elsewhere, &GatewayCommand::OccupancyList)
    } else {
        RemoteSurvey::default()
    };
    let remote_occupied: Vec<String> = survey
        .answers
        .iter()
        .filter_map(|(_, outcome)| match outcome {
            RemoteOutcome::Answered(reading) => reading.occupancy.as_ref(),
            _ => None,
        })
        .flat_map(|listing| {
            listing
                .get("positions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|row| current_tenure(row).is_some())
                .filter_map(|row| row.get("position_ref").and_then(Value::as_str))
                .map(str::to_owned)
        })
        .collect();

    // An occupied Position this World should show but Central does not define
    // is still someone here: list it, marked, rather than hide them.
    if let Some(occupancy) = &occupancy {
        let world_prefix = project_world_ref
            .as_str()
            .map(|world| format!("central:position:{world}:"));
        for reference in occupancy.keys().chain(remote_occupied.iter()) {
            let in_world = world_prefix
                .as_deref()
                .is_none_or(|prefix| reference.starts_with(prefix))
                || reference.starts_with("central:position:control:root:");
            if in_world && !rows.contains_key(reference) {
                rows.insert(
                    reference.clone(),
                    json!({
                        "position_ref": reference,
                        "handle": Value::Null,
                        "label": Value::Null,
                        "role_ref": Value::Null,
                        "inherited": reference.starts_with("central:position:control:root:")
                            && project_world_ref.as_str() != Some("control:root"),
                        "definition": if definitions_available { "absent" } else { "unavailable" },
                    }),
                );
            }
        }
    }

    // Undelivered counts (the gateway's own journal).
    let counts: Option<BTreeMap<String, usize>> =
        match gateway.call(GatewayCommand::CommuniqueCounts) {
            Ok(GatewayResponse::CommuniqueCounts { counts }) => Some(
                counts
                    .into_iter()
                    .map(|count| (count.position_ref, count.undelivered))
                    .collect(),
            ),
            Ok(other) => {
                absences.push(absence(
                    "communiques",
                    unexpected(&other).to_string(),
                    "aikit gateway",
                ));
                None
            }
            Err(error) => {
                absences.push(absence("communiques", error.to_string(), "aikit gateway"));
                None
            }
        };

    // Agency identities (Central's agent-profile registry): every registered
    // profile is addressable by default, and its row says where — if
    // anywhere — the agency is currently embodied, and which registry the row
    // came from.
    let mut agents = Vec::new();
    match owners.agent_profiles() {
        Ok(listing) => {
            for entry in profile_entries(&listing) {
                let profile = profile_record(entry);
                let Some(agent_ref) = profile.get("agent_ref").and_then(Value::as_str) else {
                    continue;
                };
                let eligible = eligibility.get(agent_ref).cloned().unwrap_or_default();
                agents.push(json!({
                    "agent_ref": agent_ref,
                    "handle": profile_handle(agent_ref),
                    "label": profile.get("role").cloned().unwrap_or(Value::Null),
                    "purpose": profile.get("purpose").cloned().unwrap_or(Value::Null),
                    "registry": "agent-profile.list",
                    "positions": eligible,
                    "occupancy": agency_occupancy(
                        eligibility.get(agent_ref).map(Vec::as_slice),
                        occupancy.as_ref(),
                        &survey,
                        local_workcell_ref.as_deref(),
                    ),
                    "communiques": match &counts {
                        Some(counts) => {
                            json!({ "undelivered": counts.get(agent_ref).copied().unwrap_or(0) })
                        }
                        None => json!({ "undelivered": Value::Null }),
                    },
                }));
            }
        }
        Err(unavailable) => {
            absences.push(absence(
                "agents",
                unavailable.reason.clone(),
                unavailable.command.clone(),
            ));
        }
    }

    let mut positions = Vec::new();
    for (reference, mut row) in rows {
        let local_tenure = occupancy.as_ref().map(|occupancy| {
            occupancy.get(&reference).and_then(|entry| {
                entry
                    .get("current")
                    .filter(|current| !current.is_null())
                    .map(|tenure| (entry, tenure))
            })
        });
        row["occupancy"] = match local_tenure {
            None => json!({ "state": "unavailable" }),
            Some(Some((entry, tenure))) => {
                let mut reading = occupied_row(entry, tenure, None);
                reading["observed_via"] = json!("local");
                reading
            }
            Some(None) => {
                let claims = survey.claims(&reference);
                match claims.as_slice() {
                    [] => json!({ "state": "vacant", "observed_via": "local" }),
                    [claim] => {
                        let entry = remote_entry(&survey, claim, &reference);
                        let mut reading = occupied_row(
                            entry.as_ref().unwrap_or(&claim.tenure),
                            &claim.tenure,
                            Some(&claim.remote.workcell_ref),
                        );
                        reading["observed_via"] = json!(format!("gateway:{}", claim.gateway_ref));
                        reading
                    }
                    _ => {
                        let refusal = ambiguous_occupancy(&reference, &claims);
                        absences.push(absence(
                            &format!("occupancy:{reference}"),
                            refusal.details().get("fact").cloned().unwrap_or_default(),
                            "aikit gateway occupancy-list (declared remotes)",
                        ));
                        json!({
                            "state": "unavailable",
                            "reason": "more than one Workcell reports a current occupant",
                            "claims": claims.iter().map(|claim| json!({
                                "workcell_ref": claim.remote.workcell_ref,
                                "gateway_ref": claim.gateway_ref,
                                "generation_ref": claim.generation_ref,
                            })).collect::<Vec<_>>(),
                        })
                    }
                }
            }
        };
        row["current_work"] = match owners.current_work(&reference, &work_dir) {
            Ok(reading) => {
                // One reading of Factory's answer for every consumer: the same
                // refs `aikit whoami` and Refocus derive (node + resolved candidates).
                let outcome = reading
                    .get("outcome")
                    .cloned()
                    .unwrap_or(json!("unavailable"));
                let refs = if outcome == json!("one") {
                    crate::inhabitation::current_work_refs(&reading)
                } else {
                    Default::default()
                };
                json!({
                    "outcome": outcome,
                    "work_ref": refs.get("work_ref"),
                    "run_ref": refs.get("run_ref"),
                    "custody_ref": refs.get("custody_ref"),
                    "candidates": reading.get("candidates").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
                })
            }
            Err(unavailable) => {
                absences.push(absence(
                    &format!("current_work:{reference}"),
                    unavailable.reason.clone(),
                    unavailable.command.clone(),
                ));
                json!({ "outcome": "unavailable" })
            }
        };
        row["communiques"] = match &counts {
            Some(counts) => json!({ "undelivered": counts.get(&reference).copied().unwrap_or(0) }),
            None => json!({ "undelivered": Value::Null }),
        };
        positions.push(row);
    }

    Ok(json!({
        "schema": POPULATION_READING_SCHEMA,
        "project_world_ref": project_world_ref,
        "local_world_ref": local_world_ref,
        "local_workcell_ref": local_workcell_ref,
        "positions": positions,
        "agents": agents,
        "remotes": survey.statuses(),
        "absences": absences,
    }))
}

/// One agency's embodiment, joined from the occupancy its eligible Positions
/// hold: `embodied-here` when this Workcell's ledger (or the reporting
/// gateway) places a current occupant here, `embodied-elsewhere` when a
/// declared Workcell reports one, `not-currently-embodied` when none does —
/// and `unavailable` when occupancy could not be read at all.
fn agency_occupancy(
    eligible: Option<&[String]>,
    occupancy: Option<&BTreeMap<String, Value>>,
    survey: &RemoteSurvey,
    local_workcell: Option<&str>,
) -> Value {
    let Some(occupancy) = occupancy else {
        return json!({ "state": "unavailable" });
    };
    // No Position naming the agency is the unembodied case itself, not an
    // absence: with an empty eligibility the agency is simply nowhere.
    let eligible = eligible.unwrap_or(&[]);
    let mut elsewhere: Vec<Value> = Vec::new();
    for reference in eligible {
        if let Some(tenure) = occupancy.get(reference).and_then(current_tenure) {
            let workcell = tenure.get("workcell_ref").and_then(Value::as_str);
            let embodied_here = match (workcell, local_workcell) {
                (Some(workcell), Some(local)) => workcell == local,
                // A tenure naming no Workcell is this ledger's own.
                _ => true,
            };
            if embodied_here {
                return json!({
                    "state": "embodied-here",
                    "position_ref": reference,
                    "generation_ref": tenure.get("generation_ref"),
                    "workcell_ref": tenure.get("workcell_ref"),
                });
            }
            elsewhere.push(json!({
                "position_ref": reference,
                "generation_ref": tenure.get("generation_ref"),
                "workcell_ref": workcell,
            }));
            continue;
        }
        if let Some(claim) = survey.claims(reference).first() {
            elsewhere.push(json!({
                "position_ref": reference,
                "generation_ref": claim.generation_ref,
                "workcell_ref": claim.remote.workcell_ref,
            }));
        }
    }
    if elsewhere.is_empty() {
        json!({ "state": "not-currently-embodied", "positions": eligible })
    } else {
        json!({ "state": "embodied-elsewhere", "via": elsewhere })
    }
}

/// The population reading's occupancy facet for a current tenure. A remote
/// tenure that names no Workcell is placed on the Workcell that reported it.
fn occupied_row(entry: &Value, tenure: &Value, reported_by: Option<&str>) -> Value {
    let presence = entry
        .get("presence")
        .filter(|presence| presence.get("generation_ref") == tenure.get("generation_ref"));
    let workcell_ref = tenure
        .get("workcell_ref")
        .filter(|workcell| !workcell.is_null())
        .cloned()
        .or_else(|| reported_by.map(|workcell| json!(workcell)))
        .unwrap_or(Value::Null);
    json!({
        "state": "occupied",
        "generation_ref": tenure.get("generation_ref"),
        "generation_ordinal": tenure.get("generation_ordinal"),
        "kind": tenure.get("kind"),
        "agent_ref": tenure.get("agent_ref"),
        "agency_ref": tenure.get("agency_ref"),
        "agent_session_ref": tenure.get("agent_session_ref"),
        "workcell_ref": workcell_ref,
        "since_unix_ms": tenure.get("began_at_unix_ms"),
        "presence": presence.and_then(|p| p.get("presence")),
        "attention": presence.and_then(|p| p.get("attention")),
    })
}

/// The remote listing row (with its presence) behind one claim.
fn remote_entry(survey: &RemoteSurvey, claim: &RemoteClaim, position_ref: &str) -> Option<Value> {
    survey
        .answers
        .iter()
        .find_map(|(remote, outcome)| match outcome {
            RemoteOutcome::Answered(reading)
                if remote.workcell_ref == claim.remote.workcell_ref =>
            {
                reading
                    .occupancy
                    .as_ref()?
                    .get("positions")?
                    .as_array()?
                    .iter()
                    .find(|row| {
                        row.get("position_ref").and_then(Value::as_str) == Some(position_ref)
                    })
                    .cloned()
            }
            _ => None,
        })
}

/// The serve loop's relay hook: one relay pass per tick, through the carrier
/// the service itself exposes. Failures are the tick's to remember, never the
/// carrier's to die of.
pub struct CommuniqueRelayTick {
    pub home: AikitHome,
    pub target: GatewayCarrierTarget,
}

impl CommuniqueRelayTick {
    pub fn run(&self) -> Result<Value> {
        let owners = crate::gateway_owners::ProcessOwners::from_env();
        let gateway = LocalGateway {
            target: self.target.clone(),
            state_file: None,
            gateway_ref: String::new(),
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut report = forward_pass(&self.home, &owners, &gateway, &cwd)?;
        // The person's decisions travel back on the same tick; a failing
        // reply pass is reported, never allowed to stop the relay.
        report["owner_replies"] =
            match owner_address::owner_reply_pass(&self.home, &owners, &gateway, &cwd) {
                Ok(replies) => replies["owner_replies"].clone(),
                Err(error) => json!({ "error": error.to_string() }),
            };
        Ok(report)
    }
}

/// The gateway service's periodic body: the Routine dispatcher pass (when
/// Central could be resolved) and the Communique relay pass. Each half fails
/// on its own; neither takes the carriers down.
pub struct GatewayServiceTick {
    pub dispatcher: Option<Box<dyn aikit_adapters::GatewayTick>>,
    pub relay: CommuniqueRelayTick,
}

impl aikit_adapters::GatewayTick for GatewayServiceTick {
    fn tick(&self) -> Result<Value> {
        let dispatcher = self.dispatcher.as_ref().map(|tick| tick.tick());
        let relay = self.relay.run();
        // An upgrade whose worker died (or whose receipt could not be announced
        // because this gateway was still coming up) is finished by a fresh
        // worker. The worker is its own process: this tick only starts it.
        let upgrade = crate::gateway_upgrade_system::adopt_orphans(&self.relay.home)
            .map_err(|error| error.to_string());
        match (&dispatcher, &relay) {
            (Some(Err(error)), _) | (None, Err(error)) => Err(AikitError::new(
                "gateway.service_tick_failed",
                error.to_string(),
            )),
            _ => Ok(json!({
                "dispatcher": dispatcher.and_then(|result| result.ok()),
                "relay": relay.unwrap_or_else(|error| json!({ "error": error.to_string() })),
                "upgrade": match upgrade {
                    Ok(Some(worker)) => json!({ "resumed_by": worker }),
                    Ok(None) => Value::Null,
                    Err(error) => json!({ "error": error }),
                },
            })),
        }
    }
}

#[cfg(test)]
mod exact_instance_binding_tests {
    //! Simulated older peers: gateways that predate exact-instance routes
    //! (no advertised feature, or a record that comes back without its
    //! `to_instance`), and a service that does not know
    //! RecordCommuniqueStanding.

    use std::cell::RefCell;

    use aikit_adapters::GatewayStatus;
    use aikit_core::resource::ResourceRef;

    use super::*;
    use crate::gateway_owners::OwnerRefusal;

    fn record(reference: &str, generation: &str, state: &str) -> Communique {
        serde_json::from_value(json!({
            "schema": "aikit.communique/v1",
            "communique_ref": reference,
            "sequence": 1,
            "attribution": "unknown",
            "attribution_basis": "test",
            "to_position_ref": "position:steward",
            "to_instance": { "generation_ref": generation },
            "instance_hold": if state == "held" { json!("instance-absent") } else { Value::Null },
            "body": "hello",
            "sent_at_unix_ms": 1,
            "state": state,
            "origin_gateway_ref": "agency-gateway/local",
        }))
        .unwrap()
    }

    fn draft(instance: Option<&str>) -> CommuniqueDraft {
        CommuniqueDraft {
            communique_ref: "communique:01test".into(),
            from_position_ref: None,
            from_generation_ref: None,
            attribution: SenderAttribution::Unknown,
            attribution_basis: "test".into(),
            to_position_ref: "position:steward".into(),
            to_workcell_ref: None,
            to_instance: instance.map(|generation| CommuniqueInstance {
                generation_ref: generation.into(),
                required_workcell_ref: None,
                agent_session_ref: None,
                agency_ref: None,
            }),
            instance_hold: None,
            body: "hello".into(),
            sent_at_unix_ms: 1,
            state: CommuniqueState::Pending,
            state_basis: "test".into(),
            reply_to: None,
            forward_to_workcell_ref: None,
            routing: None,
        }
    }

    fn accepted_from(draft: &CommuniqueDraft) -> Communique {
        let mut communique = record(&draft.communique_ref, "unused", "pending");
        communique.to_instance = draft.to_instance.clone();
        communique
    }

    fn protocol(features: &[&str]) -> GatewayResponse {
        GatewayResponse::Protocol {
            gateway_version: "aikit.agency-gateway/v1".into(),
            connector_sdk_version: "x".into(),
            connector_wire_version: "x".into(),
            actuation_stream_schema: "x".into(),
            features: features.iter().map(|f| (*f).to_owned()).collect(),
            build: None,
            sender_attestation_key: None,
        }
    }

    /// A gateway double. `features` is what its protocol answer advertises;
    /// `strip` drops `to_instance` from every record it answers with (what an
    /// older binary's serde does); `refuse_standing` fails
    /// RecordCommuniqueStanding for those refs. `fates` answers the
    /// sender-side readback (ref → the record's fate there, `None` for "no
    /// such record"); `delivered` collects the readback completions.
    #[derive(Default)]
    struct StubGateway {
        features: Vec<&'static str>,
        strip: bool,
        refuse_standing: Vec<String>,
        queue: Vec<Communique>,
        fates: BTreeMap<String, Option<&'static str>>,
        delivered: RefCell<Vec<String>>,
        log: RefCell<Vec<String>>,
    }

    impl StubGateway {
        fn modern() -> Self {
            Self {
                features: vec![GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE],
                ..Self::default()
            }
        }

        fn answer(&self, mut communique: Communique) -> Communique {
            if self.strip {
                communique.to_instance = None;
            }
            communique
        }

        fn saw(&self, kind: &str) -> bool {
            self.log.borrow().iter().any(|entry| entry == kind)
        }
    }

    impl GatewayAccess for StubGateway {
        fn call(&self, command: GatewayCommand) -> Result<GatewayResponse> {
            let kind = serde_json::to_value(&command).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned();
            self.log.borrow_mut().push(kind);
            match command {
                GatewayCommand::Protocol => Ok(protocol(&self.features)),
                GatewayCommand::Status => Ok(GatewayResponse::Status {
                    status: GatewayStatus {
                        version: "v1".into(),
                        gateway_ref: ResourceRef::parse("agency-gateway/local").unwrap(),
                        connector_count: 0,
                        binding_count: 0,
                        stream_count: 0,
                        pending_delivery_count: 0,
                        delivery_receipt_count: 0,
                        connector_health: Vec::new(),
                        build: None,
                        listeners: Vec::new(),
                        pending_operations: Vec::new(),
                    },
                }),
                GatewayCommand::SendCommunique { draft } => {
                    Ok(GatewayResponse::CommuniqueAccepted {
                        communique: self.answer(accepted_from(&draft)),
                        replayed: false,
                        accepted_by: "agency-gateway/local".into(),
                    })
                }
                GatewayCommand::IngestCommunique { communique, .. } => {
                    Ok(GatewayResponse::CommuniqueAccepted {
                        communique: self.answer(*communique),
                        replayed: false,
                        accepted_by: "agency-gateway/remote".into(),
                    })
                }
                GatewayCommand::CommuniqueForwardQueue => Ok(GatewayResponse::CommuniqueList {
                    communiques: self.queue.clone(),
                }),
                GatewayCommand::RecordCommuniqueForward { communique_ref, .. } => {
                    let found = self
                        .queue
                        .iter()
                        .find(|c| c.communique_ref == communique_ref)
                        .cloned()
                        .unwrap_or_else(|| record(&communique_ref, "g", "pending"));
                    Ok(GatewayResponse::CommuniqueRecord {
                        communique: self.answer(found),
                    })
                }
                GatewayCommand::CommuniqueFate { communique_ref } => {
                    Ok(GatewayResponse::CommuniqueFate {
                        fate: self.fates.get(&communique_ref).and_then(|fate| {
                            fate.map(|_state| aikit_adapters::CommuniqueFate {
                                state: CommuniqueState::Delivered,
                                delivered_at_unix_ms: Some(2),
                                delivered_to_generation_ref: Some("gen-there".into()),
                            })
                        }),
                        communique_ref,
                    })
                }
                GatewayCommand::RecordRemoteDelivery {
                    communique_ref,
                    basis,
                    ..
                } => {
                    self.delivered
                        .borrow_mut()
                        .push(format!("{communique_ref}|{basis}"));
                    let found = self
                        .queue
                        .iter()
                        .find(|c| c.communique_ref == communique_ref)
                        .cloned()
                        .unwrap_or_else(|| record(&communique_ref, "g", "pending"));
                    Ok(GatewayResponse::CommuniqueRecord {
                        communique: self.answer(found),
                    })
                }
                GatewayCommand::RecordCommuniqueStanding {
                    communique_ref,
                    state,
                    instance_hold,
                    ..
                } => {
                    if self.refuse_standing.contains(&communique_ref) {
                        return Err(AikitError::new(
                            "agency_gateway.unknown_command",
                            "unknown variant `record-communique-standing`",
                        ));
                    }
                    let mut found = self
                        .queue
                        .iter()
                        .find(|c| c.communique_ref == communique_ref)
                        .cloned()
                        .unwrap();
                    found.state = state;
                    found.instance_hold = instance_hold;
                    Ok(GatewayResponse::CommuniqueRecord { communique: found })
                }
                other => panic!("unexpected command {other:?}"),
            }
        }
    }

    fn remote_b() -> GatewayRemote {
        GatewayRemote {
            workcell_ref: "workcell:b".into(),
            websocket_bind: "127.0.0.1:1".into(),
            websocket_path: "/".into(),
            token_location: "env:AIKIT_TEST_UNUSED_TOKEN".into(),
        }
    }

    fn code(error: &AikitError) -> &str {
        error.code()
    }

    /// An owners double whose ledger says the Position's current occupant
    /// stands on the named Workcell — the Elsewhere case both deciders must
    /// read identically (#481-1).
    struct TenuredOn(&'static str);

    impl ContactOwners for TenuredOn {
        fn position_list(&self, _: Option<&str>) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn agent_profiles(&self) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn position_read(&self, _: &str) -> std::result::Result<PositionLookup, OwnerUnavailable> {
            Err(unavailable())
        }
        fn world_here(&self, _: &Path) -> std::result::Result<Value, OwnerUnavailable> {
            Ok(json!({"workcells": [{"ref": "workcell:a", "role": "current"}]}))
        }
        fn occupancy_list(&self) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn occupancy_read(&self, _: &str) -> std::result::Result<Value, OwnerUnavailable> {
            Ok(json!({
                "position_ref": "position:steward",
                "state": "occupied",
                "current": {
                    "generation_ref": "gen-there",
                    "workcell_ref": self.0,
                }
            }))
        }
        fn occupancy_verify(
            &self,
            _: &str,
            _generation_ref: &str,
        ) -> std::result::Result<OccupancyVerdict, OwnerUnavailable> {
            // The ledger reads the tenure directly; verification is not on
            // this test's path.
            Ok(OccupancyVerdict::Current(json!({
                "generation_ref": "gen-there",
                "workcell_ref": self.0,
            })))
        }
        fn current_work(&self, _: &str, _: &Path) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn custody_assign(
            &self,
            _: &CustodyAssign,
            _: &Path,
        ) -> std::result::Result<std::result::Result<Value, OwnerRefusal>, OwnerUnavailable>
        {
            Err(unavailable())
        }
    }

    fn recipient_position() -> Recipient {
        Recipient {
            position_ref: "position:steward".into(),
            handle: None,
            label: None,
            source: "test".into(),
            agency_ref: None,
        }
    }

    /// The durable-route convergence (#481-1): send-time routing and the
    /// relay pass decide from ONE reading of the ledger and ONE vacant
    /// resolution — an occupant on another Workcell reaches EXACTLY ONE
    /// recipient, and a re-offer is answered as a replay, never a second
    /// delivery.
    #[test]
    fn one_decision_delivers_to_exactly_one_recipient_across_both_deciders() {
        if std::env::var(WORKCELL_ENV).is_ok() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        // A usable declared token: the relay's carrier resolves it for real.
        let token_path = dir.path().join("peer-b.token");
        std::fs::write(&token_path, "peer-b-secret").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let remote_b_declared = GatewayRemote {
            workcell_ref: "workcell:b".into(),
            websocket_bind: "127.0.0.1:1".into(),
            websocket_path: "/".into(),
            token_location: format!("file:{}", token_path.display()),
        };
        let path = remotes_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&GatewayRemotes {
                schema: GATEWAY_REMOTES_SCHEMA.into(),
                remotes: vec![remote_b_declared],
            })
            .unwrap(),
        )
        .unwrap();

        // Decision #1 — send-time routing: the occupant is on workcell:b,
        // so the route names b and nothing else.
        let route = route_to_occupancy(
            &home,
            &TenuredOn("workcell:b"),
            Some("workcell:a"),
            &recipient_position(),
        )
        .unwrap();
        let relayed_to = route
            .remote
            .as_ref()
            .map(|entry| entry.workcell_ref.clone());
        assert_eq!(
            relayed_to.as_deref(),
            Some("workcell:b"),
            "state: {:?} remote: {:?}",
            route.state,
            route.remote
        );
        assert_eq!(route.state, CommuniqueState::Pending);

        // Decision #2 — the relay pass re-evaluates the SAME record with ITS
        // decider. The remote journal answers replayed on the re-offer, so
        // the record is ingested at exactly one recipient, however many
        // passes re-run the same decision.
        let mut record = record("communique:01one", "gen-x", "pending");
        record.to_instance = None;
        record.forward = Some(CommuniqueForward::Queued {
            workcell_ref: "workcell:b".into(),
            attempts: 0,
            last_error: None,
            last_attempt_at_unix_ms: None,
        });
        let local = StubGateway {
            queue: vec![record],
            ..StubGateway::modern()
        };
        let remote = StubGateway::modern();
        let carrier = |_remote: &GatewayRemote, command: GatewayCommand| remote.call(command);
        let pass = forward_pass_via(
            &home,
            &TenuredOn("workcell:b"),
            &local,
            dir.path(),
            &carrier,
        )
        .unwrap();
        let ingests = remote
            .log
            .borrow()
            .iter()
            .filter(|k| **k == "ingest-communique")
            .count();
        assert_eq!(ingests, 1, "one offer at the one recipient; pass: {pass}");
        // And the OTHER Workcell got nothing: the stub owns the only ingest.
        let _ = &local;

        // Consistency for the unreadable ledger: send-time answers
        // pending-here (never relayed on an unreadable ledger), and the
        // pass's decider answers Unknown — both keep the record local.
        let unreadable = route_to_occupancy(
            &home,
            &StubOwners,
            Some("workcell:a"),
            &recipient_position(),
        )
        .unwrap();
        assert_eq!(unreadable.state, CommuniqueState::Pending);
        assert!(
            unreadable.remote.is_none(),
            "an unreadable ledger is never relayed on"
        );
        assert!(unreadable.delivery_notice.is_some());
        let placement = ledger_placement(
            &TenuredOn("workcell:b"),
            "position:steward",
            Some("workcell:a"),
        );
        assert!(matches!(placement, LedgerPlacement::Elsewhere(ref w) if w == "workcell:b"));
        let placement_unknown =
            ledger_placement(&StubOwners, "position:steward", Some("workcell:a"));
        assert!(matches!(placement_unknown, LedgerPlacement::Unknown));
    }

    fn test_home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        std::fs::create_dir_all(home.state()).unwrap();
        (dir, home)
    }

    #[test]
    fn an_exact_route_is_refused_by_a_local_gateway_that_does_not_advertise_the_feature() {
        let old = StubGateway::default();
        let (_dir, home) = test_home();
        let error =
            accept_and_relay_via(&home, &old, draft(Some("g1")), None, json!({}), &|_, _| {
                unreachable!("no remote")
            })
            .unwrap_err();
        assert_eq!(code(&error), "gateway.exact_instance_unsupported");
        assert!(!old.saw("send-communique"), "nothing is handed to it");

        // A durable Position route still goes to an older gateway.
        let sent = accept_and_relay_via(&home, &old, draft(None), None, json!({}), &|_, _| {
            unreachable!("no remote")
        })
        .unwrap();
        assert!(sent["communique"].get("to_instance").is_none());
    }

    #[test]
    fn an_exact_route_whose_binding_the_local_gateway_drops_is_refused_not_reported() {
        let stripping = StubGateway {
            strip: true,
            ..StubGateway::modern()
        };
        let (_dir, home) = test_home();
        let error = accept_and_relay_via(
            &home,
            &stripping,
            draft(Some("g1")),
            None,
            json!({"route": "exact-instance"}),
            &|_, _| unreachable!("no remote"),
        )
        .unwrap_err();
        assert_eq!(code(&error), "gateway.communique_instance_binding_lost");
    }

    #[test]
    fn an_exact_relay_to_an_older_remote_gateway_is_refused_before_anything_is_recorded() {
        let local = StubGateway::modern();
        let old_remote = StubGateway::default();
        let (_dir, home) = test_home();
        let error = accept_and_relay_via(
            &home,
            &local,
            draft(Some("g1")),
            Some(remote_b()),
            json!({}),
            &|_, command| old_remote.call(command),
        )
        .unwrap_err();
        assert_eq!(code(&error), "gateway.exact_instance_unsupported");
        assert!(!local.saw("send-communique"));
        assert!(!old_remote.saw("ingest-communique"));
    }

    #[test]
    fn a_remote_that_drops_the_binding_on_ingest_is_recorded_failed_and_refused() {
        let queued = record("communique:01relay", "g1", "pending");
        let local = StubGateway {
            queue: vec![queued.clone()],
            ..StubGateway::modern()
        };
        let stripping_remote = StubGateway {
            strip: true,
            ..StubGateway::modern()
        };
        let (_dir, home) = test_home();
        let error = forward_one_via(
            &home,
            &local,
            &queued,
            &remote_b(),
            "agency-gateway/local",
            None,
            &|_, command| stripping_remote.call(command),
        )
        .unwrap_err();
        assert_eq!(code(&error), "gateway.communique_instance_binding_lost");
        assert!(stripping_remote.saw("ingest-communique"));
        assert!(
            local.saw("record-communique-forward"),
            "the refused relay is recorded here, so the record stays queued"
        );

        // A remote that never advertised the feature is never handed it.
        let old_remote = StubGateway::default();
        let (_dir, home) = test_home();
        let error = forward_one_via(
            &home,
            &local,
            &queued,
            &remote_b(),
            "agency-gateway/local",
            None,
            &|_, command| old_remote.call(command),
        )
        .unwrap_err();
        assert_eq!(code(&error), "gateway.exact_instance_unsupported");
        assert!(!old_remote.saw("ingest-communique"));
    }

    struct StubOwners;

    impl ContactOwners for StubOwners {
        fn position_list(&self, _: Option<&str>) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn agent_profiles(&self) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn position_read(&self, _: &str) -> std::result::Result<PositionLookup, OwnerUnavailable> {
            Err(unavailable())
        }
        fn world_here(&self, _: &Path) -> std::result::Result<Value, OwnerUnavailable> {
            Ok(json!({"workcells": [{"ref": "workcell:a", "role": "current"}]}))
        }
        fn occupancy_list(&self) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn occupancy_read(&self, _: &str) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn occupancy_verify(
            &self,
            _: &str,
            generation_ref: &str,
        ) -> std::result::Result<OccupancyVerdict, OwnerUnavailable> {
            // gen-here stands on this Workcell; gen-there on workcell:b.
            let workcell = if generation_ref == "gen-here" {
                "workcell:a"
            } else {
                "workcell:b"
            };
            Ok(OccupancyVerdict::Current(json!({
                "generation_ref": generation_ref,
                "workcell_ref": workcell,
            })))
        }
        fn current_work(&self, _: &str, _: &Path) -> std::result::Result<Value, OwnerUnavailable> {
            Err(unavailable())
        }
        fn custody_assign(
            &self,
            _: &CustodyAssign,
            _: &Path,
        ) -> std::result::Result<std::result::Result<Value, OwnerRefusal>, OwnerUnavailable>
        {
            Err(unavailable())
        }
    }

    fn unavailable() -> OwnerUnavailable {
        OwnerUnavailable {
            command: "stub".into(),
            reason: "not in this test".into(),
        }
    }

    /// The sender side of remote delivery (#481 item 3): a record this
    /// gateway forwarded learns its delivery from the gateway it was relayed
    /// to, on the relay pass. A record the remote does not know stays exactly
    /// as it was, named unresolved — never read as delivered.
    #[test]
    fn the_sender_copy_learns_remote_delivery_on_the_relay_pass_and_unknown_stays_unresolved() {
        if std::env::var(WORKCELL_ENV).is_ok() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        let path = remotes_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&GatewayRemotes {
                schema: GATEWAY_REMOTES_SCHEMA.into(),
                remotes: vec![remote_b()],
            })
            .unwrap(),
        )
        .unwrap();
        let mut forwarded = record("communique:01fwd", "gen-x", "pending");
        forwarded.to_instance = None;
        forwarded.forward = Some(CommuniqueForward::Forwarded {
            workcell_ref: "workcell:b".into(),
            remote_gateway_ref: "agency-gateway/b".into(),
            forwarded_at_unix_ms: 1,
            attempts: 1,
        });
        let mut unknown = forwarded.clone();
        unknown.communique_ref = "communique:02unknown".into();
        // The remote knows the first was delivered and holds no record of the
        // second (a foreign or retired journal).
        let local = StubGateway {
            queue: vec![forwarded, unknown],
            ..StubGateway::modern()
        };
        let remote = StubGateway {
            fates: BTreeMap::from([
                ("communique:01fwd".to_owned(), Some("delivered")),
                ("communique:02unknown".to_owned(), None),
            ]),
            ..StubGateway::modern()
        };
        let pass = forward_pass_via(&home, &StubOwners, &local, dir.path(), &|_, command| {
            remote.call(command)
        })
        .unwrap();
        let readback = &pass["readback"];
        let learned = readback["learned_delivery"].as_array().unwrap();
        assert_eq!(learned.len(), 1, "{pass:#}");
        assert_eq!(learned[0]["communique_ref"], "communique:01fwd");
        assert_eq!(learned[0]["remote_gateway_ref"], "agency-gateway/b");
        let unresolved = readback["unresolved"].as_array().unwrap();
        assert_eq!(unresolved.len(), 1, "{pass:#}");
        assert_eq!(unresolved[0]["communique_ref"], "communique:02unknown");
        // The completion was recorded on the SENDER gateway, for the learned
        // record only, with the basis naming where delivery was read back.
        let delivered = local.delivered.borrow();
        assert_eq!(delivered.len(), 1, "{delivered:?}");
        assert!(
            delivered[0].starts_with("communique:01fwd|"),
            "{delivered:?}"
        );
        assert!(delivered[0].contains("workcell:b"), "{delivered:?}");
        // And the remote was asked about the fate of both, never more.
        assert_eq!(
            remote
                .log
                .borrow()
                .iter()
                .filter(|k| **k == "communique-fate")
                .count(),
            2
        );
    }

    #[test]
    fn one_failing_restand_does_not_stop_the_pass_relaying_the_others() {
        if std::env::var(WORKCELL_ENV).is_ok() {
            // The local Workcell must come from the stub owners.
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        let path = remotes_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&GatewayRemotes {
                schema: GATEWAY_REMOTES_SCHEMA.into(),
                remotes: vec![remote_b()],
            })
            .unwrap(),
        )
        .unwrap();
        // The first record's standing cannot be recorded (an older service,
        // or a delivered-meanwhile race); the second must still be relayed.
        let local = StubGateway {
            queue: vec![
                record("communique:01first", "gen-here", "held"),
                record("communique:02second", "gen-there", "pending"),
            ],
            refuse_standing: vec!["communique:01first".into()],
            ..StubGateway::modern()
        };
        let remote = StubGateway::modern();
        let pass = forward_pass_via(&home, &StubOwners, &local, dir.path(), &|_, command| {
            remote.call(command)
        })
        .unwrap();
        let forwarded = pass["forwarded"].as_array().unwrap();
        assert_eq!(forwarded.len(), 1, "{pass:#}");
        assert_eq!(forwarded[0]["communique_ref"], "communique:02second");
        let skipped = pass["skipped"].as_array().unwrap();
        assert_eq!(skipped.len(), 1, "{pass:#}");
        assert_eq!(skipped[0]["communique_ref"], "communique:01first");
        assert_eq!(skipped[0]["code"], "agency_gateway.unknown_command");
    }
}
