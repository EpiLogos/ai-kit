//! Connector pumps: the workers that make a gateway service actually run
//! connectors.
//!
//! One pump per enabled configured connector. The loop is the connector
//! lifecycle the public SDK describes: connect → report health → loop
//! `next_event`, ingesting each event through the shared kernel exactly as a
//! carrier command would (mutating commands persist state), then — when the
//! kernel hands it a prepared [`OutboundOperation`] — execute and record the
//! delivery receipt. Health observations flow into the kernel so
//! `gateway status` shows them.
//!
//! Laws kept here:
//!
//! - A pump failure never takes the carriers down. Workers catch everything;
//!   a connector that cannot connect leaves the service up with Unavailable
//!   health and a named detail.
//! - An inbound event whose (connector, conversation, native_event_id) was
//!   already ingested is skipped, and the skip is recorded in the connector's
//!   health detail — not as a new kernel concept.
//! - Ingest appends are published to the live subscription registry while the
//!   kernel lock is held, so subscribers never see a gap or a reordered event.
//!
//! Outbound latency is bounded by the connector's own `next_event` blocking
//! behaviour: a worker services the outbound queue between event polls.
//! Connectors that poll cooperatively (the stdio wire host yields a quiet-poll
//! error when no frame arrives within its window) hand prepared operations to
//! the platform promptly; long-poll connectors wait out one poll.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};

use crate::gateway_connector::{
    ConnectorConnectionState, ConnectorHealth, DeliveryReceipt, DeliveryState, GatewayConnector,
    InboundEvent, OutboundOperation,
};
use crate::gateway_connector_config::GatewayConnectorFactory;
use crate::gateway_runtime::{
    execute_gateway_command, AgencyGateway, GatewayCommand, GatewayIngressResult, GatewayResponse,
};
use crate::gateway_service::{persist_gateway_state, SubscriptionHub};

/// Error code a cooperative connector yields from `next_event` when no frame
/// arrived within its poll window — a tick, not a failure. The worker services
/// the outbound queue and health between these ticks.
pub const CONNECTOR_QUIET_POLL_CODE: &str = "gateway_connector_wire.quiet_poll";

/// Consecutive `next_event` errors tolerated before a reconnect cycle.
const MAX_EVENT_FAILURES: usize = 3;
/// Connect attempts before a connector is left Unavailable.
const MAX_CONNECT_ATTEMPTS: usize = 5;
/// Linear reconnect backoff step: attempt N waits N × this.
const BACKOFF_STEP: Duration = Duration::from_millis(200);

/// The thread-safe outbound hand-off: anything holding the kernel mutex can
/// push a prepared operation to the owning connector's worker.
#[derive(Default)]
pub struct ConnectorOutbound {
    queue: Mutex<VecDeque<OutboundOperation>>,
}

impl ConnectorOutbound {
    pub fn push(&self, operation: OutboundOperation) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(operation);
        }
    }

    fn try_pop(&self) -> Option<OutboundOperation> {
        self.queue.lock().ok().and_then(|mut queue| queue.pop_front())
    }
}

/// All connectors' outbound queues, keyed by connector ref.
#[derive(Default)]
pub struct ConnectorQueues {
    queues: Mutex<BTreeMap<ResourceRef, Arc<ConnectorOutbound>>>,
}

impl ConnectorQueues {
    pub fn queue_for(&self, connector_ref: &ResourceRef) -> Arc<ConnectorOutbound> {
        let mut queues = self.queues.lock().expect("connector queue registry");
        queues
            .entry(connector_ref.clone())
            .or_default()
            .clone()
    }
}

struct WorkerContext {
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    state_file: Option<PathBuf>,
    hub: Arc<SubscriptionHub>,
    queue: Arc<ConnectorOutbound>,
}

/// Spawn one pump per enabled factory. Disabled entries are named on stderr
/// and not started. Workers exit when `shutdown` is set; join them before the
/// service's final state write.
pub fn spawn_connector_workers(
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    state_file: Option<PathBuf>,
    hub: Arc<SubscriptionHub>,
    queues: Arc<ConnectorQueues>,
    factories: Vec<Box<dyn GatewayConnectorFactory>>,
) -> Vec<JoinHandle<()>> {
    let mut workers = Vec::new();
    for factory in factories {
        let entry = factory.entry();
        if !entry.enabled {
            eprintln!(
                "connector {} is declared disabled in the connectors file; not starting it",
                entry.connector_ref
            );
            continue;
        }
        let connector_ref = match ResourceRef::parse(&entry.connector_ref) {
            Ok(connector_ref) => connector_ref,
            Err(error) => {
                eprintln!(
                    "connector {} has an unusable ref and cannot start: {error}",
                    entry.connector_ref
                );
                continue;
            }
        };
        let context = WorkerContext {
            gateway: Arc::clone(&gateway),
            shutdown: Arc::clone(&shutdown),
            state_file: state_file.clone(),
            hub: Arc::clone(&hub),
            queue: queues.queue_for(&connector_ref),
        };
        workers.push(thread::spawn(move || {
            run_connector_worker(context, factory, connector_ref);
        }));
    }
    workers
}

