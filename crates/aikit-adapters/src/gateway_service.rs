//! Persistent carriers for the AIKit Agency Gateway service body.
//!
//! Gateway semantics remain in `gateway_runtime`; this module only materialises
//! the already-versioned request/response protocol over durable process carriers.
//! Workcell can therefore start/observe/release `aikit-gateway` as an ordinary
//! managed service without importing AgentSession, ActuationStream or connector
//! semantics.
//!
//! Network carrier: RFC 6455 WebSocket with bearer authentication during the HTTP
//! upgrade. Same-host carrier: Unix-domain socket with owner-only filesystem
//! permissions. Both execute the same [`GatewayRequestEnvelope`] commands as the
//! existing stdio carrier.

use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::gateway_connector_config::GatewayConnectorFactory;
use crate::gateway_connector_pump::{spawn_connector_workers, ConnectorQueues};
use crate::gateway_runtime::{
    execute_gateway_command, AgencyGateway, GatewayCommand, GatewayIngressResult,
    GatewayOccupancyReading, GatewayRequestEnvelope, GatewayResponse, GatewayResponseEnvelope,
    GatewayStreamEvent,
};

pub const GATEWAY_SERVICE_CARRIER_VERSION: &str = "aikit.gateway-service-carrier/v1";
pub const DEFAULT_GATEWAY_MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
pub(crate) const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayServiceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websocket_bind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websocket_bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unix_socket: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_file: Option<PathBuf>,
    #[serde(default = "default_max_frame_bytes")]
    pub max_frame_bytes: usize,
}

fn default_max_frame_bytes() -> usize {
    DEFAULT_GATEWAY_MAX_FRAME_BYTES
}

impl GatewayServiceConfig {
    pub fn validate(&self) -> Result<()> {
        if self.websocket_bind.is_none() && self.unix_socket.is_none() {
            return Err(AikitError::new(
                "agency_gateway_service.no_carrier",
                "gateway service mode requires --ws and/or --unix",
            ));
        }
        if self.websocket_bind.is_some()
            && self
                .websocket_bearer_token
                .as_deref()
                .is_none_or(|token| token.trim().is_empty())
        {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_auth_required",
                "network WebSocket carrier requires a non-empty bearer token",
            ));
        }
        if self
            .websocket_bind
            .as_deref()
            .is_some_and(|bind| bind.trim().is_empty())
        {
            return Err(AikitError::new(
                "agency_gateway_service.empty_websocket_bind",
                "WebSocket bind address must not be empty",
            ));
        }
        if self.max_frame_bytes == 0 {
            return Err(AikitError::new(
                "agency_gateway_service.invalid_frame_limit",
                "gateway WebSocket frame limit must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// Load a previously persisted semantic gateway snapshot when present.
///
/// The persisted document is the canonical gateway snapshot itself. It contains
/// no PID, socket or Workcell allocation identity.
pub fn restore_gateway_state(
    fresh: AgencyGateway,
    state_file: Option<&Path>,
) -> Result<AgencyGateway> {
    let Some(path) = state_file else {
        return Ok(fresh);
    };
    if !path.exists() {
        return Ok(fresh);
    }
    let content = fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_read",
            format!("read gateway state {}: {error}", path.display()),
        )
    })?;
    let snapshot = serde_json::from_str(&content).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_decode",
            format!("decode gateway state {}: {error}", path.display()),
        )
    })?;
    AgencyGateway::from_snapshot(snapshot)
}

pub fn persist_gateway_state(gateway: &AgencyGateway, state_file: Option<&Path>) -> Result<()> {
    let Some(path) = state_file else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_directory",
                format!(
                    "create gateway state directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let encoded = serde_json::to_vec_pretty(&gateway.snapshot()).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_encode",
            format!("encode gateway state: {error}"),
        )
    })?;
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("json")
    ));
    {
        let mut file = fs::File::create(&tmp).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_write",
                format!("create gateway state {}: {error}", tmp.display()),
            )
        })?;
        file.write_all(&encoded).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_write",
                format!("write gateway state {}: {error}", tmp.display()),
            )
        })?;
        file.sync_all().map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_sync",
                format!("sync gateway state {}: {error}", tmp.display()),
            )
        })?;
    }
    fs::rename(&tmp, path).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.state_replace",
            format!(
                "replace gateway state {} from {}: {error}",
                path.display(),
                tmp.display()
            ),
        )
    })?;
    Ok(())
}

/// An advisory, kernel-held lock on one gateway state file.
///
/// A running service holds it for its whole lifetime; an offline writer (a
/// CLI verb executing a command straight against the state file because no
/// service answered) holds it for one command. The two can therefore never
/// interleave: the service keeps its state in memory and rewrites the whole
/// file after every command, so an offline write under a running service would
/// be silently lost. The lock is released by the kernel when the holder exits,
/// so a crashed holder never wedges the file.
pub struct GatewayStateLock {
    _file: fs::File,
    path: PathBuf,
}

impl GatewayStateLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn state_lock_path(state_file: &Path) -> PathBuf {
    let mut name = state_file
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "gateway.json".into());
    name.push(".lock");
    state_file.with_file_name(name)
}

/// Take the state file's lock, polling until `timeout`. A holder that does not
/// let go in time is reported by what it wrote into the lock file.
pub fn acquire_gateway_state_lock(
    state_file: &Path,
    timeout: Duration,
    purpose: &str,
) -> Result<GatewayStateLock> {
    use fs4::FileExt;

    let path = state_lock_path(state_file);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_directory",
                format!(
                    "create gateway state directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| {
            AikitError::new(
                "agency_gateway_service.state_lock",
                format!("open gateway state lock {}: {error}", path.display()),
            )
        })?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match FileExt::try_lock(&file) {
            Ok(()) => break,
            Err(fs4::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= deadline {
                    let mut holder = String::new();
                    let _ = fs::File::open(&path).and_then(|mut f| f.read_to_string(&mut holder));
                    return Err(AikitError::new(
                        "agency_gateway_service.state_locked",
                        format!(
                            "gateway state {} is held by another process ({})",
                            state_file.display(),
                            if holder.trim().is_empty() {
                                "holder unrecorded".to_owned()
                            } else {
                                holder.trim().to_owned()
                            }
                        ),
                    )
                    .with("state_file", state_file.display().to_string()));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(fs4::TryLockError::Error(error)) => {
                return Err(AikitError::new(
                    "agency_gateway_service.state_lock",
                    format!("lock gateway state {}: {error}", path.display()),
                ))
            }
        }
    }
    let record = format!("pid {} ({purpose})", std::process::id());
    let _ = file
        .set_len(0)
        .and_then(|()| {
            use std::io::Seek;
            file.seek(io::SeekFrom::Start(0)).map(|_| ())
        })
        .and_then(|()| file.write_all(record.as_bytes()));
    Ok(GatewayStateLock { _file: file, path })
}

/// Execute one gateway command directly against a persisted state file, for
/// when no service is running. Restores the semantic snapshot, runs the same
/// kernel command a carrier would, and persists only on success — under the
/// state lock, so a service that starts meanwhile waits for this write.
pub fn execute_against_state_file(
    fresh: AgencyGateway,
    state_file: &Path,
    command: GatewayCommand,
    lock_timeout: Duration,
) -> Result<GatewayResponse> {
    let _lock = acquire_gateway_state_lock(state_file, lock_timeout, "offline gateway command")?;
    let mut gateway = restore_gateway_state(fresh, Some(state_file))?;
    let read_only = command.is_read_only();
    let response = execute_gateway_command(&mut gateway, command)?;
    if !read_only {
        persist_gateway_state(&gateway, Some(state_file))?;
    }
    Ok(response)
}

/// Run every configured service carrier against one shared gateway state.
pub fn run_gateway_service(gateway: AgencyGateway, config: GatewayServiceConfig) -> Result<()> {
    run_gateway_service_with_ticks(gateway, config, None)
}

/// A periodic hook the service runs beside its carriers: the Routine
/// dispatcher's tick. The hook never touches gateway state and its failures
/// are remembered, never fatal — a failed scheduling pass must not take the
/// carrier down.
pub trait GatewayTick: Send + 'static {
    fn tick(&self) -> Result<serde_json::Value>;
}

/// The tick loop's configuration.
pub struct GatewayTickLoop {
    pub interval: std::time::Duration,
    pub hook: Box<dyn GatewayTick>,
}

