//! The gateway-native route for a Flow conversation request to a recipient held
//! by another Workcell's encounter owner.
//!
//! Until this, such a request left the requesting owner as an `ssh` command
//! line (`aikit session-space encounter --request-json …`), authenticated by an
//! ssh login, declared inside the request itself, with no feature negotiation
//! and no connection to the endpoints the two gateways already declare for
//! each other. Now the same four requests travel as one gateway command over
//! the authenticated carrier:
//!
//! ```text
//! requesting owner ──EncounterRelay──▶ remote gateway ──▶ remote encounter owner
//!   (conversation)    WebSocket, peer token   (service hook)   (its own admission)
//!                  ◀── {ok, data | error} ◀──────────────────────────────────────
//! ```
//!
//! Nothing new is decided on the way: the remote owner admits, sends and
//! answers exactly as for a local send; the gateways carry the request and the
//! answer. Which route a request takes is decided **once**, here, by
//! negotiation — never "try one, then the other":
//!
//! * the Workcell that holds the session has a declared gateway endpoint, and
//!   it advertises `encounter-request-relay` → **native**;
//! * it has a declared endpoint and answers, but does not advertise the
//!   feature (an older build) → the legacy ssh/exec route, *named* as chosen
//!   because of that missing feature;
//! * it has a declared endpoint and does not answer → **unavailable**: the
//!   request is held and retried, not sent down another path that might
//!   double-deliver;
//! * no endpoint is declared for that Workcell → the legacy route, as before.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use aikit_adapters::{
    gateway_command_within, GatewayCarrierTarget, GatewayCommand, GatewayEncounterRelay,
    GatewayResponse, GATEWAY_FEATURE_ENCOUNTER_RELAY,
};
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::Value;

use crate::gateway_contact::{load_remotes, GatewayRemote};

/// How long one relayed request may take end to end.
const RELAY_TIMEOUT: Duration = Duration::from_secs(25);
/// How long a peer's advertised features are trusted before it is asked again.
const FEATURE_TTL: Duration = Duration::from_secs(30);

/// This Workcell's encounter owner, reached for a peer gateway.
pub struct OwnerEncounterRelay {
    pub home: AikitHome,
}

impl GatewayEncounterRelay for OwnerEncounterRelay {
    fn relay(&self, action: &str, request: Value) -> Result<Value> {
        let mut body = request;
        let Some(object) = body.as_object_mut() else {
            return Err(AikitError::new(
                "gateway.encounter_relay_request",
                "a relayed encounter request is a JSON object",
            ));
        };
        object.insert("action".into(), Value::String(action.to_owned()));
        let request: crate::encounter_service::EncounterRequest = serde_json::from_value(body)
            .map_err(|error| {
                AikitError::new(
                    "gateway.encounter_relay_request",
                    format!(
                        "the relayed {action} request is not a valid encounter request: {error}"
                    ),
                )
            })?;
        #[cfg(unix)]
        {
            crate::encounter_service::request(
                &crate::encounter_service::socket_path(&self.home),
                &request,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            Err(AikitError::new(
                "gateway.encounter_relay_unsupported",
                "the encounter owner needs a unix platform",
            ))
        }
    }
}

/// Where a request to another Workcell's owner goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Over the declared gateway endpoint, with the feature present.
    Native(GatewayRemote),
    /// The ssh/exec route, and why.
    Legacy(String),
    /// A declared endpoint that does not answer: hold and retry.
    Unavailable(String),
}

/// What a peer advertised and when it was asked, by endpoint.
type FeatureCache = Mutex<HashMap<String, (Instant, Vec<String>)>>;

fn feature_cache() -> &'static FeatureCache {
    static CACHE: OnceLock<FeatureCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Forget what peers advertised (a peer that was just upgraded is asked again).
pub fn forget_peer_features() {
    if let Ok(mut cache) = feature_cache().lock() {
        cache.clear();
    }
}

fn target_for(remote: &GatewayRemote) -> Result<GatewayCarrierTarget> {
    let token = crate::secret_location::SecretLocation::parse(&remote.token_location)
        .and_then(|location| location.resolve())?;
    Ok(GatewayCarrierTarget::WebSocket {
        bind: remote.websocket_bind.clone(),
        path: remote.websocket_path.clone(),
        bearer_token: token.expose().to_owned(),
    })
}

