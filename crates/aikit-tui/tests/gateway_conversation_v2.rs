//! The Conversation aperture over the real gateway seam.
//!
//! An in-process fake gateway endpoint serves the pinned wire contract on a
//! Unix socket — the same envelope protocol `GatewayClient` speaks and the
//! same subscribe posture the service implements: a subscribe is answered
//! with the replay payload, then one pushed `stream-event` frame per
//! subsequently appended journal event. The tests drive the real application
//! surface (keys, idle ticks, renders) against it and hold the aperture to
//! four facts: history renders from the journal replay; live events appear
//! without user action; a dropped carrier degrades and reconnects from the
//! last seen sequence, covering the gap with no duplicates; and nothing the
//! operator sent is ever re-sent by a reconnect.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use common::*;

use aikit_adapters::gateway_connector::{
    ConnectorConnectionState, ConnectorHealth, ConversationAddress, SenderIdentity, SenderKind,
};
use aikit_adapters::gateway_runtime::{
    GatewayBinding, GatewayCommand, GatewayDiscovery, GatewayEcology, GatewayEcologyAgency,
    GatewayEcologySession, GatewayEcologyStream, GatewayEcologySurface, GatewayIngressDecision,
    GatewayIngressPolicy, GatewayIngressResult, GatewayReplay, GatewayRequestEnvelope,
    GatewayResponse, GatewayResponseEnvelope, GatewayStatus, GatewayStreamEvent,
    AGENCY_GATEWAY_VERSION,
};
use aikit_core::resource::ResourceRef;
use aikit_tui::application_surface::{
    ApplicationSurfaceController, ApplicationSurfaceRequest, ApplicationSurfaceStep,
};
use aikit_tui::conversation_surface::ConversationCarrier;
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Glyphs;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde_json::{json, Value};

const STREAM_REF: &str = "actuation-stream/fixture";
const BINDING_REF: &str = "gateway-binding/fixture";
const CONNECTOR_REF: &str = "gateway-connector/fixture/main";
const GATEWAY_REF: &str = "agency-gateway/fixture";

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: char) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL))
}

fn typed(text: &str) -> Vec<PaletteEvent> {
    text.chars()
        .map(|character| key(KeyCode::Char(character)))
        .chain(std::iter::once(key(KeyCode::Enter)))
        .collect()
}

fn wait_until(seconds: u64, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !condition() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(condition(), "condition not reached within {seconds}s");
}

/// One subscribed connection the fake gateway pushes to. The gate mirrors
/// the real service's subscription sink: the connection is registered before
/// the replay answer is written, but pushes queue behind the gate until the
/// replay is on the wire — so an append can neither be missed by a client
/// still opening, nor overtake the replay.
struct Subscriber {
    gate: Mutex<bool>,
    queue: Mutex<Vec<String>>,
    writer: Mutex<UnixStream>,
}

impl Subscriber {
    fn new(writer: UnixStream) -> Self {
        Self {
            gate: Mutex::new(false),
            queue: Mutex::new(Vec::new()),
            writer: Mutex::new(writer),
        }
    }

    fn push(&self, frame: &str) {
        let mut queue = self.queue.lock().unwrap();
        if *self.gate.lock().unwrap() {
            drop(queue);
            self.write(frame);
        } else {
            queue.push(frame.to_string());
        }
    }

    fn open_gate(&self) {
        *self.gate.lock().unwrap() = true;
        let queued = {
            let mut queue = self.queue.lock().unwrap();
            queue.drain(..).collect::<Vec<_>>()
        };
        for frame in queued {
            self.write(&frame);
        }
    }

    fn write(&self, frame: &str) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writeln!(writer, "{frame}");
        let _ = writer.flush();
    }
}

/// The envelope: an in-process endpoint speaking the pinned gateway wire
/// contract over a Unix socket.
struct FakeGateway {
    socket_path: PathBuf,
    state: Mutex<FakeState>,
    subscribers: Mutex<Vec<Arc<Subscriber>>>,
    /// Texts of every ingest the endpoint admitted, in arrival order.
    ingests: Mutex<Vec<String>>,
    /// What the endpoint's ingress policy does with an ingest.
    ingress: Mutex<GatewayIngressDecision>,
    connector_state: Mutex<ConnectorConnectionState>,
    listening: AtomicBool,
}

