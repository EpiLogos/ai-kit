//! Gateway-side stdio wire host: a [`GatewayConnector`] over a child process
//! speaking `aikit.gateway-connector-wire/v1` JSON frames on stdio.
//!
//! This is the external-style connector seam: an out-of-process connector is
//! spawned as `program…`, sends `hello` (descriptor) and — when it wishes —
//! `inbound`, `delivery-receipt`, `health` and `error` frames; the host sends
//! `outbound` and `shutdown` frames. Frames are one JSON document per line.
//!
//! The host's `next_event` never blocks indefinitely: when no frame arrives
//! within its poll window it yields the quiet-poll error
//! (`CONNECTOR_QUIET_POLL_CODE`) so the owning pump can service outbound work.
//! The child process itself keeps running across those ticks.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};

use crate::gateway_connector::{
    ConnectorDescriptor, ConnectorFuture, ConnectorHealth, ConnectorHello, ConnectorWireFrame,
    DeliveryReceipt, DeliveryState, GatewayConnector, InboundEvent, OutboundOperation,
    GATEWAY_CONNECTOR_WIRE_VERSION,
};
use crate::gateway_connector_pump::CONNECTOR_QUIET_POLL_CODE;

/// How long `next_event` waits for a frame before yielding to the pump.
const DEFAULT_POLL_WINDOW: Duration = Duration::from_millis(250);
/// How long `execute` waits for the connector's receipt.
const DEFAULT_EXECUTE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long `connect` waits for the connector's hello.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// One spawned out-of-process connector over the wire protocol.
pub struct StdioWireConnector {
    program: Vec<String>,
    connector_ref: ResourceRef,
    platform: String,
    configuration_ref: Option<ResourceRef>,
    session: Option<WireSession>,
    poll_window: Duration,
    execute_timeout: Duration,
    connect_timeout: Duration,
}

struct WireSession {
    child: Child,
    stdin: Option<ChildStdin>,
    hello_rx: mpsc::Receiver<ConnectorHello>,
    inbound_rx: mpsc::Receiver<InboundEvent>,
    pending_receipts: Arc<Mutex<BTreeMap<ResourceRef, mpsc::Sender<DeliveryReceipt>>>>,
    health: Arc<Mutex<Option<ConnectorHealth>>>,
    last_error: Arc<Mutex<Option<(Instant, String)>>>,
    reader_done: Arc<AtomicBool>,
    descriptor: Option<ConnectorDescriptor>,
    reader: JoinHandle<()>,
}