fn run_connector_worker(
    context: WorkerContext,
    factory: Box<dyn GatewayConnectorFactory>,
    connector_ref: ResourceRef,
) {
    let entry = factory.entry().clone();
    let mut connector = match factory.build() {
        Ok(connector) => connector,
        Err(error) => {
            park_unavailable(
                &context,
                &connector_ref,
                format!("connector could not be built: {error}"),
            );
            return;
        }
    };
    // Register the connector's own descriptor first: every kernel observation
    // (health, ingest, delivery) is refused for an unregistered connector.
    let descriptor = connector.descriptor();
    if let Err(error) = locked_command(
        &context,
        GatewayCommand::RegisterConnector { descriptor },
    ) {
        park_unavailable(
            &context,
            &connector_ref,
            format!(
                "connector {} was refused by the kernel: {error}",
                entry.connector_ref
            ),
        );
        return;
    }

    let mut connect_failures = 0usize;
    let mut last_connect_error = String::new();
    loop {
        if context.shutdown.load(Ordering::SeqCst) {
            let _ = block_on(connector.disconnect());
            return;
        }
        match block_on(connector.connect()) {
            Ok(hello) => match hello.validate().and_then(|()| {
                check_hello_identity(&hello.descriptor, &connector_ref, &entry.platform)
            }) {
                Ok(()) => {
                    if let Err(error) = locked_command(
                        &context,
                        GatewayCommand::RegisterConnector {
                            descriptor: hello.descriptor.clone(),
                        },
                    ) {
                        park_unavailable(
                            &context,
                            &connector_ref,
                            format!("connector hello was refused by the kernel: {error}"),
                        );
                        return;
                    }
                    let generation = hello.descriptor;
                    record_health(
                        &context,
                        ConnectorHealth {
                            connector_ref: connector_ref.clone(),
                            state: ConnectorConnectionState::Connected,
                            detail: Some(format!(
                                "implementation {} connected",
                                entry.implementation
                            )),
                            provenance: vec!["gateway connector pump".into()],
                        },
                    );
                    connect_failures = 0;
                    match serve_event_loop(&context, connector.as_mut(), &generation) {
                        LoopExit::Shutdown => {
                            let _ = block_on(connector.disconnect());
                            return;
                        }
                        LoopExit::Ended(reason) => {
                            record_health(
                                &context,
                                ConnectorHealth {
                                    connector_ref: connector_ref.clone(),
                                    state: ConnectorConnectionState::Reconnecting,
                                    detail: Some(reason),
                                    provenance: vec!["gateway connector pump".into()],
                                },
                            );
                            let _ = block_on(connector.disconnect());
                            sleep_with_shutdown(&context, BACKOFF_STEP);
                        }
                    }
                }
                Err(error) => {
                    last_connect_error = format!("connector hello refused: {error}");
                    note_connect_failure(&context, &connector_ref, &last_connect_error);
                    let _ = block_on(connector.disconnect());
                    connect_failures += 1;
                }
            },
            Err(error) => {
                connect_failures += 1;
                last_connect_error = format!("connect attempt {connect_failures} failed: {error}");
                note_connect_failure(&context, &connector_ref, &last_connect_error);
                // A failed connect may still hold a half-started session.
                let _ = block_on(connector.disconnect());
            }
        }
        if connect_failures >= MAX_CONNECT_ATTEMPTS {
            record_health(
                &context,
                ConnectorHealth {
                    connector_ref: connector_ref.clone(),
                    state: ConnectorConnectionState::Unavailable,
                    detail: Some(format!(
                        "gave up after {MAX_CONNECT_ATTEMPTS} connect attempts; last: \
                         {last_connect_error}"
                    )),
                    provenance: vec!["gateway connector pump".into()],
                },
            );
            park_until_shutdown(&context);
            return;
        }
        sleep_with_shutdown(&context, BACKOFF_STEP * connect_failures.max(1) as u32);
    }
}

fn note_connect_failure(context: &WorkerContext, connector_ref: &ResourceRef, detail: &str) {
    record_health(
        context,
        ConnectorHealth {
            connector_ref: connector_ref.clone(),
            state: ConnectorConnectionState::Unavailable,
            detail: Some(detail.to_owned()),
            provenance: vec!["gateway connector pump".into()],
        },
    );
}

