//! The gateway conversation engine: real model turns, canonical control, and
//! restart-as-recovery, proven against the real `aikit` binary and against the
//! deterministic engine suite.
//!
//! The deterministic tests drive the engine directly with the scripted
//! [`FixtureTurnSource`] and a recording test connector — no harness binary is
//! required. The binary-level test proves a requested gateway restart is a
//! drained, state-preserving rematerialisation: snapshot intact, binding and
//! stream identity unchanged, connector reconnected, and a subscriber that
//! re-subscribes from its cursor misses nothing and receives nothing twice.

#[path = "support/serve_guard.rs"]
mod serve_guard;
use serve_guard::ServeGuard;

use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use aikit_adapters::{
    execute_gateway_command, parse_slash, spawn_connector_workers, AgencyGateway, CommuniqueDraft,
    CommuniqueForward, CommuniqueForwardOutcome, CommuniqueRouting, CommuniqueState,
    ConnectorCapabilities, ConnectorConnectionState, ConnectorDescriptor, ConnectorFuture,
    ConnectorHealth, ConnectorHello, ConnectorOperation, ConnectorPumpControls, ConnectorQueues,
    ConversationAddress, DeliveryReceipt, DeliveryState, FixtureTurnSource, GatewayAskRequest,
    GatewayAskRoute, GatewayAskRouter, GatewayBinding, GatewayCommand, GatewayConnector,
    GatewayConnectorEntry, GatewayConnectorFactory, GatewayIngressDecision, GatewayIngressPolicy,
    GatewayIngressResult, GatewayResponse, GatewayTurnSourceResolver, InboundEvent,
    InboundEventKind, OutboundOperation, SenderAttribution, SenderIdentity, SlashParse,
    CONNECTOR_QUIET_POLL_CODE, GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION,
};
use aikit_cli::gateway_contact::route_ask_with_owners;
use aikit_cli::gateway_owners::{
    ContactOwners, CustodyAssign, OccupancyVerdict, OwnerRefusal, OwnerUnavailable, PositionLookup,
};
use aikit_core::resource::ResourceRef;
use aikit_core::AikitError;
use aikit_store::AikitHome;
use serde_json::{json, Value};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Deterministic in-process suite
// ---------------------------------------------------------------------------

const CONNECTOR_REF: &str = "gateway-connector/fixture/main";
const BINDING_REF: &str = "gateway-binding/fixture";
const STREAM_REF: &str = "actuation-stream/fixture";

fn r(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn fixture_descriptor() -> ConnectorDescriptor {
    ConnectorDescriptor {
        version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
        connector_ref: r(CONNECTOR_REF),
        platform: "fixture".into(),
        implementation: "conversation-test".into(),
        capabilities: ConnectorCapabilities {
            operations: [ConnectorOperation::Send, ConnectorOperation::Typing]
                .into_iter()
                .collect(),
            max_text_bytes: None,
            max_media_bytes: None,
            media_types: Default::default(),
            provenance: vec!["conversation-test".into()],
        },
        configuration_ref: None,
        provenance: vec!["conversation-test".into()],
    }
}

/// The streaming variant: the connector can edit its own messages and
/// declares Streaming, so the engine lets a reply grow on it.
fn streaming_fixture_descriptor() -> ConnectorDescriptor {
    let mut descriptor = fixture_descriptor();
    descriptor
        .capabilities
        .operations
        .extend([ConnectorOperation::Edit, ConnectorOperation::Streaming]);
    descriptor
}

fn fixture_address() -> ConversationAddress {
    ConversationAddress {
        platform: "fixture".into(),
        scope_id: None,
        conversation_id: "chat-1".into(),
        thread_id: None,
    }
}

fn fixture_inbound(text: &str, id: &str) -> InboundEvent {
    InboundEvent {
        event_ref: r(&format!("gateway-ingress/fixture/{id}")),
        connector_ref: r(CONNECTOR_REF),
        address: fixture_address(),
        sender: SenderIdentity {
            native_sender_id: "fixture-user".into(),
            kind: aikit_adapters::SenderKind::Human,
            display_name: None,
            metadata: Default::default(),
        },
        kind: InboundEventKind::Message,
        custom_kind: None,
        native_event_id: Some(format!("native-{id}")),
        native_message_id: Some(format!("message-{id}")),
        reply_to_native_message_id: None,
        text: Some(text.to_owned()),
        media: Vec::new(),
        observed_at: None,
        native: Default::default(),
        provenance: vec!["conversation-test".into()],
    }
}

/// A recording test connector: emits queued inbound events, executes every
/// outbound operation handed to it, and stays connected otherwise (the
/// quiet-poll tick, exactly like the stdio wire host).
struct RecordingConnector {
    connector_ref: ResourceRef,
    inner: Arc<RecordingInner>,
    /// The descriptor the harness registered: the connector's hello must
    /// carry the same capabilities, or the pump's registration would
    /// silently replace them.
    descriptor: ConnectorDescriptor,
}

struct RecordingInner {
    events: Mutex<VecDeque<Option<InboundEvent>>>,
    executed: Mutex<Vec<OutboundOperation>>,
    connected: AtomicBool,
    /// When set, every Send is answered with a Failed receipt — the standing
    /// fault the streaming fallback must survive.
    fail_sends: AtomicBool,
}

impl RecordingInner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(VecDeque::new()),
            executed: Mutex::new(Vec::new()),
            connected: AtomicBool::new(false),
            fail_sends: AtomicBool::new(false),
        })
    }
}

impl GatewayConnector for RecordingConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        self.descriptor.clone()
    }

    fn connect(&mut self) -> ConnectorFuture<'_, ConnectorHello> {
        self.inner.connected.store(true, Ordering::SeqCst);
        let result: aikit_core::Result<ConnectorHello> = Ok(ConnectorHello {
            wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
            descriptor: self.descriptor(),
        });
        Box::pin(async move { result })
    }

    fn next_event(&mut self) -> ConnectorFuture<'_, Option<InboundEvent>> {
        let next = self.inner.events.lock().unwrap().pop_front();
        let result = match next {
            Some(Some(event)) => Ok(Some(event)),
            Some(None) => {
                self.inner.connected.store(false, Ordering::SeqCst);
                Ok(None)
            }
            None => {
                thread::sleep(Duration::from_millis(2));
                Err(aikit_core::AikitError::new(
                    CONNECTOR_QUIET_POLL_CODE,
                    "no test event queued",
                ))
            }
        };
        Box::pin(async move { result })
    }

    fn execute(&mut self, operation: OutboundOperation) -> ConnectorFuture<'_, DeliveryReceipt> {
        operation.validate(&self.descriptor()).unwrap();
        self.inner.executed.lock().unwrap().push(operation.clone());
        let name = operation_operation_name(&operation);
        let failed_send = self.inner.fail_sends.load(Ordering::SeqCst)
            && matches!(
                operation.operation,
                aikit_adapters::OutboundOperationKind::Send { .. }
            );
        let result: aikit_core::Result<DeliveryReceipt> = if failed_send {
            Ok(DeliveryReceipt {
                operation_ref: operation.operation_ref,
                connector_ref: operation.connector_ref,
                state: DeliveryState::Failed,
                native_message_id: None,
                detail: Some("the platform refused the message".into()),
                native: Default::default(),
                provenance: vec!["conversation-test".into()],
            })
        } else {
            Ok(DeliveryReceipt {
                operation_ref: operation.operation_ref.clone(),
                connector_ref: operation.connector_ref.clone(),
                state: DeliveryState::Delivered,
                native_message_id: Some(format!("test-out-{}", operation.operation_ref)),
                detail: Some(format!("test connector executed {name}")),
                native: Default::default(),
                provenance: vec!["conversation-test".into()],
            })
        };
        Box::pin(async move { result })
    }

    fn health(&mut self) -> ConnectorFuture<'_, ConnectorHealth> {
        let connected = self.inner.connected.load(Ordering::SeqCst);
        let result: aikit_core::Result<ConnectorHealth> = Ok(ConnectorHealth {
            connector_ref: self.connector_ref.clone(),
            state: if connected {
                ConnectorConnectionState::Connected
            } else {
                ConnectorConnectionState::Disconnected
            },
            detail: Some("test connector".into()),
            provenance: Vec::new(),
        });
        Box::pin(async move { result })
    }

    fn disconnect(&mut self) -> ConnectorFuture<'_, ()> {
        self.inner.connected.store(false, Ordering::SeqCst);
        let result: aikit_core::Result<()> = Ok(());
        Box::pin(async move { result })
    }
}

fn operation_operation_name(operation: &OutboundOperation) -> String {
    match &operation.operation {
        aikit_adapters::OutboundOperationKind::Send { .. } => "send".into(),
        aikit_adapters::OutboundOperationKind::Edit { .. } => "edit".into(),
        aikit_adapters::OutboundOperationKind::Delete { .. } => "delete".into(),
        aikit_adapters::OutboundOperationKind::React { .. } => "react".into(),
        aikit_adapters::OutboundOperationKind::Typing { .. } => "typing".into(),
    }
}

struct RecordingFactory {
    entry: GatewayConnectorEntry,
    descriptor: ConnectorDescriptor,
    inner: Arc<RecordingInner>,
}

impl GatewayConnectorFactory for RecordingFactory {
    fn entry(&self) -> &GatewayConnectorEntry {
        &self.entry
    }

    fn build(&self) -> aikit_core::Result<Box<dyn GatewayConnector>> {
        Ok(Box::new(RecordingConnector {
            connector_ref: r(CONNECTOR_REF),
            descriptor: self.descriptor.clone(),
            inner: Arc::clone(&self.inner),
        }))
    }
}

/// Resolves the one fixture connector ref to the scripted turn source. The
/// `backings` roster is what the canonical Harness listing discloses.
struct FixtureResolver {
    connector_ref: ResourceRef,
    source: Arc<FixtureTurnSource>,
    backings: Vec<Value>,
}

impl FixtureResolver {
    fn new(connector_ref: ResourceRef, source: Arc<FixtureTurnSource>) -> Self {
        Self {
            connector_ref,
            source,
            backings: Vec::new(),
        }
    }
}

impl GatewayTurnSourceResolver for FixtureResolver {
    fn turn_source_for(
        &self,
        connector_ref: &ResourceRef,
        _platform: &str,
    ) -> Option<Arc<dyn aikit_adapters::ConversationTurnSource>> {
        if *connector_ref == self.connector_ref {
            Some(Arc::clone(&self.source) as Arc<dyn aikit_adapters::ConversationTurnSource>)
        } else {
            None
        }
    }

    fn available_backings(&self) -> Vec<Value> {
        self.backings.clone()
    }
}

struct Harness {
    gateway: Arc<Mutex<AgencyGateway>>,
    queues: Arc<ConnectorQueues>,
    controls: Arc<ConnectorPumpControls>,
    engine: Arc<aikit_adapters::GatewayConversationEngine>,
    source: Arc<FixtureTurnSource>,
    recording: Arc<RecordingInner>,
    state_file: std::path::PathBuf,
    binding_ref: ResourceRef,
    shutdown: Arc<AtomicBool>,
    _dir: TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

impl Harness {
    fn new(policy: aikit_adapters::EnginePolicy) -> Self {
        Self::with_descriptor(policy, fixture_descriptor())
    }

    /// A harness whose connector registers `descriptor` — the non-streaming
    /// default, or the Streaming variant for the progressive-reply suite.
    fn with_descriptor(
        policy: aikit_adapters::EnginePolicy,
        descriptor: ConnectorDescriptor,
    ) -> Self {
        Self::assemble(policy, descriptor, None)
    }

    /// A harness with the scripted ask router wired behind `/ask`.
    fn with_ask_router(
        policy: aikit_adapters::EnginePolicy,
        router: Arc<FixtureAskRouter>,
    ) -> Self {
        Self::with_dyn_router(policy, router as Arc<dyn GatewayAskRouter>)
    }

    /// A harness with any ask router wired behind `/ask` — the scripted
    /// fixture, or the production resolution over fixture owners.
    fn with_dyn_router(
        policy: aikit_adapters::EnginePolicy,
        router: Arc<dyn GatewayAskRouter>,
    ) -> Self {
        Self::assemble(policy, fixture_descriptor(), Some(router))
    }

    fn assemble(
        policy: aikit_adapters::EnginePolicy,
        descriptor: ConnectorDescriptor,
        ask_router: Option<Arc<dyn GatewayAskRouter>>,
    ) -> Self {
        let dir = TempDir::new().unwrap();
        let mut gateway = AgencyGateway::new(r("agency-gateway/test"));
        gateway.register_connector(descriptor.clone()).unwrap();
        gateway
            .bind(GatewayBinding {
                binding_ref: r(BINDING_REF),
                connector_ref: r(CONNECTOR_REF),
                address: fixture_address(),
                agent_session_ref: r("agent-session/fixture"),
                agency_ref: r("agency/fixture"),
                actuation_ref: r("actuation/fixture"),
                actuation_stream_ref: r(STREAM_REF),
                agent_ref: None,
                harness_ref: None,
                surface_ref: None,
                forked_from: None,
                context_revision: 1,
                ingress: GatewayIngressPolicy {
                    default: GatewayIngressDecision::Allow,
                    sender_overrides: Default::default(),
                },
                provenance: Vec::new(),
            })
            .unwrap();
        let gateway = Arc::new(Mutex::new(gateway));
        let queues = Arc::new(ConnectorQueues::default());
        let controls = Arc::new(ConnectorPumpControls::default());
        let source = Arc::new(FixtureTurnSource::named("fixture-harness"));
        let state_file = dir.path().join("gateway.json");
        let engine = aikit_adapters::GatewayConversationEngine::new(
            Arc::clone(&gateway),
            Arc::new(aikit_adapters::SubscriptionHub::default()),
            Arc::clone(&queues),
            Arc::clone(&controls),
            Some(state_file.clone()),
            Some(Arc::new({
                let resolver = FixtureResolver::new(r(CONNECTOR_REF), Arc::clone(&source));
                FixtureResolver {
                    backings: vec![
                        json!({"id": "provider-alpha", "label": "Provider Alpha", "protocol": "acp"}),
                        json!({"id": "provider-beta", "label": "Provider Beta", "protocol": "pi-rpc"}),
                    ],
                    ..resolver
                }
            })),
            policy,
        );
        if let Some(router) = ask_router.as_ref() {
            engine.attach_ask_router(Arc::clone(router) as Arc<dyn GatewayAskRouter>);
        }
        let shutdown = Arc::new(AtomicBool::new(false));
        let harness = Self {
            gateway,
            queues,
            controls,
            engine,
            source,
            recording: RecordingInner::new(),
            state_file,
            binding_ref: r(BINDING_REF),
            shutdown: Arc::clone(&shutdown),
            _dir: dir,
        };
        // The pump that makes the outbound queue real: executes every prepared
        // operation and records the receipt through the kernel, as deployed.
        let mut workers = spawn_connector_workers(
            Arc::clone(&harness.gateway),
            shutdown,
            Some(harness.state_file.clone()),
            Arc::new(aikit_adapters::SubscriptionHub::default()),
            Arc::clone(&harness.queues),
            Arc::clone(&harness.controls),
            Some(Arc::clone(&harness.engine)),
            vec![Box::new(RecordingFactory {
                entry: GatewayConnectorEntry {
                    connector_ref: CONNECTOR_REF.into(),
                    platform: "fixture".into(),
                    implementation: "conversation-test".into(),
                    enabled: true,
                    token_location: None,
                    configuration_ref: None,
                    program: Vec::new(),
                    agent_backing: None,
                    stream_replies: true,
                    provenance: Vec::new(),
                },
                descriptor: descriptor.clone(),
                inner: Arc::clone(&harness.recording),
            })],
        );
        assert_eq!(workers.len(), 1);
        thread::spawn(move || {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        });
        harness
    }

