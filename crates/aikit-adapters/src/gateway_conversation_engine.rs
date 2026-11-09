//! The gateway conversation engine: the plane between an admitted inbound
//! message and the outbound reply.
//!
//! When an admitted inbound message arrives for a connector-config-backed
//! binding, the engine runs an actual agent turn through a
//! [`ConversationTurnSource`] and delivers the answer back through the same
//! connector: the reply is appended to the *same* Stream journal the inbound
//! event appended to (never a fabricated inbound human event), a Send
//! operation answering the inbound native message rides the existing outbound
//! queue to the connector's pump, and the pump records the delivery receipt
//! exactly as it does for any other prepared operation.
//!
//! Conversation control exists here as canonical operations
//! ([`GatewayConversationOperation`], shared with the kernel's
//! `GatewayCommand::Conversation`). Connector ingress may spell them with
//! slash strings at the surface edge — [`parse_slash`] is that edge, and it is
//! the only place strings become operations. The same operations are callable
//! from any carrier through `GatewayCommand::Conversation`.
//!
//! `/ask` is the cross-face edge: in a connector conversation it sends one
//! attributable Communique from the asking agency to the named Position,
//! routed exactly as `gateway send` routes — local occupancy, cross-Workcell
//! relay, held/vacant — and answers the chat honestly with the same three-part
//! outcomes. Attribution comes from occupancy, never from the chat: the ask
//! router names the asking agency's Position from Actuation's ledger (this
//! conversation's agent session, or the agency it is bound to), or the
//! Communique is labelled `<unknown sender>` with the connector provenance
//! carried in its attribution basis. The append is kernel work done here, in
//! process, through the gateway's own journal; what the engine does not hold —
//! Central's Positions, Actuation's ledger, the declared remotes — is resolved
//! by the [`GatewayAskRouter`] the service wires (the same inversion as the
//! occupancy reader). A refusal answers before anything is appended: nothing
//! is recorded on a refusal.
//!
//! Restart law: a requested restart is a drained, state-preserving
//! rematerialisation. The engine stops admitting new work, resolves the
//! in-flight turn under an explicit bounded policy (grace, then interrupt,
//! recorded honestly through the host's own interruption receipt), persists
//! the snapshot, answers the requesting surface, and the service then exits
//! cleanly so its service manager rematerialises it. Semantic identity never
//! changes across the restart: the snapshot is the same one a crash would
//! restore from.
//!
//! Steering and stopping: a plain message that arrives while the binding's
//! turn plane is busy — a turn in flight, or one still starting — is queued
//! and becomes the next turn in arrival order. Steering at the turn boundary;
//! the message is never lost to the lane's one-turn-at-a-time guard. `/stop`
//! interrupts the in-flight turn through the harness's own cancel and drops
//! whatever was queued; the conversation's next message starts fresh. The
//! plane stays one-turn-at-a-time on purpose: a backing lane carries one
//! turn, and the queue is how the conversation steers it. `/new` is a hard
//! cut: it drops the queue with the old context and names the drop.
//!
//! The engine is deterministic-testable: the turn source is a small trait with
//! the scripted [`FixtureTurnSource`] for the deterministic suite, and the
//! [`AgentHostTurnSource`] backed by [`AgentSessionHost`] for a connector
//! config that names a harness. Deterministic proof never requires a harness
//! binary.
//!
//! Live replies: when the binding's connector declares the Streaming
//! capability, the sender watches the turn work. The typing indicator begins
//! at admission — the turn worker's first act, before any harness process is
//! spawned — and is refreshed on a bounded cadence until the turn's last
//! outbound message is queued; no `Typing(active)` is ever queued after the
//! final message. Each completed assistant text segment arrives as its own
//! message the moment the wire closes it (the real rpc wires complete whole
//! segments; there are no fine-grained deltas to grow), and each tool use
//! arrives as its own short, honest line between the segments. A connector
//! that does not declare Streaming, and a turn whose source produces no live
//! progress, keeps the exact send-one-final-reply behaviour; with streaming
//! off, tool lines are journalled but never sent.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_connection::{ConnectionSignalKind, SessionOpenMode, SessionOpenRequest};
use crate::agent_session_host::{
    AgentSessionHost, AgentSessionHostLimits, HostEvent, InterruptReceipt, TurnHandle, TurnStop,
};
use crate::gateway_communique::{
    Communique, CommuniqueCount, CommuniqueDraft, CommuniqueForwardOutcome, CommuniqueState,
    SenderAttribution,
};
use crate::gateway_connector::{ConnectorOperation, OutboundOperationKind};
use crate::gateway_connector_pump::{ConnectorPumpControls, ConnectorQueues};
use crate::gateway_runtime::{
    execute_gateway_command, AgencyGateway, GatewayAgentReply, GatewayAgentReplyFailure,
    GatewayBinding, GatewayCommand, GatewayConversationOperation, GatewayResponse,
    GatewayStreamEvent,
};
use crate::gateway_service::{persist_gateway_state, SubscriptionHub};

/// Timing discipline for the streaming reply path: explicit, bounded, and
/// deterministic-testable. The defaults are what production runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamTiming {
    /// How often the streaming wait wakes to read the turn's progress.
    pub poll: Duration,
    /// How often the typing indicator is refreshed while a turn runs, so it
    /// reads as "responding…" for the whole turn, not a five-second blip
    /// (Telegram expires the indicator after ~5s; the cadence stays under it).
    pub typing_refresh: Duration,
}

impl Default for StreamTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(150),
            typing_refresh: Duration::from_secs(4),
        }
    }
}

/// Bounded policy for resolving work at a restart drain and for interruption
/// waits. Explicit, never ambient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnginePolicy {
    /// How long a restart drain waits for an in-flight turn to finish before
    /// interrupting it.
    pub turn_grace: Duration,
    /// How long an interrupted turn is waited for after the interrupt was
    /// issued, before the drain records the interruption and moves on.
    pub interrupt_grace: Duration,
    /// The streaming reply path's timing. Explicit, never ambient.
    pub stream: StreamTiming,
}

impl Default for EnginePolicy {
    fn default() -> Self {
        Self {
            turn_grace: Duration::from_secs(3),
            interrupt_grace: Duration::from_secs(5),
            stream: StreamTiming::default(),
        }
    }
}

/// One turn request: the prompt the inbound conversation admitted, with the
/// binding identity the turn is attributed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationTurnRequest {
    pub binding_ref: ResourceRef,
    pub agent_session_ref: ResourceRef,
    pub prompt: String,
    /// The stream sequence of the inbound event this turn answers.
    pub in_reply_to_sequence: u64,
}

/// How a turn actually ended, from the turn source's own observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationTurnOutcome {
    Replied { text: String },
    Failed { reason: String },
    Interrupted { detail: Option<String> },
}

/// One ordered item of a running turn's live progress: a completed assistant
/// text segment, or one honest tool line — exactly the facts a streaming wire
/// publishes, in the order it published them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStreamItem {
    Segment { text: String },
    ToolLine { line: String },
}

/// A running turn's live progress: an ordered queue of stream items shared
/// between the turn's reader thread and the engine's streaming loop. The
/// reader appends what the wire completes; the loop drains it and delivers
/// each item as its own message.
#[derive(Default)]
pub struct TurnProgress {
    items: Mutex<VecDeque<TurnStreamItem>>,
}

impl TurnProgress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one completed assistant text segment. An empty (or blank) text
    /// is ignored: no wire fact, no item.
    pub fn push_segment(&self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        if let Ok(mut items) = self.items.lock() {
            items.push_back(TurnStreamItem::Segment { text });
        }
    }

    /// Append one honest tool line. An empty (or blank) line is ignored.
    pub fn push_tool_line(&self, line: impl Into<String>) {
        let line = line.into();
        if line.trim().is_empty() {
            return;
        }
        if let Ok(mut items) = self.items.lock() {
            items.push_back(TurnStreamItem::ToolLine { line });
        }
    }

    /// Take everything published so far, in wire order.
    pub fn drain(&self) -> Vec<TurnStreamItem> {
        match self.items.lock() {
            Ok(mut items) => items.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// A live turn: waitable to its outcome, interruptible on request. The receipt
/// returned by [`ConversationTurn::interrupt`] is what the host actually
/// issued — recorded honestly, never invented.
pub trait ConversationTurn: Send + Sync {
    fn wait(&self) -> ConversationTurnOutcome;
    fn wait_timeout(&self, timeout: Duration) -> Option<ConversationTurnOutcome>;
    fn interrupt(&self, reason: Option<String>) -> Result<String>;
    /// Whether a terminal outcome was recorded. A finished turn whose
    /// `wait_timeout` still answers None has had its outcome taken by another
    /// owner — the restart drain — and this waiter's duty is over.
    fn finished(&self) -> bool {
        false
    }
    /// The turn's live progress cell, when its source produces one. The
    /// default is None: a source without progress streams nothing and its
    /// reply lands exactly as it did before.
    fn progress(&self) -> Option<Arc<TurnProgress>> {
        None
    }
}

/// Where turns come from. One implementation per agent backing: the scripted
/// [`FixtureTurnSource`] for deterministic proof, the [`AgentHostTurnSource`]
/// for a real harness.
pub trait ConversationTurnSource: Send + Sync {
    /// The harness backing this source, as status discloses it.
    fn harness(&self) -> Option<String> {
        None
    }
    /// Begin one turn. The handle is registered with the engine so a canonical
    /// Stop can interrupt it and a restart drain can resolve it.
    fn prompt(&self, request: ConversationTurnRequest) -> Result<Arc<dyn ConversationTurn>>;
    /// Open a fresh turn context for a regenerated binding (canonical New).
    /// What happened to the previous context is named by the engine's result.
    fn reset(&self, agent_session: &ResourceRef) -> Result<()>;
    /// The sessions this source currently holds.
    fn sessions(&self) -> Result<Vec<Value>>;
    /// The harness's own native model selector, read for this binding's agent
    /// session. The harness's controls are the substance of a model selector;
    /// the gateway exposes them and invents no parallel notion. The default is
    /// the honest refusal of a source with no such seam.
    fn model_controls(&self, _agent_session: &ResourceRef) -> Result<Value> {
        Err(AikitError::new(
            "gateway_conversation.model_controls_unsupported",
            "this turn source does not expose the harness's model selector",
        ))
    }
    /// Select a provider-advertised model through the harness's own native
    /// seam, and answer with the receipt the harness confirmed.
    fn set_model(&self, _agent_session: &ResourceRef, _model: &str) -> Result<Value> {
        Err(AikitError::new(
            "gateway_conversation.model_selection_unsupported",
            "this turn source cannot select a model",
        ))
    }
    /// The aikit skill surface available to the backed harness. The harness
    /// carries skills in-turn; a listing here is a disclosure of that surface,
    /// never an executor. The default is an honest empty disclosure.
    fn skills(&self) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }
}

/// Resolves the turn source for a binding's connector, from the connector
/// configuration's agent backing. `None` means the connector names no agent
/// backing and the gateway stays a journal plane for it.
pub trait GatewayTurnSourceResolver: Send + Sync {
    fn turn_source_for(
        &self,
        connector_ref: &ResourceRef,
        platform: &str,
    ) -> Option<Arc<dyn ConversationTurnSource>>;
    /// The backings this resolver could name for a connector, as the provider
    /// registry names them (id, label, protocol — no credentials, no argv).
    /// Read-only disclosure for the canonical Harness listing.
    fn available_backings(&self) -> Vec<Value> {
        Vec::new()
    }
}

/// One connector-originated ask, as the world around the gateway must read
/// it: who is asking (the binding's semantic identity and the connector
/// conversation it arrived through) and what is asked of whom. The chat text
/// never carries identity; this request does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayAskRequest {
    pub binding_ref: ResourceRef,
    pub connector_ref: String,
    pub agent_session_ref: String,
    pub agency_ref: String,
    pub platform: String,
    pub conversation_id: String,
    /// The recipient as the asker spelled it: a Position ref or an @handle.
    pub recipient: String,
    pub message: String,
}

/// A routed ask: the draft shaped for acceptance (attribution, recipient and
/// routing already resolved by the owners, origin provenance carried in the
/// attribution basis), and the delivery notice the asker's chat is owed.
#[derive(Debug, Clone, PartialEq)]
pub struct GatewayAskRoute {
    pub draft: CommuniqueDraft,
    pub recipient_position_ref: String,
    /// The structured delivery notice for a held or occupancy-unreadable
    /// route, in `gateway send`'s three-part shape (`fact`, `consequence`,
    /// `action`); `Value::Null` when the plain pending sentence says
    /// everything.
    pub delivery: Value,
}

/// The inverted hook behind `/ask`. Resolves a connector-originated ask with
/// the exact `gateway send` laws — Central names the recipient, Actuation
/// names occupancy and the asking agency's own Position, the declared remotes
/// carry relay — and relays an appended record to the Workcell its route
/// names. The engine holds the journal; the router holds the owners. It is
/// the same inversion as `GatewayOccupancyReader`: production wires it at
/// service assembly, and the deterministic suite scripts it.
pub trait GatewayAskRouter: Send + Sync {
    /// Resolve recipient, attribution and routing, or refuse with the
    /// three-part send refusal. Nothing is recorded on a refusal: the engine
    /// appends only after this answers.
    fn route(&self, ask: &GatewayAskRequest) -> Result<GatewayAskRoute>;
    /// One relay attempt of an already-appended record to the Workcell its
    /// route names, `relayed_by` naming this gateway. A remote that cannot be
    /// reached is a `Failed` outcome (the record stays queued for the next
    /// relay pass), never an error.
    fn relay(&self, communique: &Communique, relayed_by: &str) -> Result<CommuniqueForwardOutcome>;
}

/// A completion slot shared between a turn's worker thread and its waiters.
/// Both the fixture and the real host turn source park their outcome here.
#[derive(Default)]
pub struct TurnSlot {
    outcome: Mutex<Option<ConversationTurnOutcome>>,
    signal: Condvar,
    /// Set once a terminal outcome was recorded; stays set after a waiter
    /// consumes the outcome, so a finished turn never reads as unfinished.
    finished: std::sync::atomic::AtomicBool,
    /// The turn's live progress cell, when its source attached one.
    progress: Option<Arc<TurnProgress>>,
}

impl TurnSlot {
    /// A slot that also carries the turn's live progress cell.
    pub fn with_progress(progress: Arc<TurnProgress>) -> Self {
        Self {
            outcome: Mutex::new(None),
            signal: Condvar::new(),
            finished: std::sync::atomic::AtomicBool::new(false),
            progress: Some(progress),
        }
    }

    /// The turn's live progress cell, when the source attached one.
    pub fn progress(&self) -> Option<Arc<TurnProgress>> {
        self.progress.clone()
    }