struct FakeState {
    journal: Vec<Value>,
}

impl FakeGateway {
    fn start(directory: &Path) -> Arc<Self> {
        let gateway = Arc::new(Self {
            socket_path: directory.join("gateway.sock"),
            state: Mutex::new(FakeState {
                journal: vec![journal_event(1, "hello there")],
            }),
            subscribers: Mutex::new(Vec::new()),
            ingests: Mutex::new(Vec::new()),
            ingress: Mutex::new(GatewayIngressDecision::Allow),
            connector_state: Mutex::new(ConnectorConnectionState::Connected),
            listening: AtomicBool::new(true),
        });
        // Bind on the calling thread: the socket exists before start returns,
        // so no test can connect ahead of the listener.
        let socket_path = gateway.socket_path.clone();
        let _ = std::fs::remove_file(&socket_path);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        gateway.serve(listener);
        gateway
    }

    fn serve(self: &Arc<Self>, listener: std::os::unix::net::UnixListener) {
        let gateway = Arc::clone(self);
        let socket_path = self.socket_path.clone();
        self.listening.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            while gateway.listening.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let gateway = Arc::clone(&gateway);
                        thread::spawn(move || gateway.handle_connection(stream));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            drop(listener);
            let _ = std::fs::remove_file(&socket_path);
        });
    }

    /// Close every subscribed connection at the socket level: the carrier
    /// drop a client sees when the gateway process goes away underneath it.
    fn kick(&self) {
        let subscribers = self.subscribers.lock().unwrap();
        for subscriber in subscribers.iter() {
            let writer = subscriber.writer.lock().unwrap();
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
    }

    fn journal(&self) -> Vec<Value> {
        self.state.lock().unwrap().journal.clone()
    }

    /// Append one journal event and push it to every live subscriber, the
    /// way the real service's subscription hub publishes an append.
    fn push_inbound(&self, text: &str) {
        self.push_inbound_from("fixture-user", text);
    }

    fn push_inbound_from(&self, sender: &str, text: &str) {
        let sequence = {
            let mut state = self.state.lock().unwrap();
            let sequence = state.journal.len() as u64 + 1;
            state.journal.push(journal_event_from(sequence, text, sender));
            sequence
        };
        self.publish(sequence, text, sender);
    }

    fn publish(&self, sequence: u64, text: &str, sender: &str) {
        let frame = serde_json::to_string(&GatewayResponseEnvelope::from_result(
            None,
            Ok(GatewayResponse::StreamEvent {
                stream_ref: ResourceRef::parse(STREAM_REF).unwrap(),
                event: GatewayStreamEvent {
                    sequence,
                    event: journal_event_from(sequence, text, sender),
                },
            }),
        ))
        .unwrap();
        for subscriber in self.subscribers.lock().unwrap().iter() {
            subscriber.push(&frame);
        }
    }

    fn ingest_texts(&self) -> Vec<String> {
        self.ingests.lock().unwrap().clone()
    }

    fn set_ingress(&self, decision: GatewayIngressDecision) {
        *self.ingress.lock().unwrap() = decision;
    }

    fn set_connector_state(&self, state: ConnectorConnectionState) {
        *self.connector_state.lock().unwrap() = state;
    }

    fn handle_connection(self: &Arc<Self>, stream: UnixStream) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let Ok(request) = serde_json::from_str::<GatewayRequestEnvelope>(line.trim()) else {
                continue;
            };
            let subscribe = matches!(request.command, GatewayCommand::Subscribe { .. });
            // Register before the replay answer goes out, gate closed — the
            // same indivisible register-then-answer the real service gives a
            // subscriber, so nothing appended after the replay can be missed.
            let subscriber = if subscribe {
                Some(Arc::new(Subscriber::new(stream.try_clone().unwrap())))
            } else {
                None
            };
            if let Some(subscriber) = &subscriber {
                self.subscribers.lock().unwrap().push(Arc::clone(subscriber));
            }
            let response = self.answer(&request.command);
            let encoded = serde_json::to_string(&GatewayResponseEnvelope::from_result(
                request.request_id,
                Ok(response),
            ))
            .unwrap();
            {
                let mut writer = stream.try_clone().unwrap();
                writeln!(writer, "{encoded}").unwrap();
                writer.flush().unwrap();
            }
            if let Some(subscriber) = subscriber {
                subscriber.open_gate();
            }
        }
    }

    fn answer(&self, command: &GatewayCommand) -> GatewayResponse {
        match command {
            GatewayCommand::Status => GatewayResponse::Status {
                status: GatewayStatus {
                    version: AGENCY_GATEWAY_VERSION.into(),
                    gateway_ref: ResourceRef::parse(GATEWAY_REF).unwrap(),
                    connector_count: 1,
                    binding_count: 1,
                    stream_count: 1,
                    pending_delivery_count: 0,
                    delivery_receipt_count: 0,
                    connector_health: vec![ConnectorHealth {
                        connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
                        state: *self.connector_state.lock().unwrap(),
                        detail: Some("fixture link".into()),
                        provenance: vec!["fixture".into()],
                    }],
                },
            },
            GatewayCommand::Ecology => GatewayResponse::Ecology {
                ecology: GatewayEcology {
                    version: AGENCY_GATEWAY_VERSION.into(),
                    gateway_ref: ResourceRef::parse(GATEWAY_REF).unwrap(),
                    authority: "presence-does-not-imply-authority".into(),
                    agencies: vec![GatewayEcologyAgency {
                        agency_ref: ResourceRef::parse("agency/fixture").unwrap(),
                        sessions: vec![GatewayEcologySession {
                            agent_session_ref: ResourceRef::parse("agent-session/fixture")
                                .unwrap(),
                            agency_ref: ResourceRef::parse("agency/fixture").unwrap(),
                            actuation_refs: vec![ResourceRef::parse("actuation/fixture").unwrap()],
                            agent_ref: None,
                            harness_ref: None,
                            streams: vec![GatewayEcologyStream {
                                stream_ref: ResourceRef::parse(STREAM_REF).unwrap(),
                                last_sequence: self.journal().len() as u64,
                                event_count: self.journal().len(),
                            }],
                            surfaces: vec![GatewayEcologySurface {
                                binding_ref: ResourceRef::parse(BINDING_REF).unwrap(),
                                connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
                                platform: "fixture".into(),
                                address: ConversationAddress {
                                    platform: "fixture".into(),
                                    scope_id: None,
                                    conversation_id: "conv-1".into(),
                                    thread_id: None,
                                },
                                surface_ref: None,
                                ingress_default: GatewayIngressDecision::Allow,
                                forked_from: None,
                                context_revision: 1,
                            }],
                            invocation_modes: vec![],
                        }],
                    }],
                },
            },
            GatewayCommand::Discover => GatewayResponse::Discovery {
                discovery: GatewayDiscovery {
                    version: AGENCY_GATEWAY_VERSION.into(),
                    gateway_ref: ResourceRef::parse(GATEWAY_REF).unwrap(),
                    connector_sdk_version: "aikit.gateway-connector-sdk/v1".into(),
                    connector_wire_version: "aikit.gateway-connector-wire/v1".into(),
                    connectors: vec![],
                    bindings: vec![fixture_binding()],
                },
            },
            GatewayCommand::Subscribe {
                after_sequence, limit, ..
            } => {
                let journal = self.journal();
                // Sequence is index+1: a replay returns events whose
                // sequence is strictly after the cursor, exactly like the
                // real kernel's journal filter.
                let events = journal
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index as u64 + 1 > *after_sequence)
                    .take(*limit)
                    .map(|(index, event)| GatewayStreamEvent {
                        sequence: index as u64 + 1,
                        event: event.clone(),
                    })
                    .collect::<Vec<_>>();
                let returned_through = events
                    .last()
                    .map(|event| event.sequence)
                    .unwrap_or(*after_sequence);
                let stream_last_sequence = journal.len() as u64;
                GatewayResponse::Replay {
                    replay: GatewayReplay {
                        stream_ref: ResourceRef::parse(STREAM_REF).unwrap(),
                        after_sequence: *after_sequence,
                        returned_through,
                        stream_last_sequence,
                        has_more: returned_through < stream_last_sequence,
                        events,
                    },
                }
            }
            GatewayCommand::Ingest { event } => {
                let text = event.text.clone().unwrap_or_default();
                let sender = event.sender.native_sender_id.clone();
                match *self.ingress.lock().unwrap() {
                    GatewayIngressDecision::Allow => {
                        let sequence = {
                            let mut state = self.state.lock().unwrap();
                            let sequence = state.journal.len() as u64 + 1;
                            state
                                .journal
                                .push(journal_event_from(sequence, &text, &sender));
                            sequence
                        };
                        self.ingests.lock().unwrap().push(text.clone());
                        self.publish(sequence, &text, &sender);
                        GatewayResponse::Ingress {
                            result: GatewayIngressResult::Appended {
                                binding_ref: ResourceRef::parse(BINDING_REF).unwrap(),
                                stream_ref: ResourceRef::parse(STREAM_REF).unwrap(),
                                event: GatewayStreamEvent {
                                    sequence,
                                    event: journal_event_from(sequence, &text, &sender),
                                },
                            },
                        }
                    }
                    GatewayIngressDecision::Deny => GatewayResponse::Ingress {
                        result: GatewayIngressResult::Denied {
                            binding_ref: ResourceRef::parse(BINDING_REF).unwrap(),
                            sender: sender_from(&event.sender.native_sender_id),
                        },
                    },
                    GatewayIngressDecision::Pair => GatewayResponse::Ingress {
                        result: GatewayIngressResult::PairingRequired {
                            binding_ref: ResourceRef::parse(BINDING_REF).unwrap(),
                            sender: sender_from(&event.sender.native_sender_id),
                        },
                    },
                }
            }
            other => panic!("the fake gateway was not prepared for {other:?}"),
        }
    }
}