/// The serving gateway's own Workcell occupancy, read from that Workcell's
/// occupancy owner (Actuation) at the moment a peer asks. The gateway keeps no
/// copy: a reader answers each `occupancy-read`/`occupancy-list` command fresh,
/// outside the gateway state lock, and never writes gateway state. It always
/// returns a reading; an owner that could not answer is recorded inside it.
pub trait GatewayOccupancyReader: Send + Sync + 'static {
    /// `position_ref: None` asks for the whole listing.
    fn read(&self, gateway_ref: &str, position_ref: Option<&str>) -> GatewayOccupancyReading;
}

/// What runs beside the carriers: the periodic tick, the occupancy reader and
/// the connector factories whose pumps drive the configured connectors.
#[derive(Default)]
pub struct GatewayServiceHooks {
    pub ticks: Option<GatewayTickLoop>,
    pub occupancy: Option<Arc<dyn GatewayOccupancyReader>>,
    pub connectors: Vec<Box<dyn GatewayConnectorFactory>>,
}

// ---------------------------------------------------------------------------
// Live event subscription
// ---------------------------------------------------------------------------

/// One subscribed connection's outbound queue. The carrier writes responses on
/// its own thread; a small writer thread drains this queue onto the connection
/// once the replay answer has been written (the gate), so a subscriber sees
/// replay first and then every appended event, in order, with no gaps.
pub struct SubscriptionSink {
    gate: Mutex<bool>,
    gate_signal: Condvar,
    queue: Mutex<VecDeque<String>>,
    queue_signal: Condvar,
    closed: std::sync::atomic::AtomicBool,
}

impl SubscriptionSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            gate: Mutex::new(false),
            gate_signal: Condvar::new(),
            queue: Mutex::new(VecDeque::new()),
            queue_signal: Condvar::new(),
            closed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Called by the carrier once the subscribe's replay answer is on the wire.
    pub fn open_gate(&self) {
        if let Ok(mut gate) = self.gate.lock() {
            *gate = true;
            self.gate_signal.notify_all();
        }
    }

    fn push(&self, frame: String) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(frame);
            self.queue_signal.notify_one();
        }
    }

    fn wait_open(&self) {
        let mut gate = self.gate.lock().expect("subscription gate");
        while !*gate && !self.closed.load(Ordering::SeqCst) {
            gate = self.gate_signal.wait(gate).expect("subscription gate");
        }
    }

    /// Blocks until frames are queued or the sink closed. `None` means closed.
    fn wait_frames(&self) -> Option<Vec<String>> {
        let mut queue = self.queue.lock().expect("subscription queue");
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return None;
            }
            if !queue.is_empty() {
                return Some(queue.drain(..).collect());
            }
            queue = self.queue_signal.wait(queue).ok()?;
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.queue_signal.notify_all();
        self.gate_signal.notify_all();
    }
}

/// The in-process broadcast registry. Updated when events are appended — under
/// the gateway state lock, so a subscriber registered before an append always
/// receives it and registration can never miss the events replay already
/// returned.
pub struct SubscriptionHub {
    inner: Mutex<HubInner>,
}

struct HubInner {
    next_id: u64,
    entries: BTreeMap<u64, HubEntry>,
}

struct HubEntry {
    stream_ref: ResourceRef,
    sink: Arc<SubscriptionSink>,
}

impl Default for SubscriptionHub {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HubInner {
                next_id: 1,
                entries: BTreeMap::new(),
            }),
        }
    }
}

impl SubscriptionHub {
    /// Register a subscriber. Called while the gateway state lock is held.
    fn register(&self, stream_ref: ResourceRef, sink: Arc<SubscriptionSink>) -> u64 {
        let mut inner = self.inner.lock().expect("subscription hub");
        let id = inner.next_id;
        inner.next_id += 1;
        inner.entries.insert(id, HubEntry { stream_ref, sink });
        id
    }

    fn unregister(&self, id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.entries.remove(&id);
        }
    }

    /// Publish an appended stream event to every subscriber of that stream.
    pub fn publish(&self, stream_ref: &ResourceRef, event: &GatewayStreamEvent) {
        let frame = {
            let response = GatewayResponseEnvelope::from_result(
                None,
                Ok(GatewayResponse::StreamEvent {
                    stream_ref: stream_ref.clone(),
                    event: event.clone(),
                }),
            );
            match serde_json::to_string(&response) {
                Ok(encoded) => encoded,
                Err(_) => return,
            }
        };
        if let Ok(inner) = self.inner.lock() {
            for entry in inner.entries.values() {
                if &entry.stream_ref == stream_ref {
                    entry.sink.push(frame.clone());
                }
            }
        }
    }
}

/// A connection's live subscription. Dropping it unregisters the subscriber
/// and closes its sink (which ends the connection's writer thread).
struct ConnectionSubscription {
    hub: Arc<SubscriptionHub>,
    id: u64,
    sink: Arc<SubscriptionSink>,
}

impl Drop for ConnectionSubscription {
    fn drop(&mut self) {
        self.hub.unregister(self.id);
        self.sink.close();
    }
}

/// Per-connection subscription state held by the carrier loops.
struct ConnectionSubscriptions {
    hub: Arc<SubscriptionHub>,
    active: Option<ConnectionSubscription>,
}

impl ConnectionSubscriptions {
    fn new(hub: Arc<SubscriptionHub>) -> Self {
        Self {
            hub,
            active: None,
        }
    }

    /// Subscribe this connection to `stream_ref`. One live subscription per
    /// connection: a re-Subscribe replaces the previous one. Called under the
    /// gateway state lock so the replay answer and the registration are one
    /// indivisible moment.
    fn subscribe(&mut self, stream_ref: ResourceRef) -> Arc<SubscriptionSink> {
        self.active = None;
        let sink = SubscriptionSink::new();
        let id = self.hub.register(stream_ref, Arc::clone(&sink));
        self.active = Some(ConnectionSubscription { hub: Arc::clone(&self.hub), id, sink: Arc::clone(&sink) });
        sink
    }
}

/// Spawn the writer thread that drains a subscription sink onto one
/// connection. It waits for the replay gate, then writes frames as they come,
/// and ends when the sink closes or a write fails.
fn spawn_subscription_writer(
    sink: Arc<SubscriptionSink>,
    mut write: impl FnMut(&str) -> Result<()> + Send + 'static,
) -> JoinHandle<()> {
    thread::spawn(move || {
        sink.wait_open();
        loop {
            let Some(frames) = sink.wait_frames() else {
                return;
            };
            for frame in frames {
                if write(&frame).is_err() {
                    return;
                }
            }
        }
    })
}

/// Run every configured service carrier against one shared gateway state,
/// with an optional periodic tick loop.
pub fn run_gateway_service_with_ticks(
    gateway: AgencyGateway,
    config: GatewayServiceConfig,
    ticks: Option<GatewayTickLoop>,
) -> Result<()> {
    run_gateway_service_with_hooks(
        gateway,
        config,
        GatewayServiceHooks {
            ticks,
            occupancy: None,
            connectors: Vec::new(),
        },
    )
}

/// What the carriers and the connector pumps share beside the gateway state.
pub struct GatewayServiceRuntime {
    pub hub: Arc<SubscriptionHub>,
    pub queues: Arc<ConnectorQueues>,
    pub connections: ConnectionRegistry,
}

/// One live carrier connection, closeable from the service's exit path.
trait ConnectionCloser: Send + Sync {
    /// End the connection: the peer's pending read returns at once (EOF for a
    /// clean shutdown), instead of hanging on a handler thread that would
    /// otherwise hold the socket open forever.
    fn close(&self);
}

struct StreamCloser<S>(S);

impl ConnectionCloser for StreamCloser<TcpStream> {
    fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

#[cfg(unix)]
impl ConnectionCloser for StreamCloser<std::os::unix::net::UnixStream> {
    fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

/// The service's live connections. When the service stops it closes them, so
/// a subscriber learns the gateway ended from its socket ending, never from
/// silence — its next read returns and it re-subscribes from the last
/// sequence it saw.
#[derive(Default)]
pub struct ConnectionRegistry {
    inner: Mutex<BTreeMap<u64, Arc<dyn ConnectionCloser>>>,
    next_id: AtomicU64,
}

impl ConnectionRegistry {
    fn register(&self, closer: Arc<dyn ConnectionCloser>) -> ConnectionGuard<'_> {
        let id = self
            .next_id
            .fetch_add(1, Ordering::SeqCst);
        if let Ok(mut inner) = self.inner.lock() {
            inner.insert(id, closer);
        }
        ConnectionGuard { registry: self, id }
    }