    /// The exact ingress seam the pump uses: ingest under the kernel lock,
    /// then hand the appended event to the engine while the lock is held.
    fn admit(&self, event: InboundEvent) -> GatewayIngressResult {
        let mut kernel = self.gateway.lock().unwrap();
        let result = kernel.ingest(event).unwrap();
        if let GatewayIngressResult::Appended { event, .. } = &result {
            self.engine.appended(&kernel, event);
        }
        result
    }

    fn stream_events(&self) -> Vec<Value> {
        self.gateway
            .lock()
            .unwrap()
            .snapshot()
            .streams
            .into_iter()
            .flat_map(|stream| stream.events.into_iter().map(|event| event.event))
            .collect()
    }

    fn receipt_count(&self) -> usize {
        self.gateway.lock().unwrap().status().delivery_receipt_count
    }

    fn executed_sends(&self) -> Vec<String> {
        self.recording
            .executed
            .lock()
            .unwrap()
            .iter()
            .filter_map(|operation| match &operation.operation {
                aikit_adapters::OutboundOperationKind::Send { text, .. } => text.clone(),
                _ => None,
            })
            .collect()
    }

    fn executed_typings(&self) -> Vec<bool> {
        self.recording
            .executed
            .lock()
            .unwrap()
            .iter()
            .filter_map(|operation| match &operation.operation {
                aikit_adapters::OutboundOperationKind::Typing { active } => Some(*active),
                _ => None,
            })
            .collect()
    }

    fn wait_until(&self, what: &str, timeout: Duration, mut condition: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition(self) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let executed: Vec<_> = self
            .recording
            .executed
            .lock()
            .unwrap()
            .iter()
            .map(|operation| format!("{:?}", operation.operation))
            .collect();
        let events: Vec<_> = self
            .stream_events()
            .iter()
            .map(|event| format!("{}:{}", event["kind"], event["content"]))
            .collect();
        panic!("timed out waiting for {what}; executed {executed:?}; stream {events:?}");
    }
}

#[test]
fn an_admitted_message_runs_a_turn_answers_on_the_same_stream_and_rides_the_outbound_queue_to_a_receipt(
) {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    harness.source.script_reply("echo: hello there");

    let result = harness.admit(fixture_inbound("hello there", "m1"));
    assert!(matches!(result, GatewayIngressResult::Appended { .. }));

    harness.wait_until(
        "the turn replies, journals on the same stream, and the pump records the receipt",
        Duration::from_secs(30),
        |harness| harness.receipt_count() >= 1 && harness.stream_events().len() == 2,
    );
    // The typing receipts can satisfy the wait above; the proof here is the
    // reply Send itself reaching the connector.
    harness.wait_until(
        "the reply Send executes at the connector",
        Duration::from_secs(30),
        |harness| !harness.executed_sends().is_empty(),
    );

    let events = harness.stream_events();
    assert_eq!(events[0]["kind"], "human-message");
    assert_eq!(events[0]["content"], "hello there");
    assert_eq!(events[0]["sequence"], 1);
    // The reply is an agent turn on the SAME stream, attributed to the
    // binding's agent session — never a fabricated inbound human event.
    assert_eq!(events[1]["kind"], "agent-message");
    assert_eq!(events[1]["content"], "echo: hello there");
    assert_eq!(events[1]["sequence"], 2);
    assert_eq!(
        events[1]["metadata"]["agent_session_ref"],
        "agent-session/fixture"
    );
    assert_eq!(events[1]["metadata"]["in_reply_to_sequence"], 1);

    // The executed Send answers the inbound native message id.
    let sends = harness.executed_sends();
    assert_eq!(sends.len(), 1, "exactly one Send reached the connector");
    assert_eq!(sends[0], "echo: hello there");
    let executed = harness.recording.executed.lock().unwrap();
    let send = executed
        .iter()
        .find(|operation| {
            matches!(
                operation.operation,
                aikit_adapters::OutboundOperationKind::Send { .. }
            )
        })
        .unwrap();
    assert_eq!(
        send.agent_session_ref.as_ref().map(|r| r.as_str()),
        Some("agent-session/fixture"),
        "the Send carries the session attribution"
    );
    let reply_to = match &send.operation {
        aikit_adapters::OutboundOperationKind::Send {
            reply_to_native_message_id,
            ..
        } => reply_to_native_message_id.clone(),
        _ => panic!("expected a Send"),
    };
    assert_eq!(
        reply_to,
        Some("message-m1".to_owned()),
        "the Send answers the inbound message"
    );
    // And the recorded receipt persisted with the journal.
    let persisted = fs::read_to_string(&harness.state_file).unwrap();
    assert!(persisted.contains("echo: hello there"), "{persisted}");
    assert!(persisted.contains("agent-message"), "{persisted}");
}

// ---------------------------------------------------------------------------
// Live replies (the Streaming capability)
// ---------------------------------------------------------------------------

/// Fast, deterministic streaming timing: a 10 ms poll, a 100 ms typing
/// refresh.
fn streaming_policy() -> aikit_adapters::EnginePolicy {
    aikit_adapters::EnginePolicy {
        stream: aikit_adapters::StreamTiming {
            poll: Duration::from_millis(10),
            typing_refresh: Duration::from_millis(100),
        },
        ..aikit_adapters::EnginePolicy::default()
    }
}

fn streaming_harness() -> Harness {
    Harness::with_descriptor(streaming_policy(), streaming_fixture_descriptor())
}

#[test]
fn a_streaming_turn_delivers_each_segment_and_tool_use_as_its_own_message() {
    let harness = streaming_harness();
    harness.source.script_park();
    harness.admit(fixture_inbound("stream me", "st1"));
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| harness.source.parked_turns() == 1,
    );

    // A tool use is announced as its own short message while the turn runs,
    // and journalled once on the stream.
    harness.source.emit_tool_line("· fixture-tool");
    harness.wait_until(
        "the tool use is its own message and is journalled",
        Duration::from_secs(30),
        |harness| {
            harness.executed_sends() == vec!["· fixture-tool".to_owned()]
                && harness
                    .stream_events()
                    .iter()
                    .any(|event| event["content"] == "· fixture-tool")
        },
    );

    // The first completed segment is its own message; a second tool use and
    // a second segment follow as their own messages, in wire order.
    harness.source.emit_segment("First part.");
    harness.source.emit_tool_line("· bash");
    harness.source.emit_segment("Second part.");
    harness.wait_until(
        "every segment and tool use arrived as its own message, in order",
        Duration::from_secs(30),
        |harness| {
            harness.executed_sends()
                == vec![
                    "· fixture-tool".to_owned(),
                    "First part.".to_owned(),
                    "· bash".to_owned(),
                    "Second part.".to_owned(),
                ]
        },
    );

    // The first segment answered the inbound message; every later message
    // stands plain in the same conversation.
    let reply_tos: Vec<Option<String>> = harness
        .recording
        .executed
        .lock()
        .unwrap()
        .iter()
        .filter_map(|operation| match &operation.operation {
            aikit_adapters::OutboundOperationKind::Send {
                reply_to_native_message_id,
                ..
            } => Some(reply_to_native_message_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reply_tos,
        vec![None, Some("message-st1".to_owned()), None, None],
        "only the first segment answers the inbound message: {reply_tos:?}"
    );

    // The turn's reply equals what was delivered: the completion sends
    // nothing more, and the journal carries one agent-message per segment.
    harness.source.respond("First part.Second part.");
    harness.wait_until(
        "the turn completes with every segment journalled",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .filter(|event| event["kind"] == "agent-message")
                .count()
                == 2
        },
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        harness.executed_sends(),
        vec![
            "· fixture-tool".to_owned(),
            "First part.".to_owned(),
            "· bash".to_owned(),
            "Second part.".to_owned(),
        ],
        "a fully streamed reply is not sent again at completion: {:?}",
        harness.executed_sends()
    );
    let events = harness.stream_events();
    let replies: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "agent-message")
        .collect();
    assert_eq!(
        replies.len(),
        2,
        "one journal event per segment: {events:?}"
    );
    assert_eq!(replies[0]["content"], "First part.");
    assert_eq!(replies[1]["content"], "Second part.");
    let activity: Vec<&Value> = events
        .iter()
        .filter(|event| event["custom_kind"] == "gateway-agent/turn-activity")
        .collect();
    assert_eq!(
        activity.len(),
        2,
        "one journal event per tool use: {events:?}"
    );

    // The turn is over: typing stopped, and nothing re-activates it.
    harness.wait_until(
        "typing stopped at completion",
        Duration::from_secs(30),
        |harness| harness.executed_typings().last() == Some(&false),
    );
}

#[test]
fn a_streaming_turn_whose_source_never_streams_replies_as_one_final_send() {
    let harness = streaming_harness();
    harness.source.script_park();
    harness.admit(fixture_inbound("quiet turn", "st2"));
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| harness.source.parked_turns() == 1,
    );

    // The turn runs under a streaming connector but produces no deltas and
    // no activity: nothing is sent while it runs.
    thread::sleep(Duration::from_millis(300));
    assert!(
        harness.executed_sends().is_empty(),
        "no anchor without any visible content: {:?}",
        harness.executed_sends()
    );

    harness.source.respond("all at once");
    harness.wait_until(
        "the final reply lands and reaches the connector",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["kind"] == "agent-message")
                && harness.executed_sends() == vec!["all at once".to_owned()]
        },
    );
    let events = harness.stream_events();
    assert_eq!(
        events.last().unwrap()["content"],
        json!("all at once"),
        "{events:?}"
    );
    assert_eq!(
        harness.executed_sends(),
        vec!["all at once".to_owned()],
        "the reply lands as one final Send"
    );
}

#[test]
fn a_failed_segment_send_does_not_kill_the_turn_and_the_reply_still_lands() {
    let harness = streaming_harness();
    harness.source.script_park();
    harness.admit(fixture_inbound("stream against a failing platform", "st3"));
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| harness.source.parked_turns() == 1,
    );

    // The platform refuses the segment's Send; the turn keeps running and
    // the segment's journal record stands.
    harness.recording.fail_sends.store(true, Ordering::SeqCst);
    harness.source.emit_segment("doomed segment");
    harness.wait_until(
        "the refused segment reached the connector and the journal",
        Duration::from_secs(30),
        |harness| {
            harness.executed_sends() == vec!["doomed segment".to_owned()]
                && harness
                    .stream_events()
                    .iter()
                    .any(|event| event["content"] == "doomed segment")
        },
    );
    {
        let executed = harness.recording.executed.lock().unwrap();
        let segment_ref = executed
            .iter()
            .find(|operation| {
                matches!(
                    operation.operation,
                    aikit_adapters::OutboundOperationKind::Send { .. }
                )
            })
            .unwrap()
            .operation_ref
            .clone();
        let kernel = harness.gateway.lock().unwrap();
        let receipt = kernel.delivery_receipt(&segment_ref).cloned().unwrap();
        assert_eq!(
            receipt.state,
            aikit_adapters::DeliveryState::Failed,
            "the segment's receipt records the refusal: {receipt:?}"
        );
    }

    // The turn is still alive: a later tool line and segment are journalled
    // (the sends still fail), and the turn's own completion still lands.
    harness.source.emit_tool_line("· bash");
    harness.wait_until(
        "the turn survived the failed segment send",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["content"] == "· bash")
        },
    );

    harness.recording.fail_sends.store(false, Ordering::SeqCst);
    harness.source.respond("final answer");
    harness.wait_until(
        "the reply lands as one message and reaches the connector",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["kind"] == "agent-message")
                && harness.executed_sends().last() == Some(&"final answer".to_owned())
        },
    );
    let events = harness.stream_events();
    assert_eq!(events.last().unwrap()["content"], json!("final answer"));
    // The honest wire: the refused segment and the refused tool line were
    // both executed and recorded as Failed; because the delivered segments
    // no longer equal the reply, the completion delivered the reply as one
    // message of its own.
    assert_eq!(
        harness.executed_sends(),
        vec![
            "doomed segment".to_owned(),
            "· bash".to_owned(),
            "final answer".to_owned()
        ],
        "the refused streaming sends, then the reply as one message: {:?}",
        harness.executed_sends()
    );
}

