//! The Conversation aperture: a terminal view over one running Agency
//! Gateway's bound conversations.
//!
//! Everything shown here is the gateway's own record. The roster is the
//! gateway's ecology read model; the history is a bounded replay window from
//! the stream journal; live lines arrive on a real subscription; a composed
//! message enters the same stream through the same ingest path a connector
//! uses, under the gateway's own ingress policy. This surface keeps a
//! rendered window, never a transcript store: a reopened or reconnected
//! conversation is rebuilt from the journal cursor, so nothing is invented,
//! nothing is duplicated, and nothing the operator typed is ever re-sent.

use std::time::{Duration, Instant};

use ratatui::text::{Line, Span};
use serde_json::{json, Value};

use aikit_adapters::gateway_client::{
    gateway_command_within, gateway_subscribe, GatewayCarrierTarget, GatewaySubscription,
    GatewaySubscriptionRead,
};
use aikit_adapters::gateway_connector::{
    ConnectorConnectionState, ConnectorHealth, ConversationAddress, InboundEvent, InboundEventKind,
    SenderIdentity, SenderKind,
};
use aikit_adapters::gateway_conversation_engine::{parse_slash, SlashParse};
use aikit_adapters::gateway_runtime::{
    GatewayBinding, GatewayCommand, GatewayConversationOperation, GatewayDiscovery, GatewayEcology,
    GatewayEcologySurface, GatewayIngressDecision, GatewayIngressResult, GatewayResponse,
    GatewayStreamEvent,
};
use aikit_core::resource::ResourceRef;
use aikit_store::home::AikitHome;

use crate::layout::Glyphs;
use crate::theme::Theme;

/// Events kept in the rendered history window. Older events stay in the
/// gateway's journal; the aperture discloses the truncation rather than
/// pretending the window is the whole conversation.
pub const HISTORY_WINDOW: usize = 200;
/// One live-wait budget. The event loop's idle tick polls this often; a
/// pushed event is therefore on screen within a tick of landing in the
/// journal, and a quiet subscription never stalls the loop by more than this.
const POLL_WAIT: Duration = Duration::from_millis(25);
/// One-shot reads (status, ecology, discover) must not stall the surface on a
/// gateway that has stopped answering.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Sending a composed message is allowed longer: it appends to the journal.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// Linear reconnect backoff: the first retry is quick, each later one waits a
/// step longer than the last. Never exponential — a restarting gateway comes
/// back on a human timescale, not an algorithmic one.
const FIRST_RETRY: Duration = Duration::from_millis(500);
const RETRY_STEP: Duration = Duration::from_secs(1);

/// Which gateway carrier the aperture addresses: the well-known same-host
/// Unix socket, the endpoint declared for a remote Workcell
/// (`AIKIT_GATEWAY_AT=<workcell-ref>`, resolved from `state/gateway-remotes.json`
/// exactly as `aikit gateway --at` resolves it), or nothing resolvable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationCarrier {
    #[cfg(unix)]
    UnixSocket(std::path::PathBuf),
    /// The authenticated WebSocket carrier of the gateway declared for the
    /// named Workcell. The surface's status pane still shows whichever
    /// `gateway_ref` answers, so an addressed remote never masquerades as
    /// this home's own gateway.
    WebSocket {
        bind: String,
        path: String,
        bearer_token: String,
        workcell_ref: String,
    },
    /// No socket carrier was resolved: either this platform has no default
    /// Unix carrier, or no AIKit home could be resolved to find one under.
    /// The aperture can still open and say so, which is more honest than
    /// pretending to poll.
    Absent,
}

impl ConversationCarrier {
    pub fn for_home(home: &AikitHome) -> Self {
        if let Some(carrier) = Self::for_env(home) {
            return carrier;
        }
        #[cfg(unix)]
        {
            Self::UnixSocket(home.gateway_socket())
        }
        #[cfg(not(unix))]
        {
            let _ = home;
            Self::Absent
        }
    }

    /// `AIKIT_GATEWAY_AT=<workcell-ref>`: address the gateway declared for
    /// that remote Workcell. An undeclared Workcell or an unusable token
    /// location resolves to `Absent`, and the aperture says the gateway is
    /// unreachable rather than silently falling back to the local one — a
    /// pointing error must never become a quiet conversation with the wrong
    /// gateway.
    fn for_env(home: &AikitHome) -> Option<Self> {
        let reference = std::env::var("AIKIT_GATEWAY_AT")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let bytes = std::fs::read(home.state().join("gateway-remotes.json")).ok()?;
        #[derive(serde::Deserialize)]
        struct Remotes {
            #[serde(default)]
            remotes: Vec<Remote>,
        }
        #[derive(serde::Deserialize)]
        struct Remote {
            workcell_ref: String,
            websocket_bind: String,
            #[serde(default = "default_ws_path")]
            websocket_path: String,
            token_location: String,
        }
        fn default_ws_path() -> String {
            "/".into()
        }
        let remotes: Remotes = serde_json::from_slice(&bytes).ok()?;
        let remote = remotes
            .remotes
            .into_iter()
            .find(|remote| remote.workcell_ref == reference)?;
        let token = resolve_token(&remote.token_location)?;
        Some(Self::WebSocket {
            bind: remote.websocket_bind,
            path: remote.websocket_path,
            bearer_token: token,
            workcell_ref: remote.workcell_ref,
        })
    }
}