    pub fn complete(&self, outcome: ConversationTurnOutcome) {
        // The outcome is stored before `finished` is raised, both under the
        // slot lock: a waiter that reads `finished` then finds the outcome
        // either still here or already taken by another owner — never not
        // yet stored.
        if let Ok(mut guard) = self.outcome.lock() {
            if guard.is_none() {
                *guard = Some(outcome);
            }
            self.finished
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.signal.notify_all();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ConversationTurn for TurnSlot {
    fn progress(&self) -> Option<Arc<TurnProgress>> {
        self.progress.clone()
    }

    fn wait(&self) -> ConversationTurnOutcome {
        let mut guard = self.outcome.lock().expect("turn slot");
        loop {
            if let Some(outcome) = guard.take() {
                return outcome;
            }
            guard = self.signal.wait(guard).expect("turn slot");
        }
    }

    fn wait_timeout(&self, timeout: Duration) -> Option<ConversationTurnOutcome> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.outcome.lock().expect("turn slot");
        loop {
            if let Some(outcome) = guard.take() {
                return Some(outcome);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (waited, _) = self
                .signal
                .wait_timeout(guard, deadline - now)
                .expect("turn slot");
            guard = waited;
        }
    }

    fn interrupt(&self, reason: Option<String>) -> Result<String> {
        self.complete(ConversationTurnOutcome::Interrupted {
            detail: reason.clone(),
        });
        Ok(match reason {
            Some(reason) => format!("interrupt issued: {reason}"),
            None => "interrupt issued".into(),
        })
    }

    fn finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// One scripted outcome for the next prompt.
#[derive(Debug, Clone)]
pub enum FixtureScript {
    Reply(String),
    Fail(String),
    /// Park until the test resolves the turn.
    Park,
}

/// The scripted turn source for the deterministic suite. A prompt either takes
/// the next scripted outcome or parks until [`FixtureTurnSource::respond`],
/// [`FixtureTurnSource::fail`] or an interrupt resolves it. No harness binary,
/// no clock dependence beyond explicit bounded waits.
pub struct FixtureTurnSource {
    name: Option<String>,
    script: Mutex<VecDeque<FixtureScript>>,
    turns: Mutex<Vec<Arc<TurnSlot>>>,
    contexts: Mutex<Vec<String>>,
    resets: AtomicUsize,
    current_model: Mutex<String>,
    stall: Mutex<StallGate>,
    stall_signal: Condvar,
}

/// The prompt-stall gate: when armed, the next `prompt` call blocks until
/// released — the deterministic stand-in for a harness process spawning.
#[derive(Default)]
struct StallGate {
    armed: bool,
    stalled: bool,
    released: bool,
}

/// The fixture's deterministic model roster, in disclosure order.
pub const FIXTURE_MODELS: [&str; 3] = ["fixture/opus", "fixture/sonnet", "fixture/haiku"];

/// The fixture's default selection from that roster.
pub const FIXTURE_DEFAULT_MODEL: &str = "fixture/sonnet";

impl FixtureTurnSource {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            script: Mutex::new(VecDeque::new()),
            turns: Mutex::new(Vec::new()),
            contexts: Mutex::new(Vec::new()),
            resets: AtomicUsize::new(0),
            current_model: Mutex::new(FIXTURE_DEFAULT_MODEL.into()),
            stall: Mutex::new(StallGate::default()),
            stall_signal: Condvar::new(),
        }
    }

    pub fn unnamed() -> Self {
        Self::named("fixture")
    }

    /// Script the next prompt to reply with `text`.
    pub fn script_reply(&self, text: impl Into<String>) -> &Self {
        self.script
            .lock()
            .expect("fixture script")
            .push_back(FixtureScript::Reply(text.into()));
        self
    }

    /// Script the next prompt to fail with `reason`.
    pub fn script_failure(&self, reason: impl Into<String>) -> &Self {
        self.script
            .lock()
            .expect("fixture script")
            .push_back(FixtureScript::Fail(reason.into()));
        self
    }

    /// Script the next prompt to park until resolved.
    pub fn script_park(&self) -> &Self {
        self.script
            .lock()
            .expect("fixture script")
            .push_back(FixtureScript::Park);
        self
    }

    /// Complete the oldest unfinished parked turn with a reply.
    pub fn respond(&self, text: impl Into<String>) {
        self.resolve_parked(ConversationTurnOutcome::Replied { text: text.into() });
    }

    /// Complete the oldest unfinished parked turn with a failure.
    pub fn fail(&self, reason: impl Into<String>) {
        self.resolve_parked(ConversationTurnOutcome::Failed {
            reason: reason.into(),
        });
    }

    fn resolve_parked(&self, outcome: ConversationTurnOutcome) {
        let turns = self.turns.lock().expect("fixture turns");
        for turn in turns.iter() {
            if !turn.is_finished() {
                turn.complete(outcome);
                return;
            }
        }
    }

    /// How many turns this source has ever been prompted for, finished or not.
    pub fn prompted_turns(&self) -> usize {
        self.turns.lock().expect("fixture turns").len()
    }

    /// How many turns are parked unfinished right now.
    pub fn parked_turns(&self) -> usize {
        self.turns
            .lock()
            .expect("fixture turns")
            .iter()
            .filter(|turn| !turn.is_finished())
            .count()
    }

    /// Script one completed text segment into the oldest unfinished turn's
    /// live progress — the deterministic stand-in for a wire closing a whole
    /// assistant message.
    pub fn emit_segment(&self, text: impl Into<String>) {
        self.with_live_progress(|progress| progress.push_segment(text.into()));
    }

    /// Script one tool line (a tool use the sender can watch) into the oldest
    /// unfinished turn's live progress.
    pub fn emit_tool_line(&self, line: impl Into<String>) {
        self.with_live_progress(|progress| progress.push_tool_line(line.into()));
    }

    /// Arm the stall: the next `prompt` call blocks inside the turn source —
    /// before any turn exists — until [`FixtureTurnSource::release_prompt`].
    pub fn stall_next_prompt(&self) {
        if let Ok(mut gate) = self.stall.lock() {
            gate.armed = true;
        }
    }

    /// Release a stalled `prompt` call.
    pub fn release_prompt(&self) {
        if let Ok(mut gate) = self.stall.lock() {
            gate.released = true;
        }
        self.stall_signal.notify_all();
    }

    /// Whether a `prompt` call is currently stalled inside the turn source.
    pub fn prompt_stalled(&self) -> bool {
        self.stall.lock().map(|gate| gate.stalled).unwrap_or(false)
    }

    fn with_live_progress(&self, act: impl FnOnce(&TurnProgress)) {
        let progress = {
            let turns = self.turns.lock().expect("fixture turns");
            turns
                .iter()
                .find(|turn| !turn.is_finished())
                .and_then(|turn| turn.progress())
        };
        if let Some(progress) = progress {
            act(&progress);
        }
    }

    pub fn resets(&self) -> usize {
        self.resets.load(Ordering::SeqCst)
    }
}

impl ConversationTurnSource for FixtureTurnSource {
    fn harness(&self) -> Option<String> {
        self.name.clone()
    }

    fn prompt(&self, request: ConversationTurnRequest) -> Result<Arc<dyn ConversationTurn>> {
        // The armed stall blocks inside the turn source — the deterministic
        // stand-in for a harness process spawning before any turn exists.
        {
            let mut gate = self.stall.lock().expect("fixture stall");
            if gate.armed {
                gate.armed = false;
                gate.stalled = true;
                while !gate.released {
                    gate = self.stall_signal.wait(gate).expect("fixture stall");
                }
                gate.stalled = false;
                gate.released = false;
            }
        }
        let slot = Arc::new(TurnSlot::with_progress(Arc::new(TurnProgress::new())));
        self.contexts
            .lock()
            .expect("fixture contexts")
            .push(request.agent_session_ref.to_string());
        let scripted = self
            .script
            .lock()
            .expect("fixture script")
            .pop_front()
            .unwrap_or(FixtureScript::Park);
        match scripted {
            FixtureScript::Reply(text) => slot.complete(ConversationTurnOutcome::Replied { text }),
            FixtureScript::Fail(reason) => {
                slot.complete(ConversationTurnOutcome::Failed { reason })
            }
            FixtureScript::Park => {}
        }
        self.turns
            .lock()
            .expect("fixture turns")
            .push(Arc::clone(&slot));
        Ok(slot)
    }

    fn reset(&self, _agent_session: &ResourceRef) -> Result<()> {
        self.resets.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn sessions(&self) -> Result<Vec<Value>> {
        Ok(self
            .contexts
            .lock()
            .expect("fixture contexts")
            .iter()
            .map(|context| json!({"agent_session_ref": context, "state": "resident"}))
            .collect())
    }

    fn model_controls(&self, _agent_session: &ResourceRef) -> Result<Value> {
        Ok(json!({
            "model_selection": true,
            "reasoning_effort_selection": false,
            "reason": Value::Null,
            "available": FIXTURE_MODELS,
            "current": self.current_model.lock().expect("fixture model").clone(),
        }))
    }

    fn set_model(&self, _agent_session: &ResourceRef, model: &str) -> Result<Value> {
        if !FIXTURE_MODELS.contains(&model) {
            return Err(AikitError::new(
                "gateway_conversation.model_unknown",
                format!(
                    "the harness does not advertise {model:?}; its models are {}",
                    FIXTURE_MODELS.join(", ")
                ),
            ));
        }
        let mut current = self.current_model.lock().expect("fixture model");
        let previous = current.clone();
        *current = model.to_owned();
        Ok(json!({
            "previous": previous,
            "current": model,
        }))
    }

    fn skills(&self) -> Result<Vec<Value>> {
        Ok(vec![
            json!({
                "name": "fixture-greeting",
                "summary": "Greet the conversation warmly.",
            }),
            json!({
                "name": "fixture-arithmetic",
                "summary": "Add two small numbers deterministically.",
            }),
        ])
    }
}

/// Which wire protocol a real harness speaks. The adapter is constructed from
/// this exactly as the encounter plane constructs its adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationHarnessProtocol {
    Acp,
    PiRpc,
    PrimeRpc,
}

/// The pi flags that launch it with nothing to call: no tools (built-in or
/// extension), no extensions, no skills, no MCP and no context files.
const PI_CONFINING_FLAGS: [&str; 5] = [
    "--no-tools",
    "--no-extensions",
    "--no-skills",
    "--no-mcp",
    "--no-context-files",
];
/// The pi flags that would give a confined launch something to call again.
const PI_WIDENING_FLAGS: [&str; 4] = ["--tools", "--extension", "--skill", "--no-builtin-tools"];

/// pi's flags as its own parser reads them (pi-coding-agent 1.5.0,
/// `dist/cli/args.js` `parseArgs`): each spelling, the long name it means,
/// and how it takes the argument after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PiTakes {
    /// A flag with no value.
    Nothing,
    /// The next argument, whatever it is, when there is one.
    Next,
    /// The next argument unless it is absent or starts with `-` (pi reports
    /// an error then, and takes nothing).
    NextUnlessFlag,
    /// `--print`: the next argument as a message, unless it starts with `@`,
    /// or with `-` but not `---`.
    PrintMessage,
    /// `--list-models`: the next argument unless it starts with `-` or `@`.
    PatternUnlessFlag,
    /// `--tui-mode`: the next argument unless it is absent or starts with `-`.
    TuiMode,
}

/// The pi release whose argv grammar [`PI_FLAGS`] and whose provider table
/// ([`pi_provider_credential_variables`]) AIKit copies: pi-coding-agent and
/// pi-ai 1.5.0. A pi of another major or minor version may read its argv
/// otherwise, so it is not held to no tools by this reading (DLW build
/// ruling 2026-11-09, be2f).
pub const PI_GRAMMAR_VERSION: &str = "1.5.0";

fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((major, minor))
}

/// Whether the version a pi reports (its `--version` output) is one this
/// reading of pi's argv was made for: the major and minor of
/// [`PI_GRAMMAR_VERSION`]. Why not, naming both versions, when it is not.
pub fn pi_version_fits(reported: &str) -> std::result::Result<(), String> {
    let made_for = major_minor(PI_GRAMMAR_VERSION);
    let read = reported
        .split_whitespace()
        .find_map(|word| major_minor(word).map(|version| (version, word)));
    match read {
        Some((version, _)) if Some(version) == made_for => Ok(()),
        Some((_, word)) => Err(format!(
            "this pi is version {word}, and AIKit reads pi's flags as pi {PI_GRAMMAR_VERSION} reads them, so it cannot say this pi is held to no tools"
        )),
        None => Err(format!(
            "this pi reports no version AIKit can read ({:?}), and AIKit reads pi's flags as pi {PI_GRAMMAR_VERSION} reads them",
            reported.trim()
        )),
    }
}

const PI_FLAGS: &[(&str, &str, PiTakes)] = &[
    ("--help", "--help", PiTakes::Nothing),
    ("-h", "--help", PiTakes::Nothing),
    ("--version", "--version", PiTakes::Nothing),
    ("-v", "--version", PiTakes::Nothing),
    ("--mode", "--mode", PiTakes::NextUnlessFlag),
    ("--continue", "--continue", PiTakes::Nothing),
    ("-c", "--continue", PiTakes::Nothing),
    ("--resume", "--resume", PiTakes::Nothing),
    ("-r", "--resume", PiTakes::Nothing),
    ("--provider", "--provider", PiTakes::Next),
    ("--model", "--model", PiTakes::Next),
    ("--api-key", "--api-key", PiTakes::Next),
    ("--system-prompt", "--system-prompt", PiTakes::Next),
    (
        "--append-system-prompt",
        "--append-system-prompt",
        PiTakes::Next,
    ),
    ("--name", "--name", PiTakes::Next),
    ("-n", "--name", PiTakes::Next),
    ("--no-session", "--no-session", PiTakes::Nothing),
    ("--session", "--session", PiTakes::Next),
    ("--session-id", "--session-id", PiTakes::Next),
    ("--fork", "--fork", PiTakes::Next),
    ("--session-dir", "--session-dir", PiTakes::Next),
    ("--models", "--models", PiTakes::Next),
    ("--no-tools", "--no-tools", PiTakes::Nothing),
    ("-nt", "--no-tools", PiTakes::Nothing),
    ("--no-builtin-tools", "--no-builtin-tools", PiTakes::Nothing),
    ("-nbt", "--no-builtin-tools", PiTakes::Nothing),
    ("--tools", "--tools", PiTakes::Next),
    ("-t", "--tools", PiTakes::Next),
    ("--exclude-tools", "--exclude-tools", PiTakes::Next),
    ("-xt", "--exclude-tools", PiTakes::Next),
    ("--thinking", "--thinking", PiTakes::Next),
    ("--print", "--print", PiTakes::PrintMessage),
    ("-p", "--print", PiTakes::PrintMessage),
    ("--export", "--export", PiTakes::Next),
    ("--extension", "--extension", PiTakes::Next),
    ("-e", "--extension", PiTakes::Next),
    ("--no-extensions", "--no-extensions", PiTakes::Nothing),
    ("-ne", "--no-extensions", PiTakes::Nothing),
    ("--no-mcp", "--no-mcp", PiTakes::Nothing),
    ("--skill", "--skill", PiTakes::Next),
    ("--prompt-template", "--prompt-template", PiTakes::Next),
    ("--theme", "--theme", PiTakes::Next),
    ("--use-theme", "--use-theme", PiTakes::NextUnlessFlag),
    ("--no-skills", "--no-skills", PiTakes::Nothing),
    ("-ns", "--no-skills", PiTakes::Nothing),
    (
        "--no-prompt-templates",
        "--no-prompt-templates",
        PiTakes::Nothing,
    ),
    ("-np", "--no-prompt-templates", PiTakes::Nothing),
    ("--no-themes", "--no-themes", PiTakes::Nothing),
    ("--no-context-files", "--no-context-files", PiTakes::Nothing),
    ("-nc", "--no-context-files", PiTakes::Nothing),
    ("--list-models", "--list-models", PiTakes::PatternUnlessFlag),
    ("--tui-mode", "--tui-mode", PiTakes::TuiMode),
    ("--verbose", "--verbose", PiTakes::Nothing),
    ("--approve", "--approve", PiTakes::Nothing),
    ("-a", "--approve", PiTakes::Nothing),
    ("--no-approve", "--no-approve", PiTakes::Nothing),
    ("-na", "--no-approve", PiTakes::Nothing),
    ("--offline", "--offline", PiTakes::Nothing),
];

/// What pi reads from the arguments after its program, read by its own
/// grammar: the flags it takes as flags, each value a flag took and which
/// flag took it, the files it attaches, the messages it is given, and what
/// it reports as an error.
#[derive(Debug, Default)]
struct PiReading<'a> {
    flags: Vec<&'static str>,
    values: Vec<(&'a str, &'a str)>,
    files: Vec<&'a str>,
    messages: Vec<&'a str>,
    errors: Vec<String>,
}

impl<'a> PiReading<'a> {
    fn read(args: &'a [String]) -> Self {
        let mut reading = Self::default();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            let next = args.get(index + 1).map(String::as_str);
            index += 1;
            if arg == "--" {
                for rest in &args[index..] {
                    match rest.strip_prefix('@') {
                        Some(file) => reading.files.push(file),
                        None => reading.messages.push(rest),
                    }
                }
                break;
            }
            if let Some((_, long, takes)) = PI_FLAGS.iter().find(|(spelling, _, takes)| {
                *spelling == arg && (*takes != PiTakes::Next || next.is_some())
            }) {
                let flag_like = |value: &str| value.starts_with('-');
                let took = match (takes, next) {
                    (PiTakes::Nothing, _) => None,
                    (PiTakes::Next, Some(value)) => Some(value),
                    (PiTakes::NextUnlessFlag | PiTakes::TuiMode, Some(value))
                        if !flag_like(value) =>
                    {
                        Some(value)
                    }
                    (PiTakes::NextUnlessFlag | PiTakes::TuiMode, _) => {
                        reading.errors.push(format!("{long} requires a value"));
                        None
                    }
                    (PiTakes::PrintMessage, Some(value))
                        if !value.starts_with('@')
                            && (!flag_like(value) || value.starts_with("---")) =>
                    {
                        reading.messages.push(value);
                        index += 1;
                        reading.flags.push(*long);
                        continue;
                    }
                    (PiTakes::PatternUnlessFlag, Some(value))
                        if !flag_like(value) && !value.starts_with('@') =>
                    {
                        Some(value)
                    }
                    _ => None,
                };
                if let Some(value) = took {
                    index += 1;
                    reading.values.push((*long, value));
                    if *long == "--mode" && !["text", "json", "rpc"].contains(&value) {
                        reading
                            .errors
                            .push(format!("--mode {value} is not text, json or rpc"));
                    }
                } else {
                    reading.flags.push(*long);
                }
                continue;
            }
            if let Some(file) = arg.strip_prefix('@') {
                reading.files.push(file);
            } else if let Some(name) = arg.strip_prefix("--") {
                // An unknown long flag: `--x=y`, or `--x` taking the next
                // argument unless that starts with `-` or `@`.
                if name.contains('=') {
                    continue;
                }
                if let Some(value) = next.filter(|value| !value.starts_with(['-', '@'])) {
                    index += 1;
                    reading.values.push((arg, value));
                }
            } else if arg.starts_with('-') {
                reading
                    .errors
                    .push(format!("{arg} is not an option pi knows"));
            } else {
                reading.messages.push(arg);
            }
        }
        reading
    }

    fn has(&self, flag: &str) -> bool {
        self.flags.contains(&flag) || self.values.iter().any(|(took, _)| *took == flag)
    }

    /// The last value `flag` took, as pi keeps it.
    fn value(&self, flag: &str) -> Option<&'a str> {
        self.values
            .iter()
            .rev()
            .find(|(took, _)| *took == flag)
            .map(|(_, value)| *value)
    }
}

/// The provider a pi argv selects (`--provider`, its last value as pi
/// keeps it), read by pi's own grammar from the arguments after its program.
pub fn pi_selected_provider(argv: &[String]) -> Option<String> {
    PiReading::read(argv.get(1..).unwrap_or_default())
        .value("--provider")
        .map(str::to_owned)
}

