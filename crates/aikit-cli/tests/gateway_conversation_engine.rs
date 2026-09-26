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

use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use aikit_adapters::{
    execute_gateway_command, spawn_connector_workers, AgencyGateway, ConnectorCapabilities,
    ConnectorConnectionState, ConnectorDescriptor, ConnectorFuture, ConnectorHealth,
    ConnectorHello, ConnectorOperation, ConnectorPumpControls, ConnectorQueues,
    ConversationAddress, DeliveryReceipt, DeliveryState, FixtureTurnSource, GatewayBinding,
    GatewayCommand, GatewayConnector, GatewayConnectorEntry, GatewayConnectorFactory,
    GatewayIngressDecision, GatewayIngressPolicy, GatewayIngressResult, GatewayResponse,
    GatewayTurnSourceResolver, InboundEvent, InboundEventKind, OutboundOperation, SenderIdentity,
    CONNECTOR_QUIET_POLL_CODE, GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION,
};
use aikit_core::resource::ResourceRef;
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
}

struct RecordingInner {
    events: Mutex<VecDeque<Option<InboundEvent>>>,
    executed: Mutex<Vec<OutboundOperation>>,
    connected: AtomicBool,
}

impl RecordingInner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(VecDeque::new()),
            executed: Mutex::new(Vec::new()),
            connected: AtomicBool::new(false),
        })
    }
}

impl GatewayConnector for RecordingConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        fixture_descriptor()
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
        let result: aikit_core::Result<DeliveryReceipt> = Ok(DeliveryReceipt {
            operation_ref: operation.operation_ref.clone(),
            connector_ref: operation.connector_ref.clone(),
            state: DeliveryState::Delivered,
            native_message_id: Some(format!("test-out-{}", operation.operation_ref)),
            detail: Some(format!("test connector executed {name}")),
            native: Default::default(),
            provenance: vec!["conversation-test".into()],
        });
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
    inner: Arc<RecordingInner>,
}

impl GatewayConnectorFactory for RecordingFactory {
    fn entry(&self) -> &GatewayConnectorEntry {
        &self.entry
    }

    fn build(&self) -> aikit_core::Result<Box<dyn GatewayConnector>> {
        Ok(Box::new(RecordingConnector {
            connector_ref: r(CONNECTOR_REF),
            inner: Arc::clone(&self.inner),
        }))
    }
}

/// Resolves the one fixture connector ref to the scripted turn source.
struct FixtureResolver {
    connector_ref: ResourceRef,
    source: Arc<FixtureTurnSource>,
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
        let dir = TempDir::new().unwrap();
        let mut gateway = AgencyGateway::new(r("agency-gateway/test"));
        gateway.register_connector(fixture_descriptor()).unwrap();
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
            Some(Arc::new(FixtureResolver {
                connector_ref: r(CONNECTOR_REF),
                source: Arc::clone(&source),
            })),
            policy,
        );
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
                    provenance: Vec::new(),
                },
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

    fn wait_until(&self, what: &str, timeout: Duration, mut condition: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition(self) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
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
        Some(Arc::new(FixtureResolver {
            connector_ref: r(CONNECTOR_REF),
            source: Arc::clone(&source),
        })),
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
    harness.wait_until(
        "the interrupted turn is journalled",
        Duration::from_secs(30),
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
                provenance: Vec::new(),
            },
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
    thread::sleep(Duration::from_millis(300));
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
    drop(paused_gateway);

    // Resume: the held event is admitted.
    controls.set_paused(&r(CONNECTOR_REF), false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let gateway = gateway.lock().unwrap();
        if gateway.status().stream_count == 1 {
            break;
        }
        drop(gateway);
        thread::sleep(Duration::from_millis(10));
    }
    let gateway = gateway.lock().unwrap();
    assert_eq!(gateway.status().stream_count, 1, "the held event landed");
    drop(gateway);

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

fn spawn_serve(home: &std::path::Path) -> std::process::Child {
    Command::new(bin())
        .args(["gateway", "serve"])
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env_remove("AIKIT_GATEWAY_TOKEN")
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("aikit gateway serve should spawn")
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
