//! A2A v1 protocol contracts: the wire-native interagent protocol types,
//! validation rules and pure card/interface selection.
//!
//! AIKit owns the interagent protocol. `session_ecology` owns the
//! authority-bearing inter-session relations, and `knowledge_living_transport`
//! already names A2A a transport host class; the protocol contracts themselves
//! live here. They are ported faithfully from O-I's `shared-field/a2a.mjs`,
//! which was the first consumer's copy: O-I's desktop floor now mirrors this
//! module for its renderer, and drift between the two is a defect, not a
//! dialect.
//!
//! This module is protocol only: serde types, validation, and pure selection
//! over an already-fetched Agent Card document. No fetch, no filesystem, no
//! async. Transport bindings — bounded responses, timeouts, redirect policy —
//! live beside it in later work and own `A2A_AGENT_CARD_MAX_BYTES`,
//! `A2A_RESPONSE_MAX_BYTES` enforcement on the wire, and the fetch timeout.
//!
//! Deliberately not ported from the JS source: `resolveA2aParticipation`
//! (depends on the O-I Participant registry), the Encounter and
//! Contribution-ingress builders (depend on O-I Encounter/Contribution types;
//! the ingress wire name is kept as [`A2A_CONTRIBUTION_INGRESS_SCHEMA`]), and
//! `boundedFetch` (transport). The pre-Phase-2 admission path is kept exactly
//! as the JS source keeps it: fail-closed, via [`admit_a2a_difference`].

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{AikitError, Result};

pub const A2A_BINDING_SCHEMA: &str = "oi.a2a-binding/v1";
pub const A2A_PRESENCE_SCHEMA: &str = "oi.a2a-presence/v1";
pub const A2A_DIFFERENCE_SCHEMA: &str = "oi.a2a-difference/v1";
pub const A2A_CONTRIBUTION_INGRESS_SCHEMA: &str = "oi.a2a-contribution-ingress/v1";
pub const A2A_PROTOCOL_VERSION: &str = "1.0";
pub const A2A_PROTOCOL_BINDING: &str = "HTTP+JSON";

/// Outbound message text bound (32 KiB), enforced by [`a2a_exchange_demand`].
pub const A2A_MESSAGE_MAX_BYTES: usize = 32 * 1024;
/// Agent Card response bound (64 KiB); enforced by the transport layer.
pub const A2A_AGENT_CARD_MAX_BYTES: usize = 64 * 1024;
/// A2A exchange response bound (1 MiB); enforced by the transport layer.
pub const A2A_RESPONSE_MAX_BYTES: usize = 1024 * 1024;

const A2A_MAX_JSON_DEPTH: usize = 12;
const A2A_MAX_JSON_COLLECTION: usize = 256;

// ---------------------------------------------------------------------------
// Shared rule helpers (the JS `record` / `string` / `integer` / `timestamp` /
// `provenance` checks, one for one)
// ---------------------------------------------------------------------------

fn require_record<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| AikitError::new("a2a.invalid_contract", format!("{name} must be an object")))
}

fn require_string<'a>(obj: &'a Map<String, Value>, key: &str, name: &str) -> Result<&'a str> {
    match obj.get(key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(AikitError::new(
            "a2a.invalid_field",
            format!("{name} must be a non-empty string"),
        )
        .with("field", name)),
    }
}

fn require_integer(obj: &Map<String, Value>, key: &str, name: &str) -> Result<u64> {
    match obj.get(key).and_then(Value::as_u64) {
        Some(value) if value >= 1 => Ok(value),
        _ => Err(AikitError::new(
            "a2a.invalid_field",
            format!("{name} must be a positive integer"),
        )
        .with("field", name)),
    }
}

fn require_timestamp(value: &str, name: &str) -> Result<()> {
    require_non_empty(value, name)?;
    value.parse::<jiff::Timestamp>().map(|_| ()).map_err(|_| {
        AikitError::new(
            "a2a.invalid_field",
            format!("{name} must be an ISO timestamp"),
        )
        .with("field", name)
    })
}

fn require_non_empty(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AikitError::new(
            "a2a.invalid_field",
            format!("{name} must be a non-empty string"),
        )
        .with("field", name));
    }
    Ok(())
}

fn validate_provenance(entries: &[A2aProvenanceEntry], name: &str) -> Result<()> {
    if entries.is_empty() {
        return Err(AikitError::new(
            "a2a.invalid_provenance",
            format!("{name} must be a non-empty array"),
        )
        .with("field", name));
    }
    for (index, entry) in entries.iter().enumerate() {
        require_non_empty(&entry.kind, &format!("{name}[{index}].kind"))?;
        require_non_empty(&entry.reference, &format!("{name}[{index}].ref"))?;
        require_non_empty(
            &entry.source_system,
            &format!("{name}[{index}].source_system"),
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract enums
// ---------------------------------------------------------------------------

/// Revocable publication state of an A2A binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum A2aBindingState {
    #[default]
    Published,
    Withdrawn,
}

impl A2aBindingState {
    pub fn as_str(self) -> &'static str {
        match self {
            A2aBindingState::Published => "published",
            A2aBindingState::Withdrawn => "withdrawn",
        }
    }
}

/// Observed reachability of a bound endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum A2aAvailability {
    Online,
    Degraded,
    Offline,
    Withdrawn,
}

impl A2aAvailability {
    pub fn as_str(self) -> &'static str {
        match self {
            A2aAvailability::Online => "online",
            A2aAvailability::Degraded => "degraded",
            A2aAvailability::Offline => "offline",
            A2aAvailability::Withdrawn => "withdrawn",
        }
    }
}

/// Kind of A2A transport object returned by a `message:send` exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum A2aTransportKind {
    Task,
    Message,
}

impl A2aTransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            A2aTransportKind::Task => "task",
            A2aTransportKind::Message => "message",
        }
    }
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// One provenance entry. `kind`, `ref` and `source_system` are required
/// non-empty strings; every other property (e.g. `revision`) is carried
/// through untouched, exactly as the JS source clones the entry verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aProvenanceEntry {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub source_system: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Binding — oi.a2a-binding/v1
// ---------------------------------------------------------------------------

/// Authoring input for [`create_a2a_binding`]. Absent `binding_revision`,
/// `state`, `protocol_version` and `protocol_binding` take the JS source's
/// `??` defaults (1, published, `1.0`, `HTTP+JSON`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aBindingInput {
    pub binding_ref: String,
    #[serde(default = "default_binding_revision")]
    pub binding_revision: u64,
    pub field_ref: String,
    pub participant_ref: String,
    pub agent_ref: String,
    pub publisher_participant_ref: String,
    pub publication_decision_ref: String,
    pub source_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_ref: Option<String>,
    pub published_at: String,
    #[serde(default)]
    pub state: A2aBindingState,
    #[serde(default = "default_protocol_version")]
    pub protocol_version: String,
    #[serde(default = "default_protocol_binding")]
    pub protocol_binding: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_card_url: Option<String>,
    pub provenance: Vec<A2aProvenanceEntry>,
}

fn default_binding_revision() -> u64 {
    1
}