impl StdioWireConnector {
    pub fn new(
        program: Vec<String>,
        connector_ref: ResourceRef,
        platform: String,
        configuration_ref: Option<ResourceRef>,
    ) -> Self {
        Self {
            program,
            connector_ref,
            platform,
            configuration_ref,
            session: None,
            poll_window: DEFAULT_POLL_WINDOW,
            execute_timeout: DEFAULT_EXECUTE_TIMEOUT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    fn spawn_session(&mut self) -> Result<&mut WireSession> {
        let (program, args) = self
            .program
            .split_first()
            .map(|(first, rest)| (first.clone(), rest.to_vec()))
            .ok_or_else(|| {
                AikitError::new(
                    "gateway_connector_wire.empty_program",
                    "the stdio connector has no program to run",
                )
            })?;
        let mut child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                AikitError::new(
                    "gateway_connector_wire.spawn",
                    format!("spawn stdio connector {program}: {error}"),
                )
            })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        let (hello_tx, hello_rx) = mpsc::channel();
        let (inbound_tx, inbound_rx) = mpsc::channel();
        let pending_receipts: Arc<Mutex<BTreeMap<ResourceRef, mpsc::Sender<DeliveryReceipt>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let health = Arc::new(Mutex::new(None));
        let last_error = Arc::new(Mutex::new(None));
        let reader_done = Arc::new(AtomicBool::new(false));

        let reader = {
            let pending_receipts = Arc::clone(&pending_receipts);
            let health = Arc::clone(&health);
            let last_error = Arc::clone(&last_error);
            let reader_done = Arc::clone(&reader_done);
            thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(count) if count > 0 => {}
                        _ => break,
                    }
                    let frame: ConnectorWireFrame = match serde_json::from_str(line.trim()) {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    };
                    match frame {
                        ConnectorWireFrame::Hello { hello } => {
                            let _ = hello_tx.send(hello);
                        }
                        ConnectorWireFrame::Inbound { event } => {
                            let _ = inbound_tx.send(event);
                        }
                        ConnectorWireFrame::DeliveryReceipt { receipt } => {
                            let sender = pending_receipts
                                .lock()
                                .ok()
                                .and_then(|mut pending| pending.remove(&receipt.operation_ref));
                            if let Some(sender) = sender {
                                let _ = sender.send(receipt);
                            }
                        }
                        ConnectorWireFrame::Health { health: observed } => {
                            if let Ok(mut slot) = health.lock() {
                                *slot = Some(observed);
                            }
                        }
                        ConnectorWireFrame::Error { code, message, .. } => {
                            if let Ok(mut slot) = last_error.lock() {
                                *slot = Some((Instant::now(), format!("{code}: {message}")));
                            }
                        }
                        ConnectorWireFrame::Shutdown { .. }
                        | ConnectorWireFrame::Outbound { .. } => {}
                    }
                }
                reader_done.store(true, Ordering::SeqCst);
            })
        };

        self.session = Some(WireSession {
            child,
            stdin: Some(stdin),
            hello_rx,
            inbound_rx,
            pending_receipts,
            health,
            last_error,
            reader_done,
            descriptor: None,
            reader,
        });
        Ok(self.session.as_mut().expect("just assigned"))
    }

    fn exchange_hello(&mut self) -> Result<ConnectorHello> {
        let deadline = Instant::now() + self.connect_timeout;
        loop {
            let session = self.session.as_mut().expect("session during hello");
            if !Self::child_alive(session) {
                return Err(AikitError::new(
                    "gateway_connector_wire.child_exited",
                    format!(
                        "stdio connector {} exited before saying hello",
                        self.program.join(" ")
                    ),
                ));
            }
            match session.hello_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(hello) => {
                    hello.validate()?;
                    if hello.descriptor.connector_ref != self.connector_ref {
                        return Err(AikitError::new(
                            "gateway_connector.connector_identity_drift",
                            format!(
                                "stdio connector {} said hello as {}",
                                self.connector_ref, hello.descriptor.connector_ref
                            ),
                        ));
                    }
                    if !hello
                        .descriptor
                        .platform
                        .eq_ignore_ascii_case(&self.platform)
                    {
                        return Err(AikitError::new(
                            "gateway_connector.platform_drift",
                            format!(
                                "stdio connector {} declared platform {} but is configured as {}",
                                self.connector_ref, hello.descriptor.platform, self.platform
                            ),
                        ));
                    }
                    let session = self.session.as_mut().expect("session during hello");
                    session.descriptor = Some(hello.descriptor.clone());
                    return Ok(hello);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if Instant::now() >= deadline {
                        return Err(AikitError::new(
                            "gateway_connector_wire.hello_timeout",
                            format!(
                                "stdio connector {} sent no hello within {:?}",
                                self.connector_ref, self.connect_timeout
                            ),
                        ));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(AikitError::new(
                        "gateway_connector_wire.child_exited",
                        "stdio connector's output ended before its hello",
                    ));
                }
            }
        }
    }

    fn teardown(&mut self) {
        if let Some(session) = self.session.take() {
            self.teardown_session(session);
        }
    }

    fn teardown_session(&self, mut session: WireSession) {
        if let Some(stdin) = session.stdin.as_mut() {
            let shutdown = ConnectorWireFrame::Shutdown {
                reason: "gateway disconnecting the connector".into(),
            };
            let _ = writeln!(stdin, "{}", serde_json::to_string(&shutdown).unwrap_or_default());
            let _ = stdin.flush();
        }
        session.stdin = None;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match session.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = session.child.kill();
                    let _ = session.child.wait();
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let _ = session.reader.join();
    }

    fn child_alive(session: &mut WireSession) -> bool {
        session.child.try_wait().map(|status| status.is_none()).unwrap_or(false)
    }

    fn failed_receipt(operation: &OutboundOperation, connector_ref: &ResourceRef, detail: String) -> DeliveryReceipt {
        DeliveryReceipt {
            operation_ref: operation.operation_ref.clone(),
            connector_ref: connector_ref.clone(),
            state: DeliveryState::Failed,
            native_message_id: None,
            detail: Some(detail),
            native: BTreeMap::new(),
            provenance: vec!["aikit stdio wire host".into()],
        }
    }
}

