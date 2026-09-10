//! Canonical persistence authority for SessionSpace application state.
//!
//! The existing provider observation files remain observations. This store writes
//! only AIKit-owned semantic state from `aikit_core::session_space_application`.
//! Current state and append-only application receipts live in one atomically
//! replaced canonical document, so History is evidence over the same authority,
//! not a second mutable truth store.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use aikit_core::project::ProjectRef;
use aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};
use aikit_core::session_space_application::{
    explain_session_space, reconstruct_session_space, stage_session_space,
    AgentSessionContinuityEvidence, SessionSpaceAuthoredState, SessionSpaceBasis,
    SessionSpaceChange, SessionSpaceExplanation, SessionSpaceMutation,
    SessionSpaceNativeObservation, SessionSpacePreview, SessionSpaceReconstructionReport,
};
use aikit_core::{AikitError, Result};

use crate::home::{create_dir_all, io_error};
use crate::{AikitHome, ContextLock, LockOptions};

pub const SESSION_SPACE_STORE_VERSION: &str = "aikit.session-space-store/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceReceipt {
    pub sequence: u64,
    pub space: SessionSpaceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<SessionSpaceBasis>,
    pub resulting_basis: SessionSpaceBasis,
    pub operation: SessionSpaceMutation,
    #[serde(default)]
    pub changed: Vec<SessionSpaceChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<SessionSpaceAuthoredState>,
    pub after: SessionSpaceAuthoredState,
    pub applied_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SessionSpaceCanonicalFile {
    version: String,
    state: SessionSpaceAuthoredState,
    #[serde(default)]
    receipts: Vec<SessionSpaceReceipt>,
}

