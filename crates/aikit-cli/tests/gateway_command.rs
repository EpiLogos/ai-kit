//! The Agency Gateway's bootstrap posture, through the real binary.
//!
//! The gateway joins the config surface by having one well-known endpoint:
//! `$AIKIT_HOME/state/gateway.sock` with state in `$AIKIT_HOME/state/gateway.json`.
//! These tests drive that contract: a bare query against a machine without a
//! gateway fails honestly and says how to start one; `aikit gateway serve`
//! with no flags brings up that endpoint; queries find it flagless; doctor
//! accounts for it in every state; a shutdown stops it and persists state.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

fn run(home: &std::path::Path, args: &[&str]) -> (bool, Value, String) {
    let output = Command::new(bin())
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(home)
        .output()
        .unwrap_or_else(|error| panic!("aikit {args:?} should run: {error}"));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; got stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope, stdout)
}

fn wait_for_socket(home: &std::path::Path) {
    let socket = home.join("state/gateway.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !socket.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(socket.exists(), "serve must bind the default endpoint");
}

fn shutdown(home: &std::path::Path) -> Value {
    let mut stream = UnixStream::connect(home.join("state/gateway.sock")).unwrap();
    let request = serde_json::json!({"command": {"type": "shutdown"}}).to_string();
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

#[test]
fn a_bare_query_without_a_gateway_fails_honestly_and_names_the_start_command() {
    let home = TempDir::new().unwrap();
    let (ok, envelope, _) = run(home.path(), &["gateway", "status"]);
    assert!(!ok, "a query with no gateway must fail, not pretend: {envelope}");
    assert_eq!(envelope["ok"], Value::Bool(false));
    let error = &envelope["error"];
    assert_eq!(error["code"], Value::from("cli.gateway_unreachable"));
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("aikit gateway serve"),
        "the failure must say how to start one: {error}"
    );
}

#[test]
fn flagless_serve_binds_the_well_known_endpoint_and_queries_find_it() {
    let home = TempDir::new().unwrap();
    let mut serve = Command::new(bin())
        .args(["gateway", "serve"])
        .env("AIKIT_HOME", home.path())
        .env("HOME", home.path())
        .current_dir(home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("aikit gateway serve should spawn");
    wait_for_socket(home.path());

    let (ok, protocol, _) = run(home.path(), &["gateway", "protocol"]);
    assert!(ok, "{protocol}");
    assert_eq!(
        protocol["data"]["gateway_version"],
        Value::from("aikit.agency-gateway/v1")
    );

    let (ok, ecology, _) = run(home.path(), &["gateway", "ecology"]);
    assert!(ok, "{ecology}");
    assert_eq!(
        ecology["data"]["ecology"]["authority"],
        Value::from("presence-does-not-imply-authority")
    );

    let (ok, doctor, _) = run(home.path(), &["doctor"]);
    assert!(ok, "{doctor}");
    let finding = doctor["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["check"] == Value::from("gateway.service"))
        .expect("doctor must account for the gateway");
    assert_eq!(finding["severity"], Value::from("note"));
    assert!(
        finding["summary"]
            .as_str()
            .unwrap()
            .contains("answers at the default endpoint"),
        "{finding}"
    );

    assert_eq!(shutdown(home.path())["ok"], Value::Bool(true));
    let status = serve.wait().expect("serve should exit after shutdown");
    assert!(status.success(), "serve should stop cleanly: {status}");
    assert!(
        home.path().join("state/gateway.json").exists(),
        "semantic state must persist at the default location"
    );

    // After the stop, doctor tells the truth again: not running, and how to.
    let (_, doctor, _) = run(home.path(), &["doctor"]);
    let finding = doctor["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["check"] == Value::from("gateway.service"))
        .unwrap();
    assert_eq!(finding["severity"], Value::from("note"));
    assert!(
        finding["detail"]
            .as_str()
            .unwrap()
            .contains("aikit gateway serve"),
        "{finding}"
    );
}

#[test]
fn restart_restores_semantic_state_from_the_default_location() {
    let home = TempDir::new().unwrap();
    let state = home.path().join("state/gateway.json");
    std::fs::create_dir_all(state.parent().unwrap()).unwrap();
    std::fs::write(
        &state,
        serde_json::json!({
            "version": "aikit.agency-gateway/v1",
            "gateway_ref": "agency-gateway/persisted",
            "connectors": [],
            "bindings": [],
            "streams": [],
            "connector_health": [],
            "pending_deliveries": [],
            "delivery_receipts": [],
            "next_operation_sequence": 1
        })
        .to_string(),
    )
    .unwrap();

    let mut serve = Command::new(bin())
        .args(["gateway", "serve"])
        .env("AIKIT_HOME", home.path())
        .env("HOME", home.path())
        .current_dir(home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_socket(home.path());
    let (ok, status, _) = run(home.path(), &["gateway", "status"]);
    assert!(ok, "{status}");
    assert_eq!(
        status["data"]["status"]["gateway_ref"],
        Value::from("agency-gateway/persisted"),
        "serve must restore the persisted gateway identity: {status}"
    );
    shutdown(home.path());
    serve.wait().unwrap();
}
