//! The concurrent AgentSession bridge over one real connection process.
//!
//! `agent_connection` owns protocol semantics and `connection_process` owns the
//! transport; both leave the read loop to the caller. That is why every consumer
//! so far has had to hold one blocking read loop per process: a slow turn blocks
//! its whole transport, a second session needs a second mutex, and an interrupt
//! can only be observed by whoever happened to be reading at the time. This
//! module is the host side of that seam for clients that carry several
//! encounters at once.
//!
//! What it owns, and nothing more:
//!
//! * one demultiplexing reader thread per connection, so an in-flight turn on
//!   one session can never block another session on the same transport, and no
//!   caller has to block on `read` to observe its own turn;
//! * per-session ordered event lanes carrying the adapter's provenance-bearing
//!   [`ConnectionSignal`]s *as they arrive*, which is what streaming is; a lane
//!   ends when the bridge stops, deliberately or not, so a caller parked on it
//!   learns of the stop instead of blocking on a dead transport;
//! * a mid-turn interrupt that issues the adapter's coordinated cancel, keeps
//!   reading until the provider actually stops the turn, and then records the
//!   interruption on that session's trail;
//! * the canonical-identity rule: the host binds every native session to an
//!   explicitly supplied `agent-session/*` ref, keeps native ids as routing
//!   facts only, and never synthesizes, rewrites or collapses that identity.
//!
//! What it deliberately does not own: Agent or AgentSession identity (the
//! caller brings it), SessionSpace attribution, permission *policy* (it only
//! carries a permission request and its answer), transcripts, or a reconnect
//! claim. Shutting the host down terminates a process; it says nothing about
//! canonical session continuity, which a caller proves from target evidence.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::process::ExitStatus;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::session_event_queue::EventQueue;
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent_connection::{
    CancelRequest, ConnectionCommand, ConnectionDescriptor, ConnectionSignal, ConnectionSignalKind,
    NativePermissionRequest, NativeSessionBinding, PromptRequest, SessionOpenRequest,
};
use crate::connection_process::{
    ConnectionControl, ConnectionProcess, ConnectionReader, ConnectionWriter,
};
use crate::interactive_connection::{InteractiveAgentConnectionAdapter, PermissionDecision};

pub const AGENT_SESSION_HOST_VERSION: &str = "aikit.agent-session-host/v1";

/// Zero means no operational total-event limit. Retention is bounded separately
/// by the disk-backed event lane; legitimate reasoning is not a protocol fault.
pub const DEFAULT_MAX_SIGNALS_PER_TURN: usize = 0;

/// Signals retained for natives no lane on this host claims. The host does not
/// silently drop what it cannot attribute.
const UNATTRIBUTED_LIMIT: usize = 64;

/// What a turn still in flight is told when the host itself stops the
/// transport. It is a stop the host chose, not a transport failure.
const DELIBERATE_STOP_REASON: &str =
    "the host was stopped deliberately while the turn was in flight";

/// Host-level limits, stated rather than ambient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSessionHostLimits {
    pub max_signals_per_turn: usize,
}

impl Default for AgentSessionHostLimits {
    fn default() -> Self {
        Self {
            max_signals_per_turn: DEFAULT_MAX_SIGNALS_PER_TURN,
        }
    }
}

/// One ordered host event as observed on a session lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HostEvent {
    /// A provenance-bearing signal, in observed wire order.
    Signal(ConnectionSignal),
    /// Terminal for one turn. Always the last event that turn produces.
    TurnEnded(TurnRecord),
}

/// How a turn actually stopped, as observed on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStop {
    Completed { stop_reason: String },
    Cancelled,
    OperationalLimit { max_signals: usize },
    Failed { reason: String },
}

/// Who asked for the stop. A human interrupt and a provider-side cancellation
/// are different facts and are recorded as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterruptOrigin {
    Human,
    Provider,
}

/// The recorded interruption of one turn. It exists only once a stop has
/// actually been observed; a request that is still in flight is an
/// [`InterruptReceipt`], which is a different fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnInterruption {
    pub agent_session: ResourceRef,
    pub native_session_id: String,
    pub origin: InterruptOrigin,
    pub reason: Option<String>,
    /// Wire commands actually issued for the interrupt. For ACP this is the
    /// coordinated cancel: pending permission answers first, then
    /// `session/cancel`. Empty when nobody on this host asked for the stop.
    pub commands: Vec<String>,
    pub requested_at_sequence: Option<u64>,
    /// What the turn ended with. `TurnStop::Failed` here means the transport
    /// stopped before the provider could report anything.
    pub observed_stop: TurnStop,
}

/// The immediate, honest answer to "interrupt this": what was issued and where
/// the turn stood when it was issued. The recorded [`TurnInterruption`] arrives
/// later, on the turn's own lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptReceipt {
    pub agent_session: ResourceRef,
    pub native_session_id: String,
    pub commands: Vec<String>,
    pub requested_at_sequence: u64,
    pub reason: Option<String>,
}

/// The full record of one finished turn, delivered as `HostEvent::TurnEnded`
/// and retained per session until the next turn replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnRecord {
    pub agent_session: ResourceRef,
    pub binding: NativeSessionBinding,
    pub stop: TurnStop,
    pub interruption: Option<TurnInterruption>,
    pub first_sequence: Option<u64>,
    pub last_sequence: u64,
    pub signals: usize,
}

/// What a session lane is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLaneState {
    Resident,
    TurnInFlight,
    InterruptRequested,
}

/// The canonical identity of a resident session, with the native binding kept
/// visibly separate from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub agent_session: ResourceRef,
    pub binding: NativeSessionBinding,
    pub state: SessionLaneState,
}

/// Outcome of a bounded wait for a turn to finish.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitOutcome {
    Finished(Box<TurnRecord>),
    Timeout,
}