fn sender_from(native_sender_id: &str) -> SenderIdentity {
    SenderIdentity {
        native_sender_id: native_sender_id.into(),
        kind: SenderKind::Human,
        display_name: None,
        metadata: Default::default(),
    }
}

fn fixture_binding() -> GatewayBinding {
    GatewayBinding {
        binding_ref: ResourceRef::parse(BINDING_REF).unwrap(),
        connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
        address: ConversationAddress {
            platform: "fixture".into(),
            scope_id: None,
            conversation_id: "conv-1".into(),
            thread_id: None,
        },
        agent_session_ref: ResourceRef::parse("agent-session/fixture").unwrap(),
        agency_ref: ResourceRef::parse("agency/fixture").unwrap(),
        actuation_ref: ResourceRef::parse("actuation/fixture").unwrap(),
        actuation_stream_ref: ResourceRef::parse(STREAM_REF).unwrap(),
        agent_ref: None,
        harness_ref: None,
        surface_ref: None,
        forked_from: None,
        context_revision: 1,
        ingress: GatewayIngressPolicy {
            default: GatewayIngressDecision::Allow,
            sender_overrides: Default::default(),
        },
        provenance: vec!["fixture".into()],
    }
}

/// The portable event shape the real gateway journals for an inbound
/// message: kind, content, sender metadata.
fn journal_event(sequence: u64, text: &str) -> Value {
    journal_event_from(sequence, text, "fixture-user")
}