fn default_protocol_version() -> String {
    A2A_PROTOCOL_VERSION.to_string()
}

fn default_protocol_binding() -> String {
    A2A_PROTOCOL_BINDING.to_string()
}

/// An A2A binding is a revocable transport relation projected by an existing
/// Participant. It is never the Participant or the canonical Agent identity.
/// Publication requires an explicit attributable decision; local runtime
/// discovery alone cannot mint one. Endpoint locators are stored in the
/// normalized [`public_url`] form and exist only while the state is
/// `published`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aBinding {
    pub schema: String,
    pub binding_ref: String,
    #[serde(default = "default_binding_revision")]
    pub binding_revision: u64,
    pub field_ref: String,
    pub participant_ref: String,
    pub agent_ref: String,
    pub publisher_participant_ref: String,
    pub publication_decision_ref: String,
    pub source_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_ref: Option<String>,
    pub published_at: String,
    #[serde(default)]
    pub state: A2aBindingState,
    #[serde(default = "default_protocol_version")]
    pub protocol_version: String,
    #[serde(default = "default_protocol_binding")]
    pub protocol_binding: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_card_url: Option<String>,
    pub provenance: Vec<A2aProvenanceEntry>,
}

/// Mint an A2A binding from an explicit publication decision. Every creation
/// rule from the JS source applies in the same order: field checks,
/// Agent/Participant/binding ref distinctness, supported state, exact protocol
/// version and binding, non-empty provenance, optional projection ref, and
/// endpoint locators required while `published` and forbidden once `withdrawn`.
pub fn create_a2a_binding(input: A2aBindingInput) -> Result<A2aBinding> {
    require_non_empty(&input.binding_ref, "A2A binding.binding_ref")?;
    if input.binding_revision < 1 {
        return Err(field_error(
            "A2A binding.binding_revision must be a positive integer",
            "A2A binding.binding_revision",
        ));
    }
    require_non_empty(&input.field_ref, "A2A binding.field_ref")?;
    require_non_empty(&input.participant_ref, "A2A binding.participant_ref")?;
    require_non_empty(&input.agent_ref, "A2A binding.agent_ref")?;
    require_non_empty(
        &input.publisher_participant_ref,
        "A2A binding.publisher_participant_ref",
    )?;
    require_non_empty(
        &input.publication_decision_ref,
        "A2A binding.publication_decision_ref",
    )?;
    require_non_empty(&input.source_revision, "A2A binding.source_revision")?;
    require_timestamp(&input.published_at, "A2A binding.published_at")?;

    if input.agent_ref == input.participant_ref
        || input.binding_ref == input.participant_ref
        || input.binding_ref == input.agent_ref
    {
        return Err(AikitError::new(
            "a2a.refs_not_distinct",
            "A2A binding, Participant and Agent semantic refs must remain distinct",
        ));
    }

    if input.protocol_version != A2A_PROTOCOL_VERSION {
        return Err(AikitError::new(
            "a2a.protocol_version_unsupported",
            format!("A2A protocol_version must be {A2A_PROTOCOL_VERSION}"),
        ));
    }
    if input.protocol_binding != A2A_PROTOCOL_BINDING {
        return Err(AikitError::new(
            "a2a.protocol_binding_unsupported",
            format!("A2A protocol_binding must be {A2A_PROTOCOL_BINDING}"),
        ));
    }

    validate_provenance(&input.provenance, "A2A binding.provenance")?;
    if let Some(projection_ref) = &input.projection_ref {
        require_non_empty(projection_ref, "A2A binding.projection_ref")?;
    }

    let (endpoint_url, agent_card_url) = match input.state {
        A2aBindingState::Published => (
            Some(public_url(
                input.endpoint_url.as_deref().unwrap_or_default(),
                "A2A binding.endpoint_url",
            )?),
            Some(public_url(
                input.agent_card_url.as_deref().unwrap_or_default(),
                "A2A binding.agent_card_url",
            )?),
        ),
        A2aBindingState::Withdrawn => {
            if input.endpoint_url.is_some() || input.agent_card_url.is_some() {
                return Err(AikitError::new(
                    "a2a.withdrawn_retains_endpoints",
                    "withdrawn A2A bindings must not retain public endpoint locators",
                ));
            }
            (None, None)
        }
    };

    Ok(A2aBinding {
        schema: A2A_BINDING_SCHEMA.to_string(),
        binding_ref: input.binding_ref,
        binding_revision: input.binding_revision,
        field_ref: input.field_ref,
        participant_ref: input.participant_ref,
        agent_ref: input.agent_ref,
        publisher_participant_ref: input.publisher_participant_ref,
        publication_decision_ref: input.publication_decision_ref,
        source_revision: input.source_revision,
        projection_ref: input.projection_ref,
        published_at: input.published_at,
        state: input.state,
        protocol_version: input.protocol_version,
        protocol_binding: input.protocol_binding,
        endpoint_url,
        agent_card_url,
        provenance: input.provenance,
    })
}

/// Validate a `oi.a2a-binding/v1` contract document and return its normalized
/// typed form. The JS source re-enters `createA2aBinding`; so does this.
pub fn validate_a2a_binding(value: &Value) -> Result<A2aBinding> {
    let obj = require_record(value, "A2A binding")?;
    match obj.get("schema").and_then(Value::as_str) {
        Some(schema) if schema == A2A_BINDING_SCHEMA => {}
        other => {
            return Err(AikitError::new(
                "a2a.unsupported_schema",
                format!(
                    "Unsupported A2A binding schema: {}",
                    other.unwrap_or("absent")
                ),
            ))
        }
    }
    let input: A2aBindingInput = serde_json::from_value(value.clone()).map_err(|error| {
        AikitError::new(
            "a2a.invalid_contract",
            format!("A2A binding is not a valid contract: {error}"),
        )
    })?;
    create_a2a_binding(input)
}

// ---------------------------------------------------------------------------
// Presence — oi.a2a-presence/v1
// ---------------------------------------------------------------------------

/// Authoring input for [`create_a2a_presence`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aPresenceInput {
    pub binding_ref: String,
    pub field_ref: String,
    pub participant_ref: String,
    pub availability: A2aAvailability,
    pub sequence: u64,
    pub observed_at: String,
    pub provenance: Vec<A2aProvenanceEntry>,
}

/// A live reachability observation bound to one explicit binding and
/// Participant. Presence is an Explore fact about reachability, never an
/// identity claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aPresence {
    pub schema: String,
    pub binding_ref: String,
    pub field_ref: String,
    pub participant_ref: String,
    pub availability: A2aAvailability,
    pub sequence: u64,
    pub observed_at: String,
    pub provenance: Vec<A2aProvenanceEntry>,
}