/// One canonical AgentSession resident on this host. Clones share one ordered
/// lane, so a lane is one encounter's stream and not a broadcast topic.
#[derive(Clone)]
pub struct SessionLane {
    shared: Arc<HostShared>,
    lane: Arc<LaneCore>,
    agent_session: ResourceRef,
    binding: NativeSessionBinding,
}

/// A live turn on a [`SessionLane`]: the streaming cursor and the handle that
/// may interrupt the turn. Dropping it does not stop the turn.
pub struct TurnHandle {
    shared: Arc<HostShared>,
    lane: Arc<LaneCore>,
}

/// Durable canonical encounter events belong to the native owner. Production
/// encounter services supply this sink; the host orders writes before delivery.
/// A failed append is a resource failure, never a provider cancellation.
pub trait SessionEventJournal: Send + Sync {
    fn append(&self, agent_session: &ResourceRef, event: &HostEvent) -> Result<()>;
}

pub struct AgentSessionHost {
    shared: Arc<HostShared>,
    reader: Option<JoinHandle<()>>,
}

struct HostShared {
    adapter: Mutex<Box<dyn InteractiveAgentConnectionAdapter + Send>>,
    writer: ConnectionWriter,
    control: ConnectionControl,
    state: Mutex<HostState>,
    /// Serializes multi-command bursts (a coordinated cancel is more than one
    /// wire command) so one session's burst stays contiguous.
    io: Mutex<()>,
    /// Serializes handshake and session-open control operations, so exactly one
    /// correlated control response is awaited at a time.
    control_gate: Mutex<()>,
    limits: AgentSessionHostLimits,
    journal: Option<Arc<dyn SessionEventJournal>>,
}

struct LaneCore {
    agent_session: ResourceRef,
    events: Mutex<EventQueue>,
    queue: EventQueue,
    journal: Option<Arc<dyn SessionEventJournal>>,
}
impl LaneCore {
    fn deliver(&self, event: HostEvent) -> std::io::Result<()> {
        if let Some(journal) = &self.journal {
            journal
                .append(&self.agent_session, &event)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        self.queue.send(event)
    }
    fn close(&self) {
        self.queue.close();
    }
}

#[derive(Default)]
struct HostState {
    control: BTreeMap<String, ControlWaiter>,
    pending_prompts: BTreeMap<String, String>,
    lanes: BTreeMap<String, Arc<LaneCore>>,
    sessions: BTreeMap<ResourceRef, SessionRecord>,
    turns: BTreeMap<String, ActiveTurn>,
    trail: BTreeMap<ResourceRef, VecDeque<TurnInterruption>>,
    last_turn: BTreeMap<ResourceRef, TurnRecord>,
    unattributed: VecDeque<ConnectionSignal>,
    last_sequence: u64,
    transport_error: Option<String>,
    closed: bool,
}

struct SessionRecord {
    binding: NativeSessionBinding,
}

struct ActiveTurn {
    agent_session: ResourceRef,
    first_sequence: Option<u64>,
    last_sequence: u64,
    signals: usize,
    interrupt: Option<PendingInterrupt>,
    operational_limit: Option<usize>,
}

struct PendingInterrupt {
    reason: Option<String>,
    requested_at_sequence: u64,
    commands: Vec<String>,
}

enum ControlWaiter {
    /// `initialize`, resolved by the negotiation signals.
    Handshake(Sender<ControlDelivery>),
    /// A session open. The lane is registered for its native id before the
    /// caller is woken, so no update can outrun the registration.
    Open {
        sender: Sender<ControlDelivery>,
        lane: Arc<LaneCore>,
    },
}

impl ControlWaiter {
    fn send(self, delivery: ControlDelivery) {
        let sender = match self {
            ControlWaiter::Handshake(sender) => sender,
            ControlWaiter::Open { sender, .. } => sender,
        };
        let _ = sender.send(delivery);
    }
}

enum ControlDelivery {
    Signals(Vec<ConnectionSignal>),
    Failed(String),
}

impl AgentSessionHost {
    /// Spawn the provider process, split the transport, and start the
    /// demultiplexing reader. The adapter is supplied already constructed; the
    /// host does not choose protocols.
    pub fn launch<A>(
        adapter: A,
        argv: &[String],
        cwd: Option<&Path>,
        limits: AgentSessionHostLimits,
    ) -> Result<Self>
    where
        A: InteractiveAgentConnectionAdapter + Send + 'static,
    {
        Self::launch_with_journal(adapter, argv, cwd, limits, None)
    }

    pub fn launch_with_journal<A>(
        adapter: A,
        argv: &[String],
        cwd: Option<&Path>,
        limits: AgentSessionHostLimits,
        journal: Option<Arc<dyn SessionEventJournal>>,
    ) -> Result<Self>
    where
        A: InteractiveAgentConnectionAdapter + Send + 'static,
    {
        let (writer, reader, control) = ConnectionProcess::spawn_split(argv, cwd)?;
        let shared = Arc::new(HostShared {
            adapter: Mutex::new(Box::new(adapter)),
            writer,
            control,
            state: Mutex::new(HostState::default()),
            io: Mutex::new(()),
            control_gate: Mutex::new(()),
            limits,
            journal,
        });
        let thread_shared = Arc::clone(&shared);
        let reader_thread = std::thread::Builder::new()
            .name("aikit-agent-session-host".to_owned())
            .spawn(move || thread_shared.serve(reader))
            .map_err(|error| {
                AikitError::new(
                    "agent_session_host.reader_thread_unavailable",
                    format!("could not start the session reader thread: {error}"),
                )
            })?;
        Ok(Self {
            shared,
            reader: Some(reader_thread),
        })
    }

