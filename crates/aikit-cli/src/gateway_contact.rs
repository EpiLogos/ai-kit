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
//! - **Occupancy is each Workcell's own.** This Workcell's Actuation ledger
//!   is read first; only when it records no current occupant are the declared
//!   remote gateways asked, and each answers from its own Actuation at the
//!   moment of asking. One answer routes; two are refused as ambiguous; none
//!   holds. Nothing is cached and no second occupancy store exists.
//! - **A Communique never mints obligation.** Only `delegate` crosses into
//!   Factory custody, and the custody ref is Factory's answer.
//! - **Every refusal is three-part**: the fact, what did or did not happen,
//!   and the exact next command.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{
    Communique, CommuniqueDraft, CommuniqueForwardOutcome, CommuniqueState, GatewayCarrierTarget,
    GatewayCommand, GatewayResponse, SenderAttribution, COMMUNIQUE_REF_PREFIX,
};
use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::gateway_owners::{
    current_tenure, position_record, ContactOwners, CustodyAssign, OccupancyVerdict,
    OwnerUnavailable, PositionLookup,
};
use crate::secret_location::SecretLocation;

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

/// The recipient Position as Central defines it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recipient {
    pub position_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: String,
}

fn nothing_sent() -> &'static str {
    "Nothing was sent; no Communique was recorded."
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
            }),
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
                "gateway.ambiguous_position",
                format!("{} Positions answer to {wanted}.", matches.len()),
                nothing_sent(),
                "Address the Position by its full ref from `aikit gateway who --json`.",
            )),
        };
    }
    if !to.starts_with("central:position:") {
        return Err(three_part(
            "gateway.invalid_recipient",
            format!("{to:?} is neither a Position ref (central:position:<world>:<slug>) nor an @handle."),
            nothing_sent(),
            "List the Positions with `aikit gateway who --json`.",
        ));
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
    let sender = resolve_sender(owners, request.from_position)?;
    let recipient = resolve_recipient(owners, request.to, request.project_world, cwd)?;
    let (local_workcell, workcell_basis) = local_workcell(owners, cwd);
    let now = now_unix_ms();

    // Where the occupant stands. This Workcell's own ledger first; only when
    // it records no current occupant are the declared remotes asked.
    let mut remote = None;
    let mut routing = None;
    let mut remotes_asked = Vec::new();
    let (state, state_basis, occupant_workcell, delivery_notice) =
        match owners.occupancy_read(&recipient.position_ref) {
            Ok(reading) => match current_tenure(&reading) {
                Some(tenure) => {
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
                None => {
                    let elsewhere = remotes_elsewhere(home, local_workcell.as_deref())?;
                    let survey = RemoteSurvey::ask(
                        &elsewhere,
                        &GatewayCommand::OccupancyRead {
                            position_ref: recipient.position_ref.clone(),
                        },
                    );
                    remotes_asked = survey.statuses();
                    let claims = survey.claims(&recipient.position_ref);
                    match claims.as_slice() {
                        [claim] => {
                            let route = claim.routing(&recipient.position_ref);
                            let basis = format!(
                                "{}; relayed to that Workcell's gateway, delivered at the occupant's next turn boundary there",
                                route.basis
                            );
                            remote = Some(claim.remote.clone());
                            routing = Some(route);
                            (
                                CommuniqueState::Pending,
                                basis,
                                Some(claim.remote.workcell_ref.clone()),
                                None,
                            )
                        }
                        [] => {
                            let (basis, notice) =
                                vacant_everywhere(&recipient.position_ref, &elsewhere, &survey);
                            (CommuniqueState::Held, basis, None, Some(notice))
                        }
                        _ => return Err(ambiguous_occupancy(&recipient.position_ref, &claims)),
                    }
                }
            },
            Err(unavailable) => (
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
    if let (None, Some(occupant), Some(local)) = (&routing, &occupant_workcell, &local_workcell) {
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

    let draft = CommuniqueDraft {
        communique_ref: format!(
            "{COMMUNIQUE_REF_PREFIX}{}",
            ulid::Ulid::generate().to_string().to_ascii_lowercase()
        ),
        from_position_ref: sender.from_position_ref.clone(),
        from_generation_ref: sender.from_generation_ref.clone(),
        attribution: sender.attribution,
        attribution_basis: sender.basis.clone(),
        to_position_ref: recipient.position_ref.clone(),
        to_workcell_ref: occupant_workcell.clone(),
        body: request.body,
        sent_at_unix_ms: now,
        state,
        state_basis,
        reply_to: request.reply_to,
        forward_to_workcell_ref: remote.as_ref().map(|entry| entry.workcell_ref.clone()),
        routing,
    };
    let (mut communique, replayed, accepted_by) =
        expect_accepted(gateway.call(GatewayCommand::SendCommunique { draft })?)?;

    let mut forward = Value::Null;
    if let Some(entry) = remote {
        let (record, report) = forward_one(gateway, &communique, &entry, &accepted_by, None)?;
        communique = record;
        forward = report;
    }

    Ok(json!({
        "communique": communique,
        "replayed": replayed,
        "accepted_by": accepted_by,
        "recipient": recipient,
        "sender": sender,
        "local_workcell": { "ref": local_workcell, "basis": workcell_basis },
        "delivery": delivery_notice,
        "forward": forward,
        "remotes": remotes_asked,
    }))
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
fn forward_one(
    gateway: &dyn GatewayAccess,
    communique: &Communique,
    remote: &GatewayRemote,
    local_gateway_ref: &str,
    routing: Option<aikit_adapters::CommuniqueRouting>,
) -> Result<(Communique, Value)> {
    let at = now_unix_ms();
    let attempt = SecretLocation::parse(&remote.token_location)
        .and_then(|location| location.resolve())
        .and_then(|token| {
            let target = GatewayCarrierTarget::WebSocket {
                bind: remote.websocket_bind.clone(),
                path: remote.websocket_path.clone(),
                bearer_token: token.expose().to_owned(),
            };
            aikit_adapters::gateway_command(
                &target,
                GatewayCommand::IngestCommunique {
                    communique: communique.clone(),
                    relayed_by: local_gateway_ref.to_owned(),
                },
                None,
            )
        })
        .and_then(expect_accepted);
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
    match owners.occupancy_read(position_ref) {
        Ok(reading) => match current_tenure(&reading) {
            None => LedgerPlacement::Vacant,
            Some(tenure) => match (tenure.get("workcell_ref").and_then(Value::as_str), local) {
                (Some(workcell), Some(local)) if workcell != local => {
                    LedgerPlacement::Elsewhere(workcell.to_owned())
                }
                _ => LedgerPlacement::Here,
            },
        },
        Err(_) => LedgerPlacement::Unknown,
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
    let (mut forwarded, mut queued, mut skipped, mut held, mut ambiguous) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for record in &queue {
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
                let claims = survey.claims(&record.to_position_ref);
                match claims.as_slice() {
                    [claim] => (
                        claim.remote.clone(),
                        Some(claim.routing(&record.to_position_ref)),
                    ),
                    [] => {
                        held.push(json!({
                            "communique_ref": record.communique_ref,
                            "position_ref": record.to_position_ref,
                            "vacant_on": survey.answered_vacant(&record.to_position_ref),
                            "unanswered": survey.unanswered(),
                        }));
                        continue;
                    }
                    _ => {
                        let refusal = ambiguous_occupancy(&record.to_position_ref, &claims);
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
        let (record, report) = forward_one(gateway, record, &entry, &local_gateway_ref, routing)?;
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
    Ok(json!({
        "considered": queue.len(),
        "local_workcell": { "ref": local, "basis": basis },
        "forwarded": forwarded,
        "queued": queued,
        "skipped": skipped,
        "held": held,
        "ambiguous": ambiguous,
        "remotes": survey.map(|survey| survey.statuses()).unwrap_or_default(),
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
}

pub fn resolve_occupant(owners: &dyn ContactOwners, explicit: Option<&str>) -> Result<Occupant> {
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
        });
    };
    match owners.occupancy_verify(&position, &generation) {
        Ok(OccupancyVerdict::Current(_)) => Ok(Occupant {
            position_ref: position,
            generation_ref: Some(generation),
            verified: true,
            basis: "actuation occupancy verify".into(),
        }),
        Ok(OccupancyVerdict::Refused(refusal)) => Ok(Occupant {
            position_ref: position,
            generation_ref: Some(generation),
            verified: false,
            basis: format!(
                "Actuation refuses this generation ({}): {}",
                refusal.code, refusal.fact
            ),
        }),
        Err(unavailable) => Ok(Occupant {
            position_ref: position,
            generation_ref: Some(generation),
            verified: false,
            basis: format!("occupancy could not be verified: {unavailable}"),
        }),
    }
}

pub fn inbox(
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    position: Option<&str>,
    ack: bool,
) -> Result<Value> {
    let occupant = resolve_occupant(owners, position)?;
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
        value["deliverable"] = json!(match (record.state, occupied_now) {
            (CommuniqueState::Held, true) => "held-now-deliverable",
            (CommuniqueState::Held, false) => "held-awaiting-occupant",
            _ => "pending",
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
    let refs: Vec<String> = records
        .iter()
        .map(|record| record.communique_ref.clone())
        .collect();
    let delivered = if refs.is_empty() {
        Vec::new()
    } else {
        expect_list(gateway.call(GatewayCommand::AcknowledgeCommuniques {
            position_ref: occupant.position_ref.clone(),
            generation_ref: generation,
            communique_refs: refs,
            delivered_at_unix_ms: now_unix_ms(),
            via: "by `aikit gateway inbox --ack`".into(),
        })?)?
    };
    Ok(json!({
        "position_ref": occupant.position_ref,
        "occupant": occupant,
        "communiques": delivered,
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
        "remotes": survey.statuses(),
        "absences": absences,
    }))
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
        forward_pass(&self.home, &owners, &gateway, &cwd)
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
        match (&dispatcher, &relay) {
            (Some(Err(error)), _) | (None, Err(error)) => Err(AikitError::new(
                "gateway.service_tick_failed",
                error.to_string(),
            )),
            _ => Ok(json!({
                "dispatcher": dispatcher.and_then(|result| result.ok()),
                "relay": relay.unwrap_or_else(|error| json!({ "error": error.to_string() })),
            })),
        }
    }
}
