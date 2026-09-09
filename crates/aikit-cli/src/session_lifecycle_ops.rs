//! Session lifecycle operation family on the canonical CLI/TUI `Service`.
//!
//! The Service already owns the injected `AikitHome`; this extension uses
//! that exact home rather than rediscovering process-global state. All
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

fn store(service: &Service) -> SessionLifecycleStore {
    SessionLifecycleStore::new(service.home().clone())
}
