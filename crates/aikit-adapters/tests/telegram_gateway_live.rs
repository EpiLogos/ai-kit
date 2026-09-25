//! Live Telegram Bot API evidence for the first-party connector (P-grade).
//!
//! Ignored by default like the other physical tests. Run against the real
//! configured bot with:
//!
//! ```sh
//! cargo test -p aikit-adapters --test telegram_gateway_live -- --ignored --nocapture
//! ```
//!
//! Requires the bot token at `AIKIT_TELEGRAM_TOKEN_LOCATION` (default
//! `~/.aikit/credentials/telegram.token`, owner-only) and the delivery chat at
//! `AIKIT_TELEGRAM_TEST_CHAT` (default: the owner's home DM). Sending is real:
//! the proof message lands in the recipient's Telegram. Polling requires this
//! process to be the only getUpdates consumer for the bot — a running Hermes
//! gateway on the same token is refused by Telegram with a 409 conflict.

use aikit_adapters::gateway_connector::{
    ConversationAddress, DeliveryState, OutboundOperation, OutboundOperationKind,
};
use aikit_adapters::telegram_gateway::{TelegramConnector, TelegramConnectorConfig};
use aikit_adapters::telegram_gateway_curl::TelegramCurlTransport;
use aikit_core::resource::ResourceRef;

fn r(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn token_location() -> String {
    format!(
        "file:{}",
        std::env::var("AIKIT_TELEGRAM_TOKEN_LOCATION")
            .unwrap_or_else(|_| format!("{}/.aikit/credentials/telegram.token", env!("HOME")))
    )
}

fn proof_operation(chat: &str) -> OutboundOperation {
    OutboundOperation {
        operation_ref: r("gateway-operation/live-telegram-proof"),
        connector_ref: r("gateway-connector/telegram/live-proof"),
        address: ConversationAddress {
            platform: "telegram".into(),
            scope_id: None,
            conversation_id: chat.to_string(),
            thread_id: None,
        },
        operation: OutboundOperationKind::Send {
            text: Some("[aikit gateway live proof] connect/send/poll round trip".into()),
            media: vec![],
            reply_to_native_message_id: None,
        },
        agent_session_ref: None,
        actuation_stream_ref: None,
        provenance: vec!["live P-grade proof".into()],
    }
}

#[test]
#[ignore = "live Telegram Bot API: real token, real delivery, exclusive polling"]
fn live_telegram_bot_api_connect_send_and_poll() {
    let transport = TelegramCurlTransport::from_token_location(&token_location())
        .expect("live token must be staged at the owner-only location");
    let config = TelegramConnectorConfig {
        connector_ref: r("gateway-connector/telegram/live-proof"),
        configuration_ref: None,
        poll_timeout_seconds: 3,
        allowed_updates: vec![],
        provenance: vec!["live P-grade proof".into()],
    };
    let mut connector = TelegramConnector::new(transport, config).expect("valid config");

    // 1. Identity: the real bot answers getMe through the curl transport.
    let hello = connector
        .connect_now()
        .expect("getMe must succeed against the live Bot API");
    assert_eq!(hello.descriptor.platform, "telegram");
    let identity = connector.bot_identity().expect("bot identity recorded");
    assert!(identity.is_bot);
    println!(
        "live getMe: bot @{} id {}",
        identity.username.as_deref().unwrap_or("(no username)"),
        identity.id
    );

    // 2. Delivery: a real message lands in the recipient's Telegram.
    let chat = std::env::var("AIKIT_TELEGRAM_TEST_CHAT")
        .unwrap_or_else(|_| "6381957258".to_string());
    let receipt = connector
        .execute_now(proof_operation(&chat))
        .expect("send must execute");
    assert_eq!(receipt.state, DeliveryState::Delivered, "{receipt:?}");
    let message_id = receipt
        .native_message_id
        .clone()
        .expect("telegram message id");
    println!("live sendMessage: delivered message_id {message_id} to chat {chat}");

    // 3. Exclusive polling: getUpdates answers cleanly (409 here would name
    //    the competing consumer, e.g. a running Hermes gateway on this token).
    let event = connector
        .next_event_now()
        .expect("getUpdates must answer without conflict");
    match event {
        Some(event) => println!(
            "live getUpdates: received event {} from {}",
            event.event_ref, event.sender.native_sender_id
        ),
        None => println!("live getUpdates: clean empty poll, no competing consumer"),
    }
}