/// What the peer advertises: from a recent answer, else by asking it now.
fn peer_features(remote: &GatewayRemote) -> std::result::Result<Vec<String>, String> {
    if let Ok(cache) = feature_cache().lock() {
        if let Some((at, features)) = cache.get(&remote.workcell_ref) {
            if at.elapsed() < FEATURE_TTL {
                return Ok(features.clone());
            }
        }
    }
    let target = target_for(remote).map_err(|error| error.to_string())?;
    match gateway_command_within(
        &target,
        GatewayCommand::Protocol,
        None,
        Duration::from_secs(4),
    ) {
        Ok(GatewayResponse::Protocol { features, .. }) => {
            if let Ok(mut cache) = feature_cache().lock() {
                cache.insert(
                    remote.workcell_ref.clone(),
                    (Instant::now(), features.clone()),
                );
            }
            Ok(features)
        }
        Ok(other) => Err(format!("it answered a protocol request with {other:?}")),
        Err(error) => Err(error.to_string()),
    }
}

/// Decide the route for a request to the owner on `workcell_ref`.
pub fn choose(home: &AikitHome, workcell_ref: &str) -> Choice {
    let declared = load_remotes(home).ok().and_then(|remotes| {
        remotes
            .remotes
            .into_iter()
            .find(|remote| remote.workcell_ref == workcell_ref)
    });
    let Some(remote) = declared else {
        return Choice::Legacy(format!(
            "no gateway endpoint is declared for {workcell_ref}"
        ));
    };
    match peer_features(&remote) {
        Ok(features)
            if features
                .iter()
                .any(|f| f == GATEWAY_FEATURE_ENCOUNTER_RELAY) =>
        {
            Choice::Native(remote)
        }
        Ok(_) => Choice::Legacy(format!(
            "the gateway for {workcell_ref} does not advertise `{GATEWAY_FEATURE_ENCOUNTER_RELAY}` \
             (an older build): upgrade it with `aikit gateway upgrade`"
        )),
        Err(detail) => Choice::Unavailable(format!(
            "the gateway for {workcell_ref} did not answer: {detail}"
        )),
    }
}

/// One request over the native route. `Err((code, message))` carries the
/// owner's own refusal, or `None` for a code when the route itself failed.
pub fn relay_native(
    remote: &GatewayRemote,
    action: &str,
    request: &Value,
) -> std::result::Result<Value, NativeFailure> {
    let target = target_for(remote).map_err(|error| NativeFailure::Route(error.to_string()))?;
    let mut body = request.clone();
    if let Some(object) = body.as_object_mut() {
        object.remove("action");
    }
    match gateway_command_within(
        &target,
        GatewayCommand::EncounterRelay {
            action: action.to_owned(),
            request: body,
        },
        None,
        RELAY_TIMEOUT,
    ) {
        Ok(GatewayResponse::EncounterRelayed { response }) => {
            if response["ok"] == true {
                Ok(response["data"].clone())
            } else {
                Err(NativeFailure::Owner(
                    response["error"]["code"]
                        .as_str()
                        .unwrap_or("conversation.remote_refused")
                        .to_owned(),
                    response["error"]["message"]
                        .as_str()
                        .unwrap_or("the remote owner refused")
                        .to_owned(),
                ))
            }
        }
        Ok(other) => Err(NativeFailure::Route(format!(
            "the gateway answered a relay with {other:?}"
        ))),
        Err(error) => {
            let code = error
                .details()
                .get("gateway_error_code")
                .cloned()
                .unwrap_or_default();
            match code.as_str() {
                // The peer's gateway refuses the command itself: it is older
                // than its advertisement said, or the owner behind it is
                // absent. Either way the request was not delivered.
                "agency_gateway.unsupported_command" | "agency_gateway.invalid_request_json" => {
                    forget_peer_features();
                    Err(NativeFailure::Route(format!(
                        "the peer gateway does not know the relay command: {error}"
                    )))
                }
                "agency_gateway.encounter_relay_not_served" => Err(NativeFailure::Route(
                    "the peer gateway has no encounter owner to relay to".to_owned(),
                )),
                "agency_gateway.encounter_relay_action_denied" => Err(NativeFailure::Owner(
                    "conversation.route".to_owned(),
                    error.to_string(),
                )),
                _ => Err(NativeFailure::Route(error.to_string())),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeFailure {
    /// The owner answered and refused (its own code and message).
    Owner(String, String),
    /// The route failed before an owner answered: hold and retry.
    Route(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workcell_with_no_declared_gateway_keeps_the_legacy_route_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        match choose(&home, "workcell:elsewhere") {
            Choice::Legacy(reason) => assert!(reason.contains("no gateway endpoint is declared")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_relayed_request_must_be_an_object() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        let relay = OwnerEncounterRelay { home };
        let error = relay.relay("send", Value::Null).unwrap_err();
        assert_eq!(error.code(), "gateway.encounter_relay_request");
    }
}