/// The variables pi reads a provider's key from (pi-ai 1.5.0,
/// `dist/env-api-keys.js` `getApiKeyEnvVars`, less `ANTHROPIC_AUTH_TOKEN`,
/// which pi never reads as a key). AIKit's own pi profile names none, since
/// pi keeps its keys in its own store; an answer turn's empty HOME has no
/// such store, so the person names the variable, and it must be one of these
/// for the provider the argv selects.
pub fn pi_provider_credential_variables(provider: &str) -> &'static [&'static str] {
    match provider {
        "github-copilot" => &["COPILOT_GITHUB_TOKEN"],
        "anthropic" => &["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        "ant-ling" => &["ANT_LING_API_KEY"],
        "qwen-token-plan" | "qwen-token-plan-individual" => &["QWEN_TOKEN_PLAN_API_KEY"],
        "qwen-token-plan-cn" => &["QWEN_TOKEN_PLAN_CN_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        "azure" => &["AZURE_OPENAI_API_KEY"],
        "nvidia" => &["NVIDIA_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY"],
        "google" => &["GEMINI_API_KEY"],
        "google-vertex" => &["GOOGLE_CLOUD_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "cerebras" => &["CEREBRAS_API_KEY"],
        "xai" => &["XAI_API_KEY"],
        "typesafe" => &["TYPESAFE_API_KEY"],
        "radius" => &["RADIUS_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        "vercel-ai-gateway" => &["AI_GATEWAY_API_KEY"],
        "zai" => &["ZAI_API_KEY"],
        "zai-coding-cn" => &["ZAI_CODING_CN_API_KEY"],
        "mistral" => &["MISTRAL_API_KEY"],
        "minimax" => &["MINIMAX_API_KEY"],
        "minimax-cn" => &["MINIMAX_CN_API_KEY"],
        "moonshotai" | "moonshotai-cn" => &["MOONSHOT_API_KEY"],
        "huggingface" => &["HF_TOKEN"],
        "fireworks" => &["FIREWORKS_API_KEY"],
        "together" => &["TOGETHER_API_KEY"],
        "baseten" => &["BASETEN_API_KEY"],
        "opencode" | "opencode-go" => &["OPENCODE_API_KEY"],
        "kimi-coding" => &["KIMI_API_KEY"],
        "meta" => &["META_API_KEY"],
        "cloudflare-workers-ai" | "cloudflare-ai-gateway" => &["CLOUDFLARE_API_KEY"],
        "xiaomi" => &["XIAOMI_API_KEY"],
        "xiaomi-token-plan-cn" => &["XIAOMI_TOKEN_PLAN_CN_API_KEY"],
        "xiaomi-token-plan-ams" => &["XIAOMI_TOKEN_PLAN_AMS_API_KEY"],
        "xiaomi-token-plan-sgp" => &["XIAOMI_TOKEN_PLAN_SGP_API_KEY"],
        _ => &[],
    }
}

/// Why a pi argv cannot be held to no tools, read by pi's own grammar (DLW
/// build ruling 2026-11-09, be2d): its program is pi; pi reports no error in
/// it; every confining flag is read as a flag, never as another flag's value;
/// none that widens it again; no key on the argv; and no file or message
/// before the prompt, which is a turn's only input.
fn pi_confinement(argv: &[String]) -> std::result::Result<(), String> {
    let Some(program) = argv.first() else {
        return Err("the argv names no program".into());
    };
    if std::path::Path::new(program).file_name() != Some(std::ffi::OsStr::new("pi")) {
        return Err(format!(
            "the program is {program}, and only pi (a program named pi, its flags read as pi {PI_GRAMMAR_VERSION} reads them) is held to no tools by its flags"
        ));
    }
    let reading = PiReading::read(&argv[1..]);
    if !reading.errors.is_empty() {
        return Err(format!(
            "pi would refuse this argv: {}",
            reading.errors.join("; ")
        ));
    }
    for flag in PI_CONFINING_FLAGS {
        if reading.flags.contains(&flag) {
            continue;
        }
        let taken_by = reading.values.iter().find(|(_, value)| {
            PI_FLAGS
                .iter()
                .any(|(spelling, long, _)| spelling == value && *long == flag)
        });
        return Err(match taken_by {
            Some((by, value)) => format!(
                "pi reads {value} as the value of {by}, not as {flag}, so this argv lacks {flag}"
            ),
            None => format!(
                "pi is held to no tools only by its own launch flags, and this argv lacks {}",
                PI_CONFINING_FLAGS
                    .iter()
                    .filter(|flag| !reading.flags.contains(flag))
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if let Some(flag) = PI_WIDENING_FLAGS.iter().find(|flag| reading.has(flag)) {
        return Err(format!(
            "this pi argv names {flag}, which gives the turn something to call"
        ));
    }
    if reading.has("--api-key") {
        return Err(
            "this pi argv carries a key (--api-key); name the provider's variable with --credential-env instead"
                .into(),
        );
    }
    if let Some(file) = reading.files.first() {
        return Err(format!(
            "pi would attach the file {file} before the prompt, which is an answer turn's only input"
        ));
    }
    if let Some(message) = reading.messages.first() {
        return Err(format!(
            "pi would take {message} as a message before the prompt, which is an answer turn's only input"
        ));
    }
    Ok(())
}

impl ConversationHarnessProtocol {
    /// How AIKit holds this protocol's harness to no tools before any prompt
    /// (DLW build ruling 2026-11-09), or why it cannot for this argv:
    ///
    /// - pi, by its own launch flags, read by pi's own grammar
    ///   ([`pi_confinement`]);
    /// - ACP, by the session it opens reporting an empty tool list: the
    ///   agent's own declaration;
    /// - Prime RPC, by the session it attaches to reporting an empty tool
    ///   registry.
    pub fn confines(self, argv: &[String]) -> std::result::Result<&'static str, String> {
        match self {
            Self::PiRpc => pi_confinement(argv).map(|()| {
                "pi launched with its tools, extensions, skills, MCP and context files disabled"
            }),
            Self::Acp => Ok("an ACP agent whose session reports an empty tool list when it opens"),
            Self::PrimeRpc => {
                Ok("a Prime session that reports an empty tool registry when it is attached")
            }
        }
    }

    /// Whether this protocol's session reports its own tools (and must, for
    /// a turn that takes none).
    fn session_reports_tools(self) -> bool {
        matches!(self, Self::Acp | Self::PrimeRpc)
    }
}

/// Whether a signal is a tool at work: its call, its result, a provider
/// update about one, or a Prime child run.
fn is_tool_event(kind: &ConnectionSignalKind) -> bool {
    match kind {
        ConnectionSignalKind::ToolCall { .. } | ConnectionSignalKind::ToolResult { .. } => true,
        ConnectionSignalKind::ProviderUpdate { session_update, .. } => {
            session_update.contains("tool")
        }
        ConnectionSignalKind::Status { message } => message.starts_with("prime-rlm-child:"),
        _ => false,
    }
}

/// How long a tool-less turn's harness may take to start (launch, handshake
/// and session open) unless the source is given its own bound.
pub const TOOL_LESS_STARTUP_BOUND: Duration = Duration::from_secs(60);

/// A turn source backed by a real [`AgentSessionHost`] driving a real harness
/// process. The host launches lazily on the first prompt; one host carries the
/// binding's agent sessions. A canonical New with a regenerated
/// `agent_session` ref opens a fresh native session on the same host; the
/// previous native session remains resident until the host stops, and the
/// engine's reset result names that.
pub struct AgentHostTurnSource {
    harness_name: String,
    protocol: ConversationHarnessProtocol,
    argv: Vec<String>,
    cwd: PathBuf,
    host: Arc<OnceLock<Mutex<AgentSessionHost>>>,
    /// The Agent Skills roots the backed harness carries its skills from.
    /// `None` means the standard roots: the home `~/.agents/skills` and the
    /// working directory's `.agents/skills`.
    skill_roots: Option<Vec<PathBuf>>,
    /// Whether this source's turns take no tool (`without_tools`).
    tool_less: bool,
    /// The launch environment; `None` inherits this process's.
    environment: Option<crate::connection_process::ModelEnvironment>,
    /// The bound on starting the harness (launch, handshake, session open).
    startup_bound: Option<Duration>,
}

/// Why a tool-less turn was not prompted: its harness is not held to none.
pub const HARNESS_NOT_CONFINED: &str = "gateway_conversation.harness_not_confined";

/// How a tool-less turn ends when its harness calls a tool anyway.
pub const TOOL_LESS_TURN_USED_A_TOOL: &str =
    "the turn called a tool, and this turn takes none: it was stopped, and nothing it said is its answer";

impl AgentHostTurnSource {
    pub fn new(
        harness_name: impl Into<String>,
        protocol: ConversationHarnessProtocol,
        argv: Vec<String>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            harness_name: harness_name.into(),
            protocol,
            argv,
            cwd,
            host: Arc::new(OnceLock::new()),
            skill_roots: None,
            tool_less: false,
            environment: None,
            startup_bound: None,
        }
    }

    /// Turns that take no tool (ai-kit #601: a World's agent answering an ask
    /// only from what its World shows): no Agent Skills root is carried, and a
    /// turn whose harness calls a tool all the same is interrupted at that call
    /// and ends `Failed` with [`TOOL_LESS_TURN_USED_A_TOOL`], so nothing a tool
    /// reached becomes its reply. Launch the harness in its own no-tools mode
    /// as well; this is the bound held here, at the seam, whatever it was given.
    ///
    /// Confined before the prompt, not interrupted after it (DLW build ruling
    /// 2026-11-09): the harness is launched with PATH and LANG only, an empty
    /// HOME of its own (beside the working folder, `<cwd>.home`) and no MCP
    /// servers, within [`TOOL_LESS_STARTUP_BOUND`]; a protocol whose argv
    /// cannot be confined is refused before launch, and a session that reports
    /// tools (or, where its protocol reports them, reports none) is refused
    /// before the prompt. The guard stays as the backstop, on every tool
    /// event: a call, a result, a provider update about one, a Prime child run.
    pub fn without_tools(mut self) -> Self {
        self.skill_roots = Some(Vec::new());
        self.tool_less = true;
        // A host launched before is not confined: this source launches its own.
        self.host = Arc::new(OnceLock::new());
        let home = self.cwd.with_file_name(format!(
            "{}.home",
            self.cwd
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "turn".into())
        ));
        self.environment = Some(crate::connection_process::ModelEnvironment::isolated(home));
        self.startup_bound = Some(TOOL_LESS_STARTUP_BOUND);
        self
    }

    /// Launch under this environment instead (a confined one carrying the
    /// provider's own credential variable, say).
    pub fn with_launch_environment(
        mut self,
        environment: crate::connection_process::ModelEnvironment,
    ) -> Self {
        self.environment = Some(environment);
        self.host = Arc::new(OnceLock::new());
        self
    }

    /// Bound the harness's start (launch, handshake, session open).
    pub fn with_startup_bound(mut self, bound: Duration) -> Self {
        self.startup_bound = Some(bound);
        self
    }

    /// Refuse a tool-less turn whose harness is not held to no tools.
    fn check_confined_argv(&self) -> Result<()> {
        if !self.tool_less {
            return Ok(());
        }
        self.protocol
            .confines(&self.argv)
            .map(|_| ())
            .map_err(|why| {
                AikitError::new(
                    HARNESS_NOT_CONFINED,
                    format!(
                        "the {} harness cannot be held to no tools: {why}",
                        self.harness_name
                    ),
                )
            })
    }

    /// After the session opened and before the prompt: a tool-less turn's
    /// session must declare no tools.
    fn check_confined_session(&self, lane: &crate::agent_session_host::SessionLane) -> Result<()> {
        if !self.tool_less {
            return Ok(());
        }
        let refuse = |why: String| {
            Err(AikitError::new(
                HARNESS_NOT_CONFINED,
                format!("the {} harness was not prompted: {why}", self.harness_name),
            ))
        };
        match &lane.binding().declared_tools {
            Some(tools) if !tools.is_empty() => refuse(format!(
                "its session reports the tools {}, and this turn takes none",
                tools.join(", ")
            )),
            None if self.protocol.session_reports_tools() => {
                refuse("its session reports no tool list, so AIKit cannot hold it to none".into())
            }
            _ => Ok(()),
        }
    }

    /// Name the Agent Skills roots explicitly (deterministic proof); the
    /// default keeps the standard roots.
    pub fn with_skill_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.skill_roots = Some(roots);
        self
    }

    fn resolved_skill_roots(&self) -> Vec<PathBuf> {
        if let Some(roots) = &self.skill_roots {
            return roots.clone();
        }
        let mut roots = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".agents").join("skills"));
        }
        roots.push(self.cwd.join(".agents").join("skills"));
        roots
    }

    fn host(&self) -> Result<&Mutex<AgentSessionHost>> {
        self.host_before(None)
    }

    fn host_before(&self, deadline: Option<Instant>) -> Result<&Mutex<AgentSessionHost>> {
        if let Some(host) = self.host.get() {
            return Ok(host);
        }
        self.check_confined_argv()?;
        let isolated_home = self
            .environment
            .as_ref()
            .and_then(crate::connection_process::ModelEnvironment::isolated_home);
        if self.tool_less && isolated_home.is_none() {
            return Err(AikitError::new(
                HARNESS_NOT_CONFINED,
                format!(
                    "the {} harness takes no tool, so it is launched only in an isolated environment of its own",
                    self.harness_name
                ),
            ));
        }
        if let Some(home) = isolated_home {
            std::fs::create_dir_all(home).map_err(|error| {
                AikitError::new(
                    "gateway_conversation.isolated_home",
                    format!("the harness's own empty HOME could not be made: {error}"),
                )
            })?;
        }
        let environment = self.environment.as_ref();
        let cwd = Some(self.cwd.as_path());
        let limits = AgentSessionHostLimits::default;
        let connection_ref = ResourceRef::parse("agent-connection/gateway-conversation")?;
        let provenance = vec!["gateway conversation engine".into()];
        let host = match self.protocol {
            ConversationHarnessProtocol::Acp => {
                AgentSessionHost::launch_with_journal_and_environment(
                    crate::interactive_connection::AcpStableConnectionAdapter::new(
                        connection_ref,
                        provenance,
                    ),
                    &self.argv,
                    cwd,
                    limits(),
                    None,
                    environment,
                )?
            }
            ConversationHarnessProtocol::PiRpc => {
                AgentSessionHost::launch_with_journal_and_environment(
                    crate::pi_rpc_connection::PiRpcConnectionAdapter::new(
                        connection_ref,
                        self.cwd.display().to_string(),
                        provenance,
                    ),
                    &self.argv,
                    cwd,
                    limits(),
                    None,
                    environment,
                )?
            }
            ConversationHarnessProtocol::PrimeRpc => {
                AgentSessionHost::launch_with_journal_and_environment(
                    crate::prime_rpc_connection::PrimeRpcConnectionAdapter::new(
                        connection_ref,
                        self.cwd.display().to_string(),
                        provenance,
                    ),
                    &self.argv,
                    cwd,
                    limits(),
                    None,
                    environment,
                )?
            }
        };
        match deadline {
            Some(deadline) => host.initialize_before(deadline)?,
            None => host.initialize()?,
        };
        let _ = self.host.set(Mutex::new(host));
        self.host.get().ok_or_else(poisoned)
    }

    fn open_lane(&self, host: &Mutex<AgentSessionHost>, agent_session: &ResourceRef) -> Result<()> {
        self.open_lane_before(host, agent_session, None)
    }

    fn open_lane_before(
        &self,
        host: &Mutex<AgentSessionHost>,
        agent_session: &ResourceRef,
        deadline: Option<Instant>,
    ) -> Result<()> {
        let locked = host.lock().map_err(|_| poisoned())?;
        if locked.lane(agent_session).is_ok() {
            return Ok(());
        }
        // The pi/prime RPC adapters attach to the session their own spawned
        // process observes — they claim no create/load/resume. The turn
        // source keeps one resident host per binding, so that observed
        // process IS the conversation; continuity across process loss is
        // pi's own session store, not a gateway claim.
        let mode = match self.protocol {
            ConversationHarnessProtocol::Acp => SessionOpenMode::Create,
            ConversationHarnessProtocol::PiRpc | ConversationHarnessProtocol::PrimeRpc => {
                SessionOpenMode::Attach
            }
        };
        let request = SessionOpenRequest {
            mode,
            native_session_id: None,
            cwd: self.cwd.display().to_string(),
            additional_directories: Vec::new(),
            mcp_servers: Vec::new(),
            agent_session: Some(agent_session.clone()),
        };
        match deadline {
            Some(deadline) => locked.open_session_before(request, deadline)?,
            None => locked.open_session(request)?,
        };
        Ok(())
    }

    fn prompt_payload(&self, prompt: &str) -> Value {
        match self.protocol {
            ConversationHarnessProtocol::Acp => json!([{"type": "text", "text": prompt}]),
            ConversationHarnessProtocol::PiRpc | ConversationHarnessProtocol::PrimeRpc => {
                json!(prompt)
            }
        }
    }
}

/// A host-backed live turn: the engine's waitable slot plus the real turn
/// handle. An interrupt is the host's own interruption — the cancel commands
/// go to the harness, the receipt is what the host actually issued, and the
/// slot's outcome arrives from the turn's real end (completed, cancelled,
/// failed), never invented locally.
struct HostTurn {
    slot: Arc<TurnSlot>,
    handle: TurnHandle,
}

impl ConversationTurn for HostTurn {
    fn wait(&self) -> ConversationTurnOutcome {
        self.slot.wait()
    }

    fn wait_timeout(&self, timeout: Duration) -> Option<ConversationTurnOutcome> {
        self.slot.wait_timeout(timeout)
    }

    fn interrupt(&self, reason: Option<String>) -> Result<String> {
        match self.handle.interrupt(reason) {
            Ok(receipt) => Ok(describe_interrupt_receipt(&receipt)),
            // The turn ended before the cancel landed: its real outcome is
            // already arriving on the slot, so there is honestly nothing to
            // cancel. A second interrupt is likewise answered, not failed:
            // the surface asked to stop, and a stop is under way.
            Err(error) if error.code() == "agent_session_host.no_turn_in_flight" => {
                Ok("the turn had already ended; no cancel was needed".into())
            }
            Err(error) if error.code() == "agent_session_host.interrupt_already_requested" => {
                Ok("an interrupt is already in flight for this turn".into())
            }
            Err(error) => Err(error),
        }
    }

    fn finished(&self) -> bool {
        self.slot.finished()
    }

    fn progress(&self) -> Option<Arc<TurnProgress>> {
        self.slot.progress()
    }
}

/// The plain-words receipt for a host interruption: what was issued, and why.
fn describe_interrupt_receipt(receipt: &InterruptReceipt) -> String {
    let what = if receipt.commands.is_empty() {
        "interrupt issued".to_string()
    } else {
        format!("interrupt issued ({})", receipt.commands.join(", "))
    };
    match &receipt.reason {
        Some(reason) => format!("{what}: {reason}"),
        None => what,
    }
}

impl ConversationTurnSource for AgentHostTurnSource {
    fn harness(&self) -> Option<String> {
        Some(self.harness_name.clone())
    }

    fn prompt(&self, request: ConversationTurnRequest) -> Result<Arc<dyn ConversationTurn>> {
        // One bound for the whole start: launch, handshake and session open.
        let deadline = self.startup_bound.map(|bound| Instant::now() + bound);
        let host = self.host_before(deadline)?;
        self.open_lane_before(host, &request.agent_session_ref, deadline)?;
        let locked = host.lock().map_err(|_| poisoned())?;
        let lane = locked.lane(&request.agent_session_ref)?;
        self.check_confined_session(&lane)?;
        let handle = lane.prompt(self.prompt_payload(&request.prompt))?;
        drop(locked);
        let slot = Arc::new(TurnSlot::with_progress(Arc::new(TurnProgress::new())));
        let reader_slot = Arc::clone(&slot);
        let progress = slot.progress();
        let reader_handle = handle.clone();
        let tool_less = self.tool_less;
        thread::spawn(move || {
            let mut text = String::new();
            let mut used_a_tool = false;
            loop {
                let outcome = match reader_handle.recv() {
                    Some(HostEvent::Signal(signal)) if tool_less && is_tool_event(&signal.kind) => {
                        if !used_a_tool {
                            used_a_tool = true;
                            let _ = reader_handle.interrupt(Some("this turn takes no tool".into()));
                        }
                        continue;
                    }
                    Some(HostEvent::Signal(signal)) => {
                        match signal.kind {
                            // Fine-grained deltas accumulate the turn's final
                            // text; they are not themselves deliverable
                            // segments — the wire closes segments whole.
                            ConnectionSignalKind::AgentMessageChunk { text: chunk } => {
                                text.push_str(&chunk);
                            }
                            ConnectionSignalKind::AgentMessageSegment { text: segment } => {
                                if let Some(progress) = &progress {
                                    progress.push_segment(segment);
                                }
                            }
                            ConnectionSignalKind::ToolCall { payload } => {
                                if let (Some(progress), Some(line)) =
                                    (&progress, tool_use_line(&payload))
                                {
                                    progress.push_tool_line(line);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    Some(HostEvent::TurnEnded(_)) if used_a_tool => {
                        ConversationTurnOutcome::Failed {
                            reason: TOOL_LESS_TURN_USED_A_TOOL.into(),
                        }
                    }
                    Some(HostEvent::TurnEnded(record)) => match record.stop {
                        TurnStop::Completed { .. } => ConversationTurnOutcome::Replied { text },
                        TurnStop::Cancelled => {
                            ConversationTurnOutcome::Interrupted { detail: None }
                        }
                        TurnStop::Failed { reason } => ConversationTurnOutcome::Failed { reason },
                        TurnStop::OperationalLimit { max_signals } => {
                            ConversationTurnOutcome::Failed {
                                reason: format!("turn hit the {max_signals}-signal limit"),
                            }
                        }
                    },
                    None => ConversationTurnOutcome::Failed {
                        reason: "the harness host closed before the turn ended".into(),
                    },
                };
                reader_slot.complete(outcome);
                return;
            }
        });
        Ok(Arc::new(HostTurn { slot, handle }))
    }

    fn reset(&self, agent_session: &ResourceRef) -> Result<()> {
        let host = self.host()?;
        self.open_lane(host, agent_session)
    }

    fn sessions(&self) -> Result<Vec<Value>> {
        let host = self.host()?;
        let locked = host.lock().map_err(|_| poisoned())?;
        Ok(locked
            .sessions()?
            .into_iter()
            .map(|session| {
                json!({
                    "agent_session_ref": session.agent_session.to_string(),
                    "native_session_id": session.binding.native_session_id,
                    "state": lane_state_name(session.state),
                })
            })
            .collect())
    }

    fn model_controls(&self, agent_session: &ResourceRef) -> Result<Value> {
        let host = self.host()?;
        let locked = host.lock().map_err(|_| poisoned())?;
        let lane = locked.lane(agent_session)?;
        let controls = lane.model_controls()?;
        serde_json::to_value(&controls).map_err(|error| {
            AikitError::new(
                "gateway_conversation.model_controls_encode",
                format!("encode the harness's model controls: {error}"),
            )
        })
    }

    fn set_model(&self, agent_session: &ResourceRef, model: &str) -> Result<Value> {
        let host = self.host()?;
        let locked = host.lock().map_err(|_| poisoned())?;
        let lane = locked.lane(agent_session)?;
        let receipt = lane.set_model(model)?;
        serde_json::to_value(&receipt).map_err(|error| {
            AikitError::new(
                "gateway_conversation.model_receipt_encode",
                format!("encode the model configuration receipt: {error}"),
            )
        })
    }

    fn skills(&self) -> Result<Vec<Value>> {
        let mut disclosed = Vec::new();
        for root in self.resolved_skill_roots() {
            let entries = match std::fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(AikitError::new(
                        "gateway_conversation.skills_root_unreadable",
                        format!("read {}: {error}", root.display()),
                    ));
                }
            };
            let mut dirs: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            dirs.sort();
            for dir in dirs {
                if !dir.join(crate::clients::agent_skills::SKILL_FILE).is_file() {
                    continue;
                }
                match crate::clients::agent_skills::validate(&dir) {
                    Ok(skill) => disclosed.push(json!({
                        "name": skill.name,
                        "summary": crate::clients::agent_skills::first_sentence(&skill.description),
                        "root": dir.display().to_string(),
                    })),
                    // One broken skill never takes the disclosure down; it is
                    // named on stderr instead of silently skipped.
                    Err(error) => {
                        eprintln!(
                            "gateway conversation engine: skill {}: {error}",
                            dir.display()
                        );
                    }
                }
            }
        }
        Ok(disclosed)
    }
}

/// A plain name for a lane state, for status disclosure.
fn lane_state_name(state: crate::agent_session_host::SessionLaneState) -> &'static str {
    match state {
        crate::agent_session_host::SessionLaneState::Resident => "resident",
        crate::agent_session_host::SessionLaneState::TurnInFlight => "turn-in-flight",
        crate::agent_session_host::SessionLaneState::InterruptRequested => "interrupt-requested",
    }
}

/// One short, plain line for a tool use, in the shapes the connection
/// adapters actually produce: a pi-rpc execution start names its `toolName`
/// (and may carry the path or command it is working on), an ACP tool-call
/// update names its `title`. Only a use's start announces it — progress on an
/// already-announced tool is the same tool still running. Nothing usable
/// means no line — never a fabricated one.
fn tool_use_line(payload: &Value) -> Option<String> {
    const NAME_KEYS: [&str; 5] = ["toolName", "tool_name", "title", "name", "tool"];
    const TARGET_KEYS: [&str; 6] = ["path", "file_path", "file", "command", "pattern", "url"];
    const MAX_LINE_CHARS: usize = 100;
    if let Some(kind) = payload.get("type").and_then(Value::as_str) {
        if kind != "tool_execution_start" {
            return None;
        }
    }
    let mut name = None;
    for key in NAME_KEYS {
        if let Some(candidate) = payload.get(key).and_then(Value::as_str) {
            let candidate = candidate.trim();
            if !candidate.is_empty() {
                name = Some(candidate);
                break;
            }
        }
    }
    let name = name?;
    let mut line = format!("· {name}");
    for key in TARGET_KEYS {
        if let Some(target) = payload
            .get("args")
            .or_else(|| payload.get("input"))
            .and_then(|args| args.get(key))
            .and_then(Value::as_str)
        {
            let room = MAX_LINE_CHARS.saturating_sub(line.chars().count() + 2);
            let mut short: String = target
                .trim()
                .chars()
                .map(|c| if c.is_whitespace() { ' ' } else { c })
                .take(room)
                .collect();
            if !short.is_empty() {
                short.push('…');
                line.push(' ');
                line.push_str(&short);
            }
            break;
        }
    }
    let truncated: String = line.chars().take(MAX_LINE_CHARS).collect();
    Some(truncated)
}

fn poisoned() -> AikitError {
    AikitError::new(
        "gateway_conversation.poisoned",
        "an engine lock was poisoned by a panicked worker",
    )
}

/// What the engine edge reads out of a surface text. Slash strings are surface
/// sugar: they parse here and nowhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashParse {
    Operation(GatewayConversationOperation),
    Unknown(String),
    NotACommand,
}

/// Parse connector ingress text into a canonical operation at the engine
/// edge. `/new` and `/reset` are the same operation; `/pause`/`/resume` with
/// no argument target the speaking conversation's own connector; `/ask
/// <position-or-@handle> <message>` carries the whole rest of the line as the
/// asked message.
pub fn parse_slash(text: &str) -> SlashParse {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return SlashParse::NotACommand;
    };
    let rest = rest.trim_start();
    let (head, remainder) = match rest.split_once(char::is_whitespace) {
        Some((head, after)) => (head, after.trim_start()),
        None => (rest, ""),
    };
    let mut parts = remainder.split_whitespace();
    let argument = parts.next();
    let connector_argument = || -> Result<Option<ResourceRef>> {
        argument
            .map(ResourceRef::parse)
            .transpose()
            .map_err(|error| {
                AikitError::new(
                    "gateway_conversation.connector_argument",
                    format!("parse connector ref {argument:?}: {error}"),
                )
            })
    };
    match head.to_ascii_lowercase().as_str() {
        "status" => SlashParse::Operation(GatewayConversationOperation::Status),
        "stop" | "interrupt" => SlashParse::Operation(GatewayConversationOperation::Stop),
        "new" | "reset" => SlashParse::Operation(GatewayConversationOperation::New),
        "sessions" => SlashParse::Operation(GatewayConversationOperation::Sessions),
        "restart" => SlashParse::Operation(GatewayConversationOperation::Restart),
        // `/upgrade` reads the plan; changing anything takes the explicit
        // word `apply`.
        "upgrade" => match argument.map(str::to_ascii_lowercase).as_deref() {
            None | Some("plan") => {
                SlashParse::Operation(GatewayConversationOperation::Upgrade { apply: false })
            }
            Some("apply") => {
                SlashParse::Operation(GatewayConversationOperation::Upgrade { apply: true })
            }
            Some(other) => SlashParse::Unknown(format!(
                "/upgrade {other} (use /upgrade to read the plan, /upgrade apply to start it)"
            )),
        },
        "ask" => {
            let mut words = remainder.split_whitespace();
            let Some(recipient) = words.next() else {
                return SlashParse::Unknown(
                    "usage: /ask <position-ref-or-@handle> <message>".into(),
                );
            };
            // The recipient is the first word; the whole rest of the line is
            // the asked message, whitespace and all.
            let message = remainder[recipient.len()..].trim();
            if message.is_empty() {
                return SlashParse::Unknown(format!(
                    "/ask {recipient} needs a message: /ask <position-ref-or-@handle> <message>"
                ));
            }
            SlashParse::Operation(GatewayConversationOperation::AskPosition {
                position: recipient.to_owned(),
                message: message.to_owned(),
            })
        }
        "pause" => match connector_argument() {
            Ok(connector_ref) => {
                SlashParse::Operation(GatewayConversationOperation::PauseConnector {
                    connector_ref,
                })
            }
            Err(error) => SlashParse::Unknown(format!("/pause ({error})")),
        },
        "resume" => match connector_argument() {
            Ok(connector_ref) => {
                SlashParse::Operation(GatewayConversationOperation::ResumeConnector {
                    connector_ref,
                })
            }
            Err(error) => SlashParse::Unknown(format!("/resume ({error})")),
        },
        "model" => SlashParse::Operation(GatewayConversationOperation::Model {
            model: argument.map(str::to_owned),
        }),
        "harness" => SlashParse::Operation(GatewayConversationOperation::Harness),
        "skills" => SlashParse::Operation(GatewayConversationOperation::Skills),
        other => SlashParse::Unknown(format!("/{other}")),
    }
}

