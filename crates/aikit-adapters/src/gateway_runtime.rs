//! AIKit Agency Gateway runtime kernel.
//!
//! The gateway is a persistent contact plane for the same situated Agency across
//! multiple communication Surfaces. It keeps semantic identity separate from the
//! process, socket, Workcell allocation or connector instance that materialises
//! the service.
//!
//! ```text
//! Agency / Actuation
//!       ↓
//! AgentSession + ActuationStream
//!       ↓
//! Gateway binding
//!       ├─ Cradle / harness-native Surface
//!       ├─ Telegram / Slack / Discord connector
//!       └─ API / webhook connector
//! ```
//!
//! This crate intentionally does not implement Workcell lifecycle semantics.
//! Workcell already exposes the provider-neutral service relation
//! `resolve_service → observe_service → release_service`; a Workcell provider may
//! materialise this long-running runtime body without changing any gateway refs.

use std::collections::{BTreeMap, BTreeSet};

use aikit_adapters::{
    ConnectorDescriptor, ConnectorHealth, ConnectorOperation, ConversationAddress, DeliveryReceipt,
    GatewayConnector, InboundEvent, InboundEventKind, MediaReference, OutboundOperation,
    OutboundOperationKind, SenderIdentity, GATEWAY_CONNECTOR_SDK_VERSION,
    GATEWAY_CONNECTOR_WIRE_VERSION,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::gateway_communique::{
    Communique, CommuniqueCount, CommuniqueDraft, CommuniqueForwardOutcome, CommuniqueInstanceHold,
    CommuniqueJournal, CommuniqueRouting, CommuniqueState,
};
use crate::gateway_posture::{
    GatewayBuildIdentity, GatewayListenerReading, GATEWAY_FEATURE_BUILD_IDENTITY,
    GATEWAY_FEATURE_CARRIER_SCOPE, GATEWAY_FEATURE_CONFIGURED_IDENTITY, GATEWAY_FEATURE_DRAIN,
    GATEWAY_FEATURE_ENCOUNTER_RELAY, GATEWAY_FEATURE_UNSUPPORTED_COMMAND,
};

pub const AGENCY_GATEWAY_VERSION: &str = "aikit.agency-gateway/v1";
pub const ACTUATION_STREAM_SCHEMA: &str = "actuation.stream/v1";
pub const GATEWAY_OCCUPANCY_READING_SCHEMA: &str = "aikit.gateway-occupancy-reading/v1";

/// Protocol feature: this gateway keeps a Communique's `to_instance` binding
/// through send, ingest, relay and its state file. A gateway that does not
/// advertise it (one built before exact-instance routes) silently drops the
/// field and would turn an exact-instance Communique into a durable Position
/// route, so a client never hands it one.
pub const GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE: &str = "communique-exact-instance";

/// Every protocol feature this gateway advertises in its `protocol` answer.
/// A peer asks for the feature it needs before it uses the command that
/// depends on it, so a gateway built earlier is named and refused for that one
/// thing instead of failing on an unknown command.
pub const GATEWAY_PROTOCOL_FEATURES: [&str; 7] = [
    GATEWAY_FEATURE_COMMUNIQUE_EXACT_INSTANCE,
    GATEWAY_FEATURE_BUILD_IDENTITY,
    GATEWAY_FEATURE_DRAIN,
    GATEWAY_FEATURE_CARRIER_SCOPE,
    GATEWAY_FEATURE_UNSUPPORTED_COMMAND,
    GATEWAY_FEATURE_CONFIGURED_IDENTITY,
    GATEWAY_FEATURE_ENCOUNTER_RELAY,
];

/// The only encounter actions a peer gateway may relay to this Workcell's
/// owner: look up a recipient's binding, send one turn, read its delivery, read
/// its reply. Everything else an encounter owner accepts (opening, configuring,
/// shutting down a session host) is the owner's and never crosses a Workcell.
pub const ENCOUNTER_RELAY_ACTIONS: [&str; 4] =
    ["agency-read", "send", "delivery", "delivery-reply"];

/// A serving gateway's answer to "who occupies this Position on your
/// Workcell" (or, with no Position, the whole listing). The gateway keeps no
/// occupancy: the answer is its own Workcell's Actuation read at the moment of
/// asking, returned verbatim beside the gateway and Workcell that answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayOccupancyReading {
    pub schema: String,
    /// The Position asked about; `None` when the whole listing was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_ref: Option<String>,
    pub gateway_ref: String,
    /// The Workcell this gateway serves; `None` when it cannot say.
    #[serde(default)]
    pub workcell_ref: Option<String>,
    pub workcell_basis: String,
    /// Actuation's document, verbatim: `actuation.position-occupancy/v1` for
    /// one Position, `actuation.position-occupancy-listing/v1` for the listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupancy: Option<Value>,
    /// Set when this Workcell's Actuation could not answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<GatewayOwnerUnavailable>,
    pub read_at_unix_ms: u64,
}

/// An owner that could not answer: the exact command and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayOwnerUnavailable {
    pub command: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayIngressDecision {
    Allow,
    Pair,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayIngressPolicy {
    pub default: GatewayIngressDecision,
    #[serde(default)]
    pub sender_overrides: BTreeMap<String, GatewayIngressDecision>,
}

impl GatewayIngressPolicy {
    pub fn decision_for(&self, sender: &SenderIdentity) -> GatewayIngressDecision {
        self.sender_overrides
            .get(&sender.native_sender_id)
            .copied()
            .unwrap_or(self.default)
    }
}

/// Smallest continuation lineage the gateway represents: the Stream/sequence
/// point a new binding continued from. Refs and a cursor only — Actuation owns
/// the semantics of forking; the gateway keeps the relation legible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayForkOrigin {
    pub stream_ref: ResourceRef,
    pub at_sequence: u64,
}

fn default_context_revision() -> u64 {
    1
}

fn default_subscribe_limit() -> usize {
    usize::MAX
}