    /// Negotiate the protocol. Blocks until the provider answers; the returned
    /// descriptor carries the capabilities the rest of the host will honour.
    pub fn initialize(&self) -> Result<ConnectionDescriptor> {
        let _gate = self.shared.gate()?;
        let command = {
            let mut adapter = self.shared.adapter()?;
            adapter.initialize()?
        };
        let receiver = self.shared.register_control(&command, None)?;
        self.shared.dispatch(&command)?;
        match self.shared.await_control(receiver)? {
            ControlDelivery::Signals(_) => {}
            ControlDelivery::Failed(reason) => {
                return Err(AikitError::new(
                    "agent_session_host.handshake_failed",
                    reason,
                ))
            }
        }
        let adapter = self.shared.adapter()?;
        Ok(adapter.descriptor())
    }

    /// Open one native session and bind it to the canonical `agent-session/*`
    /// identity the caller supplies. The ref is mandatory and is never derived
    /// from a native id. Two native sessions may not claim one canonical
    /// identity on one host, and one native id may not be bound to two
    /// identities.
    pub fn open_session(&self, request: SessionOpenRequest) -> Result<SessionLane> {
        let canonical = request.agent_session.clone().ok_or_else(|| {
            AikitError::new(
                "agent_session_host.canonical_identity_required",
                "the session host binds every native session to an explicit canonical \
                 agent-session ref and never synthesizes one from a native id",
            )
        })?;
        let _gate = self.shared.gate()?;
        {
            let state = self.shared.state()?;
            if state.sessions.contains_key(&canonical) {
                return Err(AikitError::new(
                    "agent_session_host.canonical_session_already_open",
                    format!(
                        "canonical AgentSession {canonical} is already resident on this host; \
                         a second native form of one identity belongs to another connection"
                    ),
                ));
            }
        }
        let queue = EventQueue::new()
            .map_err(|e| AikitError::new("agent_session_host.event_storage", e.to_string()))?;
        let lane = Arc::new(LaneCore {
            agent_session: canonical.clone(),
            events: Mutex::new(queue.clone()),
            queue,
            journal: self.shared.journal.clone(),
        });
        let command = {
            let mut adapter = self.shared.adapter()?;
            adapter.open_session(request)?
        };
        let control_receiver = self
            .shared
            .register_control(&command, Some(Arc::clone(&lane)))?;
        self.shared.dispatch(&command)?;
        let signals = match self.shared.await_control(control_receiver)? {
            ControlDelivery::Signals(signals) => signals,
            ControlDelivery::Failed(reason) => {
                return Err(AikitError::new("agent_session_host.open_failed", reason))
            }
        };
        let binding = signals
            .iter()
            .find_map(|signal| match &signal.kind {
                ConnectionSignalKind::SessionOpened { binding } => Some(binding.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                AikitError::new(
                    "agent_session_host.open_failed",
                    "provider returned no SessionOpened binding for the open request",
                )
            })?;
        if binding.agent_session.as_ref() != Some(&canonical) {
            return Err(AikitError::new(
                "agent_session_host.identity_not_preserved",
                format!(
                    "provider binding for native session {} did not preserve canonical \
                     AgentSession {canonical}",
                    binding.native_session_id
                ),
            ));
        }
        Ok(SessionLane {
            shared: Arc::clone(&self.shared),
            lane,
            agent_session: canonical,
            binding,
        })
    }

    /// Every canonical identity resident on this host, with its current state.
    pub fn sessions(&self) -> Result<Vec<SessionIdentity>> {
        let state = self.shared.state()?;
        Ok(state
            .sessions
            .iter()
            .map(|(agent_session, record)| SessionIdentity {
                agent_session: agent_session.clone(),
                binding: record.binding.clone(),
                state: state.lane_state(&record.binding.native_session_id),
            })
            .collect())
    }

    /// One session's canonical identity and native binding.
    pub fn identity(&self, agent_session: &ResourceRef) -> Result<SessionIdentity> {
        let state = self.shared.state()?;
        let record = state
            .sessions
            .get(agent_session)
            .ok_or_else(|| session_not_open(agent_session))?;
        Ok(SessionIdentity {
            agent_session: agent_session.clone(),
            binding: record.binding.clone(),
            state: state.lane_state(&record.binding.native_session_id),
        })
    }

    /// Address one resident session for streaming and interrupt.
    pub fn lane(&self, agent_session: &ResourceRef) -> Result<SessionLane> {
        let (lane, binding) = {
            let state = self.shared.state()?;
            let record = state
                .sessions
                .get(agent_session)
                .ok_or_else(|| session_not_open(agent_session))?;
            let lane = state
                .lanes
                .get(&record.binding.native_session_id)
                .cloned()
                .ok_or_else(|| session_not_open(agent_session))?;
            (lane, record.binding.clone())
        };
        Ok(SessionLane {
            shared: Arc::clone(&self.shared),
            lane,
            agent_session: agent_session.clone(),
            binding,
        })
    }

    /// The most recent 128 observed interruptions, in turn order. The durable
    /// owner journal retains the complete history beyond this memory window.
    pub fn interruptions(&self, agent_session: &ResourceRef) -> Result<Vec<TurnInterruption>> {
        let state = self.shared.state()?;
        Ok(state
            .trail
            .get(agent_session)
            .map(|trail| trail.iter().cloned().collect())
            .unwrap_or_default())
    }

    /// The last finished turn of one session, if any.
    pub fn last_turn(&self, agent_session: &ResourceRef) -> Result<Option<TurnRecord>> {
        let state = self.shared.state()?;
        Ok(state.last_turn.get(agent_session).cloned())
    }

    /// Signals that named a native session no lane on this host claims. The
    /// host records rather than drops them, so a routing gap stays explainable.
    pub fn unattributed(&self) -> Result<Vec<ConnectionSignal>> {
        let state = self.shared.state()?;
        Ok(state.unattributed.iter().cloned().collect())
    }

    pub fn descriptor(&self) -> Result<ConnectionDescriptor> {
        let adapter = self.shared.adapter()?;
        Ok(adapter.descriptor())
    }

    pub fn is_running(&self) -> Result<bool> {
        self.shared.control.is_running()
    }

    /// Why the bridge stopped, if it stopped on its own. A deliberate
    /// [`AgentSessionHost::shutdown`] records no failure.
    pub fn transport_error(&self) -> Option<String> {
        let state = self.shared.state().ok()?;
        state.transport_error.clone()
    }

    /// Interrupt the in-flight turn of one session from anywhere. Returns the
    /// receipt for what was issued; the recorded interruption arrives on the
    /// turn's own lane and joins the session trail.
    pub fn interrupt(
        &self,
        agent_session: &ResourceRef,
        reason: Option<String>,
    ) -> Result<InterruptReceipt> {
        self.shared.interrupt(agent_session, reason)
    }

    /// Stop the transport. Canonical AgentSession identity is untouched: what
    /// dies here is a process, and continuity is proven from target evidence,
    /// never from this call. A turn still in flight is closed as
    /// [`TurnStop::Failed`] on its own lane, so no caller stays parked on it.
    pub fn shutdown(mut self) -> Result<Option<ExitStatus>> {
        self.stop_reader()
    }

    /// Wake every lane first (a deliberate stop is not a transport failure,
    /// but a stopped lane is a stopped lane), then terminate the process
    /// *before* joining the reader: the reader is blocked on the child's
    /// stdout, and the child is reaped only after the reader releases the
    /// shared state, so joining first would deadlock. Closing stdout is what
    /// lets the reader finish.
    fn stop_reader(&mut self) -> Result<Option<ExitStatus>> {
        state_stop_bridge(&self.shared.state, DELIBERATE_STOP_REASON, true);
        let status = self.shared.control.terminate();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        status
    }
}

impl std::fmt::Debug for AgentSessionHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSessionHost")
            .field("descriptor", &self.descriptor().ok())
            .field("transport_error", &self.transport_error())
            .finish()
    }
}

