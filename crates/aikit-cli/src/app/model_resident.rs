//! The normal application-service model operation reaches the same native
//! encounter owner as Direct work. Eligibility, credential delivery and actual
//! model observation stay at that execution boundary, not in caller booleans.
use super::Service;
use crate::encounter_service::PreparedModel;
use aikit_adapters::agency_admission::AdmittedAgency;
use aikit_core::resource::ProviderRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{resolve_harness_composition, CompositionCatalog, HarnessCompositionRequest};
use aikit_core::{AikitError, ResourceRef, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentTarget {
    space: SessionSpaceRef,
    agent_session: ResourceRef,
    socket: PathBuf,
    #[serde(default)]
    body: Option<String>,
}

const EXPLICIT_SELECTION_SCHEMA: &str = "aikit.explicit-model-selection/v1";
const EXPLICIT_SELECTION_BASIS_SCHEMA: &str = "aikit.explicit-model-selection-basis/v1";
const ROUTE_BASIS_SCHEMA: &str = "aikit.model-route-basis/v1";
const SORTED_JSON_DIGEST_CONTRACT: &str = "aikit.sorted-json/v1";

#[derive(Serialize)]
struct RouteBasis<'a> {
    schema: &'static str,
    model_ref: &'a ResourceRef,
    provider_ref: &'a ProviderRef,
    native_provider: &'a str,
    provider_native_id: &'a str,
    policy_source: &'a crate::encounter_service::EncounterRequiredSource,
    catalogue_digest: &'a str,
}

fn sorted_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(sorted_json).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), sorted_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        value => value.clone(),
    }
}

