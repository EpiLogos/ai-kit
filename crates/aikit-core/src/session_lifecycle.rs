//! Durable session lifecycle events: start/end, thinking, cancellation and
//! permission requests, exposed as a typed, schema-stamped read model.
//!
//! ## One owner, one history
//!
//! `aikit-store` remains the sole persistence authority. This module owns only
//! the typed facts: what a lifecycle event is, which identities it must carry,
//! and how a read model is *derived* from recorded events. Nothing here does
//! I/O and nothing here fabricates: an unknown session is an explicit error
//! (`session_lifecycle.unknown_session`), never an empty history returned as
//! if nothing had happened.
//!
//! ## The permission join
//!
//! Every permission event carries two stable identities, verbatim:
//!
//! * [`SessionActivityId`] — the activity the request belongs to. Correlators
//!   on every side (Actuation, an O:I kernel, a UI) quote the same string;
//!   it is never re-derived, so all four sides agree by construction.
//! * [`PermissionRequestId`] — the join key across the issued / granted /
//!   refused triple. Granted and refused events recover the activity identity
//!   from the original request event, so the whole triple shares one activity.
//!
//! Both identities are plain strings on disk; history reopen cannot change
//! them, and the store tests prove it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::{EventId, PermissionRequestId, SessionActivityId, SessionId};
use crate::{AikitError, Result};

pub const SESSION_LIFECYCLE_VERSION: &str = "aikit.session-lifecycle/v1";

// ---------------------------------------------------------------------------
// Event kinds
// ---------------------------------------------------------------------------

/// What happened in one session's lifecycle. The vocabulary is closed so a
/// query for permission events cannot silently miss a spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionLifecycleEventKind {
    /// The session's lifecycle began. Always the first recorded event.
    SessionStarted,
    /// The session's lifecycle ended. Nothing may be recorded after it.
    SessionEnded,
    /// An in-flight reasoning state marker: the actor is thinking, in the
    /// labelled state (e.g. `reasoning`, `awaiting-tool`).
    Thinking,
    /// The session was cancelled, with its reason and origin recorded.
    Cancelled,
    /// A tool permission request was issued.
    PermissionRequested,
    /// A tool permission request was granted.
    PermissionGranted,
    /// A tool permission request was refused.
    PermissionRefused,
}

impl SessionLifecycleEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStarted => "session-started",
            Self::SessionEnded => "session-ended",
            Self::Thinking => "thinking",
            Self::Cancelled => "cancelled",
            Self::PermissionRequested => "permission-requested",
            Self::PermissionGranted => "permission-granted",
            Self::PermissionRefused => "permission-refused",
        }
    }
}

// ---------------------------------------------------------------------------
// The event
// ---------------------------------------------------------------------------

/// One recorded fact about a session's lifecycle.
///
/// The identity fields (`session`, `activity`, and `permission_request` on
/// permission events) are the correlation contract: downstream consumers
/// quote them verbatim, and they are the only join keys the history offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLifecycleEvent {
    pub event_id: EventId,
    pub session: SessionId,
    /// The stable activity identity this event belongs to.
    pub activity: SessionActivityId,
    pub recorded_at_unix_ms: u128,
    pub kind: SessionLifecycleEventKind,
    /// Who or what originated the event: `operator`, `agent:<name>`, or a
    /// correlator's own name. Recorded, never inferred.
    pub origin: String,
    /// Cancellation or refusal reason. Present exactly on `cancelled` and
    /// `permission-refused` events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The tool a permission event names. Present exactly on permission events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The stable request identity. Present on all three permission events;
    /// it is what makes issued/granted/refused one joinable triple.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_request: Option<PermissionRequestId>,
    /// The in-flight reasoning state label. Present exactly on `thinking`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl SessionLifecycleEvent {
    /// A fully-formed event as recorded by the owner operation. The store
    /// validates it against the existing history before persisting.
    pub fn new(
        session: SessionId,
        activity: SessionActivityId,
        kind: SessionLifecycleEventKind,
        origin: impl Into<String>,
    ) -> Self {
        Self {
            event_id: EventId::generate(),
            session,
            activity,
            kind,
            origin: origin.into(),
            recorded_at_unix_ms: now_unix_ms(),
            reason: None,
            tool: None,
            permission_request: None,
            state: None,
        }
    }

    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    #[must_use]
    pub fn for_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    #[must_use]
    pub fn for_permission_request(mut self, request: PermissionRequestId) -> Self {
        self.permission_request = Some(request);
        self
    }

    #[must_use]
    pub fn in_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }
}