impl std::fmt::Debug for SessionLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionLane")
            .field("agent_session", &self.agent_session)
            .field("binding", &self.binding)
            .finish()
    }
}

impl std::fmt::Debug for TurnHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnHandle")
            .field("agent_session", &self.lane.agent_session)
            .finish()
    }
}

impl Drop for AgentSessionHost {
    fn drop(&mut self) {
        let _ = self.stop_reader();
    }
}

impl SessionLane {
    pub fn agent_session(&self) -> &ResourceRef {
        &self.agent_session
    }

    pub fn binding(&self) -> &NativeSessionBinding {
        &self.binding
    }

    /// Start one turn. Returns immediately; the turn's signals arrive on this
    /// lane in observed wire order, ending with `HostEvent::TurnEnded`.
    pub fn prompt(&self, prompt: Value) -> Result<TurnHandle> {
        let command = {
            let state = self.shared.state()?;
            let record = state
                .sessions
                .get(&self.agent_session)
                .ok_or_else(|| session_not_open(&self.agent_session))?;
            if record.binding.native_session_id != self.binding.native_session_id {
                return Err(AikitError::new(
                    "agent_session_host.stale_lane",
                    format!(
                        "this lane was taken from native session {} but the host now binds \
                         native session {} for {}",
                        self.binding.native_session_id,
                        record.binding.native_session_id,
                        self.agent_session
                    ),
                ));
            }
            let native_session_id = record.binding.native_session_id.clone();
            drop(state);
            let mut adapter = self.shared.adapter()?;
            adapter.prompt(PromptRequest {
                native_session_id,
                prompt,
            })?
        };
        self.shared.begin_turn(&self.agent_session, &command)?;
        self.shared.dispatch(&command)?;
        Ok(TurnHandle {
            shared: Arc::clone(&self.shared),
            lane: Arc::clone(&self.lane),
        })
    }

    /// Interrupt this lane's in-flight turn.
    pub fn interrupt(&self, reason: Option<String>) -> Result<InterruptReceipt> {
        self.shared.interrupt(&self.agent_session, reason)
    }

    /// Answer a permission request that arrived on this lane. The decision is
    /// carried to the provider; the host adds no authority of its own.
    pub fn respond_permission(
        &self,
        request: &NativePermissionRequest,
        decision: PermissionDecision,
    ) -> Result<()> {
        let command = {
            let mut adapter = self.shared.adapter()?;
            adapter.respond_permission(request, decision)?
        };
        self.shared.dispatch(&command)
    }

    /// Receive the next event on this lane, blocking. `None` means the bridge
    /// stopped; read [`AgentSessionHost::transport_error`] for why.
    pub fn recv(&self) -> Option<HostEvent> {
        self.locked(|events| events.recv().ok())
    }

    pub fn try_recv(&self) -> std::result::Result<HostEvent, TryRecvError> {
        self.locked(|events| events.try_recv())
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<HostEvent, RecvTimeoutError> {
        self.locked(|events| events.recv_timeout(timeout))
    }

    fn locked<T>(&self, read: impl FnOnce(&mut EventQueue) -> T) -> T {
        let mut events = self
            .shared
            .lane_events(&self.lane)
            .expect("session lane event lock poisoned");
        let result = read(&mut events);
        drop(events);
        if let Some(error) = self.lane.queue.error() {
            state_stop_bridge(
                &self.shared.state,
                &format!("agent_session_host.event_storage: {error}"),
                false,
            );
            let _ = self.shared.control.terminate();
        }
        result
    }
}

impl TurnHandle {
    pub fn agent_session(&self) -> &ResourceRef {
        &self.lane.agent_session
    }