enum LoopExit {
    Shutdown,
    Ended(String),
}

/// The connected loop: outbound queue, health, then one event per iteration.
fn serve_event_loop(
    context: &WorkerContext,
    connector: &mut dyn GatewayConnector,
    generation: &crate::gateway_connector::ConnectorDescriptor,
) -> LoopExit {
    let mut failures = 0usize;
    let mut seen_native_events: BTreeSet<String> = BTreeSet::new();
    let mut duplicates_skipped = 0usize;
    let mut last_health: Option<ConnectorHealth> = None;
    let mut last_ingress_note: Option<String> = None;

    loop {
        if context.shutdown.load(Ordering::SeqCst) {
            return LoopExit::Shutdown;
        }
        // Outbound first: anything the kernel prepared for this connector.
        while let Some(operation) = context.queue.try_pop() {
            if let Err(error) = execute_and_record(context, connector, operation, generation) {
                eprintln!("connector pump could not record a delivery: {error}");
            }
        }
        // Health flows into the kernel so `gateway status` shows it.
        let health = match block_on(connector.health()) {
            Ok(health) => health,
            Err(error) => ConnectorHealth {
                connector_ref: generation.connector_ref.clone(),
                state: ConnectorConnectionState::Degraded,
                detail: Some(format!("health query failed: {error}")),
                provenance: vec!["gateway connector pump".into()],
            },
        };
        let health = if duplicates_skipped > 0 {
            let mut health = health;
            let base = health.detail.unwrap_or_else(|| "connected".into());
            health.detail = Some(format!(
                "{base}; {duplicates_skipped} duplicate events skipped"
            ));
            health
        } else {
            health
        };
        if last_health.as_ref() != Some(&health) {
            record_health(context, health.clone());
            last_health = Some(health);
        }
        // Ingress: one event per iteration.
        match block_on(connector.next_event()) {
            Ok(Some(event)) => {
                failures = 0;
                if let Some(key) = duplicate_key(&event) {
                    if !seen_native_events.insert(key) {
                        duplicates_skipped += 1;
                        continue;
                    }
                }
                match ingest_and_publish(context, event) {
                    Ok(()) => last_ingress_note = None,
                    Err(error) => {
                        // An unbound conversation or a refused sender is the
                        // kernel's answer, not a pump failure; keep it visible.
                        let note = format!("ingress not appended: {error}");
                        if last_ingress_note.as_deref() != Some(note.as_str()) {
                            eprintln!("connector pump: {note}");
                            last_ingress_note = Some(note);
                        }
                    }
                }
            }
            Ok(None) => {
                return LoopExit::Ended("connector ended its event stream".into());
            }
            Err(error) if error.code() == CONNECTOR_QUIET_POLL_CODE => {
                continue;
            }
            Err(error) => {
                failures += 1;
                record_health(
                    context,
                    ConnectorHealth {
                        connector_ref: generation.connector_ref.clone(),
                        state: ConnectorConnectionState::Degraded,
                        detail: Some(format!("next_event failed ({failures}): {error}")),
                        provenance: vec!["gateway connector pump".into()],
                    },
                );
                if failures >= MAX_EVENT_FAILURES {
                    return LoopExit::Ended(format!(
                        "connector kept failing after {failures} consecutive errors"
                    ));
                }
            }
        }
    }
}

fn execute_and_record(
    context: &WorkerContext,
    connector: &mut dyn GatewayConnector,
    operation: OutboundOperation,
    generation: &crate::gateway_connector::ConnectorDescriptor,
) -> Result<()> {
    let receipt = match block_on(connector.execute(operation.clone())) {
        Ok(receipt) => receipt,
        Err(error) => DeliveryReceipt {
            operation_ref: operation.operation_ref.clone(),
            connector_ref: generation.connector_ref.clone(),
            state: DeliveryState::Failed,
            native_message_id: None,
            detail: Some(format!("connector could not execute the operation: {error}")),
            native: Default::default(),
            provenance: vec!["gateway connector pump".into()],
        },
    };
    locked_command(context, GatewayCommand::RecordDelivery { receipt }).map(|_| ())
}

fn duplicate_key(event: &InboundEvent) -> Option<String> {
    let native_event_id = event.native_event_id.as_deref()?;
    Some(format!(
        "{}|{}|{}|{}|{}",
        event.address.platform,
        event.address.scope_id.as_deref().unwrap_or(""),
        event.address.conversation_id,
        event.address.thread_id.as_deref().unwrap_or(""),
        native_event_id,
    ))
}