#[test]
fn typing_begins_at_admission_before_the_turn_source_spawns() {
    let harness = streaming_harness();
    // The turn source stalls inside `prompt` — the stand-in for a harness
    // process spawning. Typing must already be with the connector while that
    // spawn is still happening, not seconds later when it returns.
    harness.source.stall_next_prompt();
    harness.admit(fixture_inbound("hello before you are ready", "st0"));

    harness.wait_until(
        "typing is live while the turn source has not even spawned",
        Duration::from_secs(30),
        |harness| {
            harness.source.prompt_stalled() && harness.executed_typings().first() == Some(&true)
        },
    );

    harness.source.release_prompt();
    // The released prompt parks (no script): wait for the turn to exist
    // before resolving it, so the resolution cannot race the registration.
    harness.wait_until(
        "the released turn registers in flight",
        Duration::from_secs(30),
        |harness| harness.source.parked_turns() == 1,
    );
    harness.source.respond("made it");
    harness.wait_until(
        "the stalled turn still completes and answers",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["content"] == "made it")
        },
    );
    // The turn's very first outbound operation was the typing pulse.
    let first = harness
        .recording
        .executed
        .lock()
        .unwrap()
        .first()
        .map(|operation| operation.operation.clone());
    assert!(
        matches!(
            first,
            Some(aikit_adapters::OutboundOperationKind::Typing { active: true })
        ),
        "the first operation of the turn is Typing(active): {first:?}"
    );
}

#[test]
fn the_typing_indicator_pulses_for_the_whole_turn_and_stops_at_completion() {
    // The non-streaming harness: the pulse law is the connector's Typing
    // capability, not a streaming privilege.
    let harness = Harness::with_descriptor(streaming_policy(), fixture_descriptor());
    harness.source.script_park();
    harness.admit(fixture_inbound("long running question", "st4"));
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| harness.source.parked_turns() == 1,
    );

    // A 1.2 s parked turn under a 100 ms refresh pulses far more than the
    // single five-second blip a fire-once indicator would give.
    thread::sleep(Duration::from_millis(1_200));
    let pulses = harness
        .executed_typings()
        .into_iter()
        .filter(|active| *active)
        .count();
    assert!(
        pulses >= 5,
        "typing refreshed across the turn: {pulses} pulse(s)"
    );

    harness.source.respond("done");
    harness.wait_until("the turn settles", Duration::from_secs(30), |harness| {
        harness
            .stream_events()
            .iter()
            .any(|event| event["kind"] == "agent-message")
    });
    // Let any in-flight loop work land; the loop is done once the reply is
    // on the stream, so nothing may type after that point.
    thread::sleep(Duration::from_millis(300));
    harness.wait_until(
        "typing stops at completion",
        Duration::from_secs(30),
        |harness| harness.executed_typings().last() == Some(&false),
    );
    let typings = harness.executed_typings();
    assert!(
        typings.last() == Some(&false),
        "typing stops at completion: {typings:?}"
    );

    // The ordering law: the reply Send is queued after the typing stop, so
    // no Typing(active) is ever delivered after the final message. After the
    // completion nothing re-activates the indicator.
    thread::sleep(Duration::from_millis(300));
    let executed = harness.recording.executed.lock().unwrap();
    let last_send = executed
        .iter()
        .rposition(|operation| {
            matches!(
                operation.operation,
                aikit_adapters::OutboundOperationKind::Send { .. }
            )
        })
        .expect("the reply Send reached the connector");
    let last_active = executed.iter().rposition(|operation| {
        matches!(
            operation.operation,
            aikit_adapters::OutboundOperationKind::Typing { active: true }
        )
    });
    assert!(
        last_active.is_none_or(|position| position < last_send),
        "no Typing(active) after the final Send: {last_active:?} vs {last_send}"
    );
    let stop = executed
        .iter()
        .position(|operation| {
            matches!(
                operation.operation,
                aikit_adapters::OutboundOperationKind::Typing { active: false }
            )
        })
        .expect("typing was stopped");
    assert!(
        stop < last_send,
        "typing stops before the final message is queued"
    );
}

#[test]
fn a_connector_that_does_not_declare_streaming_keeps_the_final_reply_only() {
    let harness = Harness::new(streaming_policy());
    harness.source.script_park();
    harness.admit(fixture_inbound("plain turn", "st5"));
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| harness.source.parked_turns() == 1,
    );

    // The connector cannot stream: whatever the turn produces while it runs,
    // nothing is sent early. Tool uses are journalled, never delivered.
    harness.source.emit_tool_line("· fixture-tool");
    harness.source.emit_segment("growing text");
    harness.wait_until(
        "the tool use is journalled although nothing may be sent",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["content"] == "· fixture-tool")
        },
    );
    thread::sleep(Duration::from_millis(300));
    assert!(
        harness.executed_sends().is_empty(),
        "a non-streaming connector receives no early delivery: {:?}",
        harness.executed_sends()
    );

    harness.source.respond("one final reply");
    harness.wait_until(
        "the final reply lands and reaches the connector",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["kind"] == "agent-message")
                && harness.executed_sends() == vec!["one final reply".to_owned()]
        },
    );
    assert_eq!(
        harness.executed_sends(),
        vec!["one final reply".to_owned()],
        "exactly the final reply, as always: {:?}",
        harness.executed_sends()
    );
    // The streamed segment was never sent on its own: the single agent
    // message on the stream is the turn's final reply, and the only journal
    // narration is the tool line.
    let events = harness.stream_events();
    let replies: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "agent-message")
        .collect();
    assert_eq!(replies.len(), 1, "exactly one reply event: {events:?}");
    assert_eq!(replies[0]["content"], "one final reply");
    let activity: Vec<&Value> = events
        .iter()
        .filter(|event| event["custom_kind"] == "gateway-agent/turn-activity")
        .collect();
    assert_eq!(
        activity.len(),
        1,
        "tool uses are journalled on a non-streaming connector: {events:?}"
    );
    assert_eq!(activity[0]["content"], "· fixture-tool");
}

#[test]
fn a_denied_or_pairing_sender_never_starts_a_turn() {
    let dir = TempDir::new().unwrap();
    let mut gateway = AgencyGateway::new(r("agency-gateway/test"));
    gateway.register_connector(fixture_descriptor()).unwrap();
    let binding = GatewayBinding {
        binding_ref: r(BINDING_REF),
        connector_ref: r(CONNECTOR_REF),
        address: fixture_address(),
        agent_session_ref: r("agent-session/fixture"),
        agency_ref: r("agency/fixture"),
        actuation_ref: r("actuation/fixture"),
        actuation_stream_ref: r(STREAM_REF),
        agent_ref: None,
        harness_ref: None,
        surface_ref: None,
        forked_from: None,
        context_revision: 1,
        ingress: GatewayIngressPolicy {
            default: GatewayIngressDecision::Pair,
            sender_overrides: [("blocked-sender".into(), GatewayIngressDecision::Deny)]
                .into_iter()
                .collect(),
        },
        provenance: Vec::new(),
    };
    let gateway = Arc::new(Mutex::new(gateway));
    let source = Arc::new(FixtureTurnSource::named("fixture-harness"));
    let engine = aikit_adapters::GatewayConversationEngine::new(
        Arc::clone(&gateway),
        Arc::new(aikit_adapters::SubscriptionHub::default()),
        Arc::new(ConnectorQueues::default()),
        Arc::new(ConnectorPumpControls::default()),
        Some(dir.path().join("gateway.json")),
        Some(Arc::new(FixtureResolver::new(
            r(CONNECTOR_REF),
            Arc::clone(&source),
        ))),
        aikit_adapters::EnginePolicy::default(),
    );
    gateway.lock().unwrap().bind(binding).unwrap();

    let pair = {
        let mut kernel = gateway.lock().unwrap();
        let result = kernel.ingest(fixture_inbound("pair me", "p1")).unwrap();
        if let GatewayIngressResult::Appended { event, .. } = &result {
            engine.appended(&kernel, event);
        }
        result
    };
    assert!(matches!(pair, GatewayIngressResult::PairingRequired { .. }));
    let denied = {
        let mut kernel = gateway.lock().unwrap();
        let result = kernel
            .ingest(fixture_inbound_for("blocked-sender", "no entry", "p2"))
            .unwrap();
        if let GatewayIngressResult::Appended { event, .. } = &result {
            engine.appended(&kernel, event);
        }
        result
    };
    assert!(matches!(denied, GatewayIngressResult::Denied { .. }));

    // Policy answered before any stream material existed and before any turn.
    thread::sleep(Duration::from_millis(100));
    let kernel = gateway.lock().unwrap();
    assert_eq!(kernel.status().stream_count, 0);
    assert_eq!(source.parked_turns(), 0);
    let sessions = kernel
        .snapshot()
        .streams
        .into_iter()
        .flat_map(|stream| stream.events)
        .count();
    assert_eq!(sessions, 0);
}

fn fixture_inbound_for(sender: &str, text: &str, id: &str) -> InboundEvent {
    let mut event = fixture_inbound(text, id);
    event.sender.native_sender_id = sender.into();
    event
}

#[test]
fn command_ingress_executes_the_canonical_operation_and_surfaces_the_result() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());

    harness.admit(fixture_inbound("/status", "c1"));
    harness.wait_until(
        "the status command surfaces its answer to the conversation",
        Duration::from_secs(30),
        |harness| !harness.executed_sends().is_empty(),
    );
    let sends = harness.executed_sends();
    assert!(
        sends
            .iter()
            .any(|text| text.contains("status:") && text.contains(BINDING_REF)),
        "the status line names the conversation: {sends:?}"
    );
    // A command is control, not a turn: no agent context was opened.
    assert_eq!(harness.source.parked_turns(), 0);

    harness.admit(fixture_inbound("/dance", "c2"));
    harness.wait_until(
        "an unknown command is answered honestly",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("unknown command /dance"))
        },
    );
}

#[test]
fn stop_interrupts_a_running_turn_and_the_surface_is_told() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    harness.source.script_park();

    harness.admit(fixture_inbound("long running question", "s1"));
    // Engine truth: wait for the turn to register in flight (parked slots
    // alone race the registration).
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| {
            let execution = harness
                .engine
                .execute(
                    harness.binding_ref.clone(),
                    aikit_adapters::GatewayConversationOperation::Status,
                )
                .unwrap();
            let GatewayResponse::Conversation { result, .. } = execution.response else {
                return false;
            };
            result["turn_in_flight"] == json!(true)
        },
    );

    let execution = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::Stop,
        )
        .unwrap();
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("stop answers with a conversation response");
    };
    assert_eq!(result["stopped"], json!(true), "{result}");
    let receipt = result["receipt"].as_str().unwrap().to_owned();
    assert!(!receipt.is_empty(), "the interruption receipt is honest");

    // The interrupted turn is journaled honestly and the surface is told.
    harness.wait_until(
        "the interruption lands on the stream and in the connector",
        Duration::from_secs(30),
        |harness| {
            harness.stream_events().len() == 2
                && harness
                    .executed_sends()
                    .iter()
                    .any(|text| text.contains("interrupted"))
        },
    );
    let events = harness.stream_events();
    assert_eq!(events[1]["kind"], "custom");
    assert_eq!(events[1]["custom_kind"], "gateway-agent/turn-failure");
    assert_eq!(
        events[1]["metadata"]["failure"]["kind"], "interrupted",
        "{:?}",
        events[1]
    );
    assert!(
        events[1]["content"]
            .as_str()
            .unwrap()
            .contains("interrupted"),
        "the failure line says what happened: {:?}",
        events[1]["content"]
    );
}

#[test]
fn canonical_new_forks_a_fresh_stream_retains_the_old_one_and_continues_the_conversation() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    harness.source.script_reply("first answer");
    harness.admit(fixture_inbound("first question", "n1"));
    harness.wait_until(
        "the first turn completes",
        Duration::from_secs(30),
        |harness| harness.stream_events().len() == 2,
    );

    let execution = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::New,
        )
        .unwrap();
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("new answers with a conversation response");
    };
    assert_eq!(result["reset"], json!(true), "{result}");
    assert_eq!(result["previous_stream_retained"], json!(true), "{result}");
    assert_eq!(result["context_revision"], json!(2), "{result}");
    assert_eq!(result["previous_stream_ref"], json!(STREAM_REF), "{result}");
    let new_stream = result["stream_ref"].as_str().unwrap().to_owned();
    assert_ne!(new_stream, STREAM_REF);
    assert_eq!(
        result["forked_from"]["at_sequence"],
        json!(2),
        "the fresh context continues from the old stream's last event: {result}"
    );
    assert_eq!(
        harness.source.resets(),
        1,
        "the source opened a fresh context"
    );

    // The old stream is retained, untouched, and the route now resolves to
    // the new generation.
    assert_eq!(harness.stream_events().len(), 2);
    let kernel = harness.gateway.lock().unwrap();
    let route = kernel
        .binding_by_route(&r(CONNECTOR_REF), &fixture_address())
        .unwrap()
        .binding_ref
        .clone();
    drop(kernel);
    assert_ne!(route, harness.binding_ref, "the binding regenerated");
    assert_eq!(route.as_str(), result["binding_ref"].as_str().unwrap());

    // The next message runs on the new generation and answers on the new
    // stream; the old stream keeps exactly its two events.
    harness.source.script_reply("second answer");
    let admitted = {
        let mut kernel = harness.gateway.lock().unwrap();
        let result = kernel
            .ingest(fixture_inbound("second question", "n2"))
            .unwrap();
        if let GatewayIngressResult::Appended { event, .. } = &result {
            harness.engine.appended(&kernel, event);
        }
        result
    };
    assert!(matches!(admitted, GatewayIngressResult::Appended { .. }));
    harness.wait_until(
        "the regenerated conversation answers on its own stream",
        Duration::from_secs(30),
        |harness| {
            harness
                .stream_events()
                .iter()
                .any(|event| event["content"] == "second answer")
        },
    );
    let kernel = harness.gateway.lock().unwrap();
    let old_stream_events = kernel
        .snapshot()
        .streams
        .into_iter()
        .find(|stream| stream.stream_ref.as_str() == STREAM_REF)
        .unwrap()
        .events
        .len();
    drop(kernel);
    assert_eq!(old_stream_events, 2, "the old stream is retained unchanged");
}