    fn unregister(&self, id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.remove(&id);
        }
    }

    /// Close every live connection. Guards still held drop afterwards and
    /// keep the registry clean.
    pub fn close_all(&self) {
        let live = self
            .inner
            .lock()
            .map(|inner| inner.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for closer in live {
            closer.close();
        }
    }
}

/// Unregisters its connection when the handler ends, however it ends.
struct ConnectionGuard<'a> {
    registry: &'a ConnectionRegistry,
    id: u64,
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        self.registry.unregister(self.id);
    }
}

/// Run every configured service carrier against one shared gateway state,
/// with the given hooks: tick loop, occupancy reader and connector pumps.
pub fn run_gateway_service_with_hooks(
    gateway: AgencyGateway,
    config: GatewayServiceConfig,
    hooks: GatewayServiceHooks,
) -> Result<()> {
    let GatewayServiceHooks {
        ticks,
        occupancy,
        connectors,
    } = hooks;
    config.validate()?;
    // Held until this function returns: the service is the only writer of its
    // state file while it runs (see `GatewayStateLock`).
    let _state_lock = match config.state_file.as_deref() {
        Some(path) => Some(acquire_gateway_state_lock(
            path,
            Duration::from_secs(10),
            "gateway service",
        )?),
        None => None,
    };
    let gateway = restore_gateway_state(gateway, config.state_file.as_deref())?;
    let gateway = Arc::new(Mutex::new(gateway));
    let shutdown = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(GatewayServiceRuntime {
        hub: Arc::new(SubscriptionHub::default()),
        queues: Arc::new(ConnectorQueues::default()),
        connections: ConnectionRegistry::default(),
    });

    // Connector pumps run beside the carriers. A pump failure never takes a
    // carrier down; workers stop when the shutdown flag is set, which every
    // carrier exit path sets.
    let connector_workers = spawn_connector_workers(
        Arc::clone(&gateway),
        Arc::clone(&shutdown),
        config.state_file.clone(),
        Arc::clone(&runtime.hub),
        Arc::clone(&runtime.queues),
        connectors,
    );

    let mut workers = Vec::new();

    let tick_loop: Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)> =
        ticks.map(|loop_config| {
            let tick_shutdown = Arc::new(AtomicBool::new(false));
            let handle = {
                let shutdown = Arc::clone(&shutdown);
                let tick_shutdown = Arc::clone(&tick_shutdown);
                thread::spawn(move || {
                    let GatewayTickLoop { interval, hook } = loop_config;
                    let mut last_error = None;
                    // Sleep in small steps so a carrier shutdown stops the loop
                    // promptly instead of waiting out the whole interval.
                    let step = Duration::from_millis(100).min(interval);
                    let mut until_next_tick = interval;
                    while !shutdown.load(Ordering::SeqCst) {
                        if until_next_tick == Duration::ZERO {
                            if let Err(error) = hook.tick() {
                                last_error = Some(error);
                            }
                            until_next_tick = interval;
                        }
                        thread::sleep(step);
                        until_next_tick = until_next_tick.saturating_sub(step);
                    }
                    let _ = last_error;
                    tick_shutdown.store(true, Ordering::SeqCst);
                })
            };
            (tick_shutdown, handle)
        });

    if let Some(bind) = config.websocket_bind.clone() {
        let token = config
            .websocket_bearer_token
            .clone()
            .expect("validated WebSocket bearer token");
        let listener = TcpListener::bind(&bind).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.websocket_bind",
                format!("bind WebSocket gateway at {bind}: {error}"),
            )
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.websocket_nonblocking",
                format!("configure WebSocket listener {bind}: {error}"),
            )
        })?;
        let gateway = Arc::clone(&gateway);
        let shutdown = Arc::clone(&shutdown);
        let state_file = config.state_file.clone();
        let max_frame_bytes = config.max_frame_bytes;
        let occupancy = occupancy.clone();
        let runtime = Arc::clone(&runtime);
        workers.push(thread::spawn(move || {
            let result = serve_websocket_listener(
                listener,
                gateway,
                Arc::clone(&shutdown),
                token,
                state_file,
                max_frame_bytes,
                occupancy,
                runtime,
            );
            // A carrier that fails stops the whole service: a half-alive
            // gateway answering on one carrier only is silent degradation.
            if result.is_err() {
                shutdown.store(true, Ordering::SeqCst);
            }
            result
        }));
    }

    #[cfg(unix)]
    if let Some(path) = config.unix_socket.clone() {
        let gateway = Arc::clone(&gateway);
        let shutdown = Arc::clone(&shutdown);
        let state_file = config.state_file.clone();
        let occupancy = occupancy.clone();
        let runtime = Arc::clone(&runtime);
        workers.push(thread::spawn(move || {
            let result = serve_unix_socket(
                path,
                gateway,
                Arc::clone(&shutdown),
                state_file,
                occupancy,
                runtime,
            );
            if result.is_err() {
                shutdown.store(true, Ordering::SeqCst);
            }
            result
        }));
    }

    #[cfg(not(unix))]
    if config.unix_socket.is_some() {
        return Err(AikitError::new(
            "agency_gateway_service.unix_unsupported",
            "Unix-domain gateway carrier is unavailable on this platform",
        ));
    }

    let mut first_error = None;
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                shutdown.store(true, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(_) => {
                shutdown.store(true, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(AikitError::new(
                        "agency_gateway_service.worker_panic",
                        "gateway service carrier worker panicked",
                    ));
                }
            }
        }
    }
    // Stop the tick loop and the connector pumps, and wait for both, before
    // the final state write.
    if let Some((tick_shutdown, handle)) = tick_loop {
        tick_shutdown.store(true, Ordering::SeqCst);
        let _ = handle.join();
    }
    for worker in connector_workers {
        let _ = worker.join();
    }
    // The service is ending: close every live carrier connection so a
    // subscriber's next read ends now and it re-subscribes from the last
    // sequence it saw, rather than waiting on a socket that outlives the
    // service.
    runtime.connections.close_all();
    if let Some(error) = first_error {
        return Err(error);
    }
    let gateway = gateway.lock().map_err(|_| {
        AikitError::new(
            "agency_gateway_service.poisoned",
            "gateway state lock was poisoned",
        )
    })?;
    persist_gateway_state(&gateway, config.state_file.as_deref())
}

type OccupancyHook = Option<Arc<dyn GatewayOccupancyReader>>;

fn serve_websocket_listener(
    listener: TcpListener,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    token: String,
    state_file: Option<PathBuf>,
    max_frame_bytes: usize,
    occupancy: OccupancyHook,
    runtime: Arc<GatewayServiceRuntime>,
) -> Result<()> {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                stream.set_nonblocking(false).map_err(|error| {
                    AikitError::new(
                        "agency_gateway_service.websocket_connection_blocking",
                        format!(
                            "configure accepted WebSocket gateway connection as blocking: {error}"
                        ),
                    )
                })?;
                let gateway = Arc::clone(&gateway);
                let shutdown = Arc::clone(&shutdown);
                let token = token.clone();
                let state_file = state_file.clone();
                let occupancy = occupancy.clone();
                let runtime = Arc::clone(&runtime);
                thread::spawn(move || {
                    let _ = handle_websocket_connection(
                        stream,
                        gateway,
                        shutdown,
                        &token,
                        state_file.as_deref(),
                        max_frame_bytes,
                        occupancy.as_deref(),
                        runtime,
                    );
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                return Err(AikitError::new(
                    "agency_gateway_service.websocket_accept",
                    format!("accept gateway WebSocket connection: {error}"),
                ));
            }
        }
    }
    Ok(())
}