/// One kernel command under the state lock, persisted like a carrier command.
/// An appended ingress is published to the live subscription registry while
/// the lock is still held so subscribers see every append, in order.
fn locked_command(context: &WorkerContext, command: GatewayCommand) -> Result<GatewayResponse> {
    let mut gateway = context.gateway.lock().map_err(|_| {
        AikitError::new(
            "agency_gateway_service.poisoned",
            "gateway state lock was poisoned",
        )
    })?;
    let response = execute_gateway_command(&mut gateway, command)?;
    if let GatewayResponse::Ingress {
        result: GatewayIngressResult::Appended { stream_ref, event, .. },
    } = &response
    {
        context.hub.publish(stream_ref, event);
    }
    persist_gateway_state(&gateway, context.state_file.as_deref())?;
    Ok(response)
}

fn ingest_and_publish(context: &WorkerContext, event: InboundEvent) -> Result<()> {
    locked_command(context, GatewayCommand::Ingest { event }).map(|_| ())
}

fn record_health(context: &WorkerContext, health: ConnectorHealth) {
    if let Err(error) = locked_command(context, GatewayCommand::SetConnectorHealth { health }) {
        eprintln!("connector pump could not record health: {error}");
    }
}

fn check_hello_identity(
    descriptor: &crate::gateway_connector::ConnectorDescriptor,
    connector_ref: &ResourceRef,
    platform: &str,
) -> Result<()> {
    if descriptor.connector_ref != *connector_ref {
        return Err(AikitError::new(
            "gateway_connector.connector_identity_drift",
            format!(
                "connector hello names {} but the configured connector is {connector_ref}",
                descriptor.connector_ref
            ),
        ));
    }
    if !descriptor.platform.eq_ignore_ascii_case(platform) {
        return Err(AikitError::new(
            "gateway_connector.platform_drift",
            format!(
                "connector hello platform {} does not match configured platform {platform}",
                descriptor.platform
            ),
        ));
    }
    Ok(())
}

fn park_unavailable(context: &WorkerContext, connector_ref: &ResourceRef, detail: String) {
    eprintln!("gateway connector not starting: {detail}");
    record_health(
        context,
        ConnectorHealth {
            connector_ref: connector_ref.clone(),
            state: ConnectorConnectionState::Unavailable,
            detail: Some(detail),
            provenance: vec!["gateway connector pump".into()],
        },
    );
    park_until_shutdown(context);
}

fn park_until_shutdown(context: &WorkerContext) {
    while !context.shutdown.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(250));
    }
}

fn sleep_with_shutdown(context: &WorkerContext, duration: Duration) {
    let step = Duration::from_millis(50);
    let mut remaining = duration;
    while remaining > Duration::ZERO && !context.shutdown.load(Ordering::SeqCst) {
        let slice = step.min(remaining);
        thread::sleep(slice);
        remaining = remaining.saturating_sub(slice);
    }
}