/// Stable semantic route between one provider-native conversation and one
/// situated AgentSession/ActuationStream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayBinding {
    pub binding_ref: ResourceRef,
    pub connector_ref: ResourceRef,
    pub address: ConversationAddress,
    pub agent_session_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub actuation_ref: ResourceRef,
    pub actuation_stream_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_ref: Option<ResourceRef>,
    /// Where this binding's Stream continued from, when it is a fork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<GatewayForkOrigin>,
    /// Recorded revision of the session's operative Context condition. The
    /// gateway represents the revision; revising the Context is Actuation's.
    #[serde(default = "default_context_revision")]
    pub context_revision: u64,
    pub ingress: GatewayIngressPolicy,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl GatewayBinding {
    pub fn validate(&self, descriptor: &ConnectorDescriptor) -> Result<()> {
        descriptor.validate()?;
        self.address.validate()?;
        if self.connector_ref != descriptor.connector_ref {
            return Err(AikitError::new(
                "agency_gateway.connector_identity_drift",
                format!(
                    "binding {} cites connector {} but descriptor is {}",
                    self.binding_ref, self.connector_ref, descriptor.connector_ref
                ),
            ));
        }
        if !self
            .address
            .platform
            .eq_ignore_ascii_case(&descriptor.platform)
        {
            return Err(AikitError::new(
                "agency_gateway.platform_drift",
                format!(
                    "binding {} platform {} does not match connector platform {}",
                    self.binding_ref, self.address.platform, descriptor.platform
                ),
            ));
        }
        let semantic = [
            ("agent_session_ref", &self.agent_session_ref),
            ("agency_ref", &self.agency_ref),
            ("actuation_ref", &self.actuation_ref),
            ("actuation_stream_ref", &self.actuation_stream_ref),
        ];
        for (index, (left_name, left)) in semantic.iter().enumerate() {
            for (right_name, right) in semantic.iter().skip(index + 1) {
                if left == right {
                    return Err(AikitError::new(
                        "agency_gateway.semantic_identity_collapse",
                        format!(
                            "binding {} collapses {left_name} and {right_name}",
                            self.binding_ref
                        ),
                    ));
                }
            }
        }
        if self
            .ingress
            .sender_overrides
            .keys()
            .any(|sender| sender.trim().is_empty())
        {
            return Err(AikitError::new(
                "agency_gateway.empty_sender_override",
                format!("binding {} has an empty sender override", self.binding_ref),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayStreamEvent {
    pub sequence: u64,
    pub event: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayStreamJournal {
    pub stream_ref: ResourceRef,
    pub actuation_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub agent_session_ref: ResourceRef,
    pub next_sequence: u64,
    #[serde(default)]
    pub events: Vec<GatewayStreamEvent>,
}

impl GatewayStreamJournal {
    fn for_binding(binding: &GatewayBinding) -> Self {
        Self {
            stream_ref: binding.actuation_stream_ref.clone(),
            actuation_ref: binding.actuation_ref.clone(),
            agency_ref: binding.agency_ref.clone(),
            agent_session_ref: binding.agent_session_ref.clone(),
            next_sequence: 1,
            events: Vec::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        let expected_next = self.events.len() as u64 + 1;
        if self.next_sequence != expected_next {
            return Err(AikitError::new(
                "agency_gateway.stream_cursor_drift",
                format!(
                    "Stream {} next sequence {} does not follow {} events",
                    self.stream_ref,
                    self.next_sequence,
                    self.events.len()
                ),
            ));
        }
        for (index, event) in self.events.iter().enumerate() {
            let expected = index as u64 + 1;
            if event.sequence != expected {
                return Err(AikitError::new(
                    "agency_gateway.stream_sequence_gap",
                    format!(
                        "Stream {} expected sequence {expected} but found {}",
                        self.stream_ref, event.sequence
                    ),
                ));
            }
            if event.event.get("sequence").and_then(Value::as_u64) != Some(expected) {
                return Err(AikitError::new(
                    "agency_gateway.stream_event_sequence_drift",
                    format!(
                        "Stream {} portable event does not carry canonical sequence {expected}",
                        self.stream_ref
                    ),
                ));
            }
        }
        Ok(())
    }

    fn ensure_binding(&self, binding: &GatewayBinding) -> Result<()> {
        if self.stream_ref != binding.actuation_stream_ref
            || self.actuation_ref != binding.actuation_ref
            || self.agency_ref != binding.agency_ref
            || self.agent_session_ref != binding.agent_session_ref
        {
            return Err(AikitError::new(
                "agency_gateway.stream_semantic_drift",
                format!(
                    "binding {} attempts to reuse Stream {} with different Actuation/Agency/AgentSession identity",
                    binding.binding_ref, self.stream_ref
                ),
            ));
        }
        Ok(())
    }

    fn append(&mut self, event: Value) -> Result<GatewayStreamEvent> {
        let sequence = self.next_sequence;
        if event.get("sequence").and_then(Value::as_u64) != Some(sequence) {
            return Err(AikitError::new(
                "agency_gateway.append_sequence_mismatch",
                format!(
                    "Stream {} append expected sequence {sequence}",
                    self.stream_ref
                ),
            ));
        }
        let item = GatewayStreamEvent { sequence, event };
        self.events.push(item.clone());
        self.next_sequence += 1;
        Ok(item)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayReplay {
    pub stream_ref: ResourceRef,
    pub after_sequence: u64,
    pub returned_through: u64,
    pub stream_last_sequence: u64,
    pub has_more: bool,
    pub events: Vec<GatewayStreamEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum GatewayIngressResult {
    Appended {
        binding_ref: ResourceRef,
        stream_ref: ResourceRef,
        event: GatewayStreamEvent,
    },
    PairingRequired {
        binding_ref: ResourceRef,
        sender: SenderIdentity,
    },
    Denied {
        binding_ref: ResourceRef,
        sender: SenderIdentity,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayActuationControlOperation {
    Interrupt,
    Cancel,
}

/// Canonical conversation-control operations. These are gateway-native: a
/// connector surface may spell them with slash strings at its own edge, a
/// carrier client may send them directly, but the operation itself is this
/// enum — never a parsed string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum GatewayConversationOperation {
    /// Conversation-scoped detail: binding, stream position, in-flight turn,
    /// agent backing and connector health.
    Status,
    /// Interrupt the in-flight turn of the binding's agent session.
    Stop,
    /// Fresh turn context for the binding: a forked Stream and a new
    /// AgentSession generation under the same connector conversation. The old
    /// Stream is retained in the journal and named in the result.
    New,
    /// List the agent sessions behind this gateway's bindings.
    Sessions,
    /// A drained, state-preserving rematerialisation: stop admitting new
    /// turn work, resolve the in-flight turn under the bounded policy, persist
    /// the snapshot, answer, and exit so the service manager restarts it.
    Restart,
    /// Stop admitting new connector ingress for one connector; the pump keeps
    /// serving outbound work and reports the pause in its health. `None`
    /// targets the requesting conversation's own connector.
    PauseConnector {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connector_ref: Option<ResourceRef>,
    },
    /// Resume connector ingress after a pause.
    ResumeConnector {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connector_ref: Option<ResourceRef>,
    },
    /// The binding's harness native model selector. `None` lists what the
    /// harness itself discloses (its controls, and a roster when it offers
    /// one); `Some(id)` selects a provider-advertised model through the same
    /// native seam. The gateway never invents a parallel notion of models.
    Model {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// The binding's agent backing and the available provider ids (name +
    /// label + protocol only). Read-only: switching a backing mid-session is a
    /// session-replacement event this kernel does not own, so the answer
    /// discloses the exact command instead of performing it.
    Harness,
    /// The aikit skill surface available to the backed harness, with the
    /// invocation law disclosed: the harness carries skills in-turn; the
    /// gateway does not execute skills.
    Skills,
    /// A connector-originated ask: one attributable Communique from the asking
    /// agency to the named Position, routed exactly as `gateway send` routes
    /// (local occupancy, cross-Workcell relay, held/vacant), with the asking
    /// agent session and connector conversation carried as origin provenance.
    /// The connector edge spells it `/ask <position-or-@handle> <message>`;
    /// the routing behind it is resolved by the ask router the service wires
    /// (`GatewayConversationHooks::ask_router`), and the append is this
    /// kernel's own journal work.
    AskPosition { position: String, message: String },
    /// Say one line into the binding's own conversation, through its
    /// connector, from the gateway itself. This is how something that
    /// outlived a restart (an upgrade's finaliser) reports back to the
    /// conversation that asked for it: the running gateway journals and
    /// queues the line exactly as it does any other reply. Owner scope only.
    Announce { text: String },
    /// Read the upgrade plan for this gateway (`apply: false`) or start the
    /// managed upgrade (`apply: true`) on behalf of this conversation. The
    /// upgrade itself runs in a worker that outlives this process; the
    /// receipt comes back here when the new build is verified running.
    Upgrade {
        #[serde(default)]
        apply: bool,
    },
}

/// One agent reply (or honest turn failure) to be journaled on the same
/// Stream the inbound message appended to, attributed to the binding's agent
/// session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayAgentReply {
    pub binding_ref: ResourceRef,
    /// The journal sequence of the inbound event this answers.
    pub in_reply_to_sequence: u64,
    pub text: String,
    /// Set when the turn failed or was interrupted: the text carried to the
    /// surface says so, and the journaled event is a turn-failure record,
    /// never a fabricated successful reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<GatewayAgentReplyFailure>,
}

/// Why a turn produced no real answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayAgentReplyFailure {
    Failed { reason: String },
    Interrupted { detail: Option<String> },
}

/// Portable control intent. A harness/Actuation control adapter performs the
/// actual operation only when the realised body supports and authorises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayActuationControlIntent {
    pub operation: GatewayActuationControlOperation,
    pub binding_ref: ResourceRef,
    pub agent_session_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub actuation_ref: ResourceRef,
    pub actuation_stream_ref: ResourceRef,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayDiscovery {
    pub version: String,
    pub gateway_ref: ResourceRef,
    pub connector_sdk_version: String,
    pub connector_wire_version: String,
    pub connectors: Vec<ConnectorDescriptor>,
    pub bindings: Vec<GatewayBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub version: String,
    pub gateway_ref: ResourceRef,
    pub connector_count: usize,
    pub binding_count: usize,
    pub stream_count: usize,
    pub pending_delivery_count: usize,
    pub delivery_receipt_count: usize,
    #[serde(default)]
    pub connector_health: Vec<ConnectorHealth>,
    /// The running process: which build, since when, from which executable.
    /// Absent in an offline reading (no process is serving) and on a gateway
    /// built before it was disclosed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<GatewayBuildIdentity>,
    /// How each carrier is bound and what scope it grants. Empty when no
    /// service is serving.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<GatewayListenerReading>,
}

/// One conversation turn a drain met.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainedTurn {
    pub binding_ref: String,
    pub in_reply_to_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What a drain found and did, exact rather than counted: work that could not
/// finish is named so nobody has to guess what a restart cost.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DrainReport {
    /// Whether a drain actually ran and counted. `false` means the counts below
    /// are UNKNOWN, not zero: a predecessor that predates the drain was stopped
    /// with its clean shutdown, and what it had in flight at that moment was
    /// never read.
    #[serde(default)]
    pub measured: bool,
    pub reason: String,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    /// The bounded grace each in-flight turn was given before being
    /// interrupted.
    pub grace_ms: u64,
    /// Turns that finished inside the grace.
    pub turns_resolved: Vec<DrainedTurn>,
    /// Turns interrupted because they did not finish. Whatever the model or
    /// its tools did before the interrupt is an UNCERTAIN effect: it is
    /// recorded, journaled as an interruption, and never replayed.
    pub turns_interrupted: Vec<DrainedTurn>,
    /// Outbound operations prepared and not receipted. They stay in the
    /// persisted state; a restart does not re-send one it cannot prove was
    /// not sent.
    pub pending_operations: Vec<String>,
    /// Communiques waiting at this gateway, by Position. They are in the
    /// journal and survive the restart.
    pub communiques: Vec<CommuniqueCount>,
}

impl DrainReport {
    /// Whether a drain actually ran and counted. A report from a gateway that has
    /// the drain but predates this field carries no `measured` flag; it does carry
    /// the times the drain started and finished, which a default (never-run) report
    /// does not. Reading only the flag would call a real drain "not measured".
    pub fn was_measured(&self) -> bool {
        self.measured || self.started_at_unix_ms != 0
    }
}

/// Qualitatively distinct co-internal relations the gateway ecology can name.
/// Listing a mode discloses that the relation is representable; it never
/// authorises it. Authority is a separate AIKit capability grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayInvocationMode {
    Communique,
    SessionContribution,
    Delegation,
    SessionFork,
    CoActuation,
}

/// Every mode is always representable; none is ever implied by presence.
pub const GATEWAY_INVOCATION_MODES: [GatewayInvocationMode; 5] = [
    GatewayInvocationMode::Communique,
    GatewayInvocationMode::SessionContribution,
    GatewayInvocationMode::Delegation,
    GatewayInvocationMode::SessionFork,
    GatewayInvocationMode::CoActuation,
];

pub const GATEWAY_ECOLOGY_AUTHORITY_LAW: &str = "presence-does-not-imply-authority";

/// One live Surface projection of a session, as the ecology read model sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayEcologySurface {
    pub binding_ref: ResourceRef,
    pub connector_ref: ResourceRef,
    pub platform: String,
    pub address: ConversationAddress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_ref: Option<ResourceRef>,
    pub ingress_default: GatewayIngressDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<GatewayForkOrigin>,
    pub context_revision: u64,
}

/// Journal-backed stream summary inside the ecology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayEcologyStream {
    pub stream_ref: ResourceRef,
    pub last_sequence: u64,
    pub event_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayEcologySession {
    pub agent_session_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub actuation_refs: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_ref: Option<ResourceRef>,
    pub streams: Vec<GatewayEcologyStream>,
    pub surfaces: Vec<GatewayEcologySurface>,
    pub invocation_modes: Vec<GatewayInvocationMode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayEcologyAgency {
    pub agency_ref: ResourceRef,
    pub sessions: Vec<GatewayEcologySession>,
}

/// Derived live-agency read model: which Agencies, AgentSessions, Streams and
/// Surfaces this gateway currently constitutes, with the invocation vocabulary
/// and the authority law disclosed. Derived from bindings and journals — not a
/// second registry; the SessionSpace/capability layers remain the authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayEcology {
    pub version: String,
    pub gateway_ref: ResourceRef,
    pub authority: String,
    pub agencies: Vec<GatewayEcologyAgency>,
}

/// Serialisable semantic state sufficient to reconstruct the gateway after a
/// process/material-service restart. It deliberately carries no PID/socket/
/// Workcell allocation identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewaySnapshot {
    pub version: String,
    pub gateway_ref: ResourceRef,
    #[serde(default)]
    pub connectors: Vec<ConnectorDescriptor>,
    #[serde(default)]
    pub bindings: Vec<GatewayBinding>,
    #[serde(default)]
    pub streams: Vec<GatewayStreamJournal>,
    #[serde(default)]
    pub connector_health: Vec<ConnectorHealth>,
    #[serde(default)]
    pub pending_deliveries: Vec<OutboundOperation>,
    #[serde(default)]
    pub delivery_receipts: Vec<DeliveryReceipt>,
    pub next_operation_sequence: u64,
    /// The Communique journal (`aikit.communique/v1`), in journal order.
    /// Absent from snapshots that never carried contact, so older state
    /// files restore unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub communiques: Vec<Communique>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GatewayRouteKey {
    connector_ref: ResourceRef,
    address: ConversationAddress,
}

impl GatewayRouteKey {
    fn new(connector_ref: ResourceRef, address: ConversationAddress) -> Self {
        Self {
            connector_ref,
            address,
        }
    }
}

/// In-memory semantic kernel. Persistence is represented by [`GatewaySnapshot`]
/// so Workcell/host integrations may choose their own durable store.
pub struct AgencyGateway {
    gateway_ref: ResourceRef,
    connectors: BTreeMap<ResourceRef, ConnectorDescriptor>,
    bindings: BTreeMap<ResourceRef, GatewayBinding>,
    routes: BTreeMap<GatewayRouteKey, ResourceRef>,
    streams: BTreeMap<ResourceRef, GatewayStreamJournal>,
    connector_health: BTreeMap<ResourceRef, ConnectorHealth>,
    pending_deliveries: BTreeMap<ResourceRef, OutboundOperation>,
    delivery_receipts: Vec<DeliveryReceipt>,
    next_operation_sequence: u64,
    communiques: CommuniqueJournal,
}

impl AgencyGateway {
    pub fn new(gateway_ref: ResourceRef) -> Self {
        Self {
            gateway_ref,
            connectors: BTreeMap::new(),
            bindings: BTreeMap::new(),
            routes: BTreeMap::new(),
            streams: BTreeMap::new(),
            connector_health: BTreeMap::new(),
            pending_deliveries: BTreeMap::new(),
            delivery_receipts: Vec::new(),
            next_operation_sequence: 1,
            communiques: CommuniqueJournal::default(),
        }
    }

    /// The Communique journal this gateway keeps.
    pub fn communiques(&self) -> &CommuniqueJournal {
        &self.communiques
    }

    /// Replace this gateway's own ref. Records already journaled keep the
    /// `origin_gateway_ref` they were written under; only what is written from
    /// now on carries the new one.
    pub fn set_gateway_ref(&mut self, gateway_ref: ResourceRef) {
        self.gateway_ref = gateway_ref;
    }

    /// The refs of outbound operations prepared and not yet receipted.
    pub fn pending_operation_refs(&self) -> Vec<String> {
        self.pending_deliveries
            .keys()
            .map(|reference| reference.to_string())
            .collect()
    }

    pub fn gateway_ref(&self) -> &ResourceRef {
        &self.gateway_ref
    }

    pub fn register_connector(&mut self, descriptor: ConnectorDescriptor) -> Result<()> {
        descriptor.validate()?;
        match self.connectors.get(&descriptor.connector_ref) {
            Some(existing) if existing == &descriptor => return Ok(()),
            Some(existing) if !existing.platform.eq_ignore_ascii_case(&descriptor.platform) => {
                return Err(AikitError::new(
                    "agency_gateway.connector_platform_rewrite",
                    format!(
                        "connector {} cannot change platform from {} to {}",
                        descriptor.connector_ref, existing.platform, descriptor.platform
                    ),
                ));
            }
            _ => {}
        }
        self.connectors
            .insert(descriptor.connector_ref.clone(), descriptor);
        Ok(())
    }

    pub fn bind(&mut self, binding: GatewayBinding) -> Result<()> {
        let descriptor = self.connectors.get(&binding.connector_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_connector",
                format!(
                    "binding {} refers to unregistered connector {}",
                    binding.binding_ref, binding.connector_ref
                ),
            )
        })?;
        binding.validate(descriptor)?;
        if let Some(existing) = self.bindings.get(&binding.binding_ref) {
            if existing == &binding {
                return Ok(());
            }
            return Err(AikitError::new(
                "agency_gateway.binding_identity_rewrite",
                format!(
                    "binding ref {} already names a different route",
                    binding.binding_ref
                ),
            ));
        }
        if binding.context_revision == 0 {
            return Err(AikitError::new(
                "agency_gateway.invalid_context_revision",
                format!(
                    "binding {} records context revision 0; revisions start at 1",
                    binding.binding_ref
                ),
            ));
        }
        if let Some(origin) = &binding.forked_from {
            if origin.stream_ref == binding.actuation_stream_ref {
                return Err(AikitError::new(
                    "agency_gateway.fork_origin_self",
                    format!(
                        "binding {} forks Stream {} from itself",
                        binding.binding_ref, origin.stream_ref
                    ),
                ));
            }
            let origin_stream = self.streams.get(&origin.stream_ref).ok_or_else(|| {
                AikitError::new(
                    "agency_gateway.unknown_fork_origin",
                    format!(
                        "binding {} forks from unknown Stream {}",
                        binding.binding_ref, origin.stream_ref
                    ),
                )
            })?;
            let origin_last = origin_stream.next_sequence.saturating_sub(1);
            if origin.at_sequence == 0 || origin.at_sequence > origin_last {
                return Err(AikitError::new(
                    "agency_gateway.fork_origin_sequence",
                    format!(
                        "binding {} forks Stream {} at sequence {} outside journal 1..={origin_last}",
                        binding.binding_ref, origin.stream_ref, origin.at_sequence
                    ),
                ));
            }
        }
        let route = GatewayRouteKey::new(binding.connector_ref.clone(), binding.address.clone());
        if let Some(existing_binding) = self.routes.get(&route) {
            return Err(AikitError::new(
                "agency_gateway.route_already_bound",
                format!(
                    "connector conversation is already bound through {}",
                    existing_binding
                ),
            ));
        }
        if let Some(stream) = self.streams.get(&binding.actuation_stream_ref) {
            stream.ensure_binding(&binding)?;
        }
        self.routes.insert(route, binding.binding_ref.clone());
        self.bindings.insert(binding.binding_ref.clone(), binding);
        Ok(())
    }

    pub fn unbind(&mut self, binding_ref: &ResourceRef) -> Result<GatewayBinding> {
        let binding = self.bindings.remove(binding_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_binding",
                format!("gateway binding {binding_ref} does not exist"),
            )
        })?;
        self.routes.remove(&GatewayRouteKey::new(
            binding.connector_ref.clone(),
            binding.address.clone(),
        ));
        Ok(binding)
    }

    pub fn ingest(&mut self, event: InboundEvent) -> Result<GatewayIngressResult> {
        let descriptor = self.connectors.get(&event.connector_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_connector",
                format!(
                    "inbound event {} uses an unregistered connector",
                    event.event_ref
                ),
            )
        })?;
        event.validate(descriptor)?;
        let route = GatewayRouteKey::new(event.connector_ref.clone(), event.address.clone());
        let binding_ref = self.routes.get(&route).cloned().ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unbound_conversation",
                format!(
                    "inbound conversation {} on {} has no canonical AgentSession binding",
                    event.address.conversation_id, event.address.platform
                ),
            )
        })?;
        let binding = self
            .bindings
            .get(&binding_ref)
            .cloned()
            .expect("route index only contains known bindings");

        match binding.ingress.decision_for(&event.sender) {
            GatewayIngressDecision::Pair => Ok(GatewayIngressResult::PairingRequired {
                binding_ref,
                sender: event.sender,
            }),
            GatewayIngressDecision::Deny => Ok(GatewayIngressResult::Denied {
                binding_ref,
                sender: event.sender,
            }),
            GatewayIngressDecision::Allow => {
                let stream = self
                    .streams
                    .entry(binding.actuation_stream_ref.clone())
                    .or_insert_with(|| GatewayStreamJournal::for_binding(&binding));
                stream.ensure_binding(&binding)?;
                let portable = portable_inbound_event(&binding, &event, stream.next_sequence);
                let appended = stream.append(portable)?;
                Ok(GatewayIngressResult::Appended {
                    binding_ref,
                    stream_ref: binding.actuation_stream_ref,
                    event: appended,
                })
            }
        }
    }

    pub fn replay(
        &self,
        stream_ref: &ResourceRef,
        after_sequence: u64,
        limit: usize,
    ) -> Result<GatewayReplay> {
        let stream = self.streams.get(stream_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_stream",
                format!("gateway has no journal for Stream {stream_ref}"),
            )
        })?;
        stream.validate()?;
        let events = stream
            .events
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let returned_through = events
            .last()
            .map(|event| event.sequence)
            .unwrap_or(after_sequence);
        let stream_last_sequence = stream.next_sequence.saturating_sub(1);
        Ok(GatewayReplay {
            stream_ref: stream_ref.clone(),
            after_sequence,
            returned_through,
            stream_last_sequence,
            has_more: returned_through < stream_last_sequence,
            events,
        })
    }

    pub fn prepare_operation(
        &mut self,
        binding_ref: &ResourceRef,
        operation: OutboundOperationKind,
    ) -> Result<OutboundOperation> {
        let binding = self.bindings.get(binding_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_binding",
                format!("gateway binding {binding_ref} does not exist"),
            )
        })?;
        let descriptor = self.connectors.get(&binding.connector_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_connector",
                format!("binding {binding_ref} has no registered connector"),
            )
        })?;
        let operation_ref = ResourceRef::parse(format!(
            "gateway-operation/{:020}",
            self.next_operation_sequence
        ))
        .map_err(|error| {
            AikitError::new(
                "agency_gateway.operation_ref",
                format!("failed to construct operation ref: {error}"),
            )
        })?;
        self.next_operation_sequence += 1;
        let prepared = OutboundOperation {
            operation_ref: operation_ref.clone(),
            connector_ref: binding.connector_ref.clone(),
            address: binding.address.clone(),
            operation,
            agent_session_ref: Some(binding.agent_session_ref.clone()),
            actuation_stream_ref: Some(binding.actuation_stream_ref.clone()),
            provenance: vec![
                format!("Agency Gateway {AGENCY_GATEWAY_VERSION}"),
                format!("binding {}", binding.binding_ref),
            ],
        };
        prepared.validate(descriptor)?;
        self.pending_deliveries
            .insert(operation_ref, prepared.clone());
        Ok(prepared)
    }

    pub fn record_delivery(&mut self, receipt: DeliveryReceipt) -> Result<()> {
        let pending = self
            .pending_deliveries
            .get(&receipt.operation_ref)
            .ok_or_else(|| {
                AikitError::new(
                    "agency_gateway.unknown_delivery",
                    format!(
                        "delivery receipt refers to unknown operation {}",
                        receipt.operation_ref
                    ),
                )
            })?;
        if pending.connector_ref != receipt.connector_ref {
            return Err(AikitError::new(
                "agency_gateway.delivery_connector_drift",
                format!(
                    "delivery receipt {} changed connector identity",
                    receipt.operation_ref
                ),
            ));
        }
        self.pending_deliveries.remove(&receipt.operation_ref);
        self.delivery_receipts.push(receipt);
        Ok(())
    }

    pub fn set_connector_health(&mut self, health: ConnectorHealth) -> Result<()> {
        if !self.connectors.contains_key(&health.connector_ref) {
            return Err(AikitError::new(
                "agency_gateway.unknown_connector",
                format!(
                    "health observation refers to unregistered connector {}",
                    health.connector_ref
                ),
            ));
        }
        self.connector_health
            .insert(health.connector_ref.clone(), health);
        Ok(())
    }

    pub fn control_intent(
        &self,
        binding_ref: &ResourceRef,
        operation: GatewayActuationControlOperation,
    ) -> Result<GatewayActuationControlIntent> {
        let binding = self.bindings.get(binding_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_binding",
                format!("gateway binding {binding_ref} does not exist"),
            )
        })?;
        Ok(GatewayActuationControlIntent {
            operation,
            binding_ref: binding.binding_ref.clone(),
            agent_session_ref: binding.agent_session_ref.clone(),
            agency_ref: binding.agency_ref.clone(),
            actuation_ref: binding.actuation_ref.clone(),
            actuation_stream_ref: binding.actuation_stream_ref.clone(),
            provenance: vec![format!("Agency Gateway {AGENCY_GATEWAY_VERSION}")],
        })
    }

    /// The binding bound to one connector conversation, if any.
    pub fn binding_by_route(
        &self,
        connector_ref: &ResourceRef,
        address: &ConversationAddress,
    ) -> Option<&GatewayBinding> {
        let binding_ref = self.routes.get(&GatewayRouteKey::new(
            connector_ref.clone(),
            address.clone(),
        ))?;
        self.bindings.get(binding_ref)
    }

    /// One binding by ref.
    pub fn binding(&self, binding_ref: &ResourceRef) -> Option<&GatewayBinding> {
        self.bindings.get(binding_ref)
    }

    /// Whether the connector advertises an operation (e.g. Typing for a
    /// best-effort indicator before a turn).
    pub fn connector_supports(
        &self,
        connector_ref: &ResourceRef,
        operation: ConnectorOperation,
    ) -> bool {
        self.connectors
            .get(connector_ref)
            .is_some_and(|descriptor| descriptor.capabilities.operations.contains(&operation))
    }

    /// One stream journal read view: last sequence and event count.
    pub fn stream_position(&self, stream_ref: &ResourceRef) -> Option<(u64, usize)> {
        self.streams
            .get(stream_ref)
            .map(|stream| (stream.next_sequence.saturating_sub(1), stream.events.len()))
    }

    /// Append an agent turn's reply (or its honest failure record) to the same
    /// Stream journal the inbound human event appended to, attributed to the
    /// binding's agent session. This is the kernel's only door for an agent
    /// reply: an inbound human event is never fabricated to carry one.
    pub fn record_agent_reply(&mut self, reply: GatewayAgentReply) -> Result<GatewayStreamEvent> {
        let binding = self.bindings.get(&reply.binding_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_binding",
                format!(
                    "agent reply cites binding {} which does not exist",
                    reply.binding_ref
                ),
            )
        })?;
        let binding = binding.clone();
        let stream = self
            .streams
            .entry(binding.actuation_stream_ref.clone())
            .or_insert_with(|| GatewayStreamJournal::for_binding(&binding));
        stream.ensure_binding(&binding)?;
        let sequence = stream.next_sequence;
        let mut metadata = Map::new();
        metadata.insert(
            "agent_session_ref".into(),
            json!(binding.agent_session_ref.to_string()),
        );
        metadata.insert("agency_ref".into(), json!(binding.agency_ref.to_string()));
        metadata.insert(
            "actuation_ref".into(),
            json!(binding.actuation_ref.to_string()),
        );
        metadata.insert(
            "in_reply_to_sequence".into(),
            json!(reply.in_reply_to_sequence),
        );
        let (kind, custom_kind) = match &reply.failure {
            None => ("agent-message", None),
            Some(failure) => {
                metadata.insert(
                    "failure".into(),
                    json!(match failure {
                        GatewayAgentReplyFailure::Failed { reason } => json!({
                            "kind": "failed",
                            "reason": reason,
                        }),
                        GatewayAgentReplyFailure::Interrupted { detail } => json!({
                            "kind": "interrupted",
                            "detail": detail,
                        }),
                    }),
                );
                ("custom", Some("gateway-agent/turn-failure"))
            }
        };
        let mut event = Map::new();
        event.insert(
            "event_ref".into(),
            json!(format!(
                "{}/gateway-event/{sequence}",
                binding.actuation_stream_ref
            )),
        );
        event.insert("sequence".into(), json!(sequence));
        event.insert("kind".into(), json!(kind));
        if let Some(custom_kind) = custom_kind {
            event.insert("custom_kind".into(), json!(custom_kind));
        }
        event.insert(
            "native_trace_ref".into(),
            json!(format!(
                "gateway-agent-reply/{}",
                reply.in_reply_to_sequence
            )),
        );
        event.insert("disclosure".into(), json!("portable"));
        event.insert("metadata".into(), Value::Object(metadata));
        if let Some(surface_ref) = &binding.surface_ref {
            event.insert("surface_ref".into(), json!(surface_ref.to_string()));
        }
        event.insert("content".into(), json!(reply.text));
        stream.append(Value::Object(event))
    }

    /// Append one honest turn-activity record to the binding's Stream journal
    /// — the additive record of what an in-flight agent turn is doing right
    /// now: a tool line, or a named streaming fallback. One custom event per
    /// line, attributed to the binding's agent session like a reply. This is
    /// the kernel's only door for in-turn notes; it never fabricates inbound
    /// or reply events, and a note never stands in for the turn's answer.
    pub fn record_agent_activity(
        &mut self,
        binding_ref: &ResourceRef,
        in_reply_to_sequence: u64,
        line: &str,
    ) -> Result<GatewayStreamEvent> {
        let binding = self.bindings.get(binding_ref).ok_or_else(|| {
            AikitError::new(
                "agency_gateway.unknown_binding",
                format!(
                    "agent activity cites binding {} which does not exist",
                    binding_ref
                ),
            )
        })?;
        let binding = binding.clone();
        let stream = self
            .streams
            .entry(binding.actuation_stream_ref.clone())
            .or_insert_with(|| GatewayStreamJournal::for_binding(&binding));
        stream.ensure_binding(&binding)?;
        let sequence = stream.next_sequence;
        let mut metadata = Map::new();
        metadata.insert(
            "agent_session_ref".into(),
            json!(binding.agent_session_ref.to_string()),
        );
        metadata.insert("agency_ref".into(), json!(binding.agency_ref.to_string()));
        metadata.insert(
            "actuation_ref".into(),
            json!(binding.actuation_ref.to_string()),
        );
        metadata.insert("in_reply_to_sequence".into(), json!(in_reply_to_sequence));
        let mut event = Map::new();
        event.insert(
            "event_ref".into(),
            json!(format!(
                "{}/gateway-event/{sequence}",
                binding.actuation_stream_ref
            )),
        );
        event.insert("sequence".into(), json!(sequence));
        event.insert("kind".into(), json!("custom"));
        event.insert("custom_kind".into(), json!("gateway-agent/turn-activity"));
        event.insert(
            "native_trace_ref".into(),
            json!(format!("gateway-agent-activity/{in_reply_to_sequence}")),
        );
        event.insert("disclosure".into(), json!("portable"));
        event.insert("metadata".into(), Value::Object(metadata));
        if let Some(surface_ref) = &binding.surface_ref {
            event.insert("surface_ref".into(), json!(surface_ref.to_string()));
        }
        event.insert("content".into(), json!(line));
        stream.append(Value::Object(event))
    }

    /// The recorded delivery receipt for one prepared operation, once its
    /// receipt has arrived. The streaming reply path reads the anchor Send's
    /// native message id here — the id its edits then target.
    pub fn delivery_receipt(&self, operation_ref: &ResourceRef) -> Option<&DeliveryReceipt> {
        self.delivery_receipts
            .iter()
            .find(|receipt| &receipt.operation_ref == operation_ref)
    }

    pub fn discovery(&self) -> GatewayDiscovery {
        GatewayDiscovery {
            version: AGENCY_GATEWAY_VERSION.into(),
            gateway_ref: self.gateway_ref.clone(),
            connector_sdk_version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
            connectors: self.connectors.values().cloned().collect(),
            bindings: self.bindings.values().cloned().collect(),
        }
    }

    pub fn status(&self) -> GatewayStatus {
        GatewayStatus {
            version: AGENCY_GATEWAY_VERSION.into(),
            gateway_ref: self.gateway_ref.clone(),
            connector_count: self.connectors.len(),
            binding_count: self.bindings.len(),
            stream_count: self.streams.len(),
            pending_delivery_count: self.pending_deliveries.len(),
            delivery_receipt_count: self.delivery_receipts.len(),
            connector_health: self.connector_health.values().cloned().collect(),
            build: None,
            listeners: Vec::new(),
        }
    }

    /// Derive the live ecology from bindings and journals. A session appears
    /// once per (Agency, AgentSession); journals with no live binding still
    /// appear, with no surfaces — a stream survives Surface loss.
    pub fn ecology(&self) -> GatewayEcology {
        struct SessionAccumulator {
            agency_ref: ResourceRef,
            agent_session_ref: ResourceRef,
            actuation_refs: BTreeSet<ResourceRef>,
            agent_ref: Option<ResourceRef>,
            harness_ref: Option<ResourceRef>,
            stream_refs: BTreeSet<ResourceRef>,
            surfaces: Vec<GatewayEcologySurface>,
        }
        fn session_entry(
            sessions: &mut BTreeMap<(ResourceRef, ResourceRef), SessionAccumulator>,
            agency_ref: ResourceRef,
            agent_session_ref: ResourceRef,
        ) -> &mut SessionAccumulator {
            sessions
                .entry((agency_ref.clone(), agent_session_ref.clone()))
                .or_insert_with(|| SessionAccumulator {
                    agency_ref,
                    agent_session_ref,
                    actuation_refs: BTreeSet::new(),
                    agent_ref: None,
                    harness_ref: None,
                    stream_refs: BTreeSet::new(),
                    surfaces: Vec::new(),
                })
        }
        let mut sessions: BTreeMap<(ResourceRef, ResourceRef), SessionAccumulator> =
            BTreeMap::new();
        for binding in self.bindings.values() {
            let session = session_entry(
                &mut sessions,
                binding.agency_ref.clone(),
                binding.agent_session_ref.clone(),
            );
            session.actuation_refs.insert(binding.actuation_ref.clone());
            session
                .stream_refs
                .insert(binding.actuation_stream_ref.clone());
            if session.agent_ref.is_none() {
                session.agent_ref = binding.agent_ref.clone();
            }
            if session.harness_ref.is_none() {
                session.harness_ref = binding.harness_ref.clone();
            }
            session.surfaces.push(GatewayEcologySurface {
                binding_ref: binding.binding_ref.clone(),
                connector_ref: binding.connector_ref.clone(),
                platform: binding.address.platform.clone(),
                address: binding.address.clone(),
                surface_ref: binding.surface_ref.clone(),
                ingress_default: binding.ingress.default,
                forked_from: binding.forked_from.clone(),
                context_revision: binding.context_revision,
            });
        }
        for stream in self.streams.values() {
            let session = session_entry(
                &mut sessions,
                stream.agency_ref.clone(),
                stream.agent_session_ref.clone(),
            );
            session.actuation_refs.insert(stream.actuation_ref.clone());
            session.stream_refs.insert(stream.stream_ref.clone());
        }
        let mut agencies: BTreeMap<ResourceRef, Vec<GatewayEcologySession>> = BTreeMap::new();
        for (
            _,
            SessionAccumulator {
                agency_ref,
                agent_session_ref,
                actuation_refs,
                agent_ref,
                harness_ref,
                stream_refs,
                surfaces,
            },
        ) in sessions
        {
            let streams = stream_refs
                .into_iter()
                .map(|stream_ref| {
                    let journal = self.streams.get(&stream_ref);
                    GatewayEcologyStream {
                        stream_ref,
                        last_sequence: journal
                            .map(|journal| journal.next_sequence.saturating_sub(1))
                            .unwrap_or(0),
                        event_count: journal.map(|journal| journal.events.len()).unwrap_or(0),
                    }
                })
                .collect();
            agencies
                .entry(agency_ref.clone())
                .or_default()
                .push(GatewayEcologySession {
                    agent_session_ref,
                    agency_ref,
                    actuation_refs: actuation_refs.into_iter().collect(),
                    agent_ref,
                    harness_ref,
                    streams,
                    surfaces,
                    invocation_modes: GATEWAY_INVOCATION_MODES.to_vec(),
                });
        }
        GatewayEcology {
            version: AGENCY_GATEWAY_VERSION.into(),
            gateway_ref: self.gateway_ref.clone(),
            authority: GATEWAY_ECOLOGY_AUTHORITY_LAW.into(),
            agencies: agencies
                .into_iter()
                .map(|(agency_ref, sessions)| GatewayEcologyAgency {
                    agency_ref,
                    sessions,
                })
                .collect(),
        }
    }

    pub fn snapshot(&self) -> GatewaySnapshot {
        GatewaySnapshot {
            version: AGENCY_GATEWAY_VERSION.into(),
            gateway_ref: self.gateway_ref.clone(),
            connectors: self.connectors.values().cloned().collect(),
            bindings: self.bindings.values().cloned().collect(),
            streams: self.streams.values().cloned().collect(),
            connector_health: self.connector_health.values().cloned().collect(),
            pending_deliveries: self.pending_deliveries.values().cloned().collect(),
            delivery_receipts: self.delivery_receipts.clone(),
            next_operation_sequence: self.next_operation_sequence,
            communiques: self.communiques.records().to_vec(),
        }
    }

    pub fn from_snapshot(snapshot: GatewaySnapshot) -> Result<Self> {
        if snapshot.version != AGENCY_GATEWAY_VERSION {
            return Err(AikitError::new(
                "agency_gateway.unsupported_snapshot",
                format!("unsupported gateway snapshot version {}", snapshot.version),
            ));
        }
        if snapshot.next_operation_sequence == 0 {
            return Err(AikitError::new(
                "agency_gateway.invalid_operation_cursor",
                "gateway operation sequence starts at 1",
            ));
        }
        let mut gateway = Self::new(snapshot.gateway_ref);
        for descriptor in snapshot.connectors {
            gateway.register_connector(descriptor)?;
        }
        // Streams restore before bindings so fork lineage and stream/binding
        // compatibility validate against the full journal set.
        for stream in snapshot.streams {
            stream.validate()?;
            let stream_ref = stream.stream_ref.clone();
            if gateway.streams.insert(stream_ref.clone(), stream).is_some() {
                return Err(AikitError::new(
                    "agency_gateway.duplicate_stream_snapshot",
                    format!("snapshot contains Stream {stream_ref} more than once"),
                ));
            }
        }
        for binding in snapshot.bindings {
            gateway.bind(binding)?;
        }
        for health in snapshot.connector_health {
            gateway.set_connector_health(health)?;
        }
        for operation in snapshot.pending_deliveries {
            if gateway
                .pending_deliveries
                .insert(operation.operation_ref.clone(), operation)
                .is_some()
            {
                return Err(AikitError::new(
                    "agency_gateway.duplicate_pending_delivery",
                    "snapshot repeats a pending delivery operation ref",
                ));
            }
        }
        gateway.delivery_receipts = snapshot.delivery_receipts;
        gateway.next_operation_sequence = snapshot.next_operation_sequence;
        gateway.communiques = CommuniqueJournal::restore(snapshot.communiques)?;
        Ok(gateway)
    }
}