/// One in-flight turn with the inbound facts its reply must carry.
struct InFlightTurn {
    turn: Arc<dyn ConversationTurn>,
    in_reply_to_sequence: u64,
    native_message_id: Option<String>,
}

/// One admitted message waiting for its binding's turn plane to free.
struct QueuedPrompt {
    prompt: String,
    in_reply_to_sequence: u64,
    native_message_id: Option<String>,
}

/// How many admitted messages may wait on one binding's turn plane. Bounded
/// on purpose: a chat storm meets an honest refusal, not unbounded memory.
const TURN_QUEUE_LIMIT: usize = 16;

/// What a running turn already delivered as its own messages: the completed
/// segments, in order. When the turn's reply equals what was delivered, the
/// completion sends nothing more — the reply already arrived piece by piece.
#[derive(Default)]
struct StreamedReplies {
    segments: Vec<String>,
}

impl StreamedReplies {
    fn joined(&self) -> String {
        self.segments.concat()
    }
}

struct EngineInner {
    /// Live turns per binding ref, interruptible by canonical Stop.
    in_flight: BTreeMap<ResourceRef, InFlightTurn>,
    /// Bindings whose turn worker is between admission and its in-flight
    /// registration — the harness may still be starting. The turn plane is
    /// busy for these too, so a mid-turn message queues instead of racing
    /// the registration.
    starting: BTreeSet<ResourceRef>,
    /// Admitted messages waiting for their binding's turn plane, in arrival
    /// order. Each becomes the next turn when the plane frees.
    queued: BTreeMap<ResourceRef, VecDeque<QueuedPrompt>>,
    /// Set when a restart was requested: no new work is admitted.
    draining: bool,
}

/// One conversation turn a drain met.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainedTurn {
    pub binding_ref: String,
    pub in_reply_to_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What a drain found and did, exact rather than counted: work that could not
/// finish is named so nobody has to guess what a restart cost. Owned by the
/// conversation engine since the gateway retired its upgrade surface.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DrainReport {
    /// Whether a drain actually ran and counted. `false` means the counts
    /// below are UNKNOWN, not zero.
    #[serde(default)]
    pub measured: bool,
    pub reason: String,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    /// The bounded grace each in-flight turn was given before being
    /// interrupted.
    pub grace_ms: u64,
    /// Turns that finished inside the grace.
    pub turns_resolved: Vec<DrainedTurn>,
    /// Turns interrupted because they did not finish. Whatever the model or
    /// its tools did before the interrupt is an UNCERTAIN effect: it is
    /// recorded, journaled as an interruption, and never replayed.
    pub turns_interrupted: Vec<DrainedTurn>,
    /// Admitted messages that were still waiting for their turn plane when
    /// the drain ran. They never started a turn, so nothing ran for them;
    /// each is on its stream's journal as the inbound message it was, and is
    /// not replayed.
    #[serde(default)]
    pub prompts_not_started: Vec<DrainedTurn>,
    /// Communiques waiting at this gateway, by Position. They are in the
    /// journal and survive the restart.
    pub communiques: Vec<CommuniqueCount>,
    /// A service stop that met the tick still running past its grace: the
    /// command it ended, and whether the tick then returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_at_stop: Option<crate::gateway_service::TickAtStop>,
    /// The gateway that drained, when its stop persisted this report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_pid: Option<u32>,
    /// Each child the runner still held when the stop had ended what it ends:
    /// its pid and whose it is (the tick's, a session's, the mux's). A
    /// gateway that lingers after its stop waits on one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<crate::runner::LiveChild>,
}

