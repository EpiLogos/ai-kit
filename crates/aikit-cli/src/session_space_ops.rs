//! CLI-side projection of the shared SessionSpace application contract.
//!
//! This module does not resolve, persist, reconstruct or explain anything on its
//! own. It gives the CLI crate the same typed operation surface used by the TUI
//! adapter and native Skill, with `SessionSpaceApplicationStore` remaining the
//! canonical apply/history authority.

use aikit_core::project::ProjectRef;
use aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};
use aikit_core::session_space_application::{
    AgentSessionContinuityEvidence, SessionSpaceMutation, SessionSpaceNativeObservation,
    SessionSpacePreview, SessionSpaceReconstructionReport,
};
use aikit_core::Result;
use aikit_store::{
    explain_session_space_with_receipts, SessionSpaceApplicationStore, SessionSpaceExplainEvidence,
    SessionSpaceHistoryComparison, SessionSpaceReceipt,
};
use serde_json::{to_value, Value};

pub struct SessionSpaceCliAdapter<'a> {
    store: &'a SessionSpaceApplicationStore,
}

impl<'a> SessionSpaceCliAdapter<'a> {
    pub fn new(store: &'a SessionSpaceApplicationStore) -> Self {
        Self { store }
    }

    pub fn list(&self) -> Result<Value> {
        json_value(self.store.list()?)
    }

    pub fn show(&self, space: &SessionSpaceRef) -> Result<Value> {
        json_value(self.store.load(space)?)
    }

    /// `open` is semantic reopening of persisted state. It never claims that a
    /// provider-native workspace/pane/session was recreated.
    pub fn open(&self, space: &SessionSpaceRef) -> Result<Value> {
        self.show(space)
    }

    pub fn discover(&self, project: Option<&ProjectRef>) -> Result<Value> {
        json_value(self.store.discover(project)?)
    }

    pub fn stage(
        &self,
        space: Option<&SessionSpaceRef>,
        intent: SessionSpaceMutation,
    ) -> Result<SessionSpacePreview> {
        self.store.stage(space, intent)
    }

    pub fn apply(&self, preview: &SessionSpacePreview) -> Result<SessionSpaceReceipt> {
        self.store.apply(preview)
    }

    pub fn history(&self, space: &SessionSpaceRef) -> Result<Value> {
        json_value(self.store.history(space)?)
    }

    pub fn compare_history(
        &self,
        space: &SessionSpaceRef,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<SessionSpaceHistoryComparison> {
        self.store
            .compare_history(space, from_sequence, to_sequence)
    }

    pub fn stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview> {
        self.store.stage_restore(space, sequence)
    }

    pub fn reconstruct(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.store
            .reconstruct(space, runtime, native_observations, continuity)
    }

    pub fn reconcile(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.reconstruct(space, runtime, native_observations, continuity)
    }

    pub fn explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplainEvidence> {
        explain_session_space_with_receipts(self.store, space, reconstruction)
    }
}

fn json_value(value: impl serde::Serialize) -> Result<Value> {
    to_value(value).map_err(|error| {
        aikit_core::AikitError::new(
            "session_space.cli_projection_unserializable",
            format!("could not project SessionSpace CLI data: {error}"),
        )
    })
}
