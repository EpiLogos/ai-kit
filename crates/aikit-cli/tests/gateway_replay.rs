//! `aikit gateway replay`: one Stream's frames from a running gateway's own
//! journal, through the real binary — the events the gateway appended for a
//! conversation, in sequence, after a point and within a limit, with
//! `has_more` naming whether more follow. A Stream the gateway holds no
//! journal for is refused in the gateway's words; with no gateway running the
//! read fails honestly; an out-of-range limit is refused before anything is
//! asked.

#[path = "support/serve_guard.rs"]
mod serve_guard;
use serve_guard::ServeGuard;

use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use aikit_adapters::{
    ConnectorCapabilities, ConnectorDescriptor, ConnectorOperation, GATEWAY_CONNECTOR_SDK_VERSION,
};
use aikit_core::ResourceRef;
use serde_json::{json, Value};
use tempfile::TempDir;

const STREAM: &str = "actuation-stream/replay-fixture";

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

fn run(home: &std::path::Path, args: &[&str]) -> (bool, Value) {
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
    (output.status.success(), envelope)
}

fn exchange(home: &std::path::Path, command: Value) -> Value {
    let mut stream = UnixStream::connect(home.join("state/gateway.sock")).unwrap();
    writeln!(stream, "{}", json!({ "command": command })).unwrap();
    stream.flush().unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

fn wait_for_socket(home: &std::path::Path) {
    let socket = home.join("state/gateway.sock");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !socket.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(socket.exists(), "serve must bind the default endpoint");
}

/// A conversation bound on the running gateway, and `count` messages in it:
/// each ingest appends one event to the binding's Stream.
fn conversation(home: &std::path::Path, count: usize) {
    let r = |value: &str| ResourceRef::parse(value).unwrap();
    let connector = ConnectorDescriptor {
        version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
        connector_ref: r("gateway-connector/replay/fixture"),
        platform: "replay".into(),
        implementation: "gateway-replay-test".into(),
        capabilities: ConnectorCapabilities {
            operations: BTreeSet::from([ConnectorOperation::Send]),
            max_text_bytes: Some(4096),
            max_media_bytes: None,
            media_types: BTreeSet::new(),
            provenance: vec!["fixture".into()],
        },
        configuration_ref: None,
        provenance: vec!["fixture".into()],
    };
    let registered = exchange(
        home,
        json!({"type": "register-connector", "descriptor": connector}),
    );
    assert_eq!(registered["ok"], true, "{registered}");
    let bound = exchange(
        home,
        json!({"type": "bind", "binding": {
            "binding_ref": "gateway-binding/replay",
            "connector_ref": "gateway-connector/replay/fixture",
            "address": {"platform": "replay", "conversation_id": "main"},
            "agent_session_ref": "agent-session/replay",
            "agency_ref": "agency/replay",
            "actuation_ref": "actuation/replay",
            "actuation_stream_ref": STREAM,
            "ingress": {"default": "allow", "sender_overrides": {}}
        }}),
    );
    assert_eq!(bound["ok"], true, "{bound}");
    for index in 1..=count {
        let ingested = exchange(
            home,
            json!({"type": "ingest", "event": {
                "event_ref": format!("gateway-ingress/replay-{index}"),
                "connector_ref": "gateway-connector/replay/fixture",
                "address": {"platform": "replay", "conversation_id": "main"},
                "sender": {"native_sender_id": "owner", "kind": "human"},
                "kind": "message",
                "native_event_id": format!("replay-native-{index}"),
                "native_message_id": format!("replay-message-{index}"),
                "text": format!("frame {index}")
            }}),
        );
        assert_eq!(ingested["ok"], true, "{ingested}");
    }
}

#[test]
fn replay_reads_one_streams_frames_after_a_sequence_within_a_limit() {
    let home = TempDir::new().unwrap();
    let mut serve = ServeGuard::new(
        Command::new(bin())
            .args(["gateway", "serve"])
            .env("AIKIT_HOME", home.path())
            .env("HOME", home.path())
            .current_dir(home.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("aikit gateway serve should spawn"),
    );
    wait_for_socket(home.path());
    conversation(home.path(), 3);

    // From the beginning: all three frames, in sequence, nothing more.
    let (ok, envelope) = run(home.path(), &["gateway", "replay", "--stream", STREAM]);
    assert!(ok, "{envelope}");
    let replay = &envelope["data"]["replay"];
    assert_eq!(envelope["data"]["type"], "replay", "{envelope}");
    assert_eq!(replay["stream_ref"], STREAM);
    let sequences: Vec<u64> = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["sequence"].as_u64().unwrap())
        .collect();
    assert_eq!(sequences, [1, 2, 3]);
    assert_eq!(replay["events"][0]["event"]["content"], "frame 1");
    assert_eq!(replay["stream_last_sequence"], 3);
    assert_eq!(replay["has_more"], false);

    // After the first, one at a time: the second, and more follow.
    let (ok, envelope) = run(
        home.path(),
        &[
            "gateway", "replay", "--stream", STREAM, "--after", "1", "--limit", "1",
        ],
    );
    assert!(ok, "{envelope}");
    let replay = &envelope["data"]["replay"];
    assert_eq!(replay["after_sequence"], 1);
    assert_eq!(replay["returned_through"], 2);
    assert_eq!(replay["events"].as_array().unwrap().len(), 1);
    assert_eq!(replay["events"][0]["event"]["content"], "frame 2");
    assert_eq!(replay["has_more"], true);

    // A Stream the gateway holds no journal for: the gateway's own refusal.
    let (ok, envelope) = run(
        home.path(),
        &["gateway", "replay", "--stream", "actuation-stream/none"],
    );
    assert!(!ok, "{envelope}");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no journal for Stream actuation-stream/none"),
        "{envelope}"
    );
    // An out-of-range limit is refused before the gateway is asked.
    let (ok, envelope) = run(
        home.path(),
        &["gateway", "replay", "--stream", STREAM, "--limit", "0"],
    );
    assert!(!ok, "{envelope}");
    assert_eq!(envelope["error"]["code"], "cli.usage", "{envelope}");

    let stopped = exchange(home.path(), json!({"type": "shutdown"}));
    assert_eq!(stopped["ok"], true, "{stopped}");
    serve.wait().unwrap();
}

