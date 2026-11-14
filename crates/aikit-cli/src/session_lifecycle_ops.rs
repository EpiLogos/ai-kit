//! Session lifecycle operation family on the canonical CLI/TUI `Service`.
//!
//! The family reads and writes through the `AikitHome` alone, so it is
//! implemented on the home; the Service delegates to the home it owns, using
//! that exact home rather than rediscovering process-global state, and a verb
//! that needs nothing else of the Service (the CLI's `session lifecycle …`)
//! works from the home without building one. All
//! durability, validation and read-model derivation stay in
//! `aikit_store::SessionLifecycleStore`; this layer only shapes owner
//! intents into fully-formed events, minting identities the caller did not
//! supply and recovering the activity identity for permission answers so the
//! issued/granted/refused triple stays on one stable join identity.

use aikit_core::session_lifecycle::{
    SessionLifecycleEvent, SessionLifecycleEventKind, SessionLifecycleReadModel,
    SessionLifecycleRecord,
};
use aikit_core::{PermissionRequestId, Result, SessionActivityId, SessionId};
use aikit_store::home::AikitHome;
use aikit_store::SessionLifecycleStore;

use crate::app::Service;

pub trait SessionLifecycleServiceOps {
    fn session_lifecycle_list(&self) -> Result<Vec<SessionId>>;
    fn session_lifecycle_history(&self, session: &SessionId) -> Result<Vec<SessionLifecycleEvent>>;
    fn session_lifecycle_read_model(
        &self,
        session: &SessionId,
    ) -> Result<SessionLifecycleReadModel>;
    /// Record one lifecycle event. `activity` is the caller-supplied stable
    /// activity identity; permission answers recover theirs from the
    /// original request event so the triple stays on one identity.
    fn session_lifecycle_record(
        &self,
        session: SessionId,
        record: SessionLifecycleRecord,
        activity: Option<SessionActivityId>,
        origin: impl Into<String>,
    ) -> Result<SessionLifecycleEvent>;
}

impl SessionLifecycleServiceOps for Service {
    fn session_lifecycle_list(&self) -> Result<Vec<SessionId>> {
        self.home().session_lifecycle_list()
    }

    fn session_lifecycle_history(&self, session: &SessionId) -> Result<Vec<SessionLifecycleEvent>> {
        self.home().session_lifecycle_history(session)
    }

    fn session_lifecycle_read_model(
        &self,
        session: &SessionId,
    ) -> Result<SessionLifecycleReadModel> {
        self.home().session_lifecycle_read_model(session)
    }

    fn session_lifecycle_record(
        &self,
        session: SessionId,
        record: SessionLifecycleRecord,
        activity: Option<SessionActivityId>,
        origin: impl Into<String>,
    ) -> Result<SessionLifecycleEvent> {
        self.home()
            .session_lifecycle_record(session, record, activity, origin)
    }
}

impl SessionLifecycleServiceOps for AikitHome {
    fn session_lifecycle_list(&self) -> Result<Vec<SessionId>> {
        store(self).list()
    }

    fn session_lifecycle_history(&self, session: &SessionId) -> Result<Vec<SessionLifecycleEvent>> {
        store(self).history(session)
    }

    fn session_lifecycle_read_model(
        &self,
        session: &SessionId,
    ) -> Result<SessionLifecycleReadModel> {
        store(self).read_model(session)
    }

    fn session_lifecycle_record(
        &self,
        session: SessionId,
        record: SessionLifecycleRecord,
        activity: Option<SessionActivityId>,
        origin: impl Into<String>,
    ) -> Result<SessionLifecycleEvent> {
        let store = store(self);
        let origin = origin.into();
        let minted = || activity.clone().unwrap_or_else(SessionActivityId::generate);
        let event = match record {
            SessionLifecycleRecord::Start => SessionLifecycleEvent::new(
                session,
                minted(),
                SessionLifecycleEventKind::SessionStarted,
                origin,
            ),
            SessionLifecycleRecord::End => SessionLifecycleEvent::new(
                session,
                minted(),
                SessionLifecycleEventKind::SessionEnded,
                origin,
            ),
            SessionLifecycleRecord::Thinking { state } => SessionLifecycleEvent::new(
                session,
                minted(),
                SessionLifecycleEventKind::Thinking,
                origin,
            )
            .in_state(state),
            SessionLifecycleRecord::Cancel { reason } => SessionLifecycleEvent::new(
                session,
                minted(),
                SessionLifecycleEventKind::Cancelled,
                origin,
            )
            .with_reason(reason),
            SessionLifecycleRecord::PermissionRequest {
                tool,
                activity,
                request,
            } => SessionLifecycleEvent::new(
                session,
                activity,
                SessionLifecycleEventKind::PermissionRequested,
                origin,
            )
            .for_tool(tool)
            .for_permission_request(request.unwrap_or_else(PermissionRequestId::generate)),
            SessionLifecycleRecord::PermissionGrant { request } => {
                let issued = permission_request_activity(&store, &session, &request)?;
                SessionLifecycleEvent::new(
                    session,
                    issued.0,
                    SessionLifecycleEventKind::PermissionGranted,
                    origin,
                )
                .for_tool(issued.1)
                .for_permission_request(request)
            }
            SessionLifecycleRecord::PermissionRefuse { request, reason } => {
                let issued = permission_request_activity(&store, &session, &request)?;
                SessionLifecycleEvent::new(
                    session,
                    issued.0,
                    SessionLifecycleEventKind::PermissionRefused,
                    origin,
                )
                .for_tool(issued.1)
                .for_permission_request(request)
                .with_reason(reason)
            }
        };
        store.record(event)
    }
}

