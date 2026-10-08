//! The elected decision-provider surface: configuration law, status, real
//! invocation through the elected provider, and the no-cloud-fallback law.
//! Loopback controlled servers stand in for the endpoint; nothing here
//! contacts the hosted API.
use aikit_cli::decide::{
    invoke_selected, parse_provider_config, DecisionProviderConfig, DecisionProviderMode,
};
use aikit_core::jev::JevRequest;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

const MODEL: &str = "kev-latest";

fn config_value(mode: &str) -> Value {
    json!({
        "schema": "aikit.decision-provider/v1",
        "mode": mode,
        "address": "127.0.0.1:9",
        "limits": {
            "timeout_ms": 2000,
            "max_attempts": 1,
            "max_input_tokens_per_attempt": 8000,
            "max_output_tokens_per_attempt": 2000,
            "model": MODEL
        },
        "decision_model": {
            "family": "kev",
            "artifact": "jaredpalmer/kev-0.8b",
            "base": "Qwen/Qwen3.5-0.8B-Base",
            "base_revision": "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68",
            "runtime": "kev.serve (uv)",
            "backend": "mlx",
            "precision": "bf16-as-stored",
            "calibration": "checkpoint-fitted-temperature",
            "license": "Apache-2.0"
        }
    })
}

fn request() -> JevRequest {
    JevRequest::parse(
        &serde_json::to_vec(&json!({
            "model": MODEL,
            "state": {"undertaking": "Route the support ticket."},
            "questions": {"escalate": {"type": "noul", "instructions": "Urgent?"}}
        }))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn the_configuration_law_keeps_each_placement_honest() {
    let none = parse_provider_config(
        &serde_json::to_vec(&json!({
            "schema": "aikit.decision-provider/v1",
            "mode": "none",
            "address": "127.0.0.1:8009"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        none.validate().unwrap_err().code(),
        "decision.config_invalid"
    );

    let hosted = parse_provider_config(&serde_json::to_vec(&json!({
        "schema": "aikit.decision-provider/v1",
        "mode": "hosted",
        "address": "127.0.0.1:8009",
        "credential_ref": "keychain://dev.aikit.credentials/typesafe",
        "jev_limits": {
            "timeout_ms": 1000, "max_attempts": 1, "max_total_reserved_microusd": 5000,
            "tariff": {"model_version": "jev-1.13.0", "source": "docs.typesafe.ai",
                "max_input_tokens_per_attempt": 8000, "max_output_tokens_per_attempt": 2000,
                "input_microusd_per_million_tokens": 42000, "output_microusd_per_million_tokens": 0}
        }
    }))
    .unwrap())
    .unwrap();
    assert_eq!(
        hosted.validate().unwrap_err().code(),
        "decision.config_invalid"
    );

    let well_formed: DecisionProviderConfig =
        parse_provider_config(&serde_json::to_vec(&config_value("managed-local")).unwrap())
            .unwrap();
    well_formed.validate().unwrap();
    assert!(matches!(
        well_formed.mode,
        DecisionProviderMode::ManagedLocal
    ));

    let bad_schema = parse_provider_config(
        &serde_json::to_vec(&json!({
            "schema": "aikit.decision-provider/v2", "mode": "none"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        bad_schema.validate().unwrap_err().code(),
        "decision.config_schema"
    );
}

#[test]
fn a_mode_none_election_refuses_invocation_and_names_the_ordinary_path() {
    let none = parse_provider_config(
        &serde_json::to_vec(&json!({
            "schema": "aikit.decision-provider/v1", "mode": "none"
        }))
        .unwrap(),
    )
    .unwrap();
    let error = none.endpoint().unwrap_err();
    assert_eq!(error.code(), "decision.provider_disabled");
    let mut no_revalidate = || -> aikit_core::Result<()> { Ok(()) };
    let error = invoke_selected(
        &none,
        &request(),
        aikit_core::ResourceRef::parse("activity/decision/none").unwrap(),
        None,
        false,
        &mut no_revalidate,
    )
    .unwrap_err();
    assert_eq!(error.code(), "decision.provider_disabled");
}

#[test]
fn an_unreachable_local_service_is_a_visible_failure_never_a_cloud_fallback() {
    // Port 9 (discard) on loopback: nothing decision-shaped listens there.
    let config =
        parse_provider_config(&serde_json::to_vec(&config_value("managed-local")).unwrap())
            .unwrap();
    let mut no_revalidate = || -> aikit_core::Result<()> { Ok(()) };
    let receipt = invoke_selected(
        &config,
        &request(),
        aikit_core::ResourceRef::parse("activity/decision/unreachable").unwrap(),
        None,
        false,
        &mut no_revalidate,
    )
    .unwrap();
    assert!(!receipt.outcome_completed());
    let message = receipt.failure_message().unwrap_or_default();
    assert!(
        message.contains("curl") || message.contains("transport") || message.contains("refused"),
        "unexpected failure message: {message}"
    );
    // The receipt records the local loopback standing; no hosted fallback
    // exists on this path, so the failure is the whole answer.
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert_eq!(encoded["standing"], "local-protocol");
    assert_eq!(encoded["endpoint"], "http://127.0.0.1:9/v1/systemone");
}

#[test]
fn a_local_election_with_the_same_request_but_a_changed_provider_moves_the_identity_digest() {
    let before =
        parse_provider_config(&serde_json::to_vec(&config_value("managed-local")).unwrap())
            .unwrap();
    let mut changed_value = config_value("endpoint");
    changed_value["decision_model"]["precision"] = json!("fp32-exact");
    let changed = parse_provider_config(&serde_json::to_vec(&changed_value).unwrap()).unwrap();
    let before_digest =
        aikit_cli::decide::DecisionReceipt::provider_identity_digest(&before).unwrap();
    let changed_digest =
        aikit_cli::decide::DecisionReceipt::provider_identity_digest(&changed).unwrap();
    assert_ne!(before_digest, changed_digest);
}

#[test]
fn status_reports_mode_none_as_the_ordinary_path_without_touching_a_network() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("provider.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schema": "aikit.decision-provider/v1", "mode": "none"
        }))
        .unwrap(),
    )
    .unwrap();
    let status = aikit_cli::decide::decide_status(aikit_cli::cli::DecideStatusArgs {
        provider_file: Some(path),
        probe: true,
        curl: None,
        allow_env_import: false,
    })
    .unwrap();
    assert_eq!(status["mode"], "none");
    assert!(status["placement"].is_null());
    assert!(status["ordinary_path"]
        .as_str()
        .unwrap()
        .contains("ordinary operation does not require one"));
}

#[test]
fn status_and_invoke_against_a_controlled_local_endpoint_report_served_facts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let answer = json!({
        "model": MODEL,
        "answers": {"escalate": {"type": "noul", "noul": 0.12}},
        "usage": {"input_tokens": 42, "output_tokens": 4}
    });
    let diagnostic_answer = json!({
        "model": MODEL,
        "answers": {"diagnostic": {"type": "noul", "noul": 1.0}},
        "usage": {"input_tokens": 17, "output_tokens": 1}
    });
    let card = json!({
        "models": [{"name": MODEL, "backend": "mlx", "dtype": "bfloat16",
                    "temperature": 2.1, "device": "mlx",
                    "base": "Qwen/Qwen3.5-0.8B-Base"}]
    });
    let done = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        for body in [card, diagnostic_answer, answer] {
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
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 8192];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                bytes.extend_from_slice(&buffer[..n]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let encoded = serde_json::to_vec(&body).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                encoded.len()
            );
            let _ = socket.write_all(head.as_bytes());
            let _ = socket.write_all(&encoded);
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let mut provider = config_value("managed-local");
    provider["address"] = json!(address.to_string());
    let path = temp.path().join("provider.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&provider).unwrap()).unwrap();

    let status = aikit_cli::decide::decide_status(aikit_cli::cli::DecideStatusArgs {
        provider_file: Some(path.clone()),
        probe: true,
        curl: None,
        allow_env_import: false,
    })
    .unwrap();
    assert_eq!(status["mode"], "managed-local");
    assert_eq!(status["placement"], address.to_string());
    assert_eq!(status["selected_model"], MODEL);
    assert_eq!(status["install"]["state"], "loaded");
    assert_eq!(status["served"]["models"][0]["backend"], "mlx");
    assert_eq!(
        status["decision_model"]["base_revision"],
        "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68"
    );
    assert_eq!(status["diagnostic"]["outcome"], "completed");
    assert_eq!(
        status["limits"]["tariff"],
        "none — a local/self-hosted endpoint has no price source; none is invented"
    );

    let request_path = temp.path().join("request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec(&json!({
            "model": MODEL,
            "state": {"ticket": "Late delivery."},
            "questions": {"escalate": {"type": "noul", "instructions": "Urgent?"}}
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = aikit_cli::decide::decide_invoke(aikit_cli::cli::DecideInvokeArgs {
        provider_file: Some(path),
        request_file: request_path,
        invocation_ref: None,
        curl: None,
        allow_env_import: false,
    })
    .unwrap();
    assert_eq!(receipt["outcome"], "completed");
    assert_eq!(receipt["answer"]["answers"]["escalate"]["noul"], 0.12);
    assert_eq!(receipt["standing"], "local-protocol");
    done.join().unwrap();
}
