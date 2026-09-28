//! Live Slack Web API evidence for the first-party connector (P-grade).
//!
//! Ignored by default like the other physical tests. Run against a real bot
//! token with:
//!
//! ```sh
//! cargo test -p aikit-adapters --test slack_gateway_live -- --ignored --nocapture
//! ```
//!
//! Requires the bot token at `AIKIT_SLACK_TOKEN_LOCATION` (default
//! `~/.aikit/credentials/slack.token`, owner-only) and the proof channel ID at
//! `AIKIT_SLACK_TEST_CHANNEL` (e.g. `C0123456789`; the bot must be a member).
//! Physical Slack evidence is an owner-authorised step: this skeleton is
//! ready and inert until a token is staged. Sending is real: the proof
//! message lands in the channel. Ingress polls `conversations.history`
//! against the same channel (a token cannot push; Socket Mode / Events API
//! webhooks are the named real-time carriers, not built in this cut).

use aikit_adapters::gateway_connector::{
    ConversationAddress, DeliveryState, OutboundOperation, OutboundOperationKind,
};
use aikit_adapters::slack_gateway::{SlackConnector, SlackConnectorConfig};
use aikit_adapters::slack_gateway_curl::SlackCurlTransport;
use aikit_core::resource::ResourceRef;

fn r(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn token_location() -> String {
    format!(
        "file:{}",
        std::env::var("AIKIT_SLACK_TOKEN_LOCATION")
            .unwrap_or_else(|_| format!("{}/.aikit/credentials/slack.token", env!("HOME")))
    )
}

fn proof_operation(channel: &str) -> OutboundOperation {
    OutboundOperation {
        operation_ref: r("gateway-operation/live-slack-proof"),
        connector_ref: r("gateway-connector/slack/live-proof"),
        address: ConversationAddress {
            platform: "slack".into(),
            scope_id: None,
            conversation_id: channel.to_string(),
            thread_id: None,
        },
        operation: OutboundOperationKind::Send {
            text: Some(
                "[aikit gateway live proof] auth.test/chat.postMessage/history round trip".into(),
            ),
            media: vec![],
            reply_to_native_message_id: None,
        },
        agent_session_ref: None,
        actuation_stream_ref: None,
        provenance: vec!["live P-grade proof".into()],
    }
}

#[test]
#[ignore = "live Slack Web API: real token, real delivery (owner-authorised evidence step)"]
fn live_slack_web_api_connect_send_and_poll() {
    let transport = SlackCurlTransport::from_token_location(&token_location())
        .expect("live token must be staged at the owner-only location");
    let channel = std::env::var("AIKIT_SLACK_TEST_CHANNEL")
        .expect("AIKIT_SLACK_TEST_CHANNEL must name a channel the bot is a member of");
    let config = SlackConnectorConfig {
        connector_ref: r("gateway-connector/slack/live-proof"),
        configuration_ref: None,
        ingress_channels: vec![channel.clone()],
        ingest_backlog: false,
        history_poll_limit: 10,
        provenance: vec!["live P-grade proof".into()],
    };
    let mut connector = SlackConnector::new(transport, config).expect("valid config");

    // 1. Identity: the real workspace answers auth.test through the curl
    //    transport, and the health detail names the ingress carrier.
    let hello = connector
        .connect_now()
        .expect("auth.test must succeed against the live Web API");
    assert_eq!(hello.descriptor.platform, "slack");
    let identity = connector.identity().expect("workspace identity recorded");
    println!(
        "live auth.test: team {} ({}) bot {}",
        identity.team.as_deref().unwrap_or("(unnamed)"),
        identity.team_id.as_deref().unwrap_or("(no team id)"),
        identity
            .bot_id
            .as_deref()
            .unwrap_or("(user token, no bot id)")
    );
    println!(
        "live health: {}",
        connector.health_now().detail.unwrap_or_default()
    );

    // 2. Delivery: a real message lands in the channel.
    let receipt = connector
        .execute_now(proof_operation(&channel))
        .expect("send must execute");
    assert_eq!(receipt.state, DeliveryState::Delivered, "{receipt:?}");
    let ts = receipt.native_message_id.clone().expect("slack message ts");
    println!("live chat.postMessage: delivered ts {ts} to channel {channel}");

    // 3. Ingress truth: the history poll answers cleanly and the watermark
    //    advances (latest mode: nothing older is replayed).
    let event = connector
        .next_event_now()
        .expect("conversations.history must answer");
    match event {
        Some(event) => println!(
            "live conversations.history: received event {} from {}",
            event.event_ref, event.sender.native_sender_id
        ),
        None => println!(
            "live conversations.history: clean poll, watermark at {}",
            connector.watermark(&channel).cloned().unwrap_or_default()
        ),
    }
}
