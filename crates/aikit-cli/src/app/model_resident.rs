//! The normal application-service model operation reaches the same native
//! encounter owner as Direct work. Eligibility, credential delivery and actual
//! model observation stay at that execution boundary, not in caller booleans.
use super::Service;
use aikit_adapters::agency_admission::AdmittedAgency;
use aikit_core::resource::ProviderRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{AikitError, ResourceRef, Result};
use serde::Deserialize;
use serde_json::{json, Value};
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

pub(super) fn realise(
    service: &Service,
    compose: &Value,
    model: &str,
    provider: Option<&str>,
    body: Option<&str>,
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
            space: target.space,
            agent_session: target.agent_session,
            cwd,
            model_ref: model_ref.clone(),
            provider_ref,
            body: body.map(str::to_owned).or(target.body),
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
    Ok(json!({
        "schema":"aikit.model-realisation/v2",
        "model_ref":model_ref,
        "selected":true,
        "executed":false,
        "resident":native["data"],
        "standing":"native catalogue/authority/credential/model-observed resident; send an addressed turn for actual inference and result evidence"
    }))
}