/// The lifecycle identity of the harness session `session_ref` names
/// (`aikit:session:<client>:<id>`), derived from the ref as an AgentSession's
/// runtime identity is: every dispatch of one session lands on one history.
pub fn harness_session(session_ref: &str) -> Result<SessionId> {
    Ok(SessionId::of_agent_session(
        &aikit_core::ResourceRef::parse(session_ref)?,
    ))
}

/// What a harness session's start found in its lifecycle (O-I #1016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessStart {
    /// The session's first start: recorded.
    First,
    /// The session had ended and is resumed (`claude --resume`, `/resume`,
    /// `codex resume` reuse its id): its start is recorded again.
    Resumed,
    /// A start replayed for a live session (resume, clear, compact), or for a
    /// cancelled one, whose cancellation is terminal: nothing recorded.
    Replayed,
}

/// Record a harness session's start (O-I #1016): its first start, and the
/// start of each resume after an end. A start replayed for a live session is
/// no second start. The event's origin is the session ref, which is also the
/// session's NOW task in Central.
pub fn record_harness_start(home: &AikitHome, session_ref: &str) -> Result<HarnessStart> {
    let session = harness_session(session_ref)?;
    let found = match home.session_lifecycle_history(&session) {
        Ok(history) => match history.last().map(|event| event.kind) {
            Some(SessionLifecycleEventKind::SessionEnded) => HarnessStart::Resumed,
            _ => return Ok(HarnessStart::Replayed),
        },
        Err(error) if error.code() == "session_lifecycle.unknown_session" => HarnessStart::First,
        Err(error) => return Err(error),
    };
    home.session_lifecycle_record(session, SessionLifecycleRecord::Start, None, session_ref)?;
    Ok(found)
}

/// Record a harness session's end. `Ok(false)` when this store never saw the
/// session start, or its history already ended or was cancelled.
pub fn record_harness_end(home: &AikitHome, session_ref: &str) -> Result<bool> {
    let session = harness_session(session_ref)?;
    let history = match home.session_lifecycle_history(&session) {
        Ok(history) => history,
        Err(error) if error.code() == "session_lifecycle.unknown_session" => return Ok(false),
        Err(error) => return Err(error),
    };
    if matches!(
        history.last().map(|event| event.kind),
        Some(SessionLifecycleEventKind::SessionEnded | SessionLifecycleEventKind::Cancelled)
    ) {
        return Ok(false);
    }
    home.session_lifecycle_record(session, SessionLifecycleRecord::End, None, session_ref)?;
    Ok(true)
}

/// Recover the activity identity and tool of one issued permission request.
/// Unknown requests are explicit here — before the store's own validation —
/// so the caller gets `session_lifecycle.unknown_permission_request` rather
/// than a minted event that could never be recorded.
fn permission_request_activity(
    store: &SessionLifecycleStore,
    session: &SessionId,
    request: &PermissionRequestId,
) -> Result<(SessionActivityId, String)> {
    let issued = store
        .history(session)?
        .into_iter()
        .find(|event| {
            event.kind == SessionLifecycleEventKind::PermissionRequested
                && event.permission_request.as_ref() == Some(request)
        })
        .ok_or_else(|| {
            aikit_core::AikitError::new(
                "session_lifecycle.unknown_permission_request",
                format!("{request} was never issued in the history of {session}"),
            )
        })?;
    Ok((issued.activity, issued.tool.unwrap_or_default()))
}

fn store(home: &AikitHome) -> SessionLifecycleStore {
    SessionLifecycleStore::new(home.clone())
}