impl GatewayConnector for StdioWireConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        if let Some(session) = &self.session {
            if let Some(descriptor) = &session.descriptor {
                return descriptor.clone();
            }
        }
        crate::gateway_connector_config::stdio_shell_descriptor(
            &self.connector_ref,
            &self.platform,
            self.configuration_ref.as_ref(),
        )
    }

    fn connect(&mut self) -> ConnectorFuture<'_, ConnectorHello> {
        let result = (|| -> Result<ConnectorHello> {
            if let Some(session) = &mut self.session {
                if Self::child_alive(session) {
                    return Err(AikitError::new(
                        "gateway_connector_wire.already_connected",
                        "the stdio connector process is already running",
                    ));
                }
                self.teardown();
            }
            self.spawn_session()?;
            self.exchange_hello()
        })();
        Box::pin(async move { result })
    }

    fn next_event(&mut self) -> ConnectorFuture<'_, Option<InboundEvent>> {
        let result = (|| -> Result<Option<InboundEvent>> {
            let mut session = match self.session.take() {
                Some(session) => session,
                None => return Ok(None),
            };
            let outcome = loop {
                if !Self::child_alive(&mut session) {
                    // The child is gone; flush whatever it already delivered.
                    loop {
                        match session.inbound_rx.try_recv() {
                            Ok(event) => {
                                self.session = Some(session);
                                return Ok(Some(event));
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                    let deadline = Instant::now() + Duration::from_millis(200);
                    let mut trailing = None;
                    while !session.reader_done.load(Ordering::SeqCst)
                        && Instant::now() < deadline
                    {
                        std::thread::sleep(Duration::from_millis(10));
                        if let Ok(event) = session.inbound_rx.try_recv() {
                            trailing = Some(event);
                            break;
                        }
                    }
                    if let Some(event) = trailing {
                        self.session = Some(session);
                        return Ok(Some(event));
                    }
                    self.teardown_session(session);
                    return Ok(None);
                }
                match session.inbound_rx.recv_timeout(self.poll_window) {
                    Ok(event) => break Ok(Some(event)),
                    Err(mpsc::RecvTimeoutError::Timeout) => break Err(AikitError::new(
                        CONNECTOR_QUIET_POLL_CODE,
                        "no connector frame within the poll window",
                    )),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        self.teardown_session(session);
                        return Ok(None);
                    }
                }
            };
            self.session = Some(session);
            outcome
        })();
        Box::pin(async move { result })
    }

    fn execute(&mut self, operation: OutboundOperation) -> ConnectorFuture<'_, DeliveryReceipt> {
        let result = (|| -> Result<DeliveryReceipt> {
            let descriptor = self.descriptor();
            operation.validate(&descriptor)?;
            let Some(session) = &mut self.session else {
                return Err(AikitError::new(
                    "gateway_connector_wire.not_connected",
                    "the stdio connector is not running",
                ));
            };
            let (tx, rx) = mpsc::channel();
            session
                .pending_receipts
                .lock()
                .expect("pending receipts")
                .insert(operation.operation_ref.clone(), tx);
            let frame = ConnectorWireFrame::Outbound {
                operation: operation.clone(),
            };
            let write = (|| -> std::io::Result<()> {
                let stdin = session.stdin.as_mut().expect("open session stdin");
                writeln!(stdin, "{}", serde_json::to_string(&frame).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?)?;
                stdin.flush()?;
                Ok(())
            })();
            if let Err(error) = write {
                session
                    .pending_receipts
                    .lock()
                    .expect("pending receipts")
                    .remove(&operation.operation_ref);
                return Err(AikitError::new(
                    "gateway_connector_wire.write",
                    format!("write to the stdio connector failed: {error}"),
                ));
            }
            match rx.recv_timeout(self.execute_timeout) {
                Ok(receipt) => Ok(receipt),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let recent = session
                        .last_error
                        .lock()
                        .as_ref()
                        .ok()
                        .and_then(|slot| slot.as_ref())
                        .filter(|(at, _)| at.elapsed() < self.execute_timeout)
                        .map(|(_, message)| message.clone());
                    Ok(Self::failed_receipt(
                        &operation,
                        &self.connector_ref,
                        recent.unwrap_or_else(|| {
                            format!(
                                "no delivery receipt within {:?}",
                                self.execute_timeout
                            )
                        }),
                    ))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => Ok(Self::failed_receipt(
                    &operation,
                    &self.connector_ref,
                    "the stdio connector went away before answering".into(),
                )),
            }
        })();
        Box::pin(async move { result })
    }

    fn health(&mut self) -> ConnectorFuture<'_, ConnectorHealth> {
        let health = (|| -> ConnectorHealth {
            let base = |state, detail| ConnectorHealth {
                connector_ref: self.connector_ref.clone(),
                state,
                detail,
                provenance: vec!["aikit stdio wire host".into()],
            };
            match &mut self.session {
                None => base(
                    crate::gateway_connector::ConnectorConnectionState::Unavailable,
                    Some("stdio connector not running".into()),
                ),
                Some(session) => {
                    if let Some(observed) = session.health.lock().ok().and_then(|slot| slot.clone())
                    {
                        return observed;
                    }
                    if Self::child_alive(session) {
                        base(
                            crate::gateway_connector::ConnectorConnectionState::Connected,
                            Some("stdio wire process alive".into()),
                        )
                    } else {
                        base(
                            crate::gateway_connector::ConnectorConnectionState::Degraded,
                            Some("stdio wire process has exited".into()),
                        )
                    }
                }
            }
        })();
        let result: Result<ConnectorHealth> = Ok(health);
        Box::pin(async move { result })
    }

    fn disconnect(&mut self) -> ConnectorFuture<'_, ()> {
        self.teardown();
        let result: Result<()> = Ok(());
        Box::pin(async move { result })
    }
}