fn handle_websocket_connection(
    stream: TcpStream,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    token: &str,
    state_file: Option<&Path>,
    max_frame_bytes: usize,
    occupancy: Option<&dyn GatewayOccupancyReader>,
    runtime: Arc<GatewayServiceRuntime>,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(io_error("configure WebSocket read timeout"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(io_error("configure WebSocket write timeout"))?;
    // Registered so the service's exit closes this connection: a subscriber's
    // next read ends instead of outliving the service.
    let _connection = runtime.connections.register(match stream.try_clone() {
        Ok(clone) => Arc::new(StreamCloser(clone)),
        Err(_) => Arc::new(NullCloser),
    });
    let writer = stream
        .try_clone()
        .map_err(io_error("clone WebSocket stream"))?;
    // Responses and subscription pushes come from two threads; every write
    // goes through this one lock so frames never interleave mid-write.
    let writer = Arc::new(Mutex::new(writer));
    let mut reader = BufReader::new(stream);
    let mut subscriptions = ConnectionSubscriptions::new(Arc::clone(&runtime.hub));
    let push_writer: Option<JoinHandle<()>> = None;
    websocket_handshake(&mut reader, &mut *writer.lock().expect("writer"), token)?;
    let mut push_writer = push_writer;

    loop {
        let frame = match read_websocket_frame(&mut reader, max_frame_bytes) {
            Ok(frame) => frame,
            Err(error)
                if is_idle_timeout(&error) && subscriptions.active.is_some() =>
            {
                // A subscribed client may sit silent waiting for events;
                // keep the connection alive with a protocol ping.
                let mut writer = writer.lock().expect("writer");
                write_websocket_frame(&mut *writer, 0x9, b"gateway")?;
                continue;
            }
            Err(error) if error.code() == "agency_gateway_service.websocket_eof" => return Ok(()),
            Err(error) => {
                let mut writer = writer.lock().expect("writer");
                let _ = write_websocket_close(&mut *writer, 1002, "protocol error");
                return Err(error);
            }
        };
        match frame.opcode {
            0x1 => {
                let text = String::from_utf8(frame.payload).map_err(|error| {
                    AikitError::new(
                        "agency_gateway_service.websocket_utf8",
                        format!("WebSocket text frame is not UTF-8: {error}"),
                    )
                })?;
                let (response, should_shutdown, subscribed) = execute_serialized_request(
                    &gateway,
                    &runtime,
                    &text,
                    state_file,
                    occupancy,
                    &mut subscriptions,
                )?;
                // The shutdown lands before the answer: a client that shuts
                // the gateway down may hang up without waiting for it, and a
                // failed answer to a gone peer must never strand the shutdown.
                if should_shutdown {
                    shutdown.store(true, Ordering::SeqCst);
                }
                {
                    let write = write_websocket_text(
                        &mut *writer.lock().expect("writer"),
                        response.as_bytes(),
                    );
                    if let Err(error) = write {
                        if !should_shutdown {
                            return Err(error);
                        }
                    }
                }
                if let Some(sink) = subscribed {
                    // The replay answer is on the wire; open the gate so the
                    // writer can begin, and start it if this is the first.
                    sink.open_gate();
                    if push_writer.is_none() {
                        let writer = Arc::clone(&writer);
                        push_writer = Some(spawn_subscription_writer(sink, move |frame| {
                            let mut writer = writer.lock().expect("writer");
                            write_websocket_text(&mut *writer, frame.as_bytes())
                        }));
                    }
                }
                if should_shutdown {
                    let mut writer = writer.lock().expect("writer");
                    let _ = write_websocket_close(&mut *writer, 1000, "gateway shutdown");
                    return Ok(());
                }
            }
            0x8 => {
                let mut writer = writer.lock().expect("writer");
                write_websocket_close(&mut *writer, 1000, "closing")?;
                return Ok(());
            }
            0x9 => {
                let mut writer = writer.lock().expect("writer");
                write_websocket_frame(&mut *writer, 0xA, &frame.payload)?
            }
            0xA => {}
            _ => {
                let mut writer = writer.lock().expect("writer");
                write_websocket_close(&mut *writer, 1003, "unsupported frame")?;
                return Ok(());
            }
        }
    }
}

fn is_idle_timeout(error: &AikitError) -> bool {
    error.code() == "agency_gateway_service.io"
        && error
            .to_string()
            .contains("read WebSocket frame header")
}

#[cfg(unix)]
fn serve_unix_socket(
    path: PathBuf,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    state_file: Option<PathBuf>,
    occupancy: OccupancyHook,
    runtime: Arc<GatewayServiceRuntime>,
) -> Result<()> {
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    if path.exists() {
        fs::remove_file(&path).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.unix_remove_stale",
                format!(
                    "remove stale Unix gateway socket {}: {error}",
                    path.display()
                ),
            )
        })?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.unix_directory",
                format!(
                    "create Unix gateway socket directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let listener = UnixListener::bind(&path).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.unix_bind",
            format!("bind Unix gateway socket {}: {error}", path.display()),
        )
    })?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.unix_permissions",
            format!(
                "set Unix gateway socket permissions {}: {error}",
                path.display()
            ),
        )
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.unix_nonblocking",
            format!("configure Unix gateway socket {}: {error}", path.display()),
        )
    })?;

    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _address)) => {
                stream.set_nonblocking(false).map_err(|error| {
                    AikitError::new(
                        "agency_gateway_service.unix_connection_blocking",
                        format!("configure accepted Unix gateway connection as blocking: {error}"),
                    )
                })?;
                let gateway = Arc::clone(&gateway);
                let shutdown = Arc::clone(&shutdown);
                let state_file = state_file.clone();
                let occupancy = occupancy.clone();
                let runtime = Arc::clone(&runtime);
                thread::spawn(move || {
                    let _ = handle_line_connection(
                        stream,
                        gateway,
                        shutdown,
                        state_file.as_deref(),
                        occupancy.as_deref(),
                        runtime,
                    );
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = fs::remove_file(&path);
                return Err(AikitError::new(
                    "agency_gateway_service.unix_accept",
                    format!("accept Unix gateway connection: {error}"),
                ));
            }
        }
    }
    drop(listener);
    let _ = fs::remove_file(path);
    Ok(())
}

/// Streams the line carrier can duplicate, giving a subscription writer its
/// own write half beside the reader, plus a service-exit closer.
trait DuplexStream: Read + Write + Send + 'static {
    fn duplicate(&self) -> io::Result<Self>
    where
        Self: Sized;

    fn closer(&self) -> Arc<dyn ConnectionCloser>;
}

impl DuplexStream for TcpStream {
    fn duplicate(&self) -> io::Result<Self> {
        self.try_clone()
    }

    fn closer(&self) -> Arc<dyn ConnectionCloser> {
        match self.try_clone() {
            Ok(clone) => Arc::new(StreamCloser(clone)),
            Err(_) => Arc::new(NullCloser),
        }
    }
}

#[cfg(unix)]
impl DuplexStream for std::os::unix::net::UnixStream {
    fn duplicate(&self) -> io::Result<Self> {
        use std::os::unix::net::UnixStream;
        UnixStream::try_clone(self)
    }

    fn closer(&self) -> Arc<dyn ConnectionCloser> {
        match self.try_clone() {
            Ok(clone) => Arc::new(StreamCloser(clone)),
            Err(_) => Arc::new(NullCloser),
        }
    }
}

/// A closer for the rare case the duplicate itself failed: the connection
/// then ends with its handler thread, as before this registry existed.
struct NullCloser;

impl ConnectionCloser for NullCloser {
    fn close(&self) {}
}

