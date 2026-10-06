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
use std::path::PathBuf;
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

/// The selected Home and the complete declared carrier basis of an observation.
/// Token material is resolved at use and never enters this key.
#[derive(Clone, PartialEq, Eq, Hash)]
struct FeatureCacheKey {
    home: PathBuf,
    workcell_ref: String,
    websocket_bind: String,
    websocket_path: String,
    token_location: String,
}

impl FeatureCacheKey {
    fn new(home: &AikitHome, remote: &GatewayRemote) -> Self {
        Self {
            home: home.root().to_path_buf(),
            workcell_ref: remote.workcell_ref.clone(),
            websocket_bind: remote.websocket_bind.clone(),
            websocket_path: remote.websocket_path.clone(),
            token_location: remote.token_location.clone(),
        }
    }
}

/// What that declared peer advertised and when it was asked.
type FeatureCache = Mutex<HashMap<FeatureCacheKey, (Instant, Vec<String>)>>;

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
fn peer_features(
    home: &AikitHome,
    remote: &GatewayRemote,
) -> std::result::Result<Vec<String>, String> {
    let key = FeatureCacheKey::new(home, remote);
    if let Ok(cache) = feature_cache().lock() {
        if let Some((at, features)) = cache.get(&key) {
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
                cache.retain(|basis, (at, _)| {
                    at.elapsed() < FEATURE_TTL
                        && (basis.home != key.home || basis.workcell_ref != key.workcell_ref)
                });
                cache.insert(key, (Instant::now(), features.clone()));
            }
            Ok(features)
        }
        Ok(other) => Err(format!("it answered a protocol request with {other:?}")),
        Err(error) => Err(error.to_string()),
    }
}

