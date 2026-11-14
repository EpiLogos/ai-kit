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
    #[serde(default)]
    released_predecessor: Option<crate::encounter_service::NativeReleasedPredecessor>,
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
    let (profile_slug, harness_ref, scope_kind, scope_standing) =
        match (
            native["data"]["protocol"].as_str(),
            body_basis["protocol"].as_str(),
            body_basis["harness_profile"].as_str(),
        ) {
            (Some("pi-rpc"), Some("pi-rpc"), Some("pi")) => (
                "pi",
                "harness/pi",
                "thin-native-pi",
                "empty selections mean this receipt claims the exact Pi body and launch basis only; it does not claim ambient skills, extensions, tools, or contributions",
            ),
            (Some("acp"), Some("acp"), Some("codex")) => (
                "codex",
                "harness/codex",
                "thin-native-codex-acp",
                "empty selections mean this receipt claims the exact Codex ACP body and launch basis only; it does not claim ambient skills, extensions, tools, or contributions",
            ),
            _ => return Ok(None),
        };
    if body_basis["schema"] != "aikit.resident-body-basis/v1" {
        return Ok(None);
    }
    let profile = aikit_adapters::profiles::for_slug(profile_slug).ok_or_else(|| {
        AikitError::new(
            "model.selection_receipt",
            format!("The embedded {profile_slug} harness profile is unavailable"),
        )
    })?;
    if native["data"]["model_observation"]["current_model_id"] != prepared.policy.provider_native_id
        || !native["data"]["native_session_id"]
            .as_str()
            .is_some_and(|session| !session.trim().is_empty())
    {
        return Err(AikitError::new(
            "model.selection_receipt",
            "The resident native session did not confirm the exact selected model",
        ));
    }
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
            harness: ResourceRef::parse(harness_ref)?,
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
            "kind":scope_kind,
            "selected_components":[],
            "ambient_components_claimed":false,
            "standing":scope_standing,
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
        "harness_ref": harness_ref,
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

/// The selection kind of a session AIKit opened without a model policy.
const SEATED_SESSION: &str = "seated-session";

