//! Thin TUI/application projection over the canonical SessionSpace store.
//!
//! This is deliberately not a SessionSpace controller or semantic service. It
//! holds no state, performs no resolution, and owns no history. Every operation
//! delegates to the shared core/store authority and returns serializable read
//! models suitable for the existing TUI application surface.

use aikit_core::project::ProjectRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceExplanation, SessionSpaceMutation, SessionSpacePreview,
    SessionSpaceReconstructionReport,
};
use aikit_core::Result;
use aikit_store::{
    SessionSpaceApplicationStore, SessionSpaceHistoryComparison, SessionSpaceReceipt,
};
use serde_json::{to_value, Value};

pub struct SessionSpaceApplicationAdapter<'a> {
    store: &'a SessionSpaceApplicationStore,
}

impl<'a> SessionSpaceApplicationAdapter<'a> {
    pub fn new(store: &'a SessionSpaceApplicationStore) -> Self {
        Self { store }
    }

    pub fn list(&self) -> Result<Value> {
        json_value(self.store.list()?)
    }

    pub fn show(&self, space: &SessionSpaceRef) -> Result<Value> {
        json_value(self.store.load(space)?)
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
        self.store.compare_history(space, from_sequence, to_sequence)
    }

    pub fn stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview> {
        self.store.stage_restore(space, sequence)
    }

    pub fn explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplanation> {
        self.store.explain(space, reconstruction)
    }
}

fn json_value(value: impl serde::Serialize) -> Result<Value> {
    to_value(value).map_err(|error| {
        aikit_core::AikitError::new(
            "session_space.projection_unserializable",
            format!("could not project SessionSpace application data: {error}"),
        )
    })
}