/// The wire version this host speaks, re-exported for specimen authors.
pub const STDIO_WIRE_VERSION: &str = GATEWAY_CONNECTOR_WIRE_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    use aikit_core::resource::ResourceRef;
    use serde_json::json;

    use crate::gateway_connector::{
        ConnectorOperation, ConversationAddress, OutboundOperation, OutboundOperationKind,
        GATEWAY_CONNECTOR_SDK_VERSION,
    };
    use crate::gateway_connector_pump::block_on;

    const CONNECTOR_REF: &str = "gateway-connector/specimen/wire-test";

    /// A scripted out-of-process connector: hello, health, one inbound event,
    /// then a receipt for the first outbound and a clean exit on shutdown.
    fn scripted_child() -> Vec<String> {
        let hello = json!({
            "type": "hello",
            "hello": {
                "wire_version": GATEWAY_CONNECTOR_WIRE_VERSION,
                "descriptor": {
                    "version": GATEWAY_CONNECTOR_SDK_VERSION,
                    "connector_ref": CONNECTOR_REF,
                    "platform": "specimen",
                    "implementation": "scripted-specimen",
                    "capabilities": {"operations": ["send"], "provenance": ["scripted"]},
                    "provenance": ["scripted"]
                }
            }
        });
        let health = json!({
            "type": "health",
            "health": {
                "connector_ref": CONNECTOR_REF,
                "state": "connected",
                "detail": "scripted specimen alive",
                "provenance": []
            }
        });
        let inbound = json!({
            "type": "inbound",
            "event": {
                "event_ref": "gateway-ingress/specimen/1",
                "connector_ref": CONNECTOR_REF,
                "address": {
                    "platform": "specimen",
                    "conversation_id": "chat-9"
                },
                "sender": {"native_sender_id": "u-1", "kind": "human"},
                "kind": "message",
                "native_event_id": "scripted-1",
                "text": "over the wire"
            }
        });
        let receipt = json!({
            "type": "delivery-receipt",
            "receipt": {
                "operation_ref": "gateway-operation/00000000000000000001",
                "connector_ref": CONNECTOR_REF,
                "state": "delivered",
                "native_message_id": "scripted-message-1",
                "detail": "scripted specimen executed the send",
                "native": {},
                "provenance": []
            }
        });
        let script = format!(
            r#"printf '%s\n' '{hello}'
printf '%s\n' '{health}'
printf '%s\n' '{inbound}'
while IFS= read -r line; do
  case "$line" in
    *'"type":"outbound"'*) printf '%s\n' '{receipt}' ;;
    *'"type":"shutdown"'*) exit 0 ;;
  esac