fn journal_event_from(sequence: u64, text: &str, sender: &str) -> Value {
    json!({
        "event_ref": format!("{STREAM_REF}/gateway-event/{sequence}"),
        "sequence": sequence,
        "kind": "human-message",
        "native_trace_ref": format!("fixture-event-{sequence}"),
        "disclosure": "portable",
        "metadata": {
            "connector_ref": CONNECTOR_REF,
            "platform": "fixture",
            "conversation_id": "conv-1",
            "native_sender_id": sender,
            "sender_kind": "human"
        },
        "content": text
    })
}

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(dir.path(), vec![skill("skill/conversation/one")]);
    (dir, backend)
}

fn surface_with(backend: &mut Fixture, socket: &Path) -> ApplicationSurfaceController {
    ApplicationSurfaceController::new(
        backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_glyphs(Glyphs::ascii())
            .with_conversation_carrier(ConversationCarrier::UnixSocket(socket.to_path_buf())),
    )
    .unwrap()
}

fn render(surface: &ApplicationSurfaceController) -> String {
    let mut terminal = Terminal::with_options(
        TestBackend::new(100, 30),
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    )
    .unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

/// Ctrl+G opens the roster; Enter opens the one fixture conversation.
fn open_conversation(surface: &mut ApplicationSurfaceController, backend: &mut Fixture) {
    surface.handle(backend, ctrl('g')).unwrap();
    surface.handle(backend, key(KeyCode::Enter)).unwrap();
}

#[test]
fn the_aperture_lists_bound_conversations_from_the_gateway_ecology() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);

    surface.handle(&mut backend, ctrl('g')).unwrap();
    let rendered = render(&surface);
    assert!(
        rendered.contains("fixture/conv-1"),
        "the roster must name the bound conversation: {rendered}"
    );
    assert!(
        rendered.contains(CONNECTOR_REF),
        "the roster must name the connector: {rendered}"
    );
    assert!(
        rendered.contains("presence-does-not-imply-authority"),
        "the roster must carry the gateway's own authority law: {rendered}"
    );
}