#[test]
fn restart_drains_the_in_flight_turn_persists_state_and_keeps_semantic_identity() {
    let harness = Harness::new(aikit_adapters::EnginePolicy {
        turn_grace: Duration::from_millis(200),
        interrupt_grace: Duration::from_secs(5),
        ..aikit_adapters::EnginePolicy::default()
    });
    harness.source.script_park();
    harness.admit(fixture_inbound("still thinking", "r1"));
    // Engine truth: the turn is registered in flight, so the drain will take
    // it (parked_turns alone races the registration).
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| {
            let execution = harness
                .engine
                .execute(
                    harness.binding_ref.clone(),
                    aikit_adapters::GatewayConversationOperation::Status,
                )
                .unwrap();
            let GatewayResponse::Conversation { result, .. } = execution.response else {
                return false;
            };
            result["turn_in_flight"] == json!(true)
        },
    );

    let before = identity(&harness);
    let execution = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::Restart,
        )
        .unwrap();
    assert!(execution.restart_requested, "restart stops the service");
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("restart answers with a conversation response");
    };
    assert_eq!(result["restarting"], json!(true), "{result}");
    assert_eq!(result["drain"]["resolved"], json!(0), "{result}");
    assert_eq!(result["drain"]["interrupted"], json!(1), "{result}");

    // The drain recorded the interruption honestly before persisting.
    // The bound is generous: this suite runs beside a live gateway, its
    // connectors and real harness processes on a loaded machine.
    harness.wait_until(
        "the interrupted turn is journalled",
        Duration::from_secs(90),
        |harness| harness.stream_events().len() == 2,
    );
    let events = harness.stream_events();
    assert_eq!(
        events[1]["metadata"]["failure"]["kind"], "interrupted",
        "{:?}",
        events[1]
    );

    // The persisted snapshot carries everything, and restoring it changes no
    // semantic identity: the restart is a rematerialisation, not a reset.
    let after = identity(&harness);
    assert_eq!(before, after, "semantic identity is unchanged");
    let restored = aikit_adapters::restore_gateway_state(
        AgencyGateway::new(r("agency-gateway/test")),
        Some(&harness.state_file),
    )
    .unwrap();
    assert_eq!(restored.gateway_ref().as_str(), "agency-gateway/test",);
    let restored_identity = restored.snapshot();
    assert_eq!(restored_identity.bindings.len(), 1);
    assert_eq!(
        restored_identity.bindings[0].binding_ref.as_str(),
        BINDING_REF
    );
    let restored_events: Vec<u64> = restored_identity
        .streams
        .into_iter()
        .flat_map(|stream| stream.events.into_iter().map(|event| event.sequence))
        .collect();
    assert_eq!(restored_events, vec![1, 2]);

    // Draining means no new work: a message admitted after the restart
    // request still journals (the gateway stays a contact plane), but it
    // starts no turn and no reply follows it.
    harness.admit(fixture_inbound("after restart request", "r2"));
    thread::sleep(Duration::from_millis(150));
    let events = harness.stream_events();
    assert_eq!(
        events.len(),
        3,
        "the message itself still journals on the stream"
    );
    assert_eq!(events[2]["kind"], "human-message", "{:?}", events[2]);
    assert!(
        !events.iter().any(|event| event["kind"] == "agent-message"),
        "a draining engine admits no new turn work"
    );
    assert_eq!(harness.source.parked_turns(), 0);
}

/// The semantic identity a restart must preserve: refs, not journal content.
/// The drain's honest interruption record appends an event — that is content
/// the snapshot carries forward, not an identity change.
fn identity(harness: &Harness) -> Value {
    let kernel = harness.gateway.lock().unwrap();
    let snapshot = kernel.snapshot();
    drop(kernel);
    json!({
        "gateway_ref": snapshot.gateway_ref.to_string(),
        "bindings": snapshot
            .bindings
            .iter()
            .map(|binding| {
                json!({
                    "binding_ref": binding.binding_ref.to_string(),
                    "agent_session_ref": binding.agent_session_ref.to_string(),
                    "stream_ref": binding.actuation_stream_ref.to_string(),
                    "context_revision": binding.context_revision,
                })
            })
            .collect::<Vec<_>>(),
        "streams": snapshot
            .streams
            .iter()
            .map(|stream| stream.stream_ref.to_string())
            .collect::<Vec<_>>(),
    })
}

#[test]
fn connector_pause_and_resume_stop_ingress_and_show_in_health() {
    let dir = TempDir::new().unwrap();
    let mut kernel = AgencyGateway::new(r("agency-gateway/test"));
    kernel.register_connector(fixture_descriptor()).unwrap();
    kernel
        .bind(GatewayBinding {
            binding_ref: r(BINDING_REF),
            connector_ref: r(CONNECTOR_REF),
            address: fixture_address(),
            agent_session_ref: r("agent-session/fixture"),
            agency_ref: r("agency/fixture"),
            actuation_ref: r("actuation/fixture"),
            actuation_stream_ref: r(STREAM_REF),
            agent_ref: None,
            harness_ref: None,
            surface_ref: None,
            forked_from: None,
            context_revision: 1,
            ingress: GatewayIngressPolicy {
                default: GatewayIngressDecision::Allow,
                sender_overrides: Default::default(),
            },
            provenance: Vec::new(),
        })
        .unwrap();
    let gateway = Arc::new(Mutex::new(kernel));
    let queues = Arc::new(ConnectorQueues::default());
    let controls = Arc::new(ConnectorPumpControls::default());
    let engine = aikit_adapters::GatewayConversationEngine::new(
        Arc::clone(&gateway),
        Arc::new(aikit_adapters::SubscriptionHub::default()),
        Arc::clone(&queues),
        Arc::clone(&controls),
        Some(dir.path().join("gateway.json")),
        None,
        aikit_adapters::EnginePolicy::default(),
    );
    let recording = RecordingInner::new();
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut workers = spawn_connector_workers(
        Arc::clone(&gateway),
        Arc::clone(&shutdown),
        None,
        Arc::new(aikit_adapters::SubscriptionHub::default()),
        Arc::clone(&queues),
        Arc::clone(&controls),
        Some(Arc::clone(&engine)),
        vec![Box::new(RecordingFactory {
            entry: GatewayConnectorEntry {
                connector_ref: CONNECTOR_REF.into(),
                platform: "fixture".into(),
                implementation: "conversation-test".into(),
                enabled: true,
                token_location: None,
                configuration_ref: None,
                program: Vec::new(),
                agent_backing: None,
                stream_replies: true,
                provenance: Vec::new(),
            },
            descriptor: fixture_descriptor(),
            inner: Arc::clone(&recording),
        })],
    );
    assert_eq!(workers.len(), 1);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let connected = gateway
            .lock()
            .unwrap()
            .status()
            .connector_health
            .into_iter()
            .any(|health| {
                health.connector_ref == r(CONNECTOR_REF)
                    && health.state == ConnectorConnectionState::Connected
            });
        if connected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the pump never registered and connected its connector"
        );
        thread::sleep(Duration::from_millis(10));
    }

    // Pause: queued inbound events stay unadmitted, and health says why.
    controls.set_paused(&r(CONNECTOR_REF), true);
    recording
        .events
        .lock()
        .unwrap()
        .push_back(Some(fixture_inbound("while paused", "pause-1")));
    let mut last_paused_health: Option<Option<ConnectorHealth>> = None;
    poll_until(
        "pump-recorded paused health",
        Duration::from_secs(10),
        || {
            let health = gateway
                .lock()
                .unwrap()
                .status()
                .connector_health
                .into_iter()
                .find(|health| health.connector_ref == r(CONNECTOR_REF));
            let paused = health.as_ref().is_some_and(|health| {
                health
                    .detail
                    .as_deref()
                    .unwrap_or_default()
                    .contains("paused by gateway command")
            });
            if last_paused_health.as_ref() != Some(&health) {
                eprintln!("actual paused connector health observation: {health:?}");
                last_paused_health = Some(health);
            }
            paused
        },
    );
    let paused_gateway = gateway.lock().unwrap();
    let health = paused_gateway
        .status()
        .connector_health
        .into_iter()
        .find(|health| health.connector_ref == r(CONNECTOR_REF))
        .expect("the connector reports health");
    assert!(
        health
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("paused by gateway command"),
        "health reflects the pause: {health:?}"
    );
    assert_eq!(
        paused_gateway
            .snapshot()
            .streams
            .iter()
            .map(|stream| stream.events.len())
            .sum::<usize>(),
        0,
        "the paused event has not entered any journal"
    );
    drop(paused_gateway);
    let pending = recording.events.lock().unwrap();
    assert_eq!(pending.len(), 1, "the paused event remains pending");
    assert_eq!(
        pending.front().and_then(Option::as_ref).unwrap().event_ref,
        r("gateway-ingress/fixture/pause-1")
    );
    drop(pending);

    // Resume: the held event is admitted.
    controls.set_paused(&r(CONNECTOR_REF), false);
    poll_until(
        "the resumed event's journal append",
        Duration::from_secs(10),
        || {
            gateway
                .lock()
                .unwrap()
                .snapshot()
                .streams
                .iter()
                .any(|stream| !stream.events.is_empty())
        },
    );
    let gateway = gateway.lock().unwrap();
    assert_eq!(gateway.status().stream_count, 1, "the held event landed");
    let snapshot = gateway.snapshot();
    let events = snapshot
        .streams
        .iter()
        .flat_map(|stream| &stream.events)
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1, "the held event was appended exactly once");
    assert_eq!(snapshot.streams[0].stream_ref, r(STREAM_REF));
    assert_eq!(events[0].sequence, 1);
    assert_eq!(
        events[0].event["metadata"]["connector_event_ref"],
        "gateway-ingress/fixture/pause-1"
    );
    assert_eq!(events[0].event["content"], "while paused");
    drop(gateway);
    assert!(recording.events.lock().unwrap().is_empty());

    shutdown.store(true, Ordering::SeqCst);
    for worker in workers.drain(..) {
        let _ = worker.join();
    }
}

#[test]
fn conversation_operations_round_trip_as_portable_json() {
    let command = GatewayCommand::Conversation {
        binding_ref: r(BINDING_REF),
        operation: aikit_adapters::GatewayConversationOperation::PauseConnector {
            connector_ref: Some(r(CONNECTOR_REF)),
        },
    };
    let encoded = serde_json::to_value(aikit_adapters::GatewayRequestEnvelope {
        request_id: Some("op-1".into()),
        command,
    })
    .unwrap();
    assert_eq!(encoded["command"]["type"], "conversation");
    assert_eq!(encoded["command"]["operation"]["op"], "pause-connector");
    assert_eq!(
        encoded["command"]["operation"]["connector_ref"],
        CONNECTOR_REF
    );

    // The new selector operations are portable too, and list/set spellings
    // are one operation with an optional argument.
    let model = serde_json::to_value(aikit_adapters::GatewayConversationOperation::Model {
        model: Some("fixture/haiku".into()),
    })
    .unwrap();
    assert_eq!(model["op"], "model");
    assert_eq!(model["model"], "fixture/haiku");
    let list =
        serde_json::to_value(aikit_adapters::GatewayConversationOperation::Model { model: None })
            .unwrap();
    assert_eq!(list["op"], "model");
    assert!(list.get("model").is_none(), "{list}");
    for (operation, op) in [
        (
            aikit_adapters::GatewayConversationOperation::Harness,
            "harness",
        ),
        (
            aikit_adapters::GatewayConversationOperation::Skills,
            "skills",
        ),
    ] {
        let encoded = serde_json::to_value(operation).unwrap();
        assert_eq!(encoded["op"], op, "{encoded}");
    }

    // The kernel itself refuses the operation honestly: only a running
    // service's engine executes it.
    let mut kernel = AgencyGateway::new(r("agency-gateway/test"));
    let error = execute_gateway_command(
        &mut kernel,
        GatewayCommand::Conversation {
            binding_ref: r(BINDING_REF),
            operation: aikit_adapters::GatewayConversationOperation::Status,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "agency_gateway.engine_absent", "{error}");
}

#[test]
fn model_listing_and_selection_ride_the_harness_native_seam() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    let model = |name: Option<&str>| {
        harness
            .engine
            .execute(
                harness.binding_ref.clone(),
                aikit_adapters::GatewayConversationOperation::Model {
                    model: name.map(str::to_owned),
                },
            )
            .unwrap()
    };

    // The list answers with the harness's own selector disclosure: roster and
    // current selection, deterministically.
    let execution = model(None);
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("model answers with a conversation response");
    };
    assert_eq!(
        result["controls"]["model_selection"],
        json!(true),
        "{result}"
    );
    assert_eq!(
        result["controls"]["available"],
        json!(["fixture/opus", "fixture/sonnet", "fixture/haiku"]),
        "{result}"
    );
    assert_eq!(result["controls"]["current"], json!("fixture/sonnet"));

    // Selection applies through the same native seam and answers with the
    // harness's own confirmation.
    let execution = model(Some("fixture/haiku"));
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("model set answers with a conversation response");
    };
    assert_eq!(result["model"], json!("fixture/haiku"), "{result}");
    assert_eq!(result["receipt"]["previous"], json!("fixture/sonnet"));
    assert_eq!(result["receipt"]["current"], json!("fixture/haiku"));

    // A model the harness does not advertise is refused, naming its roster.
    let error = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::Model {
                model: Some("fixture/gpt-5".into()),
            },
        )
        .err()
        .expect("an unadvertised model is refused");
    assert!(
        error.to_string().contains("fixture/opus"),
        "the refusal names the harness's models: {error}"
    );

    // And the next list reads the new state of the native selector.
    let GatewayResponse::Conversation { result, .. } = model(None).response else {
        panic!("model answers with a conversation response");
    };
    assert_eq!(result["controls"]["current"], json!("fixture/haiku"));

    // The surface alias answers through the connector too.
    harness.admit(fixture_inbound("/model", "mo1"));
    harness.wait_until(
        "the /model alias surfaces the roster",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("model: harness fixture-harness offers"))
        },
    );
    harness.admit(fixture_inbound("/model fixture/opus", "mo2"));
    harness.wait_until(
        "the /model alias selects through the seam",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("fixture/opus selected"))
        },
    );
}

