//! Public Telegram connector surface over the provider-local Bot API adapter.
//!
//! Telegram's `sendChatAction("typing")` is a pulse with a short provider TTL.
//! There is no Telegram Bot API "cancel typing" action. The public gateway
//! semantics therefore treat `Typing { active: true }` as one provider pulse and
//! `Typing { active: false }` as stopping local refresh, with no Telegram request.
//! A caller that wants a continuous indicator can periodically refresh `active:
//! true` while the Actuation is working, as mature Telegram gateway
//! implementations do.

use std::collections::BTreeMap;

use aikit_core::Result;
use serde_json::json;

use crate::gateway_connector::{
    ConnectorDescriptor, ConnectorFuture, ConnectorHealth, ConnectorHello, DeliveryReceipt,
    DeliveryState, GatewayConnector, InboundEvent, OutboundOperation, OutboundOperationKind,
};
use crate::telegram_bot_api::TelegramConnector as BotApiTelegramConnector;

pub use crate::telegram_bot_api::{
    TelegramBotApiTransport, TelegramBotIdentity, TelegramConnectorConfig, TELEGRAM_BOT_API_BASE,
    TELEGRAM_GATEWAY_CONNECTOR_VERSION,
};

/// First-party Telegram Gateway connector.
///
/// The inner adapter owns Telegram update/delivery mechanics. This wrapper owns
/// the small provider-lifecycle translation needed by the generic gateway SDK.
pub struct TelegramConnector<T> {
    inner: BotApiTelegramConnector<T>,
}

impl<T: TelegramBotApiTransport> TelegramConnector<T> {
    pub fn new(transport: T, config: TelegramConnectorConfig) -> Result<Self> {
        Ok(Self {
            inner: BotApiTelegramConnector::new(transport, config)?,
        })
    }

    pub fn descriptor_ref(&self) -> &ConnectorDescriptor {
        self.inner.descriptor_ref()
    }

    pub fn next_update_offset(&self) -> Option<i64> {
        self.inner.next_update_offset()
    }

    pub fn bot_identity(&self) -> Option<&TelegramBotIdentity> {
        self.inner.bot_identity()
    }

    pub fn transport_ref(&self) -> &T {
        self.inner.transport_ref()
    }

    pub fn transport_mut(&mut self) -> &mut T {
        self.inner.transport_mut()
    }

    pub fn connect_now(&mut self) -> Result<ConnectorHello> {
        self.inner.connect_now()
    }

    pub fn next_event_now(&mut self) -> Result<Option<InboundEvent>> {
        self.inner.next_event_now()
    }

    pub fn execute_now(&mut self, operation: OutboundOperation) -> Result<DeliveryReceipt> {
        if matches!(
            operation.operation,
            OutboundOperationKind::Typing { active: false }
        ) {
            operation.validate(self.inner.descriptor_ref())?;
            return Ok(DeliveryReceipt {
                operation_ref: operation.operation_ref,
                connector_ref: operation.connector_ref,
                state: DeliveryState::Delivered,
                native_message_id: None,
                detail: Some(
                    "Telegram typing refresh stopped locally; provider indicator expires by TTL"
                        .into(),
                ),
                native: BTreeMap::from([(
                    "telegram_typing".into(),
                    json!({
                        "active": false,
                        "provider_request": false,
                        "reason": "stop-refresh"
                    }),
                )]),
                provenance: vec![
                    TELEGRAM_GATEWAY_CONNECTOR_VERSION.into(),
                    "Telegram sendChatAction typing pulse lifecycle".into(),
                ],
            });
        }
        self.inner.execute_now(operation)
    }

    pub fn health_now(&self) -> ConnectorHealth {
        self.inner.health_now()
    }

    pub fn disconnect_now(&mut self) -> Result<()> {
        self.inner.disconnect_now()
    }
}

impl<T: TelegramBotApiTransport> GatewayConnector for TelegramConnector<T> {
    fn descriptor(&self) -> ConnectorDescriptor {
        self.inner.descriptor()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    use aikit_core::{resource::ResourceRef, AikitError};
    use serde_json::{json, Value};

    use crate::gateway_connector::{ConversationAddress, OutboundOperation, OutboundOperationKind};

    #[derive(Debug, Default)]
    struct FakeTransport {
        replies: VecDeque<Value>,
        calls: Vec<(String, Value)>,
    }

    impl FakeTransport {
        fn with_replies(replies: Vec<Value>) -> Self {
            Self {
                replies: replies.into(),
                calls: Vec::new(),
            }
        }
    }

    impl TelegramBotApiTransport for FakeTransport {
        fn call(&mut self, method: &str, params: Value) -> Result<Value> {
            self.calls.push((method.into(), params));
            self.replies.pop_front().ok_or_else(|| {
                AikitError::new(
                    "telegram_gateway.fake_exhausted",
                    format!("no fake Telegram response for {method}"),
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
            allowed_updates: Vec::new(),
            provenance: vec!["typing lifecycle fixture".into()],
        }
    }

    fn typing(active: bool) -> OutboundOperation {
        OutboundOperation {
            operation_ref: r(if active {
                "gateway-operation/typing-on"
            } else {
                "gateway-operation/typing-off"
            }),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(),
                scope_id: Some("private".into()),
                conversation_id: "42".into(),
                thread_id: None,
            },
            operation: OutboundOperationKind::Typing { active },
            agent_session_ref: Some(r("agent-session/root")),
            actuation_stream_ref: Some(r("actuation-stream/root")),
            provenance: Vec::new(),
        }
    }

    #[test]
    fn typing_true_sends_provider_pulse_and_false_only_stops_refresh() {
        let transport = FakeTransport::with_replies(vec![
            json!({"ok":true,"result":{"id":777,"is_bot":true}}),
            json!({"ok":true,"result":true}),
        ]);
        let mut connector = TelegramConnector::new(transport, config()).unwrap();
        connector.connect_now().unwrap();

        let started = connector.execute_now(typing(true)).unwrap();
        assert_eq!(started.state, DeliveryState::Delivered);
        assert_eq!(connector.transport_ref().calls[1].0, "sendChatAction");
        assert_eq!(connector.transport_ref().calls[1].1["action"], "typing");

        let stopped = connector.execute_now(typing(false)).unwrap();
        assert_eq!(stopped.state, DeliveryState::Delivered);
        assert_eq!(connector.transport_ref().calls.len(), 2);
        assert_eq!(stopped.native["telegram_typing"]["provider_request"], false);
    }
}