/// Mint an A2A presence observation.
pub fn create_a2a_presence(input: A2aPresenceInput) -> Result<A2aPresence> {
    require_non_empty(&input.binding_ref, "A2A presence.binding_ref")?;
    require_non_empty(&input.field_ref, "A2A presence.field_ref")?;
    require_non_empty(&input.participant_ref, "A2A presence.participant_ref")?;
    if input.sequence < 1 {
        return Err(field_error(
            "A2A presence.sequence must be a positive integer",
            "A2A presence.sequence",
        ));
    }
    require_timestamp(&input.observed_at, "A2A presence.observed_at")?;
    validate_provenance(&input.provenance, "A2A presence.provenance")?;

    Ok(A2aPresence {
        schema: A2A_PRESENCE_SCHEMA.to_string(),
        binding_ref: input.binding_ref,
        field_ref: input.field_ref,
        participant_ref: input.participant_ref,
        availability: input.availability,
        sequence: input.sequence,
        observed_at: input.observed_at,
        provenance: input.provenance,
    })
}

/// Validate a `oi.a2a-presence/v1` contract document and return its typed form.
pub fn validate_a2a_presence(value: &Value) -> Result<A2aPresence> {
    let obj = require_record(value, "A2A presence")?;
    match obj.get("schema").and_then(Value::as_str) {
        Some(schema) if schema == A2A_PRESENCE_SCHEMA => {}
        other => {
            return Err(AikitError::new(
                "a2a.unsupported_schema",
                format!(
                    "Unsupported A2A presence schema: {}",
                    other.unwrap_or("absent")
                ),
            ))
        }
    }
    let input: A2aPresenceInput = serde_json::from_value(value.clone()).map_err(|error| {
        AikitError::new(
            "a2a.invalid_contract",
            format!("A2A presence is not a valid contract: {error}"),
        )
    })?;
    create_a2a_presence(input)
}

// ---------------------------------------------------------------------------
// Difference — oi.a2a-difference/v1
// ---------------------------------------------------------------------------

/// The explicit Exchange-authority lineage a returned difference carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct A2aExchangeAuthority {
    pub grant_ref: String,
    pub operation_id: String,
}

/// The A2A Task or Message a `message:send` exchange returned. The payload is
/// untrusted transport material: carried, never interpreted here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aTransportResult {
    pub kind: A2aTransportKind,
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

/// The untrusted returned difference of one A2A exchange. It begins `pending`
/// and carries the exact Exchange grant and operation lineage; admission,
/// indexing, projection and execution authority all stay with their own
/// receiving-side owners.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aDifference {
    pub schema: String,
    pub exchange_ref: String,
    pub field_ref: String,
    pub initiator_participant_ref: String,
    pub recipient_participant_ref: String,
    pub agent_ref: String,
    pub binding_ref: String,
    pub binding_revision: u64,
    pub request_message_id: String,
    pub exchange_authority: A2aExchangeAuthority,
    pub transport_result: A2aTransportResult,
    pub admission: String,
    /// Transport provenance is carried verbatim and is deliberately not
    /// rule-checked here, matching `validateA2aDifference`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_provenance: Option<Value>,
}

/// Validate a `oi.a2a-difference/v1` contract document, mirroring
/// `validateA2aDifference` rule for rule and in the same order.
pub fn validate_a2a_difference(value: &Value) -> Result<A2aDifference> {
    let obj = require_record(value, "A2A difference")?;
    match obj.get("schema").and_then(Value::as_str) {
        Some(schema) if schema == A2A_DIFFERENCE_SCHEMA => {}
        other => {
            return Err(AikitError::new(
                "a2a.unsupported_schema",
                format!(
                    "Unsupported A2A difference schema: {}",
                    other.unwrap_or("absent")
                ),
            ))
        }
    }
    let exchange_ref = require_string(obj, "exchange_ref", "A2A difference.exchange_ref")?;
    let field_ref = require_string(obj, "field_ref", "A2A difference.field_ref")?;
    let initiator_participant_ref = require_string(
        obj,
        "initiator_participant_ref",
        "A2A difference.initiator_participant_ref",
    )?;
    let recipient_participant_ref = require_string(
        obj,
        "recipient_participant_ref",
        "A2A difference.recipient_participant_ref",
    )?;
    let agent_ref = require_string(obj, "agent_ref", "A2A difference.agent_ref")?;
    let binding_ref = require_string(obj, "binding_ref", "A2A difference.binding_ref")?;
    let binding_revision =
        require_integer(obj, "binding_revision", "A2A difference.binding_revision")?;
    let request_message_id = require_string(
        obj,
        "request_message_id",
        "A2A difference.request_message_id",
    )?;

    let authority = require_record(
        obj.get("exchange_authority").unwrap_or(&Value::Null),
        "A2A difference.exchange_authority",
    )?;
    let grant_ref = require_string(
        authority,
        "grant_ref",
        "A2A difference.exchange_authority.grant_ref",
    )?;
    let operation_id = require_string(
        authority,
        "operation_id",
        "A2A difference.exchange_authority.operation_id",
    )?;

    let transport = require_record(
        obj.get("transport_result").unwrap_or(&Value::Null),
        "A2A difference.transport_result",
    )?;
    let kind = match transport.get("kind").and_then(Value::as_str) {
        Some("task") => A2aTransportKind::Task,
        Some("message") => A2aTransportKind::Message,
        _ => {
            return Err(AikitError::new(
                "a2a.transport_kind_invalid",
                "A2A transport result must be task or message",
            ))
        }
    };
    let reference = require_string(transport, "ref", "A2A difference.transport_result.ref")?;

    if obj.get("admission").and_then(Value::as_str) != Some("pending") {
        return Err(AikitError::new(
            "a2a.admission_not_pending",
            "received A2A difference must begin pending explicit admission",
        ));
    }

    Ok(A2aDifference {
        schema: A2A_DIFFERENCE_SCHEMA.to_string(),
        exchange_ref: exchange_ref.to_string(),
        field_ref: field_ref.to_string(),
        initiator_participant_ref: initiator_participant_ref.to_string(),
        recipient_participant_ref: recipient_participant_ref.to_string(),
        agent_ref: agent_ref.to_string(),
        binding_ref: binding_ref.to_string(),
        binding_revision,
        request_message_id: request_message_id.to_string(),
        exchange_authority: A2aExchangeAuthority {
            grant_ref: grant_ref.to_string(),
            operation_id: operation_id.to_string(),
        },
        transport_result: A2aTransportResult {
            kind,
            reference: reference.to_string(),
            payload: transport.get("payload").cloned(),
        },
        admission: "pending".to_string(),
        transport_provenance: obj.get("transport_provenance").cloned(),
    })
}

// ---------------------------------------------------------------------------
// Exchange preconditions and the pure authority demand
// ---------------------------------------------------------------------------

/// The outbound A2A message as authored by the initiator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aMessage {
    pub message_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Value>,
}

/// The demand handed to an explicit Exchange-authority resolver before any A2A
/// network I/O. A denied or missing resolution must produce zero outbound
/// requests; credentials never enter this demand or the returned difference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aExchangeDemand {
    pub field_ref: String,
    pub initiator_participant_ref: String,
    pub counterparty_participant_ref: String,
    pub protocol: String,
    pub protocol_version: String,
    pub protocol_binding: String,
    pub mode: String,
    pub binding_ref: String,
    pub binding_revision: u64,
    pub operation_id: String,
    pub purpose: String,
    pub scope_json: String,
}