/// Decide the route for a request to the owner on `workcell_ref`.
pub fn choose(home: &AikitHome, workcell_ref: &str) -> Choice {
    // A registry that cannot be read is not "no endpoint declared": an operator
    // who declared one must not be silently routed over ssh because the file is
    // damaged. The request is held, with the reason.
    let remotes = match load_remotes(home) {
        Ok(remotes) => remotes,
        Err(error) => {
            return Choice::Unavailable(format!(
                "the declared gateway endpoints cannot be read, so no route is chosen \
                 (nothing was sent over any other route): {error}"
            ))
        }
    };
    let declared = remotes
        .remotes
        .into_iter()
        .find(|remote| remote.workcell_ref == workcell_ref);
    let Some(remote) = declared else {
        return Choice::Legacy(format!(
            "no gateway endpoint is declared for {workcell_ref}"
        ));
    };
    match peer_features(home, &remote) {
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
    fn a_damaged_endpoint_registry_holds_the_request_and_never_falls_back_to_ssh() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        std::fs::create_dir_all(
            crate::gateway_contact::remotes_path(&home)
                .parent()
                .unwrap(),
        )
        .unwrap();
        std::fs::write(crate::gateway_contact::remotes_path(&home), b"{ not json").unwrap();
        match choose(&home, "workcell:omarchy") {
            Choice::Unavailable(reason) => {
                assert!(reason.contains("cannot be read"), "{reason}");
                assert!(
                    reason.contains("nothing was sent over any other route"),
                    "{reason}"
                );
            }
            other => panic!("a damaged registry must hold, not route: {other:?}"),
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

#[cfg(all(test, unix))]
mod native_cache_tests {
    use super::*;
    use aikit_adapters::gateway_service::{
        run_gateway_service_on_websocket_listener, GatewayServiceConfig, GatewayServiceHooks,
    };
    use aikit_adapters::AgencyGateway;
    use aikit_core::resource::ResourceRef;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::net::{SocketAddr, TcpListener};
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::{self, JoinHandle};

    // These tests exercise the process-global cache's explicit invalidation.
    static CASES: Mutex<()> = Mutex::new(());

    struct LiveGateway {
        remote: GatewayRemote,
        stop: Arc<AtomicBool>,
        done: mpsc::Receiver<Result<()>>,
        worker: Option<JoinHandle<()>>,
    }

    impl LiveGateway {
        fn retire(&mut self) -> std::result::Result<(), String> {
            if self.worker.is_none() {
                return Ok(());
            }
            self.stop.store(true, Ordering::SeqCst);
            let result = self
                .done
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| format!("gateway retirement not observed: {error}"))?;
            let joined = self.worker.take().unwrap().join();
            if joined.is_err() {
                return Err("gateway service thread panicked after its result".into());
            }
            result.map_err(|error| format!("gateway service ended with its actual error: {error}"))
        }
    }

    struct NativeFixture {
        material: Option<tempfile::TempDir>,
        gateways: Vec<LiveGateway>,
    }

    impl NativeFixture {
        fn new() -> Self {
            let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../ProjectCentral/now/tmp");
            std::fs::create_dir_all(&scratch).unwrap();
            let scratch = scratch.canonicalize().unwrap();
            Self {
                material: Some(
                    tempfile::Builder::new()
                        .prefix("gw-")
                        .tempdir_in(scratch)
                        .unwrap(),
                ),
                gateways: Vec::new(),
            }
        }

        fn home(&self, member: &str) -> AikitHome {
            AikitHome::at(self.material.as_ref().unwrap().path().join(member))
        }

        fn start(&mut self, member: &str, allocate_in_service: bool) -> usize {
            let root = self.material.as_ref().unwrap().path();
            let token_path = root.join(format!("{member}.token"));
            let token = format!("owned-gateway-{member}-bearer");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&token_path)
                .unwrap();
            file.write_all(token.as_bytes()).unwrap();
            drop(file);
            // The websocket listener is always pre-bound here, whatever the
            // unix-socket allocation does: the actual address is known by
            // construction, and the service must serve exactly the listener
            // it was handed.
            let listener = Some(TcpListener::bind("127.0.0.1:0").unwrap());
            let address = listener
                .as_ref()
                .map(|listener| listener.local_addr().unwrap().to_string())
                .unwrap();
            let socket = root.join("s");
            if allocate_in_service {
                use std::os::unix::ffi::OsStrExt;
                assert!(socket.as_os_str().as_bytes().len() < 104);
            }
            let config = GatewayServiceConfig {
                websocket_bind: Some(address.clone()),
                websocket_bearer_token: Some(token),
                unix_socket: allocate_in_service.then(|| socket.clone()),
                state_file: None,
                max_frame_bytes: 64 * 1024,
            };
            let stop = Arc::new(AtomicBool::new(false));
            let hooks = GatewayServiceHooks {
                stop_signal: Some(Arc::clone(&stop)),
                ..Default::default()
            };
            let gateway =
                AgencyGateway::new(ResourceRef::parse(format!("gateway:cache-{member}")).unwrap());
            let (send, done) = mpsc::channel();
            let worker = thread::spawn(move || {
                // The websocket listener is always pre-bound here: the actual
                // address is known by construction, and the service must serve
                // exactly the listener it was handed.
                let result = run_gateway_service_on_websocket_listener(
                    gateway,
                    config,
                    hooks,
                    listener.expect("fixture always pre-binds its websocket listener"),
                );
                let _ = send.send(result);
            });
            let index = self.gateways.len();
            self.gateways.push(LiveGateway {
                remote: GatewayRemote {
                    workcell_ref: "workcell:cache-proof".into(),
                    websocket_bind: address,
                    websocket_path: "/".into(),
                    token_location: format!("file:{}", token_path.display()),
                },
                stop,
                done,
                worker: Some(worker),
            });
            index
        }

        fn declare(&self, home: &AikitHome, remote: &GatewayRemote) {
            crate::gateway_contact::remote_add(
                home,
                &remote.workcell_ref,
                &remote.websocket_bind,
                &remote.websocket_path,
                &remote.token_location,
            )
            .unwrap();
        }

        fn retire(&mut self, index: usize) {
            self.gateways[index].retire().unwrap();
        }
    }

    impl Drop for NativeFixture {
        fn drop(&mut self) {
            let failures = self
                .gateways
                .iter_mut()
                .filter_map(|gateway| gateway.retire().err())
                .collect::<Vec<_>>();
            if !failures.is_empty() {
                let retained = self.material.take().unwrap().keep();
                let detail = format!(
                    "actual gateway retirement failed: {failures:?}; material retained at {}",
                    retained.display()
                );
                if thread::panicking() {
                    eprintln!("{detail}");
                } else {
                    panic!("{detail}");
                }
            }
        }
    }

    #[test]
    fn a_current_gateway_observation_cannot_cross_selected_homes() {
        let _serial = CASES.lock().unwrap();
        let mut fixture = NativeFixture::new();
        let index = fixture.start("home-scope", false);
        let remote = fixture.gateways[index].remote.clone();
        let first = fixture.home("first");
        let second = fixture.home("second");
        fixture.declare(&first, &remote);
        fixture.declare(&second, &remote);
        assert_eq!(
            choose(&first, &remote.workcell_ref),
            Choice::Native(remote.clone())
        );
        let observed = Instant::now();
        fixture.retire(index);
        assert!(observed.elapsed() < FEATURE_TTL);
        // The existing TTL still accelerates exactly the admitted key.
        assert_eq!(
            choose(&first, &remote.workcell_ref),
            Choice::Native(remote.clone())
        );
        assert!(matches!(
            choose(&second, &remote.workcell_ref),
            Choice::Unavailable(_)
        ));
        forget_peer_features();
        assert!(matches!(
            choose(&first, &remote.workcell_ref),
            Choice::Unavailable(_)
        ));
    }

    #[test]
    fn endpoint_and_credential_reconfiguration_require_the_current_owner_probe() {
        let _serial = CASES.lock().unwrap();
        let mut fixture = NativeFixture::new();
        let first = fixture.start("endpoint-first", false);
        let second = fixture.start("endpoint-second", false);
        let first_remote = fixture.gateways[first].remote.clone();
        let second_remote = fixture.gateways[second].remote.clone();
        let home = fixture.home("reconfigured");
        fixture.declare(&home, &first_remote);
        assert_eq!(
            choose(&home, &first_remote.workcell_ref),
            Choice::Native(first_remote.clone())
        );
        let mut wrong_credential = second_remote.clone();
        wrong_credential.token_location = first_remote.token_location.clone();
        fixture.declare(&home, &wrong_credential);
        assert!(matches!(
            choose(&home, &wrong_credential.workcell_ref),
            Choice::Unavailable(_)
        ));
        fixture.declare(&home, &second_remote);
        assert_eq!(
            choose(&home, &second_remote.workcell_ref),
            Choice::Native(second_remote.clone())
        );
        // The re-declared endpoint is not just admitted by the cache: the
        // gateway it names actually answers there.
        let target = target_for(&second_remote).unwrap();
        let answer = gateway_command_within(
            &target,
            GatewayCommand::Protocol,
            None,
            Duration::from_secs(4),
        )
        .unwrap();
        assert!(matches!(answer, GatewayResponse::Protocol { .. }));
    }

    #[test]
    fn a_prebound_port_zero_listener_serves_at_its_actual_address_only() {
        let _serial = CASES.lock().unwrap();
        let mut fixture = NativeFixture::new();
        let index = fixture.start("allocated", true);
        let remote = fixture.gateways[index].remote.clone();
        // The fixture pre-bound port 0, so the declared bind is the listener's
        // actual address: nonzero, concrete, and the only address served.
        let address: SocketAddr = remote.websocket_bind.parse().unwrap();
        assert!(address.port() > 0);
        assert_ne!(address.to_string(), "127.0.0.1:0");
        let home = fixture.home("allocated-home");
        fixture.declare(&home, &remote);
        assert_eq!(
            choose(&home, &remote.workcell_ref),
            Choice::Native(remote.clone())
        );
        // The gateway answers at the actual address, and nothing can even be
        // contacted on the literal configured port 0 coordinate.
        let answer = gateway_command_within(
            &target_for(&remote).unwrap(),
            GatewayCommand::Protocol,
            None,
            Duration::from_secs(4),
        )
        .unwrap();
        assert!(matches!(answer, GatewayResponse::Protocol { .. }));
        assert!(
            std::net::TcpStream::connect("127.0.0.1:0").is_err(),
            "nothing serves the literal port 0"
        );
        fixture.retire(index);
    }

    #[test]
    fn a_held_listener_cannot_be_relabelled_as_another_declared_address() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(listener.local_addr().unwrap().port() > 0);
        let error = run_gateway_service_on_websocket_listener(
            AgencyGateway::new(ResourceRef::parse("gateway:held-refusal").unwrap()),
            GatewayServiceConfig {
                websocket_bind: Some("127.0.0.1:0".into()),
                websocket_bearer_token: Some("owned-refusal-bearer".into()),
                unix_socket: None,
                state_file: None,
                max_frame_bytes: 64 * 1024,
            },
            GatewayServiceHooks::default(),
            listener,
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            "agency_gateway_service.websocket_listener_mismatch"
        );
    }

    #[test]
    fn the_cache_key_retains_every_declared_target_coordinate() {
        let home = AikitHome::at("selected-home");
        let remote = GatewayRemote {
            workcell_ref: "workcell:key-proof".into(),
            websocket_bind: "127.0.0.1:1".into(),
            websocket_path: "/".into(),
            token_location: "file:/declared-location".into(),
        };
        let basis = FeatureCacheKey::new(&home, &remote);
        assert!(basis == FeatureCacheKey::new(&home, &remote));
        assert!(basis != FeatureCacheKey::new(&AikitHome::at("another-home"), &remote));
        for field in 0..4 {
            let mut changed = remote.clone();
            match field {
                0 => changed.workcell_ref.push_str("-changed"),
                1 => changed.websocket_bind = "127.0.0.1:2".into(),
                2 => changed.websocket_path = "/changed".into(),
                3 => changed.token_location = "file:/another-location".into(),
                _ => unreachable!(),
            }
            assert!(basis != FeatureCacheKey::new(&home, &changed));
        }
    }
}