#[test]
fn harness_listing_discloses_the_backing_the_providers_and_the_switch_law() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    let execution = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::Harness,
        )
        .unwrap();
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("harness answers with a conversation response");
    };
    assert_eq!(result["current"], json!("fixture-harness"), "{result}");
    let available = result["available"].as_array().unwrap();
    assert_eq!(available.len(), 2, "{result}");
    assert_eq!(available[0]["id"], json!("provider-alpha"));
    assert_eq!(available[0]["label"], json!("Provider Alpha"));
    assert_eq!(available[0]["protocol"], json!("acp"));
    assert!(
        result["switch_command"]
            .as_str()
            .unwrap()
            .contains("--agent-backing <id>"),
        "{result}"
    );
    assert!(
        result["law"]
            .as_str()
            .unwrap()
            .contains("session-replacement"),
        "{result}"
    );

    // The surface alias is the same disclosure, in one line.
    harness.admit(fixture_inbound("/harness", "ha1"));
    harness.wait_until(
        "the /harness alias surfaces backing and providers",
        Duration::from_secs(30),
        |harness| {
            harness.executed_sends().iter().any(|text| {
                text.contains("harness: backing fixture-harness")
                    && text.contains("provider-alpha")
                    && text.contains("session-replacement")
            })
        },
    );
}

#[test]
fn skills_listing_discloses_the_surface_and_names_the_invocation_law() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    let execution = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::Skills,
        )
        .unwrap();
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("skills answers with a conversation response");
    };
    let skills = result["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2, "{result}");
    let names: Vec<&str> = skills
        .iter()
        .filter_map(|skill| skill["name"].as_str())
        .collect();
    assert!(names.contains(&"fixture-arithmetic"), "{result}");
    assert!(names.contains(&"fixture-greeting"), "{result}");
    assert!(
        result["invocation"]
            .as_str()
            .unwrap()
            .contains("does not execute"),
        "{result}"
    );

    // The surface alias discloses the same surface with the invocation law.
    harness.admit(fixture_inbound("/skills", "sk1"));
    harness.wait_until(
        "the /skills alias surfaces the skill surface",
        Duration::from_secs(30),
        |harness| {
            harness.executed_sends().iter().any(|text| {
                text.contains("skills: 2 available on harness fixture-harness")
                    && text.contains("does not execute")
            })
        },
    );

    // A conversation with no agent backing is answered honestly: there is no
    // skill surface to disclose.
    let dir = TempDir::new().unwrap();
    let mut kernel = AgencyGateway::new(r("agency-gateway/test"));
    kernel.register_connector(fixture_descriptor()).unwrap();
    kernel
        .bind(GatewayBinding {
            binding_ref: r(BINDING_REF),
            connector_ref: r(CONNECTOR_REF),
            address: fixture_address(),
            agent_session_ref: r("agent-session/fixture"),
            agency_ref: r("agency/fixture"),
            actuation_ref: r("actuation/fixture"),
            actuation_stream_ref: r(STREAM_REF),
            agent_ref: None,
            harness_ref: None,
            surface_ref: None,
            forked_from: None,
            context_revision: 1,
            ingress: GatewayIngressPolicy {
                default: GatewayIngressDecision::Allow,
                sender_overrides: Default::default(),
            },
            provenance: Vec::new(),
        })
        .unwrap();
    let engine = aikit_adapters::GatewayConversationEngine::new(
        Arc::new(Mutex::new(kernel)),
        Arc::new(aikit_adapters::SubscriptionHub::default()),
        Arc::new(ConnectorQueues::default()),
        Arc::new(ConnectorPumpControls::default()),
        Some(dir.path().join("gateway.json")),
        None,
        aikit_adapters::EnginePolicy::default(),
    );
    let execution = engine
        .execute(
            r(BINDING_REF),
            aikit_adapters::GatewayConversationOperation::Skills,
        )
        .unwrap();
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!("skills answers with a conversation response");
    };
    assert_eq!(result["available"], json!(false), "{result}");
    let GatewayResponse::Conversation { result, .. } = engine
        .execute(
            r(BINDING_REF),
            aikit_adapters::GatewayConversationOperation::Model { model: None },
        )
        .unwrap()
        .response
    else {
        panic!("model answers with a conversation response");
    };
    assert_eq!(result["available"], json!(false), "{result}");
}

// ---------------------------------------------------------------------------
// The connector-originated ask (/ask → attributable Communique)
// ---------------------------------------------------------------------------

/// The asking Position the fixture ledger names for the bound conversation.
const ASKER_POSITION: &str = "central:position:project:O-I:connector-agency";
const RECIPIENT_POSITION: &str = "central:position:project:O-I:cradle-steward";

/// The scripted ask router: records every request the engine hands it,
/// answers with the next scripted route (or the default pending route derived
/// from the request itself, the way the production router composes origin
/// provenance), and scripts relay outcomes. The journal append is the
/// engine's own; this fixture only shapes the world around it.
struct FixtureAskRouter {
    requests: Mutex<Vec<GatewayAskRequest>>,
    routes: Mutex<VecDeque<Result<GatewayAskRoute, AikitError>>>,
    relays: Mutex<VecDeque<CommuniqueForwardOutcome>>,
    relayed: Mutex<Vec<String>>,
    counter: std::sync::atomic::AtomicUsize,
}

impl FixtureAskRouter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            routes: Mutex::new(VecDeque::new()),
            relays: Mutex::new(VecDeque::new()),
            relayed: Mutex::new(Vec::new()),
            counter: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn script_route(&self, route: GatewayAskRoute) {
        self.routes.lock().unwrap().push_back(Ok(route));
    }

    fn script_refusal(&self, error: AikitError) {
        self.routes.lock().unwrap().push_back(Err(error));
    }

    fn script_relay(&self, outcome: CommuniqueForwardOutcome) {
        self.relays.lock().unwrap().push_back(outcome);
    }

    fn requests(&self) -> Vec<GatewayAskRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn relayed_refs(&self) -> Vec<String> {
        self.relayed.lock().unwrap().clone()
    }

    /// The default route: verified attribution from the ledger-shaped answer
    /// the request carries, origin provenance composed into the attribution
    /// basis — the production law, scripted.
    fn pending_route(&self, ask: &GatewayAskRequest) -> GatewayAskRoute {
        let serial = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        GatewayAskRoute {
            draft: CommuniqueDraft {
                communique_ref: format!("aikit:communique:fixture-ask-{serial}"),
                from_position_ref: Some(ASKER_POSITION.into()),
                from_generation_ref: Some("actuation:generation:asker-1".into()),
                attribution: SenderAttribution::Verified,
                attribution_basis: format!(
                    "actuation occupancy list: actuation:generation:asker-1 is the current \
                     occupant of {ASKER_POSITION} and its tenure carries this conversation's \
                     agent session {}; asked from connector conversation {} on {} (binding {}, \
                     agent session {}, agency {})",
                    ask.agent_session_ref,
                    ask.conversation_id,
                    ask.platform,
                    ask.binding_ref,
                    ask.agent_session_ref,
                    ask.agency_ref
                ),
                to_position_ref: RECIPIENT_POSITION.into(),
                to_workcell_ref: None,
                to_instance: None,
                instance_hold: None,
                body: ask.message.clone(),
                sent_at_unix_ms: 1_000,
                state: CommuniqueState::Pending,
                state_basis: format!(
                    "{RECIPIENT_POSITION} is occupied by actuation:generation:recipient-1; \
                     delivered at its next turn boundary"
                ),
                reply_to: None,
                forward_to_workcell_ref: None,
                routing: None,
            },
            recipient_position_ref: RECIPIENT_POSITION.into(),
            delivery: Value::Null,
        }
    }

    /// A held route: the recipient is vacant everywhere surveyed.
    fn held_route(&self, ask: &GatewayAskRequest) -> GatewayAskRoute {
        let mut route = self.pending_route(ask);
        route.draft.state = CommuniqueState::Held;
        route.draft.state_basis =
            format!("{RECIPIENT_POSITION} is vacant on this Workcell and no declared Workcell reports an occupant; held for the next occupant");
        route.delivery = json!({
            "fact": format!("{RECIPIENT_POSITION} is vacant: Actuation on this Workcell records no current occupant, and no other Workcell is declared."),
            "consequence": "The Communique is recorded held in this gateway's journal; nothing has been delivered yet.",
            "action": "It is delivered to the next occupant that claims the Position at that occupant's first turn boundary.",
        });
        route
    }

    /// A relay route: the recipient's occupant stands on another Workcell.
    fn relay_route(&self, ask: &GatewayAskRequest, serial_hint: &str) -> GatewayAskRoute {
        let mut route = self.pending_route(ask);
        route.draft.communique_ref = format!("aikit:communique:fixture-relay-{serial_hint}");
        route.draft.to_workcell_ref = Some("workcell:omarchy".into());
        route.draft.forward_to_workcell_ref = Some("workcell:omarchy".into());
        route.draft.state_basis = format!(
            "{RECIPIENT_POSITION} has no current occupant on this Workcell; gateway \
             agency-gateway/omarchy of workcell:omarchy reports \
             actuation:generation:recipient-2 current there; relayed to that Workcell's \
             gateway, delivered at the occupant's next turn boundary there"
        );
        route.draft.routing = Some(CommuniqueRouting {
            workcell_ref: "workcell:omarchy".into(),
            gateway_ref: "agency-gateway/omarchy".into(),
            generation_ref: Some("actuation:generation:recipient-2".into()),
            basis: format!(
                "{RECIPIENT_POSITION} has no current occupant on this Workcell; gateway \
                 agency-gateway/omarchy of workcell:omarchy reports \
                 actuation:generation:recipient-2 current there"
            ),
            observed_at_unix_ms: 1_000,
        });
        route
    }
}

impl GatewayAskRouter for FixtureAskRouter {
    fn route(&self, ask: &GatewayAskRequest) -> Result<GatewayAskRoute, AikitError> {
        self.requests.lock().unwrap().push(ask.clone());
        match self.routes.lock().unwrap().pop_front() {
            Some(route) => route,
            None => Ok(self.pending_route(ask)),
        }
    }

    fn relay(
        &self,
        communique: &aikit_adapters::Communique,
        _relayed_by: &str,
    ) -> Result<CommuniqueForwardOutcome, AikitError> {
        self.relayed
            .lock()
            .unwrap()
            .push(communique.communique_ref.clone());
        Ok(self
            .relays
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(CommuniqueForwardOutcome::Forwarded {
                workcell_ref: "workcell:omarchy".into(),
                remote_gateway_ref: "agency-gateway/omarchy".into(),
                at_unix_ms: 2_000,
            }))
    }
}

/// Central and Actuation answered in memory: the authoritative agent profile
/// registry (one agency, no Position anywhere naming it) beside an empty
/// occupancy ledger. The fake owners behind the production resolution stand
/// exactly where the fixture script stands for the contact suite.
struct ResidentOwners {
    positions: Value,
    profiles: Value,
}

impl ResidentOwners {
    fn new() -> Self {
        Self {
            positions: json!({
                "schema": "central.position-listing/v1",
                "world_ref": "control:root",
                "positions": [],
                "inherited": [],
                "invalid": [],
            }),
            profiles: json!({
                "profiles": [{
                    "profile": {
                        "schema": "central.agent-profile/v1",
                        "ref": "agent-profile:anuttara",
                        "agent_ref": "agent/anuttara",
                        "role": "M0 domain agent",
                        "purpose": "Faculty #0 (proof).",
                        "revision": "r1",
                    },
                    "source_path": "Control/agents/profiles/fixture.json",
                }],
                "scope": "root",
                "source_payloads_disclosed": false,
            }),
        }
    }
}

impl ContactOwners for ResidentOwners {
    fn position_list(&self, _project: Option<&str>) -> Result<Value, OwnerUnavailable> {
        Ok(self.positions.clone())
    }

    fn agent_profiles(&self) -> Result<Value, OwnerUnavailable> {
        Ok(self.profiles.clone())
    }

    fn position_read(&self, position_ref: &str) -> Result<PositionLookup, OwnerUnavailable> {
        Ok(PositionLookup::NotFound(OwnerRefusal {
            command: "ctrl --json action run central.position.read".into(),
            code: "central.position_not_found".into(),
            fact: format!("No Position definition exists at {position_ref}."),
            consequence: "Nothing was read.".into(),
            action: "ctrl --json action run central.position.list '{}'".into(),
        }))
    }

    fn world_here(&self, _cwd: &Path) -> Result<Value, OwnerUnavailable> {
        Ok(json!({
            "schema": "central.world-here/v1",
            "local_world": {"ref": "control:root", "root": "/fixture"},
            "project_world": {"state": "absent"},
            "workcells": [],
        }))
    }

    fn occupancy_list(&self) -> Result<Value, OwnerUnavailable> {
        Ok(json!({
            "schema": "actuation.position-occupancy-listing/v1",
            "store": "proof",
            "positions": [],
            "invalid": [],
        }))
    }

    fn occupancy_read(&self, position_ref: &str) -> Result<Value, OwnerUnavailable> {
        Ok(json!({
            "schema": "actuation.position-occupancy/v1",
            "position_ref": position_ref,
            "state": "vacant",
            "generations": [],
        }))
    }

    fn occupancy_verify(
        &self,
        position_ref: &str,
        generation_ref: &str,
    ) -> Result<OccupancyVerdict, OwnerUnavailable> {
        Ok(OccupancyVerdict::Refused(OwnerRefusal {
            command: "actuation occupancy verify".into(),
            code: "occupancy.unknown_generation".into(),
            fact: format!("{generation_ref} never held {position_ref}."),
            consequence: "Nothing was verified.".into(),
            action: "actuation occupancy read".into(),
        }))
    }