fn digest(value: &impl Serialize) -> Result<String> {
    let value = serde_json::to_value(value)
        .map_err(|error| AikitError::new("model.selection_receipt", error.to_string()))?;
    let bytes = serde_json::to_vec(&sorted_json(&value))
        .map_err(|error| AikitError::new("model.selection_receipt", error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn explicit_factory_selection(
    compose: &Value,
    target: &ResidentTarget,
    native: &Value,
) -> Result<Option<Value>> {
    let prepared: PreparedModel = serde_json::from_value(native["data"]["model_selection"].clone())
        .map_err(|error| {
            AikitError::new(
                "model.selection_receipt",
                format!("Native selected-model basis is unreadable: {error}"),
            )
        })?;
    let body_basis = native["data"]["body_basis"].clone();
    if native["data"]["protocol"] != "pi-rpc"
        || body_basis["schema"] != "aikit.resident-body-basis/v1"
        || body_basis["protocol"] != "pi-rpc"
        || body_basis["harness_profile"] != "pi"
    {
        return Ok(None);
    }
    let profile = aikit_adapters::profiles::for_slug("pi").ok_or_else(|| {
        AikitError::new(
            "model.selection_receipt",
            "The embedded Pi harness profile is unavailable",
        )
    })?;
    let profile_digest = format!("blake3:{}", digest(profile)?);
    let model_basis_digest = prepared.fingerprint()?;
    if body_basis["model_basis_digest"] != model_basis_digest {
        return Err(AikitError::new(
            "model.selection_receipt",
            "The resident body model basis differs from the selected native policy",
        ));
    }
    let harness_profile = json!({
        "schema": profile.schema,
        "slug": profile.slug,
        "digest": profile_digest,
    });
    let composition_target_basis = json!({
        "schema":"aikit.harness-composition-target-basis/v1",
        "digest_contract":SORTED_JSON_DIGEST_CONTRACT,
        "harness_profile":harness_profile,
        "resident_body_basis":body_basis,
        "agency_source":prepared.agency_source,
        "world_binding_ref":prepared.world_binding_ref,
    });
    let composition_target_revision = format!("blake3:{}", digest(&composition_target_basis)?);
    let project = compose
        .pointer("/project_binding/project")
        .and_then(Value::as_str)
        .map(ResourceRef::parse)
        .transpose()?;
    let composition = resolve_harness_composition(
        &CompositionCatalog::default(),
        HarnessCompositionRequest {
            harness: ResourceRef::parse("harness/pi")?,
            project,
            agent: Some(prepared.policy.agent_ref.clone()),
            agency: Some(prepared.agency_ref.clone()),
            session: Some(target.agent_session.to_string()),
            model: Some(prepared.policy.model_ref.clone()),
            selections: Vec::new(),
            target_revision: Some(composition_target_revision.clone()),
            generation: None,
        },
    )?;
    let harness_composition_ref = format!("harness-composition/{}", composition.fingerprint);
    ResourceRef::parse(&harness_composition_ref)?;
    let route_basis = RouteBasis {
        schema: ROUTE_BASIS_SCHEMA,
        model_ref: &prepared.policy.model_ref,
        provider_ref: &prepared.policy.provider_ref,
        native_provider: &prepared.policy.native_provider,
        provider_native_id: &prepared.policy.provider_native_id,
        policy_source: &prepared.policy_source,
        catalogue_digest: &prepared.catalogue_digest,
    };
    let route_basis = serde_json::to_value(route_basis)
        .map_err(|error| AikitError::new("model.selection_receipt", error.to_string()))?;
    let route_ref = format!("model-route/{}", digest(&route_basis)?);
    ResourceRef::parse(&route_ref)?;
    let basis = json!({
        "schema": EXPLICIT_SELECTION_BASIS_SCHEMA,
        "selection_kind": "explicit-pin",
        "digest_contract":SORTED_JSON_DIGEST_CONTRACT,
        "route_basis": route_basis,
        "composition_scope": {
            "kind":"thin-native-pi",
            "selected_components":[],
            "ambient_components_claimed":false,
            "standing":"empty selections mean this receipt claims the exact Pi body and launch basis only; it does not claim ambient skills, extensions, tools, or contributions",
        },
        "composition_target_basis":composition_target_basis,
        "harness_composition": composition,
        "native": {
            "space":target.space,
            "agent_session": target.agent_session,
            "native_session_id": native["data"]["native_session_id"],
            "model_observation": native["data"]["model_observation"],
        },
    });
    let basis_digest = format!("blake3:{}", digest(&basis)?);
    let explanation = json!({
        "schema": EXPLICIT_SELECTION_SCHEMA,
        "selection_kind": "explicit-pin",
        "model_ref": prepared.policy.model_ref,
        "provider_ref": prepared.policy.provider_ref,
        "route_ref": route_ref,
        "harness_ref": "harness/pi",
        "harness_composition_ref": harness_composition_ref,
        "basis": basis,
        "basis_digest": basis_digest,
        "digest_contract":SORTED_JSON_DIGEST_CONTRACT,
        "standing": "explicit source-backed model pin confirmed by the resident native owner; not a ranking or inference receipt",
    });
    Ok(Some(json!({
        "roster_version": aikit_core::resource::MODEL_ROSTER_VERSION,
        "model_ref": explanation["model_ref"],
        "provider_ref": explanation["provider_ref"],
        "ranking_policy": "EXPLICIT_PIN",
        "ranking_explanation": explanation,
        "provenance": [
            prepared.policy_source.source,
            prepared.policy_source.revision,
            prepared.policy_source.content_digest,
            prepared.catalogue_digest,
        ],
    })))
}

pub(super) fn realise(
    service: &Service,
    compose: &Value,
    model: &str,
    provider: Option<&str>,
    body: Option<&str>,
    resolution: Option<Value>,
) -> Result<Value> {
    let target: ResidentTarget = serde_json::from_value(
        compose.get("resident_target").cloned().ok_or_else(|| {
            AikitError::new("model.resident_target_required",
                "Model realisation needs an explicit existing AgentSession, SessionSpace and owner socket; a composition or instantiation record is not a running body")
        })?,
    ).map_err(|e| AikitError::new("model.invalid_resident_target", e.to_string()))?;
    if !target.socket.is_absolute() {
        return Err(AikitError::new(
            "model.invalid_resident_target",
            "The native owner socket must be absolute",
        ));
    }
    if body
        .zip(target.body.as_deref())
        .is_some_and(|(a, b)| a != b)
    {
        return Err(AikitError::new(
            "model.body_conflict",
            "The explicit body differs from the composed resident target",
        ));
    }
    let expected_agency: AdmittedAgency =
        serde_json::from_value(compose.get("agency_admission").cloned().ok_or_else(|| {
            AikitError::new(
                "model.native_agency_required",
                "Supply the source-backed selected Agency/WorldBinding from native composition",
            )
        })?)
        .map_err(|e| AikitError::new("model.native_agency_required", e.to_string()))?;
    let model_ref = ResourceRef::parse(model)?;
    if !model_ref.as_str().starts_with("model:") {
        return Err(AikitError::new(
            "model.canonical_ref_required",
            "Choose a canonical catalogue model reference",
        ));
    }
    let provider_ref = provider.map(ProviderRef::parse).transpose()?;
    let cwd = service
        .invocation_cwd
        .canonicalize()
        .map_err(|e| AikitError::new("model.cwd_unavailable", e.to_string()))?;
    let request = crate::encounter_service::EncounterRequest::OpenModel {
        request: Box::new(crate::encounter_service::EncounterModelOpen {
            space: target.space.clone(),
            agent_session: target.agent_session.clone(),
            cwd,
            model_ref: model_ref.clone(),
            provider_ref,
            body: body.map(str::to_owned).or(target.body.clone()),
            expected_agency,
        }),
    };
    let native = crate::encounter_service::request(&target.socket, &request)?;
    if native["ok"] != true
        || native["data"]["selected"] != true
        || native["data"]["executed"] != false
    {
        return Err(AikitError::new("model.resident_refused",
            "Native selected-model admission or observation failed; no instantiation receipt or implicit fallback substitutes for a resident")
            .with("native_response", native.to_string()));
    }
    let factory_selection = match &resolution {
        Some(_) => None,
        None => explicit_factory_selection(compose, &target, &native)?,
    };
    let mut receipt = json!({
        "schema":"aikit.model-realisation/v2",
        "model_ref":model_ref,
        "selected":true,
        "executed":false,
        "resident":native["data"],
        "standing":"native catalogue/authority/credential/model-observed resident; send an addressed turn for actual inference and result evidence"
    });
    if let Some(factory_selection) = factory_selection {
        receipt["factory_selection"] = factory_selection;
    }
    // When the roster chose this model, the receipt names why: the winning
    // pair, the policy and the ranking explanation ride the realisation.
    if let Some(resolution) = resolution {
        receipt["resolution"] = resolution;
    }
    Ok(receipt)
}
