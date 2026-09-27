//! First-party Slack connector for the AIKit Agency Gateway.
//!
//! Slack contributes provider-native channel/thread/user/message semantics
//! through the Slack Web API. The connector translates those into the public
//! gateway connector contract; the Agency Gateway remains responsible for
//! binding a Slack conversation to canonical
//! AgentSession/Agency/Actuation/ActuationStream identity.
//!
//! Platform truth for this cut, stated in one place and repeated nowhere else
//! loosely:
//!
//! - **Egress first.** `chat.postMessage`, `chat.update`, `chat.delete`,
//!   `reactions.add` ride the token directly. Slack has **no typing
//!   indicator API**, so [`ConnectorOperation::Typing`] is never advertised
//!   and a typing operation fails conformance by design.
//! - **Outbound media is absent.** Uploading bytes (`files.upload` v2) is a
//!   three-call multipart flow outside the JSON transport seam, so
//!   [`ConnectorOperation::Media`] is not advertised and a media send fails
//!   conformance. Inbound file *metadata* is still carried faithfully on
//!   ingested events.
//! - **Ingress is `conversations.history` polling** per configured channel,
//!   with a per-channel `ts` watermark. A token alone cannot push: real-time
//!   ingress requires Socket Mode (a WebSocket client — not built in this
//!   cut) or a public Events API webhook (an external prerequisite). Both are
//!   named in the connector's health detail.
//!
//! Network access is behind [`SlackBotApiTransport`]. This keeps the Web API
//! state machine fully deterministic in hosted CI while the live HTTPS body is
//! supplied independently ([`crate::slack_gateway_curl`]) without changing
//! connector semantics.

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

pub const SLACK_GATEWAY_CONNECTOR_VERSION: &str = "aikit.slack-gateway/v1";
pub const SLACK_WEB_API_BASE: &str = "https://slack.com/api";

/// Safety bound on `conversations.history` cursor pages followed inside one
/// poll when `ingest_backlog` is on; each page carries at most
/// `history_poll_limit` messages.
const MAX_HISTORY_PAGES: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackConnectorConfig {
    pub connector_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_ref: Option<ResourceRef>,
    /// Channel IDs ingested through `conversations.history` polling. Empty
    /// means no polling ingress: the connector is delivery-only.
    #[serde(default)]
    pub ingress_channels: Vec<String>,
    /// When false (the default) the first poll of a channel records its
    /// latest `ts` as the watermark and ingests nothing older; only messages
    /// arriving after the connector started are ingested. When true, the
    /// connector also pages through existing channel history (bounded by
    /// [`MAX_HISTORY_PAGES`]).
    #[serde(default)]
    pub ingest_backlog: bool,
    /// `conversations.history` `limit` per call (Slack maximum: 1000).
    #[serde(default = "default_history_poll_limit")]
    pub history_poll_limit: u32,
    #[serde(default)]
    pub provenance: Vec<String>,
}

fn default_history_poll_limit() -> u32 {
    100
}

impl SlackConnectorConfig {
    pub fn validate(&self) -> Result<()> {
        if !(1..=1000).contains(&self.history_poll_limit) {
            return Err(AikitError::new(
                "slack_gateway.history_poll_limit",
                "Slack history poll limit must be between 1 and 1000",
            ));
        }
        if self
            .ingress_channels
            .iter()
            .any(|channel| channel.trim().is_empty())
        {
            return Err(AikitError::new(
                "slack_gateway.empty_ingress_channel",
                "Slack ingress_channels cannot contain empty channel IDs",
            ));
        }
        Ok(())
    }
}

