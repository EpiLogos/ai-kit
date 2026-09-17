//! Provider-neutral Model execution/materialisation relation and application read model.
//!
//! This module is intentionally below Profile/SkillSet application composition. It
//! creates no Model, Provider or Harness registry. Canonical Model identity remains a
//! `ResourceRef`; engine and Workcell facts are execution/material provenance, while
//! resolved [`HarnessComposition`] remains component/Contract/Surface truth.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition::{HarnessComposition, RetractionMode, SurfaceKind};
use crate::model_modality::{
    compose_stage_modalities, surface_interaction_support, surface_modality_support,
    ComposedModalityView, InteractionCapability, ModalityDirection, ModalitySupport,
    ModelModalityContract, ModelModality,
};
use crate::resource::{ProviderRef, ResourceKind, ResourceRef};
use crate::{AikitError, Result};

pub const MODEL_RUNTIME_RELATION_VERSION: &str = "aikit.model-runtime/v1";

/// Source revisions inspected for the first comparative provider set. They are
/// evidence pins, never provider or Model identity.
pub const OLLAMA_CONFORMANCE_REVISION: &str =
    "48cb7b94e446bb3f32555d8e21a5552ebe463711";
pub const LLAMA_CPP_CONFORMANCE_REVISION: &str =
    "ce8d842306b6e206f2833e04d472cff79c3c9be1";
pub const VLLM_CONFORMANCE_REVISION: &str =
    "a0a3c32dd705fd447488262c757ffa18ab9e39d3";

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
    Unavailable { reason: String },
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
    /// The surface's modality/interaction/transport contract, when the
    /// surface declares one. `None` is a fact about declaration, not about
    /// capability: queries through [`ModelRuntimeReadModel`] answer unknown
    /// rather than unsupported, keeping unproven and proven-absent distinct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modality: Option<ModelModalityContract>,
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
        ("material-control-access", &relation.model_surface.access.material_control),
        ("model-interior-access", &relation.model_surface.access.interior),
    ] {
        if let AccessFieldReading::Unavailable { reason } = access {
            unavailable.push(RuntimeUnavailability {
                field: field.to_string(),
                reason: reason.clone(),
            });
        }
    }
    // A degraded or unavailable modality surface is an availability fact in
    // its own right, not silently absorbed into the capability sets.
    if let Some(modality) = &relation.model_surface.modality {
        if let Err(error) = modality.validate() {
            return Err(error.with("surface", relation.model_surface.protocol.clone()));
        }
        if let crate::resource::CredentialCondition::Required { hint } = &modality.credential {
            unavailable.push(RuntimeUnavailability {
                field: "modality-credential".to_string(),
                reason: format!("the surface needs a credential it does not have bound: {hint}"),
            });
        }
        match &modality.availability {
            crate::model_modality::SurfaceAvailability::Available => {}
            crate::model_modality::SurfaceAvailability::Degraded { reason } => {
                unavailable.push(RuntimeUnavailability {
                    field: "modality-availability".to_string(),
                    reason: format!("degraded: {reason}"),
                });
            }
            crate::model_modality::SurfaceAvailability::Unavailable { reason } => {
                unavailable.push(RuntimeUnavailability {
                    field: "modality-availability".to_string(),
                    reason: reason.clone(),
                });
            }
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
        (&left.consumer_component, &left.contract).cmp(&(&right.consumer_component, &right.contract))
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
    unavailable.sort_by(|left, right| (&left.field, &left.reason).cmp(&(&right.field, &right.reason)));

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

impl ModelRuntimeReadModel {
    /// The surface's modality contract, when one was declared.
    pub fn modality(&self) -> Option<&ModelModalityContract> {
        self.relation.model_surface.modality.as_ref()
    }

    /// Whether the resolved body carries interactive speech in both
    /// directions. `None` when no modality contract was declared: unproven,
    /// not refuted.
    pub fn speech_capable(&self) -> Option<bool> {
        self.modality().map(|contract| contract.is_speech_capable())
    }

    /// Explicit state of one interaction capability on this body.
    pub fn interaction_support(&self, capability: InteractionCapability) -> ModalitySupport {
        surface_interaction_support(self.modality(), capability)
    }

    /// Explicit state of one transform capability on this body.
    pub fn transform_support(
        &self,
        capability: crate::model_modality::TransformCapability,
    ) -> ModalitySupport {
        match self.modality() {
            Some(contract) => contract.transform_support(capability),
            None => ModalitySupport::Unknown {
                reason: "no modality contract was declared for this surface".to_string(),
            },
        }
    }

    /// Explicit state of one input/output modality on this body.
    pub fn modality_support(
        &self,
        direction: ModalityDirection,
        modality: ModelModality,
    ) -> ModalitySupport {
        surface_modality_support(self.modality(), direction, modality)
    }
}

pub const MODEL_STAGE_RUNTIME_VERSION: &str = "aikit.model-stage-runtime/v1";

/// One model-bearing stage of a composed body, in body order. Each stage
/// keeps its own complete model/provider/engine/materialisation relation —
/// a cascade never collapses its stages into one fictional provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStageRelation {
    /// The model-bearing Component of the resolved composition this stage
    /// binds to. The component is in the composition's `component_bindings`;
    /// the binding is the only place stage identity comes from.
    pub component: ResourceRef,
    pub relation: ModelRuntimeRelation,
}

/// The read model of a multi-model body: a resolved HarnessComposition whose
/// model-bearing components each carry their own runtime relation (for
/// example speech-to-text surface, text reasoning harness, text-to-speech
/// surface). Body/provider replacement across any stage changes the stage
/// facts and the fingerprint — never the Project, Agent, Agency, Harness or
/// AgentSession identity the body belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedModelRuntimeReadModel {
    pub version: String,
    pub project: Option<ResourceRef>,
    pub agent: Option<ResourceRef>,
    pub agency: Option<ResourceRef>,
    pub harness: ResourceRef,
    pub agent_session: Option<String>,
    pub harness_composition_fingerprint: String,
    pub stages: Vec<ModelStageRelation>,
    /// The derived body-level modality view, with the stage-named basis of
    /// every derived answer.
    pub composed_modality: ComposedModalityView,
    #[serde(default)]
    pub unavailable: Vec<RuntimeUnavailability>,
}

