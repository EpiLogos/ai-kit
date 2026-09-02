//! Provider-neutral Model execution/materialisation relation and application read model.
//!
//! This module is intentionally below Profile/SkillSet application composition. It
//! creates no Model, Provider or Harness registry. Canonical Model identity remains a
//! `ResourceRef`; engine and Workcell facts are execution/material provenance, while
//! resolved [`HarnessComposition`] remains component/Contract/Surface truth.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition::{HarnessComposition, RetractionMode, SurfaceKind};
use crate::resource::{ProviderRef, ResourceKind, ResourceRef};
use crate::{AikitError, Result};

pub const MODEL_RUNTIME_RELATION_VERSION: &str = "aikit.model-runtime/v1";

/// Source revisions inspected for the first comparative provider set. They are
/// evidence pins, never provider or Model identity.
pub const OLLAMA_CONFORMANCE_REVISION: &str = "48cb7b94e446bb3f32555d8e21a5552ebe463711";
pub const LLAMA_CPP_CONFORMANCE_REVISION: &str = "ce8d842306b6e206f2833e04d472cff79c3c9be1";
pub const VLLM_CONFORMANCE_REVISION: &str = "a0a3c32dd705fd447488262c757ffa18ab9e39d3";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVariantReading {
    pub model: ResourceRef,
    pub variant: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceEngineForm {
    Direct,
    LightweightServer,
    ManagedService,
    ServingRuntime,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceEngineReading {
    pub engine: ResourceRef,
    pub provider: ProviderRef,
    pub form: InferenceEngineForm,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Engine flags/configuration stay provider/material provenance.
    #[serde(default)]
    pub provider_native: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlacementObservation {
    Unknown,
    Local,
    Remote,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MaterialResourceReading {
    #[serde(default)]
    pub process: BTreeMap<String, String>,
    #[serde(default)]
    pub service: BTreeMap<String, String>,
    #[serde(default)]
    pub storage: BTreeMap<String, String>,
    #[serde(default)]
    pub accelerator: BTreeMap<String, String>,
    #[serde(default)]
    pub network: BTreeMap<String, String>,
    #[serde(default)]
    pub lifecycle: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMaterialisationReading {
    /// Opaque current material binding; a Workcell binding may occupy this field but
    /// can never become Model/Harness identity.
    pub binding_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workcell_ref: Option<String>,
    pub placement: PlacementObservation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Provider-native model IDs, endpoint facts, ports, PIDs and engine state.
    #[serde(default)]
    pub provider_native: BTreeMap<String, String>,
    #[serde(default)]
    pub resources: MaterialResourceReading,
    pub lifetime_owner: String,
    pub retraction: RetractionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AccessFieldReading {
    Available {
        #[serde(default)]
        capabilities: BTreeSet<String>,
    },
    Unavailable {
        reason: String,
    },
}

impl AccessFieldReading {
    pub fn available(capabilities: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Available {
            capabilities: capabilities.into_iter().map(Into::into).collect(),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// These axes are independent: inference availability grants neither material
/// control nor model-interior/research access by implication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAccessReading {
    pub inference: AccessFieldReading,
    pub material_control: AccessFieldReading,
    pub interior: AccessFieldReading,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSurfaceReading {
    /// Contract consumed by the Harness. A thin/direct target may legitimately have
    /// no addressable composition Contract; absence is disclosed rather than faked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<ResourceRef>,
    pub protocol: String,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    pub access: ModelAccessReading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeChangeApplication {
    Live,
    NextSession,
    Restart,
    Generation,
    ProcedureMediated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRuntimeRelation {
    pub model: ModelVariantReading,
    pub engine: InferenceEngineReading,
    pub materialisation: ModelMaterialisationReading,
    pub model_surface: ModelSurfaceReading,
    pub change_application: RuntimeChangeApplication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractProviderReading {
    pub consumer_component: ResourceRef,
    pub contract: ResourceRef,
    pub provider: ResourceRef,
    pub reactive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSurfaceReading {
    pub surface: ResourceRef,
    pub kind: SurfaceKind,
    /// Canonical Actions projected here. One Action may occur on several Surfaces.
    #[serde(default)]
    pub action_refs: Vec<ResourceRef>,
    /// Non-Action readings/resources stay explicitly non-Action.
    #[serde(default)]
    pub non_action_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub retraction_modes: BTreeSet<RetractionMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeUnavailability {
    pub field: String,
    pub reason: String,
}

/// UI-neutral disclosure for CLI/TUI/agent application consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRuntimeReadModel {
    pub version: String,
    pub project: Option<ResourceRef>,
    pub agent: Option<ResourceRef>,
    pub agency: Option<ResourceRef>,
    pub harness: ResourceRef,
    pub agent_session: Option<String>,
    pub harness_composition_fingerprint: String,
    pub relation: ModelRuntimeRelation,
    #[serde(default)]
    pub components: Vec<ResourceRef>,
    #[serde(default)]
    pub contracts: Vec<ContractProviderReading>,
    #[serde(default)]
    pub surfaces: Vec<RuntimeSurfaceReading>,
    #[serde(default)]
    pub unavailable: Vec<RuntimeUnavailability>,
}

/// Attach provider/material observation to an already-resolved Harness body.
/// Provider/material rebinding can change the body facts, but cannot manufacture a
/// different Model, Harness, Project, Agent, Agency or AgentSession identity.
pub fn disclose_model_runtime(
    composition: &HarnessComposition,
    relation: ModelRuntimeRelation,
) -> Result<ModelRuntimeReadModel> {
    if let Some(selected_model) = &composition.model {
        if selected_model != &relation.model.model {
            return Err(AikitError::new(
                "model_runtime.model_identity_mismatch",
                format!(
                    "HarnessComposition selected Model {selected_model} but material relation describes {}",
                    relation.model.model
                ),
            ));
        }
    }

    let mut unavailable = composition
        .absences
        .iter()
        .map(|absence| RuntimeUnavailability {
            field: absence.requirement.to_string(),
            reason: absence.reason.clone(),
        })
        .collect::<Vec<_>>();
    for (field, access) in [
        ("inference-access", &relation.model_surface.access.inference),
        (
            "material-control-access",
            &relation.model_surface.access.material_control,
        ),
        (
            "model-interior-access",
            &relation.model_surface.access.interior,
        ),
    ] {
        if let AccessFieldReading::Unavailable { reason } = access {
            unavailable.push(RuntimeUnavailability {
                field: field.to_string(),
                reason: reason.clone(),
            });
        }
    }
    if relation.model_surface.contract.is_none() {
        unavailable.push(RuntimeUnavailability {
            field: "inference-contract".to_string(),
            reason: "thin/direct target has no addressable HarnessComposition inference Contract"
                .to_string(),
        });
    }

    let mut components = composition
        .component_bindings
        .iter()
        .map(|binding| binding.component.clone())
        .collect::<Vec<_>>();
    components.sort();

    let mut contracts = composition
        .contract_bindings
        .iter()
        .map(|binding| ContractProviderReading {
            consumer_component: binding.consumer_component.clone(),
            contract: binding.contract.clone(),
            provider: binding.provider.clone(),
            reactive: binding.reactive,
        })
        .collect::<Vec<_>>();
    contracts.sort_by(|left, right| {
        (&left.consumer_component, &left.contract)
            .cmp(&(&right.consumer_component, &right.contract))
    });

    let mut surfaces = composition
        .surfaces
        .iter()
        .map(|surface| {
            let mut action_refs = Vec::new();
            let mut non_action_refs = Vec::new();
            for projection in composition
                .projections
                .iter()
                .filter(|projection| projection.surface == surface.resource)
            {
                if projection.canonical_kind == ResourceKind::Action {
                    action_refs.push(projection.canonical_ref.clone());
                } else {
                    non_action_refs.push(projection.canonical_ref.clone());
                }
            }
            action_refs.sort();
            non_action_refs.sort();
            let retraction_modes = composition
                .contributions
                .iter()
                .filter(|contribution| contribution.surface.as_ref() == Some(&surface.resource))
                .map(|contribution| contribution.retraction_mode)
                .collect();
            RuntimeSurfaceReading {
                surface: surface.resource.clone(),
                kind: surface.kind,
                action_refs,
                non_action_refs,
                retraction_modes,
            }
        })
        .collect::<Vec<_>>();
    surfaces.sort_by(|left, right| left.surface.cmp(&right.surface));
    unavailable
        .sort_by(|left, right| (&left.field, &left.reason).cmp(&(&right.field, &right.reason)));

    Ok(ModelRuntimeReadModel {
        version: MODEL_RUNTIME_RELATION_VERSION.to_string(),
        project: composition.project.clone(),
        agent: composition.agent.clone(),
        agency: composition.agency.clone(),
        harness: composition.harness.clone(),
        agent_session: composition.session.clone(),
        harness_composition_fingerprint: composition.fingerprint.clone(),
        relation,
        components,
        contracts,
        surfaces,
        unavailable,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProviderConformanceFixture {
    pub provider: &'static str,
    pub upstream_revision: &'static str,
    pub engine_form: InferenceEngineForm,
    pub daemon_required: bool,
    pub material_shape: &'static str,
}

/// Current source-pinned comparative shapes. Workcell owns actual acquisition,
/// process/service start/stop, storage, accelerator, network and lifecycle.
pub fn model_provider_conformance_fixtures() -> [ModelProviderConformanceFixture; 3] {
    [
        ModelProviderConformanceFixture {
            provider: "ollama",
            upstream_revision: OLLAMA_CONFORMANCE_REVISION,
            engine_form: InferenceEngineForm::ManagedService,
            daemon_required: true,
            material_shape: "managed local service with model acquisition/runtime lifecycle",
        },
        ModelProviderConformanceFixture {
            provider: "llama.cpp",
            upstream_revision: LLAMA_CPP_CONFORMANCE_REVISION,
            engine_form: InferenceEngineForm::Direct,
            daemon_required: false,
            material_shape: "direct CLI/in-process engine or optional lightweight llama-server",
        },
        ModelProviderConformanceFixture {
            provider: "vllm",
            upstream_revision: VLLM_CONFORMANCE_REVISION,
            engine_form: InferenceEngineForm::ServingRuntime,
            daemon_required: false,
            material_shape:
                "rich serving runtime whose placement may expand across accelerators/hosts",
        },
    ]
}
