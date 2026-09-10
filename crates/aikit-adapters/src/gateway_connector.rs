//! Public connector SDK for the AIKit Agency Gateway.
//!
//! A connector adapts one external communication system (Telegram, Slack,
//! Discord, a webhook service, an external gateway, etc.) to a small portable
//! ingress/delivery contract. It does **not** own AgentSession, Agency, Actuation,
//! ActuationStream or Surface identity.
//!
//! First-party Rust connectors may implement [`GatewayConnector`] directly and
//! compile into the gateway runtime. Out-of-process connectors use the same
//! portable types through `aikit.gateway-connector-wire/v1` over a carrier such as
//! stdio, a local socket or authenticated WebSocket.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GATEWAY_CONNECTOR_SDK_VERSION: &str = "aikit.gateway-connector/v1";
pub const GATEWAY_CONNECTOR_WIRE_VERSION: &str = "aikit.gateway-connector-wire/v1";
pub const GATEWAY_CONNECTOR_SCHEMA_PATH: &str = "contracts/gateway-connector-v1.schema.json";

pub type ConnectorFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorOperation {
    Send,
    Edit,
    Delete,
    React,
    Typing,
    Media,
    Threads,
    Streaming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConnectorCapabilities {
    #[serde(default)]
    pub operations: BTreeSet<ConnectorOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_text_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_media_bytes: Option<u64>,
    #[serde(default)]
    pub media_types: BTreeSet<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ConnectorCapabilities {
    pub fn supports(&self, operation: ConnectorOperation) -> bool {
        self.operations.contains(&operation)
    }
}

/// One connector implementation/material instance as seen by the gateway.
///
/// `connector_ref` is connector identity, not a Surface or AgentSession ref.
/// Credentials are deliberately absent: implementations receive secrets through
/// their deployment/configuration mechanism and may disclose only a non-secret
/// `configuration_ref` for provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorDescriptor {
    pub version: String,
    pub connector_ref: ResourceRef,
    pub platform: String,
    pub implementation: String,
    pub capabilities: ConnectorCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_ref: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ConnectorDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.version != GATEWAY_CONNECTOR_SDK_VERSION {
            return Err(AikitError::new(
                "gateway_connector.unsupported_sdk",
                format!(
                    "connector {} uses unsupported SDK version {}",
                    self.connector_ref, self.version
                ),
            ));
        }
        if self.platform.trim().is_empty() {
            return Err(AikitError::new(
                "gateway_connector.empty_platform",
                format!("connector {} must name a platform", self.connector_ref),
            ));
        }
        if self.implementation.trim().is_empty() {
            return Err(AikitError::new(
                "gateway_connector.empty_implementation",
                format!(
                    "connector {} must name its implementation",
                    self.connector_ref
                ),
            ));
        }
        if self
            .capabilities
            .media_types
            .iter()
            .any(|media_type| media_type.trim().is_empty())
        {
            return Err(AikitError::new(
                "gateway_connector.empty_media_type",
                format!(
                    "connector {} advertises an empty media type",
                    self.connector_ref
                ),
            ));
        }
        Ok(())
    }
}

/// Provider-native conversation locator.
///
/// None of these values become canonical AgentSession/Surface identity by
/// implication. Gateway binding resolves this address to semantic O:I refs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConversationAddress {
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

