//! SessionSpace operations projected through the canonical V2 `ApplicationService`.
//!
//! This is an extension trait, not another service or state machine. It delegates
//! directly to the same `PaletteBackend` instance the final TUI surface already
//! uses, whose SessionSpace methods resolve to `SessionSpaceApplicationStore`.

use aikit_core::project::ProjectRef;
use aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};
use aikit_core::session_space_application::{
    AgentSessionContinuityEvidence, SessionSpaceAuthoredState, SessionSpaceMutation,
    SessionSpaceNativeObservation, SessionSpacePreview, SessionSpaceReconstructionReport,
};
use aikit_core::Result;
use aikit_store::{
    SessionSpaceExplainEvidence, SessionSpaceHistoryComparison, SessionSpaceReceipt,
};

use crate::application_service::ApplicationService;

pub trait SessionSpaceApplicationProjection {
    fn session_space_list(&self) -> Result<Vec<SessionSpaceAuthoredState>>;
    fn session_space_show(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState>;
    fn session_space_open(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState>;
    fn session_space_discover(
        &self,
        project: Option<&ProjectRef>,
    ) -> Result<Vec<SessionSpaceAuthoredState>>;
    fn session_space_stage(
        &self,
        space: Option<&SessionSpaceRef>,
        intent: SessionSpaceMutation,
    ) -> Result<SessionSpacePreview>;
    fn session_space_apply(&mut self, preview: &SessionSpacePreview)
        -> Result<SessionSpaceReceipt>;
    fn session_space_history(&self, space: &SessionSpaceRef) -> Result<Vec<SessionSpaceReceipt>>;
    fn session_space_compare_history(
        &self,
        space: &SessionSpaceRef,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<SessionSpaceHistoryComparison>;
    fn session_space_stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview>;
    fn session_space_reconstruct(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport>;
    fn session_space_reconcile(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport>;
    fn session_space_explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplainEvidence>;
}

impl SessionSpaceApplicationProjection for ApplicationService<'_> {
    fn session_space_list(&self) -> Result<Vec<SessionSpaceAuthoredState>> {
        self.backend().session_space_list()
    }

    fn session_space_show(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        self.backend().session_space_show(space)
    }

    fn session_space_open(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        self.backend().session_space_open(space)
    }

    fn session_space_discover(
        &self,
        project: Option<&ProjectRef>,
    ) -> Result<Vec<SessionSpaceAuthoredState>> {
        self.backend().session_space_discover(project)
    }

    fn session_space_stage(
        &self,
        space: Option<&SessionSpaceRef>,
        intent: SessionSpaceMutation,
    ) -> Result<SessionSpacePreview> {
        self.backend().session_space_stage(space, intent)
    }

    fn session_space_apply(
        &mut self,
        preview: &SessionSpacePreview,
    ) -> Result<SessionSpaceReceipt> {
        self.backend_mut().session_space_apply(preview)
    }

    fn session_space_history(&self, space: &SessionSpaceRef) -> Result<Vec<SessionSpaceReceipt>> {
        self.backend().session_space_history(space)
    }

    fn session_space_compare_history(
        &self,
        space: &SessionSpaceRef,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<SessionSpaceHistoryComparison> {
        self.backend()
            .session_space_compare_history(space, from_sequence, to_sequence)
    }

    fn session_space_stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview> {
        self.backend().session_space_stage_restore(space, sequence)
    }

    fn session_space_reconstruct(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.backend()
            .session_space_reconstruct(space, runtime, native_observations, continuity)
    }

    fn session_space_reconcile(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.backend()
            .session_space_reconcile(space, runtime, native_observations, continuity)
    }

    fn session_space_explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplainEvidence> {
        self.backend().session_space_explain(space, reconstruction)
    }
}
