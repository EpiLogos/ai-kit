//! Canonical persistence authority for durable session lifecycle history.
//!
//! One canonical document per session holds the append-only event history;
//! the read model in `aikit_core::session_lifecycle` is derived from it at
//! read time, so History and reads can never describe different things.
//! Writes go through the same cross-process lock law as SessionSpace
//! application, and every write validates the candidate event through the
//! core recording law before touching the filesystem.
//!
//! Reads are strictly reads: `history`, `read_model` and `list` never create
//! a file, never repair one, and report an absent session as the explicit
//! `session_lifecycle.unknown_session` error rather than fabricating an
//! empty history.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::session_lifecycle::{
    session_lifecycle_read_model, validate_next_event, SessionLifecycleEvent,
    SessionLifecycleReadModel,
};
use aikit_core::{AikitError, Result, SessionId};

use crate::home::{create_dir_all, io_error};
use crate::{AikitHome, ContextLock, LockOptions};

pub const SESSION_LIFECYCLE_STORE_VERSION: &str = "aikit.session-lifecycle-store/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SessionLifecycleCanonicalFile {
    version: String,
    session: SessionId,
    events: Vec<SessionLifecycleEvent>,
}

#[derive(Debug, Clone)]
pub struct SessionLifecycleStore {
    home: AikitHome,
}

impl SessionLifecycleStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    pub fn root(&self) -> PathBuf {
        self.home.state().join("session-lifecycle")
    }

    /// Every session that has a lifecycle history, sorted by identity.
    /// A store root that was never written returns an empty list; that is a
    /// true statement about the store, not a fabricated session.
    pub fn list(&self) -> Result<Vec<SessionId>> {
        let root = self.root();
        if !root.exists() {
            return Ok(Vec::new());
        }
        if !root.is_dir() {
            return Err(AikitError::new(
                "session_lifecycle.store_unavailable",
                format!(
                    "{} exists but is not a directory; the session lifecycle store is unavailable",
                    root.display()
                ),
            ));
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&root)
            .map_err(|error| io_error("session_lifecycle.list_failed", &root, &error))?
        {
            let entry =
                entry.map_err(|error| io_error("session_lifecycle.list_failed", &root, &error))?;
            let path = entry.path().join("events.json");
            if !path.is_file() {
                continue;
            }
            sessions.push(self.read_file_at(&path)?.session);
        }
        sessions.sort();
        sessions.dedup();
        Ok(sessions)
    }

    /// The durable history of one session. Unknown sessions are an explicit
    /// error; a session with no history is unknown to this store.
    pub fn history(&self, session: &SessionId) -> Result<Vec<SessionLifecycleEvent>> {
        self.load_file(session).map(|file| file.events)
    }

    /// The typed, schema-stamped read model of one session, derived from the
    /// durable history. A read: it records nothing.
    pub fn read_model(&self, session: &SessionId) -> Result<SessionLifecycleReadModel> {
        let file = self.load_file(session)?;
        session_lifecycle_read_model(&file.session, file.events)
    }

    /// Record one lifecycle event. The candidate is validated against the
    /// current canonical history under the session lock; a rejected event
    /// changes nothing on disk.
    pub fn record(&self, event: SessionLifecycleEvent) -> Result<SessionLifecycleEvent> {
        let key = format!("session-lifecycle-{}", session_key(&event.session));
        let _lock = ContextLock::acquire(
            &self.home,
            &key,
            LockOptions::default()
                .with_purpose(format!("record lifecycle event for {}", event.session)),
        )?;

        let path = self.events_file(&event.session);
        let existing = if path.is_file() {
            Some(self.read_file_at(&path)?)
        } else {
            None
        };
        let current: &[SessionLifecycleEvent] = existing
            .as_ref()
            .map(|file| file.events.as_slice())
            .unwrap_or(&[]);
        match (existing.as_ref(), current) {
            (Some(file), _) if file.session != event.session => {
                return Err(AikitError::new(
                    "session_lifecycle.identity_mismatch",
                    format!(
                        "{} contains the history of {}",
                        path.display(),
                        file.session
                    ),
                ));
            }
            _ => {}
        }
        validate_next_event(&event.session, current, &event)?;

        let canonical = match existing {
            Some(mut file) => {
                file.events.push(event.clone());
                file
            }
            None => SessionLifecycleCanonicalFile {
                version: SESSION_LIFECYCLE_STORE_VERSION.into(),
                session: event.session.clone(),
                events: vec![event.clone()],
            },
        };
        self.write_file(&path, &canonical)?;
        Ok(event)
    }

    fn load_file(&self, session: &SessionId) -> Result<SessionLifecycleCanonicalFile> {
        let path = self.events_file(session);
        if !path.is_file() {
            return Err(AikitError::new(
                "session_lifecycle.unknown_session",
                format!("{session} has no lifecycle history in this store"),
            )
            .with("path", path.display().to_string()));
        }
        let file = self.read_file_at(&path)?;
        if &file.session != session {
            return Err(AikitError::new(
                "session_lifecycle.identity_mismatch",
                format!(
                    "{} contains the history of {}",
                    path.display(),
                    file.session
                ),
            ));
        }
        Ok(file)
    }

    fn read_file_at(&self, path: &Path) -> Result<SessionLifecycleCanonicalFile> {
        let bytes = fs::read(path)
            .map_err(|error| io_error("session_lifecycle.read_failed", path, &error))?;
        let file: SessionLifecycleCanonicalFile =
            serde_json::from_slice(&bytes).map_err(|error| {
                AikitError::new(
                    "session_lifecycle.invalid_history",
                    format!("{}: {error}", path.display()),
                )
                .with("path", path.display().to_string())
            })?;
        if file.version != SESSION_LIFECYCLE_STORE_VERSION {
            return Err(AikitError::new(
                "session_lifecycle.unsupported_store_version",
                format!(
                    "{} uses unsupported version {}",
                    path.display(),
                    file.version
                ),
            ));
        }
        Ok(file)
    }

    fn events_file(&self, session: &SessionId) -> PathBuf {
        self.root().join(session_key(session)).join("events.json")
    }

    fn write_file(&self, path: &Path, file: &SessionLifecycleCanonicalFile) -> Result<()> {
        let parent = path
            .parent()
            .expect("session lifecycle path always has a parent");
        create_dir_all(parent)?;
        let encoded = serde_json::to_vec_pretty(file).map_err(|error| {
            AikitError::new(
                "session_lifecycle.history_unserializable",
                format!("could not encode canonical session lifecycle history: {error}"),
            )
        })?;
        let temp = parent.join(format!(".events-{}.tmp", std::process::id()));
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)
            .map_err(|error| io_error("session_lifecycle.write_failed", &temp, &error))?;
        output
            .write_all(&encoded)
            .and_then(|_| output.sync_all())
            .map_err(|error| io_error("session_lifecycle.write_failed", &temp, &error))?;
        fs::rename(&temp, path)
            .map_err(|error| io_error("session_lifecycle.commit_failed", path, &error))
    }
}

