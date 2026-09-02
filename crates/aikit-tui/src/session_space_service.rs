//! SessionSpace operations projected through the canonical V2 `ApplicationService`.
//!
//! This is an extension trait, not another service or state machine. It delegates
//! directly to the same `PaletteBackend` instance the final TUI surface already
//! uses, whose SessionSpace methods resolve to `SessionSpaceApplicationStore`.

use aikit_core::project::ProjectRef;
use aikit_core::resource::{ResourceDescriptor, ResourceKind, ResourceRecord, ResourceSearchIndex};
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

/// Project authored SessionSpaces into AIKit's common Resource navigation field.
///
/// This creates no SessionSpace state and no provider-specific identity. The
/// canonical `SessionSpaceRef` is reused as the ResourceRef so TUI, Agent and
/// application consumers can co-refer through the same Resource/Action grammar.
pub fn install_session_space_navigation_resources(
    index: &mut ResourceSearchIndex,
    states: &[SessionSpaceAuthoredState],
) {
    for state in states {
        let resource = state.id().as_resource_ref().clone();
        let label = state
            .label
            .clone()
            .unwrap_or_else(|| resource.as_str().to_string());
        let summary = format!(
            "SessionSpace revision {} · {} Project context(s) · {} AgentSession intent(s) · {} Surface intent(s) · {} native reference(s)",
            state.revision,
            state.project_contexts.len(),
            state.agent_sessions.len(),
            state.surfaces.len(),
            state.native_references.len(),
        );
        let mut descriptor = ResourceDescriptor::new(
            resource,
            ResourceKind::SessionSpace,
            label,
            summary,
        );
        descriptor.annotations.insert(
            "session-space-revision".into(),
            state.revision.to_string(),
        );
        index.insert_resource(ResourceRecord::new(descriptor), Vec::new());
    }
}

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
    fn session_space_apply(&mut self, preview: &SessionSpacePreview) -> Result<SessionSpaceReceipt>;
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

    fn session_space_apply(&mut self, preview: &SessionSpacePreview) -> Result<SessionSpaceReceipt> {
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
        self.backend().session_space_reconstruct(
            space,
            runtime,
            native_observations,
            continuity,
        )
    }

    fn session_space_reconcile(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.backend().session_space_reconcile(
            space,
            runtime,
            native_observations,
            continuity,
        )
    }

    fn session_space_explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplainEvidence> {
        self.backend().session_space_explain(space, reconstruction)
    }
}


#[cfg(test)]
mod resource_projection_tests {
    use super::*;
    use aikit_core::resource::ResourceIndex;
    use aikit_core::{
        install_explain_history_actions, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
    };

    #[test]
    fn authored_session_space_enters_common_resource_action_field_without_identity_collapse() {
        let id = SessionSpaceRef::parse("session-space/omarchy-reference").unwrap();
        let mut state = SessionSpaceAuthoredState::new(id.clone());
        state.label = Some("Omarchy reference".into());
        state.revision = 7;

        let mut index = ResourceSearchIndex::default();
        install_session_space_navigation_resources(&mut index, &[state]);

        let resource = ResourceIndex::resource(&index, id.as_resource_ref()).unwrap();
        assert_eq!(resource.descriptor.id, *id.as_resource_ref());
        assert_eq!(resource.descriptor.kind, ResourceKind::SessionSpace);
        assert_eq!(resource.descriptor.name, "Omarchy reference");
        assert_eq!(
            resource.descriptor.annotations.get("session-space-revision"),
            Some(&"7".to_string())
        );

        install_explain_history_actions(&mut index).unwrap();
        let actions = index.actions_for(id.as_resource_ref());
        assert!(actions
            .iter()
            .any(|action| action.action.as_str() == EXPLAIN_ACTION_REF));
        assert!(actions
            .iter()
            .any(|action| action.action.as_str() == HISTORY_ACTION_REF));
        assert!(actions.iter().all(|action| action.subject == *id.as_resource_ref()));

        let hits = index.search("Omarchy reference", 8);
        assert!(hits.iter().any(|hit| {
            hit.resource == *id.as_resource_ref() && hit.kind == ResourceKind::SessionSpace
        }));
    }
}