    /// Next event of this turn, blocking.
    pub fn recv(&self) -> Option<HostEvent> {
        self.locked(|events| events.recv().ok())
    }

    pub fn try_recv(&self) -> std::result::Result<HostEvent, TryRecvError> {
        self.locked(|events| events.try_recv())
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<HostEvent, RecvTimeoutError> {
        self.locked(|events| events.recv_timeout(timeout))
    }

    /// Interrupt this turn from the thread that is streaming it.
    pub fn interrupt(&self, reason: Option<String>) -> Result<InterruptReceipt> {
        self.shared.interrupt(&self.lane.agent_session, reason)
    }

    /// Drain the lane until this turn ends and return its record. A lane
    /// carries one turn at a time, so everything read here belongs to this
    /// turn.
    pub fn wait(self) -> Result<TurnRecord> {
        loop {
            match self.recv() {
                Some(HostEvent::TurnEnded(record)) => return Ok(record),
                Some(HostEvent::Signal(_)) => continue,
                None => {
                    return Err(self
                        .shared
                        .transport_error("the session bridge stopped before the turn ended"))
                }
            }
        }
    }

    /// [`TurnHandle::wait`] with a bound, so a provider that never answers a
    /// cancel cannot hold the caller forever. On [`WaitOutcome::Timeout`] the
    /// turn is still in flight and may still be waited on or interrupted.
    pub fn wait_timeout(&self, timeout: Duration) -> Result<WaitOutcome> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(WaitOutcome::Timeout);
            }
            match self.recv_timeout(remaining) {
                Ok(HostEvent::TurnEnded(record)) => {
                    return Ok(WaitOutcome::Finished(Box::new(record)))
                }
                Ok(HostEvent::Signal(_)) => continue,
                Err(RecvTimeoutError::Timeout) => return Ok(WaitOutcome::Timeout),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self
                        .shared
                        .transport_error("the session bridge stopped before the turn ended"))
                }
            }
        }
    }

    fn locked<T>(&self, read: impl FnOnce(&mut EventQueue) -> T) -> T {
        let mut events = self
            .shared
            .lane_events(&self.lane)
            .expect("session lane event lock poisoned");
        let result = read(&mut events);
        drop(events);
        if let Some(error) = self.lane.queue.error() {
            state_stop_bridge(
                &self.shared.state,
                &format!("agent_session_host.event_storage: {error}"),
                false,
            );
            let _ = self.shared.control.terminate();
        }
        result
    }
}

impl HostShared {
    // ---------------------------------------------------------------- reading

    fn serve(self: &Arc<Self>, mut reader: ConnectionReader) {
        loop {
            let raw = match reader.read_json() {
                Ok(raw) => raw,
                Err(error) => {
                    self.record_transport_failure(error);
                    return;
                }
            };
            if !self.serve_message(raw) {
                return;
            }
        }
    }

    /// Interpret one wire message and deliver what it resolved. Returns `false`
    /// when the bridge must stop.
    fn serve_message(self: &Arc<Self>, raw: Value) -> bool {
        let response_token = response_token(&raw);
        let mut waiter = None;
        let mut prompt_session = None;
        if let Some(token) = &response_token {
            let Ok(mut state) = self.state.lock() else {
                return false;
            };
            waiter = state.control.remove(token);
            prompt_session = state.pending_prompts.remove(token);
        }

        let ingested = {
            let Ok(mut adapter) = self.adapter.lock() else {
                self.record_transport_failure(adapter_lock_poisoned());
                return false;
            };
            adapter.ingest(raw)
        };

        let signals = match ingested {
            Ok(signals) => signals,
            Err(error) => {
                // A response the host can correlate to something it asked for is
                // that operation's failure, not the transport's. Anything else
                // means this stream can no longer be interpreted.
                if let Some(waiter) = waiter {
                    waiter.send(ControlDelivery::Failed(error.to_string()));
                    return true;
                }
                if let Some(native_session_id) = prompt_session {
                    let deliveries =
                        state_fail_turn(&self.state, &native_session_id, &error.to_string());
                    self.deliver(deliveries);
                    return true;
                }
                self.record_transport_failure(error);
                return false;
            }
        };

        match waiter {
            Some(ControlWaiter::Handshake(sender)) => {
                let _ = sender.send(ControlDelivery::Signals(signals));
            }
            Some(ControlWaiter::Open { sender, lane }) => {
                state_register_open(&self.state, &lane, &signals);
                let _ = sender.send(ControlDelivery::Signals(signals));
            }
            None => self.route(signals),
        }
        true
    }

    /// Route interpreted signals to their lanes and turn bookkeeping.
    fn route(self: &Arc<Self>, signals: Vec<ConnectionSignal>) {
        let mut deliveries: Vec<(Arc<LaneCore>, HostEvent)> = Vec::new();
        let mut limited = Vec::new();
        {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            for signal in signals {
                state.last_sequence = state.last_sequence.max(signal.sequence);
                let Some(native_session_id) = signal.native_session_id.clone() else {
                    state.push_unattributed(signal);
                    continue;
                };
                let Some(lane) = state.lanes.get(&native_session_id).cloned() else {
                    state.push_unattributed(signal);
                    continue;
                };
                if let Some(turn) = state.turns.get_mut(&native_session_id) {
                    turn.signals += 1;
                    turn.last_sequence = signal.sequence;
                    if turn.first_sequence.is_none() {
                        turn.first_sequence = Some(signal.sequence);
                    }
                }
                let terminal = matches!(
                    signal.kind,
                    ConnectionSignalKind::Completed { .. }
                        | ConnectionSignalKind::Cancelled
                        | ConnectionSignalKind::Failed { .. }
                );
                deliveries.push((Arc::clone(&lane), HostEvent::Signal(signal.clone())));
                if terminal {
                    if let Some(record) = state.close_turn(&native_session_id, &signal) {
                        state.record_turn(&record);
                        deliveries.push((lane, HostEvent::TurnEnded(record)));
                    }
                } else if state.reached_limit(&native_session_id, self.limits.max_signals_per_turn)
                {
                    limited.push(native_session_id);
                }
            }
        }
        self.deliver(deliveries);
        for native_session_id in limited {
            let commands = self.adapter().and_then(|mut adapter| {
                adapter.coordinated_cancel(CancelRequest { native_session_id })
            });
            match commands {
                Ok(commands) => {
                    for command in commands {
                        if let Err(error) = self.dispatch(&command) {
                            self.record_transport_failure(error);
                            return;
                        }
                    }
                }
                Err(error) => {
                    self.record_transport_failure(error);
                    return;
                }
            }
        }
    }

