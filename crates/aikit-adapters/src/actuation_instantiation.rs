//! AIKit intake of the Actuation-owned `actuation.instantiation/v1` receipt
//! (legacy `actuation.model-bearing/v1` documents read unchanged in the
//! accept window).
//!
//! Actuation owns model/harness/loop infrastructure. AIKit consumes the full
//! object as *refs + facts* and never re-authors Agency, Model, Harness or loop
//! identity. The promoted refs (`harness_ref`, `model_relation.model_ref`,
//! `agency_ref`, `agent_session_ref`) flow into AIKit's resolution inputs;
//! engine, materialisation and inference-surface stay nested facts, never
//! promoted root identities — matching the contract's own framing that "engine,
//! materialisation and surface remain nested refs/facts rather than promoted
//! root identities."

use aikit_core::context_resolution::RequestedActors;
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ACTUATION_INSTANTIATION_SCHEMA: &str = "actuation.instantiation/v1";

/// Legacy window: the receipt contract was renamed from
/// `actuation.model-bearing/v1`; documents carrying the old schema string
/// read as instantiation receipts during the accept window, in step with
/// Actuation's own contract rename.
pub const LEGACY_MODEL_BEARING_SCHEMA: &str = "actuation.model-bearing/v1";

/// One `allowed`/`denied` access set from the model-bearing access profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstantiationSet {
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub denied: Vec<String>,
}

/// Interior access carries an explicit depth grant beside its access set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstantiationInterior {
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub denied: Vec<String>,
    /// `opaque`..`learning` — the interior access depth the Actuation grants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstantiationAccessProfile {
    #[serde(default)]
    pub inference: InstantiationSet,
    #[serde(default)]
    pub control: InstantiationSet,
    #[serde(default)]
    pub interior: InstantiationInterior,
}

/// `model_relation` — nested refs/facts, never promoted to root identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstantiationModelRelation {
    pub model_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_ref: Option<ResourceRef>,
    /// Engine facts (`implementation_ref`/`provider_ref`/`facts`), verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Value>,
    /// Material facts (`binding_ref`/`placement`/`facts`), verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Value>,
    /// Inference-surface facts (`contract_ref`/`binding_ref`/`facts`), verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_surface: Option<Value>,
}

/// The full Actuation model-bearing projection, consumed as refs + facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActuationInstantiationProjection {
    pub actuation_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub world_binding_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_composition_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_ref: Option<ResourceRef>,
    pub model_relation: InstantiationModelRelation,
    #[serde(default)]
    pub access_profile: InstantiationAccessProfile,
    #[serde(default)]
    pub bounds_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub evidence_refs: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
}

impl ActuationInstantiationProjection {
    /// Deserialize a raw `actuation.model-bearing/v1` value and validate it.
    /// The caller is responsible for having fetched the object from Actuation's
    /// native surface; this intake never invokes Actuation itself.
    pub fn parse(value: &Value) -> Result<Self> {
        // Accept window: documents carrying the legacy schema string read as
        // instantiation receipts; anything else is refused loudly.
        let schema = value
            .get("schema")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match schema {
            ACTUATION_INSTANTIATION_SCHEMA | LEGACY_MODEL_BEARING_SCHEMA => {}
            other => {
                return Err(AikitError::new(
                    "actuation_instantiation.wrong_schema",
                    format!(
                        "schema must be {ACTUATION_INSTANTIATION_SCHEMA} (legacy {LEGACY_MODEL_BEARING_SCHEMA} accepted); got {other:?}"
                    ),
                ));
            }
        }
        let projection: Self = serde_json::from_value(value.clone()).map_err(|error| {
            AikitError::new(
                "actuation_instantiation.parse",
                format!("could not read {ACTUATION_INSTANTIATION_SCHEMA}: {error}"),
            )
        })?;
        projection.validate()?;
        Ok(projection)
    }