fn handle_line_connection<S>(
    stream: S,
    gateway: Arc<Mutex<AgencyGateway>>,
    shutdown: Arc<AtomicBool>,
    state_file: Option<&Path>,
    occupancy: Option<&dyn GatewayOccupancyReader>,
    runtime: Arc<GatewayServiceRuntime>,
) -> Result<()>
where
    S: DuplexStream,
{
    // Responses and subscription pushes come from two threads; every write
    // goes through this one lock so lines never interleave mid-write.
    let writer = Arc::new(Mutex::new(stream.duplicate().map_err(io_error("duplicate gateway line stream"))?));
    // Registered so the service's exit closes this connection: a subscriber's
    // next read ends instead of outliving the service.
    let _connection = runtime.connections.register(stream.closer());
    let mut reader = BufReader::new(stream);
    let mut subscriptions = ConnectionSubscriptions::new(Arc::clone(&runtime.hub));
    let mut push_writer: Option<JoinHandle<()>> = None;
    loop {
        let mut line = String::new();
        let count = reader
            .read_line(&mut line)
            .map_err(io_error("read gateway line"))?;
        if count == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let (response, should_shutdown, subscribed) = execute_serialized_request(
            &gateway,
            &runtime,
            line.trim_end(),
            state_file,
            occupancy,
            &mut subscriptions,
        )?;
        // The shutdown lands before the answer: a client that shuts the
        // gateway down may hang up without waiting for the answer, and the
        // answer failing to a gone peer must never strand the shutdown.
        if should_shutdown {
            shutdown.store(true, Ordering::SeqCst);
        }
        {
            let mut writer = writer.lock().expect("gateway line writer");
            let write = writer
                .write_all(response.as_bytes())
                .and_then(|()| writer.write_all(b"\n"))
                .and_then(|()| writer.flush());
            if let Err(error) = write {
                if !should_shutdown {
                    return Err(io_error("write gateway line response")(error));
                }
            }
        }
        if let Some(sink) = subscribed {
            // The replay answer is on the wire; open the gate and start the
            // connection's subscription writer if this is the first.
            sink.open_gate();
            if push_writer.is_none() {
                let writer = Arc::clone(&writer);
                push_writer = Some(spawn_subscription_writer(sink, move |frame| {
                    let mut writer = writer.lock().expect("gateway line writer");
                    writer
                        .write_all(frame.as_bytes())
                        .map_err(io_error("write gateway push"))?;
                    writer
                        .write_all(b"\n")
                        .map_err(io_error("write gateway push terminator"))?;
                    writer
                        .flush()
                        .map_err(io_error("flush gateway push"))?;
                    Ok(())
                }));
            }
        }
        if should_shutdown {
            break;
        }
    }
    // Dropping the subscription unregisters it and closes the sink, which
    // ends the writer thread.
    drop(subscriptions);
    if let Some(handle) = push_writer {
        let _ = handle.join();
    }
    Ok(())
}

/// Execute one serialized carrier request against the shared gateway state.
///
/// Returns the response line, whether the service should shut down, and — for
/// a successful `subscribe` — the connection's new subscription sink. The
/// sink's gate opens once the carrier has written the replay answer, so a
/// subscriber always sees replay first, then live pushes, never a gap.
fn execute_serialized_request(
    gateway: &Arc<Mutex<AgencyGateway>>,
    runtime: &GatewayServiceRuntime,
    input: &str,
    state_file: Option<&Path>,
    occupancy: Option<&dyn GatewayOccupancyReader>,
    subscriptions: &mut ConnectionSubscriptions,
) -> Result<(String, bool, Option<Arc<SubscriptionSink>>)> {
    let request = match serde_json::from_str::<GatewayRequestEnvelope>(input) {
        Ok(request) => request,
        Err(error) => {
            let response = serde_json::json!({
                "request_id": null,
                "ok": false,
                "error": {
                    "code": "agency_gateway.invalid_request_json",
                    "message": error.to_string()
                }
            });
            return Ok((response.to_string(), false, None));
        }
    };
    // An occupancy query is the Workcell owner's answer, not gateway state:
    // read it outside the state lock so a slow owner never stalls the journal.
    if let (Some(position_ref), Some(reader)) = (request.command.occupancy_query(), occupancy) {
        let gateway_ref = gateway
            .lock()
            .map_err(|_| {
                AikitError::new(
                    "agency_gateway_service.poisoned",
                    "gateway state lock was poisoned",
                )
            })?
            .gateway_ref()
            .to_string();
        let reading = reader.read(&gateway_ref, position_ref);
        let response = GatewayResponseEnvelope::from_result(
            request.request_id,
            Ok(GatewayResponse::Occupancy { reading }),
        );
        let encoded = serde_json::to_string(&response).map_err(|error| {
            AikitError::new(
                "agency_gateway_service.response_encode",
                format!("encode gateway response: {error}"),
            )
        })?;
        return Ok((encoded, false, None));
    }
    let should_shutdown = request.command.is_shutdown();
    let mut gateway = gateway.lock().map_err(|_| {
        AikitError::new(
            "agency_gateway_service.poisoned",
            "gateway state lock was poisoned",
        )
    })?;
    let result = execute_gateway_command(&mut gateway, request.command.clone());
    let mut subscribed = None;
    match &result {
        // A subscribe is a replay plus a live attachment, made one indivisible
        // moment by holding the gateway state lock across both: no event can
        // slip between the replay snapshot and the registration.
        Ok(GatewayResponse::Replay { .. })
            if matches!(request.command, GatewayCommand::Subscribe { .. }) =>
        {
            let GatewayCommand::Subscribe { stream_ref, .. } = &request.command else {
                unreachable!("matched above")
            };
            subscribed = Some(subscriptions.subscribe(stream_ref.clone()));
        }
        Ok(GatewayResponse::OperationPrepared { operation }) => {
            // Hand the prepared operation to its connector's pump. The queue
            // is the thread-safe seam: the pump executes and records the
            // receipt through the kernel like any carrier command.
            runtime
                .queues
                .queue_for(&operation.connector_ref)
                .push(operation.clone());
        }
        Ok(GatewayResponse::Ingress {
            result: GatewayIngressResult::Appended { stream_ref, event, .. },
        }) => {
            // Carrier-appended events reach live subscribers exactly like
            // pump-appended ones, under the same lock.
            runtime.hub.publish(stream_ref, event);
        }
        _ => {}
    }
    let response = GatewayResponseEnvelope::from_result(request.request_id, result);
    if response.ok {
        persist_gateway_state(&gateway, state_file)?;
    }
    let encoded = serde_json::to_string(&response).map_err(|error| {
        AikitError::new(
            "agency_gateway_service.response_encode",
            format!("encode gateway response: {error}"),
        )
    })?;
    Ok((encoded, should_shutdown, subscribed))
}