impl DrainReport {
    /// The report as a stop's log line: the turns it met and, when the tick
    /// was still running past its grace, the command it ended.
    pub fn stop_summary(&self) -> String {
        let turns = format!(
            "{} turn(s) resolved, {} interrupted",
            self.turns_resolved.len(),
            self.turns_interrupted.len()
        );
        match &self.tick_at_stop {
            Some(tick) => format!("{turns}; {}", tick.sentence()),
            None => turns,
        }
    }
}

/// The conversation an upgrade was asked for from, so its receipt returns
/// there. Only refs: the receipt is posted through the running gateway
/// (`Announce`), never by reaching into a connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeOrigin {
    pub binding_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to_sequence: Option<u64>,
}

/// What a conversation's `/upgrade` asks of the machine's upgrade owner. The
/// engine holds no installer and no service manager; the binary that owns
/// them wires this once at service assembly. The upgrade itself runs in a
/// worker that outlives the gateway process, because the gateway is what the
/// upgrade restarts.
pub trait GatewayUpgradeLauncher: Send + Sync + 'static {
    /// The plan, as data and one plain-words line. Changes nothing.
    fn plan(&self) -> Result<(Value, String)>;
    /// Start the managed upgrade in a detached worker; the receipt returns to
    /// `origin` once the new build is verified running.
    fn start(&self, origin: UpgradeOrigin) -> Result<(Value, String)>;
}

/// What a canonical operation answered, and whether the service should now
/// exit cleanly so its service manager rematerialises it.
pub struct ConversationExecution {
    pub response: GatewayResponse,
    pub restart_requested: bool,
}

/// The conversation engine. See the module docs for the reply path and the
/// restart law.
pub struct GatewayConversationEngine {
    gateway: Arc<Mutex<AgencyGateway>>,
    hub: Arc<SubscriptionHub>,
    queues: Arc<ConnectorQueues>,
    controls: Arc<ConnectorPumpControls>,
    state_file: Option<PathBuf>,
    resolver: Option<Arc<dyn GatewayTurnSourceResolver>>,
    policy: EnginePolicy,
    /// The inverted hook behind `/ask`, wired once at service assembly
    /// (`GatewayConversationHooks::ask_router`). Absent, `/ask` refuses
    /// honestly: no occupancy or recipient owners stand behind this gateway.
    ask: OnceLock<Arc<dyn GatewayAskRouter>>,
    /// The managed-upgrade owner behind `/upgrade`, wired once at service
    /// assembly. Absent, `/upgrade` says so and names the command to run.
    upgrade: OnceLock<Arc<dyn GatewayUpgradeLauncher>>,
    inner: Mutex<EngineInner>,
}

