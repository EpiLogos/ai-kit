//! First-party Telegram connector for the AIKit Agency Gateway.
//!
//! Telegram contributes provider-native update/chat/thread/message semantics. The
//! connector translates those into the public gateway connector contract; the
//! Agency Gateway remains responsible for binding a Telegram conversation to
//! canonical AgentSession/Agency/Actuation/ActuationStream identity.
//!
//! Network access is behind [`TelegramBotApiTransport`]. This keeps the Bot API
//! state machine fully deterministic in hosted CI while allowing the live HTTPS
//! body to be supplied independently without changing connector semantics.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::gateway_connector::{
    ConnectorCapabilities, ConnectorConnectionState, ConnectorDescriptor, ConnectorFuture,
    ConnectorHealth, ConnectorHello, ConnectorOperation, ConversationAddress, DeliveryReceipt,
    DeliveryState, GatewayConnector, InboundEvent, InboundEventKind, MediaReference,
    OutboundOperation, OutboundOperationKind, SenderIdentity, SenderKind,
    GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION,
};

pub const TELEGRAM_GATEWAY_CONNECTOR_VERSION: &str = "aikit.telegram-gateway/v1";
pub const TELEGRAM_BOT_API_BASE: &str = "https://api.telegram.org";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramConnectorConfig {
    pub connector_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_ref: Option<ResourceRef>,
    #[serde(default = "default_poll_timeout_seconds")]
    pub poll_timeout_seconds: u32,
    #[serde(default)]
    pub allowed_updates: Vec<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

fn default_poll_timeout_seconds() -> u32 {
    30
}

impl TelegramConnectorConfig {
    pub fn validate(&self) -> Result<()> {
        if self.poll_timeout_seconds > 50 {
            return Err(AikitError::new(
                "telegram_gateway.poll_timeout",
                "Telegram long-poll timeout must be between 0 and 50 seconds",
            ));
        }
        if self.allowed_updates.iter().any(|item| item.trim().is_empty()) {
            return Err(AikitError::new(
                "telegram_gateway.empty_allowed_update",
                "Telegram allowed_updates cannot contain empty names",
            ));
        }
        Ok(())
    }
}

/// Minimal provider-neutral Bot API execution seam.
///
/// Implementations return the normal Telegram Bot API response envelope
/// `{ "ok": bool, "result"?: ..., "description"?: ... }` and must never expose
/// the bot token through connector descriptors or gateway wire frames.
pub trait TelegramBotApiTransport: Send {
    fn call(&mut self, method: &str, params: Value) -> Result<Value>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramBotIdentity {
    pub id: i64,
    pub is_bot: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
}

pub struct TelegramConnector<T> {
    transport: T,
    config: TelegramConnectorConfig,
    descriptor: ConnectorDescriptor,
    bot: Option<TelegramBotIdentity>,
    next_update_offset: Option<i64>,
    pending: VecDeque<InboundEvent>,
    health: ConnectorHealth,
}

impl<T: TelegramBotApiTransport> TelegramConnector<T> {
    pub fn new(transport: T, config: TelegramConnectorConfig) -> Result<Self> {
        config.validate()?;
        let descriptor = ConnectorDescriptor {
            version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: config.connector_ref.clone(),
            platform: "telegram".into(),
            implementation: TELEGRAM_GATEWAY_CONNECTOR_VERSION.into(),
            capabilities: ConnectorCapabilities {
                operations: BTreeSet::from([
                    ConnectorOperation::Send,
                    ConnectorOperation::Edit,
                    ConnectorOperation::Delete,
                    ConnectorOperation::React,
                    ConnectorOperation::Typing,
                    ConnectorOperation::Media,
                    ConnectorOperation::Threads,
                ]),
                max_text_bytes: Some(4096 * 4),
                max_media_bytes: None,
                media_types: BTreeSet::from([
                    "image/*".into(),
                    "audio/*".into(),
                    "video/*".into(),
                    "application/*".into(),
                ]),
                provenance: vec![
                    "Telegram Bot API connector".into(),
                    TELEGRAM_GATEWAY_CONNECTOR_VERSION.into(),
                ],
            },
            configuration_ref: config.configuration_ref.clone(),
            provenance: config.provenance.clone(),
        };
        descriptor.validate()?;
        let connector_ref = descriptor.connector_ref.clone();
        Ok(Self {
            transport,
            config,
            descriptor,
            bot: None,
            next_update_offset: None,
            pending: VecDeque::new(),
            health: ConnectorHealth {
                connector_ref,
                state: ConnectorConnectionState::Disconnected,
                detail: None,
                provenance: vec![TELEGRAM_GATEWAY_CONNECTOR_VERSION.into()],
            },
        })
    }

    pub fn descriptor_ref(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    pub fn next_update_offset(&self) -> Option<i64> {
        self.next_update_offset
    }

    pub fn bot_identity(&self) -> Option<&TelegramBotIdentity> {
        self.bot.as_ref()
    }

    pub fn transport_ref(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn connect_now(&mut self) -> Result<ConnectorHello> {
        self.health.state = ConnectorConnectionState::Connecting;
        let response = self.transport.call("getMe", json!({}));
        match response.and_then(telegram_result) {
            Ok(result) => {
                let id = required_i64(&result, "id", "telegram_gateway.get_me")?;
                let is_bot = result.get("is_bot").and_then(Value::as_bool).unwrap_or(true);
                self.bot = Some(TelegramBotIdentity {
                    id,
                    is_bot,
                    username: optional_string(&result, "username"),
                    first_name: optional_string(&result, "first_name"),
                });
                self.health.state = ConnectorConnectionState::Connected;
                self.health.detail = Some(format!("Telegram bot {id} reachable"));
                Ok(ConnectorHello {
                    wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
                    descriptor: self.descriptor.clone(),
                })
            }
            Err(error) => {
                self.health.state = ConnectorConnectionState::Unavailable;
                self.health.detail = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn next_event_now(&mut self) -> Result<Option<InboundEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        if !matches!(
            self.health.state,
            ConnectorConnectionState::Connected
                | ConnectorConnectionState::Degraded
                | ConnectorConnectionState::Reconnecting
        ) {
            return Err(AikitError::new(
                "telegram_gateway.not_connected",
                "Telegram connector must be connected before polling",
            ));
        }

        let mut params = Map::new();
        if let Some(offset) = self.next_update_offset {
            params.insert("offset".into(), json!(offset));
        }
        params.insert("timeout".into(), json!(self.config.poll_timeout_seconds));
        if !self.config.allowed_updates.is_empty() {
            params.insert(
                "allowed_updates".into(),
                json!(self.config.allowed_updates),
            );
        }

        self.health.state = ConnectorConnectionState::Connected;
        let response = self.transport.call("getUpdates", Value::Object(params));
        let result = match response.and_then(telegram_result) {
            Ok(result) => result,
            Err(error) => {
                self.health.state = ConnectorConnectionState::Reconnecting;
                self.health.detail = Some(error.to_string());
                return Err(error);
            }
        };
        let updates = result.as_array().ok_or_else(|| {
            AikitError::new(
                "telegram_gateway.invalid_updates",
                "Telegram getUpdates result must be an array",
            )
        })?;

        let mut ordered = updates.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|update| update.get("update_id").and_then(Value::as_i64).unwrap_or(i64::MAX));
        for update in ordered {
            let update_id = required_i64(update, "update_id", "telegram_gateway.update_id")?;
            self.next_update_offset = Some(
                self.next_update_offset
                    .unwrap_or(i64::MIN)
                    .max(update_id.saturating_add(1)),
            );
            if let Some(event) = telegram_update_to_inbound(&self.descriptor, update)? {
                self.pending.push_back(event);
            }
        }
        self.health.state = ConnectorConnectionState::Connected;
        self.health.detail = Some(format!(
            "Telegram polling through update offset {}",
            self.next_update_offset.unwrap_or(0)
        ));
        Ok(self.pending.pop_front())
    }

    pub fn execute_now(&mut self, operation: OutboundOperation) -> Result<DeliveryReceipt> {
        operation.validate(&self.descriptor)?;
        let method_and_params = telegram_outbound_request(&operation)?;
        let (method, params) = method_and_params;
        let result = self
            .transport
            .call(method, params)
            .and_then(telegram_result);
        match result {
            Ok(result) => {
                self.health.state = ConnectorConnectionState::Connected;
                self.health.detail = Some(format!("Telegram {method} succeeded"));
                Ok(DeliveryReceipt {
                    operation_ref: operation.operation_ref,
                    connector_ref: operation.connector_ref,
                    state: DeliveryState::Delivered,
                    native_message_id: telegram_message_id(&result),
                    detail: None,
                    native: BTreeMap::from([
                        ("method".into(), json!(method)),
                        ("result".into(), result),
                    ]),
                    provenance: vec![
                        TELEGRAM_GATEWAY_CONNECTOR_VERSION.into(),
                        format!("Telegram Bot API {method}"),
                    ],
                })
            }
            Err(error) => {
                self.health.state = ConnectorConnectionState::Degraded;
                self.health.detail = Some(error.to_string());
                Ok(DeliveryReceipt {
                    operation_ref: operation.operation_ref,
                    connector_ref: operation.connector_ref,
                    state: DeliveryState::Failed,
                    native_message_id: None,
                    detail: Some(error.to_string()),
                    native: BTreeMap::from([("method".into(), json!(method))]),
                    provenance: vec![TELEGRAM_GATEWAY_CONNECTOR_VERSION.into()],
                })
            }
        }
    }

    pub fn health_now(&self) -> ConnectorHealth {
        self.health.clone()
    }

    pub fn disconnect_now(&mut self) -> Result<()> {
        self.pending.clear();
        self.health.state = ConnectorConnectionState::Closed;
        self.health.detail = Some("Telegram connector closed".into());
        Ok(())
    }
}

impl<T: TelegramBotApiTransport> GatewayConnector for TelegramConnector<T> {
    fn descriptor(&self) -> ConnectorDescriptor {
        self.descriptor.clone()
    }

    fn connect(&mut self) -> ConnectorFuture<'_, ConnectorHello> {
        let result = self.connect_now();
        Box::pin(async move { result })
    }

    fn next_event(&mut self) -> ConnectorFuture<'_, Option<InboundEvent>> {
        let result = self.next_event_now();
        Box::pin(async move { result })
    }

    fn execute(&mut self, operation: OutboundOperation) -> ConnectorFuture<'_, DeliveryReceipt> {
        let result = self.execute_now(operation);
        Box::pin(async move { result })
    }

    fn health(&mut self) -> ConnectorFuture<'_, ConnectorHealth> {
        let health = self.health_now();
        Box::pin(async move { Ok(health) })
    }

    fn disconnect(&mut self) -> ConnectorFuture<'_, ()> {
        let result = self.disconnect_now();
        Box::pin(async move { result })
    }
}