    fn current_work(&self, _position_ref: &str, _cwd: &Path) -> Result<Value, OwnerUnavailable> {
        Ok(json!({
            "schema": "factory.current-work/v1",
            "outcome": "none",
            "candidates": [],
        }))
    }

    fn custody_assign(
        &self,
        _request: &CustodyAssign,
        _cwd: &Path,
    ) -> Result<Result<Value, OwnerRefusal>, OwnerUnavailable> {
        Err(OwnerUnavailable {
            command: "factory development custody assign".into(),
            reason: "no custody is assigned in this proof".into(),
        })
    }
}

/// The production ask resolution the service wires, over the resident owners:
/// the same `route_ask_with_owners` laws, deterministic.
struct ProductionResolution {
    home: AikitHome,
    owners: Arc<ResidentOwners>,
}

impl GatewayAskRouter for ProductionResolution {
    fn route(&self, ask: &GatewayAskRequest) -> Result<GatewayAskRoute, AikitError> {
        route_ask_with_owners(&self.home, self.owners.as_ref(), Path::new("/fixture"), ask)
    }

    fn relay(
        &self,
        _communique: &aikit_adapters::Communique,
        _relayed_by: &str,
    ) -> Result<CommuniqueForwardOutcome, AikitError> {
        Ok(CommuniqueForwardOutcome::Failed {
            workcell_ref: "workcell:proof".into(),
            error: "no relay is exercised in this proof".into(),
            at_unix_ms: 0,
        })
    }
}

fn ask_engine(harness: &Harness, position: &str, message: &str) -> GatewayResponse {
    harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::AskPosition {
                position: position.into(),
                message: message.into(),
            },
        )
        .unwrap()
        .response
}

#[test]
fn the_ask_edge_parses_the_whole_line_into_one_canonical_operation() {
    match parse_slash("/ask @steward does the eastern gate read green?") {
        SlashParse::Operation(aikit_adapters::GatewayConversationOperation::AskPosition {
            position,
            message,
        }) => {
            assert_eq!(position, "@steward");
            assert_eq!(message, "does the eastern gate read green?");
        }
        other => panic!("the ask edge parses into AskPosition, got {other:?}"),
    }
    // The full Position ref is a recipient too, and the message keeps its
    // interior whitespace.
    match parse_slash("/ask central:position:project:O-I:steward   one  two ") {
        SlashParse::Operation(aikit_adapters::GatewayConversationOperation::AskPosition {
            position,
            message,
        }) => {
            assert_eq!(position, "central:position:project:O-I:steward");
            assert_eq!(message, "one  two");
        }
        other => panic!("the ask edge parses into AskPosition, got {other:?}"),
    }
    for (spelling, fragment) in [
        ("/ask", "usage: /ask"),
        ("/ask @steward", "needs a message"),
        ("/ask @steward   ", "needs a message"),
    ] {
        match parse_slash(spelling) {
            SlashParse::Unknown(line) => {
                assert!(line.contains(fragment), "{spelling}: {line}")
            }
            other => panic!("{spelling} is refused as usage, got {other:?}"),
        }
    }
    // The canonical operation is portable, spelled ask-position.
    let encoded = serde_json::to_value(aikit_adapters::GatewayConversationOperation::AskPosition {
        position: "@steward".into(),
        message: "hello".into(),
    })
    .unwrap();
    assert_eq!(encoded["op"], "ask-position");
    assert_eq!(encoded["position"], "@steward");
    assert_eq!(encoded["message"], "hello");
}

#[test]
fn an_admitted_ask_appends_an_attributed_communique_with_origin_provenance_and_answers_the_chat() {
    let router = FixtureAskRouter::new();
    let harness = Harness::with_ask_router(aikit_adapters::EnginePolicy::default(), router.clone());

    harness.admit(fixture_inbound(
        &format!("/ask {RECIPIENT_POSITION} does the eastern gate read green?"),
        "a1",
    ));
    harness.wait_until(
        "the ask appends its Communique and answers the chat",
        Duration::from_secs(30),
        |harness| !harness.executed_sends().is_empty(),
    );

    // The canonical operation behind the slash edge received exactly what the
    // conversation carries: the binding's semantic identity and provenance.
    let requests = router.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].binding_ref.as_str(), BINDING_REF);
    assert_eq!(requests[0].agent_session_ref, "agent-session/fixture");
    assert_eq!(requests[0].agency_ref, "agency/fixture");
    assert_eq!(requests[0].platform, "fixture");
    assert_eq!(requests[0].conversation_id, "chat-1");
    assert_eq!(requests[0].recipient, RECIPIENT_POSITION);
    assert_eq!(requests[0].message, "does the eastern gate read green?");

    // The journal holds one attributable Communique; the origin provenance —
    // asking agent session, binding, connector conversation — rides its
    // attribution basis, never the body.
    let records: Vec<aikit_adapters::Communique> = harness
        .gateway
        .lock()
        .unwrap()
        .communiques()
        .records()
        .to_vec();
    assert_eq!(records.len(), 1, "{:?}", harness.executed_sends());
    let record = &records[0];
    assert_eq!(record.to_position_ref, RECIPIENT_POSITION);
    assert_eq!(
        record.from_position_ref.as_deref(),
        Some(ASKER_POSITION),
        "the ask is attributed to the asking agency's Position"
    );
    assert_eq!(record.attribution, SenderAttribution::Verified);
    assert_eq!(record.state, CommuniqueState::Pending);
    assert_eq!(record.body, "does the eastern gate read green?");
    assert!(
        record.attribution_basis.contains("agent-session/fixture"),
        "the basis names the asking agent session: {}",
        record.attribution_basis
    );
    assert!(
        record.attribution_basis.contains(BINDING_REF),
        "the basis names the binding: {}",
        record.attribution_basis
    );
    assert!(
        record.attribution_basis.contains("chat-1"),
        "the basis names the connector conversation: {}",
        record.attribution_basis
    );

    // The chat's answer mirrors `gateway send`'s pending outcome honestly.
    let sends = harness.executed_sends();
    assert!(
        sends.iter().any(|text| text.contains("ask:")
            && text.contains("queued for")
            && text.contains(RECIPIENT_POSITION)
            && text.contains(&record.communique_ref)),
        "the ask answers with the queued outcome and its ref: {sends:?}"
    );
    // An ask is control, not a turn: no agent context was opened.
    assert_eq!(harness.source.parked_turns(), 0);

    // The canonical operation answers a carrier with the same facts.
    let GatewayResponse::Conversation { result, .. } =
        ask_engine(&harness, "@steward", "and the western one?")
    else {
        panic!("ask answers with a conversation response");
    };
    assert_eq!(result["ask"], json!(true), "{result}");
    assert_eq!(result["state"], json!("pending"), "{result}");
    assert_eq!(
        result["to_position_ref"],
        json!(RECIPIENT_POSITION),
        "{result}"
    );
    assert_eq!(result["attribution"], json!("verified"), "{result}");
    let records = harness
        .gateway
        .lock()
        .unwrap()
        .communiques()
        .records()
        .to_vec();
    assert_eq!(records.len(), 2);
    assert_eq!(result["communique_ref"], json!(records[1].communique_ref));
    assert_eq!(records[1].body, "and the western one?");
}

#[test]
fn an_ask_to_an_unknown_position_is_refused_and_nothing_is_recorded() {
    let router = FixtureAskRouter::new();
    router.script_refusal(AikitError::new(
        "gateway.unknown_position",
        "No Position with handle @nobody is defined in this World. Nothing was sent; no \
         Communique was recorded. List the Positions and their handles with `aikit gateway \
         who --json`.",
    ));
    let harness = Harness::with_ask_router(aikit_adapters::EnginePolicy::default(), router);

    harness.admit(fixture_inbound("/ask @nobody are you there?", "a2"));
    harness.wait_until(
        "the refusal surfaces with the named remedy",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("No Position with handle @nobody"))
        },
    );

    // Nothing was recorded and no turn was started.
    assert_eq!(harness.gateway.lock().unwrap().communiques().len(), 0);
    assert_eq!(harness.source.parked_turns(), 0);
    // A canonical carrier ask is refused the same way, before any append.
    let router = FixtureAskRouter::new();
    router.script_refusal(AikitError::new(
        "gateway.unknown_position",
        "No Position with handle @nobody is defined in this World. Nothing was sent.",
    ));
    let harness = Harness::with_ask_router(aikit_adapters::EnginePolicy::default(), router);
    let error = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::AskPosition {
                position: "@nobody".into(),
                message: "are you there?".into(),
            },
        )
        .err()
        .expect("the refused ask is an error");
    assert_eq!(error.code(), "gateway.unknown_position", "{error}");
    assert_eq!(harness.gateway.lock().unwrap().communiques().len(), 0);
}

#[test]
fn a_held_ask_answers_with_the_vacancy_notice() {
    let router = FixtureAskRouter::new();
    let harness = Harness::with_ask_router(aikit_adapters::EnginePolicy::default(), router.clone());
    router.script_route(router.held_route(&canned_ask("anyone home?")));

    let held = harness.admit(fixture_inbound(
        &format!("/ask {RECIPIENT_POSITION} anyone home?"),
        "a3",
    ));
    assert!(matches!(held, GatewayIngressResult::Appended { .. }));
    harness.wait_until(
        "the held ask answers the chat with the vacancy notice",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("held") && text.contains("vacant"))
        },
    );
    let records = harness
        .gateway
        .lock()
        .unwrap()
        .communiques()
        .records()
        .to_vec();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, CommuniqueState::Held);
    let sends = harness.executed_sends();
    assert!(
        sends
            .iter()
            .any(|text| text.contains("no other Workcell is declared")),
        "the answer carries the vacancy fact: {sends:?}"
    );
}

#[test]
fn an_ask_to_a_registered_profile_without_a_position_holds_at_the_agent_address() {
    let dir = TempDir::new().unwrap();
    let router = Arc::new(ProductionResolution {
        home: AikitHome::at(dir.path()),
        owners: Arc::new(ResidentOwners::new()),
    });
    let harness = Harness::with_dyn_router(aikit_adapters::EnginePolicy::default(), router);

    harness.admit(fixture_inbound(
        "/ask @anuttara are you embodied anywhere?",
        "a7",
    ));
    harness.wait_until(
        "the ask to the Agent address appends its held Communique and answers the chat",
        Duration::from_secs(30),
        |harness| !harness.executed_sends().is_empty(),
    );

    // The profile supplies an Agent address. The journal retains the mail
    // there without inferring native Agency, occupancy or admission from
    // that source; the held address stays recoverable.
    let records = harness
        .gateway
        .lock()
        .unwrap()
        .communiques()
        .records()
        .to_vec();
    assert_eq!(records.len(), 1, "{:?}", harness.executed_sends());
    let record = &records[0];
    assert_eq!(record.to_position_ref, "agent/anuttara");
    assert_eq!(record.state, CommuniqueState::Held);
    let basis = &record.transitions[0].basis;
    assert!(
        basis.contains("Central AgentProfile with no Position")
            && basis.contains("held for the Agent address")
            && basis.contains("native identity and Agency admission are not established"),
        "{basis}"
    );
    // Origin provenance still rides the attribution basis.
    assert!(
        record.attribution_basis.contains(BINDING_REF),
        "{}",
        record.attribution_basis
    );
    assert!(
        record.attribution_basis.contains("chat-1"),
        "{}",
        record.attribution_basis
    );
    assert!(
        record.attribution_basis.contains("agent-session/fixture"),
        "{}",
        record.attribution_basis
    );

    // The notice reports held mail and the source's lack of Agency proof.
    let sends = harness.executed_sends();
    assert!(
        sends.iter().any(|text| {
            text.contains("held at the same Agent address")
                && text.contains("no native Agency or occupancy proof")
                && text.contains("nothing has been delivered")
        }),
        "the answer retains the held Agent address without claiming delivery: {sends:?}"
    );
    assert_eq!(harness.source.parked_turns(), 0);

    // A handle the registry does not name either is refused as unknown by the
    // production resolution, before anything is recorded.
    let router = Arc::new(ProductionResolution {
        home: AikitHome::at(dir.path()),
        owners: Arc::new(ResidentOwners::new()),
    });
    let error = router
        .route(&canned_ask_to("@notregistered", "anyone?"))
        .expect_err("an unknown handle is refused");
    assert_eq!(error.code(), "gateway.unknown_position", "{error}");
}

#[test]
fn an_ask_relays_across_workcells_and_answers_both_outcomes_honestly() {
    let router = FixtureAskRouter::new();
    let harness = Harness::with_ask_router(aikit_adapters::EnginePolicy::default(), router.clone());

    // First ask: the route names workcell:omarchy, and the relay attempt
    // succeeds there.
    router.script_route(router.relay_route(&canned_ask("where did the run stop?"), "one"));
    harness.admit(fixture_inbound(
        &format!("/ask {RECIPIENT_POSITION} where did the run stop?"),
        "a4",
    ));
    harness.wait_until(
        "the relayed ask answers and the remote ingest was attempted",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("relayed to Workcell workcell:omarchy"))
        },
    );
    let records = harness
        .gateway
        .lock()
        .unwrap()
        .communiques()
        .records()
        .to_vec();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        &records[0].forward,
        Some(CommuniqueForward::Forwarded { remote_gateway_ref, .. })
            if remote_gateway_ref == "agency-gateway/omarchy"
    ));
    assert_eq!(
        router.relayed_refs(),
        vec![records[0].communique_ref.clone()]
    );
    assert!(
        harness
            .executed_sends()
            .iter()
            .any(|text| text.contains("through gateway agency-gateway/omarchy")),
        "the answer names the relaying gateway: {:?}",
        harness.executed_sends()
    );

    // Second ask: the remote gateway cannot be reached; the record stays
    // queued for the next relay pass and the chat is told.
    router.script_route(router.relay_route(&canned_ask("and now?"), "two"));
    router.script_relay(CommuniqueForwardOutcome::Failed {
        workcell_ref: "workcell:omarchy".into(),
        error: "connection refused".into(),
        at_unix_ms: 3_000,
    });
    harness.admit(fixture_inbound(
        &format!("/ask {RECIPIENT_POSITION} and now?"),
        "a5",
    ));
    harness.wait_until(
        "the queued relay is answered honestly",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("queued for relay"))
        },
    );
    let records = harness
        .gateway
        .lock()
        .unwrap()
        .communiques()
        .records()
        .to_vec();
    assert_eq!(records.len(), 2);
    assert!(matches!(
        &records[1].forward,
        Some(CommuniqueForward::Queued { attempts: 1, last_error, .. })
            if last_error.as_deref() == Some("connection refused")
    ));
    assert_eq!(router.relayed_refs().len(), 2);
}