impl SessionSpaceCanonicalFile {
    fn new(state: SessionSpaceAuthoredState, receipt: SessionSpaceReceipt) -> Self {
        Self {
            version: SESSION_SPACE_STORE_VERSION.into(),
            state,
            receipts: vec![receipt],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceHistoryComparison {
    pub space: SessionSpaceRef,
    pub from_sequence: u64,
    pub to_sequence: u64,
    pub from_basis: SessionSpaceBasis,
    pub to_basis: SessionSpaceBasis,
    pub project_context_changed: bool,
    pub agent_session_intent_changed: bool,
    pub surface_intent_changed: bool,
    pub native_reference_changed: bool,
    pub focus_changed: bool,
}

#[derive(Debug, Clone)]
pub struct SessionSpaceApplicationStore {
    home: AikitHome,
}

impl SessionSpaceApplicationStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    pub fn root(&self) -> PathBuf {
        self.home.state().join("session-spaces")
    }

    pub fn list(&self) -> Result<Vec<SessionSpaceAuthoredState>> {
        let root = self.root();
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut states = Vec::new();
        for entry in fs::read_dir(&root)
            .map_err(|error| io_error("session_space.list_failed", &root, &error))?
        {
            let entry =
                entry.map_err(|error| io_error("session_space.list_failed", &root, &error))?;
            let path = entry.path().join("state.json");
            if !path.is_file() {
                continue;
            }
            states.push(self.read_file_at(&path)?.state);
        }
        states.sort_by_key(|left| left.id().to_string());
        Ok(states)
    }

    pub fn discover(&self, project: Option<&ProjectRef>) -> Result<Vec<SessionSpaceAuthoredState>> {
        let mut states = self.list()?;
        if let Some(project) = project {
            states.retain(|state| {
                // Authored membership is the stable "which World is this
                // Project in" answer; observed project context is richer
                // evidence but must not be the only discovery signal.
                state.project_contexts.contains_key(project)
                    || state
                        .definition
                        .projects
                        .iter()
                        .any(|member| member.as_str() == project.as_str())
            });
        }
        Ok(states)
    }

    pub fn load(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        self.load_file(space).map(|file| file.state)
    }

    pub fn history(&self, space: &SessionSpaceRef) -> Result<Vec<SessionSpaceReceipt>> {
        self.load_file(space).map(|file| file.receipts)
    }

    pub fn stage(
        &self,
        space: Option<&SessionSpaceRef>,
        intent: SessionSpaceMutation,
    ) -> Result<SessionSpacePreview> {
        let current = match space {
            Some(space) => Some(self.load(space)?),
            None => None,
        };
        stage_session_space(current.as_ref(), intent)
    }

    /// Apply exactly the reviewed preview. The current canonical file is re-read
    /// under the same cross-process lock used by generation application; stale
    /// basis or changed preview output is rejected before any write.
    pub fn apply(&self, preview: &SessionSpacePreview) -> Result<SessionSpaceReceipt> {
        let key = format!("session-space-{}", space_key(&preview.space));
        let _lock = ContextLock::acquire(
            &self.home,
            &key,
            LockOptions::default().with_purpose(format!("apply SessionSpace {}", preview.space)),
        )?;

        let path = self.state_file(&preview.space);
        let existing = if path.exists() {
            Some(self.read_file_at(&path)?)
        } else {
            None
        };
        let current = existing.as_ref().map(|file| &file.state);
        validate_basis(preview.basis.as_ref(), current)?;

        let fresh = stage_session_space(current, preview.intent.clone())?;
        if fresh.proposed != preview.proposed || fresh.changed != preview.changed {
            return Err(AikitError::new(
                "session_space.preview_stale",
                "accepted SessionSpace preview no longer matches canonical application semantics",
            ));
        }

        let resulting_basis = fresh.proposed.basis()?;
        let sequence = existing
            .as_ref()
            .and_then(|file| file.receipts.last().map(|receipt| receipt.sequence + 1))
            .unwrap_or(0);
        let receipt = SessionSpaceReceipt {
            sequence,
            space: preview.space.clone(),
            basis: preview.basis.clone(),
            resulting_basis,
            operation: preview.intent.clone(),
            changed: preview.changed.clone(),
            before: current.cloned(),
            after: fresh.proposed.clone(),
            applied_at_unix_ms: now_unix_ms(),
        };

        let canonical = match existing {
            Some(mut file) => {
                file.state = fresh.proposed;
                file.receipts.push(receipt.clone());
                file
            }
            None => SessionSpaceCanonicalFile::new(fresh.proposed, receipt.clone()),
        };
        self.write_file(&path, &canonical)?;
        Ok(receipt)
    }

    /// Stage restoration from one exact prior receipt through the same current
    /// basis/preview/apply authority. History never writes canonical state itself.
    pub fn stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview> {
        let file = self.load_file(space)?;
        let target = file
            .receipts
            .iter()
            .find(|receipt| receipt.sequence == sequence)
            .ok_or_else(|| {
                AikitError::new(
                    "session_space.history_not_found",
                    format!("SessionSpace {space} has no receipt sequence {sequence}"),
                )
            })?;
        stage_session_space(
            Some(&file.state),
            SessionSpaceMutation::Restore {
                target: Box::new(target.after.clone()),
                evidence: format!("SessionSpace receipt sequence {sequence}"),
            },
        )
    }

    pub fn compare_history(
        &self,
        space: &SessionSpaceRef,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<SessionSpaceHistoryComparison> {
        let receipts = self.history(space)?;
        let from = receipt_at(&receipts, space, from_sequence)?;
        let to = receipt_at(&receipts, space, to_sequence)?;
        Ok(SessionSpaceHistoryComparison {
            space: space.clone(),
            from_sequence,
            to_sequence,
            from_basis: from.resulting_basis.clone(),
            to_basis: to.resulting_basis.clone(),
            project_context_changed: from.after.project_contexts != to.after.project_contexts,
            agent_session_intent_changed: from.after.agent_sessions != to.after.agent_sessions,
            surface_intent_changed: from.after.surfaces != to.after.surfaces,
            native_reference_changed: from.after.native_references != to.after.native_references,
            focus_changed: from.after.focus != to.after.focus,
        })
    }

    pub fn reconstruct(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        let authored = self.load(space)?;
        Ok(reconstruct_session_space(
            &authored,
            runtime,
            native_observations,
            continuity,
        ))
    }

    pub fn explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplanation> {
        let authored = self.load(space)?;
        Ok(explain_session_space(&authored, reconstruction))
    }

    fn load_file(&self, space: &SessionSpaceRef) -> Result<SessionSpaceCanonicalFile> {
        let path = self.state_file(space);
        if !path.is_file() {
            return Err(AikitError::new(
                "session_space.not_found",
                format!("SessionSpace {space} has no canonical state"),
            )
            .with("path", path.display().to_string()));
        }
        let file = self.read_file_at(&path)?;
        if file.state.id() != space {
            return Err(AikitError::new(
                "session_space.identity_mismatch",
                format!("{} contains SessionSpace {}", path.display(), file.state.id()),
            ));
        }
        Ok(file)
    }

    fn read_file_at(&self, path: &Path) -> Result<SessionSpaceCanonicalFile> {
        let bytes = fs::read(path)
            .map_err(|error| io_error("session_space.read_failed", path, &error))?;
        let file: SessionSpaceCanonicalFile = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "session_space.invalid_state",
                format!("{}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        if file.version != SESSION_SPACE_STORE_VERSION {
            return Err(AikitError::new(
                "session_space.unsupported_store_version",
                format!("{} uses unsupported version {}", path.display(), file.version),
            ));
        }
        file.state.validate()?;
        Ok(file)
    }

    fn state_file(&self, space: &SessionSpaceRef) -> PathBuf {
        self.root().join(space_key(space)).join("state.json")
    }

    fn write_file(&self, path: &Path, file: &SessionSpaceCanonicalFile) -> Result<()> {
        let parent = path
            .parent()
            .expect("SessionSpace state path always has a parent");
        create_dir_all(parent)?;
        let encoded = serde_json::to_vec_pretty(file).map_err(|error| {
            AikitError::new(
                "session_space.state_unserializable",
                format!("could not encode canonical SessionSpace state: {error}"),
            )
        })?;
        let temp = parent.join(format!(".state-{}.tmp", std::process::id()));
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)
            .map_err(|error| io_error("session_space.write_failed", &temp, &error))?;
        output
            .write_all(&encoded)
            .and_then(|_| output.sync_all())
            .map_err(|error| io_error("session_space.write_failed", &temp, &error))?;
        fs::rename(&temp, path)
            .map_err(|error| io_error("session_space.commit_failed", path, &error))
    }
}

fn receipt_at<'a>(
    receipts: &'a [SessionSpaceReceipt],
    space: &SessionSpaceRef,
    sequence: u64,
) -> Result<&'a SessionSpaceReceipt> {
    receipts
        .iter()
        .find(|receipt| receipt.sequence == sequence)
        .ok_or_else(|| {
            AikitError::new(
                "session_space.history_not_found",
                format!("SessionSpace {space} has no receipt sequence {sequence}"),
            )
        })
}