/// Drive an SDK future to completion. Every in-tree connector resolves its
/// futures eagerly; the poll loop only defends against a future that parks.
pub(crate) fn block_on<T>(
    mut future: Pin<Box<dyn Future<Output = Result<T>> + Send + '_>>,
) -> Result<T> {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park_timeout(Duration::from_millis(5)),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    use aikit_core::resource::ResourceRef;
    use serde_json::json;

    use crate::gateway_connector::{
        ConnectorDescriptor, ConnectorFuture, ConnectorHello, ConversationAddress, InboundEvent,
        InboundEventKind, OutboundOperationKind, SenderIdentity, GATEWAY_CONNECTOR_SDK_VERSION,
        GATEWAY_CONNECTOR_WIRE_VERSION,
    };
    use crate::gateway_connector_config::GatewayConnectorEntry;
    use crate::gateway_runtime::{
        AgencyGateway, GatewayBinding, GatewayCommand, GatewayIngressDecision,
        GatewayIngressPolicy,
    };
    use crate::telegram_bot_api::TelegramConnectorConfig;
    use crate::telegram_gateway::TelegramConnector;

    pub(crate) const PLATFORM: &str = "fixture";
    pub(crate) const CONNECTOR_REF: &str = "gateway-connector/fixture/main";
    pub(crate) const BINDING_REF: &str = "gateway-binding/fixture";
    pub(crate) const STREAM_REF: &str = "actuation-stream/fixture";

    /// A controllable in-process connector. `events` drives next_event; a
    /// `None` entry ends the stream (a disconnect). An empty queue yields the
    /// quiet-poll tick, exactly like the stdio wire host.
    pub(crate) struct FixtureInner {
        pub connect_failures_remaining: AtomicUsize,
        pub connected: AtomicBool,
        pub connects: AtomicUsize,
        pub disconnects: AtomicUsize,
        pub events: Mutex<VecDeque<Option<InboundEvent>>>,
        pub executed: Mutex<Vec<OutboundOperation>>,
    }

    impl FixtureInner {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                connect_failures_remaining: AtomicUsize::new(0),
                connected: AtomicBool::new(false),
                connects: AtomicUsize::new(0),
                disconnects: AtomicUsize::new(0),
                events: Mutex::new(VecDeque::new()),
                executed: Mutex::new(Vec::new()),
            })
        }

        pub(crate) fn push_text(&self, text: &str) {
            self.events
                .lock()
                .unwrap()
                .push_back(Some(fixture_event(text)));
        }

        pub(crate) fn push_disconnect(&self) {
            self.events.lock().unwrap().push_back(None);
        }

        pub(crate) fn fail_next_connects(&self, count: usize) {
            self.connect_failures_remaining
                .store(count, Ordering::SeqCst);
        }
    }

    pub(crate) fn fixture_connector(inner: Arc<FixtureInner>) -> FixtureConnector {
        FixtureConnector {
            connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
            inner,
        }
    }

    pub(crate) struct FixtureConnector {
        connector_ref: ResourceRef,
        inner: Arc<FixtureInner>,
    }

    impl GatewayConnector for FixtureConnector {
        fn descriptor(&self) -> ConnectorDescriptor {
            fixture_descriptor()
        }

        fn connect(&mut self) -> ConnectorFuture<'_, ConnectorHello> {
            let result = (|| -> Result<ConnectorHello> {
                self.inner.connects.fetch_add(1, Ordering::SeqCst);
                let remaining = self
                    .inner
                    .connect_failures_remaining
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                        if n == 0 { None } else { Some(n - 1) }
                    });
                if remaining.is_ok() {
                    return Err(AikitError::new(
                        "gateway_connector_fixture.connect_refused",
                        "fixture connector refused the connection",
                    ));
                }
                self.inner.connected.store(true, Ordering::SeqCst);
                Ok(ConnectorHello {
                    wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
                    descriptor: self.descriptor(),
                })
            })();
            Box::pin(async move { result })
        }

        fn next_event(&mut self) -> ConnectorFuture<'_, Option<InboundEvent>> {
            let result = (|| -> Result<Option<InboundEvent>> {
                let next = self.inner.events.lock().unwrap().pop_front();
                match next {
                    Some(Some(event)) => Ok(Some(event)),
                    Some(None) => {
                        self.inner.connected.store(false, Ordering::SeqCst);
                        Ok(None)
                    }
                    None => {
                        std::thread::sleep(Duration::from_millis(2));
                        Err(AikitError::new(
                            CONNECTOR_QUIET_POLL_CODE,
                            "no fixture event queued",
                        ))
                    }
                }
            })();
            Box::pin(async move { result })
        }

        fn execute(
            &mut self,
            operation: OutboundOperation,
        ) -> ConnectorFuture<'_, DeliveryReceipt> {
            let result = (|| -> Result<DeliveryReceipt> {
                operation.validate(&self.descriptor())?;
                self.inner.executed.lock().unwrap().push(operation.clone());
                Ok(DeliveryReceipt {
                    operation_ref: operation.operation_ref,
                    connector_ref: operation.connector_ref,
                    state: DeliveryState::Delivered,
                    native_message_id: Some("fixture-message-1".into()),
                    detail: Some("fixture executed the operation".into()),
                    native: Default::default(),
                    provenance: vec!["fixture".into()],
                })
            })();
            Box::pin(async move { result })
        }

        fn health(&mut self) -> ConnectorFuture<'_, ConnectorHealth> {
            let connected = self.inner.connected.load(Ordering::SeqCst);
            let health = ConnectorHealth {
                connector_ref: self.connector_ref.clone(),
                state: if connected {
                    ConnectorConnectionState::Connected
                } else {
                    ConnectorConnectionState::Disconnected
                },
                detail: Some("fixture connector".into()),
                provenance: Vec::new(),
            };
            let result: Result<ConnectorHealth> = Ok(health);
            Box::pin(async move { result })
        }

        fn disconnect(&mut self) -> ConnectorFuture<'_, ()> {
            self.inner.disconnects.fetch_add(1, Ordering::SeqCst);
            self.inner.connected.store(false, Ordering::SeqCst);
            let result: Result<()> = Ok(());
            Box::pin(async move { result })
        }
    }

    pub(crate) struct FixtureFactory {
        pub entry: GatewayConnectorEntry,
        pub inner: Arc<FixtureInner>,
    }

    impl GatewayConnectorFactory for FixtureFactory {
        fn entry(&self) -> &GatewayConnectorEntry {
            &self.entry
        }

        fn build(&self) -> Result<Box<dyn GatewayConnector>> {
            Ok(Box::new(fixture_connector(Arc::clone(&self.inner))))
        }
    }

    pub(crate) fn fixture_entry() -> GatewayConnectorEntry {
        GatewayConnectorEntry {
            connector_ref: CONNECTOR_REF.into(),
            platform: PLATFORM.into(),
            implementation: "fixture".into(),
            enabled: true,
            token_location: None,
            configuration_ref: None,
            program: Vec::new(),
            provenance: Vec::new(),
        }
    }

    pub(crate) fn fixture_descriptor() -> ConnectorDescriptor {
        use crate::gateway_connector::{ConnectorCapabilities, ConnectorOperation};
        ConnectorDescriptor {
            version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
            platform: PLATFORM.into(),
            implementation: "fixture".into(),
            capabilities: ConnectorCapabilities {
                operations: [ConnectorOperation::Send, ConnectorOperation::Typing]
                    .into_iter()
                    .collect(),
                max_text_bytes: None,
                max_media_bytes: None,
                media_types: Default::default(),
                provenance: vec!["fixture".into()],
            },
            configuration_ref: None,
            provenance: vec!["fixture".into()],
        }
    }

    pub(crate) fn fixture_event(text: &str) -> InboundEvent {
        InboundEvent {
            event_ref: ResourceRef::parse(format!("gateway-ingress/fixture/{text}")).unwrap(),
            connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
            address: ConversationAddress {
                platform: PLATFORM.into(),
                scope_id: None,
                conversation_id: "chat-1".into(),
                thread_id: None,
            },
            sender: SenderIdentity {
                native_sender_id: "fixture-user".into(),
                kind: crate::gateway_connector::SenderKind::Human,
                display_name: None,
                metadata: Default::default(),
            },
            kind: InboundEventKind::Message,
            custom_kind: None,
            native_event_id: Some(format!("fixture-native-{text}")),
            native_message_id: Some(format!("fixture-message-{text}")),
            reply_to_native_message_id: None,
            text: Some(text.to_owned()),
            media: Vec::new(),
            observed_at: None,
            native: Default::default(),
            provenance: vec!["fixture".into()],
        }
    }

    /// Pre-seed the kernel with the connector and its binding so the pump's
    /// ingests append, exactly as a deployed gateway's restored state would.
    pub(crate) fn seed_binding(gateway: &mut AgencyGateway) {
        gateway.register_connector(fixture_descriptor()).unwrap();
        gateway
            .bind(GatewayBinding {
                binding_ref: ResourceRef::parse(BINDING_REF).unwrap(),
                connector_ref: ResourceRef::parse(CONNECTOR_REF).unwrap(),
                address: fixture_event("any").address,
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
                provenance: Vec::new(),
            })
            .unwrap();
    }

    struct Harness {
        gateway: Arc<Mutex<AgencyGateway>>,
        shutdown: Arc<AtomicBool>,
        hub: Arc<SubscriptionHub>,
        queues: Arc<ConnectorQueues>,
        state_file: std::path::PathBuf,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let gateway = AgencyGateway::new(ResourceRef::parse("agency-gateway/fixture").unwrap());
            let harness = Self {
                gateway: Arc::new(Mutex::new(gateway)),
                shutdown: Arc::new(AtomicBool::new(false)),
                hub: Arc::new(SubscriptionHub::default()),
                queues: Arc::new(ConnectorQueues::default()),
                state_file: dir.path().join("gateway.json"),
                _dir: dir,
            };
            seed_binding(&mut harness.gateway.lock().unwrap());
            harness
        }

        fn spawn_worker(&self, inner: Arc<FixtureInner>) -> JoinHandle<()> {
            let mut workers = spawn_connector_workers(
                Arc::clone(&self.gateway),
                Arc::clone(&self.shutdown),
                Some(self.state_file.clone()),
                Arc::clone(&self.hub),
                Arc::clone(&self.queues),
                vec![Box::new(FixtureFactory {
                    entry: fixture_entry(),
                    inner,
                })],
            );
            assert_eq!(workers.len(), 1, "one enabled factory, one worker");
            workers.remove(0)
        }

        fn status(&self) -> crate::gateway_runtime::GatewayStatus {
            self.gateway.lock().unwrap().status()
        }

        fn health(&self) -> Option<ConnectorHealth> {
            self.status().connector_health.into_iter().next()
        }

        fn stream_events(&self) -> Vec<crate::gateway_runtime::GatewayStreamEvent> {
            self.gateway
                .lock()
                .unwrap()
                .snapshot()
                .streams
                .into_iter()
                .flat_map(|stream| stream.events)
                .collect()
        }

        fn wait_until(&self, what: &str, condition: impl Fn(&Self) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if condition(self) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("timed out waiting for {what}");
        }
    }

    #[test]
    fn pump_turns_connector_events_into_kernel_state_and_a_persisted_snapshot() {
        let harness = Harness::new();
        let inner = FixtureInner::new();
        inner.push_text("one");
        inner.push_text("two");
        // An outbound operation prepared the way a carrier would, queued for
        // the connector's worker before it starts.
        let operation = harness
            .gateway
            .lock()
            .unwrap()
            .prepare_operation(
                &ResourceRef::parse(BINDING_REF).unwrap(),
                OutboundOperationKind::Send {
                    text: Some("outbound-one".into()),
                    media: Vec::new(),
                    reply_to_native_message_id: None,
                },
            )
            .unwrap();
        harness
            .queues
            .queue_for(&ResourceRef::parse(CONNECTOR_REF).unwrap())
            .push(operation);

        let worker = harness.spawn_worker(Arc::clone(&inner));
        harness.wait_until("both events ingested and the receipt recorded", |harness| {
            harness.status().delivery_receipt_count == 1
                && harness.stream_events().len() == 2
        });
        harness.shutdown.store(true, Ordering::SeqCst);
        worker.join().unwrap();

        let events = harness.stream_events();
        assert_eq!(events[0].event["content"], "one");
        assert_eq!(events[1].event["content"], "two");
        assert_eq!(
            events[0].event["metadata"]["connector_event_ref"],
            "gateway-ingress/fixture/one"
        );
        assert_eq!(harness.status().pending_delivery_count, 0);
        // The pump persists state like a carrier does: the file carries the
        // appended events and the recorded receipt.
        let persisted = std::fs::read_to_string(&harness.state_file).unwrap();
        assert!(persisted.contains("gateway-ingress/fixture/one"));
        assert!(persisted.contains("fixture executed the operation"));
        // The queue's operation really reached the connector.
        assert_eq!(inner.executed.lock().unwrap().len(), 1);
    }

    #[test]
    fn duplicate_native_events_are_ingested_once_and_counted_in_health() {
        let harness = Harness::new();
        let inner = FixtureInner::new();
        inner.push_text("dupe");
        inner.push_text("dupe");
        inner.push_text("fresh");

        let worker = harness.spawn_worker(Arc::clone(&inner));
        harness.wait_until("the fresh event lands after the duplicate is skipped", |harness| {
            let events = harness.stream_events();
            events.len() == 2
                && events.iter().any(|event| event.event["content"] == "fresh")
        });
        harness.wait_until("the skip is visible in connector health", |harness| {
            harness
                .health()
                .and_then(|health| health.detail)
                .is_some_and(|detail| detail.contains("1 duplicate events skipped"))
        });
        harness.shutdown.store(true, Ordering::SeqCst);
        worker.join().unwrap();

        let events = harness.stream_events();
        assert_eq!(events[0].event["content"], "dupe");
        assert_eq!(events[1].event["content"], "fresh");
    }

    #[test]
    fn health_follows_connect_disconnect_and_reconnect() {
        let harness = Harness::new();
        let inner = FixtureInner::new();
        inner.fail_next_connects(2);
        inner.push_text("after-reconnect");

        let worker = harness.spawn_worker(Arc::clone(&inner));
        harness.wait_until("the first connect failure is recorded", |harness| {
            matches!(
                harness.health().map(|health| health.state),
                Some(ConnectorConnectionState::Unavailable)
            )
        });
        harness.wait_until("the connector recovers and ingests", |harness| {
            matches!(
                harness.health().map(|health| health.state),
                Some(ConnectorConnectionState::Connected)
            ) && harness.stream_events().len() == 1
        });

        // The connector ends its stream: health names the reconnecting state,
        // the worker reconnects, and the service never went down.
        inner.push_disconnect();
        harness.wait_until("the ended stream is reported", |harness| {
            matches!(
                harness.health().map(|health| health.state),
                Some(ConnectorConnectionState::Reconnecting)
            )
        });
        assert!(inner.disconnects.load(Ordering::SeqCst) >= 1);
        harness.shutdown.store(true, Ordering::SeqCst);
        worker.join().unwrap();
    }

    #[test]
    fn a_connector_that_never_connects_leaves_the_service_up_with_named_health() {
        let harness = Harness::new();
        let inner = FixtureInner::new();
        inner.fail_next_connects(usize::MAX);

        let worker = harness.spawn_worker(Arc::clone(&inner));
        harness.wait_until("the connector gives up with a named detail", |harness| {
            harness
                .health()
                .and_then(|health| health.detail)
                .is_some_and(|detail| detail.contains("gave up after 5 connect attempts"))
        });
        assert_eq!(
            harness.health().unwrap().state,
            ConnectorConnectionState::Unavailable
        );
        // The kernel carries no phantom ingress.
        assert!(harness.stream_events().is_empty());
        harness.shutdown.store(true, Ordering::SeqCst);
        worker.join().unwrap();
    }

    /// A Telegram transport that answers getMe, one getUpdates with a real
    /// message update, then sendMessage for the prepared operation.
    struct TelegramScriptedTransport;

    impl crate::telegram_bot_api::TelegramBotApiTransport for TelegramScriptedTransport {
        fn call(&mut self, method: &str, _params: serde_json::Value) -> Result<serde_json::Value> {
            match method {
                "getMe" => Ok(json!({"ok": true, "result": {"id": 7, "is_bot": true}})),
                "getUpdates" => Ok(json!({
                    "ok": true,
                    "result": [{
                        "update_id": 500,
                        "message": {
                            "message_id": 51,
                            "date": 1,
                            "chat": {"id": 42, "type": "private"},
                            "from": {"id": 9, "is_bot": false, "first_name": "Ada"},
                            "text": "hello from telegram"
                        }
                    }]
                })),
                "sendMessage" => Ok(json!({"ok": true, "result": {"message_id": 52}})),
                other => Err(AikitError::new(
                    "telegram_gateway_fixture.unexpected",
                    format!("unexpected scripted method {other}"),
                )),
            }
        }
    }

    fn telegram_config() -> TelegramConnectorConfig {
        TelegramConnectorConfig {
            connector_ref: ResourceRef::parse("gateway-connector/telegram/main").unwrap(),
            configuration_ref: None,
            poll_timeout_seconds: 0,
            allowed_updates: Vec::new(),
            provenance: Vec::new(),
        }
    }

    #[test]
    fn the_pump_drives_the_telegram_connector_end_to_end() {
        let harness = Harness::new();
        let mut connector =
            TelegramConnector::new(TelegramScriptedTransport, telegram_config()).unwrap();
        // The connector's own lifecycle: connect, then one real update.
        harness
            .gateway
            .lock()
            .unwrap()
            .register_connector(connector.descriptor())
            .unwrap();
        let hello = block_on(connector.connect()).unwrap();
        hello.validate().unwrap();
        let event = block_on(connector.next_event()).unwrap().unwrap();
        event.validate(&hello.descriptor).unwrap();
        harness
            .gateway
            .lock()
            .unwrap()
            .bind(GatewayBinding {
                binding_ref: ResourceRef::parse("gateway-binding/telegram").unwrap(),
                connector_ref: ResourceRef::parse("gateway-connector/telegram/main").unwrap(),
                address: event.address.clone(),
                agent_session_ref: ResourceRef::parse("agent-session/telegram").unwrap(),
                agency_ref: ResourceRef::parse("agency/telegram").unwrap(),
                actuation_ref: ResourceRef::parse("actuation/telegram").unwrap(),
                actuation_stream_ref: ResourceRef::parse("actuation-stream/telegram").unwrap(),
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
        let context = WorkerContext {
            gateway: Arc::clone(&harness.gateway),
            shutdown: Arc::new(AtomicBool::new(false)),
            state_file: Some(harness.state_file.clone()),
            hub: Arc::clone(&harness.hub),
            queue: harness
                .queues
                .queue_for(&ResourceRef::parse("gateway-connector/telegram/main").unwrap()),
        };
        // Ingest through the exact command path a pump iteration uses.
        locked_command(&context, GatewayCommand::Ingest { event }).unwrap();
        // A Telegram Send rides the same prepare→execute→record path.
        let operation = harness
            .gateway
            .lock()
            .unwrap()
            .prepare_operation(
                &ResourceRef::parse("gateway-binding/telegram").unwrap(),
                OutboundOperationKind::Send {
                    text: Some("reply".into()),
                    media: Vec::new(),
                    reply_to_native_message_id: None,
                },
            )
            .unwrap();
        let receipt = block_on(connector.execute(operation)).unwrap();
        assert_eq!(receipt.state, DeliveryState::Delivered);
        locked_command(&context, GatewayCommand::RecordDelivery { receipt }).unwrap();
        let _ = block_on(connector.disconnect());
        let events: Vec<_> = harness
            .gateway
            .lock()
            .unwrap()
            .snapshot()
            .streams
            .into_iter()
            .flat_map(|stream| stream.events)
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event["content"], "hello from telegram");
        assert_eq!(events[0].event["metadata"]["native_sender_id"], "9");
        assert_eq!(harness.status().delivery_receipt_count, 1);
        assert!(std::fs::read_to_string(&harness.state_file)
            .unwrap()
            .contains("hello from telegram"));
    }
}