/// `aikit gateway agent --binding B sessions` — the document O-I's kernel
/// reads for the Gateway device — carries the binding's link from the
/// running gateway's own state: its frames (each stamped as the gateway
/// journaled it), their count, the check-in's age, the turns running and the
/// frames in flight.
#[test]
fn the_sessions_reading_carries_the_bindings_link_through_the_binary() {
    let home = TempDir::new().unwrap();
    let mut serve = ServeGuard::new(
        Command::new(bin())
            .args(["gateway", "serve"])
            .env("AIKIT_HOME", home.path())
            .env("HOME", home.path())
            .current_dir(home.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("aikit gateway serve should spawn"),
    );
    wait_for_socket(home.path());
    conversation(home.path(), 3);

    let (ok, envelope) = run(
        home.path(),
        &[
            "gateway",
            "agent",
            "--binding",
            "gateway-binding/replay",
            "sessions",
        ],
    );
    assert!(ok, "{envelope}");
    // The carrier's answer wraps the engine's result: `data.result` is the document.
    let link = &envelope["data"]["result"]["link"];
    assert_eq!(link["frame_count"], 3, "{envelope}");
    assert_eq!(link["last_sequence"], 3, "{envelope}");
    assert_eq!(
        link["turns_running"], 0,
        "no backing runs a turn: {envelope}"
    );
    assert_eq!(link["frames_in_flight"], 0, "{envelope}");
    let notes: Vec<&str> = link["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["note"].as_str().unwrap())
        .collect();
    assert_eq!(notes, ["frame 1", "frame 2", "frame 3"]);
    assert!(link["frames"]
        .as_array()
        .unwrap()
        .iter()
        .all(|frame| frame["direction"] == "to-agent"
            && frame["recorded_at_unix_ms"].as_u64().is_some()));
    assert_eq!(
        link["last_check_in_unix_ms"], link["frames"][2]["recorded_at_unix_ms"],
        "{envelope}"
    );
    assert!(link["check_in_age_seconds"].as_f64().unwrap() >= 0.0);

    let stopped = exchange(home.path(), json!({"type": "shutdown"}));
    assert_eq!(stopped["ok"], true, "{stopped}");
    serve.wait().unwrap();
}

#[test]
fn replay_without_a_running_gateway_fails_honestly() {
    let home = TempDir::new().unwrap();
    let (ok, envelope) = run(home.path(), &["gateway", "replay", "--stream", STREAM]);
    assert!(!ok, "{envelope}");
    assert_eq!(envelope["error"]["code"], "cli.gateway_unreachable");
}