#[test]
fn opening_a_conversation_renders_the_journal_history_and_the_status_strip() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);

    open_conversation(&mut surface, &mut backend);
    let rendered = render(&surface);
    assert!(
        rendered.contains("fixture-user: hello there"),
        "the replayed history must render the journal's content and sender: {rendered}"
    );
    assert!(
        rendered.contains("gateway live"),
        "the status strip must show the live link: {rendered}"
    );
    assert!(
        rendered.contains(&format!("connector {CONNECTOR_REF} connected")),
        "the status strip must show the connector's health reading: {rendered}"
    );
    assert!(
        rendered.contains("seq 1"),
        "the status strip must show the last sequence seen: {rendered}"
    );
}

#[test]
fn a_live_event_appears_without_user_action() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    gateway.push_inbound("arrived while you watched");
    // One idle tick is the aperture's only poll; no key, no query, no redraw
    // request from the user.
    surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
    let rendered = render(&surface);
    assert!(
        rendered.contains("fixture-user: arrived while you watched"),
        "a pushed event must render after one idle poll: {rendered}"
    );
}

#[test]
fn a_dropped_carrier_degrades_then_reconnect_covers_the_gap_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    // The gateway goes away underneath the aperture.
    gateway.kick();
    surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
    let rendered = render(&surface);
    assert!(
        rendered.contains("gateway degraded"),
        "a dropped carrier must show the degraded state: {rendered}"
    );

    // Events append while nobody is connected; they land only in the journal.
    gateway.push_inbound("missed while down");
    gateway.push_inbound("missed while down too");

    // Idle ticks retry with the linear backoff; poll until the reconnect has
    // covered the gap.
    wait_until(15, || {
        surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
        render(&surface).contains("missed while down too")
    });

    let rendered = render(&surface);
    assert!(
        rendered.contains("gateway live"),
        "the reconnect must restore the live state: {rendered}"
    );
    assert!(
        rendered.contains("fixture-user: hello there"),
        "the history window must still hold the earlier events: {rendered}"
    );
    assert_eq!(
        rendered.matches("missed while down too").count(),
        1,
        "the gap must be covered exactly once: {rendered}"
    );
    assert_eq!(
        rendered.matches("fixture-user: hello there").count(),
        1,
        "no line may be duplicated by the reconnect replay: {rendered}"
    );
    assert!(
        rendered.contains("seq 3"),
        "the strip must show the last sequence seen after the gap: {rendered}"
    );
}

#[test]
fn a_composed_message_enters_the_same_stream_once_and_survives_reconnect_without_resending() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    for event in typed("is the build red?") {
        surface.handle(&mut backend, event).unwrap();
    }

    assert_eq!(
        gateway.ingest_texts(),
        vec!["is the build red?".to_string()],
        "Enter must send exactly the composed text through the gateway's ingest path"
    );

    // The appended message reaches the pane through the subscription, not
    // through local echo.
    surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
    let rendered = render(&surface);
    assert!(
        rendered.contains("aikit-tui-operator: is the build red?"),
        "the sent message must render from the stream itself: {rendered}"
    );
    assert_eq!(
        rendered.matches("is the build red?").count(),
        1,
        "a sent message renders once - the compose lane is cleared, not echoed: {rendered}"
    );

    // The carrier drops, an event appends while the link is down, and the
    // reconnect must cover the gap without re-sending the operator's text.
    gateway.kick();
    surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
    gateway.push_inbound("reply while the link was down");
    wait_until(15, || {
        surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
        render(&surface).contains("reply while the link was down")
    });
    let rendered = render(&surface);
    assert!(
        rendered.contains("gateway live"),
        "the reconnect must restore the live state: {rendered}"
    );
    assert_eq!(
        gateway.ingest_texts().len(),
        1,
        "a reconnect must never re-send what the operator already sent"
    );
    assert_eq!(
        rendered.matches("is the build red?").count(),
        1,
        "the operator's message stays exactly once across the reconnect: {rendered}"
    );
    assert_eq!(
        rendered.matches("reply while the link was down").count(),
        1,
        "the gap is covered exactly once: {rendered}"
    );
}

