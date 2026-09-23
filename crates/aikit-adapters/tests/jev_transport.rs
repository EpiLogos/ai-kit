//! Real native curl -> controlled HTTP protocol, not a mocked command runner.
//! No live provider credential, human source or remote model is used here.
use aikit_adapters::jev::{
    CurlJevProvider, JevBoundary, JevCancellation, JevEndpoint, JevOutcome, JevStanding,
};
use aikit_core::{
    jev::{JevLimits, JevRequest, JevTariff},
    AikitError, ResourceRef, SecretValue,
};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

fn request() -> JevRequest {
    JevRequest::parse(&serde_json::to_vec(&json!({"model":"jev-1.13.0","state":{"current":"Backup is still running.\nDo not stop it.","other":"quoted \"source\" with \\ slashes"},"questions":{"defer":{"type":"noul","instructions":"Should the maintenance wait?"}}})).unwrap()).unwrap()
}
fn response() -> Value {
    json!({"model":"jev-1.13.0","answers":{"defer":{"type":"noul","noul":0.98}},"usage":{"input_tokens":101,"output_tokens":7}})
}
fn limits() -> JevLimits {
    JevLimits {
        timeout_ms: 3000,
        max_attempts: 3,
        max_total_reserved_microusd: 10000,
        tariff: JevTariff {
            model_version: "jev-1.13.0".into(),
            source: "controlled conformance tariff; no vendor invoice".into(),
            max_input_tokens_per_attempt: 64000,
            max_output_tokens_per_attempt: 64000,
            input_microusd_per_million_tokens: 42000,
            output_microusd_per_million_tokens: 0,
        },
    }
}
fn id() -> ResourceRef {
    ResourceRef::parse("activity/jev/protocol-test").unwrap()
}
struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(&'static str, &'static str)>,
    delay: Duration,
}
impl Reply {
    fn ok() -> Self {
        Self {
            status: 200,
            body: serde_json::to_vec(&response()).unwrap(),
            headers: vec![],
            delay: Duration::ZERO,
        }
    }
}
struct Server {
    address: SocketAddr,
    seen: Arc<Mutex<Vec<Value>>>,
    done: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
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
                // On macOS/BSD an accepted socket inherits the listener's
                // O_NONBLOCK; read with the timeout, not a spurious WouldBlock.
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let (headers, body) = read_request(&mut socket);
                assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
                assert!(
                    headers.contains("Authorization: Bearer aikit-controlled-protocol-only\r\n")
                );
                record
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                thread::sleep(reply.delay);
                let mut headers=format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",reply.status,reply.body.len());
                for (k, v) in reply.headers {
                    headers.push_str(&format!("{k}: {v}\r\n"));
                }
                headers.push_str("\r\n");
                let _ = socket.write_all(headers.as_bytes());
                let _ = socket.write_all(&reply.body);
            }
        });
        Self {
            address,
            seen,
            done: Some(done),
        }
    }
    fn provider(&self) -> CurlJevProvider {
        CurlJevProvider::new("curl", JevEndpoint::Controlled(self.address))
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
    let split = loop {
        let n = stream.read(&mut buffer).unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break split + 4;
        }
        assert!(bytes.len() < 32768);
    };
    let headers = String::from_utf8(bytes[..split].to_vec()).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() - split < length {
        let n = stream.read(&mut buffer).unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&buffer[..n]);
    }
    (headers, bytes[split..].to_vec())
}
#[test]
fn real_native_http_retains_arbitrary_typed_state_and_actual_usage() {
    let server = Server::start(vec![Reply::ok()]);
    let mut boundaries = Vec::new();
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |b| {
                boundaries.push(b);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(result.outcome, JevOutcome::Completed);
    assert_eq!(result.standing, JevStanding::ControlledProtocol);
    assert_eq!(result.answer.unwrap().usage.input_tokens, 101);
    assert_eq!(result.attempts[0].tariff_cost_microusd, Some(5));
    assert!(matches!(
        boundaries.as_slice(),
        [
            JevBoundary::BeforeAttempt { ordinal: 1, .. },
            JevBoundary::BeforeReturn
        ]
    ));
    assert_eq!(
        server.seen.lock().unwrap()[0],
        serde_json::to_value(request()).unwrap()
    );
}
#[test]
fn missing_answer_is_failure_and_never_zero_cost() {
    let mut reply = Reply::ok();
    reply.body =
        br#"{"model":"jev-1.13.0","answers":{},"usage":{"input_tokens":1,"output_tokens":0}}"#
            .to_vec();
    let server = Server::start(vec![reply]);
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(result.outcome, JevOutcome::Failed);
    assert!(result.answer.is_none());
    assert_eq!(result.failure.unwrap().code, "jev.invalid_answer");
    assert!(result.attempts[0].usage.is_none());
    assert!(result.attempts[0].effect_uncertain);
    assert_eq!(result.total_reserved_microusd, 2688);
}
#[test]
fn explicit_overload_retries_are_bounded_and_unknown_usage_remains_reserved() {
    let mut busy = Reply::ok();
    busy.status = 429;
    busy.headers.push(("Retry-After", "0"));
    let server = Server::start(vec![busy, Reply::ok()]);
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(result.outcome, JevOutcome::Completed);
    assert_eq!(result.attempts.len(), 2);
    assert_eq!(result.total_reserved_microusd, 5376);
    assert!(result.attempts[0].usage.is_none());
    assert_eq!(result.attempts[1].usage.as_ref().unwrap().input_tokens, 101);
}
#[test]
fn budget_refuses_a_second_attempt_before_another_request() {
    let mut busy = Reply::ok();
    busy.status = 529;
    busy.headers.push(("Retry-After", "0"));
    let server = Server::start(vec![busy]);
    let mut bounds = limits();
    bounds.max_total_reserved_microusd = 2688;
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &bounds,
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(result.attempts.len(), 1);
    assert_eq!(result.failure.unwrap().code, "jev.budget_exhausted");
    assert_eq!(server.seen.lock().unwrap().len(), 1);
}
#[test]
fn authentication_failure_is_not_retried_and_diagnostics_do_not_copy_provider_payload() {
    let mut reply = Reply::ok();
    reply.status = 401;
    reply.body = b"private-source-and-credential-echo".to_vec();
    let server = Server::start(vec![reply]);
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(result.attempts.len(), 1);
    assert_eq!(result.outcome, JevOutcome::Failed);
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("private-source-and-credential-echo"));
}
#[test]
fn cancellation_kills_and_reaps_the_real_transport_without_retry() {
    let mut reply = Reply::ok();
    reply.delay = Duration::from_millis(600);
    let server = Server::start(vec![reply]);
    let token = JevCancellation::default();
    let cancellation = token.clone();
    let killer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancellation.cancel();
    });
    let start = Instant::now();
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &token,
            &mut |_| Ok(()),
        )
        .unwrap();
    killer.join().unwrap();
    assert_eq!(result.outcome, JevOutcome::Cancelled);
    assert!(result.answer.is_none());
    assert!(start.elapsed() < Duration::from_millis(550));
    assert_eq!(result.attempts.len(), 1);
}
#[test]
fn timeout_is_finite_and_unknown_transport_is_not_replayed() {
    let mut reply = Reply::ok();
    reply.delay = Duration::from_millis(400);
    let server = Server::start(vec![reply]);
    let mut bounds = limits();
    bounds.timeout_ms = 100;
    let start = Instant::now();
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &bounds,
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(result.outcome, JevOutcome::Failed);
    assert!(start.elapsed() < Duration::from_millis(350));
    assert_eq!(result.attempts.len(), 1);
    assert!(result.attempts[0].effect_uncertain);
}
#[test]
fn disclosure_revocation_after_inference_withholds_the_answer_but_retains_usage() {
    let server = Server::start(vec![Reply::ok()]);
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |boundary| match boundary {
                JevBoundary::BeforeReturn => {
                    Err(AikitError::new("context.revoked", "Disclosure revoked"))
                }
                _ => Ok(()),
            },
        )
        .unwrap();
    assert_eq!(result.outcome, JevOutcome::Failed);
    assert!(result.answer.is_none());
    assert_eq!(result.attempts[0].usage.as_ref().unwrap().input_tokens, 101);
    assert_eq!(result.failure.unwrap().code, "context.revoked");
}
#[test]
fn withdrawal_before_the_attempt_does_not_run_a_transport() {
    let provider = CurlJevProvider::new("a-program-that-does-not-exist", JevEndpoint::Official);
    let result = provider
        .invoke(
            id(),
            &request(),
            &limits(),
            &SecretValue::new("not-a-live-key").unwrap(),
            &JevCancellation::default(),
            &mut |_| Err(AikitError::new("agency.withdrawn", "Withdrawn")),
        )
        .unwrap();
    assert!(result.attempts.is_empty());
    assert_eq!(result.total_reserved_microusd, 0);
    assert_eq!(result.failure.unwrap().code, "agency.withdrawn");
}
#[test]
fn a_changed_returned_model_never_uses_the_old_tariff_as_approval() {
    let mut reply = Reply::ok();
    let mut answer = response();
    answer["model"] = json!("jev-1.14.0");
    reply.body = serde_json::to_vec(&answer).unwrap();
    let server = Server::start(vec![reply]);
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert!(result.answer.is_none());
    assert_eq!(
        result.failure.unwrap().code,
        "jev.provider_budget_basis_changed"
    );
    assert_eq!(
        result.attempts[0].model_version.as_deref(),
        Some("jev-1.14.0")
    );
}
#[test]
fn oversized_response_does_not_become_a_truncated_success() {
    let mut reply = Reply::ok();
    reply.body = vec![b'x'; 1_100_000];
    let server = Server::start(vec![reply]);
    let result = server
        .provider()
        .invoke(
            id(),
            &request(),
            &limits(),
            &JevEndpoint::controlled_key(),
            &JevCancellation::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(result.outcome, JevOutcome::Failed);
    assert!(result.answer.is_none());
    assert_eq!(result.attempts.len(), 1);
}
#[test]
fn controlled_test_route_refuses_external_hosts_and_real_credentials() {
    for endpoint in ["192.0.2.1:8000", "127.0.0.1:8000"] {
        let provider =
            CurlJevProvider::new("curl", JevEndpoint::Controlled(endpoint.parse().unwrap()));
        assert_eq!(
            provider
                .invoke(
                    id(),
                    &request(),
                    &limits(),
                    &SecretValue::new("an-operator-key").unwrap(),
                    &JevCancellation::default(),
                    &mut |_| Ok(())
                )
                .unwrap_err()
                .code(),
            "jev.controlled_boundary"
        );
    }
}