/// Check the pure preconditions of an A2A HTTP+JSON v1 exchange and build the
/// Exchange-authority demand — the protocol half of the JS
/// `performA2aExchange`. Binding and presence must already be validated forms
/// ([`validate_a2a_binding`] / [`validate_a2a_presence`]); the fetch, the
/// authority resolver call and the response bounding are the transport layer's
/// half and are deliberately not here.
pub fn a2a_exchange_demand(
    binding: &A2aBinding,
    presence: &A2aPresence,
    initiator_participant_ref: &str,
    message: &A2aMessage,
) -> Result<A2aExchangeDemand> {
    require_non_empty(initiator_participant_ref, "initiator_participant_ref")?;
    require_non_empty(&message.message_id, "A2A message.message_id")?;
    require_non_empty(&message.text, "A2A message.text")?;
    if message.text.len() > A2A_MESSAGE_MAX_BYTES {
        return Err(AikitError::new(
            "a2a.message_too_large",
            format!("A2A message.text exceeds {A2A_MESSAGE_MAX_BYTES} byte limit"),
        ));
    }

    if binding.state != A2aBindingState::Published {
        return Err(AikitError::new(
            "a2a.binding_not_published",
            "A2A exchange requires an explicitly published binding",
        ));
    }
    if presence.binding_ref != binding.binding_ref
        || presence.participant_ref != binding.participant_ref
    {
        return Err(AikitError::new(
            "a2a.presence_binding_mismatch",
            "A2A presence must belong to the selected binding/Participant",
        ));
    }
    if !matches!(
        presence.availability,
        A2aAvailability::Online | A2aAvailability::Degraded
    ) {
        return Err(AikitError::new(
            "a2a.endpoint_unreachable",
            format!(
                "A2A endpoint is not currently reachable: {}",
                presence.availability.as_str()
            ),
        ));
    }

    let operation_id = match &message.exchange_operation_id {
        Some(operation_id) => operation_id.clone(),
        None => message.message_id.clone(),
    };
    require_non_empty(&operation_id, "A2A exchange operation id")?;
    let scope = message.scope.clone().unwrap_or_else(|| {
        Value::Object(Map::from_iter([(
            "kind".to_string(),
            Value::String("message".to_string()),
        )]))
    });
    let scope_json = serde_json::to_string(&scope).map_err(|error| {
        AikitError::new(
            "a2a.invalid_contract",
            format!("A2A message scope is not valid JSON: {error}"),
        )
    })?;

    Ok(A2aExchangeDemand {
        field_ref: binding.field_ref.clone(),
        initiator_participant_ref: initiator_participant_ref.to_string(),
        counterparty_participant_ref: binding.participant_ref.clone(),
        protocol: "a2a".to_string(),
        protocol_version: binding.protocol_version.clone(),
        protocol_binding: binding.protocol_binding.clone(),
        mode: "message:send".to_string(),
        binding_ref: binding.binding_ref.clone(),
        binding_revision: binding.binding_revision,
        operation_id,
        purpose: message
            .purpose
            .clone()
            .unwrap_or_else(|| "a2a-message-exchange".to_string()),
        scope_json,
    })
}

// ---------------------------------------------------------------------------
// Agent Card selection (pure, over an already-fetched card document)
// ---------------------------------------------------------------------------

/// Select the Agent Card interface that advertises exactly the explicitly
/// published binding: same protocol binding, same protocol version, and a
/// normalized endpoint URL equal to the binding's. Card metadata that claims
/// Agent or Participant identity is transport material and is ignored; a
/// candidate interface that fails its own URL check is skipped, not fatal.
/// Returns the selected interface document verbatim.
pub fn assert_a2a_card(card: &Value, binding: &A2aBinding) -> Result<Value> {
    let obj = require_record(card, "A2A Agent Card")?;
    let name = obj.get("name").and_then(Value::as_str);
    match name {
        Some(name) if !name.trim().is_empty() => {}
        _ => {
            return Err(field_error(
                "A2A Agent Card.name must be a non-empty string",
                "A2A Agent Card.name",
            ))
        }
    }
    let interfaces = obj
        .get("supportedInterfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            field_error(
                "A2A Agent Card.supportedInterfaces must be an array",
                "A2A Agent Card.supportedInterfaces",
            )
        })?;
    let endpoint_url = binding.endpoint_url.as_deref().unwrap_or_default();
    let expected_endpoint = public_url(endpoint_url, "A2A binding.endpoint_url")?;

    for candidate in interfaces {
        let Some(candidate_obj) = candidate.as_object() else {
            continue;
        };
        if candidate_obj.get("protocolBinding").and_then(Value::as_str)
            != Some(&binding.protocol_binding)
        {
            continue;
        }
        if candidate_obj.get("protocolVersion").and_then(Value::as_str)
            != Some(&binding.protocol_version)
        {
            continue;
        }
        let Some(candidate_url) = candidate_obj.get("url").and_then(Value::as_str) else {
            continue;
        };
        if let Ok(selected_endpoint) = public_url(candidate_url, "A2A AgentInterface.url") {
            if selected_endpoint == expected_endpoint {
                return Ok(candidate.clone());
            }
        }
    }

    Err(AikitError::new(
        "a2a.card_interface_not_advertised",
        "A2A Agent Card does not advertise the explicitly published interface",
    ))
}

/// Build the `message:send` URL for a selected Agent interface, optionally
/// tenant-prefixed with percent-encoding (`encodeURIComponent` semantics).
pub fn a2a_send_url(interface_url: &str, tenant: Option<&str>) -> Result<String> {
    let base = public_url(interface_url, "A2A AgentInterface.url")?;
    Ok(match tenant {
        Some(tenant) => format!("{base}/{}/message:send", encode_uri_component(tenant)),
        None => format!("{base}/message:send"),
    })
}

/// A resolved A2A transport reference from a `SendMessageResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct A2aTransportRef {
    pub kind: A2aTransportKind,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// Read the returned transport reference from a parsed `SendMessageResponse`:
/// a `task` carries `id`, a `message` carries `messageId`, anything else is
/// refused.
pub fn transport_ref(response: &Value) -> Result<A2aTransportRef> {
    if let Some(task) = response.get("task") {
        let id = task
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| field_error("A2A Task.id must be a non-empty string", "A2A Task.id"))?;
        return Ok(A2aTransportRef {
            kind: A2aTransportKind::Task,
            reference: id.to_string(),
        });
    }
    if let Some(message) = response.get("message") {
        let message_id = message
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                field_error(
                    "A2A Message.messageId must be a non-empty string",
                    "A2A Message.messageId",
                )
            })?;
        return Ok(A2aTransportRef {
            kind: A2aTransportKind::Message,
            reference: message_id.to_string(),
        });
    }
    Err(AikitError::new(
        "a2a.response_missing_transport_ref",
        "A2A SendMessageResponse must contain task or message",
    ))
}

// ---------------------------------------------------------------------------
// Bounded JSON shape (the pure half of the JS `boundedJsonResponse`)
// ---------------------------------------------------------------------------