/// Resolve a declared token location the way the CLI's `SecretLocation` does:
/// an owner-only `file:` path, or a declared secret ref through the resolver
/// suite. Material lives only as long as one request.
fn resolve_token(location: &str) -> Option<String> {
    if let Some(path) = location.trim().strip_prefix("file:") {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path).ok()?;
            if metadata.permissions().mode() & 0o077 != 0 {
                return None;
            }
        }
        return std::fs::read_to_string(path)
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty());
    }
    let secret_ref = aikit_core::secret_ref::SecretRef::parse(location).ok()?;
    use aikit_core::SecretResolver as _;
    aikit_adapters::secret_resolver::SuiteSecretResolver::default()
        .resolve(&secret_ref)
        .ok()
        .map(|secret| secret.expose().to_owned())
}

/// One bound conversation the gateway discloses: a connector conversation
/// routed to an AgentSession and its ActuationStream. The binding's refs are
/// the only identity this surface knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationEntry {
    pub binding_ref: ResourceRef,
    pub connector_ref: ResourceRef,
    pub platform: String,
    pub conversation_id: String,
    pub agent_session_ref: ResourceRef,
    pub ingress_default: GatewayIngressDecision,
    /// The stream this conversation writes to, when the session's ecology
    /// names exactly one. Otherwise it is resolved from the binding itself at
    /// open time.
    pub stream_ref: Option<ResourceRef>,
    pub last_sequence: u64,
}

/// One rendered line of an open conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationLine {
    pub role: LineRole,
    pub sequence: u64,
    pub text: String,
}

/// What kind of journal event a rendered line carries. Colour and glyph stay
/// with the renderer; this carries the distinction itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    /// An inbound human/attribution line.
    User,
    /// An assistant/reply line.
    Agent,
    /// An honest turn-failure record: the turn failed or was interrupted, and
    /// the journal says so instead of carrying a fabricated answer.
    Failure,
    /// A control event (tool call, permission, membership, harness noise).
    Control,
    /// A kind this surface does not render, disclosed rather than hidden.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkState {
    Live,
    Degraded { attempts: u32, next_retry: Instant },
}

/// An open conversation: one subscription, its bounded history window, and
/// the last sequence this surface has actually seen.
#[derive(Debug)]
struct OpenConversation {
    entry: ConversationEntry,
    stream_ref: ResourceRef,
    address: ConversationAddress,
    history: Vec<ConversationLine>,
    last_seen: u64,
    /// The journal holds older events than the window shows.
    older_history_exists: bool,
    link: LinkState,
    subscription: Option<GatewaySubscription>,
    /// Rows the operator has paged back from the live tail.
    scroll_back: usize,
}

/// How far the real seam let a composed message travel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositionOutcome {
    /// The gateway appended it to the stream; it will render like any other
    /// event, through the subscription or the next replay.
    Appended { sequence: u64 },
    /// The gateway wants this sender paired before it records anything.
    PairingRequired { sender: String },
    /// The gateway's ingress policy refused this sender.
    Denied { sender: String },
    /// The gateway could not be asked. The composed text is kept; nothing is
    /// sent again without the operator pressing Enter.
    Unreachable { reason: String },
    /// A canonical conversation-control operation was answered by the engine
    /// as a conversation response; the answer is rendered in the conversation.
    Answered { operation: String },
}

/// The whole aperture state: roster, health, the open conversation and the
/// compose lane. Controller-owned, like the Graph's layout cache: live
/// carrier state cannot live in the cloned, reducer-owned `TuiState`.
#[derive(Debug)]
pub struct ConversationSurface {
    open: bool,
    carrier: Option<ConversationCarrier>,
    gateway_ref: Option<ResourceRef>,
    authority: Option<String>,
    reachable: bool,
    entries: Vec<ConversationEntry>,
    connector_health: Vec<ConnectorHealth>,
    selected: usize,
    open_conversation: Option<OpenConversation>,
    compose: String,
    /// The last honest note: a refusal, a failure, a disclosure. Rendered in
    /// the pane, never folded into a success.
    note: Option<String>,
    compose_counter: u64,
}

impl Default for ConversationSurface {
    #[allow(clippy::derivable_impls)] // the field order below is the reading order of the pane
    fn default() -> Self {
        Self {
            open: false,
            carrier: None,
            gateway_ref: None,
            authority: None,
            reachable: false,
            entries: Vec::new(),
            connector_health: Vec::new(),
            selected: 0,
            open_conversation: None,
            compose: String::new(),
            note: None,
            compose_counter: 0,
        }
    }
}

impl ConversationSurface {
    /// Pin the carrier this aperture addresses. `None` means the well-known
    /// AIKit home socket — the same default the CLI resolves; a test pins an
    /// explicit endpoint. This is the crate's one carrier seam.
    pub(crate) fn set_carrier(&mut self, carrier: ConversationCarrier) {
        self.carrier = Some(carrier);
    }

