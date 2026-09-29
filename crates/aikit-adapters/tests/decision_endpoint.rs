//! Real native curl -> controlled loopback HTTP for the endpoint decision
//! transport. This proves the production transport's own laws against a
//! controlled stand-in server; it never impersonates a live credentialed
//! provider and never touches a remote host.
use aikit_adapters::decision_endpoint::{
    probe_models, DecisionEndpoint, DecisionStanding, EndpointDecisionProvider,
};
use aikit_adapters::jev::{JevBoundary, JevCancellation, JevOutcome};
use aikit_core::jev::{DecisionLimits, JevRequest};
use aikit_core::{ResourceRef, SecretValue};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const MODEL: &str = "kev-latest";

fn request() -> JevRequest {
    JevRequest::parse(&serde_json::to_vec(&json!({
        "model": MODEL,
        "state": {"ticket": "Shoes arrived late and in the wrong size."},
        "questions": {
            "escalate": {"type": "noul", "instructions": "Does this need urgent human attention?"},
            "department": {"type": "choice", "instructions": "Which team?",
                "criteria": {"returns": "Exchanges and refunds", "shipping": "Delivery status"}}
        }
    }))
    .unwrap())
    .unwrap()
}
fn answer() -> Value {
    json!({
        "model": MODEL,
        "answers": {
            "escalate": {"type": "noul", "noul": 0.93},
            "department": {"type": "choice", "choice": "shipping",
                "probabilities": {"returns": 0.47, "shipping": 0.53}, "confidence": 0.21}
        },
        "usage": {"input_tokens": 101, "output_tokens": 33}
    })
}
fn limits() -> DecisionLimits {
    DecisionLimits {
        timeout_ms: 3000,
        max_attempts: 2,
        max_input_tokens_per_attempt: 64_000,
        max_output_tokens_per_attempt: 4_096,
        model: MODEL.into(),
    }
}
fn id() -> ResourceRef {
    ResourceRef::parse("activity/decision/endpoint-test").unwrap()
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(&'static str, &'static str)>,
}
struct Server {
    address: SocketAddr,
    seen: Arc<Mutex<Vec<Value>>>,
    auth_seen: Arc<Mutex<Vec<Option<String>>>>,
    done: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let auth_seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let record_auth = auth_seen.clone();
        let done = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for reply in replies {
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return;
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("server accept: {e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let (headers, body) = read_request(&mut socket);
                assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
                record
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                record_auth.lock().unwrap().push(
                    headers
                        .lines()
                        .find(|l| l.starts_with("Authorization:"))
                        .map(str::to_owned),
                );
                let mut headers = format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reply.body.len()
                );
                for (k, v) in reply.headers {
                    headers.push_str(&format!("{k}: {v}\r\n"));
                }
                let _ = socket.write_all(headers.as_bytes());
                let _ = socket.write_all(&reply.body);
            }
        });
        Self {
            address,
            seen,
            auth_seen,
            done: Some(done),
        }
    }
    fn endpoint(&self) -> DecisionEndpoint {
        DecisionEndpoint::resolve(&self.address.to_string(), false).unwrap()
    }
    fn provider(&self) -> EndpointDecisionProvider {
        EndpointDecisionProvider::new("curl", self.endpoint())
    }
    fn seen_requests(&self) -> Vec<Value> {
        self.seen.lock().unwrap().clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            done.join().unwrap();
        }
    }
}
fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = stream.read(&mut buffer).unwrap();
        bytes.extend_from_slice(&buffer[..n]);
        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..split]).to_string();
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().ok())
                    })
                    .flatten()
                    .unwrap_or(0);
                let body_started = split + 4;
                if bytes.len() >= body_started + length {
                    return (head, bytes[body_started..body_started + length].to_vec());
                }
            }
        }
    }
}

fn invoke(
    provider: &EndpointDecisionProvider,
    secret: Option<&SecretValue>,
) -> aikit_adapters::decision_endpoint::DecisionInvocation {
    let mut guard = |_: JevBoundary| -> aikit_core::Result<()> { Ok(()) };
    provider
        .invoke(
            id(),
            &request(),
            &limits(),
            secret,
            &JevCancellation::default(),
            &mut guard,
        )
        .unwrap()
}

