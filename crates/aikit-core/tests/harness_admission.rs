use aikit_core::{
    unsupported_harness_gap, verify_activation_truth, ActivationEffect, FacultySupport,
    HarnessActivationObservation, HarnessActivationState, HarnessAdmissionAdapter,
    HarnessAdmissionDescriptor, HarnessEditionKind, HarnessFaculty, HarnessFacultyObservation,
    ProjectionPlan, ResolvedContext, TargetAdapter, TargetCapabilities, TargetId,
    HARNESS_ADAPTER_SDK_VERSION,
};

struct ExternalFixtureAdapter;

impl TargetAdapter for ExternalFixtureAdapter {
    fn target(&self) -> TargetId {
        TargetId::new("fixture-harness")
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            live_reload: false,
            symlinks: false,
            isolated_per_context: true,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> aikit_core::Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::next_session_only("fixture reads project guidance at session start"),
        ))
    }

    fn activation_effect(
        &self,
        _old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        new.effect.clone()
    }
}

impl HarnessAdmissionAdapter for ExternalFixtureAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor {
        HarnessAdmissionDescriptor {
            schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
            adapter_ref: "adapter/external-fixture".to_string(),
            adapter_version: "1.0.0".to_string(),
            target: self.target(),
            product: "External Fixture Harness".to_string(),
            edition: HarnessEditionKind::Cli,
            native_version: Some("0.7.0".to_string()),
            source_revision: Some("fixture-source-revision".to_string()),
            realised_actuation_ref: Some("actuation:realised/fixture".to_string()),
            project_binding_ref: Some("project/fixture".to_string()),
            faculties: vec![
                HarnessFacultyObservation {
                    faculty: HarnessFaculty::ProjectInstructions,
                    support: FacultySupport::Supported,
                    evidence_refs: vec!["source/fixture/project-instructions".to_string()],
                    note: None,
                },
                HarnessFacultyObservation {
                    faculty: HarnessFaculty::LiveReload,
                    support: FacultySupport::Unsupported,
                    evidence_refs: vec![],
                    note: Some("target reads at next session".to_string()),
                },
            ],
        }
    }
}

#[test]
fn an_external_style_adapter_uses_existing_target_projection_plus_public_admission() {
    let adapter = ExternalFixtureAdapter;
    let admission = adapter.admission();
    admission.validate().unwrap();
    assert_eq!(admission.target.as_str(), "fixture-harness");
    assert_eq!(
        admission.faculty(HarnessFaculty::LiveReload).unwrap().support,
        FacultySupport::Unsupported
    );
}

#[test]
fn supported_faculties_require_evidence() {
    let mut admission = ExternalFixtureAdapter.admission();
    admission.faculties[0].evidence_refs.clear();
    assert!(admission.validate().is_err());
}

#[test]
fn generated_material_cannot_be_called_loaded_without_target_evidence() {
    let plan = ProjectionPlan::new(
        TargetId::new("fixture-harness"),
        ActivationEffect::live(),
    );
    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: plan.target.clone(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec![],
        native_revision: None,
        note: None,
    };
    assert!(verify_activation_truth(&plan, &observation).is_err());
}

#[test]
fn next_session_targets_cannot_be_overstated_as_loaded_now() {
    let plan = ProjectionPlan::new(
        TargetId::new("fixture-harness"),
        ActivationEffect::next_session_only("reads at session start"),
    );
    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: plan.target.clone(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec!["observation/fixture/current-process".to_string()],
        native_revision: Some("fixture-native-rev".to_string()),
        note: None,
    };
    assert!(verify_activation_truth(&plan, &observation).is_err());
}

#[test]
fn a_truthful_next_session_observation_is_valid() {
    let plan = ProjectionPlan::new(
        TargetId::new("fixture-harness"),
        ActivationEffect::next_session_only("reads at session start"),
    );
    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: plan.target.clone(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::NextSession,
        evidence_refs: vec!["source/fixture/lifecycle".to_string()],
        native_revision: None,
        note: None,
    };
    verify_activation_truth(&plan, &observation).unwrap();
}

#[test]
fn unsupported_targets_return_an_actionable_public_extension_path() {
    let gap = unsupported_harness_gap(
        TargetId::new("unknown-editor"),
        "Unknown Editor",
        HarnessEditionKind::Ide,
        Some("4.2".to_string()),
        vec![HarnessFacultyObservation {
            faculty: HarnessFaculty::ProjectInstructions,
            support: FacultySupport::Unknown,
            evidence_refs: vec![],
            note: Some("visible project configuration discovered; no accepted adapter".to_string()),
        }],
    )
    .unwrap();

    assert_eq!(gap.missing_contract, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(gap.sdk_ref, "aikit:harness-adapter-sdk/v1");
    assert_eq!(gap.authoring_skill_ref, "skill/aikit/harness-adapter-authoring");
    assert!(gap.required_conformance.contains(&"activation-reload-truth".to_string()));
}