    /// Model/harness/loop identity must stay distinct — a collapsed ref is a
    /// caller error, never something AIKit silently repairs.
    pub fn validate(&self) -> Result<()> {
        let required = [
            ("actuation_ref", &self.actuation_ref),
            ("agency_ref", &self.agency_ref),
            ("world_binding_ref", &self.world_binding_ref),
        ];
        for (index, (left_name, left)) in required.iter().enumerate() {
            if left.as_str().is_empty() {
                return Err(AikitError::new(
                    "actuation_instantiation.invalid_ref",
                    format!("{left_name} must not be empty"),
                ));
            }
            for (right_name, right) in required.iter().skip(index + 1) {
                if left == right {
                    return Err(AikitError::new(
                        "actuation_instantiation.identity_collapse",
                        format!("{left_name} and {right_name} must remain distinct"),
                    ));
                }
            }
        }
        if self.model_relation.model_ref.as_str().is_empty() {
            return Err(AikitError::new(
                "actuation_instantiation.invalid_ref",
                "model_relation.model_ref must not be empty",
            ));
        }
        for (name, candidate) in [
            ("harness_ref", self.harness_ref.as_ref()),
            ("agent_session_ref", self.agent_session_ref.as_ref()),
        ] {
            if let Some(candidate) = candidate {
                if candidate.as_str().is_empty() {
                    return Err(AikitError::new(
                        "actuation_instantiation.invalid_ref",
                        format!("{name} must not be empty"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The model-bearing refs AIKit promotes into its resolution inputs.
    /// Identity stays a `ResourceRef`; nothing here re-types engine facts.
    pub fn actuation(&self) -> ResourceRef {
        self.actuation_ref.clone()
    }

    pub fn agency(&self) -> ResourceRef {
        self.agency_ref.clone()
    }

    pub fn world_binding(&self) -> ResourceRef {
        self.world_binding_ref.clone()
    }

    pub fn harness(&self) -> Option<ResourceRef> {
        self.harness_ref.clone()
    }

    pub fn model(&self) -> Option<ResourceRef> {
        Some(self.model_relation.model_ref.clone())
    }

    pub fn agent_session(&self) -> Option<String> {
        self.agent_session_ref.as_ref().map(ToString::to_string)
    }

    /// The `RequestedActors` slice this projection supplies. AIKit resolves the
    /// canonical refs against its own resource index; Actuation remains owner.
    pub fn requested_actors(&self) -> RequestedActors {
        RequestedActors {
            agency: Some(self.agency_ref.clone()),
            ..RequestedActors::default()
        }
    }
}

/// The Central-authored slice of an Actor's identity: enduring Agent, machine
/// Host, and authored profile/praxis refs. Central owns these; AIKit consumes
/// them as refs and never re-authors them. This is the composition seam shape,
/// not a re-serialization of `central.agent-profile/v1`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CentralAuthoredProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ref: Option<ResourceRef>,
    #[serde(default)]
    pub profile_refs: Vec<ResourceRef>,
    /// Exact parsed Central source, including its revision and distinct assignment
    /// categories. Retention is not effective selection, disclosure or authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_source: Option<crate::central_agent_profile::CentralAgentProfileProjection>,
}

/// Where the two authoritative projections meet: Actuation supplies the live
/// model-bearing object, Central supplies the authored identity, and AIKit
/// composes the resolution inputs it resolves capabilities/contexts against.
/// Neither projection is re-owned; only canonical refs move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedActorInputs {
    /// Requested authored basis retained for downstream resolution and Explain.
    /// This does not claim that its sources or praxis are active in the harness.
    pub authored: CentralAuthoredProjection,
    /// Ephemeral observed source records; no execution/provider grant.
    pub source_resources: Vec<aikit_core::resource::ResourceRecord>,
    pub requested_actors: RequestedActors,
    pub selected_harness: Option<ResourceRef>,
    pub selected_model: Option<ResourceRef>,
    pub agent_session: Option<String>,
    /// Actual native identities, never inferred from a host or SessionSpace.
    pub actuation_ref: Option<ResourceRef>,
    pub world_binding_ref: Option<ResourceRef>,
}

pub fn compose_actor_inputs(
    actuation: &ActuationInstantiationProjection,
    central: &CentralAuthoredProjection,
) -> ComposedActorInputs {
    ComposedActorInputs {
        authored: central.clone(),
        source_resources: Vec::new(),
        requested_actors: RequestedActors {
            agent: central.agent_ref.clone(),
            agency: Some(actuation.agency_ref.clone()),
            host: central.host_ref.clone(),
        },
        selected_harness: actuation.harness(),
        selected_model: actuation.model(),
        agent_session: actuation.agent_session(),
        actuation_ref: Some(actuation.actuation_ref.clone()),
        world_binding_ref: Some(actuation.world_binding_ref.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::actor_bootstrap::ActorBootstrapRequest;

    fn projection_value() -> Value {
        serde_json::json!({
            "schema": "actuation.model-bearing/v1",
            "actuation_ref": "actuation/root",
            "agency_ref": "agency/mahamaya-build",
            "world_binding_ref": "world-binding/central",
            "harness_ref": "harness/codex",
            "harness_composition_ref": "harness-composition/codex-current",
            "agent_session_ref": "agent-session/codex-7",
            "model_relation": {
                "schema": "actuation.model-bearing/v1",
                "model_ref": "model/deepseek-chat",
                "variant_ref": "variant/deepseek-chat/main",
                "engine": {
                    "implementation_ref": "engine/deepseek-gateway",
                    "provider_ref": "provider/deepseek",
                    "facts": { "temperature": 0.7 }
                },
                "material": { "binding_ref": "material/local", "placement": "local" },
                "inference_surface": { "contract_ref": "surface/openai-compatible" }
            },
            "access_profile": {
                "schema": "actuation.model-bearing/v1",
                "inference": { "allowed": ["model/deepseek-chat"], "denied": [] },
                "control": { "allowed": [], "denied": ["tool/shell"] },
                "interior": { "allowed": ["evidence/read"], "denied": [], "depth": "outputs" }
            },
            "bounds_refs": ["bound/central-constraints"],
            "evidence_refs": ["evidence/model-bearing-1"],
            "return_ref": "return/agency-mahamaya-build",
            "observed_at": "2026-09-04T12:00:00Z"
        })
    }

    #[test]
    fn parses_full_model_bearing_object_as_refs_and_nested_facts() {
        let projection = ActuationInstantiationProjection::parse(&projection_value()).unwrap();
        assert_eq!(projection.actuation_ref.as_str(), "actuation/root");
        assert_eq!(projection.agency_ref.as_str(), "agency/mahamaya-build");
        assert_eq!(
            projection.world_binding_ref.as_str(),
            "world-binding/central"
        );
        assert_eq!(
            projection.harness(),
            Some(ResourceRef::parse("harness/codex").unwrap())
        );
        assert_eq!(
            projection.model(),
            Some(ResourceRef::parse("model/deepseek-chat").unwrap())
        );
        assert_eq!(
            projection.agent_session(),
            Some("agent-session/codex-7".to_string())
        );
        // Engine/material/surface remain nested facts, never promoted roots.
        assert!(projection.model_relation.engine.is_some());
        assert_eq!(
            projection.model_relation.engine.as_ref().unwrap()["implementation_ref"],
            "engine/deepseek-gateway"
        );
        assert_eq!(
            projection.access_profile.interior.depth.as_deref(),
            Some("outputs")
        );
    }

    #[test]
    fn legacy_schema_reads_as_instantiation_receipt() {
        // The fixture itself is a legacy document; it must parse unchanged.
        let projection = ActuationInstantiationProjection::parse(&projection_value()).unwrap();
        assert_eq!(projection.actuation_ref.as_str(), "actuation/root");
    }

    #[test]
    fn current_schema_and_wrong_schema_are_distinguished() {
        let mut current = projection_value();
        current["schema"] = serde_json::json!(ACTUATION_INSTANTIATION_SCHEMA);
        ActuationInstantiationProjection::parse(&current).unwrap();

        let mut wrong = projection_value();
        wrong["schema"] = serde_json::json!("actuation.model-bearing/v2");
        let error = ActuationInstantiationProjection::parse(&wrong).unwrap_err();
        assert!(error
            .to_string()
            .contains("schema must be actuation.instantiation/v1"));
        assert!(error
            .to_string()
            .contains("legacy actuation.model-bearing/v1 accepted"));
    }

    #[test]
    fn promoted_refs_compose_into_resolution_inputs_without_reowning() {
        let projection = ActuationInstantiationProjection::parse(&projection_value()).unwrap();
        let request = ActorBootstrapRequest {
            selected_harness: projection.harness(),
            selected_model: projection.model(),
            agent_session: projection.agent_session(),
            ..ActorBootstrapRequest::default()
        };
        let actors = projection.requested_actors();

        assert_eq!(
            request.selected_harness,
            Some(ResourceRef::parse("harness/codex").unwrap())
        );
        assert_eq!(
            request.selected_model,
            Some(ResourceRef::parse("model/deepseek-chat").unwrap())
        );
        assert_eq!(
            actors.agency,
            Some(ResourceRef::parse("agency/mahamaya-build").unwrap())
        );
        // Nothing here manufactures a Model/Harness registry: only refs move.
    }

    #[test]
    fn identity_collapse_is_rejected_not_repaired() {
        let mut value = projection_value();
        value["agency_ref"] = serde_json::json!("actuation/root");
        assert_eq!(
            ActuationInstantiationProjection::parse(&value)
                .unwrap_err()
                .code(),
            "actuation_instantiation.identity_collapse"
        );
    }

    #[test]
    fn empty_model_ref_is_rejected() {
        let mut value = projection_value();
        value["model_relation"]["model_ref"] = serde_json::json!("");
        assert_eq!(
            ActuationInstantiationProjection::parse(&value)
                .unwrap_err()
                .code(),
            "actuation_instantiation.invalid_ref"
        );
    }

    #[test]
    fn composition_joins_actuation_live_and_central_authored_without_reowning() {
        let actuation = ActuationInstantiationProjection::parse(&projection_value()).unwrap();
        let central = CentralAuthoredProjection {
            agent_ref: Some(ResourceRef::parse("agent/mahamaya").unwrap()),
            host_ref: Some(ResourceRef::parse("host/central").unwrap()),
            profile_refs: vec![ResourceRef::parse("profile/central/build").unwrap()],
            ..Default::default()
        };

        let inputs = compose_actor_inputs(&actuation, &central);

        assert_eq!(
            inputs.requested_actors.agent,
            Some(ResourceRef::parse("agent/mahamaya").unwrap())
        );
        assert_eq!(
            inputs.requested_actors.agency,
            Some(ResourceRef::parse("agency/mahamaya-build").unwrap())
        );
        assert_eq!(
            inputs.requested_actors.host,
            Some(ResourceRef::parse("host/central").unwrap())
        );
        assert_eq!(
            inputs.selected_harness,
            Some(ResourceRef::parse("harness/codex").unwrap())
        );
        assert_eq!(
            inputs.selected_model,
            Some(ResourceRef::parse("model/deepseek-chat").unwrap())
        );
        assert_eq!(
            inputs.agent_session,
            Some("agent-session/codex-7".to_string())
        );
    }
}