fn validate_basis(
    expected: Option<&SessionSpaceBasis>,
    current: Option<&SessionSpaceAuthoredState>,
) -> Result<()> {
    let matches = match (expected, current) {
        (None, None) => true,
        (Some(expected), Some(current)) => current.basis().is_ok_and(|basis| basis == *expected),
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(AikitError::new(
            "session_space.preview_stale",
            "SessionSpace canonical state changed after the preview was accepted",
        ))
    }
}

fn space_key(space: &SessionSpaceRef) -> String {
    let digest = blake3::hash(space.to_string().as_bytes()).to_hex().to_string();
    digest[..24].to_string()
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::resource::ResourceRef;
    use aikit_core::session_space_application::{
        SessionSpaceAgentAttachmentIntent, SessionSpaceFocus,
    };

    fn store() -> (tempfile::TempDir, SessionSpaceApplicationStore) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        (dir, SessionSpaceApplicationStore::new(home))
    }

    fn create(store: &SessionSpaceApplicationStore, name: &str) -> SessionSpaceReceipt {
        let id = SessionSpaceRef::parse(&format!("session-space/{name}")).unwrap();
        let preview = store
            .stage(
                None,
                SessionSpaceMutation::Create {
                    id,
                    label: Some(name.into()),
                },
            )
            .unwrap();
        store.apply(&preview).unwrap()
    }

    #[test]
    fn preview_is_write_free_and_apply_rejects_stale_basis() {
        let (_dir, store) = store();
        let created = create(&store, "stale");
        let id = created.space;
        let target = ResourceRef::parse("project:a").unwrap();
        let stale = store
            .stage(
                Some(&id),
                SessionSpaceMutation::Focus {
                    focus: Some(SessionSpaceFocus {
                        target: target.clone(),
                        region: Some("editor".into()),
                        provenance: vec!["user".into()],
                    }),
                },
            )
            .unwrap();
        assert_eq!(store.load(&id).unwrap().revision, 0);

        let intervening = store
            .stage(
                Some(&id),
                SessionSpaceMutation::AttachAgentSession {
                    attachment: SessionSpaceAgentAttachmentIntent {
                        agent_session: ResourceRef::parse("agent-session/one").unwrap(),
                        purpose: Some("coding".into()),
                        provenance: vec!["operator".into()],
                    },
                },
            )
            .unwrap();
        store.apply(&intervening).unwrap();
        let error = store.apply(&stale).unwrap_err();
        assert_eq!(error.code(), "session_space.preview_stale");
        assert!(store.load(&id).unwrap().focus.is_none());
    }

    #[test]
    fn authored_membership_alone_makes_a_session_space_discoverable_by_project() {
        let (_dir, store) = store();
        let created = create(&store, "membership");
        let id = created.space;

        let mut target = store.load(&id).unwrap();
        target
            .definition
            .projects
            .insert(ResourceRef::parse("project:a").unwrap());
        let preview = store
            .stage(
                Some(&id),
                SessionSpaceMutation::Restore {
                    target: Box::new(target),
                    evidence: "authored membership".into(),
                },
            )
            .unwrap();
        store.apply(&preview).unwrap();

        let project = aikit_core::project::ProjectRef::parse("project:a").unwrap();
        let discovered = store.discover(Some(&project)).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].id(), &id);

        let other = aikit_core::project::ProjectRef::parse("project:b").unwrap();
        assert!(store.discover(Some(&other)).unwrap().is_empty());
    }

    #[test]
    fn canonical_semantic_state_survives_store_restart() {
        let (dir, store) = store();
        let created = create(&store, "restart");
        let id = created.space;
        let preview = store
            .stage(
                Some(&id),
                SessionSpaceMutation::AttachSurface {
                    attachment: aikit_core::session_space_application::SessionSpaceSurfaceAttachmentIntent {
                        surface: ResourceRef::parse("surface/editor").unwrap(),
                        component: None,
                        purpose: Some("primary editing surface".into()),
                        provenance: vec!["authored".into()],
                    },
                },
            )
            .unwrap();
        store.apply(&preview).unwrap();
        drop(store);

        let reopened = SessionSpaceApplicationStore::new(AikitHome::at(dir.path()));
        let state = reopened.load(&id).unwrap();
        assert!(state
            .surfaces
            .contains_key(&ResourceRef::parse("surface/editor").unwrap()));
        assert_eq!(reopened.history(&id).unwrap().len(), 2);
    }

    #[test]
    fn history_restore_is_staged_through_current_authority() {
        let (_dir, store) = store();
        let created = create(&store, "restore");
        let id = created.space;
        let agent = ResourceRef::parse("agent-session/one").unwrap();
        let attach = store
            .stage(
                Some(&id),
                SessionSpaceMutation::AttachAgentSession {
                    attachment: SessionSpaceAgentAttachmentIntent {
                        agent_session: agent.clone(),
                        purpose: None,
                        provenance: vec!["authored".into()],
                    },
                },
            )
            .unwrap();
        let attached = store.apply(&attach).unwrap();
        let detach = store
            .stage(
                Some(&id),
                SessionSpaceMutation::DetachAgentSession {
                    agent_session: agent.clone(),
                },
            )
            .unwrap();
        store.apply(&detach).unwrap();
        assert!(!store.load(&id).unwrap().agent_sessions.contains_key(&agent));

        let restore = store.stage_restore(&id, attached.sequence).unwrap();
        let restored = store.apply(&restore).unwrap();
        assert!(restored.after.agent_sessions.contains_key(&agent));
        assert_eq!(store.history(&id).unwrap().len(), 4);
    }
}