impl ConversationAddress {
    pub fn validate(&self) -> Result<()> {
        if self.platform.trim().is_empty() || self.conversation_id.trim().is_empty() {
            return Err(AikitError::new(
                "gateway_connector.invalid_conversation",
                "conversation address requires non-empty platform and conversation_id",
            ));
        }
        if self
            .scope_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
            || self
                .thread_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err(AikitError::new(
                "gateway_connector.invalid_conversation",
                "optional conversation scope/thread ids must be non-empty when supplied",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SenderKind {
    Human,
    Agent,
    Bot,
    System,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderIdentity {
    pub native_sender_id: String,
    pub kind: SenderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl SenderIdentity {
    pub fn validate(&self) -> Result<()> {
        if self.native_sender_id.trim().is_empty() {
            return Err(AikitError::new(
                "gateway_connector.invalid_sender",
                "sender native id must not be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboundEventKind {
    Message,
    Reaction,
    Media,
    Membership,
    Command,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaReference {
    pub media_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

/// Normalized ingress with provider-native provenance preserved alongside it.
/// Admission to an AgentSession/ActuationStream happens after gateway policy and
/// binding resolution; connector ingress itself never decides that relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundEvent {
    pub event_ref: ResourceRef,
    pub connector_ref: ResourceRef,
    pub address: ConversationAddress,
    pub sender: SenderIdentity,
    pub kind: InboundEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_native_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub media: Vec<MediaReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(default)]
    pub native: BTreeMap<String, Value>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl InboundEvent {
    pub fn validate(&self, descriptor: &ConnectorDescriptor) -> Result<()> {
        descriptor.validate()?;
        self.address.validate()?;
        self.sender.validate()?;
        if self.connector_ref != descriptor.connector_ref {
            return Err(AikitError::new(
                "gateway_connector.connector_identity_drift",
                format!(
                    "inbound event {} cites connector {} but was emitted by {}",
                    self.event_ref, self.connector_ref, descriptor.connector_ref
                ),
            ));
        }
        if !self
            .address
            .platform
            .eq_ignore_ascii_case(&descriptor.platform)
        {
            return Err(AikitError::new(
                "gateway_connector.platform_drift",
                format!(
                    "inbound event {} platform {} does not match connector platform {}",
                    self.event_ref, self.address.platform, descriptor.platform
                ),
            ));
        }
        match self.kind {
            InboundEventKind::Custom if self.custom_kind.as_deref().is_none_or(str::is_empty) => {
                return Err(AikitError::new(
                    "gateway_connector.custom_kind_missing",
                    format!(
                        "custom inbound event {} requires custom_kind",
                        self.event_ref
                    ),
                ));
            }
            InboundEventKind::Custom => {}
            _ if self.custom_kind.is_some() => {
                return Err(AikitError::new(
                    "gateway_connector.unexpected_custom_kind",
                    format!(
                        "inbound event {} may only use custom_kind with kind=custom",
                        self.event_ref
                    ),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum OutboundOperationKind {
    Send {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default)]
        media: Vec<MediaReference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to_native_message_id: Option<String>,
    },
    Edit {
        native_message_id: String,
        text: String,
    },
    Delete {
        native_message_id: String,
    },
    React {
        native_message_id: String,
        reaction: String,
    },
    Typing {
        active: bool,
    },
}

impl OutboundOperationKind {
    pub fn required_capability(&self) -> ConnectorOperation {
        match self {
            Self::Send { media, .. } if !media.is_empty() => ConnectorOperation::Media,
            Self::Send { .. } => ConnectorOperation::Send,
            Self::Edit { .. } => ConnectorOperation::Edit,
            Self::Delete { .. } => ConnectorOperation::Delete,
            Self::React { .. } => ConnectorOperation::React,
            Self::Typing { .. } => ConnectorOperation::Typing,
        }
    }
}

/// Gateway-issued delivery intent. Semantic Agency/Session/Stream lineage belongs
/// to the gateway/Actuation side and may be carried as refs for attribution, while
/// the connector only executes the platform operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboundOperation {
    pub operation_ref: ResourceRef,
    pub connector_ref: ResourceRef,
    pub address: ConversationAddress,
    pub operation: OutboundOperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actuation_stream_ref: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl OutboundOperation {
    pub fn validate(&self, descriptor: &ConnectorDescriptor) -> Result<()> {
        descriptor.validate()?;
        self.address.validate()?;
        if self.connector_ref != descriptor.connector_ref {
            return Err(AikitError::new(
                "gateway_connector.connector_identity_drift",
                format!(
                    "outbound operation {} targets connector {} but current connector is {}",
                    self.operation_ref, self.connector_ref, descriptor.connector_ref
                ),
            ));
        }
        if !self
            .address
            .platform
            .eq_ignore_ascii_case(&descriptor.platform)
        {
            return Err(AikitError::new(
                "gateway_connector.platform_drift",
                format!(
                    "outbound operation {} platform {} does not match connector platform {}",
                    self.operation_ref, self.address.platform, descriptor.platform
                ),
            ));
        }
        let required = self.operation.required_capability();
        if !descriptor.capabilities.supports(required) {
            return Err(AikitError::new(
                "gateway_connector.unsupported_operation",
                format!(
                    "connector {} does not advertise {:?} required by operation {}",
                    descriptor.connector_ref, required, self.operation_ref
                ),
            ));
        }
        if let OutboundOperationKind::Send { text, media, .. } = &self.operation {
            if text.as_deref().is_none_or(str::is_empty) && media.is_empty() {
                return Err(AikitError::new(
                    "gateway_connector.empty_send",
                    format!("send operation {} has no text or media", self.operation_ref),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryState {
    Accepted,
    Delivered,
    Rejected,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub operation_ref: ResourceRef,
    pub connector_ref: ResourceRef,
    pub state: DeliveryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub native: BTreeMap<String, Value>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Degraded,
    Reconnecting,
    Unavailable,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorHealth {
    pub connector_ref: ResourceRef,
    pub state: ConnectorConnectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorHello {
    pub wire_version: String,
    pub descriptor: ConnectorDescriptor,
}

impl ConnectorHello {
    pub fn validate(&self) -> Result<()> {
        if self.wire_version != GATEWAY_CONNECTOR_WIRE_VERSION {
            return Err(AikitError::new(
                "gateway_connector.unsupported_wire",
                format!("unsupported connector wire version {}", self.wire_version),
            ));
        }
        self.descriptor.validate()
    }
}

/// Language-neutral frame family used by out-of-process connectors. A carrier is
/// deliberately not encoded here: JSON frames can ride stdio, UDS/named pipe,
/// authenticated WebSocket or a later transport without changing connector
/// semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ConnectorWireFrame {
    Hello {
        hello: ConnectorHello,
    },
    Inbound {
        event: InboundEvent,
    },
    Outbound {
        operation: OutboundOperation,
    },
    DeliveryReceipt {
        receipt: DeliveryReceipt,
    },
    Health {
        health: ConnectorHealth,
    },
    Shutdown {
        reason: String,
    },
    Error {
        code: String,
        message: String,
        #[serde(default)]
        detail: BTreeMap<String, Value>,
    },
}

/// Object-safe asynchronous Rust connector seam. No dynamic Rust ABI is implied:
/// this is a source-level SDK trait. Third-party binary connectors use the wire
/// contract instead.
pub trait GatewayConnector: Send {
    fn descriptor(&self) -> ConnectorDescriptor;
    fn connect(&mut self) -> ConnectorFuture<'_, ConnectorHello>;
    fn next_event(&mut self) -> ConnectorFuture<'_, Option<InboundEvent>>;
    fn execute(&mut self, operation: OutboundOperation) -> ConnectorFuture<'_, DeliveryReceipt>;
    fn health(&mut self) -> ConnectorFuture<'_, ConnectorHealth>;
    fn disconnect(&mut self) -> ConnectorFuture<'_, ()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorConformance {
    pub connector_ref: ResourceRef,
    pub platform: String,
    pub operations: BTreeSet<ConnectorOperation>,
}

/// Structural conformance shared by first-party and external-style connector
/// specimens. Live platform/API behaviour remains connector-specific evidence.
pub fn verify_connector_descriptor(
    descriptor: &ConnectorDescriptor,
) -> Result<ConnectorConformance> {
    descriptor.validate()?;
    Ok(ConnectorConformance {
        connector_ref: descriptor.connector_ref.clone(),
        platform: descriptor.platform.clone(),
        operations: descriptor.capabilities.operations.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn descriptor() -> ConnectorDescriptor {
        ConnectorDescriptor {
            version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: r("gateway-connector/telegram/test"),
            platform: "telegram".into(),
            implementation: "fixture".into(),
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
                media_types: BTreeSet::from(["image/*".into(), "audio/*".into()]),
                provenance: vec!["fixture".into()],
            },
            configuration_ref: Some(r("gateway-connector-config/telegram/test")),
            provenance: vec!["public SDK fixture".into()],
        }
    }

    fn address() -> ConversationAddress {
        ConversationAddress {
            platform: "telegram".into(),
            scope_id: None,
            conversation_id: "chat-42".into(),
            thread_id: Some("topic-7".into()),
        }
    }

    #[test]
    fn descriptor_conformance_preserves_connector_identity_and_capabilities() {
        let report = verify_connector_descriptor(&descriptor()).unwrap();
        assert_eq!(report.connector_ref, r("gateway-connector/telegram/test"));
        assert!(report.operations.contains(&ConnectorOperation::Send));
        assert!(report.operations.contains(&ConnectorOperation::Threads));
    }

    #[test]
    fn inbound_conversation_identity_does_not_imply_agent_session_identity() {
        let event = InboundEvent {
            event_ref: r("gateway-ingress/telegram/event-1"),
            connector_ref: r("gateway-connector/telegram/test"),
            address: address(),
            sender: SenderIdentity {
                native_sender_id: "user-17".into(),
                kind: SenderKind::Human,
                display_name: Some("Ada".into()),
                metadata: BTreeMap::new(),
            },
            kind: InboundEventKind::Message,
            custom_kind: None,
            native_event_id: Some("update-500".into()),
            native_message_id: Some("message-51".into()),
            reply_to_native_message_id: None,
            text: Some("hello".into()),
            media: Vec::new(),
            observed_at: Some("2026-08-27T11:00:00Z".into()),
            native: BTreeMap::new(),
            provenance: vec!["Telegram update".into()],
        };
        event.validate(&descriptor()).unwrap();
        let encoded = serde_json::to_value(&event).unwrap();
        assert!(encoded.get("agent_session_ref").is_none());
        assert_eq!(encoded["address"]["conversation_id"], "chat-42");
    }

    #[test]
    fn platform_or_connector_identity_drift_is_rejected() {
        let mut event = InboundEvent {
            event_ref: r("gateway-ingress/telegram/event-1"),
            connector_ref: r("gateway-connector/slack/wrong"),
            address: address(),
            sender: SenderIdentity {
                native_sender_id: "user-17".into(),
                kind: SenderKind::Human,
                display_name: None,
                metadata: BTreeMap::new(),
            },
            kind: InboundEventKind::Message,
            custom_kind: None,
            native_event_id: None,
            native_message_id: None,
            reply_to_native_message_id: None,
            text: Some("hello".into()),
            media: Vec::new(),
            observed_at: None,
            native: BTreeMap::new(),
            provenance: Vec::new(),
        };
        assert_eq!(
            event.validate(&descriptor()).unwrap_err().code(),
            "gateway_connector.connector_identity_drift"
        );
        event.connector_ref = descriptor().connector_ref;
        event.address.platform = "slack".into();
        assert_eq!(
            event.validate(&descriptor()).unwrap_err().code(),
            "gateway_connector.platform_drift"
        );
    }

    #[test]
    fn outbound_operation_requires_advertised_capability() {
        let mut limited = descriptor();
        limited
            .capabilities
            .operations
            .remove(&ConnectorOperation::Edit);
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/edit-1"),
            connector_ref: limited.connector_ref.clone(),
            address: address(),
            operation: OutboundOperationKind::Edit {
                native_message_id: "message-51".into(),
                text: "updated".into(),
            },
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: vec!["gateway delivery".into()],
        };
        assert_eq!(
            operation.validate(&limited).unwrap_err().code(),
            "gateway_connector.unsupported_operation"
        );
    }

    #[test]
    fn media_send_requires_media_capability_and_preserves_stream_attribution() {
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/send-media-1"),
            connector_ref: descriptor().connector_ref,
            address: address(),
            operation: OutboundOperationKind::Send {
                text: Some("artifact".into()),
                media: vec![MediaReference {
                    media_ref: r("media/artifact-1"),
                    mime_type: Some("image/png".into()),
                    file_name: Some("plot.png".into()),
                    size_bytes: Some(2400),
                    metadata: BTreeMap::from([("sha256".into(), json!("abc"))]),
                }],
                reply_to_native_message_id: None,
            },
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: Vec::new(),
        };
        operation.validate(&descriptor()).unwrap();
        assert_eq!(
            operation.actuation_stream_ref.as_ref(),
            Some(&r("actuation-stream/root"))
        );
    }

    #[test]
    fn wire_frame_round_trip_is_transport_independent() {
        let frame = ConnectorWireFrame::Hello {
            hello: ConnectorHello {
                wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
                descriptor: descriptor(),
            },
        };
        let encoded = serde_json::to_string(&frame).unwrap();
        let decoded: ConnectorWireFrame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, frame);
        assert!(!encoded.contains("websocket"));
        assert!(!encoded.contains("stdio"));
    }

    #[test]
    fn credentials_are_not_part_of_the_public_descriptor_or_wire_hello() {
        let hello = ConnectorHello {
            wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
            descriptor: descriptor(),
        };
        hello.validate().unwrap();
        let encoded = serde_json::to_value(hello).unwrap();
        let descriptor = &encoded["descriptor"];
        assert!(descriptor.get("token").is_none());
        assert!(descriptor.get("secret").is_none());
        assert!(descriptor.get("credentials").is_none());
        assert!(descriptor.get("configuration_ref").is_some());
    }
}