fn portable_inbound_event(
    binding: &GatewayBinding,
    inbound: &InboundEvent,
    sequence: u64,
) -> Value {
    let (kind, custom_kind) = match inbound.kind {
        InboundEventKind::Message | InboundEventKind::Media | InboundEventKind::Command => {
            ("human-message", None)
        }
        InboundEventKind::Reaction => ("custom", Some("gateway-inbound/reaction")),
        InboundEventKind::Membership => ("custom", Some("gateway-inbound/membership")),
        InboundEventKind::Custom => (
            "custom",
            inbound
                .custom_kind
                .as_deref()
                .or(Some("gateway-inbound/custom")),
        ),
    };

    let mut metadata = Map::new();
    metadata.insert(
        "connector_ref".into(),
        json!(inbound.connector_ref.to_string()),
    );
    metadata.insert(
        "connector_event_ref".into(),
        json!(inbound.event_ref.to_string()),
    );
    metadata.insert("platform".into(), json!(inbound.address.platform));
    metadata.insert(
        "conversation_id".into(),
        json!(inbound.address.conversation_id),
    );
    metadata.insert(
        "native_sender_id".into(),
        json!(inbound.sender.native_sender_id),
    );
    metadata.insert("sender_kind".into(), json!(inbound.sender.kind));
    if let Some(scope_id) = &inbound.address.scope_id {
        metadata.insert("scope_id".into(), json!(scope_id));
    }
    if let Some(thread_id) = &inbound.address.thread_id {
        metadata.insert("thread_id".into(), json!(thread_id));
    }
    if let Some(native_event_id) = &inbound.native_event_id {
        metadata.insert("native_event_id".into(), json!(native_event_id));
    }
    if let Some(native_message_id) = &inbound.native_message_id {
        metadata.insert("native_message_id".into(), json!(native_message_id));
    }
    if let Some(reply_to) = &inbound.reply_to_native_message_id {
        metadata.insert("reply_to_native_message_id".into(), json!(reply_to));
    }
    if !inbound.native.is_empty() {
        metadata.insert("native".into(), json!(inbound.native));
    }
    if !inbound.provenance.is_empty() {
        metadata.insert("connector_provenance".into(), json!(inbound.provenance));
    }

    let mut event = Map::new();
    event.insert(
        "event_ref".into(),
        json!(format!(
            "{}/gateway-event/{sequence}",
            binding.actuation_stream_ref
        )),
    );
    event.insert("sequence".into(), json!(sequence));
    event.insert("kind".into(), json!(kind));
    if let Some(custom_kind) = custom_kind {
        event.insert("custom_kind".into(), json!(custom_kind));
    }
    event.insert(
        "native_trace_ref".into(),
        json!(inbound.event_ref.to_string()),
    );
    event.insert("disclosure".into(), json!("portable"));
    event.insert("metadata".into(), Value::Object(metadata));
    if let Some(surface_ref) = &binding.surface_ref {
        event.insert("surface_ref".into(), json!(surface_ref.to_string()));
    }
    if let Some(text) = &inbound.text {
        event.insert("content".into(), json!(text));
    }
    if !inbound.media.is_empty() {
        event.insert(
            "resource_refs".into(),
            json!(inbound
                .media
                .iter()
                .map(|media| media.media_ref.to_string())
                .collect::<Vec<_>>()),
        );
    }
    if let Some(observed_at) = &inbound.observed_at {
        event.insert("observed_at".into(), json!(observed_at));
    }
    Value::Object(event)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum GatewayCommand {
    Protocol,
    Discover,
    Status,
    Ecology,
    RegisterConnector {
        descriptor: ConnectorDescriptor,
    },
    Bind {
        binding: GatewayBinding,
    },
    Unbind {
        binding_ref: ResourceRef,
    },
    Ingest {
        event: InboundEvent,
    },
    Replay {
        stream_ref: ResourceRef,
        #[serde(default)]
        after_sequence: u64,
        limit: usize,
    },
    /// Live subscription: answered immediately with the replay payload (the
    /// same response a Replay gets); a running service then pushes each
    /// subsequently appended event of that stream as its own `stream-event`
    /// response frame on the same connection until disconnect. A client that
    /// returns re-subscribes from its last seen sequence: the replay covers
    /// the gap, so no appended event is missed and none is repeated.
    Subscribe {
        stream_ref: ResourceRef,
        #[serde(default)]
        after_sequence: u64,
        #[serde(default = "default_subscribe_limit")]
        limit: usize,
    },
    PrepareOperation {
        binding_ref: ResourceRef,
        operation: OutboundOperationKind,
    },
    RecordDelivery {
        receipt: DeliveryReceipt,
    },
    SetConnectorHealth {
        health: ConnectorHealth,
    },
    Control {
        binding_ref: ResourceRef,
        operation: GatewayActuationControlOperation,
    },
    /// A canonical conversation-control operation (status, stop, new, sessions,
    /// restart, connector pause/resume). The kernel holds no turn sources: a
    /// running gateway service routes this to its conversation engine; offline
    /// execution is refused honestly.
    Conversation {
        binding_ref: ResourceRef,
        operation: GatewayConversationOperation,
    },
    Snapshot,
    Restore {
        snapshot: GatewaySnapshot,
    },
    /// Append a sender's Communique (non-blocking contact).
    SendCommunique {
        draft: Box<CommuniqueDraft>,
    },
    /// Accept a Communique relayed by another Workcell's gateway.
    IngestCommunique {
        communique: Box<Communique>,
        relayed_by: String,
    },
    /// Undelivered Communiques addressed to one Position.
    CommuniqueInbox {
        position_ref: String,
    },
    /// Mark Communiques delivered to one occupant generation.
    AcknowledgeCommuniques {
        position_ref: String,
        generation_ref: String,
        /// The Workcell the acknowledging generation stands on, when known;
        /// an exact-instance Communique that requires a Workcell is refused
        /// without it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workcell_ref: Option<String>,
        communique_refs: Vec<String>,
        delivered_at_unix_ms: u64,
        via: String,
    },
    /// Both directions between two Positions, from the journal.
    CommuniqueConversation {
        position_ref: String,
        with_position_ref: String,
    },
    ReadCommunique {
        communique_ref: String,
    },
    /// Record the explicit crossing into Factory custody.
    EscalateCommunique {
        communique_ref: String,
        custody_ref: String,
        escalated_at_unix_ms: u64,
        basis: String,
    },
    CommuniqueCounts,
    CommuniqueForwardQueue,
    RecordCommuniqueForward {
        communique_ref: String,
        outcome: CommuniqueForwardOutcome,
        /// The remote occupancy answer this relay followed, when it did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<CommuniqueRouting>,
    },
    /// Record a changed standing of an exact-instance Communique (pending, or
    /// held with its reason), read by the caller from the owners.
    RecordCommuniqueStanding {
        communique_ref: String,
        state: CommuniqueState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance_hold: Option<CommuniqueInstanceHold>,
        at_unix_ms: u64,
        basis: String,
    },
    /// Who occupies this Position on the serving gateway's Workcell. Answered
    /// by the service from its own Workcell's Actuation; the kernel holds no
    /// occupancy and refuses it.
    OccupancyRead {
        position_ref: String,
    },
    /// Every Position's occupancy on the serving gateway's Workcell, the same
    /// way.
    OccupancyList,
    /// One Flow-conversation request relayed from another Workcell's gateway
    /// to this Workcell's encounter owner (`action` is one of
    /// [`ENCOUNTER_RELAY_ACTIONS`]; `request` is the owner request without its
    /// action). The owner does its own admission exactly as for a local send;
    /// this carries the request and the owner's answer, nothing more.
    EncounterRelay {
        action: String,
        request: Value,
    },
    /// Stop admitting conversation work, resolve what is in flight under a
    /// bounded grace, record exactly what could not finish, persist, and
    /// (with `exit`) stop the service after answering so its supervisor
    /// starts the next build. The caller names the process it means: a drain
    /// aimed at one gateway never lands on another. Owner scope only.
    Drain {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_pid: Option<u32>,
        reason: String,
        #[serde(default)]
        exit: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        grace_ms: Option<u64>,
    },
    Shutdown,
}

impl GatewayCommand {
    pub fn is_shutdown(&self) -> bool {
        matches!(self, Self::Shutdown)
    }

    /// Reads that change no semantic state. An offline execution against the
    /// state file does not rewrite the file for these.
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::Protocol
                | Self::Discover
                | Self::Status
                | Self::Ecology
                | Self::Replay { .. }
                | Self::Subscribe { .. }
                | Self::Control { .. }
                | Self::Snapshot
                | Self::CommuniqueInbox { .. }
                | Self::CommuniqueConversation { .. }
                | Self::ReadCommunique { .. }
                | Self::CommuniqueCounts
                | Self::CommuniqueForwardQueue
                | Self::OccupancyRead { .. }
                | Self::OccupancyList
        )
    }

    /// The wire name of this command (`"shutdown"`, `"send-communique"`).
    pub fn wire_name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(|t| t.as_str().map(str::to_owned))
            })
            .unwrap_or_else(|| "unknown".into())
    }

    /// Commands a peer carrier may issue: what another gateway or an operator
    /// reading through `--at` needs — protocol and status reads, contact,
    /// relay and occupancy. Everything else — binding conversations,
    /// registering connectors, restoring or snapshotting state, draining,
    /// stopping — is the owner's, and a command added later is owner-only
    /// until it is named here.
    pub fn peer_permitted(&self) -> bool {
        matches!(
            self,
            Self::Protocol
                | Self::Status
                | Self::Discover
                | Self::Ecology
                | Self::SendCommunique { .. }
                | Self::IngestCommunique { .. }
                | Self::CommuniqueInbox { .. }
                | Self::AcknowledgeCommuniques { .. }
                | Self::CommuniqueConversation { .. }
                | Self::ReadCommunique { .. }
                | Self::EscalateCommunique { .. }
                | Self::CommuniqueCounts
                | Self::CommuniqueForwardQueue
                | Self::RecordCommuniqueForward { .. }
                | Self::RecordCommuniqueStanding { .. }
                | Self::OccupancyRead { .. }
                | Self::OccupancyList
        ) || matches!(
            self,
            Self::EncounterRelay { action, .. } if ENCOUNTER_RELAY_ACTIONS.contains(&action.as_str())
        )
    }

    /// The Position an occupancy query asks about: `Some(Some(P))` for one
    /// Position, `Some(None)` for the listing, `None` for any other command.
    pub fn occupancy_query(&self) -> Option<Option<&str>> {
        match self {
            Self::OccupancyRead { position_ref } => Some(Some(position_ref.as_str())),
            Self::OccupancyList => Some(None),
            _ => None,
        }
    }

    /// The conversation-control request this command carries, if any.
    pub fn conversation_request(&self) -> Option<(&ResourceRef, &GatewayConversationOperation)> {
        match self {
            Self::Conversation {
                binding_ref,
                operation,
            } => Some((binding_ref, operation)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum GatewayResponse {
    Protocol {
        gateway_version: String,
        connector_sdk_version: String,
        connector_wire_version: String,
        actuation_stream_schema: String,
        /// Protocol features this gateway supports. Absent on gateways that
        /// predate feature advertisement: they support none of them.
        #[serde(default)]
        features: Vec<String>,
        /// The running process. Absent on a gateway that predates
        /// `gateway-build-identity`, which is itself a fact a caller can use.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build: Option<GatewayBuildIdentity>,
    },
    Discovery {
        discovery: GatewayDiscovery,
    },
    Status {
        status: GatewayStatus,
    },
    Ecology {
        ecology: GatewayEcology,
    },
    Registered {
        connector_ref: ResourceRef,
    },
    Bound {
        binding_ref: ResourceRef,
    },
    Unbound {
        binding_ref: ResourceRef,
    },
    Ingress {
        result: GatewayIngressResult,
    },
    Replay {
        replay: GatewayReplay,
    },
    /// A live push: one event appended to a subscribed Stream. The service
    /// emits this response without a request; clients never send it.
    StreamEvent {
        stream_ref: ResourceRef,
        event: GatewayStreamEvent,
    },
    OperationPrepared {
        operation: OutboundOperation,
    },
    DeliveryRecorded {
        operation_ref: ResourceRef,
    },
    ConnectorHealthRecorded {
        connector_ref: ResourceRef,
    },
    ControlIntent {
        intent: GatewayActuationControlIntent,
    },
    /// A conversation-control operation's answer. The result document is the
    /// engine's own reading (status detail, session list, receipts); the
    /// heterogeneous shapes share one response kind.
    Conversation {
        binding_ref: ResourceRef,
        operation: GatewayConversationOperation,
        result: Value,
    },
    Snapshot {
        snapshot: GatewaySnapshot,
    },
    Restored {
        status: GatewayStatus,
    },
    CommuniqueAccepted {
        communique: Communique,
        replayed: bool,
        /// The gateway whose journal now holds the record.
        accepted_by: String,
    },
    CommuniqueList {
        communiques: Vec<Communique>,
    },
    CommuniqueRecord {
        communique: Communique,
    },
    CommuniqueCounts {
        counts: Vec<CommuniqueCount>,
    },
    Occupancy {
        reading: GatewayOccupancyReading,
    },
    /// The encounter owner's own answer (`{ok, data | error}`), verbatim.
    EncounterRelayed {
        response: Value,
    },
    Drained {
        report: DrainReport,
        /// The service stops after this answer and its supervisor starts the
        /// next process.
        exiting: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build: Option<GatewayBuildIdentity>,
    },
    Shutdown,
}

pub fn execute_gateway_command(
    gateway: &mut AgencyGateway,
    command: GatewayCommand,
) -> Result<GatewayResponse> {
    match command {
        GatewayCommand::Protocol => Ok(GatewayResponse::Protocol {
            gateway_version: AGENCY_GATEWAY_VERSION.into(),
            connector_sdk_version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
            actuation_stream_schema: ACTUATION_STREAM_SCHEMA.into(),
            features: GATEWAY_PROTOCOL_FEATURES
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            build: None,
        }),
        GatewayCommand::Discover => Ok(GatewayResponse::Discovery {
            discovery: gateway.discovery(),
        }),
        GatewayCommand::Status => Ok(GatewayResponse::Status {
            status: gateway.status(),
        }),
        GatewayCommand::Ecology => Ok(GatewayResponse::Ecology {
            ecology: gateway.ecology(),
        }),
        GatewayCommand::RegisterConnector { descriptor } => {
            let connector_ref = descriptor.connector_ref.clone();
            gateway.register_connector(descriptor)?;
            Ok(GatewayResponse::Registered { connector_ref })
        }
        GatewayCommand::Bind { binding } => {
            let binding_ref = binding.binding_ref.clone();
            gateway.bind(binding)?;
            Ok(GatewayResponse::Bound { binding_ref })
        }
        GatewayCommand::Unbind { binding_ref } => {
            gateway.unbind(&binding_ref)?;
            Ok(GatewayResponse::Unbound { binding_ref })
        }
        GatewayCommand::Ingest { event } => Ok(GatewayResponse::Ingress {
            result: gateway.ingest(event)?,
        }),
        GatewayCommand::Replay {
            stream_ref,
            after_sequence,
            limit,
        } => Ok(GatewayResponse::Replay {
            replay: gateway.replay(&stream_ref, after_sequence, limit)?,
        }),
        // A subscribe's kernel answer is the replay payload; the running
        // service attaches the live push while answering (see
        // gateway_service), so the replay and the registration are atomic.
        GatewayCommand::Subscribe {
            stream_ref,
            after_sequence,
            limit,
        } => Ok(GatewayResponse::Replay {
            replay: gateway.replay(&stream_ref, after_sequence, limit)?,
        }),
        GatewayCommand::PrepareOperation {
            binding_ref,
            operation,
        } => Ok(GatewayResponse::OperationPrepared {
            operation: gateway.prepare_operation(&binding_ref, operation)?,
        }),
        GatewayCommand::RecordDelivery { receipt } => {
            let operation_ref = receipt.operation_ref.clone();
            gateway.record_delivery(receipt)?;
            Ok(GatewayResponse::DeliveryRecorded { operation_ref })
        }
        GatewayCommand::SetConnectorHealth { health } => {
            let connector_ref = health.connector_ref.clone();
            gateway.set_connector_health(health)?;
            Ok(GatewayResponse::ConnectorHealthRecorded { connector_ref })
        }
        GatewayCommand::Control {
            binding_ref,
            operation,
        } => Ok(GatewayResponse::ControlIntent {
            intent: gateway.control_intent(&binding_ref, operation)?,
        }),
        GatewayCommand::Conversation {
            binding_ref,
            operation,
        } => Err(AikitError::new(
            "agency_gateway.engine_absent",
            format!(
                "conversation operation {operation:?} for binding {binding_ref} is executed by a \
                 running gateway service's conversation engine; this kernel holds no turn sources"
            ),
        )),
        GatewayCommand::Snapshot => Ok(GatewayResponse::Snapshot {
            snapshot: gateway.snapshot(),
        }),
        GatewayCommand::Restore { snapshot } => {
            *gateway = AgencyGateway::from_snapshot(snapshot)?;
            Ok(GatewayResponse::Restored {
                status: gateway.status(),
            })
        }
        GatewayCommand::SendCommunique { draft } => {
            let gateway_ref = gateway.gateway_ref.to_string();
            let (communique, replayed) = gateway.communiques.send(&gateway_ref, *draft)?;
            Ok(GatewayResponse::CommuniqueAccepted {
                communique,
                replayed,
                accepted_by: gateway_ref,
            })
        }
        GatewayCommand::IngestCommunique {
            communique,
            relayed_by,
        } => {
            let at = communique_now_unix_ms();
            let (communique, replayed) = gateway.communiques.ingest(*communique, &relayed_by, at)?;
            Ok(GatewayResponse::CommuniqueAccepted {
                communique,
                replayed,
                accepted_by: gateway.gateway_ref.to_string(),
            })
        }
        GatewayCommand::CommuniqueInbox { position_ref } => Ok(GatewayResponse::CommuniqueList {
            communiques: gateway.communiques.inbox(&position_ref),
        }),
        GatewayCommand::AcknowledgeCommuniques {
            position_ref,
            generation_ref,
            workcell_ref,
            communique_refs,
            delivered_at_unix_ms,
            via,
        } => Ok(GatewayResponse::CommuniqueList {
            communiques: gateway.communiques.acknowledge(
                &position_ref,
                &generation_ref,
                workcell_ref.as_deref(),
                &communique_refs,
                delivered_at_unix_ms,
                &via,
            )?,
        }),
        GatewayCommand::CommuniqueConversation {
            position_ref,
            with_position_ref,
        } => Ok(GatewayResponse::CommuniqueList {
            communiques: gateway
                .communiques
                .conversation(&position_ref, &with_position_ref),
        }),
        GatewayCommand::ReadCommunique { communique_ref } => {
            Ok(GatewayResponse::CommuniqueRecord {
                communique: gateway.communiques.get(&communique_ref)?.clone(),
            })
        }
        GatewayCommand::EscalateCommunique {
            communique_ref,
            custody_ref,
            escalated_at_unix_ms,
            basis,
        } => {
            let (communique, replayed) = gateway.communiques.escalate(
                &communique_ref,
                &custody_ref,
                escalated_at_unix_ms,
                &basis,
            )?;
            Ok(GatewayResponse::CommuniqueAccepted {
                communique,
                replayed,
                accepted_by: gateway.gateway_ref.to_string(),
            })
        }
        GatewayCommand::CommuniqueCounts => Ok(GatewayResponse::CommuniqueCounts {
            counts: gateway.communiques.counts(),
        }),
        GatewayCommand::CommuniqueForwardQueue => Ok(GatewayResponse::CommuniqueList {
            communiques: gateway.communiques.forward_queue(),
        }),
        GatewayCommand::RecordCommuniqueStanding {
            communique_ref,
            state,
            instance_hold,
            at_unix_ms,
            basis,
        } => Ok(GatewayResponse::CommuniqueRecord {
            communique: gateway
                .communiques
                .restand(&communique_ref, state, instance_hold, at_unix_ms, &basis)?
                .0,
        }),
        GatewayCommand::RecordCommuniqueForward {
            communique_ref,
            outcome,
            routing,
        } => Ok(GatewayResponse::CommuniqueRecord {
            communique: gateway
                .communiques
                .record_forward(&communique_ref, outcome, routing)?,
        }),
        GatewayCommand::OccupancyRead { .. } | GatewayCommand::OccupancyList => {
            Err(AikitError::new(
                "agency_gateway.occupancy_not_served",
                "this gateway holds no occupancy; only a running gateway service answers occupancy \
                 queries, from its own Workcell's Actuation",
            ))
        }
        GatewayCommand::EncounterRelay { .. } => Err(AikitError::new(
            "agency_gateway.encounter_relay_not_served",
            "only a running gateway service relays to its Workcell's encounter owner",
        )),
        GatewayCommand::Drain { .. } => Err(AikitError::new(
            "agency_gateway.drain_not_served",
            "a drain is the running service's: it resolves in-flight turns the kernel does not hold",
        )),
        GatewayCommand::Shutdown => Ok(GatewayResponse::Shutdown),
    }
}

/// The relaying gateway stamps receipt time itself: a relayed record's own
/// timestamps are the sender's, never the receiver's clock.
fn communique_now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayRequestEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub command: GatewayCommand,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayResponseEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<GatewayResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GatewayErrorEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayErrorEnvelope {
    pub code: String,
    pub message: String,
}

impl GatewayResponseEnvelope {
    pub fn from_result(request_id: Option<String>, result: Result<GatewayResponse>) -> Self {
        match result {
            Ok(response) => Self {
                request_id,
                ok: true,
                response: Some(response),
                error: None,
            },
            Err(error) => Self {
                request_id,
                ok: false,
                response: None,
                error: Some(GatewayErrorEnvelope {
                    code: error.code().into(),
                    message: error.to_string(),
                }),
            },
        }
    }
}

/// Source-level proof that first-party connectors can be compiled into a runtime
/// without changing the public connector contract. The kernel itself stores only
/// descriptors; connector polling/execution belongs to a carrier/host loop.
pub fn connector_descriptor<C: GatewayConnector + ?Sized>(connector: &C) -> ConnectorDescriptor {
    connector.descriptor()
}

/// Utility for connector/platform tests that need a media-free text operation.
pub fn text_send(text: impl Into<String>) -> OutboundOperationKind {
    OutboundOperationKind::Send {
        text: Some(text.into()),
        media: Vec::<MediaReference>::new(),
        reply_to_native_message_id: None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_drain_report_from_a_gateway_that_predates_the_measured_flag_still_reads_as_measured() {
        // An older gateway that has the drain sends a report with its times and no
        // `measured` field. It ran; its counts are real.
        let from_an_older_gateway: DrainReport = serde_json::from_value(serde_json::json!({
            "reason": "upgrade upg-x",
            "started_at_unix_ms": 1_000,
            "finished_at_unix_ms": 1_250,
            "grace_ms": 60_000,
            "turns_resolved": [],
            "turns_interrupted": [],
            "pending_operations": [],
            "communiques": []
        }))
        .unwrap();
        assert!(
            !from_an_older_gateway.measured,
            "the old report carries no flag"
        );
        assert!(
            from_an_older_gateway.was_measured(),
            "but it carries the times of a drain that ran"
        );
        // The report of a predecessor that has no drain at all is the default one.
        assert!(!DrainReport::default().was_measured());
        assert!(DrainReport {
            measured: true,
            ..DrainReport::default()
        }
        .was_measured());
    }

    use super::*;
    use aikit_adapters::{
        ConnectorCapabilities, ConnectorConnectionState, ConnectorOperation, DeliveryState,
        SenderKind,
    };

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn connector(platform: &str) -> ConnectorDescriptor {
        ConnectorDescriptor {
            version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: r(&format!("gateway-connector/{platform}/fixture")),
            platform: platform.into(),
            implementation: "gateway-kernel-test".into(),
            capabilities: ConnectorCapabilities {
                operations: BTreeSet::from([
                    ConnectorOperation::Send,
                    ConnectorOperation::Edit,
                    ConnectorOperation::React,
                    ConnectorOperation::Typing,
                    ConnectorOperation::Media,
                    ConnectorOperation::Threads,
                ]),
                max_text_bytes: Some(16_384),
                max_media_bytes: Some(10_000_000),
                media_types: BTreeSet::from(["image/*".into()]),
                provenance: vec!["fixture".into()],
            },
            configuration_ref: Some(r(&format!("gateway-config/{platform}/fixture"))),
            provenance: vec!["fixture".into()],
        }
    }

    fn address(platform: &str, conversation: &str) -> ConversationAddress {
        ConversationAddress {
            platform: platform.into(),
            scope_id: None,
            conversation_id: conversation.into(),
            thread_id: None,
        }
    }

    fn binding(platform: &str, conversation: &str, suffix: &str) -> GatewayBinding {
        GatewayBinding {
            binding_ref: r(&format!("gateway-binding/{suffix}")),
            connector_ref: r(&format!("gateway-connector/{platform}/fixture")),
            address: address(platform, conversation),
            agent_session_ref: r("agent-session/root"),
            agency_ref: r("agency/root"),
            actuation_ref: r("actuation/root"),
            actuation_stream_ref: r("actuation-stream/root"),
            agent_ref: Some(r("agent/root")),
            harness_ref: Some(r("harness/codex")),
            surface_ref: Some(r(&format!("surface/{platform}"))),
            forked_from: None,
            context_revision: 1,
            ingress: GatewayIngressPolicy {
                default: GatewayIngressDecision::Allow,
                sender_overrides: BTreeMap::new(),
            },
            provenance: vec!["fixture".into()],
        }
    }

    fn inbound(platform: &str, conversation: &str, id: &str, sender: &str) -> InboundEvent {
        InboundEvent {
            event_ref: r(&format!("gateway-ingress/{id}")),
            connector_ref: r(&format!("gateway-connector/{platform}/fixture")),
            address: address(platform, conversation),
            sender: SenderIdentity {
                native_sender_id: sender.into(),
                kind: SenderKind::Human,
                display_name: None,
                metadata: BTreeMap::new(),
            },
            kind: InboundEventKind::Message,
            custom_kind: None,
            native_event_id: Some(id.into()),
            native_message_id: Some(format!("message-{id}")),
            reply_to_native_message_id: None,
            text: Some(format!("hello {id}")),
            media: Vec::new(),
            observed_at: Some("2026-08-31T11:00:00Z".into()),
            native: BTreeMap::new(),
            provenance: vec!["provider fixture".into()],
        }
    }

    fn gateway() -> AgencyGateway {
        AgencyGateway::new(r("agency-gateway/local"))
    }

    #[test]
    fn one_conversation_routes_into_canonical_actuation_stream() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        let result = gateway
            .ingest(inbound("telegram", "chat-42", "1", "user-7"))
            .unwrap();
        let GatewayIngressResult::Appended {
            stream_ref, event, ..
        } = result
        else {
            panic!("allowed ingress should append");
        };
        assert_eq!(stream_ref, r("actuation-stream/root"));
        assert_eq!(event.sequence, 1);
        assert_eq!(event.event["kind"], "human-message");
        assert_eq!(event.event["surface_ref"], "surface/telegram");
        assert_eq!(event.event["content"], "hello 1");
        assert_eq!(event.event["metadata"]["native_sender_id"], "user-7");
    }

    #[test]
    fn multiple_surfaces_share_one_stream_without_multiplying_agent_identity() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway.register_connector(connector("slack")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        gateway
            .bind(binding("slack", "channel-7", "slack"))
            .unwrap();
        gateway
            .ingest(inbound("telegram", "chat-42", "1", "user-7"))
            .unwrap();
        gateway
            .ingest(inbound("slack", "channel-7", "2", "user-7"))
            .unwrap();
        let replay = gateway.replay(&r("actuation-stream/root"), 0, 10).unwrap();
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].sequence, 1);
        assert_eq!(replay.events[1].sequence, 2);
        assert_eq!(gateway.status().stream_count, 1);
        assert_eq!(gateway.status().binding_count, 2);
    }

    #[test]
    fn pair_or_deny_policy_does_not_append_stream_material() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        let mut pair = binding("telegram", "chat-42", "telegram");
        pair.ingress.default = GatewayIngressDecision::Pair;
        pair.ingress
            .sender_overrides
            .insert("blocked".into(), GatewayIngressDecision::Deny);
        gateway.bind(pair).unwrap();

        assert!(matches!(
            gateway
                .ingest(inbound("telegram", "chat-42", "1", "unknown"))
                .unwrap(),
            GatewayIngressResult::PairingRequired { .. }
        ));
        assert!(matches!(
            gateway
                .ingest(inbound("telegram", "chat-42", "2", "blocked"))
                .unwrap(),
            GatewayIngressResult::Denied { .. }
        ));
        assert_eq!(gateway.status().stream_count, 0);
    }

    #[test]
    fn replay_is_cursor_bounded_and_deterministic() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        for id in 1..=4 {
            gateway
                .ingest(inbound("telegram", "chat-42", &id.to_string(), "user-7"))
                .unwrap();
        }
        let replay = gateway.replay(&r("actuation-stream/root"), 1, 2).unwrap();
        assert_eq!(
            replay
                .events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(replay.returned_through, 3);
        assert_eq!(replay.stream_last_sequence, 4);
        assert!(replay.has_more);
    }

    #[test]
    fn outbound_operation_keeps_session_and_stream_attribution() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        let operation = gateway
            .prepare_operation(&r("gateway-binding/telegram"), text_send("done"))
            .unwrap();
        assert_eq!(operation.agent_session_ref, Some(r("agent-session/root")));
        assert_eq!(
            operation.actuation_stream_ref,
            Some(r("actuation-stream/root"))
        );
        assert_eq!(gateway.status().pending_delivery_count, 1);

        gateway
            .record_delivery(DeliveryReceipt {
                operation_ref: operation.operation_ref,
                connector_ref: r("gateway-connector/telegram/fixture"),
                state: DeliveryState::Delivered,
                native_message_id: Some("message-99".into()),
                detail: None,
                native: BTreeMap::new(),
                provenance: vec!["Telegram fixture".into()],
            })
            .unwrap();
        assert_eq!(gateway.status().pending_delivery_count, 0);
        assert_eq!(gateway.status().delivery_receipt_count, 1);
    }

    #[test]
    fn semantic_snapshot_survives_material_restart_without_identity_drift() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        gateway
            .ingest(inbound("telegram", "chat-42", "1", "user-7"))
            .unwrap();
        gateway
            .set_connector_health(ConnectorHealth {
                connector_ref: r("gateway-connector/telegram/fixture"),
                state: ConnectorConnectionState::Connected,
                detail: Some("fixture healthy".into()),
                provenance: vec!["fixture".into()],
            })
            .unwrap();

        let snapshot = gateway.snapshot();
        let restored = AgencyGateway::from_snapshot(snapshot).unwrap();
        assert_eq!(restored.gateway_ref(), &r("agency-gateway/local"));
        assert_eq!(restored.status().connector_count, 1);
        assert_eq!(restored.status().binding_count, 1);
        let replay = restored.replay(&r("actuation-stream/root"), 0, 10).unwrap();
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].sequence, 1);
    }

    #[test]
    fn snapshot_contains_no_workcell_or_process_identity_requirement() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        let encoded = serde_json::to_value(gateway.snapshot()).unwrap();
        let text = serde_json::to_string(&encoded).unwrap();
        assert!(!text.contains("pid"));
        assert!(!text.contains("socket"));
        assert!(!text.contains("workcell_ref"));
        assert!(text.contains("agent-session/root"));
        assert!(text.contains("actuation-stream/root"));
    }

    #[test]
    fn control_intent_preserves_actuation_identity_and_does_not_fake_execution() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        let intent = gateway
            .control_intent(
                &r("gateway-binding/telegram"),
                GatewayActuationControlOperation::Interrupt,
            )
            .unwrap();
        assert_eq!(intent.actuation_ref, r("actuation/root"));
        assert_eq!(intent.agent_session_ref, r("agent-session/root"));
        assert_eq!(intent.actuation_stream_ref, r("actuation-stream/root"));
    }

    #[test]
    fn duplicate_native_route_cannot_silently_target_two_sessions() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway.bind(binding("telegram", "chat-42", "one")).unwrap();
        let mut second = binding("telegram", "chat-42", "two");
        second.agent_session_ref = r("agent-session/other");
        second.actuation_ref = r("actuation/other");
        second.actuation_stream_ref = r("actuation-stream/other");
        assert_eq!(
            gateway.bind(second).unwrap_err().code(),
            "agency_gateway.route_already_bound"
        );
    }

    #[test]
    fn stdio_command_shapes_round_trip_as_portable_json() {
        let request = GatewayRequestEnvelope {
            request_id: Some("req-1".into()),
            command: GatewayCommand::Protocol,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: GatewayRequestEnvelope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, request);
        assert!(!encoded.contains("websocket"));
        assert!(!encoded.contains("unix"));
    }

    fn forked_binding(
        platform: &str,
        conversation: &str,
        suffix: &str,
        origin: GatewayForkOrigin,
        context_revision: u64,
    ) -> GatewayBinding {
        let mut fork = binding(platform, conversation, suffix);
        fork.agent_session_ref = r("agent-session/fork");
        fork.actuation_ref = r("actuation/fork");
        fork.actuation_stream_ref = r("actuation-stream/fork");
        fork.forked_from = Some(origin);
        fork.context_revision = context_revision;
        fork
    }

    fn journal_with_events(gateway: &mut AgencyGateway, count: usize) {
        for index in 0..count {
            gateway
                .ingest(inbound(
                    "telegram",
                    "chat-42",
                    &format!("fork-{index}"),
                    "user-7",
                ))
                .unwrap();
        }
    }

    #[test]
    fn fork_lineage_binds_only_against_a_real_journal_point() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        journal_with_events(&mut gateway, 3);

        let valid = forked_binding(
            "slack",
            "channel-9",
            "fork-good",
            GatewayForkOrigin {
                stream_ref: r("actuation-stream/root"),
                at_sequence: 2,
            },
            2,
        );
        gateway.register_connector(connector("slack")).unwrap();
        gateway.bind(valid).unwrap();

        let unknown_origin = forked_binding(
            "slack",
            "channel-10",
            "fork-unknown",
            GatewayForkOrigin {
                stream_ref: r("actuation-stream/absent"),
                at_sequence: 1,
            },
            1,
        );
        assert_eq!(
            gateway.bind(unknown_origin).unwrap_err().code(),
            "agency_gateway.unknown_fork_origin"
        );

        let beyond_journal = forked_binding(
            "slack",
            "channel-11",
            "fork-beyond",
            GatewayForkOrigin {
                stream_ref: r("actuation-stream/root"),
                at_sequence: 4,
            },
            1,
        );
        assert_eq!(
            gateway.bind(beyond_journal).unwrap_err().code(),
            "agency_gateway.fork_origin_sequence"
        );

        let self_fork = forked_binding(
            "slack",
            "channel-12",
            "fork-self",
            GatewayForkOrigin {
                stream_ref: r("actuation-stream/fork"),
                at_sequence: 1,
            },
            1,
        );
        assert_eq!(
            gateway.bind(self_fork).unwrap_err().code(),
            "agency_gateway.fork_origin_self"
        );

        let zero_revision = forked_binding(
            "slack",
            "channel-13",
            "fork-zero",
            GatewayForkOrigin {
                stream_ref: r("actuation-stream/root"),
                at_sequence: 1,
            },
            0,
        );
        assert_eq!(
            gateway.bind(zero_revision).unwrap_err().code(),
            "agency_gateway.invalid_context_revision"
        );

        // The bound fork round-trips through a snapshot with its lineage.
        let snapshot = gateway.snapshot();
        let restored = AgencyGateway::from_snapshot(snapshot).unwrap();
        let ecology = restored.ecology();
        let fork_binding = ecology
            .agencies
            .iter()
            .flat_map(|agency| agency.sessions.iter())
            .flat_map(|session| session.surfaces.iter())
            .find(|surface| surface.binding_ref == r("gateway-binding/fork-good"))
            .unwrap();
        assert_eq!(fork_binding.forked_from.as_ref().unwrap().at_sequence, 2);
        assert_eq!(fork_binding.context_revision, 2);
    }

    #[test]
    fn ecology_groups_agencies_sessions_and_discloses_authority_law() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway.register_connector(connector("slack")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        gateway
            .bind(binding("slack", "channel-7", "slack"))
            .unwrap();
        journal_with_events(&mut gateway, 2);
        let mut other_session = binding("telegram", "chat-99", "other");
        other_session.agent_session_ref = r("agent-session/second");
        other_session.actuation_ref = r("actuation/second");
        other_session.actuation_stream_ref = r("actuation-stream/second");
        gateway.bind(other_session).unwrap();

        let ecology = gateway.ecology();
        assert_eq!(ecology.version, AGENCY_GATEWAY_VERSION);
        assert_eq!(ecology.authority, "presence-does-not-imply-authority");
        assert_eq!(ecology.agencies.len(), 1);
        let agency = &ecology.agencies[0];
        assert_eq!(agency.agency_ref, r("agency/root"));
        assert_eq!(agency.sessions.len(), 2);

        let root = agency
            .sessions
            .iter()
            .find(|session| session.agent_session_ref == r("agent-session/root"))
            .unwrap();
        assert_eq!(root.surfaces.len(), 2);
        assert_eq!(
            root.streams,
            vec![GatewayEcologyStream {
                stream_ref: r("actuation-stream/root"),
                last_sequence: 2,
                event_count: 2,
            }]
        );
        assert_eq!(root.agent_ref.as_ref(), Some(&r("agent/root")));
        assert_eq!(
            root.invocation_modes,
            vec![
                GatewayInvocationMode::Communique,
                GatewayInvocationMode::SessionContribution,
                GatewayInvocationMode::Delegation,
                GatewayInvocationMode::SessionFork,
                GatewayInvocationMode::CoActuation,
            ]
        );

        let second = agency
            .sessions
            .iter()
            .find(|session| session.agent_session_ref == r("agent-session/second"))
            .unwrap();
        assert_eq!(second.surfaces.len(), 1);
        assert_eq!(second.streams[0].last_sequence, 0);
        assert_eq!(second.streams[0].event_count, 0);

        // Surface loss keeps the stream legible: unbind leaves the journal.
        gateway.unbind(&r("gateway-binding/slack")).unwrap();
        let ecology = gateway.ecology();
        let root = ecology.agencies[0]
            .sessions
            .iter()
            .find(|session| session.agent_session_ref == r("agent-session/root"))
            .unwrap();
        assert_eq!(root.surfaces.len(), 1);
        assert_eq!(root.streams[0].last_sequence, 2);
    }

    #[test]
    fn ecology_command_round_trips_through_the_portable_protocol() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway
            .bind(binding("telegram", "chat-42", "telegram"))
            .unwrap();
        let response = execute_gateway_command(&mut gateway, GatewayCommand::Ecology).unwrap();
        let GatewayResponse::Ecology { ecology } = response else {
            panic!("ecology command should answer with the ecology read model");
        };
        assert_eq!(ecology.agencies.len(), 1);
        let encoded = serde_json::to_value(GatewayRequestEnvelope {
            request_id: None,
            command: GatewayCommand::Ecology,
        })
        .unwrap();
        assert_eq!(encoded["command"]["type"], "ecology");
    }
}