impl StagedModelRuntimeReadModel {
    pub fn stage(&self, component: &ResourceRef) -> Option<&ModelStageRelation> {
        self.stages.iter().find(|stage| &stage.component == component)
    }

    pub fn interaction_support(&self, capability: InteractionCapability) -> ModalitySupport {
        self.composed_modality.interaction_support(capability)
    }

    pub fn speech_capable(&self) -> bool {
        self.composed_modality.speech_capable
    }
}

/// Attach ordered per-stage provider/material relations to an already-resolved
/// Harness body. Stage components must be bound in the composition and must
/// not repeat; a composition that names a body-level `model` is a single-model
/// body and must carry exactly one stage for that model.
pub fn disclose_staged_model_runtime(
    composition: &HarnessComposition,
    stages: Vec<ModelStageRelation>,
) -> Result<StagedModelRuntimeReadModel> {
    if stages.is_empty() {
        return Err(AikitError::new(
            "model_runtime.stages_empty",
            "a staged model runtime must carry at least one stage",
        ));
    }
    let mut seen = BTreeSet::new();
    for stage in &stages {
        if !seen.insert(stage.component.clone()) {
            return Err(AikitError::new(
                "model_runtime.duplicate_stage_component",
                format!("component {} was staged more than once", stage.component),
            ));
        }
        if !composition
            .component_bindings
            .iter()
            .any(|binding| binding.component == stage.component)
        {
            return Err(AikitError::new(
                "model_runtime.stage_component_unbound",
                format!(
                    "stage component {} is not a bound Component of this composition",
                    stage.component
                ),
            )
            .with("component", stage.component.to_string()));
        }
        if let Some(modality) = &stage.relation.model_surface.modality {
            modality
                .validate()
                .map_err(|error| error.with("component", stage.component.to_string()))?;
        }
    }
    match &composition.model {
        Some(selected) if stages.len() == 1 => {
            if selected != &stages[0].relation.model.model {
                return Err(AikitError::new(
                    "model_runtime.model_identity_mismatch",
                    format!(
                        "HarnessComposition selected Model {selected} but the single stage describes {}",
                        stages[0].relation.model.model
                    ),
                ));
            }
        }
        Some(selected) => {
            return Err(AikitError::new(
                "model_runtime.multi_stage_model_conflict",
                format!(
                    "a body-level Model ({selected}) is a single-model fact; a {}-stage body must leave it unset and carry per-stage models",
                    stages.len()
                ),
            ));
        }
        None => {}
    }

    let mut unavailable = composition
        .absences
        .iter()
        .map(|absence| RuntimeUnavailability {
            field: absence.requirement.to_string(),
            reason: absence.reason.clone(),
        })
        .collect::<Vec<_>>();
    for stage in &stages {
        let prefix = format!("stage:{}", stage.component);
        for (field, access) in [
            ("inference-access", &stage.relation.model_surface.access.inference),
            (
                "material-control-access",
                &stage.relation.model_surface.access.material_control,
            ),
            (
                "model-interior-access",
                &stage.relation.model_surface.access.interior,
            ),
        ] {
            if let AccessFieldReading::Unavailable { reason } = access {
                unavailable.push(RuntimeUnavailability {
                    field: format!("{prefix}:{field}"),
                    reason: reason.clone(),
                });
            }
        }
        if let Some(modality) = &stage.relation.model_surface.modality {
            if let crate::resource::CredentialCondition::Required { hint } = &modality.credential {
                unavailable.push(RuntimeUnavailability {
                    field: format!("{prefix}:modality-credential"),
                    reason: format!("the stage needs a credential it does not have bound: {hint}"),
                });
            }
            match &modality.availability {
                crate::model_modality::SurfaceAvailability::Available => {}
                crate::model_modality::SurfaceAvailability::Degraded { reason } => {
                    unavailable.push(RuntimeUnavailability {
                        field: format!("{prefix}:modality-availability"),
                        reason: format!("degraded: {reason}"),
                    });
                }
                crate::model_modality::SurfaceAvailability::Unavailable { reason } => {
                    unavailable.push(RuntimeUnavailability {
                        field: format!("{prefix}:modality-availability"),
                        reason: reason.clone(),
                    });
                }
            }
        }
    }
    unavailable.sort_by(|left, right| (&left.field, &left.reason).cmp(&(&right.field, &right.reason)));

    let stage_views: Vec<(&ResourceRef, Option<&ModelModalityContract>)> = stages
        .iter()
        .map(|stage| (&stage.component, stage.relation.model_surface.modality.as_ref()))
        .collect();
    let composed_modality = compose_stage_modalities(&stage_views);

    Ok(StagedModelRuntimeReadModel {
        version: MODEL_STAGE_RUNTIME_VERSION.to_string(),
        project: composition.project.clone(),
        agent: composition.agent.clone(),
        agency: composition.agency.clone(),
        harness: composition.harness.clone(),
        agent_session: composition.session.clone(),
        harness_composition_fingerprint: composition.fingerprint.clone(),
        stages,
        composed_modality,
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
            material_shape: "rich serving runtime whose placement may expand across accelerators/hosts",
        },
    ]
}