fn telegram_result(response: Value) -> Result<Value> {
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(response.get("result").cloned().unwrap_or(Value::Null));
    }
    let description = response
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("Telegram Bot API request failed");
    let code = response
        .get("error_code")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into());
    Err(AikitError::new(
        "telegram_gateway.bot_api",
        format!("Telegram Bot API error {code}: {description}"),
    ))
}

fn telegram_update_to_inbound(
    descriptor: &ConnectorDescriptor,
    update: &Value,
) -> Result<Option<InboundEvent>> {
    let update_id = required_i64(update, "update_id", "telegram_gateway.update_id")?;
    for key in [
        "message",
        "edited_message",
        "channel_post",
        "edited_channel_post",
        "business_message",
        "edited_business_message",
    ] {
        if let Some(message) = update.get(key) {
            return Ok(Some(telegram_message_to_inbound(
                descriptor, update_id, key, message,
            )?));
        }
    }
    if let Some(reaction) = update.get("message_reaction") {
        return Ok(Some(telegram_reaction_to_inbound(
            descriptor, update_id, reaction,
        )?));
    }
    if let Some(callback) = update.get("callback_query") {
        return Ok(Some(telegram_callback_to_inbound(
            descriptor, update_id, callback,
        )?));
    }
    Ok(None)
}