/// Check a parsed JSON document against the bounded-shape rules: maximum depth
/// [`A2A_MAX_JSON_DEPTH`], maximum array/object cardinality
/// [`A2A_MAX_JSON_COLLECTION`], and no string over [`A2A_RESPONSE_MAX_BYTES`]
/// UTF-8 bytes. The transport layer applies this to every A2A response body
/// before remote material becomes returned data.
pub fn bounded_json_shape(value: &Value, name: &str) -> Result<()> {
    bounded_json_shape_at(value, name, 0)
}

fn bounded_json_shape_at(value: &Value, name: &str, depth: usize) -> Result<()> {
    if depth > A2A_MAX_JSON_DEPTH {
        return Err(AikitError::new(
            "a2a.json_depth_exceeded",
            format!("{name} exceeds maximum JSON depth {A2A_MAX_JSON_DEPTH}"),
        ));
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        Value::String(text) => {
            if text.len() > A2A_RESPONSE_MAX_BYTES {
                Err(AikitError::new(
                    "a2a.json_string_oversized",
                    format!("{name} contains an oversized string"),
                ))
            } else {
                Ok(())
            }
        }
        Value::Array(entries) => {
            if entries.len() > A2A_MAX_JSON_COLLECTION {
                return Err(AikitError::new(
                    "a2a.json_collection_exceeded",
                    format!("{name} exceeds {A2A_MAX_JSON_COLLECTION} array entries"),
                ));
            }
            for (index, entry) in entries.iter().enumerate() {
                bounded_json_shape_at(entry, &format!("{name}[{index}]"), depth + 1)?;
            }
            Ok(())
        }
        Value::Object(entries) => {
            if entries.len() > A2A_MAX_JSON_COLLECTION {
                return Err(AikitError::new(
                    "a2a.json_collection_exceeded",
                    format!("{name} exceeds {A2A_MAX_JSON_COLLECTION} object properties"),
                ));
            }
            for (key, entry) in entries {
                bounded_json_shape_at(entry, &format!("{name}.{key}"), depth + 1)?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Fail-closed tombstone
// ---------------------------------------------------------------------------

/// Compatibility tombstone: the pre-Phase-2 helper could directly mint
/// semantic Contribution/Projection material. That path is intentionally
/// fail-closed; route a returned difference through the hosted generic
/// Contribution ingress instead.
pub fn admit_a2a_difference<T>() -> Result<T> {
    Err(AikitError::new(
        "a2a.admission_disabled",
        "A2A-specific Admission is disabled; route the returned difference through prepareA2aContributionIngress and hosted generic Contribution Admission",
    ))
}

// ---------------------------------------------------------------------------
// URL and encoding rules (the JS `publicUrl` / `encodeURIComponent`)
// ---------------------------------------------------------------------------

/// Validate and normalize a public endpoint locator, mirroring the JS
/// `publicUrl`: absolute http(s) URL; HTTPS, or HTTP to `127.0.0.1`,
/// `localhost` or `::1` (loopback development); no embedded credentials; no
/// query or fragment. The result is the canonical serialization with one
/// trailing slash removed.
pub fn public_url(value: &str, name: &str) -> Result<String> {
    require_non_empty(value, name)?;

    let Some((scheme, rest)) = split_scheme(value) else {
        return Err(absolute_url_error(name, value));
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(https_error(name));
    }
    // Special schemes tolerate any number of leading slashes (WHATWG URL
    // "special authority slashes" state), including none.
    let after_scheme = rest.trim_start_matches('/');
    if after_scheme.is_empty() {
        return Err(https_error(name));
    }

    // Split authority / path; a '?' or '#' anywhere is refused outright, but
    // only after the earlier checks have had their turn (JS check order).
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let path = &after_scheme[authority_end..];
    let truncated = path.contains('?') || path.contains('#');

    // Authority: optional userinfo (up to the last '@'), then host[:port].
    let (userinfo, host_port) = match authority.rfind('@') {
        Some(index) => (&authority[..index], &authority[index + 1..]),
        None => ("", authority),
    };
    let (username, password) = match userinfo.split_once(':') {
        Some((username, password)) => (username, password),
        None => (userinfo, ""),
    };

    let (hostname, port) = if let Some(rest) = host_port.strip_prefix('[') {
        // IPv6 literal: [::1] or [::1]:8443
        let Some(close) = rest.find(']') else {
            return Err(absolute_url_error(name, value));
        };
        let hostname = &rest[..close];
        let port = rest[close + 1..].strip_prefix(':').unwrap_or_default();
        (hostname, port)
    } else {
        match host_port.split_once(':') {
            Some((hostname, port)) => (hostname, port),
            None => (host_port, ""),
        }
    };

    // JS parses the URL completely before checking policy, so an invalid port
    // surfaces as an absolute-URL failure ahead of every rule below.
    let port_number: Option<u16> = if port.is_empty() {
        None
    } else {
        match port.parse::<u16>() {
            Ok(port) => Some(port),
            _ => return Err(absolute_url_error(name, value)),
        }
    };

    // JS check order: absolute -> https/loopback -> credentials -> query/fragment.
    let hostname = hostname.to_ascii_lowercase();
    let loopback = matches!(hostname.as_str(), "127.0.0.1" | "localhost" | "::1");
    if scheme != "https" && !loopback {
        return Err(https_error(name));
    }
    if !username.is_empty() || !password.is_empty() {
        return Err(AikitError::new(
            "a2a.url_credentials",
            format!("{name} must not embed credentials"),
        )
        .with("field", name));
    }
    if truncated {
        return Err(AikitError::new(
            "a2a.url_query_or_fragment",
            format!("{name} must not expose query credentials or fragments"),
        )
        .with("field", name));
    }

    // Serialize: default ports are elided (http :80, https :443), an empty
    // port is elided, the hostname keeps brackets when it is an IPv6 literal,
    // an empty path serializes as "/", and finally one trailing slash is
    // stripped — exactly the JS `url.toString().replace(/\/$/, '')`.
    let default_port = u16::from(scheme == "http") * 80 + u16::from(scheme == "https") * 443;
    let port_suffix = match port_number {
        Some(port) if port != default_port => format!(":{port}"),
        _ => String::new(),
    };
    let host = if hostname.contains(':') {
        format!("[{hostname}]")
    } else {
        hostname
    };
    let path = if path.is_empty() { "/" } else { path };
    let mut url = format!("{scheme}://{host}{port_suffix}{path}");
    if url.ends_with('/') {
        url.pop();
    }
    Ok(url)
}

/// Split `scheme:rest` with a valid URI scheme (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`).
fn split_scheme(value: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = value.split_once(':')?;
    let mut chars = scheme.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return None,
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    Some((scheme, rest))
}

/// `encodeURIComponent`: percent-encode every byte outside the JS unreserved
/// set, as uppercase hex over UTF-8.
fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => encoded.push(byte as char),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

fn field_error(message: &str, field: &str) -> AikitError {
    AikitError::new("a2a.invalid_field", message).with("field", field)
}

fn absolute_url_error(name: &str, value: &str) -> AikitError {
    AikitError::new(
        "a2a.url_not_absolute",
        format!("{name} must be an absolute URL"),
    )
    .with("field", name)
    .with("value", value)
}

fn https_error(name: &str) -> AikitError {
    AikitError::new(
        "a2a.url_not_https",
        format!("{name} must use HTTPS outside loopback development"),
    )
    .with("field", name)
}

// ---------------------------------------------------------------------------
// Tests — the validation coverage of shared-field/a2a.test.mjs, ported
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIELD: &str = "field:o-i:shared";
    const AGENT: &str = "agent:remote:canonical";
    const PARTICIPANT: &str = "participant:remote:canonical";
    const LOCAL_PARTICIPANT: &str = "participant:local:canonical";

    fn provenance_fixture() -> Vec<A2aProvenanceEntry> {
        vec![A2aProvenanceEntry {
            kind: "agent-projection".to_string(),
            reference: "projection:agent:7".to_string(),
            source_system: "O:I".to_string(),
            extra: Map::from_iter([("revision".to_string(), Value::String("7".to_string()))]),
        }]
    }

    fn binding_input(endpoint_url: &str, agent_card_url: &str) -> A2aBindingInput {
        A2aBindingInput {
            binding_ref: "a2a-binding:remote".to_string(),
            binding_revision: 1,
            field_ref: FIELD.to_string(),
            participant_ref: PARTICIPANT.to_string(),
            agent_ref: AGENT.to_string(),
            publisher_participant_ref: PARTICIPANT.to_string(),
            publication_decision_ref: "decision:publish:a2a:1".to_string(),
            source_revision: "agent@7".to_string(),
            projection_ref: None,
            published_at: "2026-08-16T21:00:00.000Z".to_string(),
            state: A2aBindingState::Published,
            protocol_version: A2A_PROTOCOL_VERSION.to_string(),
            protocol_binding: A2A_PROTOCOL_BINDING.to_string(),
            endpoint_url: Some(endpoint_url.to_string()),
            agent_card_url: Some(agent_card_url.to_string()),
            provenance: provenance_fixture(),
        }
    }

    fn published_binding(endpoint_url: &str, agent_card_url: &str) -> A2aBinding {
        create_a2a_binding(binding_input(endpoint_url, agent_card_url)).expect("binding is valid")
    }

    fn online_presence(binding: &A2aBinding) -> A2aPresence {
        create_a2a_presence(A2aPresenceInput {
            binding_ref: binding.binding_ref.clone(),
            field_ref: binding.field_ref.clone(),
            participant_ref: binding.participant_ref.clone(),
            availability: A2aAvailability::Online,
            sequence: 1,
            observed_at: "2026-08-16T21:01:00.000Z".to_string(),
            provenance: vec![A2aProvenanceEntry {
                kind: "reachability-observation".to_string(),
                reference: "probe:1".to_string(),
                source_system: "O:I".to_string(),
                extra: Map::new(),
            }],
        })
        .expect("presence is valid")
    }

    fn message_fixture() -> A2aMessage {
        A2aMessage {
            message_id: "a2a-message:request-1".to_string(),
            text: "hello".to_string(),
            exchange_operation_id: None,
            exchange_ref: None,
            purpose: None,
            scope: None,
        }
    }

    /// Concrete fixture ported from the O-I conformance corpus: the returned
    /// difference shape of the encounter test, verbatim.
    fn difference_fixture() -> Value {
        json!({
            "schema": "oi.a2a-difference/v1",
            "exchange_ref": "a2a-exchange:encounter-1",
            "field_ref": FIELD,
            "initiator_participant_ref": LOCAL_PARTICIPANT,
            "recipient_participant_ref": PARTICIPANT,
            "agent_ref": AGENT,
            "binding_ref": "a2a-binding:remote",
            "binding_revision": 4,
            "request_message_id": "a2a-message:encounter-1",
            "exchange_authority": {
                "grant_ref": "exchange-grant:encounter",
                "operation_id": "operation:encounter"
            },
            "transport_result": {
                "kind": "message",
                "ref": "a2a-message:return-encounter",
                "payload": { "message": { "messageId": "a2a-message:return-encounter" } }
            },
            "admission": "pending",
            "transport_provenance": {
                "protocol": "A2A", "protocol_version": "1.0", "protocol_binding": "HTTP+JSON"
            }
        })
    }

    /// Concrete fixture ported from the O-I conformance corpus: the
    /// deliberately adversarial Agent Card served by the test A2A server —
    /// card metadata claims canonical identity and must stay transport-only.
    fn adversarial_agent_card(endpoint_url: &str) -> Value {
        json!({
            "name": AGENT,
            "description": "Conformance fixture",
            "version": "1.0.1-fixture",
            "supportedInterfaces": [{
                "url": endpoint_url,
                "protocolBinding": "HTTP+JSON",
                "protocolVersion": "1.0",
                "agentRef": "agent:malicious-card-claim",
                "participantRef": "participant:malicious-card-claim"
            }],
            "capabilities": {},
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain"],
            "skills": []
        })
    }

    #[test]
    fn binding_round_trips_through_json_and_revalidation() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        assert_eq!(binding.schema, A2A_BINDING_SCHEMA);
        assert_eq!(binding.state, A2aBindingState::Published);
        assert_eq!(
            binding.endpoint_url.as_deref(),
            Some("https://agent.example/a2a")
        );

        let json = serde_json::to_value(&binding).expect("serializes");
        let reparsed = validate_a2a_binding(&json).expect("revalidates");
        assert_eq!(binding, reparsed);
    }

    #[test]
    fn binding_defaults_follow_the_js_source() {
        let json = json!({
            "schema": "oi.a2a-binding/v1",
            "binding_ref": "a2a-binding:remote",
            "field_ref": FIELD,
            "participant_ref": PARTICIPANT,
            "agent_ref": AGENT,
            "publisher_participant_ref": PARTICIPANT,
            "publication_decision_ref": "decision:publish:a2a:1",
            "source_revision": "agent@7",
            "published_at": "2026-08-16T21:00:00.000Z",
            "endpoint_url": "https://agent.example/a2a",
            "agent_card_url": "https://agent.example/.well-known/agent-card.json",
            "provenance": [{
                "kind": "agent-projection",
                "ref": "projection:agent:7",
                "source_system": "O:I"
            }]
        });
        let binding = validate_a2a_binding(&json).expect("defaults apply");
        assert_eq!(binding.binding_revision, 1);
        assert_eq!(binding.protocol_version, "1.0");
        assert_eq!(binding.protocol_binding, "HTTP+JSON");
        assert_eq!(binding.state, A2aBindingState::Published);
        // Unknown provenance properties survive the round trip (JS clone semantics).
        let serialized = serde_json::to_value(&binding).expect("serializes");
        assert_eq!(
            serialized["provenance"][0]["ref"],
            json!("projection:agent:7")
        );
    }

    #[test]
    fn publication_is_explicit_and_semantic_refs_cannot_collapse() {
        let mut input = binding_input(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        input.binding_ref = AGENT.to_string();
        let error = create_a2a_binding(input).expect_err("refs must stay distinct");
        assert!(error.message().contains("must remain distinct"));
    }

    #[test]
    fn endpoint_locators_reject_credentials_queries_and_plain_http() {
        let cases = [
            (
                "https://user:secret@agent.example/a2a",
                "https://agent.example/.well-known/agent-card.json",
                "must not embed credentials",
            ),
            (
                "https://agent.example/a2a?token=secret",
                "https://agent.example/.well-known/agent-card.json",
                "must not expose query credentials or fragments",
            ),
            (
                "http://agent.example/a2a",
                "https://agent.example/.well-known/agent-card.json",
                "must use HTTPS outside loopback development",
            ),
            (
                "not a url",
                "https://agent.example/.well-known/agent-card.json",
                "must be an absolute URL",
            ),
        ];
        for (endpoint_url, agent_card_url, expected) in cases {
            let error = create_a2a_binding(binding_input(endpoint_url, agent_card_url))
                .expect_err(endpoint_url);
            assert!(error.message().contains(expected), "{error}");
        }
        // Loopback development HTTP stays legal.
        let binding = published_binding(
            "http://127.0.0.1:8080/a2a",
            "http://localhost:8080/.well-known/agent-card.json",
        );
        assert_eq!(
            binding.endpoint_url.as_deref(),
            Some("http://127.0.0.1:8080/a2a")
        );
    }

    #[test]
    fn withdrawn_bindings_must_not_retain_public_endpoint_locators() {
        let mut input = binding_input(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        input.state = A2aBindingState::Withdrawn;
        input.endpoint_url = None;
        input.agent_card_url = None;
        let withdrawn = create_a2a_binding(input).expect("withdrawn without locators");
        assert_eq!(withdrawn.state, A2aBindingState::Withdrawn);
        assert_eq!(withdrawn.endpoint_url, None);
        assert_eq!(withdrawn.agent_card_url, None);

        let mut retaining = binding_input(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        retaining.state = A2aBindingState::Withdrawn;
        let error = create_a2a_binding(retaining).expect_err("retains locators");
        assert!(error.message().contains("withdrawn A2A bindings"));
    }

    #[test]
    fn binding_rejects_wrong_schema_protocol_and_empty_provenance() {
        let mut json = serde_json::to_value(published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        ))
        .expect("serializes");
        json["schema"] = json!("oi.other/v1");
        let error = validate_a2a_binding(&json).expect_err("schema");
        assert!(error.message().contains("Unsupported A2A binding schema"));

        let mut input = binding_input(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        input.provenance = Vec::new();
        let error = create_a2a_binding(input).expect_err("provenance");
        assert!(error.message().contains("non-empty array"));

        let mut wrong_protocol = binding_input(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        wrong_protocol.protocol_version = "2.0".to_string();
        let error = create_a2a_binding(wrong_protocol).expect_err("protocol version");
        assert!(error.message().contains("protocol_version must be 1.0"));
    }

    #[test]
    fn presence_round_trips_and_rejects_wrong_schema() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let presence = online_presence(&binding);
        let json = serde_json::to_value(&presence).expect("serializes");
        let reparsed = validate_a2a_presence(&json).expect("revalidates");
        assert_eq!(presence, reparsed);

        let mut wrong = json;
        wrong["schema"] = json!("oi.other/v1");
        let error = validate_a2a_presence(&wrong).expect_err("schema");
        assert!(error.message().contains("Unsupported A2A presence schema"));

        // Every availability state of the JS vocabulary is representable.
        for (raw, expected) in [
            ("online", A2aAvailability::Online),
            ("degraded", A2aAvailability::Degraded),
            ("offline", A2aAvailability::Offline),
            ("withdrawn", A2aAvailability::Withdrawn),
        ] {
            let mut document = serde_json::to_value(&presence).expect("serializes");
            document["availability"] = json!(raw);
            let parsed = validate_a2a_presence(&document).expect(raw);
            assert_eq!(parsed.availability, expected);
        }
        let mut unsupported = serde_json::to_value(&presence).expect("serializes");
        unsupported["availability"] = json!("away");
        assert!(validate_a2a_presence(&unsupported).is_err());
    }

    #[test]
    fn exchange_demand_happy_path_and_defaults() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let presence = online_presence(&binding);
        let demand =
            a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &message_fixture())
                .expect("exchange is admissible");
        assert_eq!(demand.protocol, "a2a");
        assert_eq!(demand.mode, "message:send");
        assert_eq!(demand.operation_id, "a2a-message:request-1");
        assert_eq!(demand.purpose, "a2a-message-exchange");
        assert_eq!(demand.scope_json, r#"{"kind":"message"}"#);
        assert_eq!(demand.counterparty_participant_ref, PARTICIPANT);
        assert_eq!(demand.field_ref, FIELD);
    }

    #[test]
    fn exchange_demand_overrides_follow_the_js_source() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let presence = online_presence(&binding);
        let mut message = message_fixture();
        message.exchange_operation_id = Some("operation:custom".to_string());
        message.purpose = Some("purpose:proof".to_string());
        message.scope = Some(json!({ "kind": "task" }));
        let demand = a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &message)
            .expect("exchange is admissible");
        assert_eq!(demand.operation_id, "operation:custom");
        assert_eq!(demand.purpose, "purpose:proof");
        assert_eq!(demand.scope_json, r#"{"kind":"task"}"#);
    }

    #[test]
    fn exchange_requires_an_explicitly_published_binding() {
        let mut input = binding_input(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        input.state = A2aBindingState::Withdrawn;
        input.endpoint_url = None;
        input.agent_card_url = None;
        let withdrawn = create_a2a_binding(input).expect("withdrawn binding");
        let presence = online_presence(&withdrawn);
        let error =
            a2a_exchange_demand(&withdrawn, &presence, LOCAL_PARTICIPANT, &message_fixture())
                .expect_err("unpublished binding");
        assert!(error.message().contains("explicitly published binding"));
    }

    #[test]
    fn exchange_presence_must_belong_to_the_binding_and_participant() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let mut presence = online_presence(&binding);
        presence.binding_ref = "a2a-binding:foreign".to_string();
        let error = a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &message_fixture())
            .expect_err("foreign presence");
        assert!(error
            .message()
            .contains("must belong to the selected binding/Participant"));

        let mut presence = online_presence(&binding);
        presence.participant_ref = "participant:foreign".to_string();
        let error = a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &message_fixture())
            .expect_err("foreign participant");
        assert!(error
            .message()
            .contains("must belong to the selected binding/Participant"));
    }

    #[test]
    fn exchange_requires_a_reachable_endpoint() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let mut presence = online_presence(&binding);
        presence.availability = A2aAvailability::Offline;
        let error = a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &message_fixture())
            .expect_err("offline presence");
        assert!(error.message().contains("not currently reachable: offline"));
    }

    #[test]
    fn exchange_message_text_respects_the_32kib_bound() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let presence = online_presence(&binding);

        let mut message = message_fixture();
        message.text = "x".repeat(A2A_MESSAGE_MAX_BYTES);
        a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &message)
            .expect("exactly 32 KiB passes");

        let mut oversized = message_fixture();
        oversized.text = "x".repeat(A2A_MESSAGE_MAX_BYTES + 1);
        let error = a2a_exchange_demand(&binding, &presence, LOCAL_PARTICIPANT, &oversized)
            .expect_err("oversized text");
        assert!(error.message().contains("32768 byte limit"));
    }

    #[test]
    fn difference_validates_and_round_trips() {
        let difference = validate_a2a_difference(&difference_fixture()).expect("valid");
        assert_eq!(difference.transport_result.kind, A2aTransportKind::Message);
        assert_eq!(
            difference.transport_result.reference,
            "a2a-message:return-encounter"
        );
        assert_eq!(
            difference.exchange_authority.grant_ref,
            "exchange-grant:encounter"
        );
        assert_eq!(difference.admission, "pending");

        let json = serde_json::to_value(&difference).expect("serializes");
        let reparsed = validate_a2a_difference(&json).expect("revalidates");
        assert_eq!(difference, reparsed);
    }

    #[test]
    fn difference_rejects_wrong_schema_admission_and_transport_kind() {
        let mut json = difference_fixture();
        json["schema"] = json!("oi.other/v1");
        let error = validate_a2a_difference(&json).expect_err("schema");
        assert!(error
            .message()
            .contains("Unsupported A2A difference schema"));

        let mut json = difference_fixture();
        json["admission"] = json!("admitted");
        let error = validate_a2a_difference(&json).expect_err("admission");
        assert!(error.message().contains("begin pending explicit admission"));

        let mut json = difference_fixture();
        json["transport_result"]["kind"] = json!("artifact");
        let error = validate_a2a_difference(&json).expect_err("transport kind");
        assert!(error.message().contains("must be task or message"));

        let mut json = difference_fixture();
        json["exchange_authority"].as_object_mut().expect("object")["grant_ref"] = json!("");
        let error = validate_a2a_difference(&json).expect_err("grant ref");
        assert!(error
            .message()
            .contains("grant_ref must be a non-empty string"));
    }

    #[test]
    fn agent_card_selection_matches_the_published_interface_and_ignores_card_claims() {
        let binding = published_binding(
            "https://agent.example/a2a",
            "https://agent.example/.well-known/agent-card.json",
        );
        let card = adversarial_agent_card("https://agent.example/a2a");
        let selected = assert_a2a_card(&card, &binding).expect("interface selected");
        // The adversarial identity claims ride along as transport material...
        assert_eq!(selected["agentRef"], json!("agent:malicious-card-claim"));
        // ...but selection matched only on protocol binding, version and endpoint.
        assert_eq!(selected["protocolBinding"], json!("HTTP+JSON"));
        assert_eq!(selected["protocolVersion"], json!("1.0"));

        let mut mismatched = card.clone();
        mismatched["supportedInterfaces"][0]["url"] = json!("https://other.example/a2a");
        let error = assert_a2a_card(&mismatched, &binding).expect_err("no matching interface");
        assert!(error.message().contains("does not advertise"));

        // A candidate with an unusable URL is skipped, not fatal (JS try/catch).
        let mut skippable = card;
        skippable["supportedInterfaces"][0]["url"] = json!("https://user:secret@agent.example/a2a");
        skippable["supportedInterfaces"]
            .as_array_mut()
            .expect("array")
            .push(json!({
                "url": "https://agent.example/a2a",
                "protocolBinding": "HTTP+JSON",
                "protocolVersion": "1.0"
            }));
        let selected = assert_a2a_card(&skippable, &binding).expect("second candidate selected");
        assert!(selected.get("agentRef").is_none());
    }

    #[test]
    fn send_url_percent_encodes_the_tenant() {
        assert_eq!(
            a2a_send_url("https://agent.example/a2a", None).expect("url"),
            "https://agent.example/a2a/message:send"
        );
        assert_eq!(
            a2a_send_url("https://agent.example/a2a", Some("acme corp")).expect("url"),
            "https://agent.example/a2a/acme%20corp/message:send"
        );
        assert!(a2a_send_url("http://agent.example/a2a", None).is_err());
    }

    #[test]
    fn transport_ref_reads_task_or_message_and_refuses_else() {
        let task = transport_ref(&json!({ "task": { "id": "a2a-task:fixture-1" } }))
            .expect("task response");
        assert_eq!(task.kind, A2aTransportKind::Task);
        assert_eq!(task.reference, "a2a-task:fixture-1");

        let message = transport_ref(&json!({
            "message": { "messageId": "a2a-message:response-1" }
        }))
        .expect("message response");
        assert_eq!(message.kind, A2aTransportKind::Message);
        assert_eq!(message.reference, "a2a-message:response-1");

        let error = transport_ref(&json!({ "neither": true })).expect_err("no ref");
        assert!(error.message().contains("must contain task or message"));
    }

    #[test]
    fn bounded_json_shape_enforces_depth_cardinality_and_string_bounds() {
        let nested = |depth: usize| {
            let mut value = json!(true);
            for _ in 0..depth {
                value = json!({ "nested": value });
            }
            value
        };
        bounded_json_shape(&nested(A2A_MAX_JSON_DEPTH), "A2A exchange").expect("within depth");
        let error = bounded_json_shape(&nested(A2A_MAX_JSON_DEPTH + 1), "A2A exchange")
            .expect_err("too deep");
        assert!(error.message().contains("maximum JSON depth 12"));

        let wide: Vec<Value> = (0..=A2A_MAX_JSON_COLLECTION).map(Value::from).collect();
        let error = bounded_json_shape(&Value::Array(wide), "A2A exchange").expect_err("too wide");
        assert!(error.message().contains("256 array entries"));

        let oversized = json!(Value::String("x".repeat(A2A_RESPONSE_MAX_BYTES + 1)));
        let error = bounded_json_shape(&oversized, "A2A exchange").expect_err("oversized string");
        assert!(error.message().contains("oversized string"));

        bounded_json_shape(
            &json!({ "message": { "parts": [{ "text": "returned difference" }] } }),
            "A2A exchange",
        )
        .expect("normal payloads pass");
    }

    #[test]
    fn public_url_normalization_matches_the_js_serialization() {
        assert_eq!(
            public_url("https://agent.example/", "endpoint").expect("url"),
            "https://agent.example"
        );
        assert_eq!(
            public_url("https://Agent.EXAMPLE/a2a/", "endpoint").expect("url"),
            "https://agent.example/a2a"
        );
        assert_eq!(
            public_url("https://agent.example:443/a2a", "endpoint").expect("url"),
            "https://agent.example/a2a"
        );
        assert_eq!(
            public_url("https://agent.example:8443/a2a", "endpoint").expect("url"),
            "https://agent.example:8443/a2a"
        );
        assert_eq!(
            public_url("http://[::1]:8080/a2a", "endpoint").expect("url"),
            "http://[::1]:8080/a2a"
        );
    }

    #[test]
    fn a2a_specific_admission_stays_fail_closed() {
        let error = admit_a2a_difference::<()>().expect_err("tombstone");
        assert!(error
            .message()
            .contains("A2A-specific Admission is disabled"));
    }
}