fn websocket_handshake<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    expected_token: &str,
) -> Result<()> {
    let mut request_line = String::new();
    let mut consumed = reader
        .read_line(&mut request_line)
        .map_err(io_error("read WebSocket request line"))?;
    if consumed == 0 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_eof",
            "WebSocket peer closed before upgrade",
        ));
    }
    if !request_line.starts_with("GET ") || !request_line.contains(" HTTP/1.1") {
        write_http_error(writer, 400, "Bad Request")?;
        return Err(AikitError::new(
            "agency_gateway_service.websocket_request",
            "WebSocket upgrade must use HTTP/1.1 GET",
        ));
    }

    let mut upgrade = None;
    let mut connection = None;
    let mut version = None;
    let mut key = None;
    let mut authorization = None;
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(io_error("read WebSocket upgrade header"))?;
        if bytes == 0 {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_eof",
                "WebSocket peer closed during upgrade",
            ));
        }
        consumed += bytes;
        if consumed > MAX_HTTP_HEADER_BYTES {
            write_http_error(writer, 431, "Request Header Fields Too Large")?;
            return Err(AikitError::new(
                "agency_gateway_service.websocket_headers_too_large",
                "WebSocket upgrade headers exceed gateway limit",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        match name.as_str() {
            "upgrade" => upgrade = Some(value),
            "connection" => connection = Some(value),
            "sec-websocket-version" => version = Some(value),
            "sec-websocket-key" => key = Some(value),
            "authorization" => authorization = Some(value),
            _ => {}
        }
    }

    let authorised = authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected_token.as_bytes()));
    if !authorised {
        write_http_error(writer, 401, "Unauthorized")?;
        return Err(AikitError::new(
            "agency_gateway_service.websocket_unauthorised",
            "gateway WebSocket bearer authentication failed",
        ));
    }
    if upgrade
        .as_deref()
        .is_none_or(|value| !value.eq_ignore_ascii_case("websocket"))
        || connection.as_deref().is_none_or(|value| {
            !value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
        || version.as_deref() != Some("13")
    {
        write_http_error(writer, 426, "Upgrade Required")?;
        return Err(AikitError::new(
            "agency_gateway_service.websocket_upgrade",
            "invalid WebSocket upgrade headers",
        ));
    }
    let key = key.ok_or_else(|| {
        AikitError::new(
            "agency_gateway_service.websocket_key",
            "WebSocket upgrade has no Sec-WebSocket-Key",
        )
    })?;
    let accept = websocket_accept(&key);
    write!(
        writer,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    .map_err(io_error("write WebSocket upgrade response"))?;
    writer
        .flush()
        .map_err(io_error("flush WebSocket upgrade response"))?;
    Ok(())
}

fn write_http_error<W: Write>(writer: &mut W, status: u16, reason: &str) -> Result<()> {
    write!(
        writer,
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    )
    .map_err(io_error("write HTTP error response"))?;
    writer
        .flush()
        .map_err(io_error("flush HTTP error response"))?;
    Ok(())
}

#[derive(Debug)]
struct WebSocketFrame {
    opcode: u8,
    payload: Vec<u8>,
}

fn read_websocket_frame<R: Read>(reader: &mut R, max_frame_bytes: usize) -> Result<WebSocketFrame> {
    let mut head = [0u8; 2];
    match reader.read_exact(&mut head) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_eof",
                "WebSocket peer closed",
            ));
        }
        Err(error) => return Err(io_error("read WebSocket frame header")(error)),
    }
    if head[0] & 0x70 != 0 || head[0] & 0x80 == 0 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_fragmentation",
            "gateway WebSocket accepts only final frames with no RSV extensions",
        ));
    }
    let opcode = head[0] & 0x0f;
    let masked = head[1] & 0x80 != 0;
    if !masked {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_unmasked_client",
            "client WebSocket frames must be masked",
        ));
    }
    let mut length = (head[1] & 0x7f) as u64;
    if length == 126 {
        let mut bytes = [0u8; 2];
        reader
            .read_exact(&mut bytes)
            .map_err(io_error("read WebSocket 16-bit length"))?;
        length = u16::from_be_bytes(bytes) as u64;
    } else if length == 127 {
        let mut bytes = [0u8; 8];
        reader
            .read_exact(&mut bytes)
            .map_err(io_error("read WebSocket 64-bit length"))?;
        if bytes[0] & 0x80 != 0 {
            return Err(AikitError::new(
                "agency_gateway_service.websocket_length",
                "WebSocket payload length has invalid high bit",
            ));
        }
        length = u64::from_be_bytes(bytes);
    }
    if length > max_frame_bytes as u64 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_frame_too_large",
            format!("WebSocket frame {length} bytes exceeds gateway limit {max_frame_bytes}"),
        ));
    }
    if matches!(opcode, 0x8..=0xA) && length > 125 {
        return Err(AikitError::new(
            "agency_gateway_service.websocket_control_length",
            "WebSocket control frame payload exceeds 125 bytes",
        ));
    }
    let mut mask = [0u8; 4];
    reader
        .read_exact(&mut mask)
        .map_err(io_error("read WebSocket mask"))?;
    let mut payload = vec![0u8; length as usize];
    reader
        .read_exact(&mut payload)
        .map_err(io_error("read WebSocket payload"))?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    Ok(WebSocketFrame { opcode, payload })
}

fn write_websocket_text<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    write_websocket_frame(writer, 0x1, payload)
}

fn write_websocket_close<W: Write>(writer: &mut W, code: u16, reason: &str) -> Result<()> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(reason.as_bytes());
    write_websocket_frame(writer, 0x8, &payload)
}

fn write_websocket_frame<W: Write>(writer: &mut W, opcode: u8, payload: &[u8]) -> Result<()> {
    writer
        .write_all(&[0x80 | (opcode & 0x0f)])
        .map_err(io_error("write WebSocket frame opcode"))?;
    match payload.len() {
        length if length < 126 => writer
            .write_all(&[length as u8])
            .map_err(io_error("write WebSocket frame length"))?,
        length if length <= u16::MAX as usize => {
            writer
                .write_all(&[126])
                .map_err(io_error("write WebSocket frame length marker"))?;
            writer
                .write_all(&(length as u16).to_be_bytes())
                .map_err(io_error("write WebSocket 16-bit length"))?;
        }
        length => {
            writer
                .write_all(&[127])
                .map_err(io_error("write WebSocket frame length marker"))?;
            writer
                .write_all(&(length as u64).to_be_bytes())
                .map_err(io_error("write WebSocket 64-bit length"))?;
        }
    }
    writer
        .write_all(payload)
        .map_err(io_error("write WebSocket frame payload"))?;
    writer.flush().map_err(io_error("flush WebSocket frame"))?;
    Ok(())
}

fn websocket_accept(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + WEBSOCKET_GUID.len());
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(WEBSOCKET_GUID.as_bytes());
    base64_encode(&sha1(&input))
}

pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