/// What the owner operation was asked to record. Kept separate from
/// [`SessionLifecycleEvent`] so the store — not the caller — owns identity
/// minting and timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLifecycleRecord {
    Start,
    End,
    Thinking {
        state: String,
    },
    Cancel {
        reason: String,
    },
    /// A tool permission request. The activity identity names the activity
    /// the request belongs to; `request` lets a correlator supply its own
    /// stable request identity, otherwise the owner mints one.
    PermissionRequest {
        tool: String,
        activity: SessionActivityId,
        request: Option<PermissionRequestId>,
    },
    /// Grant a previously issued request. The activity identity is recovered
    /// from the original request event, keeping the triple on one activity.
    PermissionGrant {
        request: PermissionRequestId,
    },
    /// Refuse a previously issued request, with a reason.
    PermissionRefuse {
        request: PermissionRequestId,
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate one candidate event against the already-recorded history before
/// any write. This is the whole recording law; the store adds only I/O.
///
/// Returns explicit errors, never silent repair:
/// * `session_lifecycle.wrong_session` — the event names a different session.
/// * `session_lifecycle.start_required` — anything recorded before a
///   `session-started` event.
/// * `session_lifecycle.already_ended` — anything recorded after
///   `session-ended`.
/// * `session_lifecycle.already_cancelled` — anything recorded after
///   `cancelled`; cancellation is terminal.
/// * `session_lifecycle.malformed_event` — a kind recorded without the fields
///   it is defined to carry (e.g. a grant with no request identity).
/// * `session_lifecycle.unknown_permission_request` — grant/refuse for a
///   request this history never issued.
/// * `session_lifecycle.permission_request_closed` — grant/refuse for a
///   request that was already answered.
/// * `session_lifecycle.permission_request_pending` — `session-ended` while a
///   permission request is still open.
pub fn validate_next_event(
    session: &SessionId,
    existing: &[SessionLifecycleEvent],
    next: &SessionLifecycleEvent,
) -> Result<()> {
    if &next.session != session {
        return Err(AikitError::new(
            "session_lifecycle.wrong_session",
            format!(
                "event for {} cannot be recorded in the history of {}",
                next.session, session
            ),
        ));
    }
    validate_event_shape(next)?;
    match existing.last().map(|event| event.kind) {
        None if next.kind == SessionLifecycleEventKind::SessionStarted => return Ok(()),
        None => {
            return Err(AikitError::new(
                "session_lifecycle.start_required",
                format!(
                    "the first lifecycle event of {} must be session-started",
                    session
                ),
            ));
        }
        Some(SessionLifecycleEventKind::SessionEnded) => {
            return Err(AikitError::new(
                "session_lifecycle.already_ended",
                format!("{} already ended; nothing more may be recorded", session),
            ));
        }
        Some(SessionLifecycleEventKind::Cancelled) => {
            return Err(AikitError::new(
                "session_lifecycle.already_cancelled",
                format!("{} was cancelled; nothing more may be recorded", session),
            ));
        }
        _ => {}
    }
    match next.kind {
        SessionLifecycleEventKind::PermissionGranted
        | SessionLifecycleEventKind::PermissionRefused => {
            let request = next.permission_request.as_ref().expect(
                "validate_event_shape guarantees a permission request identity on answered events",
            );
            let issued = existing.iter().find(|event| {
                event.kind == SessionLifecycleEventKind::PermissionRequested
                    && event.permission_request.as_ref() == Some(request)
            });
            match issued {
                None => {
                    return Err(AikitError::new(
                        "session_lifecycle.unknown_permission_request",
                        format!("{request} was never issued in the history of {session}"),
                    ));
                }
                Some(_) if permission_request_open(existing, request).is_none() => {
                    return Err(AikitError::new(
                        "session_lifecycle.permission_request_closed",
                        format!("{request} was already answered in the history of {session}"),
                    ));
                }
                Some(issued) => {
                    if issued.activity != next.activity {
                        return Err(AikitError::new(
                            "session_lifecycle.permission_activity_mismatch",
                            format!(
                                "{request} belongs to activity {}, not {}",
                                issued.activity, next.activity
                            ),
                        ));
                    }
                    if issued.tool != next.tool {
                        return Err(AikitError::new(
                            "session_lifecycle.permission_tool_mismatch",
                            format!(
                                "{request} names tool {:?}, not {:?}",
                                issued.tool, next.tool
                            ),
                        ));
                    }
                }
            }
        }
        SessionLifecycleEventKind::SessionEnded => {
            if let Some(open) = open_permission_requests(existing).into_iter().next() {
                return Err(AikitError::new(
                    "session_lifecycle.permission_request_pending",
                    format!(
                        "cannot end {} while permission request {open} is still open",
                        session
                    ),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// The shape each kind is defined to carry. A malformed event is rejected
/// before any sequence rule is consulted.
fn validate_event_shape(event: &SessionLifecycleEvent) -> Result<()> {
    let malformed = |what: &str| {
        Err(AikitError::new(
            "session_lifecycle.malformed_event",
            format!("a {} event must carry {}", event.kind.as_str(), what),
        ))
    };
    match event.kind {
        SessionLifecycleEventKind::SessionStarted | SessionLifecycleEventKind::SessionEnded => {
            if event.reason.is_some()
                || event.tool.is_some()
                || event.permission_request.is_some()
                || event.state.is_some()
            {
                return malformed("no reason, tool, request or state");
            }
        }
        SessionLifecycleEventKind::Thinking => {
            if event.state.as_deref().map(str::trim) == Some("") || event.state.is_none() {
                return malformed("a non-empty state label");
            }
            if event.tool.is_some() || event.permission_request.is_some() {
                return malformed("no tool or request");
            }
        }
        SessionLifecycleEventKind::Cancelled => {
            if event.reason.as_deref().map(str::trim) == Some("") || event.reason.is_none() {
                return malformed("a non-empty reason");
            }
            if event.tool.is_some() || event.permission_request.is_some() || event.state.is_some() {
                return malformed("no tool, request or state");
            }
        }
        SessionLifecycleEventKind::PermissionRequested => {
            if event.permission_request.is_none() {
                return malformed("a stable permission request identity");
            }
            if event.tool.as_deref().map(str::trim) == Some("") || event.tool.is_none() {
                return malformed("the tool the request names");
            }
            if event.reason.is_some() || event.state.is_some() {
                return malformed("no reason or state");
            }
        }
        SessionLifecycleEventKind::PermissionGranted
        | SessionLifecycleEventKind::PermissionRefused => {
            if event.permission_request.is_none() {
                return malformed("the permission request identity being answered");
            }
            if matches!(event.kind, SessionLifecycleEventKind::PermissionGranted)
                && event.reason.is_some()
            {
                return malformed("no reason (only a refusal records one)");
            }
        }
    }
    Ok(())
}

/// The activity identity attached to a request when it was issued, if the
/// request is still open. `None` means it was never issued or is closed.
pub fn permission_request_open(
    events: &[SessionLifecycleEvent],
    request: &PermissionRequestId,
) -> Option<SessionActivityId> {
    let mut activity = None;
    for event in events {
        match (&event.permission_request, event.kind) {
            (Some(id), SessionLifecycleEventKind::PermissionRequested) if id == request => {
                activity = Some(event.activity.clone());
            }
            (Some(id), _)
                if id == request
                    && matches!(
                        event.kind,
                        SessionLifecycleEventKind::PermissionGranted
                            | SessionLifecycleEventKind::PermissionRefused
                    ) =>
            {
                return None;
            }
            _ => {}
        }
    }
    activity
}

fn open_permission_requests(events: &[SessionLifecycleEvent]) -> Vec<PermissionRequestId> {
    let mut requested: Vec<&PermissionRequestId> = Vec::new();
    let mut closed: Vec<&PermissionRequestId> = Vec::new();
    for event in events {
        if let Some(request) = event.permission_request.as_ref() {
            match event.kind {
                SessionLifecycleEventKind::PermissionRequested => requested.push(request),
                SessionLifecycleEventKind::PermissionGranted
                | SessionLifecycleEventKind::PermissionRefused => closed.push(request),
                _ => {}
            }
        }
    }
    requested
        .into_iter()
        .filter(|request| !closed.contains(request))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Read model
// ---------------------------------------------------------------------------

/// The derived lifecycle state of one session. Computed from recorded events
/// only — it is a reading, never a second truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionLifecycleState {
    /// The last event was `session-started` (or a permission answer).
    Running,
    /// The last recorded event is an in-flight `thinking` marker.
    Thinking,
    /// A permission request is waiting for an answer.
    PermissionPending,
    /// The session was cancelled. Terminal like `ended`, but distinguishable.
    Cancelled,
    /// The session ended normally. Terminal.
    Ended,
}

/// The typed, schema-stamped read model of one session's lifecycle. Every
/// field is derived from the durable history; the identity strings are
/// quoted verbatim from the recorded events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLifecycleReadModel {
    pub version: String,
    pub session: SessionId,
    pub state: SessionLifecycleState,
    pub event_count: usize,
    pub started_at_unix_ms: u128,
    pub last_event_at_unix_ms: u128,
    /// Every activity identity that appears in the history, in first-seen
    /// order. Correlators can match these against their own records without
    /// re-deriving anything.
    pub activities: Vec<SessionActivityId>,
    /// Permission requests still awaiting an answer.
    pub open_permission_requests: Vec<PermissionRequestId>,
    pub events: Vec<SessionLifecycleEvent>,
}

/// Derive the read model from recorded events. The caller (the store)
/// guarantees the events all belong to `session`; a history containing an
/// event for another session is a corrupt store and is reported, not
/// silently tolerated.
pub fn session_lifecycle_read_model(
    session: &SessionId,
    mut events: Vec<SessionLifecycleEvent>,
) -> Result<SessionLifecycleReadModel> {
    for event in &events {
        if &event.session != session {
            return Err(AikitError::new(
                "session_lifecycle.identity_mismatch",
                format!(
                    "history of {} contains an event for {}",
                    session, event.session
                ),
            ));
        }
    }
    events.sort_by_key(|event| (event.recorded_at_unix_ms, event.event_id.clone()));
    let state = derive_state(&events);
    let mut activities: Vec<SessionActivityId> = Vec::new();
    for event in &events {
        if !activities.contains(&event.activity) {
            activities.push(event.activity.clone());
        }
    }
    Ok(SessionLifecycleReadModel {
        version: SESSION_LIFECYCLE_VERSION.into(),
        session: session.clone(),
        state,
        event_count: events.len(),
        started_at_unix_ms: events
            .first()
            .map(|event| event.recorded_at_unix_ms)
            .unwrap_or(0),
        last_event_at_unix_ms: events
            .last()
            .map(|event| event.recorded_at_unix_ms)
            .unwrap_or(0),
        activities,
        open_permission_requests: open_permission_requests(&events),
        events,
    })
}

fn derive_state(events: &[SessionLifecycleEvent]) -> SessionLifecycleState {
    let mut state = SessionLifecycleState::Running;
    for event in events {
        state = match event.kind {
            SessionLifecycleEventKind::SessionStarted => SessionLifecycleState::Running,
            SessionLifecycleEventKind::SessionEnded => SessionLifecycleState::Ended,
            SessionLifecycleEventKind::Thinking => SessionLifecycleState::Thinking,
            SessionLifecycleEventKind::Cancelled => SessionLifecycleState::Cancelled,
            SessionLifecycleEventKind::PermissionRequested => {
                SessionLifecycleState::PermissionPending
            }
            SessionLifecycleEventKind::PermissionGranted
            | SessionLifecycleEventKind::PermissionRefused => SessionLifecycleState::Running,
        };
    }
    // A still-open permission request dominates the derived state.
    if !open_permission_requests(events).is_empty() {
        state = SessionLifecycleState::PermissionPending;
    }
    state
}

/// The set of distinct activity identities across a history. Convenience for
/// correlators that only need the join column.
pub fn lifecycle_activities(events: &[SessionLifecycleEvent]) -> BTreeSet<SessionActivityId> {
    events.iter().map(|event| event.activity.clone()).collect()
}

fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionId {
        SessionId::parse("ses_testlifecycle").unwrap()
    }

    fn activity(name: &str) -> SessionActivityId {
        SessionActivityId::parse(&format!("act_{name}")).unwrap()
    }

    fn request(name: &str) -> PermissionRequestId {
        PermissionRequestId::parse(&format!("prq_{name}")).unwrap()
    }

    fn start() -> SessionLifecycleEvent {
        SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::SessionStarted,
            "operator",
        )
    }

    fn request_event() -> SessionLifecycleEvent {
        SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::PermissionRequested,
            "agent:worker",
        )
        .for_tool("shell/exec")
        .for_permission_request(request("r1"))
    }

    #[test]
    fn first_event_must_be_session_started() {
        let error = validate_next_event(&session(), &[], &request_event()).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.start_required");
    }

    #[test]
    fn unknown_session_event_is_rejected_with_wrong_session() {
        let other = SessionId::parse("ses_other").unwrap();
        let mut event = start();
        event.session = other;
        let error = validate_next_event(&session(), &[], &event).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.wrong_session");
    }

    #[test]
    fn grant_requires_an_issued_request_and_recovers_its_activity() {
        let mut issued = request_event();
        let history = vec![start(), issued.clone()];

        // A grant carries the same activity and tool as the request.
        let grant = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::PermissionGranted,
            "operator",
        )
        .for_tool("shell/exec")
        .for_permission_request(request("r1"));
        validate_next_event(&session(), &history, &grant).unwrap();

        // The wrong activity is an explicit mismatch, never a silent re-join.
        let wrong_activity = SessionLifecycleEvent::new(
            session(),
            activity("two"),
            SessionLifecycleEventKind::PermissionGranted,
            "operator",
        )
        .for_tool("shell/exec")
        .for_permission_request(request("r1"));
        let error = validate_next_event(&session(), &history, &wrong_activity).unwrap_err();
        assert_eq!(
            error.code(),
            "session_lifecycle.permission_activity_mismatch"
        );

        // A never-issued request is explicit.
        let unknown = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::PermissionGranted,
            "operator",
        )
        .for_tool("shell/exec")
        .for_permission_request(request("absent"));
        let error = validate_next_event(&session(), &history, &unknown).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.unknown_permission_request");

        // A closed request cannot be answered twice.
        issued.event_id = EventId::generate();
        let mut history = history;
        history.push(grant.clone());
        let second = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::PermissionRefused,
            "operator",
        )
        .for_tool("shell/exec")
        .for_permission_request(request("r1"))
        .with_reason("revoked");
        let error = validate_next_event(&session(), &history, &second).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.permission_request_closed");
    }

    #[test]
    fn nothing_records_after_session_ended() {
        let ended = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::SessionEnded,
            "operator",
        );
        let history = vec![start(), ended];
        let error = validate_next_event(&session(), &history, &request_event()).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.already_ended");
    }

    #[test]
    fn cancellation_is_terminal() {
        let cancelled = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::Cancelled,
            "operator",
        )
        .with_reason("operator interrupt");
        let history = vec![start(), cancelled];
        let ended = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::SessionEnded,
            "operator",
        );
        let error = validate_next_event(&session(), &history, &ended).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.already_cancelled");
    }

    #[test]
    fn session_cannot_end_with_an_open_permission_request() {
        let history = vec![start(), request_event()];
        let ended = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::SessionEnded,
            "operator",
        );
        let error = validate_next_event(&session(), &history, &ended).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.permission_request_pending");
    }

    #[test]
    fn thinking_requires_a_state_label_and_cancellation_a_reason() {
        let bare = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::Thinking,
            "agent:worker",
        );
        let error = validate_next_event(&session(), &[start()], &bare).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.malformed_event");

        let thinking = bare.clone().in_state("awaiting-tool");
        validate_next_event(&session(), &[start()], &thinking).unwrap();

        let bare_cancel = SessionLifecycleEvent::new(
            session(),
            activity("one"),
            SessionLifecycleEventKind::Cancelled,
            "operator",
        );
        let error = validate_next_event(&session(), &[start()], &bare_cancel).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.malformed_event");
        let cancelled = bare_cancel.with_reason("operator interrupt");
        validate_next_event(&session(), &[start()], &cancelled).unwrap();
    }

    #[test]
    fn read_model_derives_state_activities_and_open_requests() {
        let history = vec![
            start(),
            request_event(),
            SessionLifecycleEvent::new(
                session(),
                activity("one"),
                SessionLifecycleEventKind::Thinking,
                "agent:worker",
            )
            .in_state("reasoning"),
            SessionLifecycleEvent::new(
                session(),
                activity("two"),
                SessionLifecycleEventKind::PermissionRequested,
                "agent:worker",
            )
            .for_tool("fs/write")
            .for_permission_request(request("r2")),
        ];
        let model = session_lifecycle_read_model(&session(), history).unwrap();
        assert_eq!(model.version, "aikit.session-lifecycle/v1");
        assert_eq!(model.state, SessionLifecycleState::PermissionPending);
        assert_eq!(model.event_count, 4);
        assert_eq!(model.activities, vec![activity("one"), activity("two")]);
        assert_eq!(
            model.open_permission_requests,
            vec![request("r1"), request("r2")]
        );

        // Answering both requests returns the derived state to running;
        // identities quote through unchanged.
        let mut history = model.events;
        history.push(
            SessionLifecycleEvent::new(
                session(),
                activity("two"),
                SessionLifecycleEventKind::PermissionRefused,
                "operator",
            )
            .for_tool("fs/write")
            .for_permission_request(request("r2"))
            .with_reason("not on this branch"),
        );
        history.push(
            SessionLifecycleEvent::new(
                session(),
                activity("one"),
                SessionLifecycleEventKind::PermissionGranted,
                "operator",
            )
            .for_tool("shell/exec")
            .for_permission_request(request("r1")),
        );
        let model = session_lifecycle_read_model(&session(), history).unwrap();
        assert_eq!(model.state, SessionLifecycleState::Running);
        assert!(model.open_permission_requests.is_empty());
        assert_eq!(
            lifecycle_activities(&model.events),
            [activity("one"), activity("two")].into_iter().collect()
        );
    }

    #[test]
    fn read_model_rejects_a_history_containing_another_session() {
        let mut foreign = start();
        foreign.session = SessionId::parse("ses_foreign").unwrap();
        let error = session_lifecycle_read_model(&session(), vec![foreign]).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.identity_mismatch");
    }

    #[test]
    fn kind_strings_are_stable_kebab_case() {
        assert_eq!(
            SessionLifecycleEventKind::PermissionRequested.as_str(),
            "permission-requested"
        );
        assert_eq!(
            SessionLifecycleEventKind::SessionStarted.as_str(),
            "session-started"
        );
        assert_eq!(SessionLifecycleEventKind::Cancelled.as_str(), "cancelled");
    }
}