fn telegram_message_to_inbound(
    descriptor: &ConnectorDescriptor,
    update_id: i64,
    update_kind: &str,
    message: &Value,
) -> Result<InboundEvent> {
    let address = telegram_message_address(message)?;
    let sender = telegram_sender(message);
    let message_id = required_i64(message, "message_id", "telegram_gateway.message_id")?;
    let text = message
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| message.get("caption").and_then(Value::as_str))
        .map(str::to_owned);
    let media = telegram_media(message)?;
    let kind = if !media.is_empty() && text.is_none() {
        InboundEventKind::Media
    } else {
        InboundEventKind::Message
    };
    let mut native = BTreeMap::new();
    native.insert("update_kind".into(), json!(update_kind));
    native.insert("message".into(), message.clone());

    Ok(InboundEvent {
        event_ref: ResourceRef::parse(format!("telegram-update/{update_id}/{update_kind}"))?,
        connector_ref: descriptor.connector_ref.clone(),
        address,
        sender,
        kind,
        custom_kind: None,
        native_event_id: Some(update_id.to_string()),
        native_message_id: Some(message_id.to_string()),
        reply_to_native_message_id: message
            .get("reply_to_message")
            .and_then(|reply| reply.get("message_id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string()),
        text,
        media,
        observed_at: message
            .get("date")
            .and_then(Value::as_i64)
            .map(|unix| format!("unix:{unix}")),
        native,
        provenance: vec![
            TELEGRAM_GATEWAY_CONNECTOR_VERSION.into(),
            format!("Telegram update {update_id}"),
            format!("Telegram {update_kind}"),
        ],
    })
}

fn telegram_reaction_to_inbound(
    descriptor: &ConnectorDescriptor,
    update_id: i64,
    reaction: &Value,
) -> Result<InboundEvent> {
    let chat = reaction.get("chat").ok_or_else(|| {
        AikitError::new(
            "telegram_gateway.reaction_chat",
            "Telegram message_reaction requires chat",
        )
    })?;
    let chat_id = required_i64(chat, "id", "telegram_gateway.chat_id")?;
    let message_id = required_i64(reaction, "message_id", "telegram_gateway.message_id")?;
    let sender = reaction
        .get("user")
        .map(telegram_user_sender)
        .unwrap_or_else(|| SenderIdentity {
            native_sender_id: "telegram-system".into(),
            kind: SenderKind::System,
            display_name: None,
            metadata: BTreeMap::new(),
        });
    Ok(InboundEvent {
        event_ref: ResourceRef::parse(format!("telegram-update/{update_id}/message-reaction"))?,
        connector_ref: descriptor.connector_ref.clone(),
        address: ConversationAddress {
            platform: "telegram".into(),
            scope_id: chat.get("type").and_then(Value::as_str).map(str::to_owned),
            conversation_id: chat_id.to_string(),
            thread_id: None,
        },
        sender,
        kind: InboundEventKind::Reaction,
        custom_kind: None,
        native_event_id: Some(update_id.to_string()),
        native_message_id: Some(message_id.to_string()),
        reply_to_native_message_id: None,
        text: None,
        media: Vec::new(),
        observed_at: reaction
            .get("date")
            .and_then(Value::as_i64)
            .map(|unix| format!("unix:{unix}")),
        native: BTreeMap::from([("message_reaction".into(), reaction.clone())]),
        provenance: vec![TELEGRAM_GATEWAY_CONNECTOR_VERSION.into()],
    })
}

fn telegram_callback_to_inbound(
    descriptor: &ConnectorDescriptor,
    update_id: i64,
    callback: &Value,
) -> Result<InboundEvent> {
    let message = callback.get("message");
    let address = if let Some(message) = message {
        telegram_message_address(message)?
    } else {
        ConversationAddress {
            platform: "telegram".into(),
            scope_id: Some("inline".into()),
            conversation_id: callback
                .get("inline_message_id")
                .and_then(Value::as_str)
                .unwrap_or("inline")
                .to_string(),
            thread_id: None,
        }
    };
    let sender = callback
        .get("from")
        .map(telegram_user_sender)
        .unwrap_or_else(|| SenderIdentity {
            native_sender_id: "telegram-unknown".into(),
            kind: SenderKind::Unknown,
            display_name: None,
            metadata: BTreeMap::new(),
        });
    Ok(InboundEvent {
        event_ref: ResourceRef::parse(format!("telegram-update/{update_id}/callback-query"))?,
        connector_ref: descriptor.connector_ref.clone(),
        address,
        sender,
        kind: InboundEventKind::Command,
        custom_kind: None,
        native_event_id: Some(update_id.to_string()),
        native_message_id: message
            .and_then(|message| message.get("message_id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string()),
        reply_to_native_message_id: None,
        text: callback.get("data").and_then(Value::as_str).map(str::to_owned),
        media: Vec::new(),
        observed_at: None,
        native: BTreeMap::from([("callback_query".into(), callback.clone())]),
        provenance: vec![TELEGRAM_GATEWAY_CONNECTOR_VERSION.into()],
    })
}

fn telegram_message_address(message: &Value) -> Result<ConversationAddress> {
    let chat = message.get("chat").ok_or_else(|| {
        AikitError::new(
            "telegram_gateway.message_chat",
            "Telegram message requires chat",
        )
    })?;
    let chat_id = required_i64(chat, "id", "telegram_gateway.chat_id")?;
    let thread_id = message
        .get("message_thread_id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .or_else(|| {
            message
                .get("direct_messages_topic")
                .and_then(|topic| topic.get("topic_id"))
                .and_then(Value::as_i64)
                .map(|id| id.to_string())
        });
    Ok(ConversationAddress {
        platform: "telegram".into(),
        scope_id: chat.get("type").and_then(Value::as_str).map(str::to_owned),
        conversation_id: chat_id.to_string(),
        thread_id,
    })
}

fn telegram_sender(message: &Value) -> SenderIdentity {
    if let Some(user) = message.get("from") {
        return telegram_user_sender(user);
    }
    if let Some(chat) = message.get("sender_chat") {
        return SenderIdentity {
            native_sender_id: chat
                .get("id")
                .and_then(Value::as_i64)
                .map(|id| id.to_string())
                .unwrap_or_else(|| "telegram-sender-chat".into()),
            kind: SenderKind::Unknown,
            display_name: chat
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned),
            metadata: BTreeMap::from([("telegram_sender_chat".into(), chat.clone().to_string())]),
        };
    }
    SenderIdentity {
        native_sender_id: "telegram-system".into(),
        kind: SenderKind::System,
        display_name: None,
        metadata: BTreeMap::new(),
    }
}

fn telegram_user_sender(user: &Value) -> SenderIdentity {
    let is_bot = user.get("is_bot").and_then(Value::as_bool).unwrap_or(false);
    let display_name = [
        user.get("first_name").and_then(Value::as_str),
        user.get("last_name").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let mut metadata = BTreeMap::new();
    if let Some(username) = user.get("username").and_then(Value::as_str) {
        metadata.insert("telegram_username".into(), username.into());
    }
    SenderIdentity {
        native_sender_id: user
            .get("id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string())
            .unwrap_or_else(|| "telegram-unknown-user".into()),
        kind: if is_bot { SenderKind::Bot } else { SenderKind::Human },
        display_name: (!display_name.is_empty()).then_some(display_name),
        metadata,
    }
}

fn telegram_media(message: &Value) -> Result<Vec<MediaReference>> {
    let mut media = Vec::new();
    if let Some(photos) = message.get("photo").and_then(Value::as_array) {
        if let Some(photo) = photos.last() {
            media.push(telegram_file_media("photo", photo, Some("image/*"))?);
        }
    }
    for (key, mime) in [
        ("document", None),
        ("audio", Some("audio/*")),
        ("voice", Some("audio/ogg")),
        ("video", Some("video/*")),
        ("video_note", Some("video/*")),
        ("animation", Some("video/*")),
        ("sticker", Some("image/webp")),
    ] {
        if let Some(file) = message.get(key) {
            media.push(telegram_file_media(key, file, mime)?);
        }
    }
    Ok(media)
}

fn telegram_file_media(
    kind: &str,
    file: &Value,
    fallback_mime: Option<&str>,
) -> Result<MediaReference> {
    let file_id = file.get("file_id").and_then(Value::as_str).ok_or_else(|| {
        AikitError::new(
            "telegram_gateway.media_file_id",
            format!("Telegram {kind} has no file_id"),
        )
    })?;
    let mut metadata = BTreeMap::from([
        ("telegram_kind".into(), json!(kind)),
        ("telegram_file_id".into(), json!(file_id)),
    ]);
    if let Some(unique) = file.get("file_unique_id").and_then(Value::as_str) {
        metadata.insert("telegram_file_unique_id".into(), json!(unique));
    }
    Ok(MediaReference {
        media_ref: ResourceRef::parse(format!("telegram-file/{file_id}"))?,
        mime_type: file
            .get("mime_type")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| fallback_mime.map(str::to_owned)),
        file_name: file
            .get("file_name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        size_bytes: file.get("file_size").and_then(Value::as_u64),
        metadata,
    })
}

fn telegram_outbound_request(operation: &OutboundOperation) -> Result<(&'static str, Value)> {
    let chat_id = parse_i64(
        &operation.address.conversation_id,
        "telegram_gateway.outbound_chat_id",
        "Telegram outbound conversation_id must be an integer chat id",
    )?;
    let thread_id = operation
        .address
        .thread_id
        .as_deref()
        .map(|value| {
            parse_i64(
                value,
                "telegram_gateway.outbound_thread_id",
                "Telegram thread_id must be an integer message_thread_id",
            )
        })
        .transpose()?;
    match &operation.operation {
        OutboundOperationKind::Send {
            text,
            media,
            reply_to_native_message_id,
        } => {
            let reply = reply_to_native_message_id
                .as_deref()
                .map(|value| {
                    parse_i64(
                        value,
                        "telegram_gateway.reply_message_id",
                        "Telegram reply message id must be an integer",
                    )
                })
                .transpose()?;
            if media.is_empty() {
                let text = text.as_deref().ok_or_else(|| {
                    AikitError::new(
                        "telegram_gateway.empty_send",
                        "Telegram text send requires text",
                    )
                })?;
                let mut params = Map::from_iter([
                    ("chat_id".into(), json!(chat_id)),
                    ("text".into(), json!(text)),
                ]);
                add_thread_and_reply(&mut params, thread_id, reply);
                return Ok(("sendMessage", Value::Object(params)));
            }
            if media.len() > 1 {
                return telegram_media_group(operation, chat_id, thread_id, reply);
            }
            let media = &media[0];
            let file_id = telegram_outbound_file_id(media)?;
            let mime = media.mime_type.as_deref().unwrap_or("");
            let (method, field) = if mime.starts_with("image/") {
                ("sendPhoto", "photo")
            } else if mime.starts_with("audio/") {
                ("sendAudio", "audio")
            } else if mime.starts_with("video/") {
                ("sendVideo", "video")
            } else {
                ("sendDocument", "document")
            };
            let mut params = Map::from_iter([
                ("chat_id".into(), json!(chat_id)),
                (field.into(), json!(file_id)),
            ]);
            if let Some(text) = text {
                params.insert("caption".into(), json!(text));
            }
            add_thread_and_reply(&mut params, thread_id, reply);
            Ok((method, Value::Object(params)))
        }
        OutboundOperationKind::Edit {
            native_message_id,
            text,
        } => {
            let message_id = parse_i64(
                native_message_id,
                "telegram_gateway.edit_message_id",
                "Telegram edit message id must be an integer",
            )?;
            Ok((
                "editMessageText",
                json!({ "chat_id": chat_id, "message_id": message_id, "text": text }),
            ))
        }
        OutboundOperationKind::Delete { native_message_id } => {
            let message_id = parse_i64(
                native_message_id,
                "telegram_gateway.delete_message_id",
                "Telegram delete message id must be an integer",
            )?;
            Ok((
                "deleteMessage",
                json!({ "chat_id": chat_id, "message_id": message_id }),
            ))
        }
        OutboundOperationKind::React {
            native_message_id,
            reaction,
        } => {
            let message_id = parse_i64(
                native_message_id,
                "telegram_gateway.reaction_message_id",
                "Telegram reaction message id must be an integer",
            )?;
            Ok((
                "setMessageReaction",
                json!({
                    "chat_id": chat_id,
                    "message_id": message_id,
                    "reaction": [{"type":"emoji", "emoji": reaction}]
                }),
            ))
        }
        OutboundOperationKind::Typing { active } => {
            if !active {
                return Ok(("sendChatAction", json!({"chat_id": chat_id, "action":"cancel"})));
            }
            let mut params = Map::from_iter([
                ("chat_id".into(), json!(chat_id)),
                ("action".into(), json!("typing")),
            ]);
            if let Some(thread_id) = thread_id {
                params.insert("message_thread_id".into(), json!(thread_id));
            }
            Ok(("sendChatAction", Value::Object(params)))
        }
    }
}

fn telegram_media_group(
    operation: &OutboundOperation,
    chat_id: i64,
    thread_id: Option<i64>,
    reply: Option<i64>,
) -> Result<(&'static str, Value)> {
    let OutboundOperationKind::Send { text, media, .. } = &operation.operation else {
        unreachable!("media group is only used for send")
    };
    let mut items = Vec::new();
    for (index, media) in media.iter().enumerate() {
        let file_id = telegram_outbound_file_id(media)?;
        let mime = media.mime_type.as_deref().unwrap_or("");
        let media_type = if mime.starts_with("image/") {
            "photo"
        } else if mime.starts_with("video/") {
            "video"
        } else if mime.starts_with("audio/") {
            "audio"
        } else {
            "document"
        };
        let mut item = Map::from_iter([
            ("type".into(), json!(media_type)),
            ("media".into(), json!(file_id)),
        ]);
        if index == 0 {
            if let Some(text) = text {
                item.insert("caption".into(), json!(text));
            }
        }
        items.push(Value::Object(item));
    }
    let mut params = Map::from_iter([
        ("chat_id".into(), json!(chat_id)),
        ("media".into(), Value::Array(items)),
    ]);
    add_thread_and_reply(&mut params, thread_id, reply);
    Ok(("sendMediaGroup", Value::Object(params)))
}

fn add_thread_and_reply(params: &mut Map<String, Value>, thread_id: Option<i64>, reply: Option<i64>) {
    if let Some(thread_id) = thread_id {
        params.insert("message_thread_id".into(), json!(thread_id));
    }
    if let Some(message_id) = reply {
        params.insert("reply_parameters".into(), json!({"message_id": message_id}));
    }
}

fn telegram_outbound_file_id(media: &MediaReference) -> Result<String> {
    media
        .metadata
        .get("telegram_file_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            AikitError::new(
                "telegram_gateway.media_not_materialised",
                format!(
                    "media {} has no Telegram file_id; upload/materialisation is required before delivery",
                    media.media_ref
                ),
            )
        })
}

fn telegram_message_id(result: &Value) -> Option<String> {
    result
        .get("message_id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .or_else(|| {
            result
                .as_array()
                .and_then(|items| items.last())
                .and_then(|item| item.get("message_id"))
                .and_then(Value::as_i64)
                .map(|id| id.to_string())
        })
}