// RFC 3174 SHA-1. SHA-1 is required by the RFC 6455 WebSocket handshake; it is
// not used here for credential hashing or any security decision.
pub(crate) fn sha1(input: &[u8]) -> [u8; 20] {
    let bit_len = (input.len() as u64) * 8;
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut h0 = 0x67452301u32;
    let mut h1 = 0xEFCDAB89u32;
    let mut h2 = 0x98BADCFEu32;
    let mut h3 = 0x10325476u32;
    let mut h4 = 0xC3D2E1F0u32;

    for chunk in message.as_chunks::<64>().0 {
        let mut words = [0u32; 80];
        for (index, bytes) in chunk.as_chunks::<4>().0.iter().enumerate() {
            words[index] = u32::from_be_bytes(*bytes);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        for (index, word) in words.iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut output = [0u8; 20];
    for (index, word) in [h0, h1, h2, h3, h4].iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(c & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn io_error(context: &'static str) -> impl FnOnce(io::Error) -> AikitError {
    move |error| AikitError::new("agency_gateway_service.io", format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::SocketAddr,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use aikit_core::resource::ResourceRef;
    use serde_json::Value;

    fn gateway() -> AgencyGateway {
        AgencyGateway::new(ResourceRef::parse("agency-gateway/test").unwrap())
    }

    fn test_runtime() -> Arc<GatewayServiceRuntime> {
        Arc::new(GatewayServiceRuntime {
            hub: Arc::new(SubscriptionHub::default()),
            queues: Arc::new(crate::gateway_connector_pump::ConnectorQueues::default()),
            connections: ConnectionRegistry::default(),
        })
    }

    #[test]
    fn websocket_accept_matches_rfc_6455_reference_vector() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn sha1_matches_reference_digest() {
        assert_eq!(
            sha1(b"abc"),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
            ]
        );
    }

    #[test]
    fn bearer_comparison_rejects_length_and_value_drift() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"sage"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
    }

    #[test]
    fn semantic_state_round_trips_through_atomic_file_without_material_identity() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("gateway.json");
        let original_gateway = gateway();
        persist_gateway_state(&original_gateway, Some(&state)).unwrap();
        let encoded = fs::read_to_string(&state).unwrap();
        assert!(!encoded.contains("pid"));
        assert!(!encoded.contains("socket"));
        assert!(!encoded.contains("workcell"));
        let restored = restore_gateway_state(gateway(), Some(&state)).unwrap();
        assert_eq!(restored.status(), original_gateway.status());
    }

    fn communique_draft(reference: &str) -> crate::gateway_communique::CommuniqueDraft {
        crate::gateway_communique::CommuniqueDraft {
            communique_ref: format!("aikit:communique:{reference}"),
            from_position_ref: None,
            from_generation_ref: None,
            attribution: crate::gateway_communique::SenderAttribution::Unknown,
            attribution_basis: "test".into(),
            to_position_ref: "central:position:control:root:keeper".into(),
            to_workcell_ref: None,
            body: "offline".into(),
            sent_at_unix_ms: 1,
            state: crate::gateway_communique::CommuniqueState::Held,
            state_basis: "test".into(),
            reply_to: None,
            forward_to_workcell_ref: None,
            routing: None,
        }
    }

    #[test]
    fn an_offline_command_never_interleaves_with_a_service_holding_the_state() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("gateway.json");
        let send = |reference: &str| GatewayCommand::SendCommunique {
            draft: communique_draft(reference),
        };

        // A service holds the state: the offline writer waits, then refuses.
        let service =
            acquire_gateway_state_lock(&state, Duration::from_secs(1), "gateway service").unwrap();
        let refused =
            execute_against_state_file(gateway(), &state, send("one"), Duration::from_millis(100))
                .unwrap_err();
        assert_eq!(refused.code(), "agency_gateway_service.state_locked");
        assert!(refused.to_string().contains("gateway service"));
        assert!(
            !state.exists(),
            "nothing was written under a holding service"
        );
        drop(service);

        // With no service, the same kernel command lands in the state file.
        execute_against_state_file(gateway(), &state, send("one"), Duration::from_secs(1)).unwrap();
        let restored = restore_gateway_state(gateway(), Some(&state)).unwrap();
        assert_eq!(restored.communiques().len(), 1);

        // A read never rewrites the file.
        let before = fs::read(&state).unwrap();
        let modified = fs::metadata(&state).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        execute_against_state_file(
            gateway(),
            &state,
            GatewayCommand::CommuniqueCounts,
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(fs::read(&state).unwrap(), before);
        assert_eq!(fs::metadata(&state).unwrap().modified().unwrap(), modified);
    }

    #[cfg(unix)]
    #[test]
    fn a_carrier_that_cannot_bind_stops_the_service_instead_of_leaving_it_half_alive() {
        let root = tempfile::tempdir().unwrap();
        // Longer than any platform's sun_path: the Unix carrier cannot bind.
        let unusable = root.path().join("x".repeat(120)).join("gateway.sock");
        let config = GatewayServiceConfig {
            websocket_bind: Some("127.0.0.1:0".into()),
            websocket_bearer_token: Some("token".into()),
            unix_socket: Some(unusable),
            state_file: Some(root.path().join("gateway.json")),
            max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(run_gateway_service(gateway(), config));
        });
        let outcome = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the service must stop when a carrier cannot bind");
        let error = outcome.unwrap_err();
        assert!(
            error.code().starts_with("agency_gateway_service.unix"),
            "{error}"
        );
    }

    #[test]
    fn invalid_websocket_auth_is_rejected_before_upgrade() {
        let request = b"GET / HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer wrong\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let mut response = Vec::new();
        let error = websocket_handshake(&mut reader, &mut response, "correct").unwrap_err();
        assert_eq!(
            error.code(),
            "agency_gateway_service.websocket_unauthorised"
        );
        assert!(String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 401"));
    }

    #[test]
    fn websocket_upgrade_accepts_authenticated_reference_handshake() {
        let request = b"GET /gateway HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer secret\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let mut response = Vec::new();
        websocket_handshake(&mut reader, &mut response, "secret").unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 101 Switching Protocols"));
        assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    }

    #[test]
    fn masked_client_frame_decodes_and_server_frame_remains_unmasked() {
        let payload = br#"{"hello":"world"}"#;
        let mask = [1u8, 2, 3, 4];
        let mut frame = vec![0x81, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        let decoded = read_websocket_frame(&mut &frame[..], 1024).unwrap();
        assert_eq!(decoded.opcode, 1);
        assert_eq!(decoded.payload, payload);

        let mut server = Vec::new();
        write_websocket_text(&mut server, payload).unwrap();
        assert_eq!(server[0], 0x81);
        assert_eq!(server[1] & 0x80, 0);
    }

    #[test]
    fn oversized_frame_is_rejected_before_payload_allocation() {
        let mut frame = vec![0x81, 0x80 | 126];
        frame.extend_from_slice(&5000u16.to_be_bytes());
        frame.extend_from_slice(&[1, 2, 3, 4]);
        let error = read_websocket_frame(&mut &frame[..], 1024).unwrap_err();
        assert_eq!(
            error.code(),
            "agency_gateway_service.websocket_frame_too_large"
        );
    }

    fn masked_text_frame(text: &str) -> Vec<u8> {
        let payload = text.as_bytes();
        let mask = [5u8, 7, 11, 13];
        let mut frame = vec![0x81];
        if payload.len() < 126 {
            frame.push(0x80 | payload.len() as u8);
        } else {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        frame
    }

    fn read_server_text(stream: &mut TcpStream) -> Value {
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).unwrap();
        assert_eq!(head[0] & 0x0f, 1);
        assert_eq!(head[1] & 0x80, 0);
        let mut length = (head[1] & 0x7f) as usize;
        if length == 126 {
            let mut bytes = [0u8; 2];
            stream.read_exact(&mut bytes).unwrap();
            length = u16::from_be_bytes(bytes) as usize;
        }
        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload).unwrap();
        serde_json::from_slice(&payload).unwrap()
    }

    #[test]
    fn websocket_service_executes_protocol_and_shutdown_on_one_shared_gateway() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let gateway = Arc::new(Mutex::new(gateway()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_gateway = Arc::clone(&gateway);
        let server_shutdown = Arc::clone(&shutdown);
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let result = serve_websocket_listener(
                listener,
                server_gateway,
                server_shutdown,
                "secret".into(),
                None,
                64 * 1024,
                None,
                test_runtime(),
            );
            done_tx.send(result).unwrap();
        });

        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "GET / HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer secret\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
        let mut handshake = Vec::new();
        let mut last_four = [0u8; 4];
        while last_four != *b"\r\n\r\n" {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).unwrap();
            handshake.push(byte[0]);
            if handshake.len() >= 4 {
                last_four.copy_from_slice(&handshake[handshake.len() - 4..]);
            }
        }
        assert!(String::from_utf8(handshake)
            .unwrap()
            .starts_with("HTTP/1.1 101"));

        let protocol = serde_json::json!({
            "request_id":"p1",
            "command":{"type":"protocol"}
        })
        .to_string();
        stream.write_all(&masked_text_frame(&protocol)).unwrap();
        let response = read_server_text(&mut stream);
        assert_eq!(response["ok"], true);
        assert_eq!(response["request_id"], "p1");
        assert_eq!(response["response"]["type"], "protocol");

        let shutdown_request = serde_json::json!({
            "request_id":"stop",
            "command":{"type":"shutdown"}
        })
        .to_string();
        stream
            .write_all(&masked_text_frame(&shutdown_request))
            .unwrap();
        let response = read_server_text(&mut stream);
        assert_eq!(response["ok"], true);
        assert_eq!(response["response"]["type"], "shutdown");

        let deadline = Instant::now() + Duration::from_secs(3);
        while !shutdown.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(shutdown.load(Ordering::SeqCst));
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_carrier_is_owner_only_and_executes_same_protocol() {
        use std::os::unix::{fs::PermissionsExt, net::UnixStream};

        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("gateway.sock");
        let state = root.path().join("gateway.json");
        let config = GatewayServiceConfig {
            websocket_bind: None,
            websocket_bearer_token: None,
            unix_socket: Some(socket.clone()),
            state_file: Some(state.clone()),
            max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            done_tx
                .send(run_gateway_service(gateway(), config))
                .unwrap()
        });

        let deadline = Instant::now() + Duration::from_secs(30);
        while !socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(socket.exists());
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let mut stream = UnixStream::connect(&socket).unwrap();
        writeln!(
            stream,
            "{}",
            serde_json::json!({"request_id":"p1","command":{"type":"protocol"}})
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        let response: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(response["response"]["type"], "protocol");

        writeln!(
            stream,
            "{}",
            serde_json::json!({"request_id":"stop","command":{"type":"shutdown"}})
        )
        .unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        assert!(state.exists());
        assert!(!socket.exists());
    }

    /// A fixture Workcell owner: answers from a map, counts how often it was
    /// asked, and proves it is consulted per query rather than cached.
    struct FixtureOccupancy {
        asked: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl GatewayOccupancyReader for FixtureOccupancy {
        fn read(&self, gateway_ref: &str, position_ref: Option<&str>) -> GatewayOccupancyReading {
            let asked = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
            GatewayOccupancyReading {
                schema: crate::gateway_runtime::GATEWAY_OCCUPANCY_READING_SCHEMA.into(),
                position_ref: position_ref.map(str::to_owned),
                gateway_ref: gateway_ref.into(),
                workcell_ref: Some("workcell:b".into()),
                workcell_basis: "fixture".into(),
                occupancy: Some(serde_json::json!({
                    "position_ref": position_ref,
                    "state": "occupied",
                    "current": {"generation_ref": format!("actuation:generation:{asked}")},
                })),
                unavailable: None,
                read_at_unix_ms: 1,
            }
        }
    }

    fn line_exchange(stream: &mut std::os::unix::net::UnixStream, request: Value) -> Value {
        writeln!(stream, "{request}").unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        serde_json::from_str(response.trim()).unwrap()
    }

    /// Read one newline-terminated response from a gateway line connection,
    /// retrying through read timeouts until the deadline.
    fn read_gateway_line(
        stream: &std::os::unix::net::UnixStream,
        deadline: Instant,
    ) -> serde_json::Value {
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(count) if count > 0 => {
                    return serde_json::from_str(line.trim()).unwrap();
                }
                _ if Instant::now() >= deadline => {
                    panic!("timed out reading a gateway line");
                }
                _ => continue,
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn subscribe_answers_with_replay_then_pushes_live_events_without_gaps_or_duplicates() {
        use crate::gateway_connector_pump::tests::{fixture_entry, FixtureFactory, FixtureInner};
        use crate::gateway_connector_pump::CONNECTOR_QUIET_POLL_CODE;
        let _ = CONNECTOR_QUIET_POLL_CODE;

        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("gateway.sock");
        let state = root.path().join("gateway.json");

        // The deployed posture: a persisted kernel that already names the
        // connector and its binding, so the pump's ingests append.
        {
            let mut gateway =
                AgencyGateway::new(ResourceRef::parse("agency-gateway/test").unwrap());
            crate::gateway_connector_pump::tests::seed_binding(&mut gateway);
            persist_gateway_state(&gateway, Some(&state)).unwrap();
        }

        let inner = FixtureInner::new();
        let config = GatewayServiceConfig {
            websocket_bind: None,
            websocket_bearer_token: None,
            unix_socket: Some(socket.clone()),
            state_file: Some(state.clone()),
            max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        let hooks = GatewayServiceHooks {
            ticks: None,
            occupancy: None,
            connectors: vec![Box::new(FixtureFactory {
                entry: fixture_entry(),
                inner: Arc::clone(&inner),
            })],
        };
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            done_tx
                .send(run_gateway_service_with_hooks(
                    AgencyGateway::new(ResourceRef::parse("agency-gateway/test").unwrap()),
                    config,
                    hooks,
                ))
                .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while !socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        let subscribe = |after_sequence: u64| -> std::os::unix::net::UnixStream {
            let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
            let request = serde_json::json!({
                "request_id": "sub",
                "command": {
                    "type": "subscribe",
                    "stream_ref": "actuation-stream/fixture",
                    "after_sequence": after_sequence
                }
            });
            use std::io::Write as _;
            writeln!(stream, "{request}").unwrap();
            stream
        };
        let far_deadline = Instant::now() + Duration::from_secs(10);

        // One event appends before anyone subscribes; the pump ingests it
        // into the kernel and persists it.
        inner.push_text("one");
        let appended = Instant::now() + Duration::from_secs(10);
        loop {
            let persisted = std::fs::read_to_string(&state).unwrap_or_default();
            if persisted.contains("\"sequence\": 1") || persisted.contains("\"sequence\":1") {
                break;
            }
            assert!(Instant::now() < appended, "the first event never landed");
            thread::sleep(Duration::from_millis(20));
        }

        // First session: the subscribe answers with the replay payload (the
        // one event already in the journal), then pushes live appends.
        let client = subscribe(0);
        let replay = read_gateway_line(&client, far_deadline);
        assert_eq!(replay["response"]["type"], "replay", "{replay}");
        assert_eq!(
            replay["response"]["replay"]["events"].as_array().unwrap().len(),
            1
        );
        inner.push_text("two");
        let push = read_gateway_line(&client, far_deadline);
        assert_eq!(push["response"]["type"], "stream-event", "{push}");
        assert_eq!(push["response"]["event"]["sequence"], 2);
        assert_eq!(push["response"]["event"]["event"]["content"], "two");

        // The client goes away; an event appends while it is disconnected.
        drop(client);
        inner.push_text("three");
        // The pump must have appended it before the re-subscribe: a subscribe
        // served earlier would see the gap arrive as a live push instead.
        let appended = Instant::now() + Duration::from_secs(10);
        loop {
            let persisted = std::fs::read_to_string(&state).unwrap_or_default();
            if persisted.contains("\"sequence\": 3") || persisted.contains("\"sequence\":3") {
                break;
            }
            assert!(Instant::now() < appended, "the away-gap event never landed");
            thread::sleep(Duration::from_millis(20));
        }

        // Re-Subscribe from the last seen sequence: the replay covers the
        // gap (three) with no duplicate of two, then live pushes resume.
        let client = subscribe(2);
        let replay = read_gateway_line(&client, far_deadline);
        let covered: Vec<u64> = replay["response"]["replay"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["sequence"].as_u64().unwrap())
            .collect();
        assert_eq!(covered, vec![3], "replay must cover the away gap: {replay}");
        inner.push_text("four");
        let push = read_gateway_line(&client, far_deadline);
        assert_eq!(push["response"]["event"]["sequence"], 4);
        assert_eq!(push["response"]["event"]["event"]["content"], "four");

        // The stream saw every event exactly once, in order. A push can beat
        // the state write to the file, so poll for the final append.
        let snapshot_deadline = Instant::now() + Duration::from_secs(10);
        let snapshot = loop {
            let snapshot: Vec<u64> = restore_gateway_state(
                AgencyGateway::new(ResourceRef::parse("agency-gateway/test").unwrap()),
                Some(&state),
            )
            .unwrap()
            .snapshot()
            .streams
            .into_iter()
            .flat_map(|stream| stream.events.into_iter().map(|event| event.sequence))
            .collect();
            if snapshot == vec![1, 2, 3, 4] {
                break snapshot;
            }
            assert!(
                Instant::now() < snapshot_deadline,
                "the stream never completed: {snapshot:?}"
            );
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(snapshot, vec![1, 2, 3, 4]);

        // Shutdown on a separate connection; the service stops cleanly.
        let mut stop = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        use std::io::Write as _;
        writeln!(
            stop,
            "{}",
            serde_json::json!({"command": {"type": "shutdown"}})
        )
        .unwrap();
        drop(client);
        done_rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    }

    #[test]
    fn an_occupancy_query_is_answered_fresh_by_the_owner_hook_and_never_by_the_kernel() {
        use std::os::unix::net::UnixStream;

        // No hook: the kernel holds no occupancy and says so.
        let mut kernel = gateway();
        let refused = execute_gateway_command(
            &mut kernel,
            GatewayCommand::OccupancyRead {
                position_ref: "central:position:project:O-I:scribe".into(),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "agency_gateway.occupancy_not_served");
        assert!(GatewayCommand::OccupancyList.is_read_only());

        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("gateway.sock");
        let state = root.path().join("gateway.json");
        let config = GatewayServiceConfig {
            websocket_bind: None,
            websocket_bearer_token: None,
            unix_socket: Some(socket.clone()),
            state_file: Some(state.clone()),
            max_frame_bytes: DEFAULT_GATEWAY_MAX_FRAME_BYTES,
        };
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hooks = GatewayServiceHooks {
            ticks: None,
            occupancy: Some(Arc::new(FixtureOccupancy {
                asked: Arc::clone(&asked),
            })),
            connectors: Vec::new(),
        };
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            done_tx
                .send(run_gateway_service_with_hooks(gateway(), config, hooks))
                .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while !socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let mut stream = UnixStream::connect(&socket).unwrap();
        let first = line_exchange(
            &mut stream,
            serde_json::json!({"request_id":"o1","command":{"type":"occupancy-read","position_ref":"central:position:project:O-I:scribe"}}),
        );
        assert_eq!(first["ok"], true, "{first}");
        let reading = &first["response"]["reading"];
        assert_eq!(first["response"]["type"], "occupancy");
        assert_eq!(reading["schema"], "aikit.gateway-occupancy-reading/v1");
        assert_eq!(reading["gateway_ref"], "agency-gateway/test");
        assert_eq!(reading["workcell_ref"], "workcell:b");
        assert_eq!(
            reading["position_ref"],
            "central:position:project:O-I:scribe"
        );
        assert_eq!(
            reading["occupancy"]["current"]["generation_ref"],
            "actuation:generation:1"
        );
        let listing = line_exchange(
            &mut stream,
            serde_json::json!({"request_id":"o2","command":{"type":"occupancy-list"}}),
        );
        assert!(listing["response"]["reading"]["position_ref"].is_null());
        // Asked twice, read twice: nothing is cached.
        assert_eq!(
            listing["response"]["reading"]["occupancy"]["current"]["generation_ref"],
            "actuation:generation:2"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // An occupancy query writes no gateway state.
        assert!(!state.exists());
        line_exchange(
            &mut stream,
            serde_json::json!({"request_id":"stop","command":{"type":"shutdown"}}),
        );
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
    }
}