/// The request any admitted fixture ask produces: the binding's semantic
/// identity and the connector conversation it arrives through. The scripted
/// routes are composed from it up front, before `route` consumes them.
fn canned_ask(message: &str) -> GatewayAskRequest {
    GatewayAskRequest {
        binding_ref: r(BINDING_REF),
        connector_ref: CONNECTOR_REF.into(),
        agent_session_ref: "agent-session/fixture".into(),
        agency_ref: "agency/fixture".into(),
        platform: "fixture".into(),
        conversation_id: "chat-1".into(),
        recipient: RECIPIENT_POSITION.into(),
        message: message.into(),
    }
}

fn canned_ask_to(recipient: &str, message: &str) -> GatewayAskRequest {
    GatewayAskRequest {
        recipient: recipient.into(),
        ..canned_ask(message)
    }
}

#[test]
fn an_ask_without_a_wired_router_is_refused_and_nothing_is_recorded() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());

    harness.admit(fixture_inbound(
        &format!("/ask {RECIPIENT_POSITION} anyone home?"),
        "a6",
    ));
    harness.wait_until(
        "the absent-router refusal names the remedy",
        Duration::from_secs(30),
        |harness| {
            harness
                .executed_sends()
                .iter()
                .any(|text| text.contains("no routing behind this gateway"))
        },
    );
    assert_eq!(harness.gateway.lock().unwrap().communiques().len(), 0);
    assert_eq!(harness.source.parked_turns(), 0);
}

#[test]
fn a_denied_or_unpaired_sender_cannot_ask() {
    let dir = TempDir::new().unwrap();
    let router = FixtureAskRouter::new();
    let mut gateway = AgencyGateway::new(r("agency-gateway/test"));
    gateway.register_connector(fixture_descriptor()).unwrap();
    let binding = GatewayBinding {
        binding_ref: r(BINDING_REF),
        connector_ref: r(CONNECTOR_REF),
        address: fixture_address(),
        agent_session_ref: r("agent-session/fixture"),
        agency_ref: r("agency/fixture"),
        actuation_ref: r("actuation/fixture"),
        actuation_stream_ref: r(STREAM_REF),
        agent_ref: None,
        harness_ref: None,
        surface_ref: None,
        forked_from: None,
        context_revision: 1,
        ingress: GatewayIngressPolicy {
            default: GatewayIngressDecision::Pair,
            sender_overrides: [("blocked-sender".into(), GatewayIngressDecision::Deny)]
                .into_iter()
                .collect(),
        },
        provenance: Vec::new(),
    };
    let gateway = Arc::new(Mutex::new(gateway));
    let source = Arc::new(FixtureTurnSource::named("fixture-harness"));
    let engine = aikit_adapters::GatewayConversationEngine::new(
        Arc::clone(&gateway),
        Arc::new(aikit_adapters::SubscriptionHub::default()),
        Arc::new(ConnectorQueues::default()),
        Arc::new(ConnectorPumpControls::default()),
        Some(dir.path().join("gateway.json")),
        Some(Arc::new(FixtureResolver::new(
            r(CONNECTOR_REF),
            Arc::clone(&source),
        ))),
        aikit_adapters::EnginePolicy::default(),
    );
    engine.attach_ask_router(Arc::clone(&router) as Arc<dyn GatewayAskRouter>);
    gateway.lock().unwrap().bind(binding).unwrap();

    // An unpaired sender's /ask asks for pairing; a denied sender's /ask is
    // refused at admission. Neither reaches the ask plane: no request was
    // routed, nothing was recorded, no answer was queued.
    let pair = {
        let mut kernel = gateway.lock().unwrap();
        let result = kernel
            .ingest(fixture_inbound("/ask @steward let me in", "p1"))
            .unwrap();
        if let GatewayIngressResult::Appended { event, .. } = &result {
            engine.appended(&kernel, event);
        }
        result
    };
    assert!(matches!(pair, GatewayIngressResult::PairingRequired { .. }));
    let denied = {
        let mut kernel = gateway.lock().unwrap();
        let result = kernel
            .ingest(fixture_inbound_for(
                "blocked-sender",
                "/ask @steward let me in",
                "p2",
            ))
            .unwrap();
        if let GatewayIngressResult::Appended { event, .. } = &result {
            engine.appended(&kernel, event);
        }
        result
    };
    assert!(matches!(denied, GatewayIngressResult::Denied { .. }));

    thread::sleep(Duration::from_millis(100));
    assert!(router.requests().is_empty(), "{:?}", router.requests());
    assert_eq!(gateway.lock().unwrap().communiques().len(), 0);
    assert_eq!(source.parked_turns(), 0);
    assert_eq!(gateway.lock().unwrap().status().stream_count, 0);
}

#[test]
fn a_drain_needs_no_binding_and_names_each_interrupted_turn_and_never_replays_it() {
    let harness = Harness::new(aikit_adapters::EnginePolicy {
        turn_grace: Duration::from_millis(200),
        interrupt_grace: Duration::from_secs(5),
        ..aikit_adapters::EnginePolicy::default()
    });
    harness.source.script_park();
    harness.admit(fixture_inbound("still thinking", "d1"));
    harness.wait_until(
        "the turn registers in flight",
        Duration::from_secs(30),
        |harness| {
            let execution = harness
                .engine
                .execute(
                    harness.binding_ref.clone(),
                    aikit_adapters::GatewayConversationOperation::Status,
                )
                .unwrap();
            let GatewayResponse::Conversation { result, .. } = execution.response else {
                return false;
            };
            result["turn_in_flight"] == json!(true)
        },
    );
    assert_eq!(harness.source.parked_turns(), 1);

    // An upgrade, a stop signal or an operator drains the whole gateway: no
    // binding, a named reason, an explicit grace.
    let report = harness
        .engine
        .drain("upgrade upg-test", Some(Duration::from_millis(150)))
        .unwrap();
    assert_eq!(report.reason, "upgrade upg-test");
    assert_eq!(report.grace_ms, 150);
    assert_eq!(report.turns_resolved.len(), 0);
    assert_eq!(report.turns_interrupted.len(), 1, "{report:?}");
    assert_eq!(report.turns_interrupted[0].binding_ref, BINDING_REF);
    assert!(
        report.turns_interrupted[0].detail.is_some(),
        "the interrupt's own receipt is kept"
    );
    assert!(report.finished_at_unix_ms >= report.started_at_unix_ms);

    // The interruption is journaled on the turn's own stream, as an uncertain
    // effect — not as a failure to retry.
    harness.wait_until(
        "the interrupted turn is journalled",
        Duration::from_secs(90),
        |harness| harness.stream_events().len() == 2,
    );
    assert_eq!(
        harness.stream_events()[1]["metadata"]["failure"]["kind"],
        "interrupted"
    );

    // Nothing is replayed and nothing new starts: the message that arrives
    // after the drain is journaled, but no second turn is ever prompted.
    harness.admit(fixture_inbound("after the drain", "d2"));
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        harness.source.prompted_turns(),
        1,
        "the interrupted turn was not re-prompted and no new turn started"
    );
    assert_eq!(
        harness.source.parked_turns(),
        0,
        "the interrupted turn is finished, not parked"
    );
    assert_eq!(harness.stream_events().len(), 3);
}

#[test]
fn an_announcement_that_cannot_be_queued_is_an_error_the_caller_sees_not_a_line_lost_on_stderr() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    // A binding the gateway does not hold: the line has nowhere to go.
    let missing = r("gateway-binding/never-bound");
    let announced = harness.engine.execute(
        missing.clone(),
        aikit_adapters::GatewayConversationOperation::Announce {
            text: "gateway upgrade upg-x — completed".into(),
        },
    );
    assert!(
        announced.is_err(),
        "an announcement that was not queued must say so, so the sender can try again"
    );
    // The same announcement to the bound conversation is queued and answered.
    let delivered = harness.engine.execute(
        harness.binding_ref.clone(),
        aikit_adapters::GatewayConversationOperation::Announce {
            text: "gateway upgrade upg-x — completed".into(),
        },
    );
    assert!(delivered.is_ok());
}

/// What a conversation's `/upgrade` asks of the machine's upgrade owner.
struct RecordingUpgrade {
    plans: Mutex<usize>,
    starts: Mutex<Vec<aikit_adapters::UpgradeOrigin>>,
}

impl aikit_adapters::GatewayUpgradeLauncher for RecordingUpgrade {
    fn plan(&self) -> aikit_core::Result<(Value, String)> {
        *self.plans.lock().unwrap() += 1;
        Ok((json!({"action": "restart"}), "the gateway is stale".into()))
    }
    fn start(&self, origin: aikit_adapters::UpgradeOrigin) -> aikit_core::Result<(Value, String)> {
        self.starts.lock().unwrap().push(origin);
        Ok((json!({"upgrade": "upg-x"}), "upgrade upg-x started".into()))
    }
}

#[test]
fn upgrade_is_planned_on_request_started_only_by_apply_and_never_from_a_group() {
    let harness = Harness::new(aikit_adapters::EnginePolicy::default());
    // The edge parses the two spellings and refuses a third.
    assert!(matches!(
        parse_slash("/upgrade"),
        SlashParse::Operation(aikit_adapters::GatewayConversationOperation::Upgrade {
            apply: false
        })
    ));
    assert!(matches!(
        parse_slash("/upgrade apply"),
        SlashParse::Operation(aikit_adapters::GatewayConversationOperation::Upgrade {
            apply: true
        })
    ));
    assert!(matches!(
        parse_slash("/upgrade now"),
        SlashParse::Unknown(_)
    ));

    // No upgrade owner wired: it says so and names the command.
    let execution = harness
        .engine
        .execute(
            harness.binding_ref.clone(),
            aikit_adapters::GatewayConversationOperation::Upgrade { apply: true },
        )
        .unwrap();
    let GatewayResponse::Conversation { result, .. } = execution.response else {
        panic!()
    };
    assert_eq!(result["upgrade"], "unavailable");
    assert!(!execution.restart_requested);

    let launcher = Arc::new(RecordingUpgrade {
        plans: Mutex::new(0),
        starts: Mutex::new(Vec::new()),
    });
    harness.engine.attach_upgrade_launcher(
        launcher.clone() as Arc<dyn aikit_adapters::GatewayUpgradeLauncher>
    );
    let ask = |apply: bool, binding: &ResourceRef| {
        let execution = harness
            .engine
            .execute(
                binding.clone(),
                aikit_adapters::GatewayConversationOperation::Upgrade { apply },
            )
            .unwrap();
        // The gateway keeps serving: the upgrade runs in a worker that drains
        // it when the new build is installed.
        assert!(!execution.restart_requested);
        let GatewayResponse::Conversation { result, .. } = execution.response else {
            panic!()
        };
        result
    };
    // Reading the plan starts nothing.
    let planned = ask(false, &harness.binding_ref);
    assert_eq!(planned["upgrade"], "plan");
    assert_eq!(*launcher.plans.lock().unwrap(), 1);
    assert!(launcher.starts.lock().unwrap().is_empty());
    // `apply` starts it, carrying the conversation the receipt returns to.
    let started = ask(true, &harness.binding_ref);
    assert_eq!(started["upgrade"], "started");
    let starts = launcher.starts.lock().unwrap();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].binding_ref, BINDING_REF);
    assert_eq!(starts[0].connector_ref.as_deref(), Some(CONNECTOR_REF));
    drop(starts);

    // A group conversation admits several senders and a slash command carries
    // a message's authority: changing the machine is refused there.
    let group = r("gateway-binding/fixture-group");
    {
        let mut kernel = harness.gateway.lock().unwrap();
        kernel
            .bind(GatewayBinding {
                binding_ref: group.clone(),
                connector_ref: r(CONNECTOR_REF),
                address: ConversationAddress {
                    platform: "fixture".into(),
                    scope_id: Some("group".into()),
                    conversation_id: "chat-group".into(),
                    thread_id: None,
                },
                agent_session_ref: r("agent-session/fixture-group"),
                agency_ref: r("agency/fixture"),
                actuation_ref: r("actuation/fixture"),
                actuation_stream_ref: r("actuation-stream/fixture-group"),
                agent_ref: None,
                harness_ref: None,
                surface_ref: None,
                forked_from: None,
                context_revision: 1,
                ingress: GatewayIngressPolicy {
                    default: GatewayIngressDecision::Allow,
                    sender_overrides: Default::default(),
                },
                provenance: Vec::new(),
            })
            .unwrap();
    }
    let refused = ask(true, &group);
    assert_eq!(refused["upgrade"], "refused");
    assert_eq!(
        launcher.starts.lock().unwrap().len(),
        1,
        "no second upgrade started"
    );
    // Reading the plan in a group is harmless and still answers.
    assert_eq!(ask(false, &group)["upgrade"], "plan");
}

// ---------------------------------------------------------------------------
// Restart proof against the real binary
// ---------------------------------------------------------------------------

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

fn specimen_bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("gateway-connector-specimen")
}

fn write_connectors_file(home: &std::path::Path) {
    let path = home.join("state/gateway-connectors.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        json!({
            "schema": "aikit.gateway-connectors/v1",
            "connectors": [{
                "connector_ref": "gateway-connector/specimen/main",
                "platform": "specimen",
                "implementation": "stdio",
                "program": [specimen_bin().display().to_string(),
                    "--connector-ref", "gateway-connector/specimen/main"]
            }]
        })
        .to_string(),
    )
    .unwrap();
}