    /// Resolve the carrier once, from the same AIKit home the CLI resolves
    /// its default gateway carrier from. Lazy: a palette run that never opens
    /// the aperture never pays for a home discovery, and a home that cannot
    /// be resolved is reported in the pane when it does open.
    fn carrier(&mut self) -> Option<&ConversationCarrier> {
        if self.carrier.is_none() {
            if let Ok(home) = AikitHome::discover() {
                self.carrier = Some(ConversationCarrier::for_home(&home));
            }
        }
        self.carrier.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether one conversation is open (compose lane and history visible),
    /// as opposed to the roster being the thing on screen.
    pub fn has_open_conversation(&self) -> bool {
        self.open_conversation.is_some()
    }

    pub fn entries(&self) -> &[ConversationEntry] {
        &self.entries
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn compose(&self) -> &str {
        &self.compose
    }

    pub fn selected(&self) -> Option<&ConversationEntry> {
        self.entries.get(self.selected)
    }

    /// Open the aperture: read the gateway's status and ecology. The pane
    /// opens even when the gateway is unreachable — it then says so, which is
    /// the state a person needs to see.
    pub fn open_aperture(&mut self) {
        self.open = true;
        self.refresh_roster();
    }

    pub fn close_aperture(&mut self) {
        self.open = false;
        self.open_conversation = None;
        self.compose.clear();
    }

    fn target(&mut self) -> Option<GatewayCarrierTarget> {
        match self.carrier()? {
            #[cfg(unix)]
            ConversationCarrier::UnixSocket(path) => {
                Some(GatewayCarrierTarget::UnixSocket(path.clone()))
            }
            ConversationCarrier::WebSocket {
                bind,
                path,
                bearer_token,
                ..
            } => Some(GatewayCarrierTarget::WebSocket {
                bind: bind.clone(),
                path: path.clone(),
                bearer_token: bearer_token.clone(),
            }),
            ConversationCarrier::Absent => None,
        }
    }

    fn refresh_roster(&mut self) {
        let Some(target) = self.target() else {
            self.reachable = false;
            self.note = Some(
                "no gateway socket was resolved for this home; start one with `aikit gateway serve`"
                    .into(),
            );
            return;
        };
        match gateway_command_within(&target, GatewayCommand::Status, None, READ_TIMEOUT) {
            Ok(GatewayResponse::Status { status }) => {
                self.reachable = true;
                self.gateway_ref = Some(status.gateway_ref.clone());
                self.connector_health = status.connector_health;
                self.note = None;
            }
            Ok(_) => {
                self.reachable = false;
                self.note = Some("the gateway answered status with something unexpected".into());
            }
            Err(error) => {
                self.reachable = false;
                self.connector_health.clear();
                self.note = Some(format!("gateway unreachable: {error}"));
            }
        }
        match gateway_command_within(&target, GatewayCommand::Ecology, None, READ_TIMEOUT) {
            Ok(GatewayResponse::Ecology { ecology }) => {
                self.authority = Some(ecology.authority.clone());
                self.entries = roster_from_ecology(&ecology);
                self.selected = self.selected.min(self.entries.len().saturating_sub(1));
            }
            Ok(_) => {
                self.note = Some("the gateway answered ecology with something unexpected".into());
            }
            Err(error) => {
                self.authority = None;
                self.entries.clear();
                self.note = Some(format!("gateway ecology unavailable: {error}"));
            }
        }
    }

    /// Move the roster cursor. No-op while a conversation is open.
    pub fn select_next(&mut self) {
        if self.open_conversation.is_none() && self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    pub fn select_previous(&mut self) {
        if self.open_conversation.is_none() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// Open the selected conversation: resolve its binding to the stream it
    /// writes to, subscribe from a window's worth back, and render the replay
    /// as the initial history.
    pub fn open_selected(&mut self) {
        let Some(target) = self.target() else {
            self.note = Some(
                "no gateway socket was resolved for this home; start one with `aikit gateway serve`"
                    .into(),
            );
            return;
        };
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return;
        };
        let binding =
            match gateway_command_within(&target, GatewayCommand::Discover, None, READ_TIMEOUT) {
                Ok(GatewayResponse::Discovery { discovery }) => {
                    find_binding(&discovery, &entry.binding_ref)
                }
                Ok(_) => None,
                Err(error) => {
                    self.note = Some(format!("could not read the gateway's bindings: {error}"));
                    None
                }
            };
        let Some(binding) = binding else {
            self.note = Some(format!(
                "binding {} is no longer on the gateway; reopening the roster",
                entry.binding_ref
            ));
            self.refresh_roster();
            return;
        };
        // The most recent window: subscribe from a window back (or the
        // journal's start) and let the replay fill forward.
        let after = entry.last_sequence.saturating_sub(HISTORY_WINDOW as u64);
        let stream_ref = binding.actuation_stream_ref.clone();
        let socket = unix_socket_path(&target);
        match gateway_subscribe(socket.as_path(), stream_ref.clone(), after, HISTORY_WINDOW) {
            Ok(subscription) => {
                let replay = subscription.replay();
                let history = replay
                    .events
                    .iter()
                    .map(conversation_line)
                    .collect::<Vec<_>>();
                let last_seen = replay.returned_through.max(replay.after_sequence);
                let older_history_exists = replay.has_more;
                self.open_conversation = Some(OpenConversation {
                    older_history_exists,
                    entry,
                    stream_ref,
                    address: binding.address.clone(),
                    history,
                    last_seen,
                    link: LinkState::Live,
                    subscription: Some(subscription),
                    scroll_back: 0,
                });
                self.compose.clear();
                self.note = None;
            }
            Err(error) => {
                self.note = Some(format!("could not subscribe to the conversation: {error}"));
            }
        }
    }

    /// Back: from an open conversation to the roster, from the roster close
    /// the aperture.
    pub fn back(&mut self) {
        if self.open_conversation.take().is_some() {
            self.compose.clear();
            return;
        }
        self.close_aperture();
    }

    /// Page back/forward through a window-fitted history of the open
    /// conversation.
    pub fn scroll_up(&mut self) {
        if let Some(open) = &mut self.open_conversation {
            open.scroll_back = open.scroll_back.saturating_add(1);
        }
    }

    pub fn scroll_down(&mut self) {
        if let Some(open) = &mut self.open_conversation {
            open.scroll_back = open.scroll_back.saturating_sub(1);
        }
    }

    pub fn compose_push(&mut self, character: char) {
        if self.open_conversation.is_some() {
            self.compose.push(character);
        }
    }

    pub fn compose_pop(&mut self) {
        if self.open_conversation.is_some() {
            self.compose.pop();
        }
    }

    /// Send the composed text into the same stream the aperture is watching,
    /// through the gateway's own ingest path — the same one connectors use,
    /// under the same ingress policy. The gateway's answer, whatever it is,
    /// becomes the note. A send that could not be asked keeps the text: the
    /// operator, not a reconnect, decides whether to send it.
    pub fn submit_compose(&mut self) -> Option<CompositionOutcome> {
        let Some(open) = &self.open_conversation else {
            return None;
        };
        let (connector_ref, address, stream_ref) = (
            open.entry.connector_ref.clone(),
            open.address.clone(),
            open.stream_ref.clone(),
        );
        if self.compose.trim().is_empty() {
            return None;
        }
        let text = self.compose.clone();
        let Some(target) = self.target() else {
            return Some(CompositionOutcome::Unreachable {
                reason: "no Unix socket carrier on this platform".into(),
            });
        };
        // A slash-composed text that parses to a canonical operation travels
        // as the landed Conversation command, so the engine's answer comes
        // back as a conversation response and renders in the pane. Anything
        // else rides the gateway's own ingest path exactly as before.
        if let SlashParse::Operation(operation) = parse_slash(&text) {
            let open = self.open_conversation.as_ref()?;
            let binding_ref = open.entry.binding_ref.clone();
            return self.submit_operation(&target, binding_ref, operation);
        }
        self.compose_counter += 1;
        let event_ref = ResourceRef::parse(format!(
            "aikit-tui/{stream_ref}/compose-{}",
            self.compose_counter
        ));
        let event_ref = match event_ref {
            Ok(event_ref) => event_ref,
            Err(error) => {
                return Some(CompositionOutcome::Unreachable {
                    reason: format!("could not name the event: {error}"),
                })
            }
        };
        let inbound = InboundEvent {
            event_ref,
            connector_ref,
            address,
            sender: tui_sender(),
            kind: InboundEventKind::Message,
            custom_kind: None,
            native_event_id: None,
            native_message_id: None,
            reply_to_native_message_id: None,
            text: Some(text),
            media: Vec::new(),
            observed_at: None,
            native: Default::default(),
            provenance: vec!["aikit-tui conversation aperture".into()],
        };
        let answer = gateway_command_within(
            &target,
            GatewayCommand::Ingest { event: inbound },
            None,
            SEND_TIMEOUT,
        );
        let outcome = match answer {
            Ok(GatewayResponse::Ingress { result }) => match result {
                GatewayIngressResult::Appended { event, .. } => CompositionOutcome::Appended {
                    sequence: event.sequence,
                },
                GatewayIngressResult::PairingRequired { sender, .. } => {
                    CompositionOutcome::PairingRequired {
                        sender: sender.native_sender_id,
                    }
                }
                GatewayIngressResult::Denied { sender, .. } => CompositionOutcome::Denied {
                    sender: sender.native_sender_id,
                },
            },
            Ok(_) => CompositionOutcome::Unreachable {
                reason: "the gateway answered the message with something unexpected".into(),
            },
            Err(error) => CompositionOutcome::Unreachable {
                reason: error.to_string(),
            },
        };
        match &outcome {
            CompositionOutcome::Appended { .. } => {
                self.compose.clear();
                self.note = None;
            }
            CompositionOutcome::Answered { .. } => {
                // The engine's answer is already rendered in the conversation;
                // there is no note to carry.
            }
            CompositionOutcome::PairingRequired { sender } => {
                self.note = Some(format!(
                    "the gateway requires pairing for sender {sender}; nothing was appended"
                ));
            }
            CompositionOutcome::Denied { sender } => {
                self.note = Some(format!(
                    "the gateway's ingress policy denied sender {sender}; nothing was appended"
                ));
            }
            CompositionOutcome::Unreachable { reason } => {
                self.note = Some(format!(
                    "the message was not sent ({reason}); your text is kept - press Enter to try again"
                ));
            }
        }
        Some(outcome)
    }

    /// Send one canonical conversation-control operation and render the
    /// engine's answer into the open conversation. The answer is the engine's
    /// own result document, received as a conversation response; it is
    /// rendered, never stored as journal history — on reopen the pane rebuilds
    /// from the journal, which holds what the stream actually carried. An
    /// operation the gateway could not answer keeps the composed text and says
    /// so, exactly as a refused message does.
    fn submit_operation(
        &mut self,
        target: &GatewayCarrierTarget,
        binding_ref: ResourceRef,
        operation: GatewayConversationOperation,
    ) -> Option<CompositionOutcome> {
        let answer = gateway_command_within(
            target,
            GatewayCommand::Conversation {
                binding_ref,
                operation,
            },
            None,
            SEND_TIMEOUT,
        );
        let outcome = match answer {
            Ok(GatewayResponse::Conversation {
                operation, result, ..
            }) => {
                if let Some(open) = &mut self.open_conversation {
                    for text in control_answer_lines(&operation, &result) {
                        open.history.push(ConversationLine {
                            role: LineRole::Control,
                            // Not a journal event: the answer carries no
                            // sequence and must never move the reconnect
                            // cursor.
                            sequence: 0,
                            text,
                        });
                    }
                    while open.history.len() > HISTORY_WINDOW {
                        open.history.remove(0);
                        open.older_history_exists = true;
                    }
                }
                self.compose.clear();
                self.note = None;
                CompositionOutcome::Answered {
                    operation: operation_name(&operation).to_string(),
                }
            }
            Ok(_) => CompositionOutcome::Unreachable {
                reason: "the gateway answered the operation with something unexpected".into(),
            },
            Err(error) => CompositionOutcome::Unreachable {
                reason: error.to_string(),
            },
        };
        if let CompositionOutcome::Unreachable { reason } = &outcome {
            self.note = Some(format!("the operation was not answered ({reason}); your text is kept - press Enter to try again"));
        }
        Some(outcome)
    }

    /// One poll of the open conversation's subscription. Infallible: a
    /// gateway that has gone away degrades the link and is reported in the
    /// pane, never allowed to break the surface.
    pub fn poll(&mut self) {
        let Some(target) = self.target() else {
            return;
        };
        let socket = unix_socket_path(&target);
        let Some(open) = &mut self.open_conversation else {
            return;
        };
        // Read the link state first, then act on it: the reconnect itself
        // rewrites the link, so the decision and the mutation cannot share a
        // borrow.
        let retry = match open.link {
            LinkState::Live => None,
            LinkState::Degraded {
                attempts,
                next_retry,
            } => Some((attempts, next_retry)),
        };
        let mut recovered = false;
        match retry {
            None => {
                let answer = open
                    .subscription
                    .as_mut()
                    .map(|subscription| subscription.next_event(POLL_WAIT));
                match answer {
                    Some(Ok(GatewaySubscriptionRead::Event(event))) => {
                        append_event(open, event);
                    }
                    Some(Ok(GatewaySubscriptionRead::Idle)) => {}
                    // A protocol failure is a dead carrier like any other:
                    // degrade and retry from the cursor.
                    Some(Err(_)) | Some(Ok(GatewaySubscriptionRead::Closed)) | None => {
                        degrade(open);
                    }
                }
            }
            Some((mut attempts, next_retry)) => {
                if Instant::now() < next_retry {
                    return;
                }
                // Re-subscribe from the last sequence this surface has seen:
                // the replay then covers exactly the gap, so nothing is
                // missed and nothing is repeated. Nothing the operator typed
                // is re-sent — the compose lane is untouched by reconnecting.
                match gateway_subscribe(
                    socket.as_path(),
                    open.stream_ref.clone(),
                    open.last_seen,
                    HISTORY_WINDOW,
                ) {
                    Ok(subscription) => {
                        let replay = subscription.replay().clone();
                        let replayed_through = replay.returned_through;
                        for event in replay.events {
                            append_event(open, event);
                        }
                        open.last_seen = open.last_seen.max(replayed_through);
                        open.subscription = Some(subscription);
                        open.link = LinkState::Live;
                        recovered = true;
                    }
                    Err(_) => {
                        // Linear backoff: each failed attempt waits one step
                        // longer than the last.
                        attempts += 1;
                        open.link = LinkState::Degraded {
                            attempts,
                            next_retry: Instant::now() + FIRST_RETRY + RETRY_STEP * attempts,
                        };
                    }
                }
            }
        }
        if recovered {
            self.note = None;
        }
    }

    /// Render the pane's lines. `visible_rows` is the content height the pane
    /// offers; the history is tail-fitted to it, honouring the operator's
    /// scroll-back.
    pub fn pane(&self, glyphs: Glyphs, visible_rows: usize) -> (String, Vec<Line<'static>>) {
        let theme = Theme::new();
        let sep = glyphs.separator();
        match &self.open_conversation {
            None => {
                let mut lines = Vec::new();
                let gateway = self
                    .gateway_ref
                    .as_ref()
                    .map(|gateway_ref| gateway_ref.to_string())
                    .unwrap_or_else(|| "unreachable".into());
                let addressed = std::env::var("AIKIT_GATEWAY_AT")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map(|workcell| format!("gateway {gateway} (via {workcell})"))
                    .unwrap_or_else(|| format!("gateway {gateway}"));
                lines.push(Line::from(Span::styled(addressed, theme.heading())));
                if let Some(authority) = &self.authority {
                    lines.push(Line::from(Span::styled(authority.clone(), theme.dim())));
                }
                lines.push(Line::from(Span::styled(
                    if self.reachable {
                        "gateway reachable"
                    } else {
                        "gateway unreachable"
                    },
                    if self.reachable {
                        theme.active()
                    } else {
                        theme.error()
                    },
                )));
                if let Some(note) = &self.note {
                    lines.push(Line::from(Span::styled(note.clone(), theme.unavailable())));
                }
                lines.push(Line::from(String::new()));
                if self.entries.is_empty() {
                    lines.push(Line::from(Span::styled(
                        if self.reachable {
                            "no bound conversations on this gateway"
                        } else {
                            "the gateway's ecology could not be read"
                        },
                        theme.dim(),
                    )));
                }
                for (index, entry) in self.entries.iter().enumerate() {
                    let cursor = if index == self.selected {
                        glyphs.list_cursor()
                    } else {
                        " "
                    };
                    let style = if index == self.selected {
                        theme.selected()
                    } else {
                        theme.base()
                    };
                    lines.push(Line::from(Span::styled(
                        format!(
                            "{cursor} {}/{}  connector {}  seq {}",
                            entry.platform,
                            entry.conversation_id,
                            entry.connector_ref,
                            entry.last_sequence
                        ),
                        style,
                    )));
                }
                lines.push(Line::from(String::new()));
                lines.push(Line::from(Span::styled(
                    format!("Enter open {sep} Esc close"),
                    theme.dim(),
                )));
                (" Conversation ".into(), lines)
            }
            Some(open) => {
                let title = format!(
                    " Conversation {sep} {}/{} ",
                    open.entry.platform, open.entry.conversation_id
                );
                let mut lines = Vec::new();
                if open.older_history_exists {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "older events remain in the journal before sequence {}",
                            open.history
                                .first()
                                .map(|line| line.sequence)
                                .unwrap_or(open.last_seen)
                        ),
                        theme.dim(),
                    )));
                }
                for line in &open.history {
                    let style = match line.role {
                        LineRole::User => theme.accent(),
                        LineRole::Agent => theme.base(),
                        LineRole::Failure => theme.error(),
                        LineRole::Control => theme.dim(),
                        LineRole::Unknown => theme.unavailable(),
                    };
                    lines.push(Line::from(Span::styled(line.text.clone(), style)));
                }
                lines.push(Line::from(String::new()));
                if let Some(note) = &self.note {
                    lines.push(Line::from(Span::styled(note.clone(), theme.unavailable())));
                }
                lines.push(status_strip(open, self, &theme, sep));
                lines.push(Line::from(Span::styled(
                    format!("compose> {}{}", self.compose, glyphs.list_cursor()),
                    theme.base(),
                )));
                lines.push(Line::from(Span::styled(
                    format!("Enter send {sep} Up/Down history {sep} Esc back to conversations"),
                    theme.dim(),
                )));
                let end = lines.len().saturating_sub(open.scroll_back);
                let start = end.saturating_sub(visible_rows.max(1));
                (title, lines[start..end].to_vec())
            }
        }
    }
}