/// Minimal provider-neutral Web API execution seam.
///
/// Implementations return the normal Slack Web API response envelope
/// `{ "ok": bool, "result"?: ..., "error"?: ... }` and must never expose the
/// bot token through connector descriptors, wire frames, or renderings.
pub trait SlackBotApiTransport: Send {
    fn call(&mut self, method: &str, params: Value) -> Result<Value>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackBotIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Present for bot tokens (`xoxb-…`); this is the self-echo key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

pub struct SlackConnector<T> {
    transport: T,
    config: SlackConnectorConfig,
    descriptor: ConnectorDescriptor,
    identity: Option<SlackBotIdentity>,
    /// Per-channel latest ingested `ts`. The next poll passes it as `oldest`.
    watermarks: BTreeMap<String, String>,
    /// Resolved display names, `None` when `users.info` failed for the id.
    display_names: BTreeMap<String, Option<String>>,
    pending: VecDeque<InboundEvent>,
    health: ConnectorHealth,
}

impl<T: SlackBotApiTransport> SlackConnector<T> {
    pub fn new(transport: T, config: SlackConnectorConfig) -> Result<Self> {
        config.validate()?;
        let descriptor = ConnectorDescriptor {
            version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: config.connector_ref.clone(),
            platform: "slack".into(),
            implementation: SLACK_GATEWAY_CONNECTOR_VERSION.into(),
            capabilities: ConnectorCapabilities {
                // Truthful for a token alone: no Typing (Slack has no typing
                // API), no Media (files.upload v2 is a multipart flow outside
                // this JSON transport), Threads carries thread_ts semantics.
                operations: BTreeSet::from([
                    ConnectorOperation::Send,
                    ConnectorOperation::Edit,
                    ConnectorOperation::Delete,
                    ConnectorOperation::React,
                    ConnectorOperation::Threads,
                ]),
                max_text_bytes: Some(40_000),
                max_media_bytes: None,
                media_types: BTreeSet::new(),
                provenance: vec![
                    "Slack Web API connector (egress-first; ingress via conversations.history \
                     polling)"
                        .into(),
                    SLACK_GATEWAY_CONNECTOR_VERSION.into(),
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
            identity: None,
            watermarks: BTreeMap::new(),
            display_names: BTreeMap::new(),
            pending: VecDeque::new(),
            health: ConnectorHealth {
                connector_ref,
                state: ConnectorConnectionState::Disconnected,
                detail: None,
                provenance: vec![SLACK_GATEWAY_CONNECTOR_VERSION.into()],
            },
        })
    }

    pub fn descriptor_ref(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    pub fn identity(&self) -> Option<&SlackBotIdentity> {
        self.identity.as_ref()
    }

    pub fn watermark(&self, channel: &str) -> Option<&String> {
        self.watermarks.get(channel)
    }

    pub fn display_name(&self, user_id: &str) -> Option<&String> {
        self.display_names.get(user_id).and_then(Option::as_ref)
    }

    pub fn transport_ref(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn connect_now(&mut self) -> Result<ConnectorHello> {
        self.health.state = ConnectorConnectionState::Connecting;
        let response = self.transport.call("auth.test", json!({}));
        match response.and_then(slack_result) {
            Ok(result) => {
                let identity = SlackBotIdentity {
                    team_id: optional_string(&result, "team_id"),
                    user_id: optional_string(&result, "user_id"),
                    bot_id: optional_string(&result, "bot_id"),
                    team: optional_string(&result, "team"),
                    user: optional_string(&result, "user"),
                };
                self.identity = Some(identity);
                self.health.state = ConnectorConnectionState::Connected;
                let ingress = if self.config.ingress_channels.is_empty() {
                    "delivery-only".to_string()
                } else {
                    format!(
                        "ingress = conversations.history polling of {} configured channel(s)",
                        self.config.ingress_channels.len()
                    )
                };
                self.health.detail = Some(format!(
                    "Slack workspace {} reachable; egress-first adapter ({ingress}; real-time \
                     ingress needs Socket Mode or an Events API webhook — not built)",
                    self.identity
                        .as_ref()
                        .and_then(|identity| identity.team_id.clone())
                        .unwrap_or_else(|| "(unknown team)".into())
                ));
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
                "slack_gateway.not_connected",
                "Slack connector must be connected before polling",
            ));
        }
        let channels = self.config.ingress_channels.clone();
        for channel in channels {
            let messages = self.poll_channel(&channel)?;
            for message in &messages {
                if let Some(event) = self.slack_message_to_inbound(&channel, message)? {
                    self.pending.push_back(event);
                }
            }
        }
        self.health.state = ConnectorConnectionState::Connected;
        self.health.detail = Some(format!(
            "Slack history polling watermarked: {}",
            self.watermarks
                .iter()
                .map(|(channel, ts)| format!("{channel}@{ts}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        Ok(self.pending.pop_front())
    }

    /// One `conversations.history` poll of a channel, following cursor pages
    /// in backlog mode. Returns the messages to translate, oldest first.
    fn poll_channel(&mut self, channel: &str) -> Result<Vec<Value>> {
        let watermark = self.watermarks.get(channel).cloned();
        let mut collected = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let mut params = Map::from_iter([
                ("channel".into(), json!(channel)),
                ("limit".into(), json!(self.config.history_poll_limit)),
            ]);
            if let Some(oldest) = &watermark {
                params.insert("oldest".into(), json!(oldest));
            }
            if let Some(cursor) = &cursor {
                params.insert("cursor".into(), json!(cursor));
            }
            let response = self
                .transport
                .call("conversations.history", Value::Object(params));
            let result = match response.and_then(slack_result) {
                Ok(result) => result,
                Err(error) => {
                    self.health.state = ConnectorConnectionState::Reconnecting;
                    self.health.detail = Some(error.to_string());
                    return Err(error);
                }
            };
            let messages = result
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            collected.extend(messages);
            let has_more = result
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let next_cursor = result
                .get("response_metadata")
                .and_then(|meta| meta.get("next_cursor"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            pages += 1;
            // In backlog mode, pages continue until the provider says the
            // history is exhausted or the safety bound is hit. With a
            // watermark the `oldest` filter already bounds the pages, so a
            // single call answers the poll.
            if !self.config.ingest_backlog
                || !has_more
                || next_cursor.is_none()
                || pages >= MAX_HISTORY_PAGES
            {
                break;
            }
            cursor = next_cursor;
        }
        // Keep only real messages, oldest first; the watermark advances past
        // everything the channel showed us (ingested or filtered), so
        // latest-mode first polls record where "now" is without replaying.
        let mut ingested: Vec<&Value> = collected
            .iter()
            .filter(|message| self.is_ingestable(channel, message))
            .collect();
        ingested.sort_by_key(|message| slack_ts_key(message));
        if let Some(latest) = collected
            .iter()
            .filter_map(|message| message.get("ts").and_then(Value::as_str))
            .max_by_key(|ts| slack_ts_parse(ts))
        {
            let latest = latest.to_string();
            match &watermark {
                Some(existing) if slack_ts_parse(&latest) <= slack_ts_parse(existing) => {}
                _ => {
                    self.watermarks.insert(channel.to_owned(), latest);
                }
            }
        }
        Ok(ingested.into_iter().cloned().collect())
    }

    /// The ingestion filter: structural echoes out, self-echo out.
    fn is_ingestable(&self, channel: &str, message: &Value) -> bool {
        let ts = message.get("ts").and_then(Value::as_str);
        let ts_new_enough = match ts {
            Some(ts) => match self.watermarks.get(channel) {
                Some(watermark) => slack_ts_parse(ts) > slack_ts_parse(watermark),
                None => self.config.ingest_backlog,
            },
            None => return false,
        };
        if !ts_new_enough {
            return false;
        }
        if let (Some(bot_id), Some(identity)) = (
            message.get("bot_id").and_then(Value::as_str),
            self.identity.as_ref().and_then(|identity| identity.bot_id.as_deref()),
        ) {
            if bot_id == identity {
                return false;
            }
        }
        if let Some(subtype) = message.get("subtype").and_then(Value::as_str) {
            if matches!(subtype, "message_changed" | "message_deleted") {
                return false;
            }
        }
        true
    }

    pub fn execute_now(&mut self, operation: OutboundOperation) -> Result<DeliveryReceipt> {
        operation.validate(&self.descriptor)?;
        let (method, params) = slack_outbound_request(&operation)?;
        let result = self.transport.call(method, params).and_then(slack_result);
        match result {
            Ok(result) => {
                self.health.state = ConnectorConnectionState::Connected;
                self.health.detail = Some(format!("Slack {method} succeeded"));
                Ok(DeliveryReceipt {
                    operation_ref: operation.operation_ref,
                    connector_ref: operation.connector_ref,
                    state: DeliveryState::Delivered,
                    native_message_id: result
                        .get("ts")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    detail: None,
                    native: BTreeMap::from([
                        ("method".into(), json!(method)),
                        ("result".into(), result),
                    ]),
                    provenance: vec![
                        SLACK_GATEWAY_CONNECTOR_VERSION.into(),
                        format!("Slack Web API {method}"),
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
                    provenance: vec![SLACK_GATEWAY_CONNECTOR_VERSION.into()],
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
        self.health.detail = Some("Slack connector closed".into());
        Ok(())
    }

    fn slack_message_to_inbound(
        &mut self,
        channel: &str,
        message: &Value,
    ) -> Result<Option<InboundEvent>> {
        let Some(ts) = message.get("ts").and_then(Value::as_str) else {
            return Ok(None);
        };
        let thread_id = message
            .get("thread_ts")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let media = slack_message_media(message)?;
        let kind = if !media.is_empty() && text.is_none() {
            InboundEventKind::Media
        } else {
            InboundEventKind::Message
        };
        let sender = self.slack_sender(message);
        let mut native = BTreeMap::new();
        native.insert("message".into(), message.clone());
        native.insert("channel".into(), json!(channel));
        native.insert("carrier".into(), json!("conversations.history"));
        Ok(Some(InboundEvent {
            event_ref: ResourceRef::parse(format!("slack-history/{channel}/{ts}"))?,
            connector_ref: self.descriptor.connector_ref.clone(),
            address: ConversationAddress {
                platform: "slack".into(),
                scope_id: self
                    .identity
                    .as_ref()
                    .and_then(|identity| identity.team_id.clone()),
                conversation_id: channel.to_string(),
                thread_id,
            },
            sender,
            kind,
            custom_kind: None,
            native_event_id: Some(format!("{channel}:{ts}")),
            native_message_id: Some(ts.to_string()),
            reply_to_native_message_id: None,
            text,
            media,
            observed_at: slack_ts_parse(ts)
                .map(|(secs, _)| format!("unix:{secs}")),
            native,
            provenance: vec![
                SLACK_GATEWAY_CONNECTOR_VERSION.into(),
                format!("Slack channel {channel} ts {ts}"),
                "conversations.history poll".into(),
            ],
        }))
    }

    fn slack_sender(&mut self, message: &Value) -> SenderIdentity {
        if let Some(user_id) = message.get("user").and_then(Value::as_str) {
            let display_name = if let Some(cached) = self.display_names.get(user_id) {
                cached.clone()
            } else {
                let resolved = self.resolve_display_name(user_id);
                self.display_names.insert(user_id.to_owned(), resolved.clone());
                resolved
            };
            return SenderIdentity {
                native_sender_id: user_id.to_owned(),
                kind: SenderKind::Human,
                display_name,
                metadata: BTreeMap::new(),
            };
        }
        if let Some(bot_id) = message.get("bot_id").and_then(Value::as_str) {
            return SenderIdentity {
                native_sender_id: bot_id.to_owned(),
                kind: SenderKind::Bot,
                display_name: message
                    .get("username")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                metadata: BTreeMap::new(),
            };
        }
        SenderIdentity {
            native_sender_id: "slack-system".into(),
            kind: SenderKind::System,
            display_name: None,
            metadata: BTreeMap::new(),
        }
    }

    fn resolve_display_name(&mut self, user_id: &str) -> Option<String> {
        let response = self
            .transport
            .call("users.info", json!({"user": user_id}))
            .and_then(slack_result);
        let Ok(user) = response else {
            // A failed lookup never fails ingestion; the bare id is the name.
            return None;
        };
        ["display_name", "real_name"]
            .iter()
            .find_map(|key| {
                user.get("profile")
                    .and_then(|profile| profile.get(*key))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                user.get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
    }
}

impl<T: SlackBotApiTransport> GatewayConnector for SlackConnector<T> {
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

/// Slack Web API envelope: `{"ok": true, ...}` carries the payload inline
/// (there is no `result` wrapper); `{"ok": false, "error": "name"}` is the
/// failure form (Slack names the reason, e.g. `rate_limited`).
fn slack_result(response: Value) -> Result<Value> {
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(match response.get("result") {
            Some(result) => result.clone(),
            // Slack returns the payload inline; keep the whole body.
            None => response,
        });
    }
    let error = response
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    Err(AikitError::new(
        "slack_gateway.web_api",
        format!("Slack Web API error: {error}"),
    ))
}

/// `OutboundOperation` → Slack Web API `(method, params)`.
fn slack_outbound_request(operation: &OutboundOperation) -> Result<(&'static str, Value)> {
    let channel = operation.address.conversation_id.clone();
    // Slack threading is one dimensional: a reply to a message *is* a post in
    // that message's thread. An explicit thread_id wins; otherwise a reply
    // target names the thread to reply within.
    let thread_ts = operation.address.thread_id.clone().or_else(|| {
        match &operation.operation {
            OutboundOperationKind::Send {
                reply_to_native_message_id,
                ..
            } => reply_to_native_message_id.clone(),
            _ => None,
        }
    });
    match &operation.operation {
        OutboundOperationKind::Send {
            text,
            media,
            reply_to_native_message_id: _,
        } => {
            if !media.is_empty() {
                // Unreachable through the contract (Media is not advertised);
                // kept as an honest guard for direct callers.
                return Err(AikitError::new(
                    "slack_gateway.media_not_materialised",
                    format!(
                        "operation {} carries media; Slack file upload (files.upload v2) is not \
                         part of this connector cut",
                        operation.operation_ref
                    ),
                ));
            }
            let text = text.as_deref().ok_or_else(|| {
                AikitError::new(
                    "slack_gateway.empty_send",
                    "Slack text send requires text",
                )
            })?;
            let mut params = Map::from_iter([
                ("channel".into(), json!(channel)),
                ("text".into(), json!(text)),
            ]);
            if let Some(thread_ts) = thread_ts {
                params.insert("thread_ts".into(), json!(thread_ts));
            }
            Ok(("chat.postMessage", Value::Object(params)))
        }
        OutboundOperationKind::Edit { native_message_id, text } => Ok((
            "chat.update",
            json!({"channel": channel, "ts": native_message_id, "text": text}),
        )),
        OutboundOperationKind::Delete { native_message_id } => Ok((
            "chat.delete",
            json!({"channel": channel, "ts": native_message_id}),
        )),
        OutboundOperationKind::React { native_message_id, reaction } => {
            Ok((
                "reactions.add",
                json!({
                    "channel": channel,
                    "timestamp": native_message_id,
                    "name": slack_emoji_name(reaction),
                }),
            ))
        }
        OutboundOperationKind::Typing { .. } => {
            // Not advertised; unreachable through the contract. Slack has no
            // typing indicator API and this connector never fakes one.
            Err(AikitError::new(
                "slack_gateway.no_typing_api",
                format!(
                    "operation {} requests typing; Slack has no typing indicator API",
                    operation.operation_ref
                ),
            ))
        }
    }
}

/// Slack reaction names are emoji names without surrounding colons.
fn slack_emoji_name(reaction: &str) -> String {
    reaction
        .trim()
        .trim_start_matches(':')
        .trim_end_matches(':')
        .to_string()
}

fn slack_message_media(message: &Value) -> Result<Vec<MediaReference>> {
    let mut media = Vec::new();
    if let Some(files) = message.get("files").and_then(Value::as_array) {
        for file in files {
            let Some(id) = file.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut metadata = BTreeMap::from([("slack_file_id".into(), json!(id))]);
            for (key, native_key) in [
                ("slack_title", "title"),
                ("slack_permalink", "permalink"),
                ("slack_mode", "mode"),
            ] {
                if let Some(value) = file.get(native_key).and_then(Value::as_str) {
                    metadata.insert(key.into(), json!(value));
                }
            }
            media.push(MediaReference {
                media_ref: ResourceRef::parse(format!("slack-file/{id}"))?,
                mime_type: file
                    .get("mimetype")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                file_name: file.get("name").and_then(Value::as_str).map(str::to_owned),
                size_bytes: file.get("size").and_then(Value::as_u64),
                metadata,
            });
        }
    }
    Ok(media)
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Slack `ts` values are `"seconds.microseconds"` strings. Comparison key:
/// `(seconds, microseconds)`; unparsable timestamps sort last.
fn slack_ts_parse(ts: &str) -> Option<(u64, u64)> {
    let (secs, frac) = match ts.split_once('.') {
        Some((secs, frac)) => (secs, frac),
        None => (ts, ""),
    };
    let secs: u64 = secs.parse().ok()?;
    let mut digits = frac.to_string();
    digits.truncate(6);
    while digits.len() < 6 {
        digits.push('0');
    }
    let frac: u64 = if digits.is_empty() { 0 } else { digits.parse().ok()? };
    Some((secs, frac))
}

fn slack_ts_key(message: &Value) -> (u64, u64) {
    message
        .get("ts")
        .and_then(Value::as_str)
        .and_then(slack_ts_parse)
        .unwrap_or((u64::MAX, u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_runtime::{
        text_send, AgencyGateway, GatewayBinding, GatewayIngressDecision, GatewayIngressPolicy,
        GatewayIngressResult,
    };
    use std::collections::VecDeque;

    #[derive(Debug, Default)]
    struct FakeSlackTransport {
        replies: VecDeque<Value>,
        calls: Vec<(String, Value)>,
    }

    impl FakeSlackTransport {
        fn with_replies(replies: Vec<Value>) -> Self {
            Self {
                replies: replies.into(),
                calls: Vec::new(),
            }
        }
    }

    impl SlackBotApiTransport for FakeSlackTransport {
        fn call(&mut self, method: &str, params: Value) -> Result<Value> {
            self.calls.push((method.into(), params));
            self.replies.pop_front().ok_or_else(|| {
                AikitError::new(
                    "slack_gateway.fake_exhausted",
                    format!("fake Slack transport has no reply for {method}"),
                )
            })
        }
    }

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn config() -> SlackConnectorConfig {
        SlackConnectorConfig {
            connector_ref: r("gateway-connector/slack/main"),
            configuration_ref: Some(r("gateway-config/slack/main")),
            ingress_channels: vec!["C0123456789".into()],
            ingest_backlog: true,
            history_poll_limit: 100,
            provenance: vec!["test configuration".into()],
        }
    }

    fn auth_test_reply() -> Value {
        json!({
            "ok": true,
            "team": "O:I Agency",
            "team_id": "T0TEAM",
            "user": "gateway",
            "user_id": "U0BOTUSER",
            "bot_id": "B0BOT"
        })
    }

    fn connected_connector(replies: Vec<Value>) -> SlackConnector<FakeSlackTransport> {
        let mut all = vec![auth_test_reply()];
        all.extend(replies);
        let mut connector = SlackConnector::new(
            FakeSlackTransport::with_replies(all),
            config(),
        )
        .unwrap();
        connector.connect_now().unwrap();
        connector
    }

    #[test]
    fn connect_discovers_workspace_identity_without_disclosing_credentials() {
        let transport = FakeSlackTransport::with_replies(vec![auth_test_reply()]);
        let mut connector = SlackConnector::new(transport, config()).unwrap();
        let hello = connector.connect_now().unwrap();
        assert_eq!(connector.identity().unwrap().team_id.as_deref(), Some("T0TEAM"));
        assert_eq!(connector.identity().unwrap().bot_id.as_deref(), Some("B0BOT"));
        assert_eq!(hello.descriptor.platform, "slack");
        let encoded = serde_json::to_string(&hello).unwrap();
        assert!(!encoded.contains("token"));
        assert!(
            connector.health_now().detail.unwrap().contains("Socket Mode"),
            "health names the real-time ingress carrier"
        );
        assert_eq!(connector.transport_ref().calls[0].0, "auth.test");
    }

    #[test]
    fn history_ingestion_maps_text_thread_sender_display_name_and_watermark() {
        let replies = vec![
            // conversations.history: oldest last (Slack returns newest first).
            json!({"ok": true, "messages": [
                {
                    "type": "message",
                    "ts": "1690000002.000200",
                    "user": "U0HUMAN",
                    "text": "standalone"
                },
                {
                    "type": "message",
                    "ts": "1690000001.000100",
                    "user": "U0HUMAN",
                    "thread_ts": "1690000001.000100",
                    "text": "thread root"
                }
            ], "has_more": false, "ok": true}),
            // users.info for the sender.
            json!({"ok": true, "id": "U0HUMAN", "name": "human",
                   "profile": {"display_name": "Ada", "real_name": "Ada L"}}),
        ];
        let mut connector = connected_connector(replies);
        let first = connector.next_event_now().unwrap().unwrap();
        assert_eq!(first.address.platform, "slack");
        assert_eq!(first.address.scope_id.as_deref(), Some("T0TEAM"));
        assert_eq!(first.address.conversation_id, "C0123456789");
        assert_eq!(first.address.thread_id.as_deref(), Some("1690000001.000100"));
        assert_eq!(first.native_message_id.as_deref(), Some("1690000001.000100"));
        assert_eq!(first.sender.native_sender_id, "U0HUMAN");
        assert_eq!(first.sender.display_name.as_deref(), Some("Ada"));
        assert_eq!(first.text.as_deref(), Some("thread root"));
        assert_eq!(first.observed_at.as_deref(), Some("unix:1690000001"));
        let second = connector.next_event_now().unwrap().unwrap();
        assert_eq!(second.native_message_id.as_deref(), Some("1690000002.000200"));
        assert!(second.address.thread_id.is_none());
        assert_eq!(connector.watermark("C0123456789").unwrap(), "1690000002.000200");

        // The next poll carries the watermark as `oldest`.
        connector.transport_mut().replies.clear();
        connector.transport_mut().replies.push_back(
            json!({"ok": true, "messages": [], "has_more": false}),
        );
        connector.next_event_now().unwrap();
        let poll = connector
            .transport_ref()
            .calls
            .iter()
            .rev()
            .find(|(method, _)| method == "conversations.history")
            .unwrap();
        assert_eq!(poll.1["oldest"], "1690000002.000200");
        assert_eq!(poll.1["limit"], 100);
    }

    #[test]
    fn latest_mode_first_poll_sets_watermark_without_replaying_history() {
        let replies = vec![json!({"ok": true, "messages": [
            {"ts": "1690000005.000001", "user": "U0HUMAN", "text": "before us"}
        ], "has_more": false})];
        let mut config = config();
        config.ingest_backlog = false;
        let mut connector =
            SlackConnector::new(FakeSlackTransport::with_replies({
                let mut all = vec![auth_test_reply()];
                all.extend(replies);
                all
            }), config)
            .unwrap();
        connector.connect_now().unwrap();
        assert!(connector.next_event_now().unwrap().is_none());
        assert_eq!(connector.watermark("C0123456789").unwrap(), "1690000005.000001");
    }

    #[test]
    fn bot_id_self_echo_is_suppressed_but_human_and_peer_messages_ingest() {
        let replies = vec![
            json!({"ok": true, "messages": [
                // Our own echo: same bot_id the auth.test returned.
                {"ts": "1690000003.000003", "bot_id": "B0BOT", "text": "my own post"},
                // A peer bot: different bot_id, still ingested.
                {"ts": "1690000002.000002", "bot_id": "B0PEER", "username": "peer", "text": "peer says"},
                // A human.
                {"ts": "1690000001.000001", "user": "U0HUMAN", "text": "human says"}
            ], "has_more": false}),
            // Display-name resolution for the human message.
            json!({"ok": true, "id": "U0HUMAN", "name": "human",
                   "profile": {"display_name": "Ada"}}),
            // The follow-up poll confirms the watermark with an empty page.
            json!({"ok": true, "messages": [], "has_more": false}),
        ];
        let mut connector = connected_connector(replies);
        let first = connector.next_event_now().unwrap().unwrap();
        assert_eq!(first.native_message_id.as_deref(), Some("1690000001.000001"));
        assert_eq!(first.sender.kind, SenderKind::Human);
        let second = connector.next_event_now().unwrap().unwrap();
        assert_eq!(second.native_message_id.as_deref(), Some("1690000002.000002"));
        assert_eq!(second.sender.kind, SenderKind::Bot);
        assert!(connector.next_event_now().unwrap().is_none());
    }

    #[test]
    fn structural_subtypes_and_unwatermarked_history_are_not_ingested() {
        let replies = vec![json!({"ok": true, "messages": [
            {"ts": "1690000003.000003", "subtype": "message_changed",
             "message": {"ts": "1690000001.000001", "text": "edited"}},
            {"ts": "1690000002.000002", "subtype": "message_deleted",
             "previous_message": {"ts": "1690000001.000001"}},
            {"user": "U0HUMAN", "text": "no ts at all"}
        ], "has_more": false})];
        let mut connector = connected_connector(replies);
        assert!(connector.next_event_now().unwrap().is_none());
    }

    #[test]
    fn file_share_messages_carry_attachment_metadata() {
        let replies = vec![json!({"ok": true, "messages": [
            {
                "ts": "1690000001.000100",
                "subtype": "file_share",
                "user": "U0HUMAN",
                "text": "the plot",
                "files": [{
                    "id": "F0FILE",
                    "name": "plot.png",
                    "mimetype": "image/png",
                    "size": 4212,
                    "title": "Result plot",
                    "permalink": "https://workspace.slack.com/files/U0HUMAN/F0FILE/plot.png"
                }]
            }
        ], "has_more": false})];
        let mut connector = connected_connector(replies);
        let event = connector.next_event_now().unwrap().unwrap();
        assert_eq!(event.media.len(), 1);
        assert_eq!(event.media[0].media_ref, r("slack-file/F0FILE"));
        assert_eq!(event.media[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(event.media[0].file_name.as_deref(), Some("plot.png"));
        assert_eq!(event.media[0].size_bytes, Some(4212));
        assert_eq!(event.media[0].metadata["slack_file_id"], "F0FILE");
        assert_eq!(event.media[0].metadata["slack_title"], "Result plot");
        assert_eq!(event.text.as_deref(), Some("the plot"));
    }

    #[test]
    fn cursor_paging_follows_has_more_in_backlog_mode() {
        let replies = vec![
            json!({"ok": true, "messages": [
                {"ts": "1690000003.000003", "user": "U0HUMAN", "text": "page one newest"}
            ], "has_more": true,
              "response_metadata": {"next_cursor": "dGVzdDoxNzAwMDAwMDI="}}),
            json!({"ok": true, "messages": [
                {"ts": "1690000001.000001", "user": "U0HUMAN", "text": "page two oldest"},
                {"ts": "1690000002.000002", "user": "U0HUMAN", "text": "page two newest"}
            ], "has_more": false}),
            // The poll after the queue drains: watermark confirmed, empty page.
            // (A users.info reply sits before it: the first translated message
            // resolves its sender's display name.)
            json!({"ok": true, "id": "U0HUMAN", "name": "human",
                   "profile": {"display_name": "Ada"}}),
            json!({"ok": true, "messages": [], "has_more": false}),
        ];
        let mut connector = connected_connector(replies);
        let first = connector.next_event_now().unwrap().unwrap();
        assert_eq!(first.text.as_deref(), Some("page two oldest"));
        let mut texts = vec![first.text.unwrap()];
        while let Some(event) = connector.next_event_now().unwrap() {
            texts.push(event.text.unwrap());
        }
        assert_eq!(
            texts,
            vec!["page two oldest", "page two newest", "page one newest"]
        );
        let history_calls = connector
            .transport_ref()
            .calls
            .iter()
            .filter(|(method, _)| method == "conversations.history")
            .collect::<Vec<_>>();
        assert_eq!(
            history_calls.len(),
            3,
            "two cursor pages plus the watermark follow-up poll"
        );
        assert!(history_calls[0].1.get("cursor").is_none());
        assert_eq!(history_calls[1].1["cursor"], "dGVzdDoxNzAwMDAwMDI=");
        assert_eq!(
            history_calls[2].1["oldest"], "1690000003.000003",
            "the follow-up poll carries the advanced watermark"
        );
    }

    #[test]
    fn send_preserves_thread_and_returns_delivery_receipt() {
        let mut connector = connected_connector(vec![json!({
            "ok": true, "channel": "C0123456789", "ts": "1690000009.000900"
        })]);
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/1"),
            connector_ref: r("gateway-connector/slack/main"),
            address: ConversationAddress {
                platform: "slack".into(),
                scope_id: Some("T0TEAM".into()),
                conversation_id: "C0123456789".into(),
                thread_id: Some("1690000001.000100".into()),
            },
            operation: OutboundOperationKind::Send {
                text: Some("done".into()),
                media: Vec::new(),
                reply_to_native_message_id: None,
            },
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: vec!["gateway".into()],
        };
        let receipt = connector.execute_now(operation).unwrap();
        assert_eq!(receipt.state, DeliveryState::Delivered);
        assert_eq!(receipt.native_message_id.as_deref(), Some("1690000009.000900"));
        let call = &connector.transport_ref().calls[1];
        assert_eq!(call.0, "chat.postMessage");
        assert_eq!(call.1["channel"], "C0123456789");
        assert_eq!(call.1["thread_ts"], "1690000001.000100");
        assert_eq!(call.1["text"], "done");
    }

    #[test]
    fn reply_without_thread_maps_to_the_replied_message_thread() {
        let mut connector = connected_connector(vec![json!({
            "ok": true, "channel": "C0123456789", "ts": "1690000010.000001"
        })]);
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/2"),
            connector_ref: r("gateway-connector/slack/main"),
            address: ConversationAddress {
                platform: "slack".into(),
                scope_id: None,
                conversation_id: "C0123456789".into(),
                thread_id: None,
            },
            operation: OutboundOperationKind::Send {
                text: Some("a reply".into()),
                media: Vec::new(),
                reply_to_native_message_id: Some("1690000001.000100".into()),
            },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        };
        connector.execute_now(operation).unwrap();
        let call = &connector.transport_ref().calls[1];
        assert_eq!(call.0, "chat.postMessage");
        assert_eq!(call.1["thread_ts"], "1690000001.000100");
    }

    #[test]
    fn edit_react_and_delete_map_to_web_api_methods() {
        let mut connector = connected_connector(vec![
            json!({"ok": true, "channel": "C0123456789", "ts": "1690000009.000900"}),
            json!({"ok": true}),
            json!({"ok": true, "channel": "C0123456789", "ts": "1690000009.000900"}),
        ]);
        let address = ConversationAddress {
            platform: "slack".into(),
            scope_id: None,
            conversation_id: "C0123456789".into(),
            thread_id: None,
        };
        let edit = OutboundOperation {
            operation_ref: r("gateway-operation/edit-1"),
            connector_ref: r("gateway-connector/slack/main"),
            address: address.clone(),
            operation: OutboundOperationKind::Edit {
                native_message_id: "1690000009.000900".into(),
                text: "corrected".into(),
            },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        };
        connector.execute_now(edit).unwrap();
        assert_eq!(connector.transport_ref().calls[1].0, "chat.update");
        assert_eq!(
            connector.transport_ref().calls[1].1["ts"],
            "1690000009.000900"
        );

        let react = OutboundOperation {
            operation_ref: r("gateway-operation/react-1"),
            connector_ref: r("gateway-connector/slack/main"),
            address: address.clone(),
            operation: OutboundOperationKind::React {
                native_message_id: "1690000009.000900".into(),
                reaction: ":white_check_mark:".into(),
            },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        };
        connector.execute_now(react).unwrap();
        assert_eq!(connector.transport_ref().calls[2].0, "reactions.add");
        assert_eq!(
            connector.transport_ref().calls[2].1["name"],
            "white_check_mark",
            "surrounding colons are stripped"
        );
        assert_eq!(
            connector.transport_ref().calls[2].1["timestamp"],
            "1690000009.000900"
        );

        let delete = OutboundOperation {
            operation_ref: r("gateway-operation/delete-1"),
            connector_ref: r("gateway-connector/slack/main"),
            address,
            operation: OutboundOperationKind::Delete {
                native_message_id: "1690000009.000900".into(),
            },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        };
        connector.execute_now(delete).unwrap();
        assert_eq!(connector.transport_ref().calls[3].0, "chat.delete");
    }

    #[test]
    fn rate_limited_envelope_yields_failed_receipt_and_degraded_health() {
        let mut connector = connected_connector(vec![
            json!({"ok": false, "error": "rate_limited"}),
        ]);
        let operation = OutboundOperation {
            operation_ref: r("gateway-operation/rate-1"),
            connector_ref: r("gateway-connector/slack/main"),
            address: ConversationAddress {
                platform: "slack".into(),
                scope_id: None,
                conversation_id: "C0123456789".into(),
                thread_id: None,
            },
            operation: text_send("hello"),
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: Vec::new(),
        };
        let receipt = connector.execute_now(operation).unwrap();
        assert_eq!(receipt.state, DeliveryState::Failed);
        assert!(receipt.detail.unwrap().contains("rate_limited"));
        assert_eq!(
            connector.health_now().state,
            ConnectorConnectionState::Degraded
        );
    }

    #[test]
    fn typing_and_media_are_not_advertised_and_fail_conformance() {
        let connector = connected_connector(vec![]);
        let capabilities = &connector.descriptor_ref().capabilities;
        assert!(!capabilities.supports(ConnectorOperation::Typing));
        assert!(!capabilities.supports(ConnectorOperation::Media));
        assert!(capabilities.supports(ConnectorOperation::Send));
        assert!(capabilities.supports(ConnectorOperation::Edit));
        assert!(capabilities.supports(ConnectorOperation::Delete));
        assert!(capabilities.supports(ConnectorOperation::React));
        assert!(capabilities.supports(ConnectorOperation::Threads));

        let typing = OutboundOperation {
            operation_ref: r("gateway-operation/typing-1"),
            connector_ref: r("gateway-connector/slack/main"),
            address: ConversationAddress {
                platform: "slack".into(),
                scope_id: None,
                conversation_id: "C0123456789".into(),
                thread_id: None,
            },
            operation: OutboundOperationKind::Typing { active: true },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        };
        assert_eq!(
            typing
                .validate(connector.descriptor_ref())
                .unwrap_err()
                .code(),
            "gateway_connector.unsupported_operation"
        );

        let media_send = OutboundOperation {
            operation_ref: r("gateway-operation/media-1"),
            connector_ref: r("gateway-connector/slack/main"),
            address: ConversationAddress {
                platform: "slack".into(),
                scope_id: None,
                conversation_id: "C0123456789".into(),
                thread_id: None,
            },
            operation: OutboundOperationKind::Send {
                text: None,
                media: vec![MediaReference {
                    media_ref: r("media/plot"),
                    mime_type: Some("image/png".into()),
                    file_name: Some("plot.png".into()),
                    size_bytes: Some(10),
                    metadata: BTreeMap::new(),
                }],
                reply_to_native_message_id: None,
            },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        };
        assert_eq!(
            media_send
                .validate(connector.descriptor_ref())
                .unwrap_err()
                .code(),
            "gateway_connector.unsupported_operation"
        );
    }

    #[test]
    fn users_info_failure_falls_back_to_bare_sender_id() {
        let replies = vec![
            json!({"ok": true, "messages": [
                {"ts": "1690000001.000001", "user": "U0GONE", "text": "who am I"}
            ], "has_more": false}),
            json!({"ok": false, "error": "user_not_found"}),
        ];
        let mut connector = connected_connector(replies);
        let event = connector.next_event_now().unwrap().unwrap();
        assert_eq!(event.sender.native_sender_id, "U0GONE");
        assert!(event.sender.display_name.is_none());
        // The failure is cached: no repeated users.info per poll.
        assert_eq!(
            connector
                .transport_ref()
                .calls
                .iter()
                .filter(|(method, _)| method == "users.info")
                .count(),
            1
        );
    }

    #[test]
    fn config_refuses_bad_limits_and_empty_channels() {
        let mut config = config();
        config.history_poll_limit = 0;
        assert_eq!(
            config.validate().unwrap_err().code(),
            "slack_gateway.history_poll_limit"
        );
        config.history_poll_limit = 1001;
        assert!(config.validate().is_err());
        config.history_poll_limit = 1000;
        config.ingress_channels = vec!["  ".into()];
        assert_eq!(
            config.validate().unwrap_err().code(),
            "slack_gateway.empty_ingress_channel"
        );
        config.ingress_channels = Vec::new();
        assert!(config.validate().is_ok(), "delivery-only is a valid posture");
    }

    #[test]
    fn slack_event_flows_through_gateway_into_same_canonical_stream() {
        let replies = vec![
            json!({"ok": true, "messages": [
                {"ts": "1690000001.000001", "user": "U0HUMAN", "text": "inspect run"}
            ], "has_more": false}),
            json!({"ok": true, "id": "U0HUMAN", "name": "human",
                   "profile": {"display_name": "Ada"}}),
        ];
        let mut connector = connected_connector(replies);
        let event = connector.next_event_now().unwrap().unwrap();

        let mut gateway = AgencyGateway::new(r("agency-gateway/local"));
        gateway.register_connector(connector.descriptor()).unwrap();
        gateway
            .bind(GatewayBinding {
                binding_ref: r("gateway-binding/slack-c01"),
                connector_ref: r("gateway-connector/slack/main"),
                address: ConversationAddress {
                    platform: "slack".into(),
                    scope_id: Some("T0TEAM".into()),
                    conversation_id: "C0123456789".into(),
                    thread_id: None,
                },
                agent_session_ref: r("agent-session/root"),
                agency_ref: r("agency/root"),
                actuation_ref: r("actuation/root"),
                actuation_stream_ref: r("actuation-stream/root"),
                agent_ref: Some(r("agent/root")),
                harness_ref: Some(r("harness/codex")),
                surface_ref: Some(r("surface/slack")),
                forked_from: None,
                context_revision: 1,
                ingress: GatewayIngressPolicy {
                    default: GatewayIngressDecision::Allow,
                    sender_overrides: BTreeMap::new(),
                },
                provenance: vec!["Slack fixture".into()],
            })
            .unwrap();
        let result = gateway.ingest(event).unwrap();
        let GatewayIngressResult::Appended {
            stream_ref, event, ..
        } = result
        else {
            panic!("Slack event should append");
        };
        assert_eq!(stream_ref, r("actuation-stream/root"));
        assert_eq!(event.event["content"], "inspect run");
        assert_eq!(event.event["surface_ref"], "surface/slack");
        assert_eq!(event.event["metadata"]["platform"], "slack");
    }
}