impl GatewayConversationEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gateway: Arc<Mutex<AgencyGateway>>,
        hub: Arc<SubscriptionHub>,
        queues: Arc<ConnectorQueues>,
        controls: Arc<ConnectorPumpControls>,
        state_file: Option<PathBuf>,
        resolver: Option<Arc<dyn GatewayTurnSourceResolver>>,
        policy: EnginePolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            gateway,
            hub,
            queues,
            controls,
            state_file,
            resolver,
            policy,
            ask: OnceLock::new(),
            upgrade: OnceLock::new(),
            inner: Mutex::new(EngineInner {
                in_flight: BTreeMap::new(),
                starting: BTreeSet::new(),
                queued: BTreeMap::new(),
                draining: false,
            }),
        })
    }

    /// Wire the ask router behind `/ask`. Called once at service assembly,
    /// before any connector pump runs; a second attach is refused on stderr,
    /// never silently replaced.
    pub fn attach_ask_router(&self, router: Arc<dyn GatewayAskRouter>) {
        if self.ask.set(router).is_err() {
            eprintln!(
                "conversation engine: an ask router was already attached; the second was refused"
            );
        }
    }

    /// Wire the upgrade owner behind `/upgrade`. Called once at service
    /// assembly; a second attach is refused on stderr, never silently replaced.
    pub fn attach_upgrade_launcher(&self, launcher: Arc<dyn GatewayUpgradeLauncher>) {
        if self.upgrade.set(launcher).is_err() {
            eprintln!(
                "conversation engine: an upgrade launcher was already attached; the second was \
                 refused"
            );
        }
    }

    /// The engine's view of one appended inbound event. The service and the
    /// connector pump call this under the gateway state lock, so the binding
    /// lookup reads the same state the append just wrote. Turn work is
    /// spawned, never run inline.
    pub fn appended(self: &Arc<Self>, kernel: &AgencyGateway, event: &GatewayStreamEvent) {
        if self.inner.lock().expect("conversation engine").draining {
            return;
        }
        let kind = event.event.get("kind").and_then(Value::as_str);
        if kind != Some("human-message") {
            return;
        }
        let Some(text) = event.event.get("content").and_then(Value::as_str) else {
            return;
        };
        let Some(metadata) = event.event.get("metadata").and_then(Value::as_object) else {
            return;
        };
        let connector_ref = metadata
            .get("connector_ref")
            .and_then(Value::as_str)
            .and_then(|raw| ResourceRef::parse(raw).ok());
        let platform = metadata.get("platform").and_then(Value::as_str);
        let conversation_id = metadata.get("conversation_id").and_then(Value::as_str);
        let (Some(connector_ref), Some(platform), Some(conversation_id)) =
            (connector_ref, platform, conversation_id)
        else {
            return;
        };
        let address = crate::gateway_connector::ConversationAddress {
            platform: platform.to_owned(),
            scope_id: metadata
                .get("scope_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            conversation_id: conversation_id.to_owned(),
            thread_id: metadata
                .get("thread_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
        };
        let Some(binding) = kernel.binding_by_route(&connector_ref, &address).cloned() else {
            return;
        };
        let native_message_id = metadata
            .get("native_message_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let engine = Arc::clone(self);
        match parse_slash(text) {
            SlashParse::Operation(operation) => {
                let binding_ref = binding.binding_ref.clone();
                thread::spawn(move || {
                    engine.execute_and_surface(&binding_ref, operation);
                });
            }
            SlashParse::Unknown(name) => {
                let binding_ref = binding.binding_ref.clone();
                thread::spawn(move || {
                    engine.send_surface_line(
                        &binding_ref,
                        format!(
                            "unknown command {name}; the conversation commands are: /status /stop \
                             /new /sessions /restart /pause /resume /model /harness /skills /ask"
                        ),
                    );
                });
            }
            SlashParse::NotACommand => {
                // A plain message for a binding with no agent backing keeps
                // the journal-plane behaviour the gateway already had.
                let Some(source) = self.source_for(&binding) else {
                    return;
                };
                let prompt = text.to_owned();
                let sequence = event.sequence;
                // A message that arrives while the binding's turn plane is
                // busy — a turn in flight, or one still starting — steers the
                // conversation: it queues and runs as the next turn in
                // arrival order, instead of failing against the lane's
                // one-turn-at-a-time guard.
                let admitted = {
                    let mut inner = self.inner.lock().expect("conversation engine");
                    if inner.draining {
                        return;
                    }
                    let busy = inner.in_flight.contains_key(&binding.binding_ref)
                        || inner.starting.contains(&binding.binding_ref);
                    if busy {
                        let queue = inner.queued.entry(binding.binding_ref.clone()).or_default();
                        if queue.len() >= TURN_QUEUE_LIMIT {
                            None
                        } else {
                            queue.push_back(QueuedPrompt {
                                prompt: prompt.clone(),
                                in_reply_to_sequence: sequence,
                                native_message_id: native_message_id.clone(),
                            });
                            Some(queue.len())
                        }
                    } else {
                        inner.starting.insert(binding.binding_ref.clone());
                        Some(0)
                    }
                };
                match admitted {
                    Some(0) => {
                        let engine = Arc::clone(self);
                        thread::spawn(move || {
                            engine.run_turn(source, binding, prompt, sequence, native_message_id);
                        });
                    }
                    // The engine runs under the caller's kernel lock, so the
                    // acknowledgment lines queue from their own threads — the
                    // same discipline as the command branches above.
                    Some(position) => {
                        let engine = Arc::clone(self);
                        let binding_ref = binding.binding_ref.clone();
                        thread::spawn(move || {
                            engine.send_surface_line(
                                &binding_ref,
                                format!(
                                    "queued: the agent is mid-turn; your message runs right \
                                     after it (position {position})"
                                ),
                            );
                        });
                    }
                    None => {
                        let engine = Arc::clone(self);
                        let binding_ref = binding.binding_ref.clone();
                        thread::spawn(move || {
                            engine.send_surface_line(
                                &binding_ref,
                                format!(
                                    "the queue for this conversation is full \
                                     ({TURN_QUEUE_LIMIT} messages waiting); send again once \
                                     the current turn answers"
                                ),
                            );
                        });
                    }
                }
            }
        }
    }

    fn source_for(&self, binding: &GatewayBinding) -> Option<Arc<dyn ConversationTurnSource>> {
        self.resolver
            .as_ref()?
            .turn_source_for(&binding.connector_ref, &binding.address.platform)
    }

    /// The reply path for one admitted message: prompt in, turn, reply event
    /// on the same stream, outbound Send, receipt recorded by the pump.
    fn run_turn(
        self: Arc<Self>,
        source: Arc<dyn ConversationTurnSource>,
        binding: GatewayBinding,
        prompt: String,
        in_reply_to_sequence: u64,
        native_message_id: Option<String>,
    ) {
        // The typing indicator begins at admission: the turn worker's first
        // act, before the turn source spawns any harness process. The sender
        // sees "responding…" while the harness is still starting.
        self.try_typing(&binding, true);
        let request = ConversationTurnRequest {
            binding_ref: binding.binding_ref.clone(),
            agent_session_ref: binding.agent_session_ref.clone(),
            prompt,
            in_reply_to_sequence,
        };
        let turn = match source.prompt(request) {
            Ok(turn) => turn,
            Err(error) => {
                // The failed start releases the plane, and the next queued
                // message takes it in the same locked step.
                self.release_starting_and_advance(&binding.binding_ref);
                self.record_outcome(
                    &binding,
                    ConversationTurnOutcome::Failed {
                        reason: format!("the agent could not start a turn: {error}"),
                    },
                    in_reply_to_sequence,
                    native_message_id,
                    None,
                );
                return;
            }
        };
        // A turn that would register after a restart was requested is refused
        // by the drain and recorded honestly: the drain owns the turn plane
        // from the moment the restart was requested.
        let registered = {
            let mut inner = self.inner.lock().expect("conversation engine");
            inner.starting.remove(&binding.binding_ref);
            if inner.draining {
                None
            } else {
                inner.in_flight.insert(
                    binding.binding_ref.clone(),
                    InFlightTurn {
                        turn: Arc::clone(&turn),
                        in_reply_to_sequence,
                        native_message_id: native_message_id.clone(),
                    },
                );
                Some(())
            }
        };
        if registered.is_none() {
            let receipt = turn
                .interrupt(Some("gateway is restarting".into()))
                .unwrap_or_else(|error| format!("interrupt could not be issued: {error}"));
            let outcome = turn.wait_timeout(self.policy.interrupt_grace).unwrap_or(
                ConversationTurnOutcome::Interrupted {
                    detail: Some(receipt),
                },
            );
            self.record_outcome(
                &binding,
                outcome,
                in_reply_to_sequence,
                native_message_id,
                None,
            );
            return;
        }
        // The whole wait: typing pulses for the turn's full duration, and —
        // when the connector streams — each completed segment and each tool
        // use is delivered as its own message while the turn runs.
        let (outcome, streamed) = self.await_outcome(
            &binding,
            &turn,
            in_reply_to_sequence,
            native_message_id.as_deref(),
        );
        // Whoever removed the in-flight record owns the recording: this worker
        // when the turn finished on its own, the restart drain when it took
        // the turn over. Exactly one of them journals the outcome.
        let owner = self
            .inner
            .lock()
            .expect("conversation engine")
            .in_flight
            .remove(&binding.binding_ref)
            .is_some();
        // Typing stops before the outcome is recorded: the turn's final
        // message (if any) is queued after the stop, so no `Typing(active)`
        // can ever land after it.
        self.try_typing(&binding, false);
        if owner {
            self.record_outcome(
                &binding,
                outcome,
                in_reply_to_sequence,
                native_message_id,
                streamed,
            );
            // The plane is free: the next queued message, if any, becomes
            // the next turn — the steering the conversation asked for.
            self.advance_queue(&binding.binding_ref);
        }
    }

    /// Release a binding's starting claim after a failed start and, in the
    /// same locked step, hand the turn plane to the next queued message. One
    /// atomic transfer: queued order is never jumped by a message admitted
    /// between the release and the pop.
    fn release_starting_and_advance(self: &Arc<Self>, binding_ref: &ResourceRef) {
        let next = {
            let mut inner = self.inner.lock().expect("conversation engine");
            inner.starting.remove(binding_ref);
            Self::pop_next_claimed(&mut inner, binding_ref)
        };
        if let Some(next) = next {
            self.spawn_turn_for(binding_ref, next);
        }
    }

    /// The turn plane just freed (a turn's outcome was recorded). The next
    /// queued message, if any, becomes the next turn.
    fn advance_queue(self: &Arc<Self>, binding_ref: &ResourceRef) {
        let next = {
            let inner = &mut self.inner.lock().expect("conversation engine");
            Self::pop_next_claimed(inner, binding_ref)
        };
        if let Some(next) = next {
            self.spawn_turn_for(binding_ref, next);
        }
    }

    /// Pop the binding's next queued prompt and claim its turn plane, when
    /// the plane is free and no restart is draining. The claim and the pop
    /// are one step under the engine lock.
    fn pop_next_claimed(
        inner: &mut EngineInner,
        binding_ref: &ResourceRef,
    ) -> Option<QueuedPrompt> {
        if inner.draining
            || inner.in_flight.contains_key(binding_ref)
            || inner.starting.contains(binding_ref)
        {
            return None;
        }
        match inner.queued.get_mut(binding_ref) {
            Some(queue) => {
                let next = queue.pop_front();
                if queue.is_empty() {
                    inner.queued.remove(binding_ref);
                }
                if next.is_some() {
                    inner.starting.insert(binding_ref.clone());
                }
                next
            }
            None => None,
        }
    }

    /// Start one turn for a popped queued prompt, with its binding read
    /// fresh from the kernel at the moment of the start.
    fn spawn_turn_for(self: &Arc<Self>, binding_ref: &ResourceRef, next: QueuedPrompt) {
        let binding = match self.gateway.lock() {
            Ok(kernel) => kernel.binding(binding_ref).cloned(),
            Err(_) => {
                eprintln!("conversation engine could not read the binding to run a queued prompt");
                None
            }
        };
        let Some(binding) = binding else {
            return;
        };
        let Some(source) = self.source_for(&binding) else {
            return;
        };
        let engine = Arc::clone(self);
        thread::spawn(move || {
            engine.run_turn(
                source,
                binding,
                next.prompt,
                next.in_reply_to_sequence,
                next.native_message_id,
            );
        });
    }

    /// Whether this binding's connector streams: it must declare the
    /// Streaming capability, which a connector's factory declares only when
    /// the owner's `stream_replies` allowance composes with what the platform
    /// can deliver. A connector that does not declare Streaming never streams.
    fn streaming_enabled(&self, binding: &GatewayBinding) -> bool {
        let kernel = match self.gateway.lock() {
            Ok(kernel) => kernel,
            Err(_) => return false,
        };
        kernel.connector_supports(&binding.connector_ref, ConnectorOperation::Streaming)
    }

    /// The turn wait for one running turn: wait bounded slices, refresh the
    /// typing indicator on its schedule for the turn's whole duration, and —
    /// when the connector streams — drain the turn's live progress and
    /// deliver each completed segment and each tool use as its own message.
    /// Answers the turn's outcome and what was already delivered (the
    /// completion sends nothing more when it equals the reply).
    ///
    /// Tick order is the ordering law: the typing refresh runs BEFORE the
    /// drain, so a pulse is always queued before the messages of the same
    /// tick; once the outcome is observed the loop returns before refreshing,
    /// so the last thing queued after the final message is the turn's
    /// `Typing(active: false)`.
    fn await_outcome(
        &self,
        binding: &GatewayBinding,
        turn: &Arc<dyn ConversationTurn>,
        in_reply_to_sequence: u64,
        native_message_id: Option<&str>,
    ) -> (ConversationTurnOutcome, Option<StreamedReplies>) {
        let timing = self.policy.stream;
        let progress = turn.progress();
        let streaming = self.streaming_enabled(binding);
        let mut streamed = StreamedReplies::default();
        let mut typing_at = Instant::now();
        loop {
            if let Some(outcome) = turn.wait_timeout(timing.poll) {
                return (outcome, Some(streamed));
            }
            if turn.finished() {
                // The outcome may have landed between the timed-out wait and
                // this check: take it now if it is still in the slot.
                if let Some(outcome) = turn.wait_timeout(Duration::ZERO) {
                    return (outcome, Some(streamed));
                }
                // The turn reached a terminal outcome whose record another
                // owner — the restart drain — already took: this waiter's
                // recording and streaming duty ends here. The caller's owner
                // check finds no in-flight record and journals nothing.
                return (
                    ConversationTurnOutcome::Interrupted {
                        detail: Some("the turn was taken over by the restart drain".into()),
                    },
                    None,
                );
            }
            if typing_at.elapsed() >= timing.typing_refresh {
                self.try_typing(binding, true);
                typing_at = Instant::now();
            }
            let Some(progress) = &progress else {
                continue;
            };
            for item in progress.drain() {
                match item {
                    TurnStreamItem::Segment { text } => {
                        if !streaming {
                            // Streaming off: the reply arrives as one final
                            // message at the turn's completion, as always.
                            continue;
                        }
                        if self.send_segment(
                            binding,
                            in_reply_to_sequence,
                            &text,
                            native_message_id,
                            streamed.segments.is_empty(),
                        ) {
                            streamed.segments.push(text);
                        }
                    }
                    TurnStreamItem::ToolLine { line } => {
                        // The honest record of the tool use is journalled on
                        // the stream whatever the connector can deliver; the
                        // short message rides only a streaming connector.
                        self.journal_activity(binding, in_reply_to_sequence, &line);
                        if streaming {
                            self.send_tool_line(binding, &line);
                        }
                    }
                }
            }
        }
    }

    /// Deliver one completed assistant segment as its own message: journal it
    /// as the agent-message event it is, then queue the Send — the first
    /// segment answers the inbound message, later ones stand plain in the
    /// same conversation. Answers whether the Send was queued; a segment that
    /// could not be sent leaves the completion to deliver the reply instead.
    fn send_segment(
        &self,
        binding: &GatewayBinding,
        in_reply_to_sequence: u64,
        text: &str,
        native_message_id: Option<&str>,
        first: bool,
    ) -> bool {
        let operation = OutboundOperationKind::Send {
            text: Some(text.to_owned()),
            media: Vec::new(),
            reply_to_native_message_id: if first {
                native_message_id.map(str::to_owned)
            } else {
                None
            },
        };
        let reply = GatewayAgentReply {
            binding_ref: binding.binding_ref.clone(),
            in_reply_to_sequence,
            text: text.to_owned(),
            failure: None,
        };
        if let Err(error) = self.journal_and_queue(binding, reply, operation) {
            eprintln!("conversation engine could not deliver a reply segment: {error}");
            return false;
        }
        true
    }

    /// Deliver one tool use as its own short message. Best effort: a send
    /// that cannot be queued is said on stderr; the turn is never killed by
    /// one lost line — its journal record already stands.
    fn send_tool_line(&self, binding: &GatewayBinding, line: &str) {
        let result = (|| -> Result<()> {
            let prepared = {
                let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
                kernel.prepare_operation(
                    &binding.binding_ref,
                    OutboundOperationKind::Send {
                        text: Some(line.to_owned()),
                        media: Vec::new(),
                        reply_to_native_message_id: None,
                    },
                )?
            };
            self.queues
                .queue_for(&prepared.connector_ref)
                .push(prepared);
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("conversation engine could not deliver a tool line: {error}");
        }
    }

    /// Journal one honest activity line on the binding's stream (the additive
    /// turn-activity record), publish it to live subscribers, and persist.
    /// Best effort: a line that cannot be journaled is said on stderr.
    fn journal_activity(&self, binding: &GatewayBinding, in_reply_to_sequence: u64, line: &str) {
        let result = (|| -> Result<()> {
            let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
            let event =
                kernel.record_agent_activity(&binding.binding_ref, in_reply_to_sequence, line)?;
            self.hub.publish(&binding.actuation_stream_ref, &event);
            persist_gateway_state(&kernel, self.state_file.as_deref())?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("conversation engine could not journal turn activity: {error}");
        }
    }

    /// Journal one turn outcome on the binding's stream, publish it to live
    /// subscribers, and hand the answer to the surface through the outbound
    /// queue. A failed or interrupted turn produces an honest failure line —
    /// never silence. A streamed turn whose segments already delivered the
    /// reply sends nothing more: the reply arrived piece by piece, each piece
    /// already journalled as its own agent-message event. Every other
    /// outcome — and any turn whose reply was not fully delivered — sends
    /// exactly as before.
    fn record_outcome(
        &self,
        binding: &GatewayBinding,
        outcome: ConversationTurnOutcome,
        in_reply_to_sequence: u64,
        native_message_id: Option<String>,
        streamed: Option<StreamedReplies>,
    ) {
        let (text, failure) = match outcome {
            ConversationTurnOutcome::Replied { text } => (text, None),
            ConversationTurnOutcome::Failed { reason } => (
                format!("the turn failed: {reason}"),
                Some(GatewayAgentReplyFailure::Failed { reason }),
            ),
            ConversationTurnOutcome::Interrupted { detail } => (
                match &detail {
                    Some(detail) => {
                        format!("the turn was interrupted before it answered ({detail})")
                    }
                    None => "the turn was interrupted before it answered".into(),
                },
                Some(GatewayAgentReplyFailure::Interrupted { detail }),
            ),
        };
        // A fully streamed reply was already delivered segment by segment,
        // and each segment is already on the stream journal. Only a reply
        // that differs from what was delivered — never streamed, or a
        // segment Send lost along the way — lands here as one message.
        let fully_streamed = failure.is_none()
            && streamed.is_some_and(|delivered| {
                !delivered.segments.is_empty() && delivered.joined() == text
            });
        if fully_streamed {
            return;
        }
        let operation = OutboundOperationKind::Send {
            text: Some(text.clone()),
            media: Vec::new(),
            reply_to_native_message_id: native_message_id,
        };
        if let Err(error) = self.journal_and_queue(
            binding,
            GatewayAgentReply {
                binding_ref: binding.binding_ref.clone(),
                in_reply_to_sequence,
                text: text.clone(),
                failure,
            },
            operation,
        ) {
            eprintln!("conversation engine could not deliver a reply: {error}");
        }
    }

    /// One kernel-critical moment under the state lock: journal the agent
    /// reply, publish it to subscribers, prepare the outbound operation, and
    /// persist. The queue push happens after the lock is released.
    fn journal_and_queue(
        &self,
        binding: &GatewayBinding,
        reply: GatewayAgentReply,
        operation: OutboundOperationKind,
    ) -> Result<()> {
        let prepared = {
            let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
            let event = kernel.record_agent_reply(reply)?;
            self.hub.publish(&binding.actuation_stream_ref, &event);
            let prepared = kernel.prepare_operation(&binding.binding_ref, operation)?;
            persist_gateway_state(&kernel, self.state_file.as_deref())?;
            prepared
        };
        self.queues
            .queue_for(&prepared.connector_ref)
            .push(prepared);
        Ok(())
    }

    /// A control line to the requesting conversation, through the outbound
    /// queue. Best effort: a control answer that cannot be queued is said on
    /// stderr, never silently lost.
    fn send_surface_line(&self, binding_ref: &ResourceRef, text: impl Into<String>) {
        if let Err(error) = self.queue_surface_line(binding_ref, text) {
            eprintln!("conversation engine could not answer the surface: {error}");
        }
    }

    /// The same, but the failure is the caller's to know: a line somebody is
    /// waiting for (an upgrade's receipt) must not be dropped with a note on
    /// stderr while the caller believes it was delivered.
    fn queue_surface_line(&self, binding_ref: &ResourceRef, text: impl Into<String>) -> Result<()> {
        let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
        let prepared = kernel.prepare_operation(
            binding_ref,
            OutboundOperationKind::Send {
                text: Some(text.into()),
                media: Vec::new(),
                reply_to_native_message_id: None,
            },
        )?;
        persist_gateway_state(&kernel, self.state_file.as_deref())?;
        self.queues
            .queue_for(&prepared.connector_ref)
            .push(prepared);
        Ok(())
    }

    fn try_typing(&self, binding: &GatewayBinding, active: bool) {
        let result = (|| -> Result<()> {
            let supported = {
                let kernel = self.gateway.lock().map_err(|_| poisoned())?;
                kernel.connector_supports(
                    &binding.connector_ref,
                    crate::gateway_connector::ConnectorOperation::Typing,
                )
            };
            if !supported {
                return Ok(());
            }
            let prepared = {
                let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
                kernel.prepare_operation(
                    &binding.binding_ref,
                    OutboundOperationKind::Typing { active },
                )?
            };
            self.queues
                .queue_for(&prepared.connector_ref)
                .push(prepared);
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("conversation engine typing indicator skipped: {error}");
        }
    }

    /// Execute one canonical conversation operation a carrier asked for (the
    /// CLI, the kernel, the TUI). The result travels as the
    /// `GatewayResponse::Conversation` payload for the requesting carrier; a
    /// control operation's line also goes to the conversation through the
    /// outbound queue. A read (status, sessions, the model listing, harness,
    /// skills) changes nothing, so it tells nobody else and puts no frame in
    /// flight; asked at the surface (a slash command), its line lands there.
    pub fn execute(
        self: &Arc<Self>,
        binding_ref: ResourceRef,
        operation: GatewayConversationOperation,
    ) -> Result<ConversationExecution> {
        self.run(binding_ref, operation, false)
    }

    /// One operation; `surface_reads` is set when the conversation itself
    /// asked (a slash command at the surface), so its answer lands there.
    fn run(
        self: &Arc<Self>,
        binding_ref: ResourceRef,
        operation: GatewayConversationOperation,
        surface_reads: bool,
    ) -> Result<ConversationExecution> {
        let (result, line, surface_target, restart_requested) =
            self.perform(&binding_ref, &operation)?;
        if let Some(line) = line.filter(|_| surface_reads || !reads_only(&operation)) {
            let target = surface_target.as_ref().unwrap_or(&binding_ref);
            if matches!(operation, GatewayConversationOperation::Announce { .. }) {
                // An announcement is a receipt somebody waits to be told: if it
                // cannot be queued the caller learns it and tries again later.
                self.queue_surface_line(target, line)?;
            } else {
                self.send_surface_line(target, line);
            }
        }
        Ok(ConversationExecution {
            response: GatewayResponse::Conversation {
                binding_ref,
                operation,
                result,
            },
            restart_requested,
        })
    }

    /// The ingress path: run the operation and surface its line, reporting
    /// failures on stderr. Turn-thread context; no carrier answer expected.
    fn execute_and_surface(
        self: Arc<Self>,
        binding_ref: &ResourceRef,
        operation: GatewayConversationOperation,
    ) {
        match self.run(binding_ref.clone(), operation, true) {
            Ok(_) => {}
            Err(error) => {
                eprintln!("conversation operation failed: {error}");
                self.send_surface_line(
                    binding_ref,
                    format!("the operation could not run: {error}"),
                );
            }
        }
    }

    fn perform(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
        operation: &GatewayConversationOperation,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        match operation {
            GatewayConversationOperation::Status => {
                let (result, line) = self.status(binding_ref)?;
                Ok((result, Some(line), None, false))
            }
            GatewayConversationOperation::Stop => self.stop(binding_ref),
            GatewayConversationOperation::New => self.reset(binding_ref),
            GatewayConversationOperation::Sessions => {
                let (result, line) = self.sessions(binding_ref)?;
                Ok((result, Some(line), None, false))
            }
            GatewayConversationOperation::Restart => self.restart(),
            GatewayConversationOperation::Announce { text } => {
                Ok((json!({"announced": true}), Some(text.clone()), None, false))
            }
            GatewayConversationOperation::Upgrade { apply } => self.upgrade(binding_ref, *apply),
            GatewayConversationOperation::PauseConnector { connector_ref } => {
                self.set_paused(binding_ref, connector_ref.clone(), true)
            }
            GatewayConversationOperation::ResumeConnector { connector_ref } => {
                self.set_paused(binding_ref, connector_ref.clone(), false)
            }
            GatewayConversationOperation::Model { model } => {
                self.model(binding_ref, model.as_deref())
            }
            GatewayConversationOperation::Harness => self.harness(binding_ref),
            GatewayConversationOperation::Skills => self.skills(binding_ref),
            GatewayConversationOperation::AskPosition { position, message } => {
                self.ask_position(binding_ref, position, message)
            }
        }
    }

    /// Canonical AskPosition: one attributable Communique from the asking
    /// agency to the named Position, appended to this gateway's own journal
    /// and routed exactly as `gateway send` routes — local occupancy, cross
    /// Workcell relay, held/vacant. The chat's answer mirrors send's outcomes
    /// honestly, and every refusal answers before anything is appended, so
    /// nothing is recorded on a refusal.
    fn ask_position(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
        recipient: &str,
        message: &str,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let router = self.ask.get().ok_or_else(|| {
            AikitError::new(
                "gateway_conversation.ask_router_absent",
                "/ask has no routing behind this gateway: no occupancy or recipient owners are \
                 wired into this service. Nothing was sent; nothing was recorded. Ask against a \
                 gateway served with its owners (`aikit gateway serve`).",
            )
        })?;
        let request = {
            let kernel = self.gateway.lock().map_err(|_| poisoned())?;
            let binding = kernel
                .binding(binding_ref)
                .ok_or_else(|| unknown_binding(binding_ref))?;
            GatewayAskRequest {
                binding_ref: binding.binding_ref.clone(),
                connector_ref: binding.connector_ref.to_string(),
                agent_session_ref: binding.agent_session_ref.to_string(),
                agency_ref: binding.agency_ref.to_string(),
                platform: binding.address.platform.clone(),
                conversation_id: binding.address.conversation_id.clone(),
                recipient: recipient.trim().to_owned(),
                message: message.to_owned(),
            }
        };
        let route = router.route(&request)?;
        // The append is kernel work, in process: the same command the send
        // plane uses, executed against this gateway's own journal.
        let (communique, replayed, accepted_by) = {
            let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
            match execute_gateway_command(
                &mut kernel,
                GatewayCommand::SendCommunique {
                    draft: Box::new(route.draft.clone()),
                },
            )? {
                GatewayResponse::CommuniqueAccepted {
                    communique,
                    replayed,
                    accepted_by,
                } => (communique, replayed, accepted_by),
                other => return Err(unexpected_answer(&other)),
            }
        };
        self.persist()?;
        // The route named another Workcell: one relay attempt now. A remote
        // that cannot be reached stays queued for the next relay pass — the
        // sender is never blocked on it.
        let mut relayed = None;
        if matches!(
            communique.forward,
            Some(crate::gateway_communique::CommuniqueForward::Queued { .. })
        ) {
            let outcome = router.relay(&communique, &accepted_by)?;
            {
                let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
                match execute_gateway_command(
                    &mut kernel,
                    GatewayCommand::RecordCommuniqueForward {
                        communique_ref: communique.communique_ref.clone(),
                        outcome: outcome.clone(),
                        routing: None,
                    },
                )? {
                    GatewayResponse::CommuniqueRecord { .. } => {}
                    other => return Err(unexpected_answer(&other)),
                }
            }
            self.persist()?;
            relayed = Some(outcome);
        }
        let result = json!({
            "ask": true,
            "communique_ref": communique.communique_ref,
            "replayed": replayed,
            "accepted_by": accepted_by,
            "to_position_ref": communique.to_position_ref,
            "from_position_ref": communique.from_position_ref,
            "attribution": communique.attribution,
            "state": communique.state,
            "forward": communique.forward,
            "routing": communique.routing,
            "relay": relayed,
            "delivery": route.delivery,
        });
        let provenance = match communique.attribution {
            SenderAttribution::Verified => String::new(),
            SenderAttribution::Claimed => " (claimed, not verified)".into(),
            SenderAttribution::Owner => " (the owner's recorded decision)".into(),
            SenderAttribution::Unknown => {
                " (no occupancy on this gateway names this conversation; attributed \
                 <unknown sender>)"
                    .into()
            }
        };
        let replay_note = if replayed {
            " (replayed; it was already recorded)"
        } else {
            ""
        };
        let line = match &relayed {
            Some(CommuniqueForwardOutcome::Forwarded {
                workcell_ref,
                remote_gateway_ref,
                ..
            }) => format!(
                "ask: {}{replay_note} relayed to Workcell {workcell_ref} through gateway \
                 {remote_gateway_ref}; it is delivered at the occupant of {}'s next turn \
                 boundary there{provenance}",
                communique.communique_ref, communique.to_position_ref
            ),
            Some(CommuniqueForwardOutcome::Failed {
                workcell_ref,
                error,
                ..
            }) => format!(
                "ask: {}{replay_note} is recorded and queued for relay to Workcell \
                 {workcell_ref}; the remote gateway could not be reached ({error}) and the next \
                 relay pass delivers it{provenance}",
                communique.communique_ref
            ),
            None => match communique.state {
                CommuniqueState::Held => {
                    let fact = route
                        .delivery
                        .get("fact")
                        .and_then(Value::as_str)
                        .unwrap_or("the recipient Position is vacant");
                    // A held address can come from an AgentProfile without
                    // Agency or occupancy proof. Keep the owner's consequence
                    // rather than promising a Position claim for every address.
                    let consequence = route
                        .delivery
                        .get("consequence")
                        .and_then(Value::as_str)
                        .filter(|text| !text.trim().is_empty());
                    let consequence = consequence.map(str::to_owned).unwrap_or_else(|| {
                        format!(
                            "it is delivered to the next occupant that claims {}",
                            communique.to_position_ref
                        )
                    });
                    format!(
                        "ask: {}{replay_note} held — {fact}; {consequence}{provenance}",
                        communique.communique_ref
                    )
                }
                _ => format!(
                    "ask: {}{replay_note} queued for {}; delivered at the recipient occupant's \
                     next turn boundary{provenance}",
                    communique.communique_ref, communique.to_position_ref
                ),
            },
        };
        Ok((result, Some(line), None, false))
    }

    fn status(&self, binding_ref: &ResourceRef) -> Result<(Value, String)> {
        let kernel = self.gateway.lock().map_err(|_| poisoned())?;
        let binding = kernel
            .binding(binding_ref)
            .cloned()
            .ok_or_else(|| unknown_binding(binding_ref))?;
        let stream_position = kernel.stream_position(&binding.actuation_stream_ref);
        let link = self.link(&kernel, &binding);
        let health = kernel
            .status()
            .connector_health
            .into_iter()
            .find(|health| health.connector_ref == binding.connector_ref);
        let backing = self
            .resolver
            .as_ref()
            .and_then(|resolver| {
                resolver.turn_source_for(&binding.connector_ref, &binding.address.platform)
            })
            .and_then(|source| source.harness());
        let (in_flight, queued_prompts) = {
            let inner = self.inner.lock().expect("conversation engine");
            (
                inner.in_flight.contains_key(binding_ref),
                inner.queued.get(binding_ref).map_or(0, |queue| queue.len()),
            )
        };
        let connector_state = health
            .as_ref()
            .map(|health| format!("{:?}", health.state))
            .unwrap_or_else(|| "unknown".into());
        let line = format!(
            "status: binding {}; stream {} at {} event(s); turn in flight: {}; queued: {}; \
             backing: {}; connector: {}",
            binding.binding_ref,
            binding.actuation_stream_ref,
            stream_position.map(|(_, count)| count).unwrap_or(0),
            if in_flight { "yes" } else { "no" },
            queued_prompts,
            backing.as_deref().unwrap_or("none"),
            connector_state,
        );
        Ok((
            json!({
                "binding_ref": binding.binding_ref.to_string(),
                "connector_ref": binding.connector_ref.to_string(),
                "address": binding.address,
                "agent_session_ref": binding.agent_session_ref.to_string(),
                "stream_ref": binding.actuation_stream_ref.to_string(),
                "stream_last_sequence": stream_position.map(|(last, _)| last),
                "stream_event_count": stream_position.map(|(_, count)| count),
                "context_revision": binding.context_revision,
                "forked_from": binding.forked_from,
                "turn_in_flight": in_flight,
                "queued_prompts": queued_prompts,
                "agent_backing": backing,
                "connector_health": health.map(|health| json!({
                    "state": health.state,
                    "detail": health.detail,
                })),
                "link": link,
            }),
            line,
        ))
    }

    fn stop(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let kernel = self.gateway.lock().map_err(|_| poisoned())?;
        kernel
            .binding(binding_ref)
            .ok_or_else(|| unknown_binding(binding_ref))?;
        drop(kernel);
        // A stop stops the conversation's work: the in-flight turn is
        // interrupted, and whatever was queued to steer it is dropped — the
        // next message starts fresh, not behind the messages it superseded.
        let (turn, dropped_queued) = {
            let mut inner = self.inner.lock().expect("conversation engine");
            let turn = inner
                .in_flight
                .get(binding_ref)
                .map(|in_flight| Arc::clone(&in_flight.turn));
            let dropped = inner
                .queued
                .remove(binding_ref)
                .map_or(0, |queue| queue.len());
            (turn, dropped)
        };
        let dropped_note = if dropped_queued > 0 {
            format!("; dropped {dropped_queued} queued message(s)")
        } else {
            String::new()
        };
        match turn {
            Some(turn) => {
                let receipt =
                    turn.interrupt(Some("stop requested from the conversation".into()))?;
                Ok((
                    json!({
                        "stopped": true,
                        "receipt": receipt,
                        "dropped_queued": dropped_queued,
                    }),
                    Some(format!(
                        "stop: {receipt}{dropped_note}; the interrupted turn is recorded on the \
                         stream"
                    )),
                    None,
                    false,
                ))
            }
            None if dropped_queued > 0 => Ok((
                json!({
                    "stopped": false,
                    "dropped_queued": dropped_queued,
                }),
                Some(format!(
                    "stop: dropped {dropped_queued} queued message(s); no turn was in flight"
                )),
                None,
                false,
            )),
            None => Ok((
                json!({
                    "stopped": false,
                    "detail": "no turn is in flight on this conversation",
                }),
                Some("stop: no turn is in flight on this conversation".into()),
                None,
                false,
            )),
        }
    }

    /// Canonical New: a fresh turn context for the binding, using the
    /// kernel's fork lineage. The old stream is retained in the journal and
    /// named in the result; the route keeps its connector conversation.
    fn reset(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let mut kernel = self.gateway.lock().map_err(|_| poisoned())?;
        let binding = kernel
            .binding(binding_ref)
            .cloned()
            .ok_or_else(|| unknown_binding(binding_ref))?;
        let revision = binding.context_revision + 1;
        let old_stream = binding.actuation_stream_ref.clone();
        let old_session = binding.agent_session_ref.clone();
        let (forked_from, fork_note) = match kernel.stream_position(&old_stream) {
            Some((last, _)) if last > 0 => (
                Some(crate::gateway_runtime::GatewayForkOrigin {
                    stream_ref: old_stream.clone(),
                    at_sequence: last,
                }),
                format!("forked from {old_stream}@{last}"),
            ),
            _ => (
                None,
                "with no journal to fork from; the new stream starts empty".into(),
            ),
        };
        let mut next = binding.clone();
        next.binding_ref = ResourceRef::parse(format!("{binding_ref}/generation-{revision}"))
            .map_err(|error| {
                AikitError::new(
                    "gateway_conversation.reset_ref",
                    format!("build regenerated binding ref: {error}"),
                )
            })?;
        next.agent_session_ref = ResourceRef::parse(format!("{old_session}/generation-{revision}"))
            .map_err(|error| {
                AikitError::new(
                    "gateway_conversation.reset_ref",
                    format!("build regenerated session ref: {error}"),
                )
            })?;
        next.actuation_ref =
            ResourceRef::parse(format!("{}/generation-{revision}", binding.actuation_ref))
                .map_err(|error| {
                    AikitError::new(
                        "gateway_conversation.reset_ref",
                        format!("build regenerated actuation ref: {error}"),
                    )
                })?;
        next.actuation_stream_ref =
            ResourceRef::parse(format!("{old_stream}/generation-{revision}")).map_err(|error| {
                AikitError::new(
                    "gateway_conversation.reset_ref",
                    format!("build regenerated stream ref: {error}"),
                )
            })?;
        next.forked_from = forked_from;
        next.context_revision = revision;
        let new_binding = next.clone();
        let previous = binding.clone();
        kernel.unbind(binding_ref)?;
        if let Err(error) = kernel.bind(new_binding.clone()) {
            // Put the previous binding back: a failed reset changes nothing.
            let _ = kernel.bind(previous.clone());
            return Err(error);
        }
        persist_gateway_state(&kernel, self.state_file.as_deref())?;
        drop(kernel);
        // A reset is a hard cut: queued messages were admitted against the
        // context being abandoned, and carrying them would run an old
        // question against a brand-new conversation. They are dropped and
        // named — never silently lost; each is still on the old stream's
        // journal as the inbound message it was.
        let dropped_queued = {
            let mut inner = self.inner.lock().expect("conversation engine");
            inner
                .queued
                .remove(&previous.binding_ref)
                .map_or(0, |queue| queue.len())
        };
        if let Some(source) = self.source_for(&new_binding) {
            if let Err(error) = source.reset(&new_binding.agent_session_ref) {
                eprintln!("conversation engine could not open the fresh turn context: {error}");
            }
        }
        let result = json!({
            "reset": true,
            "previous_binding_ref": previous.binding_ref.to_string(),
            "binding_ref": new_binding.binding_ref.to_string(),
            "previous_agent_session_ref": old_session.to_string(),
            "agent_session_ref": new_binding.agent_session_ref.to_string(),
            "previous_stream_ref": old_stream.to_string(),
            "previous_stream_retained": true,
            "stream_ref": new_binding.actuation_stream_ref.to_string(),
            "forked_from": new_binding.forked_from,
            "context_revision": revision,
            "queued_dropped": dropped_queued,
        });
        let dropped_note = if dropped_queued > 0 {
            format!(
                "; {dropped_queued} queued message(s) dropped with the old context — send \
                 them again here"
            )
        } else {
            String::new()
        };
        let line = format!(
            "conversation reset: new session {}; the previous session {} and stream {} are \
             retained, {fork_note}; context revision {revision}{dropped_note}",
            new_binding.agent_session_ref, old_session, old_stream,
        );
        Ok((
            result,
            Some(line),
            Some(new_binding.binding_ref.clone()),
            false,
        ))
    }

    fn sessions(self: &Arc<Self>, binding_ref: &ResourceRef) -> Result<(Value, String)> {
        let kernel = self.gateway.lock().map_err(|_| poisoned())?;
        let binding = kernel
            .binding(binding_ref)
            .cloned()
            .ok_or_else(|| unknown_binding(binding_ref))?;
        let link = self.link(&kernel, &binding);
        drop(kernel);
        let source = self.source_for(&binding);
        let native = match &source {
            Some(source) => source.sessions()?,
            None => Vec::new(),
        };
        let harness = source
            .as_ref()
            .and_then(|source| source.harness())
            .unwrap_or_else(|| "none".into());
        let line = format!(
            "sessions: agent session {} on harness {} holds {} native session(s); {}",
            binding.agent_session_ref,
            harness,
            native.len(),
            link_line(&link),
        );
        Ok((
            json!({
                "agent_session_ref": binding.agent_session_ref.to_string(),
                "harness": source.as_ref().and_then(|source| source.harness()),
                "sessions": native,
                "link": link,
            }),
            line,
        ))
    }

    /// The binding's link as this gateway knows it, read under the kernel
    /// lock: the newest frames of its Stream (each stamped when the gateway
    /// journaled it) and how many the journal holds; the check-in — the
    /// newest frame's instant — and its age by the gateway's own clock; the
    /// turns running on the binding's turn plane (one at a time, starting or
    /// in flight); and the frames in flight, the admitted prompts waiting for
    /// the harness and the sends waiting for the connector's receipt.
    fn link(&self, kernel: &AgencyGateway, binding: &GatewayBinding) -> Value {
        let observed_at = crate::gateway_posture::unix_ms_now();
        let (tail, held) = kernel
            .stream_tail(&binding.actuation_stream_ref, LINK_FRAME_LIMIT)
            .unwrap_or((&[], 0));
        let checked_in = tail.last().and_then(|frame| frame.recorded_at_unix_ms);
        let (running, queued) = {
            let inner = self.inner.lock().expect("conversation engine");
            let busy = inner.in_flight.contains_key(&binding.binding_ref)
                || inner.starting.contains(&binding.binding_ref);
            (
                usize::from(busy),
                inner
                    .queued
                    .get(&binding.binding_ref)
                    .map_or(0, |queue| queue.len()),
            )
        };
        let pending = kernel.pending_deliveries_for(&binding.actuation_stream_ref);
        json!({
            "observed_at_unix_ms": observed_at,
            "last_check_in_unix_ms": checked_in,
            "check_in_age_seconds": checked_in
                .map(|at| observed_at.saturating_sub(at) as f64 / 1000.0),
            "turns_running": running,
            "frames_in_flight": queued + pending,
            "queued_prompts": queued,
            "pending_deliveries": pending,
            "frame_count": held,
            "last_sequence": tail.last().map(|frame| frame.sequence),
            "frames_limit": LINK_FRAME_LIMIT,
            "frames": tail.iter().map(link_frame).collect::<Vec<_>>(),
        })
    }

    /// The binding behind a surface request and its turn source, so the
    /// selector operations share one lookup.
    fn binding_and_source(
        &self,
        binding_ref: &ResourceRef,
    ) -> Result<(GatewayBinding, Option<Arc<dyn ConversationTurnSource>>)> {
        let binding = {
            let kernel = self.gateway.lock().map_err(|_| poisoned())?;
            kernel
                .binding(binding_ref)
                .cloned()
                .ok_or_else(|| unknown_binding(binding_ref))?
        };
        let source = self.source_for(&binding);
        Ok((binding, source))
    }

    /// Canonical Model: list what the harness's own native selector discloses,
    /// or select one provider-advertised model through the same seam. No
    /// backing, or a harness with no selector, is answered honestly.
    fn model(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
        model: Option<&str>,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let (binding, source) = self.binding_and_source(binding_ref)?;
        let Some(source) = source else {
            return Ok((
                json!({
                    "available": false,
                    "reason": "this conversation has no agent backing",
                }),
                Some(
                    "model: this conversation has no agent backing, so there is no model \
                     selector"
                        .into(),
                ),
                None,
                false,
            ));
        };
        let harness = source.harness().unwrap_or_else(|| "unnamed".into());
        match model {
            None => {
                let controls = source.model_controls(&binding.agent_session_ref)?;
                let line = if controls.get("available").is_some_and(Value::is_array) {
                    let roster = controls["available"]
                        .as_array()
                        .expect("checked array")
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "model: harness {harness} offers {roster}; select one with /model <name>"
                    )
                } else if controls
                    .get("model_selection")
                    .is_some_and(Value::is_boolean)
                {
                    if controls["model_selection"] == json!(true) {
                        format!(
                            "model: harness {harness} exposes a native model selector; name the \
                             provider model with /model <name>"
                        )
                    } else {
                        let reason = controls
                            .get("reason")
                            .and_then(Value::as_str)
                            .filter(|reason| !reason.is_empty())
                            .unwrap_or("the harness discloses no selector");
                        format!("model: harness {harness} exposes no model selector ({reason})")
                    }
                } else {
                    format!("model: harness {harness} disclosed its selector: {controls}")
                };
                Ok((
                    json!({
                        "harness": harness,
                        "agent_session_ref": binding.agent_session_ref.to_string(),
                        "controls": controls,
                    }),
                    Some(line),
                    None,
                    false,
                ))
            }
            Some(id) => {
                let receipt = source.set_model(&binding.agent_session_ref, id)?;
                let previous = receipt
                    .get("previous")
                    .and_then(Value::as_str)
                    .unwrap_or("unrecorded");
                Ok((
                    json!({
                        "harness": harness,
                        "agent_session_ref": binding.agent_session_ref.to_string(),
                        "model": id,
                        "receipt": receipt,
                    }),
                    Some(format!(
                        "model: {id} selected on harness {harness} (previous: {previous}); the \
                         receipt is the harness's own confirmation"
                    )),
                    None,
                    false,
                ))
            }
        }
    }

    /// Canonical Harness: the binding's current backing and the available
    /// provider ids. Switching is disclosed as the exact command, never
    /// performed: a live harness swap is a session-replacement event this
    /// kernel does not own.
    fn harness(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let (binding, source) = self.binding_and_source(binding_ref)?;
        let current = source.as_ref().and_then(|source| source.harness());
        let available = self
            .resolver
            .as_ref()
            .map(|resolver| resolver.available_backings())
            .unwrap_or_default();
        let names = available
            .iter()
            .filter_map(|backing| backing.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(", ");
        let switch_command = format!(
            "aikit gateway connector add --platform {} --ref {} --agent-backing <id> …",
            binding.address.platform, binding.connector_ref
        );
        let line = format!(
            "harness: backing {}; available backings: {}; switching re-declares the connector \
             and restarts the service — a live swap is a session-replacement event the gateway \
             does not perform",
            current.as_deref().unwrap_or("none"),
            if names.is_empty() { "none" } else { &names },
        );
        Ok((
            json!({
                "current": current,
                "available": available,
                "switch_command": switch_command,
                "law": "a backing switch is a session-replacement event; the gateway discloses \
                        the command and never performs it",
            }),
            Some(line),
            None,
            false,
        ))
    }

    /// Canonical Skills: the aikit skill surface the backed harness carries,
    /// with the invocation law disclosed — the harness carries skills in-turn;
    /// the gateway does not execute skills.
    fn skills(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let (binding, source) = self.binding_and_source(binding_ref)?;
        let Some(source) = source else {
            return Ok((
                json!({
                    "available": false,
                    "reason": "this conversation has no agent backing",
                }),
                Some(
                    "skills: this conversation has no agent backing, so there is no skill \
                     surface to disclose"
                        .into(),
                ),
                None,
                false,
            ));
        };
        let harness = source.harness().unwrap_or_else(|| "unnamed".into());
        let skills = source.skills()?;
        let line = format!(
            "skills: {} available on harness {harness}; the harness carries skills in-turn — \
             name the skill in your message (the gateway does not execute skills)",
            skills.len(),
        );
        Ok((
            json!({
                "harness": harness,
                "agent_session_ref": binding.agent_session_ref.to_string(),
                "skills": skills,
                "invocation": "the harness carries skills in-turn; name the skill in \
                               conversation — the gateway does not execute skills",
            }),
            Some(line),
            None,
            false,
        ))
    }

    /// The drain: stop admitting, resolve every in-flight turn under a bounded
    /// grace, record honestly, persist — and report exactly what it met. No
    /// binding is needed: a restart asked for by a signal or an operator
    /// drains the whole gateway the same way.
    ///
    /// A turn that does not finish inside the grace is interrupted, and what
    /// it did before then is an uncertain effect: the interruption is
    /// journaled on its stream, named in the report, and the turn is never
    /// replayed.
    pub fn drain(self: &Arc<Self>, reason: &str, grace: Option<Duration>) -> Result<DrainReport> {
        let grace = grace.unwrap_or(self.policy.turn_grace);
        let started_at_unix_ms = crate::gateway_posture::unix_ms_now();
        // The drain takes the in-flight turns out — removing each record is
        // what makes the drain its outcome's owner, so the turn's own worker
        // stands down and exactly one of the two journals the outcome. The
        // queued prompts it takes too: they never started a turn, so nothing
        // ran for them — they are named in the report, not silently dropped.
        let (taken, queued_taken) = {
            let mut inner = self.inner.lock().expect("conversation engine");
            inner.draining = true;
            let queued = std::mem::take(&mut inner.queued)
                .into_iter()
                .flat_map(|(binding_ref, queue)| {
                    queue
                        .into_iter()
                        .map(move |prompt| (binding_ref.clone(), prompt))
                })
                .collect::<Vec<_>>();
            (
                std::mem::take(&mut inner.in_flight)
                    .into_iter()
                    .collect::<Vec<_>>(),
                queued,
            )
        };
        let mut turns_resolved = Vec::new();
        let mut turns_interrupted = Vec::new();
        let mut prompts_not_started = Vec::new();
        for (binding_ref, prompt) in queued_taken {
            prompts_not_started.push(DrainedTurn {
                binding_ref: binding_ref.to_string(),
                in_reply_to_sequence: prompt.in_reply_to_sequence,
                detail: Some("queued; the turn never started".into()),
            });
        }
        for (binding_ref, in_flight) in taken {
            let (outcome, was_interrupted, detail) = match in_flight.turn.wait_timeout(grace) {
                Some(outcome) => (outcome, false, None),
                None => {
                    let receipt = in_flight
                        .turn
                        .interrupt(Some(format!("gateway drain: {reason}")))
                        .unwrap_or_else(|error| format!("interrupt could not be issued: {error}"));
                    let outcome = in_flight
                        .turn
                        .wait_timeout(self.policy.interrupt_grace)
                        .unwrap_or(ConversationTurnOutcome::Interrupted {
                            detail: Some(receipt.clone()),
                        });
                    (outcome, true, Some(receipt))
                }
            };
            let record = DrainedTurn {
                binding_ref: binding_ref.to_string(),
                in_reply_to_sequence: in_flight.in_reply_to_sequence,
                detail,
            };
            if was_interrupted {
                turns_interrupted.push(record);
            } else {
                turns_resolved.push(record);
            }
            let binding = {
                let kernel = self.gateway.lock().map_err(|_| poisoned())?;
                kernel.binding(&binding_ref).cloned()
            };
            if let Some(binding) = binding {
                self.record_outcome(
                    &binding,
                    outcome,
                    in_flight.in_reply_to_sequence,
                    in_flight.native_message_id,
                    None,
                );
            }
        }
        self.persist()?;
        let communiques = {
            let kernel = self.gateway.lock().map_err(|_| poisoned())?;
            kernel.communiques().counts()
        };
        Ok(DrainReport {
            measured: true,
            reason: reason.to_owned(),
            started_at_unix_ms,
            finished_at_unix_ms: crate::gateway_posture::unix_ms_now(),
            grace_ms: grace.as_millis() as u64,
            turns_resolved,
            turns_interrupted,
            prompts_not_started,
            communiques,
            tick_at_stop: None,
            gateway_pid: None,
            children: Vec::new(),
        })
    }

    /// The restart drain: [`drain`](Self::drain), then answer; the service
    /// exits so its service manager rematerialises it.
    fn restart(self: &Arc<Self>) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let report = self.drain("conversation restart", None)?;
        let resolved = report.turns_resolved.len();
        let interrupted = report.turns_interrupted.len();
        let not_started = report.prompts_not_started.len();
        let summary = json!({
            "resolved": resolved,
            "interrupted": interrupted,
            "prompts_not_started": not_started,
            "turn_grace_ms": report.grace_ms,
        });
        let queued_note = if not_started > 0 {
            format!(", {not_started} queued message(s) never started and are named in the report")
        } else {
            String::new()
        };
        Ok((
            json!({"restarting": true, "drain": summary}),
            Some(format!(
                "restarting: {resolved} turn(s) resolved, {interrupted} \
                 interrupted{queued_note}; the state snapshot is persisted and the service \
                 manager will rematerialise the gateway"
            )),
            None,
            true,
        ))
    }

    /// `/upgrade` (plan) and `/upgrade apply` (start) for one conversation.
    /// The worker that performs the upgrade is not a child of this process:
    /// this gateway is what it restarts.
    fn upgrade(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
        apply: bool,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let Some(launcher) = self.upgrade.get() else {
            let line = "no upgrade owner stands behind this gateway; run `aikit gateway upgrade \
                        plan` on the machine it serves"
                .to_owned();
            return Ok((
                json!({"upgrade": "unavailable", "reason": line}),
                Some(line),
                None,
                false,
            ));
        };
        if !apply {
            let (plan, line) = launcher.plan()?;
            return Ok((
                json!({"upgrade": "plan", "plan": plan}),
                Some(line),
                None,
                false,
            ));
        }
        let origin = {
            let kernel = self.gateway.lock().map_err(|_| poisoned())?;
            let binding = kernel.binding(binding_ref).cloned();
            // A group admits several senders under one binding, and a slash
            // command carries a message's authority, not an owner's. Changing
            // the machine is asked for in a direct conversation.
            if binding
                .as_ref()
                .is_some_and(|b| b.address.scope_id.as_deref() == Some("group"))
            {
                let line = "/upgrade apply is refused in a group conversation: a group \
                            admits several senders. Ask in a direct conversation with this \
                            gateway, or run `aikit gateway upgrade apply` on the machine"
                    .to_owned();
                return Ok((
                    json!({"upgrade": "refused", "reason": line}),
                    Some(line),
                    None,
                    false,
                ));
            }
            UpgradeOrigin {
                binding_ref: binding_ref.to_string(),
                connector_ref: binding.as_ref().map(|b| b.connector_ref.to_string()),
                in_reply_to_sequence: None,
            }
        };
        let (started, line) = launcher.start(origin)?;
        // The gateway keeps serving: the worker drains it once the new build is
        // installed, and the receipt comes back to this conversation.
        Ok((
            json!({"upgrade": "started", "started": started}),
            Some(line),
            None,
            false,
        ))
    }

    fn set_paused(
        self: &Arc<Self>,
        binding_ref: &ResourceRef,
        connector_ref: Option<ResourceRef>,
        paused: bool,
    ) -> Result<(Value, Option<String>, Option<ResourceRef>, bool)> {
        let connector_ref = match connector_ref {
            Some(connector_ref) => connector_ref,
            None => {
                let kernel = self.gateway.lock().map_err(|_| poisoned())?;
                kernel
                    .binding(binding_ref)
                    .ok_or_else(|| unknown_binding(binding_ref))?
                    .connector_ref
                    .clone()
            }
        };
        self.controls.set_paused(&connector_ref, paused);
        let state = if paused { "paused" } else { "resumed" };
        Ok((
            json!({"connector_ref": connector_ref.to_string(), "paused": paused}),
            Some(format!(
                "connector {connector_ref} {state}; its pump stops (or resumes) admitting new \
                 ingress and the pause shows in connector health"
            )),
            None,
            false,
        ))
    }

    fn persist(&self) -> Result<()> {
        let kernel = self.gateway.lock().map_err(|_| poisoned())?;
        persist_gateway_state(&kernel, self.state_file.as_deref())
    }

    /// The file the gateway's state is persisted to, when it has one.
    pub fn state_file(&self) -> Option<&std::path::Path> {
        self.state_file.as_deref()
    }
}

/// How many of a binding's newest frames its link carries.
const LINK_FRAME_LIMIT: usize = 100;
/// How many characters of a frame's words its link carries.
const LINK_NOTE_CHARS: usize = 240;

/// Whether an operation only reads: it changes nothing, so a carrier's ask is
/// answered to the carrier and nothing is said into the conversation.
fn reads_only(operation: &GatewayConversationOperation) -> bool {
    matches!(
        operation,
        GatewayConversationOperation::Status
            | GatewayConversationOperation::Sessions
            | GatewayConversationOperation::Harness
            | GatewayConversationOperation::Skills
            | GatewayConversationOperation::Model { model: None }
    )
}

/// One journaled frame as the link carries it: its sequence, its kind, which
/// way it went — named by the door that journaled it (ingress marks the
/// connector's event, an agent reply or activity the inbound frame it
/// answers) — when the gateway journaled it, and how its words open.
fn link_frame(frame: &GatewayStreamEvent) -> Value {
    let event = &frame.event;
    let metadata = event.get("metadata");
    let marked = |key: &str| metadata.and_then(|metadata| metadata.get(key)).is_some();
    let direction = if marked("connector_event_ref") {
        Some("to-agent")
    } else if marked("in_reply_to_sequence") {
        Some("from-agent")
    } else {
        None
    };
    let mut row = serde_json::Map::new();
    row.insert("sequence".into(), json!(frame.sequence));
    for key in ["kind", "custom_kind"] {
        if let Some(word) = event.get(key).and_then(Value::as_str) {
            row.insert(key.into(), json!(word));
        }
    }
    if let Some(direction) = direction {
        row.insert("direction".into(), json!(direction));
    }
    if let Some(at) = frame.recorded_at_unix_ms {
        row.insert("recorded_at_unix_ms".into(), json!(at));
    }
    if let Some(text) = event.get("content").and_then(Value::as_str) {
        let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let note = match words.char_indices().nth(LINK_NOTE_CHARS) {
            Some((cut, _)) => format!("{}…", &words[..cut]),
            None => words,
        };
        row.insert("note".into(), json!(note));
    }
    Value::Object(row)
}

/// The link in one line, for the surface.
fn link_line(link: &Value) -> String {
    let count = |key: &str| link.get(key).and_then(Value::as_u64).unwrap_or(0);
    let check_in = match link.get("check_in_age_seconds").and_then(Value::as_f64) {
        Some(age) => format!("last check-in {} s ago", age.round()),
        None if count("frame_count") == 0 => "no check-in yet".to_owned(),
        None => "no stamped check-in".to_owned(),
    };
    format!(
        "link: {} frame(s), {check_in}, {} turn(s) running, {} frame(s) in flight",
        count("frame_count"),
        count("turns_running"),
        count("frames_in_flight"),
    )
}

fn unknown_binding(binding_ref: &ResourceRef) -> AikitError {
    AikitError::new(
        "agency_gateway.unknown_binding",
        format!("gateway binding {binding_ref} does not exist"),
    )
}

fn unexpected_answer(response: &GatewayResponse) -> AikitError {
    AikitError::new(
        "gateway_conversation.unexpected_kernel_answer",
        format!(
            "the kernel answered {} to a Communique command",
            serde_json::to_value(response)
                .ok()
                .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| "an unknown response".into())
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_connection::AgentConnectionAdapter;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    const CONFINED: [&str; 8] = [
        "pi",
        "--mode",
        "rpc",
        "--no-tools",
        "--no-extensions",
        "--no-skills",
        "--no-mcp",
        "--no-context-files",
    ];

    /// be2d (review 5): pi is held to no tools only by flags pi's own parser
    /// reads as flags. A confining flag read as another flag's value, a flag
    /// pi would refuse, a key, a file or a message before the prompt, or a
    /// program that is not pi, are each refused in words.
    #[test]
    fn pi_is_held_to_no_tools_only_by_flags_its_own_grammar_reads() {
        let pi = |args: &[&str]| ConversationHarnessProtocol::PiRpc.confines(&argv(args));
        assert!(pi(&CONFINED).is_ok());
        assert!(pi(&["/opt/bin/pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc"]).is_ok());
        for (args, why) in [
            (
                vec![
                    "pi",
                    "--system-prompt",
                    "--no-tools",
                    "-ne",
                    "-ns",
                    "--no-mcp",
                    "-nc",
                ],
                "pi reads --no-tools as the value of --system-prompt",
            ),
            (
                vec!["pi", "--name", "-nt", "-ne", "-ns", "--no-mcp", "-nc"],
                "pi reads -nt as the value of --name",
            ),
            (
                vec![
                    "pi",
                    "--tools",
                    "--no-tools",
                    "-ne",
                    "-ns",
                    "--no-mcp",
                    "-nc",
                ],
                "the value of --tools",
            ),
            (
                vec!["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc", "-x"],
                "-x is not an option pi knows",
            ),
            (
                vec!["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc", "--mode", "-p"],
                "--mode requires a value",
            ),
            (
                vec!["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc", "-e", "x.ts"],
                "names --extension",
            ),
            (
                vec!["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc", "-nbt"],
                "names --no-builtin-tools",
            ),
            (
                vec![
                    "pi",
                    "-nt",
                    "-ne",
                    "-ns",
                    "--no-mcp",
                    "-nc",
                    "--api-key",
                    "k",
                ],
                "carries a key",
            ),
            (
                vec!["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc", "@/etc/hosts"],
                "attach the file /etc/hosts",
            ),
            (
                vec![
                    "pi",
                    "-nt",
                    "-ne",
                    "-ns",
                    "--no-mcp",
                    "-nc",
                    "--",
                    "--no-tools",
                ],
                "take --no-tools as a message",
            ),
            (
                vec![
                    "pi",
                    "-nt",
                    "-ne",
                    "-ns",
                    "--no-mcp",
                    "-nc",
                    "read /etc/hosts",
                ],
                "as a message",
            ),
            (
                vec!["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc", "-p", "hello"],
                "as a message",
            ),
            (
                vec!["python3", "-nt", "-ne", "-ns", "--no-mcp", "-nc"],
                "only pi (a program named pi",
            ),
        ] {
            let error = pi(&args).expect_err(&format!("{args:?}"));
            assert!(error.contains(why), "{args:?}: {error}");
        }
        // `--` ends pi's options: a confining flag after it is a message.
        let error = pi(&["pi", "--", "-nt", "-ne", "-ns", "--no-mcp", "-nc"]).unwrap_err();
        assert!(error.contains("lacks --no-tools"), "{error}");
    }

    /// be2f (review 5): the reading of pi's flags holds only for the pi it
    /// was made for, by major and minor version; any other is named, with
    /// both versions.
    #[test]
    fn pis_flags_are_read_only_for_the_pi_version_the_reading_was_made_for() {
        for fits in ["1.5.0", "1.5.0\n", "1.5.12", "v1.5.1", "pi 1.5.0-beta.2"] {
            assert!(pi_version_fits(fits).is_ok(), "{fits:?}");
        }
        for other in ["1.6.0", "1.4.9", "2.5.0", "0.84.4"] {
            let why = pi_version_fits(other).unwrap_err();
            assert!(
                why.contains(&format!("this pi is version {other}"))
                    && why.contains("as pi 1.5.0 reads them"),
                "{other}: {why}"
            );
        }
        let why = pi_version_fits("").unwrap_err();
        assert!(why.contains("reports no version AIKit can read"), "{why}");
    }

    /// be2d (review 5): the provider an argv selects is read as pi keeps it,
    /// and only pi's own variable for it is that provider's credential.
    #[test]
    fn a_pi_argv_selects_its_provider_and_pi_names_its_credential() {
        let mut args = argv(&CONFINED);
        assert_eq!(pi_selected_provider(&args), None);
        args.extend(argv(&["--provider", "openai", "--provider", "anthropic"]));
        assert_eq!(pi_selected_provider(&args).as_deref(), Some("anthropic"));
        assert_eq!(
            pi_selected_provider(&argv(&["pi", "--system-prompt", "--provider", "x"])),
            None,
            "a value, not the flag"
        );
        assert_eq!(
            pi_provider_credential_variables("anthropic"),
            ["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"]
        );
        assert_eq!(pi_provider_credential_variables("zai"), ["ZAI_API_KEY"]);
        assert!(pi_provider_credential_variables("nobody").is_empty());
    }

    /// Review gap (be2d): the guard ends a tool-less turn on exactly what each
    /// adapter makes of a tool at work: pi's and Prime's tool results, and
    /// Prime's child run, as well as a tool call.
    #[test]
    fn the_guard_reads_each_adapters_own_tool_signals() {
        let connection = ResourceRef::parse("connection/test/guard").unwrap();
        let mut pi = crate::pi_rpc_connection::PiRpcConnectionAdapter::new(
            connection.clone(),
            "/work".into(),
            vec![],
        );
        let mut prime = crate::prime_rpc_connection::PrimeRpcConnectionAdapter::new(
            connection,
            "/work".into(),
            vec![],
        );
        let kinds = |signals: Vec<crate::agent_connection::ConnectionSignal>| {
            signals
                .into_iter()
                .map(|signal| signal.kind)
                .collect::<Vec<_>>()
        };
        for (adapter, wire) in [
            (
                "pi",
                json!({"type": "tool_execution_end", "toolCallId": "t1"}),
            ),
            (
                "pi",
                json!({"type": "tool_execution_start", "toolCallId": "t1"}),
            ),
            (
                "prime",
                json!({"type": "tool_execution_end", "toolCallId": "t1"}),
            ),
            ("prime", json!({"type": "rlm_child_update", "child": "c1"})),
        ] {
            let signals = if adapter == "pi" {
                kinds(pi.ingest(wire.clone()).unwrap())
            } else {
                kinds(prime.ingest(wire.clone()).unwrap())
            };
            assert_eq!(signals.len(), 1, "{adapter} {wire}");
            assert!(
                is_tool_event(&signals[0]),
                "{adapter} {wire}: {:?}",
                signals[0]
            );
        }
        let said = kinds(
            pi.ingest(json!({"type": "message_update",
                "assistantMessageEvent": {"type": "text_delta", "delta": "hello"}}))
                .unwrap(),
        );
        assert!(!is_tool_event(&said[0]), "words are no tool");
    }
}