fn unix_socket_path(target: &GatewayCarrierTarget) -> std::path::PathBuf {
    match target {
        #[cfg(unix)]
        GatewayCarrierTarget::UnixSocket(path) => path.clone(),
        GatewayCarrierTarget::WebSocket { .. } => std::path::PathBuf::new(),
    }
}

fn tui_sender() -> SenderIdentity {
    SenderIdentity {
        native_sender_id: "aikit-tui-operator".into(),
        kind: SenderKind::Human,
        display_name: Some("terminal operator".into()),
        metadata: Default::default(),
    }
}

fn roster_from_ecology(ecology: &GatewayEcology) -> Vec<ConversationEntry> {
    let mut entries = Vec::new();
    for agency in &ecology.agencies {
        for session in &agency.sessions {
            for surface in &session.surfaces {
                entries.push(entry_from_surface(
                    session.agent_session_ref.clone(),
                    session.streams.as_slice(),
                    surface,
                ));
            }
        }
    }
    entries
}

fn entry_from_surface(
    agent_session_ref: ResourceRef,
    streams: &[aikit_adapters::gateway_runtime::GatewayEcologyStream],
    surface: &GatewayEcologySurface,
) -> ConversationEntry {
    // A session with exactly one stream resolves the surface's stream here; a
    // session with several leaves it to the binding itself at open time,
    // because guessing a route is how a surface ends up watching the wrong
    // conversation.
    let stream = if streams.len() == 1 {
        streams.first()
    } else {
        None
    };
    ConversationEntry {
        binding_ref: surface.binding_ref.clone(),
        connector_ref: surface.connector_ref.clone(),
        platform: surface.platform.clone(),
        conversation_id: surface.address.conversation_id.clone(),
        agent_session_ref,
        ingress_default: surface.ingress_default,
        stream_ref: stream.map(|stream| stream.stream_ref.clone()),
        last_sequence: stream.map(|stream| stream.last_sequence).unwrap_or(0),
    }
}