fn required_i64(value: &Value, key: &str, code: &'static str) -> Result<i64> {
    value.get(key).and_then(Value::as_i64).ok_or_else(|| {
        AikitError::new(code, format!("Telegram payload requires integer `{key}`"))
    })
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn parse_i64(value: &str, code: &'static str, message: &'static str) -> Result<i64> {
    value.parse::<i64>().map_err(|_| AikitError::new(code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_runtime::{text_send, AgencyGateway, GatewayBinding, GatewayIngressDecision, GatewayIngressPolicy, GatewayIngressResult};
    use std::collections::VecDeque;

    #[derive(Debug, Default)]
    struct FakeTelegramTransport {
        replies: VecDeque<Value>,
        calls: Vec<(String, Value)>,
    }

    impl FakeTelegramTransport {
        fn with_replies(replies: Vec<Value>) -> Self {
            Self {
                replies: replies.into(),
                calls: Vec::new(),
            }
        }
    }

    impl TelegramBotApiTransport for FakeTelegramTransport {
        fn call(&mut self, method: &str, params: Value) -> Result<Value> {
            self.calls.push((method.into(), params));
            self.replies.pop_front().ok_or_else(|| {
                AikitError::new(
                    "telegram_gateway.fake_exhausted",
                    format!("fake Telegram transport has no reply for {method}"),
                )
            })
        }
    }

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn config() -> TelegramConnectorConfig {
        TelegramConnectorConfig {
            connector_ref: r("gateway-connector/telegram/main"),
            configuration_ref: Some(r("gateway-config/telegram/main")),
            poll_timeout_seconds: 30,
            allowed_updates: vec![
                "message".into(),
                "edited_message".into(),
                "message_reaction".into(),
                "callback_query".into(),
            ],
            provenance: vec!["test configuration".into()],
        }
    }

    fn connected_connector(updates: Value) -> TelegramConnector<FakeTelegramTransport> {
        let transport = FakeTelegramTransport::with_replies(vec![
            json!({"ok":true,"result":{"id":777,"is_bot":true,"username":"epi_bot","first_name":"Epi"}}),
            json!({"ok":true,"result":updates}),
        ]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        connector.connect_now().unwrap();
        connector
    }

    #[test]
    fn connect_discovers_bot_without_disclosing_credentials() {
        let transport = FakeTelegramTransport::with_replies(vec![json!({
            "ok":true,
            "result":{"id":777,"is_bot":true,"username":"epi_bot","first_name":"Epi"}
        })]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        let hello = connector.connect_now().unwrap();
        assert_eq!(connector.bot_identity().unwrap().id, 777);
        assert_eq!(hello.descriptor.platform, "telegram");
        let encoded = serde_json::to_string(&hello).unwrap();
        assert!(!encoded.contains("token"));
        assert_eq!(connector.transport_ref().calls[0].0, "getMe");
    }

    #[test]
    fn get_updates_advances_offset_and_maps_group_topic_identity() {
        let updates = json!([
            {
                "update_id":100,
                "message":{
                    "message_id":7,
                    "message_thread_id":88,
                    "date":1788174000,
                    "chat":{"id":-100123,"type":"supergroup","title":"O:I"},
                    "from":{"id":42,"is_bot":false,"first_name":"Frank","username":"f"},
                    "text":"hello gateway"
                }
            },
            {
                "update_id":101,
                "message":{
                    "message_id":8,
                    "date":1788174001,
                    "chat":{"id":55,"type":"private"},
                    "from":{"id":42,"is_bot":false,"first_name":"Frank"},
                    "text":"second"
                }
            }
        ]);
        let mut connector = connected_connector(updates);
        let first = connector.next_event_now().unwrap().unwrap();
        assert_eq!(first.address.conversation_id, "-100123");
        assert_eq!(first.address.scope_id.as_deref(), Some("supergroup"));
        assert_eq!(first.address.thread_id.as_deref(), Some("88"));
        assert_eq!(first.sender.native_sender_id, "42");
        assert_eq!(first.text.as_deref(), Some("hello gateway"));
        assert_eq!(connector.next_update_offset(), Some(102));
        let second = connector.next_event_now().unwrap().unwrap();
        assert_eq!(second.address.conversation_id, "55");
        assert_eq!(second.native_message_id.as_deref(), Some("8"));
        let poll = &connector.transport_ref().calls[1];
        assert_eq!(poll.0, "getUpdates");
        assert_eq!(poll.1["timeout"], 30);
    }

    #[test]
    fn next_poll_confirms_prior_updates_with_last_update_plus_one_offset() {
        let transport = FakeTelegramTransport::with_replies(vec![
            json!({"ok":true,"result":{"id":777,"is_bot":true}}),
            json!({"ok":true,"result":[{
                "update_id":500,
                "message":{
                    "message_id":1,"date":1,
                    "chat":{"id":55,"type":"private"},
                    "from":{"id":42,"is_bot":false,"first_name":"F"},
                    "text":"one"
                }
            }]}),
            json!({"ok":true,"result":[]}),
        ]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        connector.connect_now().unwrap();
        connector.next_event_now().unwrap();
        assert_eq!(connector.next_update_offset(), Some(501));
        assert!(connector.next_event_now().unwrap().is_none());
        let calls = &connector.transport_ref().calls;
        assert_eq!(calls[2].0, "getUpdates");
        assert_eq!(calls[2].1["offset"], 501);
    }

    #[test]
    fn photo_ingress_preserves_telegram_file_identity_as_media_material() {
        let updates = json!([{
            "update_id":200,
            "message":{
                "message_id":9,"date":1,
                "chat":{"id":55,"type":"private"},
                "from":{"id":42,"is_bot":false,"first_name":"F"},
                "caption":"look",
                "photo":[
                    {"file_id":"small","file_unique_id":"u1","file_size":100},
                    {"file_id":"large","file_unique_id":"u2","file_size":1000}
                ]
            }
        }]);
        let mut connector = connected_connector(updates);
        let event = connector.next_event_now().unwrap().unwrap();
        assert_eq!(event.media.len(), 1);
        assert_eq!(event.media[0].media_ref, r("telegram-file/large"));
        assert_eq!(event.media[0].metadata["telegram_file_id"], "large");
        assert_eq!(event.text.as_deref(), Some("look"));
    }

    #[test]
    fn send_message_preserves_topic_and_reply_and_returns_delivery_receipt() {
        let transport = FakeTelegramTransport::with_replies(vec![
            json!({"ok":true,"result":{"id":777,"is_bot":true}}),
            json!({"ok":true,"result":{"message_id":99,"chat":{"id":-100123}}}),
        ]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        connector.connect_now().unwrap();
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/1"),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(),
                scope_id: Some("supergroup".into()),
                conversation_id: "-100123".into(),
                thread_id: Some("88".into()),
            },
            operation: OutboundOperationKind::Send {
                text: Some("done".into()),
                media: Vec::new(),
                reply_to_native_message_id: Some("7".into()),
            },
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: vec!["gateway".into()],
        };
        let receipt = connector.execute_now(operation).unwrap();
        assert_eq!(receipt.state, DeliveryState::Delivered);
        assert_eq!(receipt.native_message_id.as_deref(), Some("99"));
        let call = &connector.transport_ref().calls[1];
        assert_eq!(call.0, "sendMessage");
        assert_eq!(call.1["chat_id"], -100123);
        assert_eq!(call.1["message_thread_id"], 88);
        assert_eq!(call.1["reply_parameters"]["message_id"], 7);
    }

    #[test]
    fn media_delivery_uses_existing_telegram_file_id_without_fake_upload() {
        let transport = FakeTelegramTransport::with_replies(vec![
            json!({"ok":true,"result":{"id":777,"is_bot":true}}),
            json!({"ok":true,"result":{"message_id":100}}),
        ]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        connector.connect_now().unwrap();
        let media = MediaReference {
            media_ref: r("media/result-plot"),
            mime_type: Some("image/png".into()),
            file_name: Some("plot.png".into()),
            size_bytes: None,
            metadata: BTreeMap::from([("telegram_file_id".into(), json!("telegram-photo-7"))]),
        };
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/2"),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(), scope_id: None,
                conversation_id: "55".into(), thread_id: None,
            },
            operation: OutboundOperationKind::Send {
                text: Some("plot".into()), media: vec![media], reply_to_native_message_id: None,
            },
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: Vec::new(),
        };
        connector.execute_now(operation).unwrap();
        let call = &connector.transport_ref().calls[1];
        assert_eq!(call.0, "sendPhoto");
        assert_eq!(call.1["photo"], "telegram-photo-7");
        assert_eq!(call.1["caption"], "plot");
    }

    #[test]
    fn provider_failure_yields_failed_receipt_and_degraded_health() {
        let transport = FakeTelegramTransport::with_replies(vec![
            json!({"ok":true,"result":{"id":777,"is_bot":true}}),
            json!({"ok":false,"error_code":429,"description":"Too Many Requests"}),
        ]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        connector.connect_now().unwrap();
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/3"),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(), scope_id: None,
                conversation_id: "55".into(), thread_id: None,
            },
            operation: text_send("hello"),
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: Vec::new(),
        };
        let receipt = connector.execute_now(operation).unwrap();
        assert_eq!(receipt.state, DeliveryState::Failed);
        assert!(receipt.detail.unwrap().contains("429"));
        assert_eq!(connector.health_now().state, ConnectorConnectionState::Degraded);
    }

    #[test]
    fn telegram_event_flows_through_gateway_into_same_canonical_stream() {
        let updates = json!([{
            "update_id":300,
            "message":{
                "message_id":12,"date":1,
                "chat":{"id":55,"type":"private"},
                "from":{"id":42,"is_bot":false,"first_name":"Frank"},
                "text":"inspect run"
            }
        }]);
        let mut connector = connected_connector(updates);
        let event = connector.next_event_now().unwrap().unwrap();

        let mut gateway = AgencyGateway::new(r("agency-gateway/local"));
        gateway.register_connector(connector.descriptor()).unwrap();
        gateway.bind(GatewayBinding {
            binding_ref: r("gateway-binding/telegram-55"),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(), scope_id: Some("private".into()),
                conversation_id: "55".into(), thread_id: None,
            },
            agent_session_ref: r("agent-session/root"),
            agency_ref: r("agency/root"),
            actuation_ref: r("actuation/root"),
            actuation_stream_ref: r("actuation-stream/root"),
            agent_ref: Some(r("agent/root")),
            harness_ref: Some(r("harness/codex")),
            surface_ref: Some(r("surface/telegram")),
            ingress: GatewayIngressPolicy {
                default: GatewayIngressDecision::Allow,
                sender_overrides: BTreeMap::new(),
            },
            provenance: vec!["Telegram fixture".into()],
        }).unwrap();
        let result = gateway.ingest(event).unwrap();
        let GatewayIngressResult::Appended { stream_ref, event, .. } = result else {
            panic!("Telegram event should append");
        };
        assert_eq!(stream_ref, r("actuation-stream/root"));
        assert_eq!(event.event["content"], "inspect run");
        assert_eq!(event.event["surface_ref"], "surface/telegram");
        assert_eq!(event.event["metadata"]["platform"], "telegram");
    }
}