#[test]
fn an_explicitly_unauthenticated_local_endpoint_serves_typed_decisions_without_a_key_or_tariff() {
    let server = Server::start(vec![Reply {
        status: 200,
        body: serde_json::to_vec(&answer()).unwrap(),
        headers: vec![],
    }]);
    let endpoint = server.endpoint();
    assert_eq!(*endpoint.standing(), DecisionStanding::LocalProtocol);
    assert!(endpoint.base_url().starts_with("http://127.0.0.1:"));
    let receipt = invoke(&server.provider(), None);
    assert_eq!(
        receipt.outcome,
        JevOutcome::Completed,
        "failure: {:?}",
        receipt.failure
    );
    assert!(receipt.failure.is_none());
    assert!(!receipt.attempts[0].effect_uncertain);
    // No credential was sent: the server observed no Authorization header, and
    // the receipt carries usage but never an invented tariff cost.
    assert_eq!(server.auth_seen.lock().unwrap().clone(), [None]);
    assert_eq!(
        receipt.attempts[0].usage.as_ref().unwrap().input_tokens,
        101
    );
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert!(encoded.get("total_reserved_microusd").is_none());
    assert!(encoded.get("tariff").is_none() && encoded.get("limits").is_some());
    // The typed questions reached the endpoint verbatim, including the
    // agent-formulated instructions.
    let seen = server.seen_requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["model"], MODEL);
    assert_eq!(seen[0]["questions"]["escalate"]["type"], "noul");
}

#[test]
fn a_mismatched_model_identity_refuses_the_determination_instead_of_misattributing_it() {
    let mut wrong = answer();
    wrong["model"] = json!("some-other-model");
    let server = Server::start(vec![Reply {
        status: 200,
        body: serde_json::to_vec(&wrong).unwrap(),
        headers: vec![],
    }]);
    let receipt = invoke(&server.provider(), None);
    assert_eq!(receipt.outcome, JevOutcome::Failed);
    assert_eq!(
        receipt.failure.as_ref().unwrap().code,
        "decision.model_mismatch"
    );
    assert!(receipt.answer.is_none());
}

#[test]
fn incomplete_or_malformed_answers_never_become_decisions() {
    let mut incomplete = answer();
    incomplete["answers"]
        .as_object_mut()
        .unwrap()
        .remove("department");
    let mut unnormalised = answer();
    unnormalised["answers"]["department"]["probabilities"] =
        json!({"returns": 0.9, "shipping": 0.3});
    for body in [incomplete, unnormalised, json!({"model": MODEL})] {
        let server = Server::start(vec![Reply {
            status: 200,
            body: serde_json::to_vec(&body).unwrap(),
            headers: vec![],
        }]);
        let receipt = invoke(&server.provider(), None);
        assert_eq!(receipt.outcome, JevOutcome::Failed);
        assert_eq!(receipt.failure.as_ref().unwrap().code, "jev.invalid_answer");
    }
}

#[test]
fn an_overloaded_endpoint_is_retried_only_within_the_declared_bounds() {
    let server = Server::start(vec![
        Reply {
            status: 429,
            body: b"{}".to_vec(),
            headers: vec![],
        },
        Reply {
            status: 200,
            body: serde_json::to_vec(&answer()).unwrap(),
            headers: vec![],
        },
    ]);
    let receipt = invoke(&server.provider(), None);
    assert_eq!(receipt.outcome, JevOutcome::Completed);
    assert_eq!(receipt.attempts.len(), 2);
    assert_eq!(receipt.attempts[0].http_status, Some(429));
}

#[test]
fn a_server_error_is_a_visible_failure_with_no_cloud_fallback() {
    let server = Server::start(vec![Reply {
        status: 503,
        body: b"{}".to_vec(),
        headers: vec![],
    }]);
    let receipt = invoke(&server.provider(), None);
    assert_eq!(receipt.outcome, JevOutcome::Failed);
    assert_eq!(
        receipt.failure.as_ref().unwrap().code,
        "decision.endpoint_http"
    );
    assert!(receipt.answer.is_none());
    assert_eq!(receipt.attempts.len(), 1);
}