done
"#,
            hello = hello,
            health = health,
            inbound = inbound,
            receipt = receipt,
        );
        vec!["sh".into(), "-c".into(), script]
    }

    fn send_operation(text: &str) -> OutboundOperation {
        OutboundOperation {
            operation_ref: ResourceRef::parse("gateway-operation/00000000000000000001").unwrap(),
            connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
            address: ConversationAddress {
                platform: "specimen".into(),
                scope_id: None,
                conversation_id: "chat-9".into(),
                thread_id: None,
            },
            operation: OutboundOperationKind::Send {
                text: Some(text.into()),
                media: Vec::new(),
                reply_to_native_message_id: None,
            },
            agent_session_ref: None,
            actuation_stream_ref: None,
            provenance: Vec::new(),
        }
    }

    fn quiet_poll(connector: &mut StdioWireConnector) -> Option<InboundEvent> {
        for _ in 0..40 {
            match block_on(connector.next_event()) {
                Err(error) if error.code() == CONNECTOR_QUIET_POLL_CODE => continue,
                other => return other.expect("event or disconnect"),
            }
        }
        panic!("the connector never delivered its inbound event");
    }

    #[test]
    fn the_stdio_wire_host_drives_a_scripted_out_of_process_connector() {
        let mut connector = StdioWireConnector::new(
            scripted_child(),
            ResourceRef::parse(CONNECTOR_REF).unwrap(),
            "specimen".into(),
            None,
        );
        // Before any hello, the host advertises an empty-capability shell.
        assert!(connector.descriptor().capabilities.operations.is_empty());

        let hello = block_on(connector.connect()).unwrap();
        hello.validate().unwrap();
        assert_eq!(hello.descriptor.implementation, "scripted-specimen");
        // The hello's descriptor replaces the shell — and it deliberately
        // does not advertise edit.
        assert!(connector
            .descriptor()
            .capabilities
            .supports(ConnectorOperation::Send));
        assert!(!connector
            .descriptor()
            .capabilities
            .supports(ConnectorOperation::Edit));

        // The inbound event crosses the wire into normalized form.
        let event = quiet_poll(&mut connector).expect("the scripted inbound event");
        assert_eq!(event.address.conversation_id, "chat-9");
        assert_eq!(event.text.as_deref(), Some("over the wire"));

        // An operation the connector advertises executes; its receipt carries
        // the child's execution marker.
        let receipt = block_on(connector.execute(send_operation("echo"))).unwrap();
        assert_eq!(receipt.state, DeliveryState::Delivered);
        assert_eq!(
            receipt.detail.as_deref(),
            Some("scripted specimen executed the send")
        );

        // An operation the connector does not advertise fails conformance at
        // the connector seam, before any frame is written.
        let mut edit = send_operation("nope");
        edit.operation = OutboundOperationKind::Edit {
            native_message_id: "scripted-message-1".into(),
            text: "nope".into(),
        };
        let error = block_on(connector.execute(edit)).unwrap_err();
        assert_eq!(error.code(), "gateway_connector.unsupported_operation");

        // Health reported by the child is what the host observes.
        let health = block_on(connector.health()).unwrap();
        assert_eq!(health.state, crate::gateway_connector::ConnectorConnectionState::Connected);
        assert_eq!(health.detail.as_deref(), Some("scripted specimen alive"));

        block_on(connector.disconnect()).unwrap();
        // After disconnect there is no session: no events, and a refused
        // child spawn is an honest error rather than a hang.
        assert!(block_on(connector.next_event()).unwrap().is_none());
    }

    #[test]
    fn a_stdio_connector_that_cannot_spawn_is_a_named_error_not_a_hang() {
        let mut connector = StdioWireConnector::new(
            vec!["/nonexistent/connector-binary".into()],
            ResourceRef::parse(CONNECTOR_REF).unwrap(),
            "specimen".into(),
            None,
        );
        let error = block_on(connector.connect()).unwrap_err();
        assert_eq!(error.code(), "gateway_connector_wire.spawn");
        // And a next_event without a session reports a disconnected stream.
        assert!(block_on(connector.next_event()).unwrap().is_none());
    }
}
