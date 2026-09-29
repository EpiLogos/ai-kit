//! `aikit now-context contemplate` over the real controlled-protocol Jev
//! transport (real `curl` -> a minimal local HTTP server), proving: an
//! explicit relation is mandatory and never asked about; a candidate above
//! the relevance threshold is selected; usage/cost/standing are retained; a
//! field document changed mid-invocation refuses the decision.

use aikit_cli::cli::NowContemplateArgs;
use aikit_cli::contemplation_field::FIELD_SCHEMA;
use aikit_cli::contemplation_intel::now_contemplate;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    time::Duration,
};

fn fixture_field() -> Value {
    json!({
        "schema": FIELD_SCHEMA,
        "pass": "prospective",
        "telos": Value::Null,
        "spine": {"source": "/fixture/ux-spine-trace.json", "practices": []},
        "matrix": {
            "matrix_id": "matrix.fixture",
            "source_manifest": "/fixture/capability-matrix.json",
            "source_csv": "/fixture/capability-matrix.csv",
            "capabilities": [
                {"id": "cap.explicit", "need": "n1", "operation": "o1", "outcome": "out1", "implementation_status": "implemented", "standing": "implementation-fact", "test_refs": []},
                {"id": "cap.candidate", "need": "n2", "operation": "o2", "outcome": "out2", "implementation_status": "implemented", "standing": "implementation-fact", "test_refs": []},
            ],
        },
        "changed_subject": {"repo_name": "fixture-repo", "base_revision": "git:aaa", "head_revision": "git:bbb", "changed_paths": ["src/one.rs"]},
        "code_lens": Value::Null,
        "joins": [
            {"from": "src/one.rs", "to": "cap.explicit", "relation": "changed-path-implements-capability", "basis": "explicit"},
        ],
        "tests_evidence": [],
        "experience_reading": Value::Null,
        "experience_reading_disclosure": "no --experience-reading was supplied",
        "now": Value::Null,
        "redis": Value::Null,
        "return_document": Value::Null,
        "knowledge_frames": [],
    })
}

fn limits() -> Value {
    json!({
        "timeout_ms": 5000,
        "max_attempts": 1,
        "max_total_reserved_microusd": 10000,
        "tariff": {
            "model_version": "jev-1.13.0",
            "source": "controlled conformance tariff; no vendor invoice",
            "max_input_tokens_per_attempt": 64000,
            "max_output_tokens_per_attempt": 64000,
            "input_microusd_per_million_tokens": 42000,
            "output_microusd_per_million_tokens": 0,
        },
    })
}

/// One-shot HTTP server: accepts exactly one connection, reads the request,
/// waits `delay` before replying (so a background writer has time to race a
/// mid-flight file change), then replies with `body`.
struct OneShotServer {
    address: SocketAddr,
    handle: Option<std::thread::JoinHandle<Vec<u8>>>,
}
impl OneShotServer {
    fn start(status: u16, body: Vec<u8>, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut buf = vec![0u8; 64 * 1024];
            let mut received = Vec::new();
            // Read until we see the end of headers + declared content-length,
            // or the peer closes. Bounded by the read timeout above.
            loop {
                match socket.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        received.extend_from_slice(&buf[..n]);
                        if received.windows(4).any(|w| w == b"\r\n\r\n") {
                            // crude but sufficient for this fixed small fixture body
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            std::thread::sleep(delay);
            let reason = if status == 200 { "OK" } else { "Error" };
            let mut response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            response.extend_from_slice(&body);
            let _ = socket.write_all(&response);
            let _ = socket.flush();
            received
        });
        Self {
            address,
            handle: Some(handle),
        }
    }
    fn join(&mut self) -> Vec<u8> {
        self.handle.take().unwrap().join().unwrap()
    }
}

