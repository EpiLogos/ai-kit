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
                None => (
                    CommuniqueState::Held,
                    format!("{} is vacant; held for the next occupant", recipient.position_ref),
                    None,
                    Some(json!({
                        "fact": format!("Position {} is vacant: Actuation records no current occupant.", recipient.position_ref),
                        "consequence": "The Communique is recorded held in this gateway's journal; nothing has been delivered yet.",
                        "action": format!(
                            "It is delivered to the next occupant that claims {} at that occupant's first turn boundary; follow it with `aikit gateway conversation --with {}`.",
                            recipient.position_ref, recipient.position_ref
                        ),
                    })),
                ),
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

    // Another Workcell holds the occupant: relay, or refuse before recording.
    let mut remote = None;
    if let (Some(occupant), Some(local)) = (&occupant_workcell, &local_workcell) {
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
    };
    let (mut communique, replayed, accepted_by) =
        expect_accepted(gateway.call(GatewayCommand::SendCommunique { draft })?)?;

    let mut forward = Value::Null;
    if let Some(entry) = remote {
        let (record, report) = forward_one(gateway, &communique, &entry, &accepted_by)?;
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
    }))
}

/// Relay one record to a declared remote gateway and record the outcome
/// locally. A remote that cannot be reached leaves the record queued; the
/// sender is never blocked on it.
fn forward_one(
    gateway: &dyn GatewayAccess,
    communique: &Communique,
    remote: &GatewayRemote,
    local_gateway_ref: &str,
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

/// Re-evaluate every Communique this gateway still has to deliver: when its
/// recipient's occupant now stands on a declared remote Workcell, relay it.
pub fn forward_pass(
    home: &AikitHome,
    owners: &dyn ContactOwners,
    gateway: &dyn GatewayAccess,
    cwd: &Path,
) -> Result<Value> {
    let queue = expect_list(gateway.call(GatewayCommand::CommuniqueForwardQueue)?)?;
    let (local, basis) = local_workcell(owners, cwd);
    let Some(local) = local else {
        return Ok(json!({
            "considered": queue.len(),
            "forwarded": [],
            "queued": [],
            "skipped": [],
            "note": format!("this home's Workcell is unknown ({basis}); nothing can be judged remote"),
        }));
    };
    let local_gateway_ref = match gateway.call(GatewayCommand::Status)? {
        GatewayResponse::Status { status } => status.gateway_ref.to_string(),
        other => return Err(unexpected(&other)),
    };
    let remotes = load_remotes(home)?;
    let mut occupant_workcell: BTreeMap<String, Option<String>> = BTreeMap::new();
    let (mut forwarded, mut queued, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for record in queue {
        let workcell = occupant_workcell
            .entry(record.to_position_ref.clone())
            .or_insert_with(|| {
                owners
                    .occupancy_read(&record.to_position_ref)
                    .ok()
                    .and_then(|reading| {
                        current_tenure(&reading)
                            .and_then(|tenure| tenure.get("workcell_ref").and_then(Value::as_str))
                            .map(str::to_owned)
                    })
            })
            .clone();
        let Some(workcell) = workcell.filter(|workcell| workcell != &local) else {
            continue;
        };
        match remotes
            .remotes
            .iter()
            .find(|entry| entry.workcell_ref == workcell)
        {
            Some(entry) => {
                let (record, report) = forward_one(gateway, &record, entry, &local_gateway_ref)?;
                if report["state"] == "forwarded" {
                    forwarded.push(
                        json!({"communique_ref": record.communique_ref, "workcell_ref": workcell}),
                    );
                } else {
                    queued.push(json!({"communique_ref": record.communique_ref, "report": report}));
                }
            }
            None => skipped.push(json!({
                "communique_ref": record.communique_ref,
                "workcell_ref": workcell,
                "action": format!("Declare the endpoint: {}", remote_command(&workcell)),
            })),
        }
    }
    Ok(json!({
        "local_workcell": local,
        "forwarded": forwarded,
        "queued": queued,
        "skipped": skipped,
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
    // An occupied Position this World should show but Central does not define
    // is still someone here: list it, marked, rather than hide them.
    if let Some(occupancy) = &occupancy {
        let world_prefix = project_world_ref
            .as_str()
            .map(|world| format!("central:position:{world}:"));
        for reference in occupancy.keys() {
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
        row["occupancy"] = match &occupancy {
            None => json!({ "state": "unavailable" }),
            Some(occupancy) => match occupancy.get(&reference) {
                None => json!({ "state": "vacant" }),
                Some(entry) => match entry.get("current").filter(|current| !current.is_null()) {
                    None => json!({ "state": "vacant" }),
                    Some(tenure) => {
                        let presence = entry.get("presence").filter(|presence| {
                            presence.get("generation_ref") == tenure.get("generation_ref")
                        });
                        json!({
                            "state": "occupied",
                            "generation_ref": tenure.get("generation_ref"),
                            "generation_ordinal": tenure.get("generation_ordinal"),
                            "kind": tenure.get("kind"),
                            "agent_ref": tenure.get("agent_ref"),
                            "agency_ref": tenure.get("agency_ref"),
                            "agent_session_ref": tenure.get("agent_session_ref"),
                            "workcell_ref": tenure.get("workcell_ref"),
                            "since_unix_ms": tenure.get("began_at_unix_ms"),
                            "presence": presence.and_then(|p| p.get("presence")),
                            "attention": presence.and_then(|p| p.get("attention")),
                        })
                    }
                },
            },
        };
        row["current_work"] = match owners.current_work(&reference, &work_dir) {
            Ok(reading) => {
                let current = reading.get("current").filter(|current| !current.is_null());
                json!({
                    "outcome": reading.get("outcome").cloned().unwrap_or(json!("unavailable")),
                    "work_ref": current.and_then(|c| c.get("work_ref").or_else(|| c.get("node_ref"))),
                    "run_ref": current.and_then(|c| c.get("run_ref")),
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
        "positions": positions,
        "absences": absences,
    }))
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
