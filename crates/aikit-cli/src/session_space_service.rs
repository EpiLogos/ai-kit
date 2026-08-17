//! SessionSpace operation family on the canonical CLI/TUI `Service`.
//!
//! The Service already owns the injected `AikitHome`; this extension uses that
//! exact home rather than rediscovering process-global state. All mutation,
//! persistence, receipt, Explain and History semantics remain in aikit-store.

use aikit_core::project::ProjectRef;
use aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};
use aikit_core::session_space_application::{
    AgentSessionContinuityEvidence, SessionSpaceAuthoredState, SessionSpaceMutation,
    SessionSpaceNativeObservation, SessionSpacePreview, SessionSpaceReconstructionReport,
};
use aikit_core::Result;
use aikit_store::{
    explain_session_space_with_receipts, SessionSpaceApplicationStore, SessionSpaceExplainEvidence,
    SessionSpaceHistoryComparison, SessionSpaceReceipt,
};

use crate::app::Service;

pub trait SessionSpaceServiceOps {
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
    fn session_space_apply(&self, preview: &SessionSpacePreview) -> Result<SessionSpaceReceipt>;
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

impl SessionSpaceServiceOps for Service {
    fn session_space_list(&self) -> Result<Vec<SessionSpaceAuthoredState>> {
        store(self).list()
    }

    fn session_space_show(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        store(self).load(space)
    }

    fn session_space_open(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        self.session_space_show(space)
    }

    fn session_space_discover(
        &self,
        project: Option<&ProjectRef>,
    ) -> Result<Vec<SessionSpaceAuthoredState>> {
        store(self).discover(project)
    }

    fn session_space_stage(
        &self,
        space: Option<&SessionSpaceRef>,
        intent: SessionSpaceMutation,
    ) -> Result<SessionSpacePreview> {
        store(self).stage(space, intent)
    }

    fn session_space_apply(&self, preview: &SessionSpacePreview) -> Result<SessionSpaceReceipt> {
        store(self).apply(preview)
    }

    fn session_space_history(&self, space: &SessionSpaceRef) -> Result<Vec<SessionSpaceReceipt>> {
        store(self).history(space)
    }

    fn session_space_compare_history(
        &self,
        space: &SessionSpaceRef,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<SessionSpaceHistoryComparison> {
        store(self).compare_history(space, from_sequence, to_sequence)
    }

    fn session_space_stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview> {
        store(self).stage_restore(space, sequence)
    }

    fn session_space_reconstruct(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        store(self).reconstruct(space, runtime, native_observations, continuity)
    }

    fn session_space_reconcile(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.session_space_reconstruct(space, runtime, native_observations, continuity)
    }

    fn session_space_explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplainEvidence> {
        let store = store(self);
        explain_session_space_with_receipts(&store, space, reconstruction)
    }
}

fn store(service: &Service) -> SessionSpaceApplicationStore {
    SessionSpaceApplicationStore::new(service.home().clone())
}