fn find_binding(discovery: &GatewayDiscovery, binding_ref: &ResourceRef) -> Option<GatewayBinding> {
    discovery
        .bindings
        .iter()
        .find(|binding| &binding.binding_ref == binding_ref)
        .cloned()
}

/// Render one journal event as a conversation line, from what the event
/// actually carries. An unknown kind is disclosed, never hidden.
fn conversation_line(event: &GatewayStreamEvent) -> ConversationLine {
    let kind = event
        .event
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let content = event
        .event
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let line = |role: LineRole, text: String| ConversationLine {
        role,
        sequence: event.sequence,
        text,
    };
    match kind {
        "human-message" => {
            let sender = event
                .event
                .pointer("/metadata/native_sender_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown sender");
            let text = if content.is_empty() {
                format!("{sender} sent an empty message")
            } else {
                format!("{sender}: {content}")
            };
            line(LineRole::User, text)
        }
        "model-delta" => {
            let text = if content.is_empty() {
                "agent sent an empty reply".to_string()
            } else {
                format!("agent: {content}")
            };
            line(LineRole::Agent, text)
        }
        "agent-message" => {
            // The engine's journaled agent turn: an honest reply attributed to
            // the binding's agent session, rendered as a first-class agent
            // line — distinct from a human line, never disclosure filler.
            let text = if content.is_empty() {
                "agent sent an empty reply".to_string()
            } else {
                format!("agent: {content}")
            };
            line(LineRole::Agent, text)
        }
        "harness-event" | "tool-request" | "tool-result" | "permission" | "cancellation" => {
            let detail = event
                .event
                .pointer("/metadata/native/event")
                .and_then(Value::as_str)
                .unwrap_or(kind);
            line(LineRole::Control, format!("- {kind}: {detail}"))
        }
        "custom" => {
            let custom = event
                .event
                .get("custom_kind")
                .and_then(Value::as_str)
                .unwrap_or("unlabelled");
            if custom == "gateway-agent/turn-failure" {
                // A failed or interrupted turn is journaled honestly; the
                // line says so in the failure voice, never as agent chatter.
                line(
                    LineRole::Failure,
                    format!("! {}", turn_failure_text(content, &event.event)),
                )
            } else {
                line(LineRole::Control, format!("- {custom}"))
            }
        }
        other => line(
            LineRole::Unknown,
            format!("? unhandled event kind \"{other}\" - shown as the journal stores it"),
        ),
    }
}