    fn record_transport_failure(self: &Arc<Self>, error: AikitError) {
        state_stop_bridge(&self.state, &error.to_string(), false);
    }

    fn deliver(&self, deliveries: Vec<(Arc<LaneCore>, HostEvent)>) {
        for (lane, event) in deliveries {
            if let Err(error) = lane.deliver(event) {
                state_stop_bridge(
                    &self.state,
                    &format!(
                        "agent_session_host.event_storage: owner event storage failed: {error}"
                    ),
                    false,
                );
                let _ = self.control.terminate();
                return;
            }
        }
    }

    // ------------------------------------------------------------- operations

    fn dispatch(&self, command: &ConnectionCommand) -> Result<()> {
        let _io = lock(&self.io)?;
        if command.operation == "interrupt" {
            return self.control.interrupt();
        }
        self.writer.send_json(command)
    }

    fn register_control(
        &self,
        command: &ConnectionCommand,
        open: Option<Arc<LaneCore>>,
    ) -> Result<Receiver<ControlDelivery>> {
        let token = control_token(&command.payload)?;
        let (sender, receiver) = mpsc::channel();
        let waiter = match open {
            Some(lane) => ControlWaiter::Open { sender, lane },
            None => ControlWaiter::Handshake(sender),
        };
        let mut state = lock(&self.state)?;
        if state.closed {
            return Err(stopped_bridge_error(
                &state,
                "the session bridge has stopped",
            ));
        }
        state.control.insert(token, waiter);
        Ok(receiver)
    }

    fn begin_turn(&self, agent_session: &ResourceRef, command: &ConnectionCommand) -> Result<()> {
        let token = control_token(&command.payload)?;
        let native_session_id = {
            let state = lock(&self.state)?;
            state
                .sessions
                .get(agent_session)
                .map(|record| record.binding.native_session_id.clone())
                .ok_or_else(|| session_not_open(agent_session))?
        };
        let mut state = lock(&self.state)?;
        if state.closed {
            return Err(stopped_bridge_error(
                &state,
                "the session bridge has stopped",
            ));
        }
        if state.turns.contains_key(&native_session_id) {
            return Err(AikitError::new(
                "agent_session_host.turn_already_in_flight",
                format!(
                    "native session {native_session_id} already has a turn in flight; a session \
                     carries one turn at a time"
                ),
            ));
        }
        let last_sequence = state.last_sequence;
        state
            .pending_prompts
            .insert(token, native_session_id.clone());
        state.turns.insert(
            native_session_id,
            ActiveTurn {
                agent_session: agent_session.clone(),
                first_sequence: None,
                last_sequence,
                signals: 0,
                interrupt: None,
                operational_limit: None,
            },
        );
        Ok(())
    }

    fn interrupt(
        self: &Arc<Self>,
        agent_session: &ResourceRef,
        reason: Option<String>,
    ) -> Result<InterruptReceipt> {
        let native_session_id;
        let requested_at_sequence;
        {
            let mut state = lock(&self.state)?;
            if state.closed {
                return Err(stopped_bridge_error(
                    &state,
                    "the session bridge has stopped",
                ));
            }
            let record = state
                .sessions
                .get(agent_session)
                .ok_or_else(|| session_not_open(agent_session))?;
            let native = record.binding.native_session_id.clone();
            let turn = state.turns.get_mut(&native).ok_or_else(|| {
                AikitError::new(
                    "agent_session_host.no_turn_in_flight",
                    format!("native session {native} has no turn in flight to interrupt"),
                )
            })?;
            if turn.interrupt.is_some() {
                return Err(AikitError::new(
                    "agent_session_host.interrupt_already_requested",
                    format!("an interrupt is already in flight for native session {native}"),
                ));
            }
            requested_at_sequence = turn.last_sequence;
            turn.interrupt = Some(PendingInterrupt {
                reason: reason.clone(),
                requested_at_sequence,
                commands: Vec::new(),
            });
            native_session_id = native;
        }
        let commands = {
            let mut adapter = self.adapter()?;
            adapter.coordinated_cancel(CancelRequest {
                native_session_id: native_session_id.clone(),
            })?
        };
        let operations: Vec<String> = commands.iter().map(|c| c.operation.clone()).collect();
        // The turn may have closed while the cancel was being prepared: the
        // terminal signal and this request race across the adapter lock. Only
        // cancel a turn that is still the one that was interrupted.
        let turn_still_open = {
            let mut state = lock(&self.state)?;
            match state
                .turns
                .get_mut(&native_session_id)
                .and_then(|turn| turn.interrupt.as_mut())
            {
                Some(pending) if pending.requested_at_sequence == requested_at_sequence => {
                    pending.commands = operations.clone();
                    true
                }
                _ => false,
            }
        };
        if !turn_still_open {
            return Err(AikitError::new(
                "agent_session_host.no_turn_in_flight",
                format!(
                    "native session {native_session_id} ended its turn before the cancel was \
                     issued"
                ),
            ));
        }
        for command in &commands {
            self.dispatch(command)?;
        }
        Ok(InterruptReceipt {
            agent_session: agent_session.clone(),
            native_session_id,
            commands: operations,
            requested_at_sequence,
            reason,
        })
    }