/// The selection receipt for a session AIKit opened without a model policy: an agent the person seated, its model the
/// harness's own (or the model default AIKit asked for when it opened). It is the receipt `explicit_factory_selection`
/// issues, of kind `seated-session`, and binds what the resident owner opened and confirms now: the body basis, the
/// harness profile, the native session, the Agency and WorldBinding the session was admitted with, and the model the
/// native session reports. That model is named by the catalogue Model that claims its provider-native id through the
/// session's provider; where no Model claims it, by the route that names it, and the offer stays unmatched
/// (model_route.rs `UnmatchedModelOffer`): no Model is minted from an observed id. A session that discloses no provider
/// (gemini and kimi over ACP report the model they run and no provider) is still answered, for the harness it stands on:
/// no provider is named, and its route names the model by the id the harness runs, through that harness.
fn seated_factory_selection(
    home: &aikit_store::AikitHome,
    compose: &Value,
    target: &ResidentTarget,
    native: &Value,
) -> Result<Value> {
    let refuse = |message: String| AikitError::new("model.seated_selection", message);
    let session = &target.agent_session;
    let data = &native["data"];
    let body_basis = data["body_basis"].clone();
    if body_basis["schema"] != "aikit.resident-body-basis/v1" {
        return Err(refuse(format!(
            "{session} stands on no resident body basis AIKit can name"
        )));
    }
    let slug = body_basis["harness_profile"].as_str().ok_or_else(|| {
        refuse(format!(
            "{session}'s resident body joins no harness profile, so AIKit names no harness for it"
        ))
    })?;
    let profile = aikit_adapters::profiles::for_slug(slug).ok_or_else(|| {
        refuse(format!(
            "The embedded {slug} harness profile is unavailable"
        ))
    })?;
    let observation = &data["model_observation"];
    let observed = observation["current_model_id"]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| refuse(format!("{session}'s native session reports no model")))?;
    if !data["native_session_id"]
        .as_str()
        .is_some_and(|id| !id.trim().is_empty())
    {
        return Err(refuse(format!("{session} has no native session")));
    }
    // The provider that serves the model: the one the harness binds natively, or, for a provider-plural harness, the
    // one its own session discloses for the model it runs, or none, where the session discloses none. A model id is
    // never parsed for it.
    let (provider, claimed_id) = match profile.models.as_ref().map(|models| &models.dispatch) {
        Some(aikit_core::harness_profile::ModelDispatchPosture::NativeProviderBinding {
            provider_ref,
            ..
        }) => (Some(provider_ref.clone()), observed.to_owned()),
        _ => {
            let disclosed = observation["available_models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|model| model["modelId"] == observed)
                .find_map(|model| {
                    Some((
                        model["rosterIdentity"]["provider_ref"].as_str()?.to_owned(),
                        model["rosterIdentity"]["provider_native_id"]
                            .as_str()?
                            .to_owned(),
                    ))
                });
            match disclosed {
                Some((provider, id)) => (Some(provider), id),
                None => (
                    observation["native_provider"]
                        .as_str()
                        .filter(|native| !native.trim().is_empty())
                        .map(|native| format!("provider:{native}")),
                    observed.to_owned(),
                ),
            }
        }
    };
    let provider_ref = provider.as_deref().map(ProviderRef::parse).transpose()?;
    let harness_ref = format!("harness/{slug}");
    // A catalogue Model claims an id through a provider; with none disclosed, none is claimed.
    let (catalogue, _) = aikit_store::model_catalogue::resolved_catalogue(home);
    let claiming: Vec<_> = match &provider_ref {
        Some(provider_ref) => catalogue
            .entries()
            .filter(|entry| {
                entry
                    .routes
                    .iter()
                    .any(|route| &route.provider == provider_ref && route.claims(&claimed_id))
            })
            .collect(),
        None => Vec::new(),
    };
    let (model_ref, catalogue_digest) = match claiming.as_slice() {
        [] => (None, None),
        [entry] => {
            let value = serde_json::to_value(entry)
                .map_err(|error| AikitError::new("model.selection_receipt", error.to_string()))?;
            (
                Some(entry.model.clone()),
                Some(format!(
                    "blake3:{}",
                    blake3::hash(value.to_string().as_bytes()).to_hex()
                )),
            )
        }
        several => {
            return Err(refuse(format!(
            "{} catalogue Models claim {claimed_id} through {} ({}); AIKit names no one of them",
            several.len(),
            provider.as_deref().unwrap_or_default(),
            several
                .iter()
                .map(|entry| entry.model.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
        }
    };
    let unmatched_offer = match (&model_ref, &provider_ref) {
        (Some(_), _) => Value::Null,
        (None, Some(provider_ref)) => json!({
            "provider_ref": provider_ref,
            "provider_native_id": claimed_id,
            "reason": "no catalogue Model claims this provider-native id through this provider: it stays an offer until a catalogue entry or a Provider Source admits it, and the route names the model the session runs",
        }),
        // The harness is the route's own: the same id run through another harness is another route.
        (None, None) => json!({
            "provider_ref": Value::Null,
            "harness_ref": harness_ref,
            "provider_native_id": claimed_id,
            "reason": "the session discloses no provider for the model it runs, and its harness binds none natively: no catalogue Model is claimed, and the route names the model by the id the harness runs, through that harness",
        }),
    };
    let route_basis = json!({
        "schema": ROUTE_BASIS_SCHEMA,
        "model_ref": model_ref,
        "provider_ref": provider_ref,
        "native_provider": provider_ref.as_ref().map(|provider| provider.as_str().strip_prefix("provider:").unwrap_or(provider.as_str())),
        "provider_native_id": observed,
        "policy_source": Value::Null,
        "catalogue_digest": catalogue_digest,
        "unmatched_offer": unmatched_offer,
    });
    let route_ref = format!("model-route/{}", digest(&route_basis)?);
    let model = match &model_ref {
        Some(model) => model.clone(),
        None => ResourceRef::parse(&route_ref)?,
    };
    let admitted: AdmittedAgency = serde_json::from_value(compose["agency_admission"].clone())
        .map_err(|error| {
            refuse(format!(
                "{session}'s admitted Agency is unreadable: {error}"
            ))
        })?;
    let composition_target_basis = json!({
        "schema":"aikit.harness-composition-target-basis/v1",
        "digest_contract":SORTED_JSON_DIGEST_CONTRACT,
        "harness_profile":{
            "schema": profile.schema,
            "slug": profile.slug,
            "digest": format!("blake3:{}", digest(profile)?),
        },
        "resident_body_basis":body_basis,
        "agency_source":admitted.basis,
        "world_binding_ref":admitted.world_binding_ref,
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
            harness: ResourceRef::parse(&harness_ref)?,
            project,
            agent: Some(admitted.agent_ref.clone()),
            agency: Some(admitted.agency_ref.clone()),
            session: Some(session.to_string()),
            model: Some(model.clone()),
            selections: Vec::new(),
            target_revision: Some(composition_target_revision),
            generation: None,
        },
    )?;
    let harness_composition_ref = format!("harness-composition/{}", composition.fingerprint);
    ResourceRef::parse(&harness_composition_ref)?;
    let basis = json!({
        "schema": EXPLICIT_SELECTION_BASIS_SCHEMA,
        "selection_kind": SEATED_SESSION,
        "digest_contract":SORTED_JSON_DIGEST_CONTRACT,
        "route_basis": route_basis,
        "composition_scope": {
            "kind":SEATED_SESSION,
            "selected_components":[],
            "ambient_components_claimed":false,
            "standing":"the session as AIKit opened it for the agent seated on it: its exact body and launch basis and the harness's own model as the session reports it; no skills, extensions, tools or contributions are claimed",
        },
        "composition_target_basis":composition_target_basis,
        "harness_composition": composition,
        "native": {
            "space":target.space,
            "agent_session": session,
            "native_session_id": data["native_session_id"],
            "model_observation": observation,
        },
    });
    let basis_digest = format!("blake3:{}", digest(&basis)?);
    let mut provenance = vec![
        format!("aikit-encounter-selection:{session}"),
        data["native_session_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    ];
    provenance.extend(catalogue_digest);
    Ok(json!({
        "roster_version": aikit_core::resource::MODEL_ROSTER_VERSION,
        "model_ref": model,
        "provider_ref": provider_ref,
        "ranking_policy": "EXPLICIT_PIN",
        "ranking_explanation": {
            "schema": EXPLICIT_SELECTION_SCHEMA,
            "selection_kind": SEATED_SESSION,
            "model_ref": model,
            "provider_ref": provider_ref,
            "route_ref": route_ref,
            "harness_ref": harness_ref,
            "harness_composition_ref": harness_composition_ref,
            "basis": basis,
            "basis_digest": basis_digest,
            "digest_contract":SORTED_JSON_DIGEST_CONTRACT,
            "standing": match provider_ref {
                Some(_) => "the seated session as its resident native owner confirms it: no policy pinned its model and no roster ranked it; the model is the one the session runs, named by the catalogue Model that claims it, else by its route",
                None => "the seated session as its resident native owner confirms it: no policy pinned its model and no roster ranked it; the model is the one the session runs, and as the session discloses no provider for it, it is named by its route through the session's harness",
            },
        },
        "provenance": provenance,
    }))
}

impl Service {
    /// `aikit compose model selection --session <s>`: the selection receipt a launch binds for a session AIKit opened,
    /// as the realisation (`aikit.model-realisation/v2`) of the resident the session stands on now, read from its owner.
    /// Nothing is opened, replaced or prompted. A session realised on an explicit model policy carries the explicit pin
    /// receipt `aikit compose --realise` issued; a session opened without one carries its seated-session receipt.
    pub fn session_selection(&self, session: &ResourceRef) -> Result<Value> {
        let compose = self.compose_session_resident(session, None)?;
        let target: ResidentTarget = serde_json::from_value(compose["resident_target"].clone())
            .map_err(|e| AikitError::new("model.invalid_resident_target", e.to_string()))?;
        let native = crate::encounter_service::request(
            &target.socket,
            &crate::encounter_service::EncounterRequest::Selection {
                agent_session: session.clone(),
            },
        )?;
        if native["ok"] != true {
            return Err(AikitError::new(
                "model.resident_unread",
                format!(
                    "{session}'s resident owner did not read it back: {}",
                    native["error"]["message"]
                        .as_str()
                        .unwrap_or("it gave no reason")
                ),
            )
            .with("native_response", native.to_string()));
        }
        let factory_selection = if native["data"]["model_selection"].is_null() {
            seated_factory_selection(&self.home, &compose, &target, &native)?
        } else {
            explicit_factory_selection(&compose, &target, &native)?.ok_or_else(|| {
                AikitError::new(
                    "model.selection_receipt",
                    format!("{session}'s selected model runs on a body AIKit issues no explicit selection receipt for"),
                )
            })?
        };
        Ok(json!({
            "schema":"aikit.model-realisation/v2",
            "model_ref":factory_selection["model_ref"],
            "selected":true,
            "executed":false,
            "resident":native["data"],
            "factory_selection":factory_selection,
            "standing":"the resident the session stands on, read from its owner: nothing was opened, replaced or prompted",
        }))
    }

    /// What `aikit compose --session <s> --realise` realises on: the session's
    /// own Agency admitted again from its owner binding, the one SessionSpace
    /// that holds the session, and the native owner that runs it (this home's,
    /// unless `socket` names another). The caller supplies the session and the
    /// model; nothing else about the session is taken from it.
    pub fn compose_session_resident(
        &self,
        session: &ResourceRef,
        socket: Option<&std::path::Path>,
    ) -> Result<Value> {
        let spaces: Vec<_> = aikit_store::SessionSpaceApplicationStore::new(self.home.clone())
            .list()?
            .into_iter()
            .filter(|state| state.agent_sessions.contains_key(session))
            .collect();
        let space = match spaces.as_slice() {
            [space] => space,
            [] => {
                return Err(AikitError::new(
                    "model.session_unplaced",
                    format!(
                        "{session} stands in no SessionSpace, so no resident of it can be opened"
                    ),
                ))
            }
            _ => {
                return Err(AikitError::new(
                    "model.session_ambiguous",
                    format!(
                        "{session} stands in {} SessionSpaces ({}); a realisation opens it in one",
                        spaces.len(),
                        spaces
                            .iter()
                            .map(|state| state.id().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ))
            }
        };
        if space.benched.contains(session) {
            return Err(AikitError::new(
                "model.session_benched",
                format!(
                    "{session} is on the bench in {}: it is given no work, so no launch reaches it",
                    space.id()
                ),
            ));
        }
        let admitted =
            crate::encounter_service::EncounterService::admit_own_agency(&self.home, session)?;
        let socket = socket
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| crate::encounter_service::socket_path(&self.home));
        Ok(json!({
            "agency_admission": admitted,
            "composition_scope": {
                "kind": "session-own-agency",
                "ambient_components_claimed": false,
                "standing": "the session's own Agency from its owner binding, admitted again; no profile, plan or ambient composition is claimed"
            },
            "resident_target": {"space": space.id(), "agent_session": session, "socket": socket}
        }))
    }
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
    let open = Box::new(crate::encounter_service::EncounterModelOpen {
        space: target.space.clone(),
        agent_session: target.agent_session.clone(),
        cwd,
        model_ref: model_ref.clone(),
        provider_ref,
        body: body.map(str::to_owned).or(target.body.clone()),
        expected_agency,
    });
    let request = match target.released_predecessor.clone() {
        Some(released_predecessor) => crate::encounter_service::EncounterRequest::ReplaceModel {
            request: open,
            released_predecessor,
        },
        None => crate::encounter_service::EncounterRequest::OpenModel { request: open },
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A codex session as `oi desktop provision` opens it for a seated agent: a plain open, no model policy, the model
    /// the harness runs as its own. The values stand where the resident owner's Selection read puts them.
    fn seat(observed: &str) -> (Value, ResidentTarget, Value) {
        let compose = json!({
            "agency_admission": {
                "basis": {
                    "source_ref": "source/agency-notes",
                    "revision": "revision/1",
                    "path": "/tmp/agency-notes.json",
                    "content_digest": format!("blake3:{}", "0".repeat(64)),
                },
                "agent_ref": "agent/codex",
                "agency_ref": "agency/notes",
                "world_binding_ref": "world-binding/notes",
                "world_ref": "world/notes",
                "scope_ref": "scope:root",
                "receipt": {},
            },
        });
        let target: ResidentTarget = serde_json::from_value(json!({
            "space": "session-space/notes-chat",
            "agent_session": "agent-session/notes-chat",
            "socket": "/tmp/aikit-owner.sock",
        }))
        .unwrap();
        let native = json!({"ok": true, "data": {
            "agent_session": "agent-session/notes-chat",
            "space": "session-space/notes-chat",
            "native_session_id": "019a6e2c-0000-7000-8000-000000000001",
            "model_observation": {
                "current_model_id": observed,
                "available_models": [{"modelId": observed, "name": observed}],
                "standing": "provider-reported-configuration-not-independent-selection-or-inference-proof",
            },
            "provider": "codex",
            "protocol": "acp",
            "body_basis": {
                "schema": "aikit.resident-body-basis/v1",
                "provider_id": "codex",
                "protocol": "acp",
                "provider_argv_digest": "a".repeat(64),
                "owner_launcher_provider_id": "codex",
                "owner_launcher_argv_digest": "a".repeat(64),
                "effective_launch_argv_digest": "b".repeat(64),
                "harness_profile": "codex",
                "task_bound": false,
                "cwd": "/tmp/notes",
                "required_context": null,
                "model_basis_digest": null,
            },
            "model_selection": null,
            "resident": true,
        }});
        (compose, target, native)
    }

    fn sorted_digest(value: &Value) -> String {
        digest(value).unwrap()
    }

    #[test]
    fn a_seated_session_is_bound_by_a_receipt_naming_the_catalogue_model_it_runs() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let (compose, target, native) = seat("gpt-5.4");
        let selection = seated_factory_selection(&home, &compose, &target, &native).unwrap();
        let receipt = &selection["ranking_explanation"];
        assert_eq!(selection["ranking_policy"], "EXPLICIT_PIN");
        assert_eq!(receipt["schema"], EXPLICIT_SELECTION_SCHEMA);
        assert_eq!(receipt["selection_kind"], SEATED_SESSION);
        // The seed catalogue claims gpt-5.4 through provider:openai, the provider codex binds natively.
        assert_eq!(selection["model_ref"], "model:gpt-5.4");
        assert_eq!(selection["provider_ref"], "provider:openai");
        let basis = &receipt["basis"];
        let route_basis = &basis["route_basis"];
        assert_eq!(route_basis["model_ref"], "model:gpt-5.4");
        assert_eq!(route_basis["provider_native_id"], "gpt-5.4");
        assert!(route_basis["unmatched_offer"].is_null());
        assert_eq!(
            receipt["route_ref"],
            format!("model-route/{}", sorted_digest(route_basis))
        );
        assert_eq!(
            receipt["basis_digest"],
            format!("blake3:{}", sorted_digest(basis))
        );
        assert_eq!(receipt["harness_ref"], "harness/codex");
        assert_eq!(
            basis["harness_composition"]["session"],
            "agent-session/notes-chat"
        );
        assert_eq!(basis["harness_composition"]["model"], "model:gpt-5.4");
        assert_eq!(
            receipt["harness_composition_ref"],
            format!(
                "harness-composition/{}",
                basis["harness_composition"]["fingerprint"]
                    .as_str()
                    .unwrap()
            )
        );
        assert_eq!(
            basis["composition_target_basis"]["resident_body_basis"]["harness_profile"],
            "codex"
        );
        assert_eq!(basis["native"]["agent_session"], "agent-session/notes-chat");
        assert_eq!(basis["native"]["space"], "session-space/notes-chat");
    }

    #[test]
    fn a_seated_model_no_catalogue_model_claims_is_named_by_its_route_and_stays_an_offer() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let (compose, target, native) = seat("gpt-6-astra");
        let selection = seated_factory_selection(&home, &compose, &target, &native).unwrap();
        let receipt = &selection["ranking_explanation"];
        let route_basis = &receipt["basis"]["route_basis"];
        // No Model is minted from the observed id: the route names what the session runs.
        assert!(route_basis["model_ref"].is_null());
        assert_eq!(selection["model_ref"], receipt["route_ref"]);
        assert!(selection["model_ref"]
            .as_str()
            .unwrap()
            .starts_with("model-route/"));
        assert_eq!(
            route_basis["unmatched_offer"]["provider_native_id"],
            "gpt-6-astra"
        );
        assert_eq!(
            route_basis["unmatched_offer"]["provider_ref"],
            "provider:openai"
        );
        assert_eq!(
            receipt["basis"]["harness_composition"]["model"],
            selection["model_ref"]
        );
    }

    /// A session on a provider-plural harness over ACP (gemini, kimi), as the resident owner reads it back: the model it
    /// runs and no provider. AIKit records no native provider for an ACP session, and the models it advertises carry no
    /// roster identity.
    fn undisclosed(harness: &str, observed: &str) -> (Value, ResidentTarget, Value) {
        let (compose, target, mut native) = seat(observed);
        native["data"]["provider"] = json!(harness);
        let body = &mut native["data"]["body_basis"];
        body["harness_profile"] = json!(harness);
        body["provider_id"] = json!(harness);
        body["owner_launcher_provider_id"] = json!(harness);
        (compose, target, native)
    }

    #[test]
    fn a_seated_session_whose_provider_nothing_discloses_is_answered_for_its_harness() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let (compose, target, native) = undisclosed("gemini", "gemini-2.5-pro");
        let selection = seated_factory_selection(&home, &compose, &target, &native).unwrap();
        let receipt = &selection["ranking_explanation"];
        let basis = &receipt["basis"];
        let route_basis = &basis["route_basis"];
        assert_eq!(selection["ranking_policy"], "EXPLICIT_PIN");
        assert_eq!(receipt["selection_kind"], SEATED_SESSION);
        assert_eq!(receipt["harness_ref"], "harness/gemini");
        // No provider is named that the session did not disclose, and none is read out of the model id.
        assert!(selection["provider_ref"].is_null(), "{selection}");
        assert!(receipt["provider_ref"].is_null());
        assert!(route_basis["provider_ref"].is_null());
        assert!(route_basis["native_provider"].is_null());
        // The route names the model by the id the harness runs, through that harness: no catalogue Model is claimed.
        assert!(route_basis["model_ref"].is_null());
        assert_eq!(route_basis["provider_native_id"], "gemini-2.5-pro");
        let offer = &route_basis["unmatched_offer"];
        assert!(offer["provider_ref"].is_null());
        assert_eq!(offer["harness_ref"], "harness/gemini");
        assert_eq!(offer["provider_native_id"], "gemini-2.5-pro");
        assert_eq!(selection["model_ref"], receipt["route_ref"]);
        assert_eq!(
            receipt["route_ref"],
            format!("model-route/{}", sorted_digest(route_basis))
        );
        assert_eq!(
            receipt["basis_digest"],
            format!("blake3:{}", sorted_digest(basis))
        );
        assert_eq!(
            basis["harness_composition"]["model"],
            selection["model_ref"]
        );
        assert_eq!(
            basis["composition_target_basis"]["resident_body_basis"]["harness_profile"],
            "gemini"
        );
        // The same id run through another harness is another route.
        let (compose, target, native) = undisclosed("kimi", "gemini-2.5-pro");
        let kimi = seated_factory_selection(&home, &compose, &target, &native).unwrap();
        assert_eq!(kimi["ranking_explanation"]["harness_ref"], "harness/kimi");
        assert_ne!(kimi["model_ref"], selection["model_ref"]);
    }
}