fn session_key(session: &SessionId) -> String {
    let digest = blake3::hash(session.to_string().as_bytes())
        .to_hex()
        .to_string();
    digest[..24].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::session_lifecycle::{SessionLifecycleEventKind, SessionLifecycleState};
    use aikit_core::{PermissionRequestId, SessionActivityId};

    fn store() -> (tempfile::TempDir, SessionLifecycleStore) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        (dir, SessionLifecycleStore::new(home))
    }

    fn sid(name: &str) -> SessionId {
        SessionId::parse(&format!("ses_{name}")).unwrap()
    }

    fn activity(name: &str) -> SessionActivityId {
        SessionActivityId::parse(&format!("act_{name}")).unwrap()
    }

    fn request(name: &str) -> PermissionRequestId {
        PermissionRequestId::parse(&format!("prq_{name}")).unwrap()
    }

    fn record(
        store: &SessionLifecycleStore,
        session: &SessionId,
        kind: SessionLifecycleEventKind,
        activity: SessionActivityId,
    ) -> SessionLifecycleEvent {
        store
            .record(SessionLifecycleEvent::new(
                session.clone(),
                activity,
                kind,
                "operator",
            ))
            .unwrap()
    }

    #[test]
    fn history_survives_store_reopen_with_identities_unchanged() {
        let (dir, store) = store();
        let session = sid("durable");
        record(
            &store,
            &session,
            SessionLifecycleEventKind::SessionStarted,
            activity("one"),
        );
        // A thinking event without a state is malformed: rejected, recorded
        // nothing.
        let malformed = SessionLifecycleEvent::new(
            session.clone(),
            activity("one"),
            SessionLifecycleEventKind::Thinking,
            "agent:worker",
        );
        let error = store.record(malformed).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.malformed_event");
        store
            .record(
                SessionLifecycleEvent::new(
                    session.clone(),
                    activity("one"),
                    SessionLifecycleEventKind::Thinking,
                    "agent:worker",
                )
                .in_state("reasoning"),
            )
            .unwrap();
        store
            .record(
                SessionLifecycleEvent::new(
                    session.clone(),
                    activity("one"),
                    SessionLifecycleEventKind::PermissionRequested,
                    "agent:worker",
                )
                .for_tool("shell/exec")
                .for_permission_request(request("durable-r1")),
            )
            .unwrap();
        let before = store.history(&session).unwrap();

        // Reopen the store over the same home: the durable history reads
        // back byte-identical, identities included.
        let reopened = SessionLifecycleStore::new(AikitHome::at(dir.path()));
        let after = reopened.history(&session).unwrap();
        assert_eq!(before, after);
        assert_eq!(after.len(), 3);
        assert!(after.iter().all(|event| event.session == session));
        let requested = after
            .iter()
            .find(|event| event.kind == SessionLifecycleEventKind::PermissionRequested)
            .unwrap();
        assert_eq!(requested.permission_request, Some(request("durable-r1")));
        assert_eq!(requested.activity, activity("one"));

        // The derived read model survives reopen too.
        let model = reopened.read_model(&session).unwrap();
        assert_eq!(model.state, SessionLifecycleState::PermissionPending);
        assert_eq!(model.open_permission_requests, vec![request("durable-r1")]);
        assert_eq!(model.activities, vec![activity("one")]);
    }

    #[test]
    fn reads_are_reads_and_unknown_sessions_are_explicit() {
        let (_dir, store) = store();
        let session = sid("reads");
        record(
            &store,
            &session,
            SessionLifecycleEventKind::SessionStarted,
            activity("one"),
        );

        let events_file = store.events_file(&session);
        let bytes_before = fs::read(&events_file).unwrap();
        store.history(&session).unwrap();
        store.read_model(&session).unwrap();
        store.list().unwrap();
        assert_eq!(
            fs::read(&events_file).unwrap(),
            bytes_before,
            "reading must not rewrite the canonical history"
        );

        let unknown = sid("nobody");
        let error = store.history(&unknown).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.unknown_session");
        let error = store.read_model(&unknown).unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.unknown_session");
    }

    #[test]
    fn grant_reopen_refusal_keeps_one_join_identity_across_reopens() {
        let (dir, store) = store();
        let session = sid("join");
        record(
            &store,
            &session,
            SessionLifecycleEventKind::SessionStarted,
            activity("main"),
        );
        store
            .record(
                SessionLifecycleEvent::new(
                    session.clone(),
                    activity("main"),
                    SessionLifecycleEventKind::PermissionRequested,
                    "agent:worker",
                )
                .for_tool("fs/write")
                .for_permission_request(request("join-r1")),
            )
            .unwrap();

        // A separate process view (a reopened store) answers the request by
        // its stable identity; the refusal event quotes the same activity.
        let reopened = SessionLifecycleStore::new(AikitHome::at(dir.path()));
        reopened
            .record(
                SessionLifecycleEvent::new(
                    session.clone(),
                    activity("main"),
                    SessionLifecycleEventKind::PermissionRefused,
                    "operator",
                )
                .for_tool("fs/write")
                .for_permission_request(request("join-r1"))
                .with_reason("not on this branch"),
            )
            .unwrap();

        let final_store = SessionLifecycleStore::new(AikitHome::at(dir.path()));
        let history = final_store.history(&session).unwrap();
        let pair: Vec<_> = history
            .iter()
            .filter(|event| event.permission_request == Some(request("join-r1")))
            .collect();
        assert_eq!(pair.len(), 2);
        assert!(pair
            .iter()
            .all(|event| event.activity == activity("main")));
        assert!(final_store
            .read_model(&session)
            .unwrap()
            .open_permission_requests
            .is_empty());
    }

    #[test]
    fn store_root_that_is_a_file_is_unavailable_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let store = SessionLifecycleStore::new(home);
        fs::write(store.root(), "not a directory").unwrap();
        let error = store.list().unwrap_err();
        assert_eq!(error.code(), "session_lifecycle.store_unavailable");
    }

    #[test]
    fn recording_is_scoped_per_session() {
        let (_dir, store) = store();
        let one = sid("one");
        let two = sid("two");
        record(
            &store,
            &one,
            SessionLifecycleEventKind::SessionStarted,
            activity("a"),
        );
        record(
            &store,
            &two,
            SessionLifecycleEventKind::SessionStarted,
            activity("b"),
        );
        assert_eq!(store.list().unwrap(), vec![one.clone(), two.clone()]);
        assert_eq!(store.history(&one).unwrap().len(), 1);
        assert_eq!(store.history(&two).unwrap().len(), 1);
    }
}
