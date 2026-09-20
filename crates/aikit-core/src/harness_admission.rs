//! Public heterogeneous-harness admission and activation-truth contract.
//!
//! This module extends the existing [`TargetAdapter`] planning seam; it does not
//! create a second resolver, projection engine, package system, or Harness
//! ontology. An adapter describes the native faculties an installed target
//! actually exposes. Provider/application code may then produce activation
//! observations after the existing projection lifecycle runs.

use serde::{Deserialize, Serialize};

use crate::platform::TargetId;
use crate::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};
use crate::{AikitError, Result};

pub const HARNESS_ADAPTER_SDK_VERSION: &str = "aikit.harness-adapter/v1";
pub const HARNESS_ADAPTER_AUTHORING_SKILL: &str = "skill/aikit/harness-adapter-authoring";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessEditionKind {
    Cli,
    Desktop,
    Ide,
    Hosted,
    Embedded,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessFaculty {
    StandingInstructions,
    ProjectInstructions,
    NativeSkills,
    SessionStartHook,
    LiveReload,
    NextSessionReload,
    RestartReload,
    ToolProtocol,
    NativeToolContribution,
    SessionResume,
    DelegatedAgents,
    ProjectRoots,
    Components,
    Surfaces,
    LiveRetraction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FacultySupport {
    Supported,
    Degraded,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessFacultyObservation {
    pub faculty: HarnessFaculty,
    pub support: FacultySupport,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl HarnessFacultyObservation {
    pub fn validate(&self) -> Result<()> {
        if self.support == FacultySupport::Supported && self.evidence_refs.is_empty() {
            return Err(AikitError::new(
                "harness_admission.support_without_evidence",
                "a supported harness faculty must carry observed/source evidence",
            ));
        }
        validate_refs(&self.evidence_refs, "faculty evidence")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessAdmissionDescriptor {
    pub schema: String,
    pub adapter_ref: String,
    pub adapter_version: String,
    pub target: TargetId,
    pub product: String,
    pub edition: HarnessEditionKind,
    #[serde(default)]
    pub native_version: Option<String>,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub realised_actuation_ref: Option<String>,
    #[serde(default)]
    pub project_binding_ref: Option<String>,
    pub faculties: Vec<HarnessFacultyObservation>,
}

impl HarnessAdmissionDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.schema != HARNESS_ADAPTER_SDK_VERSION {
            return Err(AikitError::new(
                "harness_admission.schema_mismatch",
                format!("harness adapter schema must be {HARNESS_ADAPTER_SDK_VERSION}"),
            ));
        }
        for (name, value) in [
            ("adapter_ref", self.adapter_ref.as_str()),
            ("adapter_version", self.adapter_version.as_str()),
            ("product", self.product.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(AikitError::new(
                    "harness_admission.empty_identity",
                    format!("{name} must be non-empty"),
                ));
            }
        }
        if let Some(value) = &self.realised_actuation_ref {
            validate_ref(value, "realised_actuation_ref")?;
        }
        if let Some(value) = &self.project_binding_ref {
            validate_ref(value, "project_binding_ref")?;
        }
        if self.faculties.is_empty() {
            return Err(AikitError::new(
                "harness_admission.faculties_empty",
                "admission must say what was observed, including explicit unsupported faculties where relevant",
            ));
        }
        for faculty in &self.faculties {
            faculty.validate()?;
        }
        Ok(())
    }

    pub fn faculty(&self, faculty: HarnessFaculty) -> Option<&HarnessFacultyObservation> {
        self.faculties.iter().find(|entry| entry.faculty == faculty)
    }
}

/// Public extension trait: a harness admission adapter is the existing target
/// projection adapter plus an evidence-backed description of this target/edition.
/// Planning remains [`TargetAdapter::plan`]; no second projection engine exists.
pub trait HarnessAdmissionAdapter: TargetAdapter {
    fn admission(&self) -> HarnessAdmissionDescriptor;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessLifecyclePhase {
    Discover,
    Plan,
    Project,
    ActivateOrReload,
    Verify,
    Explain,
    UpdateOrRetract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessActivationState {
    Loaded,
    ReloadPending,
    RestartRequired,
    NextSession,
    Brokered,
    Unsupported,
    Retracted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessActivationObservation {
    pub schema: String,
    pub target: TargetId,
    pub projection_digest: String,
    pub state: HarnessActivationState,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub native_revision: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl HarnessActivationObservation {
    pub fn validate_against(&self, plan: &ProjectionPlan) -> Result<()> {
        if self.schema != HARNESS_ADAPTER_SDK_VERSION {
            return Err(AikitError::new(
                "harness_activation.schema_mismatch",
                format!("activation observation schema must be {HARNESS_ADAPTER_SDK_VERSION}"),
            ));
        }
        if self.target != plan.target {
            return Err(AikitError::new(
                "harness_activation.target_mismatch",
                "activation observation target differs from the projected target",
            ));
        }
        if self.projection_digest != plan.digest() {
            return Err(AikitError::new(
                "harness_activation.projection_mismatch",
                "activation observation does not refer to the exact projected generation",
            ));
        }
        validate_refs(&self.evidence_refs, "activation evidence")?;
        if self.state == HarnessActivationState::Loaded && self.evidence_refs.is_empty() {
            return Err(AikitError::new(
                "harness_activation.loaded_without_evidence",
                "generated/projected material is not proof that a running target loaded it",
            ));
        }
        Ok(())
    }
}

/// Check that an observed activation state does not overstate the target-native
/// lifecycle promised by the existing projection adapter.
pub fn verify_activation_truth(
    plan: &ProjectionPlan,
    observation: &HarnessActivationObservation,
) -> Result<()> {
    observation.validate_against(plan)?;

    let impossible = matches!(
        (&plan.effect, observation.state),
        (
            ActivationEffect::Unsupported { .. },
            HarnessActivationState::Loaded
        ) | (
            ActivationEffect::Brokered { .. },
            HarnessActivationState::Loaded
        ) | (
            ActivationEffect::NextSessionOnly { .. },
            HarnessActivationState::Loaded
        ) | (
            ActivationEffect::RestartClient { .. },
            HarnessActivationState::Loaded
        )
    );
    if impossible {
        return Err(AikitError::new(
            "harness_activation.lifecycle_overclaim",
            "observed activation state overstates the adapter's evidence-backed native lifecycle",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompatibilityGap {
    pub schema: String,
    pub target: TargetId,
    pub product: String,
    pub edition: HarnessEditionKind,
    #[serde(default)]
    pub native_version: Option<String>,
    #[serde(default)]
    pub visible_faculties: Vec<HarnessFacultyObservation>,
    pub missing_contract: String,
    pub sdk_ref: String,
    pub authoring_skill_ref: String,
    pub required_conformance: Vec<String>,
}

pub fn unsupported_harness_gap(
    target: TargetId,
    product: impl Into<String>,
    edition: HarnessEditionKind,
    native_version: Option<String>,
    visible_faculties: Vec<HarnessFacultyObservation>,
) -> Result<HarnessCompatibilityGap> {
    for faculty in &visible_faculties {
        faculty.validate()?;
    }
    Ok(HarnessCompatibilityGap {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target,
        product: product.into(),
        edition,
        native_version,
        visible_faculties,
        missing_contract: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        sdk_ref: "aikit:harness-adapter-sdk/v1".to_string(),
        authoring_skill_ref: HARNESS_ADAPTER_AUTHORING_SKILL.to_string(),
        required_conformance: vec![
            "source-ownership".to_string(),
            "idempotent-projection".to_string(),
            "authored-file-conflict".to_string(),
            "activation-reload-truth".to_string(),
            "update-retract".to_string(),
            "unavailable-faculties".to_string(),
            "identity-non-collapse".to_string(),
            "explain-provenance".to_string(),
        ],
    })
}

fn validate_ref(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AikitError::new(
            "harness_admission.empty_ref",
            format!("{name} must be a non-empty ref"),
        ));
    }
    Ok(())
}

fn validate_refs(values: &[String], name: &str) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(AikitError::new(
            "harness_admission.empty_ref",
            format!("{name} cannot contain empty refs"),
        ));
    }
    Ok(())
}