/// The honest failure sentence a turn-failure record carries. The engine
/// journals the sentence in `content`; the structured `metadata.failure` is
/// the fallback when the content is empty.
fn turn_failure_text(content: &str, event: &Value) -> String {
    if !content.is_empty() {
        return content.to_string();
    }
    let failure = event.pointer("/metadata/failure");
    match failure
        .and_then(|failure| failure.get("kind"))
        .and_then(Value::as_str)
    {
        Some("failed") => format!(
            "the turn failed: {}",
            failure
                .and_then(|failure| failure.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or("no reason recorded"),
        ),
        Some("interrupted") => {
            let detail = failure
                .and_then(|failure| failure.get("detail"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if detail.is_empty() {
                "the turn was interrupted before it answered".to_string()
            } else {
                format!("the turn was interrupted before it answered ({detail})")
            }
        }
        _ => "the turn produced no answer".to_string(),
    }
}

fn operation_name(operation: &GatewayConversationOperation) -> &'static str {
    match operation {
        GatewayConversationOperation::Status => "status",
        GatewayConversationOperation::Stop => "stop",
        GatewayConversationOperation::New => "new",
        GatewayConversationOperation::Sessions => "sessions",
        GatewayConversationOperation::Restart => "restart",
        GatewayConversationOperation::PauseConnector { .. } => "pause",
        GatewayConversationOperation::ResumeConnector { .. } => "resume",
        GatewayConversationOperation::Model { .. } => "model",
        GatewayConversationOperation::Harness => "harness",
        GatewayConversationOperation::Skills => "skills",
        GatewayConversationOperation::AskPosition { .. } => "ask",
        GatewayConversationOperation::Upgrade { .. } => "upgrade",
        GatewayConversationOperation::Announce { .. } => "announce",
    }
}

/// Render one engine answer — the `GatewayResponse::Conversation` result
/// document — as readable control lines, from the shapes the engine answers
/// with today. The model selector, the harness disclosure, the skill surface
/// and the conversation status are shaped; any other result renders field by
/// field, never hidden.
fn control_answer_lines(operation: &GatewayConversationOperation, result: &Value) -> Vec<String> {
    let head = |text: String| format!("- {text}");
    let named = |field: &str| {
        result
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string()
    };
    match operation {
        GatewayConversationOperation::Status => {
            let stream_at = match (
                result.get("stream_last_sequence").and_then(Value::as_u64),
                result.get("stream_event_count").and_then(Value::as_u64),
            ) {
                (Some(last), Some(count)) => format!("stream at {last} ({count} events)"),
                _ => "stream unread".to_string(),
            };
            let turn = match result.get("turn_in_flight").and_then(Value::as_bool) {
                Some(true) => "turn in flight: yes",
                Some(false) => "turn in flight: no",
                None => "turn in flight: unknown",
            };
            let backing = result
                .get("agent_backing")
                .and_then(Value::as_str)
                .unwrap_or("none");
            let connector = result
                .pointer("/connector_health/state")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            vec![
                head(format!(
                    "status: binding {}; {stream_at}; {turn}",
                    named("binding_ref"),
                )),
                head(format!("status: backing {backing}; connector {connector}")),
            ]
        }
        GatewayConversationOperation::Model { model } => {
            let harness = named("harness");
            match model {
                Some(id) => {
                    let previous = result
                        .pointer("/receipt/previous")
                        .and_then(Value::as_str)
                        .unwrap_or("unrecorded");
                    vec![head(format!(
                        "model: {id} selected on harness {harness} (previous: {previous})"
                    ))]
                }
                None => {
                    let controls = result.get("controls").unwrap_or(&Value::Null);
                    let available: Vec<&str> = controls
                        .get("available")
                        .and_then(Value::as_array)
                        .map(|models| models.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    if !available.is_empty() {
                        let mut lines = vec![head(format!(
                            "model: harness {harness} offers {}",
                            available.join(", "),
                        ))];
                        if let Some(current) = controls.get("current").and_then(Value::as_str) {
                            lines.push(head(format!("model: current selection {current}")));
                        }
                        lines
                    } else if controls.get("model_selection") == Some(&json!(true)) {
                        vec![head(format!(
                            "model: harness {harness} exposes a native model selector; name the \
                             provider model with /model <name>"
                        ))]
                    } else {
                        let reason = controls
                            .get("reason")
                            .and_then(Value::as_str)
                            .filter(|reason| !reason.is_empty())
                            .unwrap_or("the harness discloses no selector");
                        vec![head(format!(
                            "model: harness {harness} exposes no model selector ({reason})"
                        ))]
                    }
                }
            }
        }
        GatewayConversationOperation::Harness => {
            let backing = result
                .get("current")
                .and_then(Value::as_str)
                .unwrap_or("none");
            let available: Vec<&str> = result
                .get("available")
                .and_then(Value::as_array)
                .map(|backings| {
                    backings
                        .iter()
                        .filter_map(|backing| backing.get("id").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default();
            let mut lines = vec![head(format!(
                "harness: backing {backing}; available backings: {}",
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                },
            ))];
            if let Some(command) = result.get("switch_command").and_then(Value::as_str) {
                lines.push(head(format!(
                    "harness switch (disclosed, not performed): {command}"
                )));
            }
            lines
        }
        GatewayConversationOperation::Skills => {
            let harness = named("harness");
            let skills = result.get("skills").and_then(Value::as_array);
            let count = skills.map(Vec::len).unwrap_or(0);
            let mut lines = vec![head(format!(
                "skills: {count} available on harness {harness}",
            ))];
            if let Some(skills) = skills {
                for skill in skills {
                    let name = skill
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unnamed");
                    let summary = skill
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    lines.push(head(if summary.is_empty() {
                        format!("skill {name}")
                    } else {
                        format!("skill {name}: {summary}")
                    }));
                }
            }
            lines.push(head(
                "skills: name the skill in your message - the gateway does not execute skills"
                    .to_string(),
            ));
            lines
        }
        other => {
            let name = operation_name(other);
            match result.as_object() {
                Some(fields) if !fields.is_empty() => fields
                    .iter()
                    .map(|(field, value)| {
                        head(format!("{name}/{}: {}", field, render_value(value)))
                    })
                    .collect(),
                _ => vec![head(format!("{name}: {result}"))],
            }
        }
    }
}

/// One field value in a fallback answer line: scalars read plainly, anything
/// structured renders as the compact JSON the engine actually answered.
fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Null => "none".to_string(),
        other => other.to_string(),
    }
}

fn append_event(open: &mut OpenConversation, event: GatewayStreamEvent) {
    // The cursor-bounded replay makes duplicates impossible; this guard keeps
    // the guarantee local if a carrier ever misbehaves.
    if event.sequence <= open.last_seen {
        return;
    }
    open.last_seen = event.sequence;
    open.history.push(conversation_line(&event));
    while open.history.len() > HISTORY_WINDOW {
        open.history.remove(0);
        open.older_history_exists = true;
    }
}

fn degrade(open: &mut OpenConversation) {
    open.subscription = None;
    open.link = LinkState::Degraded {
        attempts: 0,
        next_retry: Instant::now() + FIRST_RETRY,
    };
}

fn status_strip<'a>(
    open: &OpenConversation,
    surface: &ConversationSurface,
    theme: &Theme,
    sep: &str,
) -> Line<'a> {
    let link = match &open.link {
        LinkState::Live => "gateway live".to_string(),
        LinkState::Degraded { attempts, .. } => {
            format!("gateway degraded - reconnecting (attempt {})", attempts + 1)
        }
    };
    let health = surface
        .connector_health
        .iter()
        .find(|health| health.connector_ref == open.entry.connector_ref);
    let connector = match health {
        Some(health) => {
            let detail = health
                .detail
                .as_deref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default();
            format!(
                "connector {} {}{}",
                health.connector_ref,
                connection_state_word(health.state),
                detail
            )
        }
        None => format!("connector {} no health reading", open.entry.connector_ref),
    };
    Line::from(Span::styled(
        format!("{link} {sep} {connector} {sep} seq {}", open.last_seen),
        if matches!(open.link, LinkState::Live) && surface.reachable {
            theme.active()
        } else {
            theme.unavailable()
        },
    ))
}

fn connection_state_word(state: ConnectorConnectionState) -> &'static str {
    match state {
        ConnectorConnectionState::Disconnected => "disconnected",
        ConnectorConnectionState::Connecting => "connecting",
        ConnectorConnectionState::Connected => "connected",
        ConnectorConnectionState::Degraded => "degraded",
        ConnectorConnectionState::Reconnecting => "reconnecting",
        ConnectorConnectionState::Unavailable => "unavailable",
        ConnectorConnectionState::Closed => "closed",
    }
}