fn has_curl() -> bool {
    std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn explicit_is_mandatory_candidate_is_selected_usage_and_cost_are_retained() {
    if !has_curl() {
        eprintln!("skip: no curl on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let field_path = dir.path().join("field.json");
    std::fs::write(
        &field_path,
        serde_json::to_vec_pretty(&fixture_field()).unwrap(),
    )
    .unwrap();
    let limits_path = dir.path().join("limits.json");
    std::fs::write(&limits_path, serde_json::to_vec_pretty(&limits()).unwrap()).unwrap();

    let response = json!({
        "model": "jev-1.13.0",
        "answers": {
            "capability/cap.candidate": {"type": "noul", "noul": 0.9},
            "evidence-sufficiency/cap.explicit": {
                "type": "choice",
                "choice": "sufficient-at-grade",
                "probabilities": {"absent": 0.0, "contradictory": 0.0, "stale": 0.0, "sufficient-at-grade": 1.0},
                "confidence": 0.95,
            },
        },
        "usage": {"input_tokens": 120, "output_tokens": 10},
    });
    let mut server =
        OneShotServer::start(200, serde_json::to_vec(&response).unwrap(), Duration::ZERO);

    std::env::set_var(
        "AIKIT_TEST_JEV_CONTROLLED_KEY",
        "aikit-controlled-protocol-only",
    );
    let result = now_contemplate(NowContemplateArgs {
        field: field_path.clone(),
        pass: "prospective".into(),
        limits_file: limits_path,
        credential_ref: "env://AIKIT_TEST_JEV_CONTROLLED_KEY".into(),
        controlled_endpoint: Some(server.address),
        curl: None,
        allow_env_import: true,
        relevance_threshold: 0.5,
        invocation_ref: None,
    })
    .unwrap();
    server.join();

    assert_eq!(result["standing"], "controlled-protocol");
    assert_eq!(result["returned_model"], "jev-1.13.0");
    assert_eq!(result["usage"]["input_tokens"], 120);
    assert!(result["tariff_cost_microusd"].as_u64().unwrap() > 0);

    let mandatory = result["mandatory"]["capability-implicated"]
        .as_array()
        .unwrap();
    assert_eq!(mandatory.len(), 1);
    assert_eq!(mandatory[0]["capability_id"], "cap.explicit");
    let answered_ids: Vec<&str> = result["answers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(
        !answered_ids.contains(&"capability/cap.explicit"),
        "an explicit relation must never appear as an asked question"
    );
    let selected = result["selected"]["capability-implicated"]
        .as_array()
        .unwrap();
    assert!(selected
        .iter()
        .any(|s| s["capability_id"] == "cap.candidate"));
}

#[test]
fn a_field_changed_during_the_invocation_refuses_the_decision() {
    if !has_curl() {
        eprintln!("skip: no curl on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let field_path = dir.path().join("field.json");
    std::fs::write(
        &field_path,
        serde_json::to_vec_pretty(&fixture_field()).unwrap(),
    )
    .unwrap();
    let limits_path = dir.path().join("limits.json");
    std::fs::write(&limits_path, serde_json::to_vec_pretty(&limits()).unwrap()).unwrap();

    let response = json!({
        "model": "jev-1.13.0",
        "answers": {
            "capability/cap.candidate": {"type": "noul", "noul": 0.1},
            "evidence-sufficiency/cap.explicit": {
                "type": "choice",
                "choice": "absent",
                "probabilities": {"absent": 1.0, "contradictory": 0.0, "stale": 0.0, "sufficient-at-grade": 0.0},
                "confidence": 0.5,
            },
        },
        "usage": {"input_tokens": 10, "output_tokens": 2},
    });
    // Give the writer thread below time to land before the server replies.
    let mut server = OneShotServer::start(
        200,
        serde_json::to_vec(&response).unwrap(),
        Duration::from_millis(300),
    );

    let field_path_for_writer = field_path.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        let mut changed = fixture_field();
        changed["changed_subject"]["changed_paths"] = json!(["src/two.rs"]);
        std::fs::write(
            &field_path_for_writer,
            serde_json::to_vec_pretty(&changed).unwrap(),
        )
        .unwrap();
    });

    std::env::set_var(
        "AIKIT_TEST_JEV_CONTROLLED_KEY",
        "aikit-controlled-protocol-only",
    );
    let error = now_contemplate(NowContemplateArgs {
        field: field_path,
        pass: "prospective".into(),
        limits_file: limits_path,
        credential_ref: "env://AIKIT_TEST_JEV_CONTROLLED_KEY".into(),
        controlled_endpoint: Some(server.address),
        curl: None,
        allow_env_import: true,
        relevance_threshold: 0.5,
        invocation_ref: None,
    })
    .unwrap_err();
    writer.join().unwrap();
    server.join();
    assert_eq!(error.code(), "contemplation_intel.field_changed");
}
