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
use serde_json::Value;

use aikit_adapters::gateway_client::{
    gateway_command_within, gateway_subscribe, GatewayCarrierTarget, GatewaySubscription,
    GatewaySubscriptionRead,
};
use aikit_adapters::gateway_connector::{
    ConnectorConnectionState, ConnectorHealth, ConversationAddress, InboundEvent,
    InboundEventKind, SenderIdentity, SenderKind,
};
use aikit_adapters::gateway_runtime::{
    GatewayBinding, GatewayCommand, GatewayDiscovery, GatewayEcology, GatewayEcologySurface,
    GatewayIngressDecision, GatewayIngressResult, GatewayResponse, GatewayStreamEvent,
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
/// Unix socket, resolved exactly as the CLI resolves its default query
/// carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationCarrier {
    #[cfg(unix)]
    UnixSocket(std::path::PathBuf),
    /// No socket carrier was resolved: either this platform has no default
    /// Unix carrier, or no AIKit home could be resolved to find one under.
    /// The aperture can still open and say so, which is more honest than
    /// pretending to poll.
    Absent,
}

impl ConversationCarrier {
    pub fn for_home(home: &AikitHome) -> Self {
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
    /// A control event (tool call, permission, membership, harness noise).
    Control,
    /// A kind this surface does not render, disclosed rather than hidden.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkState {
    Live,
    Degraded {
        attempts: u32,
        next_retry: Instant,
    },
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
        let binding = match gateway_command_within(&target, GatewayCommand::Discover, None, READ_TIMEOUT)
        {
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
        let (connector_ref, address, stream_ref) = match &self.open_conversation {
            Some(open) => (
                open.entry.connector_ref.clone(),
                open.address.clone(),
                open.stream_ref.clone(),
            ),
            None => return None,
        };
        if self.compose.trim().is_empty() {
            return None;
        }
        let text = self.compose.clone();
        let Some(target) = self.target() else {
            return Some(CompositionOutcome::Unreachable {
                reason: "no Unix socket carrier on this platform".into(),
            });
        };
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
            LinkState::Degraded { attempts, next_retry } => Some((attempts, next_retry)),
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
                lines.push(Line::from(Span::styled(
                    format!("gateway {gateway}"),
                    theme.heading(),
                )));
                if let Some(authority) = &self.authority {
                    lines.push(Line::from(Span::styled(
                        authority.clone(),
                        theme.dim(),
                    )));
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
                            entry.platform, entry.conversation_id, entry.connector_ref,
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
                    format!(
                        "Enter send {sep} Up/Down history {sep} Esc back to conversations"
                    ),
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
            line(LineRole::Control, format!("- {custom}"))
        }
        other => line(
            LineRole::Unknown,
            format!("? unhandled event kind \"{other}\" - shown as the journal stores it"),
        ),
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
        None => format!(
            "connector {} no health reading",
            open.entry.connector_ref
        ),
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
