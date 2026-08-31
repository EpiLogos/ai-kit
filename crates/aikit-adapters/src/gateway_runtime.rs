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
    ConnectorDescriptor, ConnectorHealth, ConversationAddress, DeliveryReceipt, GatewayConnector,
    InboundEvent, InboundEventKind, MediaReference, OutboundOperation, OutboundOperationKind,
    SenderIdentity, GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const AGENCY_GATEWAY_VERSION: &str = "aikit.agency-gateway/v1";
pub const ACTUATION_STREAM_SCHEMA: &str = "actuation.stream/v1";

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
        if !self.address.platform.eq_ignore_ascii_case(&descriptor.platform) {
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
        }
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
                format!("inbound event {} uses an unregistered connector", event.event_ref),
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
        let pending = self.pending_deliveries.get(&receipt.operation_ref).ok_or_else(|| {
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
        for binding in snapshot.bindings {
            gateway.bind(binding)?;
        }
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
        for binding in gateway.bindings.values() {
            if let Some(stream) = gateway.streams.get(&binding.actuation_stream_ref) {
                stream.ensure_binding(binding)?;
            }
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
            inbound.custom_kind.as_deref().or(Some("gateway-inbound/custom")),
        ),
    };

    let mut metadata = Map::new();
    metadata.insert("connector_ref".into(), json!(inbound.connector_ref.to_string()));
    metadata.insert("connector_event_ref".into(), json!(inbound.event_ref.to_string()));
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
            json!(
                inbound
                    .media
                    .iter()
                    .map(|media| media.media_ref.to_string())
                    .collect::<Vec<_>>()
            ),
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
    RegisterConnector { descriptor: ConnectorDescriptor },
    Bind { binding: GatewayBinding },
    Unbind { binding_ref: ResourceRef },
    Ingest { event: InboundEvent },
    Replay {
        stream_ref: ResourceRef,
        #[serde(default)]
        after_sequence: u64,
        limit: usize,
    },
    PrepareOperation {
        binding_ref: ResourceRef,
        operation: OutboundOperationKind,
    },
    RecordDelivery { receipt: DeliveryReceipt },
    SetConnectorHealth { health: ConnectorHealth },
    Control {
        binding_ref: ResourceRef,
        operation: GatewayActuationControlOperation,
    },
    Snapshot,
    Restore { snapshot: GatewaySnapshot },
    Shutdown,
}

impl GatewayCommand {
    pub fn is_shutdown(&self) -> bool {
        matches!(self, Self::Shutdown)
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
    },
    Discovery { discovery: GatewayDiscovery },
    Status { status: GatewayStatus },
    Registered { connector_ref: ResourceRef },
    Bound { binding_ref: ResourceRef },
    Unbound { binding_ref: ResourceRef },
    Ingress { result: GatewayIngressResult },
    Replay { replay: GatewayReplay },
    OperationPrepared { operation: OutboundOperation },
    DeliveryRecorded { operation_ref: ResourceRef },
    ConnectorHealthRecorded { connector_ref: ResourceRef },
    ControlIntent { intent: GatewayActuationControlIntent },
    Snapshot { snapshot: GatewaySnapshot },
    Restored { status: GatewayStatus },
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
        }),
        GatewayCommand::Discover => Ok(GatewayResponse::Discovery {
            discovery: gateway.discovery(),
        }),
        GatewayCommand::Status => Ok(GatewayResponse::Status {
            status: gateway.status(),
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
        GatewayCommand::Snapshot => Ok(GatewayResponse::Snapshot {
            snapshot: gateway.snapshot(),
        }),
        GatewayCommand::Restore { snapshot } => {
            *gateway = AgencyGateway::from_snapshot(snapshot)?;
            Ok(GatewayResponse::Restored {
                status: gateway.status(),
            })
        }
        GatewayCommand::Shutdown => Ok(GatewayResponse::Shutdown),
    }
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
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
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
        assert_eq!(
            event.event["metadata"]["native_sender_id"],
            "user-7"
        );
    }

    #[test]
    fn multiple_surfaces_share_one_stream_without_multiplying_agent_identity() {
        let mut gateway = gateway();
        gateway.register_connector(connector("telegram")).unwrap();
        gateway.register_connector(connector("slack")).unwrap();
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
        gateway.bind(binding("slack", "channel-7", "slack")).unwrap();
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
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
        for id in 1..=4 {
            gateway
                .ingest(inbound(
                    "telegram",
                    "chat-42",
                    &id.to_string(),
                    "user-7",
                ))
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
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
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
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
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
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
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
        gateway.bind(binding("telegram", "chat-42", "telegram")).unwrap();
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
}