    fn await_control(&self, receiver: Receiver<ControlDelivery>) -> Result<ControlDelivery> {
        match receiver.recv() {
            Ok(delivery) => Ok(delivery),
            Err(_) => {
                let mut state = lock(&self.state)?;
                match state.transport_error.clone() {
                    Some(reason) => Err(AikitError::new(
                        "agent_session_host.transport_closed",
                        reason,
                    )),
                    None => {
                        state.closed = true;
                        Err(AikitError::new(
                            "agent_session_host.control_abandoned",
                            "the host stopped before the control response arrived",
                        ))
                    }
                }
            }
        }
    }

    fn adapter(&self) -> Result<MutexGuard<'_, Box<dyn InteractiveAgentConnectionAdapter + Send>>> {
        lock(&self.adapter)
    }

    fn state(&self) -> Result<MutexGuard<'_, HostState>> {
        lock(&self.state)
    }

    fn gate(&self) -> Result<MutexGuard<'_, ()>> {
        lock(&self.control_gate)
    }

    fn lane_events<'a>(&self, lane: &'a LaneCore) -> Result<MutexGuard<'a, EventQueue>> {
        lock(&lane.events)
    }

    fn transport_error(&self, fallback: &str) -> AikitError {
        match self.state.lock() {
            Ok(state) => stopped_bridge_error(&state, fallback),
            Err(_) => AikitError::new("agent_session_host.transport_closed", fallback),
        }
    }
}

/// The stopped-bridge error, read from a state lock the caller already holds.
/// [`HostShared::transport_error`] re-locks `state`, so a caller holding the
/// state guard must use this instead — re-locking here is a self-deadlock.
fn stopped_bridge_error(state: &HostState, fallback: &str) -> AikitError {
    AikitError::new(
        "agent_session_host.transport_closed",
        state
            .transport_error
            .clone()
            .unwrap_or_else(|| fallback.to_owned()),
    )
}

/// Register an opened session for its native id *before* its caller is woken,
/// so no later update can outrun the registration.
fn state_register_open(
    state: &Mutex<HostState>,
    lane: &Arc<LaneCore>,
    signals: &[ConnectionSignal],
) {
    let Ok(mut state) = state.lock() else {
        return;
    };
    for signal in signals {
        if let ConnectionSignalKind::SessionOpened { binding } = &signal.kind {
            state
                .lanes
                .insert(binding.native_session_id.clone(), Arc::clone(lane));
            state.sessions.insert(
                lane.agent_session.clone(),
                SessionRecord {
                    binding: binding.clone(),
                },
            );
        }
    }
}

/// Close one turn because its prompt failed at the protocol level.
fn state_fail_turn(
    state: &Mutex<HostState>,
    native_session_id: &str,
    reason: &str,
) -> Vec<(Arc<LaneCore>, HostEvent)> {
    let mut deliveries = Vec::new();
    let Ok(mut state) = state.lock() else {
        return deliveries;
    };
    if let Some(record) = state.fail_turn(native_session_id, reason) {
        state.record_turn(&record);
        if let Some(lane) = state.lanes.get(native_session_id).cloned() {
            deliveries.push((lane, HostEvent::TurnEnded(record)));
        }
    }
    deliveries
}

/// The bridge stops, deliberately or not: queue every in-flight turn's honest
/// record on its lane, then end every lane, so a caller parked on a transport
/// that can no longer produce anything drains its record and then sees the
/// end. A deliberate stop is not a transport failure.
fn state_stop_bridge(state: &Mutex<HostState>, reason: &str, deliberate: bool) {
    let Ok(mut state) = state.lock() else {
        return;
    };
    if state.closed {
        return;
    }
    let natives: Vec<String> = state.turns.keys().cloned().collect();
    for native_session_id in natives {
        if let Some(record) = state.fail_turn(&native_session_id, reason) {
            state.record_turn(&record);
            if let Some(lane) = state.lanes.get(&native_session_id).cloned() {
                // Queue the record before the lane ends: closing the sender
                // first would drop the very event that explains the stop.
                let _ = lane.deliver(HostEvent::TurnEnded(record));
            }
        }
    }
    if !deliberate {
        state.transport_error = Some(reason.to_owned());
    }
    state.closed = true;
    state.control.clear();
    state.pending_prompts.clear();
    for (_, lane) in std::mem::take(&mut state.lanes) {
        lane.close();
    }
}

impl HostState {
    fn lane_state(&self, native_session_id: &str) -> SessionLaneState {
        match self.turns.get(native_session_id) {
            Some(turn) if turn.interrupt.is_some() => SessionLaneState::InterruptRequested,
            Some(_) => SessionLaneState::TurnInFlight,
            None => SessionLaneState::Resident,
        }
    }

    fn push_unattributed(&mut self, signal: ConnectionSignal) {
        if self.unattributed.len() >= UNATTRIBUTED_LIMIT {
            self.unattributed.pop_front();
        }
        self.unattributed.push_back(signal);
    }

    fn record_turn(&mut self, record: &TurnRecord) {
        if let Some(interruption) = &record.interruption {
            let trail = self.trail.entry(record.agent_session.clone()).or_default();
            if trail.len() == 128 {
                trail.pop_front();
            }
            trail.push_back(interruption.clone());
        }
        self.last_turn
            .insert(record.agent_session.clone(), record.clone());
    }