#[test]
fn configured_bearer_credentials_travel_over_the_private_header_only() {
    let server = Server::start(vec![Reply {
        status: 200,
        body: serde_json::to_vec(&answer()).unwrap(),
        headers: vec![],
    }]);
    let secret = SecretValue::new("local-optional-bearer").unwrap();
    let receipt = invoke(&server.provider(), Some(&secret));
    assert_eq!(receipt.outcome, JevOutcome::Completed);
    let auth = server.auth_seen.lock().unwrap().clone();
    assert_eq!(
        auth,
        [Some("Authorization: Bearer local-optional-bearer".into())]
    );
}

#[test]
fn endpoint_resolution_keeps_local_http_loopback_only_and_refuses_silent_remote_widening() {
    let local = DecisionEndpoint::resolve("127.0.0.1:8009", false).unwrap();
    assert_eq!(*local.standing(), DecisionStanding::LocalProtocol);
    assert!(local.base_url().starts_with("http://"));
    assert!(local.base_url().ends_with("/v1/systemone"));

    let remote = "192.0.2.10:9000"; // TEST-NET, never contacted
    let refused = DecisionEndpoint::resolve(remote, false).unwrap_err();
    assert_eq!(refused.code(), "decision.remote_refused");
    let admitted = DecisionEndpoint::resolve(remote, true).unwrap();
    assert_eq!(*admitted.standing(), DecisionStanding::SelfHostedProtocol);
    assert!(admitted.base_url().starts_with("https://"));

    let scheme = DecisionEndpoint::resolve("http://127.0.0.1:8009", false).unwrap_err();
    assert_eq!(scheme.code(), "decision.endpoint_invalid");
}

#[test]
fn the_model_card_diagnostic_reports_what_the_endpoint_actually_serves() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let done = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return,
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let _ = read_request(&mut socket);
        let body = serde_json::to_vec(&json!({
            "models": [{
                "name": MODEL,
                "description": "Kev pointer head on Qwen/Qwen3.5-0.8B-Base, serving jaredpalmer/kev-0.8b at temperature 2.10",
                "base": "Qwen/Qwen3.5-0.8B-Base",
                "backend": "mlx",
                "dtype": "bfloat16",
                "temperature": 2.1
            }]
        }))
        .unwrap();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.write_all(&body);
    });
    let endpoint = DecisionEndpoint::resolve(&address.to_string(), false).unwrap();
    let card = probe_models("curl", &endpoint, 2000, None).unwrap();
    done.join().unwrap();
    assert_eq!(card["models"][0]["name"], MODEL);
    assert_eq!(card["models"][0]["backend"], "mlx");
    assert_eq!(card["models"][0]["temperature"], 2.1);
}

#[test]
fn cancellation_stops_waiting_and_marks_the_effect_uncertain() {
    // A listener that accepts but never answers: the transport must stay
    // cancellable rather than wait out its whole deadline.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let held = listener.accept().unwrap().0;
        thread::sleep(Duration::from_millis(900));
        drop(held);
    });
    let endpoint = DecisionEndpoint::resolve(&address.to_string(), false).unwrap();
    let provider = EndpointDecisionProvider::new("curl", endpoint);
    let cancellation = JevCancellation::default();
    let handle = {
        let cancellation = cancellation.clone();
        thread::spawn(move || {
            let mut guard = |_: JevBoundary| -> aikit_core::Result<()> { Ok(()) };
            provider
                .invoke(
                    id(),
                    &request(),
                    &DecisionLimits {
                        timeout_ms: 30_000,
                        ..limits()
                    },
                    None,
                    &cancellation,
                    &mut guard,
                )
                .unwrap()
        })
    };
    thread::sleep(Duration::from_millis(150));
    cancellation.cancel();
    let receipt = handle.join().unwrap();
    assert_eq!(receipt.outcome, JevOutcome::Cancelled);
    assert!(receipt.attempts[0].effect_uncertain);
    assert_eq!(receipt.failure.as_ref().unwrap().code, "jev.cancelled");
}