fn spawn_serve(home: &std::path::Path) -> ServeGuard {
    ServeGuard::new(
        Command::new(bin())
            .args(["gateway", "serve"])
            .env("AIKIT_HOME", home)
            .env("HOME", home)
            .env_remove("AIKIT_GATEWAY_TOKEN")
            .current_dir(home)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("aikit gateway serve should spawn"),
    )
}

fn wait_for_socket(home: &std::path::Path) {
    let socket = home.join("state/gateway.sock");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !socket.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(socket.exists(), "serve must bind the default endpoint");
}

fn exchange(socket: &std::path::Path, request: Value) -> Value {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    writeln!(stream, "{request}").unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

fn ingest_via_socket(home: &std::path::Path, text: &str, id: &str) -> Value {
    exchange(
        &home.join("state/gateway.sock"),
        json!({
            "request_id": id,
            "command": {
                "type": "ingest",
                "event": {
                    "event_ref": format!("gateway-ingress/socket/{id}"),
                    "connector_ref": "gateway-connector/specimen/main",
                    "address": {"platform": "specimen", "conversation_id": "main"},
                    "sender": {"native_sender_id": "tester", "kind": "human"},
                    "kind": "message",
                    "native_event_id": format!("socket-native-{id}"),
                    "native_message_id": format!("socket-message-{id}"),
                    "text": text
                }
            }
        }),
    )
}

/// The persisted snapshot, or an empty document when the service has not
/// written yet — a poll that beats the first mutation reads as empty, never
/// a panic.
fn persisted_snapshot(home: &std::path::Path) -> Value {
    let content = fs::read_to_string(home.join("state/gateway.json")).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or(Value::Null)
}

fn wait_specimen_connected(home: &std::path::Path) {
    // The pump's first registration precedes its hello; the hello is what
    // advertises Send. Binding before the hello would make any prepared Send
    // race a capability-less descriptor.
    poll_until(
        "the specimen hello is registered (health connected)",
        Duration::from_secs(30),
        || {
            let snapshot = persisted_snapshot(home);
            snapshot["connector_health"]
                .as_array()
                .map(|healths| {
                    healths.iter().any(|health| {
                        health["connector_ref"] == "gateway-connector/specimen/main"
                            && health["state"] == "connected"
                    })
                })
                .unwrap_or(false)
        },
    );
}

fn bind_specimen(home: &std::path::Path) {
    let socket = home.join("state/gateway.sock");
    let binding = json!({
        "command": {
            "type": "bind",
            "binding": {
                "binding_ref": "gateway-binding/specimen",
                "connector_ref": "gateway-connector/specimen/main",
                "address": {"platform": "specimen", "conversation_id": "main"},
                "agent_session_ref": "agent-session/specimen",
                "agency_ref": "agency/specimen",
                "actuation_ref": "actuation/specimen",
                "actuation_stream_ref": "actuation-stream/specimen",
                "ingress": {"default": "allow", "sender_overrides": {}}
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let response = exchange(&socket, binding.clone());
        if response["ok"] == Value::Bool(true) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the bind never landed: {response}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn persisted_identity(home: &std::path::Path) -> Value {
    let content = fs::read_to_string(home.join("state/gateway.json")).unwrap();
    let snapshot: Value = serde_json::from_str(&content).unwrap();
    json!({
        "gateway_ref": snapshot["gateway_ref"],
        "bindings": snapshot["bindings"].as_array().unwrap().iter()
            .map(|binding| json!({
                "binding_ref": binding["binding_ref"],
                "agent_session_ref": binding["agent_session_ref"],
                "actuation_stream_ref": binding["actuation_stream_ref"],
            }))
            .collect::<Vec<_>>(),
        "streams": snapshot["streams"].as_array().unwrap().iter()
            .map(|stream| json!({
                "stream_ref": stream["stream_ref"],
                "events": stream["events"].as_array().unwrap().iter()
                    .map(|event| event["sequence"].clone())
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn poll_until(what: &str, timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn read_line_with_deadline(stream: &UnixStream, deadline: Instant) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(count) if count > 0 => return serde_json::from_str(line.trim()).unwrap(),
            _ if Instant::now() >= deadline => panic!("timed out reading a gateway line"),
            _ => continue,
        }
    }
}

fn protocol_ok(socket: &std::path::Path) -> bool {
    let connect = || -> std::io::Result<bool> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        writeln!(stream, "{}", json!({"command": {"type": "protocol"}}))?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let parsed: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
        Ok(parsed["ok"] == Value::Bool(true))
    };
    connect().unwrap_or(false)
}

#[test]
fn a_requested_gateway_restart_remateralises_with_identity_intact_and_no_missed_or_duplicated_events(
) {
    let home = TempDir::new().unwrap();
    write_connectors_file(home.path());
    let socket = home.path().join("state/gateway.sock");

    // First life: connector up, conversation bound, two events in the journal,
    // a subscriber that has seen both and then goes away.
    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    wait_specimen_connected(home.path());
    bind_specimen(home.path());
    let first = ingest_via_socket(home.path(), "before restart one", "pre1");
    assert_eq!(first["ok"], Value::Bool(true), "{first}");
    let second = ingest_via_socket(home.path(), "before restart two", "pre2");
    assert_eq!(second["ok"], Value::Bool(true), "{second}");
    poll_until("both events persist", Duration::from_secs(30), || {
        persisted_identity(home.path())["streams"]
            .as_array()
            .map(|streams| {
                streams
                    .iter()
                    .flat_map(|stream| stream["events"].as_array().unwrap().iter())
                    .count()
                    == 2
            })
            .unwrap_or(false)
    });

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    writeln!(
        subscriber,
        "{}",
        json!({"command": {"type": "subscribe", "stream_ref": "actuation-stream/specimen", "after_sequence": 0}})
    )
    .unwrap();
    let replay = read_line_with_deadline(&subscriber, Instant::now() + Duration::from_secs(10));
    assert_eq!(replay["response"]["type"], "replay", "{replay}");
    assert_eq!(
        replay["response"]["replay"]["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(subscriber);

    // The requested restart: a canonical conversation operation. The service
    // drains, answers, persists, and exits cleanly so the service manager (the
    // test) rematerialises it.
    let identity_before = persisted_identity(home.path());
    let restart = exchange(
        &socket,
        json!({
            "request_id": "restart-1",
            "command": {
                "type": "conversation",
                "binding_ref": "gateway-binding/specimen",
                "operation": {"op": "restart"}
            }
        }),
    );
    assert_eq!(restart["ok"], Value::Bool(true), "{restart}");
    assert_eq!(restart["response"]["type"], "conversation", "{restart}");
    assert_eq!(
        restart["response"]["result"]["restarting"],
        Value::Bool(true),
        "{restart}"
    );
    let status = serve.wait().expect("serve should exit after the restart");
    assert!(
        status.success(),
        "a requested restart exits cleanly: {status}"
    );

    // Second life: the rematerialised gateway carries the same semantic
    // identity, and the connector reconnects.
    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    let identity_after = persisted_identity(home.path());
    assert_eq!(
        identity_before, identity_after,
        "the snapshot across the restart changes no semantic identity"
    );
    poll_until(
        "the specimen connector reconnects after the restart",
        Duration::from_secs(30),
        || {
            let snapshot = persisted_snapshot(home.path());
            snapshot["connector_health"]
                .as_array()
                .map(|healths| {
                    healths.iter().any(|health| {
                        health["connector_ref"] == "gateway-connector/specimen/main"
                            && health["state"] == "connected"
                    })
                })
                .unwrap_or(false)
        },
    );

    // A third event appends on the second life while the subscriber is away.
    let third = ingest_via_socket(home.path(), "after restart three", "post1");
    assert_eq!(third["ok"], Value::Bool(true), "{third}");
    poll_until(
        "the away-gap event persists",
        Duration::from_secs(30),
        || {
            persisted_identity(home.path())["streams"]
                .as_array()
                .map(|streams| {
                    streams
                        .iter()
                        .flat_map(|stream| stream["events"].as_array().unwrap().iter())
                        .count()
                        == 3
                })
                .unwrap_or(false)
        },
    );

    // The subscriber re-subscribes from its cursor (2): the replay covers the
    // gap exactly — event 3, nothing repeated — and live pushes resume.
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    writeln!(
        subscriber,
        "{}",
        json!({"command": {"type": "subscribe", "stream_ref": "actuation-stream/specimen", "after_sequence": 2}})
    )
    .unwrap();
    let replay = read_line_with_deadline(&subscriber, Instant::now() + Duration::from_secs(10));
    let covered: Vec<u64> = replay["response"]["replay"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["sequence"].as_u64().unwrap())
        .collect();
    assert_eq!(
        covered,
        vec![3],
        "the replay covers exactly the gap: {replay}"
    );
    assert_eq!(
        replay["response"]["replay"]["events"].as_array().unwrap()[0]["event"]["content"],
        "after restart three"
    );
    let fourth = ingest_via_socket(home.path(), "after restart four", "post2");
    assert_eq!(fourth["ok"], Value::Bool(true), "{fourth}");
    let push = read_line_with_deadline(&subscriber, Instant::now() + Duration::from_secs(10));
    assert_eq!(push["response"]["type"], "stream-event", "{push}");
    assert_eq!(push["response"]["event"]["sequence"], 4, "{push}");
    drop(subscriber);

    // And a hard kill does not corrupt the identity either: what restores is
    // exactly what was persisted.
    let identity_before_kill = persisted_identity(home.path());
    let killed = Command::new("kill")
        .args(["-9", &serve.id().to_string()])
        .status()
        .expect("kill -9 should run");
    assert!(killed.success());
    let status = serve.wait().unwrap();
    assert!(!status.success(), "a killed serve is a killed serve");
    let identity_after_kill = persisted_identity(home.path());
    assert_eq!(identity_before_kill, identity_after_kill);

    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    poll_until(
        "the gateway answers again after the kill cycle",
        Duration::from_secs(30),
        || protocol_ok(&socket),
    );
    let identity_after_rematerialise = persisted_identity(home.path());
    assert_eq!(
        identity_before_kill, identity_after_rematerialise,
        "a killed gateway rematerialises on the same identity"
    );

    exchange(&socket, json!({"command": {"type": "shutdown"}}));
    serve.wait().unwrap();
}

// ---------------------------------------------------------------------------
// Live harness turn (opt-in)
// ---------------------------------------------------------------------------

/// A real agent turn through the real binary, driven by the connector
/// config's agent backing and the encounter plane's provider registry.
///
/// Skipped unless `AIKIT_GATEWAY_LIVE_HARNESS` names a harness binary on
/// PATH (e.g. `pi` or another ACP-speaking agent). Deterministic proof never
/// requires this test.
#[test]
#[ignore = "drives a real harness; set AIKIT_GATEWAY_LIVE_HARNESS=<binary on PATH> to run"]
fn a_real_harness_backs_a_connector_conversation_end_to_end() {
    let Some(harness_binary) = std::env::var("AIKIT_GATEWAY_LIVE_HARNESS")
        .ok()
        .filter(|name| !name.trim().is_empty())
    else {
        eprintln!(
            "skipping: set AIKIT_GATEWAY_LIVE_HARNESS=<binary on PATH> to drive a real harness \
             turn"
        );
        return;
    };
    let which = Command::new("which")
        .arg(&harness_binary)
        .output()
        .expect("which should run");
    if !which.status.success() {
        eprintln!(
            "skipping: {harness_binary} is not on PATH; no live harness turn is possible here"
        );
        return;
    }
    let argv_path = String::from_utf8_lossy(&which.stdout).trim().to_owned();

    let home = TempDir::new().unwrap();

    // The encounter plane's provider registry: one provider whose argv names
    // the live harness, and the connector config naming it as agent backing.
    let providers = home.path().join("state/encounter-providers");
    fs::create_dir_all(&providers).unwrap();
    fs::write(
        providers.join("live-harness.json"),
        json!({
            "id": "live-harness",
            "label": "Live harness for gateway conversation proof",
            "protocol": "acp",
            "argv": [argv_path]
        })
        .to_string(),
    )
    .unwrap();
    let connectors = home.path().join("state/gateway-connectors.json");
    fs::create_dir_all(connectors.parent().unwrap()).unwrap();
    fs::write(
        &connectors,
        json!({
            "schema": "aikit.gateway-connectors/v1",
            "connectors": [{
                "connector_ref": "gateway-connector/specimen/main",
                "platform": "specimen",
                "implementation": "stdio",
                "agent_backing": "live-harness",
                "program": [specimen_bin().display().to_string(),
                    "--connector-ref", "gateway-connector/specimen/main"]
            }]
        })
        .to_string(),
    )
    .unwrap();

    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    bind_specimen(home.path());

    let admitted = ingest_via_socket(
        home.path(),
        "What is one plus one? Answer briefly.",
        "live1",
    );
    assert_eq!(admitted["ok"], Value::Bool(true), "{admitted}");

    // The real harness turn: an agent-message lands on the stream and the
    // reply reaches the connector as a Send answering the inbound message.
    let deadline = Instant::now() + Duration::from_secs(120);
    let reply = loop {
        let snapshot = persisted_snapshot(home.path());
        let reply = snapshot["streams"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|stream| stream["events"].as_array().unwrap().iter())
            .find(|event| event["kind"] == "agent-message")
            .cloned();
        if let Some(reply) = reply {
            break reply;
        }
        assert!(
            Instant::now() < deadline,
            "the live harness turn never produced an agent reply"
        );
        thread::sleep(Duration::from_millis(250));
    };
    let text = reply["content"].as_str().unwrap_or_default();
    assert!(
        !text.trim().is_empty(),
        "the live harness produced an empty reply: {reply}"
    );
    eprintln!("live harness replied: {text}");

    poll_until(
        "the reply's Send is executed and its receipt recorded",
        Duration::from_secs(30),
        || {
            let snapshot = persisted_snapshot(home.path());
            snapshot["delivery_receipts"]
                .as_array()
                .map(|receipts| !receipts.is_empty())
                .unwrap_or(false)
        },
    );

    exchange(
        &home.path().join("state/gateway.sock"),
        json!({"command": {"type": "shutdown"}}),
    );
    serve.wait().unwrap();
}