    /// Close the turn on `native_session_id` because `signal` stopped it.
    fn close_turn(
        &mut self,
        native_session_id: &str,
        signal: &ConnectionSignal,
    ) -> Option<TurnRecord> {
        let turn = self.turns.remove(native_session_id)?;
        let stop = if let Some(max_signals) = turn.operational_limit {
            TurnStop::OperationalLimit { max_signals }
        } else {
            match &signal.kind {
                ConnectionSignalKind::Completed { stop_reason } => TurnStop::Completed {
                    stop_reason: stop_reason.clone(),
                },
                ConnectionSignalKind::Failed { reason } => TurnStop::Failed {
                    reason: reason.clone(),
                },
                _ => TurnStop::Cancelled,
            }
        };
        let interruption = match (&turn.interrupt, &stop) {
            // A cancel was asked for and the provider stopped the turn: a human
            // interruption, however the stop was carried on the wire.
            (Some(pending), TurnStop::Cancelled) => Some(TurnInterruption {
                agent_session: turn.agent_session.clone(),
                native_session_id: native_session_id.to_owned(),
                origin: InterruptOrigin::Human,
                reason: pending.reason.clone(),
                commands: pending.commands.clone(),
                requested_at_sequence: Some(pending.requested_at_sequence),
                observed_stop: stop.clone(),
            }),
            // The turn ran to completion despite the cancel. That is not an
            // interruption, and recording one would put a human act on the
            // trail for a turn nobody stopped.
            (Some(_), TurnStop::Completed { .. }) => None,
            (None, TurnStop::Cancelled) => Some(TurnInterruption {
                agent_session: turn.agent_session.clone(),
                native_session_id: native_session_id.to_owned(),
                origin: InterruptOrigin::Provider,
                reason: None,
                commands: Vec::new(),
                requested_at_sequence: None,
                observed_stop: TurnStop::Cancelled,
            }),
            (None, _) => None,
            // A transport failure is `fail_turn`'s record to make; `close_turn`
            // only closes turns the provider stopped on the wire.
            (Some(_), TurnStop::Failed { .. } | TurnStop::OperationalLimit { .. }) => None,
        };
        Some(TurnRecord {
            agent_session: turn.agent_session.clone(),
            binding: self.binding_for(native_session_id),
            stop,
            interruption,
            first_sequence: turn.first_sequence,
            last_sequence: turn.last_sequence,
            signals: turn.signals,
        })
    }

    /// Close a turn that never observed a stop.
    fn fail_turn(&mut self, native_session_id: &str, reason: &str) -> Option<TurnRecord> {
        let turn = self.turns.remove(native_session_id)?;
        let interruption = turn.interrupt.as_ref().map(|pending| TurnInterruption {
            agent_session: turn.agent_session.clone(),
            native_session_id: native_session_id.to_owned(),
            origin: InterruptOrigin::Human,
            reason: pending.reason.clone(),
            commands: pending.commands.clone(),
            requested_at_sequence: Some(pending.requested_at_sequence),
            observed_stop: TurnStop::Failed {
                reason: reason.to_owned(),
            },
        });
        Some(TurnRecord {
            agent_session: turn.agent_session.clone(),
            binding: self.binding_for(native_session_id),
            stop: TurnStop::Failed {
                reason: reason.to_owned(),
            },
            interruption,
            first_sequence: turn.first_sequence,
            last_sequence: turn.last_sequence,
            signals: turn.signals,
        })
    }

    /// An explicit operational ceiling requests cancellation and leaves the
    /// turn in flight until a native terminal event is observed.
    fn reached_limit(&mut self, native_session_id: &str, limit: usize) -> bool {
        let Some(turn) = self.turns.get_mut(native_session_id) else {
            return false;
        };
        if limit == 0 || turn.signals <= limit || turn.operational_limit.is_some() {
            return false;
        }
        turn.operational_limit = Some(limit);
        true
    }

    fn binding_for(&self, native_session_id: &str) -> NativeSessionBinding {
        self.lanes
            .get(native_session_id)
            .and_then(|lane| self.sessions.get(&lane.agent_session))
            .map(|record| record.binding.clone())
            .unwrap_or_else(|| {
                NativeSessionBinding::unbound(
                    native_session_id,
                    crate::agent_connection::SessionOpenMode::Create,
                )
            })
    }
}

fn session_not_open(agent_session: &ResourceRef) -> AikitError {
    AikitError::new(
        "agent_session_host.session_not_open",
        format!(
            "no native session is bound to canonical AgentSession {agent_session} on this host"
        ),
    )
}

fn adapter_lock_poisoned() -> AikitError {
    AikitError::new(
        "agent_session_host.lock_poisoned",
        "the session host adapter lock was poisoned by a failed session thread",
    )
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| {
        AikitError::new(
            "agent_session_host.lock_poisoned",
            "a session host lock was poisoned by a failed session thread",
        )
    })
}

/// The JSON-RPC id of an outbound host command, in the token form the inbound
/// correlation uses.
fn control_token(payload: &Value) -> Result<String> {
    match payload.get("id") {
        Some(Value::String(value)) => Ok(format!("s:{value}")),
        Some(Value::Number(value)) if value.as_i64().is_some() => {
            Ok(format!("n:{}", value.as_i64().unwrap_or_default()))
        }
        Some(Value::Null) => Ok("null".into()),
        _ => Err(AikitError::new(
            "agent_session_host.invalid_control_request",
            "host control request carries no correlatable JSON-RPC id",
        )),
    }
}

/// The correlation token of an inbound response, if it is one.
fn response_token(message: &Value) -> Option<String> {
    if message.get("method").is_some() {
        return None;
    }
    match message.get("id") {
        Some(Value::String(value)) => Some(format!("s:{value}")),
        Some(Value::Number(value)) if value.as_i64().is_some() => {
            Some(format!("n:{}", value.as_i64().unwrap_or_default()))
        }
        Some(Value::Null) => Some("null".into()),
        _ => None,
    }
}