#[test]
fn a_gateway_refusal_is_disclosed_and_nothing_is_appended() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    gateway.set_ingress(GatewayIngressDecision::Deny);
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    for event in typed("denied text") {
        surface.handle(&mut backend, event).unwrap();
    }

    assert!(
        gateway.ingest_texts().is_empty(),
        "a denied message is not recorded as an accepted ingest"
    );
    let rendered = render(&surface);
    assert!(
        rendered.contains("ingress policy denied"),
        "the refusal must be disclosed in the pane: {rendered}"
    );
    assert!(
        rendered.contains("denied text"),
        "the refused text is kept in the compose lane: {rendered}"
    );
    assert!(
        !rendered.contains("aikit-tui-operator: denied text"),
        "nothing refused may render as if it were on the stream: {rendered}"
    );
}

#[test]
fn the_status_strip_reports_a_degraded_connector_honestly() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    gateway.set_connector_state(ConnectorConnectionState::Degraded);
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    let rendered = render(&surface);
    assert!(
        rendered.contains(&format!("connector {CONNECTOR_REF} degraded")),
        "the strip must carry the connector's actual health state: {rendered}"
    );
    assert!(
        rendered.contains("fixture link"),
        "the strip must carry the health reading's detail: {rendered}"
    );
}

#[test]
fn an_unreachable_gateway_opens_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    // No gateway serves this socket at all.
    let socket = dir.path().join("absent.sock");
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &socket);

    surface.handle(&mut backend, ctrl('g')).unwrap();
    let rendered = render(&surface);
    assert!(
        rendered.contains("gateway unreachable"),
        "an unreachable gateway must be disclosed: {rendered}"
    );
    assert!(
        rendered.contains("ecology could not be read"),
        "an unreachable gateway must not be read as an empty ecology: {rendered}"
    );
    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert!(
        !render(&surface).contains(" Conversation "),
        "Esc must close the aperture again"
    );
}

#[test]
fn esc_from_an_open_conversation_returns_to_the_roster_and_esc_again_closes() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    let rendered = render(&surface);
    assert!(
        rendered.contains("fixture/conv-1"),
        "Esc must return to the roster: {rendered}"
    );
    assert!(
        !rendered.contains("compose>"),
        "the compose lane must not survive the open conversation: {rendered}"
    );

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert!(
        !render(&surface).contains(" Conversation "),
        "Esc from the roster must close the aperture"
    );
}

#[test]
fn an_unknown_event_kind_is_disclosed_not_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    {
        let mut state = gateway.state.lock().unwrap();
        state.journal.push(json!({
            "event_ref": format!("{STREAM_REF}/gateway-event/2"),
            "sequence": 2,
            "kind": "teleportation",
            "disclosure": "portable",
            "metadata": {}
        }));
    }
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);
    open_conversation(&mut surface, &mut backend);

    let rendered = render(&surface);
    assert!(
        rendered.contains("unhandled event kind"),
        "an unknown kind must be disclosed: {rendered}"
    );
    assert!(
        rendered.contains("teleportation"),
        "the unknown kind's name must be shown: {rendered}"
    );
}

#[test]
fn the_aperture_is_inert_until_opened_and_the_loop_stays_quiet_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = FakeGateway::start(dir.path());
    let (_fixture_dir, mut backend) = fixture();
    let mut surface = surface_with(&mut backend, &gateway.socket_path);

    // Idle ticks with the aperture closed poll nothing and render nothing
    // new; the ordinary shell behaves exactly as before.
    surface.handle(&mut backend, PaletteEvent::Idle).unwrap();
    let rendered = render(&surface);
    assert!(
        !rendered.contains(" Conversation "),
        "the aperture must not exist until Ctrl+G opens it: {rendered}"
    );
    assert_eq!(
        surface.handle(&mut backend, key(KeyCode::Esc)).unwrap(),
        ApplicationSurfaceStep::Continue
    );
}
