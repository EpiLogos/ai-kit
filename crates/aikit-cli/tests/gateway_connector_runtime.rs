//! Connector runtime conformance, through the real `aikit` binary.
//!
//! These tests prove the gateway service actually runs connectors: the stdio
//! specimen connector is configured as this home would configure it, the
//! service spawns it through the public out-of-process wire seam, its inbound
//! events land in kernel state and the persisted snapshot, a prepared
//! outbound operation rides the pump to a recorded delivery receipt, an
//! operation the specimen does not advertise fails conformance without
//! corrupting state, and the configuration plane refuses token values and
//! unusable token locations.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tempfile::TempDir;

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

fn specimen_bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("gateway-connector-specimen")
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
    let deadline = Instant::now() + Duration::from_secs(30);
    while !socket.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(socket.exists(), "serve must bind the default endpoint");
}

fn exchange(socket: &std::path::Path, request: Value) -> Value {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    writeln!(stream, "{request}").unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

fn shutdown(home: &std::path::Path) {
    let response = exchange(
        &home.join("state/gateway.sock"),
        json!({"command": {"type": "shutdown"}}),
    );
    assert_eq!(response["ok"], Value::Bool(true), "{response}");
}

fn snapshot(home: &std::path::Path) -> Value {
    let (ok, envelope, _) = run(home, &["gateway", "snapshot"]);
    assert!(ok, "snapshot must read: {envelope}");
    envelope["data"]["snapshot"].clone()
}

fn poll_until(what: &str, timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn token_file(dir: &std::path::Path, name: &str, mode: u32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, "connector-bot-token-value\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

fn write_connectors_file(home: &std::path::Path, connectors: Value) {
    let path = home.join("state/gateway-connectors.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::json!({
            "schema": "aikit.gateway-connectors/v1",
            "connectors": connectors
        })
        .to_string(),
    )
    .unwrap();
}

fn specimen_program(extra: &[&str]) -> Value {
    let mut program = vec![specimen_bin().display().to_string()];
    program.extend(extra.iter().map(|part| part.to_string()));
    serde_json::Value::Array(program.into_iter().map(Value::String).collect())
}

fn spawn_serve(home: &std::path::Path) -> std::process::Child {
    Command::new(bin())
        .args(["gateway", "serve"])
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env_remove("AIKIT_GATEWAY_TOKEN")
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("aikit gateway serve should spawn")
}

fn bind_specimen(home: &std::path::Path) {
    let socket = home.join("state/gateway.sock");
    let binding = json!({
        "command": {
            "type": "bind",
            "binding": {
                "binding_ref": "gateway-binding/specimen",
                "connector_ref": "gateway-connector/specimen/main",
                "address": {"platform": "specimen", "conversation_id": "main"},
                "agent_session_ref": "agent-session/specimen",
                "agency_ref": "agency/specimen",
                "actuation_ref": "actuation/specimen",
                "actuation_stream_ref": "actuation-stream/specimen",
                "ingress": {"default": "allow", "sender_overrides": {}}
            }
        }
    });
    poll_until(
        "the pump registers the specimen so the bind lands",
        Duration::from_secs(30),
        || {
            let response = exchange(&socket, binding.clone());
            response["ok"] == Value::Bool(true)
        },
    );
}

#[test]
fn the_service_runs_the_specimen_connector_end_to_end_over_the_public_wire_seam() {
    let home = TempDir::new().unwrap();
    write_connectors_file(
        home.path(),
        json!([{
            "connector_ref": "gateway-connector/specimen/main",
            "platform": "specimen",
            "implementation": "stdio",
            "program": specimen_program(&[
                "--connector-ref", "gateway-connector/specimen/main",
                "--emit-inbound", "ping-from-specimen",
                "--emit-inbound-delay-ms", "2500",
                "--health-detail", "specimen hello",
            ])
        }]),
    );
    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    bind_specimen(home.path());

    // The pump ingested the specimen's inbound event into kernel state, and
    // the mutated state persisted to the snapshot.
    poll_until(
        "the inbound event lands in the stream",
        Duration::from_secs(30),
        || {
            let events = snapshot(home.path())["streams"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|stream| stream["events"].as_array().unwrap().iter())
                .cloned()
                .collect::<Vec<_>>();
            events.len() == 1 && events[0]["event"]["content"] == "ping-from-specimen"
        },
    );
    let health = snapshot(home.path())["connector_health"]
        .as_array()
        .unwrap()
        .iter()
        .find(|health| health["connector_ref"] == "gateway-connector/specimen/main")
        .cloned();
    let health = health.expect("the specimen connector reports health");
    assert_eq!(health["state"], "connected", "{health}");
    // The child's own health frame is what reached the kernel, proving
    // health observations flow over the wire into `gateway status`.
    assert_eq!(health["detail"], "specimen hello", "{health}");

    // The echoed outbound: the gateway prepares a Send whose text echoes the
    // inbound, the pump hands it to the specimen over the wire, and the
    // specimen's execution marker lands as a recorded DeliveryReceipt.
    let prepared = exchange(
        &home.path().join("state/gateway.sock"),
        json!({
            "command": {
                "type": "prepare-operation",
                "binding_ref": "gateway-binding/specimen",
                "operation": {"kind": "send", "text": "echo: ping-from-specimen"}
            }
        }),
    );
    assert_eq!(prepared["ok"], Value::Bool(true), "{prepared}");
    poll_until(
        "the echoed outbound lands as a recorded receipt",
        Duration::from_secs(30),
        || {
            snapshot(home.path())["delivery_receipts"]
                .as_array()
                .map(|receipts| receipts.len() == 1)
                .unwrap_or(false)
        },
    );
    let receipts = snapshot(home.path())["delivery_receipts"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(receipts[0]["state"], "delivered", "{}", receipts[0]);
    let detail = receipts[0]["detail"].as_str().unwrap();
    assert!(detail.contains("specimen executed send"), "{detail}");
    assert!(
        detail.contains("echo: ping-from-specimen"),
        "the receipt echoes the text: {detail}"
    );
    assert_eq!(receipts[0]["native_message_id"], "specimen-message-out-1");

    // The state file itself carries the recorded receipt: persisted, not
    // just in-memory.
    let persisted = std::fs::read_to_string(home.path().join("state/gateway.json")).unwrap();
    assert!(persisted.contains("specimen executed send"), "{persisted}");

    // An operation the specimen deliberately does not advertise fails
    // conformance and corrupts nothing.
    let refused = exchange(
        &home.path().join("state/gateway.sock"),
        json!({
            "command": {
                "type": "prepare-operation",
                "binding_ref": "gateway-binding/specimen",
                "operation": {"kind": "edit", "native_message_id": "specimen-message-out-1", "text": "nope"}
            }
        }),
    );
    assert_eq!(refused["ok"], Value::Bool(false), "{refused}");
    assert_eq!(
        refused["error"]["code"],
        "gateway_connector.unsupported_operation"
    );
    assert!(refused["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Edit"));
    assert_eq!(
        snapshot(home.path())["pending_deliveries"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "a refused operation must not leave pending state"
    );

    shutdown(home.path());
    let status = serve.wait().expect("serve should exit after shutdown");
    assert!(status.success(), "serve should stop cleanly: {status}");
}

#[test]
fn duplicate_native_events_are_ingested_once_and_the_skip_is_recorded() {
    let home = TempDir::new().unwrap();
    write_connectors_file(
        home.path(),
        json!([{
            "connector_ref": "gateway-connector/specimen/main",
            "platform": "specimen",
            "implementation": "stdio",
            "program": specimen_program(&[
                "--connector-ref", "gateway-connector/specimen/main",
                "--emit-inbound", "same-text",
                "--emit-inbound", "same-text",
                "--emit-inbound-delay-ms", "2500",
            ])
        }]),
    );
    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    bind_specimen(home.path());

    poll_until(
        "the duplicate collapses to one appended event",
        Duration::from_secs(30),
        || {
            let events = snapshot(home.path())["streams"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|stream| stream["events"].as_array().unwrap().iter())
                .cloned()
                .collect::<Vec<_>>();
            events.len() == 1 && events[0]["event"]["content"] == "same-text"
        },
    );
    // The second emission carries the same native_event_id, so the pump
    // skipped it and said so in the connector's health detail.
    poll_until(
        "the skip is visible in connector health",
        Duration::from_secs(30),
        || {
            snapshot(home.path())["connector_health"]
                .as_array()
                .unwrap()
                .iter()
                .any(|health| {
                    health["detail"]
                        .as_str()
                        .map(|detail| detail.contains("1 duplicate events skipped"))
                        .unwrap_or(false)
                })
        },
    );

    shutdown(home.path());
    serve.wait().unwrap();
}

#[test]
fn subscribe_delivers_live_pushes_and_reconnect_replays_cover_the_gap() {
    let home = TempDir::new().unwrap();
    write_connectors_file(
        home.path(),
        json!([{
            "connector_ref": "gateway-connector/specimen/main",
            "platform": "specimen",
            "implementation": "stdio",
            "program": specimen_program(&[
                "--connector-ref", "gateway-connector/specimen/main",
                "--emit-inbound", "first",
                "--emit-inbound", "second",
                "--emit-inbound", "third",
                "--emit-inbound", "fourth",
                "--emit-inbound-delay-ms", "2500",
                "--emit-inbound-interval-ms", "700",
            ])
        }]),
    );
    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());
    bind_specimen(home.path());

    let socket = home.path().join("state/gateway.sock");
    let far_deadline = Instant::now() + Duration::from_secs(30);
    let mut read_line = |stream: &UnixStream| -> Value {
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(count) if count > 0 => {
                    return serde_json::from_str(line.trim()).unwrap();
                }
                _ if Instant::now() >= far_deadline => {
                    panic!("timed out reading a gateway push");
                }
                _ => continue,
            }
        }
    };

    // Wait for the first event, then subscribe from the journal's beginning.
    // Exactly where in the emission schedule the subscribe lands is not
    // assumed: the replay answers first, covering whatever already appended.
    poll_until(
        "the first specimen event appends",
        Duration::from_secs(30),
        || {
            snapshot(home.path())["streams"]
                .as_array()
                .map(|streams| {
                    streams
                        .iter()
                        .flat_map(|stream| stream["events"].as_array().unwrap().iter())
                        .count()
                        >= 1
                })
                .unwrap_or(false)
        },
    );
    let mut client = UnixStream::connect(&socket).unwrap();
    writeln!(
        client,
        "{}",
        json!({
            "command": {"type": "subscribe", "stream_ref": "actuation-stream/specimen", "after_sequence": 0}
        })
    )
    .unwrap();
    let replay = read_line(&client);
    assert_eq!(replay["response"]["type"], "replay", "{replay}");
    let seen: Vec<u64> = replay["response"]["replay"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["sequence"].as_u64().unwrap())
        .collect();
    let contiguous: Vec<u64> = (1..=seen.len() as u64).collect();
    assert_eq!(
        seen, contiguous,
        "the replay covers the journal contiguously: {replay}"
    );

    // Every appended event past the replay cursor is pushed live on the
    // connection, in order, each as its own stream-event frame.
    let contents = ["first", "second", "third"];
    let mut last = seen.len() as u64;
    while last < 3 {
        let push = read_line(&client);
        assert_eq!(push["response"]["type"], "stream-event", "{push}");
        last += 1;
        assert_eq!(push["response"]["event"]["sequence"], last, "{push}");
        assert_eq!(
            push["response"]["event"]["event"]["content"],
            contents[(last - 1) as usize],
            "{push}"
        );
    }

    // The client goes away; the fourth event appends while it is disconnected.
    drop(client);
    poll_until(
        "the fourth event appends while no one listens",
        Duration::from_secs(30),
        || {
            snapshot(home.path())["streams"]
                .as_array()
                .map(|streams| {
                    streams
                        .iter()
                        .flat_map(|stream| stream["events"].as_array().unwrap().iter())
                        .count()
                        == 4
                })
                .unwrap_or(false)
        },
    );

    // Re-Subscribe from the last seen sequence: replay covers the gap (4)
    // without repeating what the pushes already delivered, and nothing
    // between the replay snapshot and the live attachment can be missed.
    let mut client = UnixStream::connect(&socket).unwrap();
    writeln!(
        client,
        "{}",
        json!({
            "command": {"type": "subscribe", "stream_ref": "actuation-stream/specimen", "after_sequence": 3}
        })
    )
    .unwrap();
    let replay = read_line(&client);
    let covered: Vec<u64> = replay["response"]["replay"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["sequence"].as_u64().unwrap())
        .collect();
    assert_eq!(
        covered,
        vec![4],
        "replay covers exactly the away gap: {replay}"
    );
    assert_eq!(
        replay["response"]["replay"]["events"].as_array().unwrap()[0]["event"]["content"],
        "fourth"
    );

    shutdown(home.path());
    serve.wait().unwrap();
}

#[test]
fn the_connectors_file_refuses_token_values_and_unusable_token_locations() {
    let home = TempDir::new().unwrap();
    let loose = token_file(home.path(), "loose.token", 0o644);
    let missing = home.path().join("no-such.token");

    // A token value on the command line is refused outright.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "telegram",
            "--ref",
            "gateway-connector/telegram/main",
            "--token",
            "bot-token-value",
        ],
    );
    assert!(!ok, "a token value must be refused: {envelope}");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(message.contains("--token-location"), "{message}");
    assert!(
        !home.path().join("state/gateway-connectors.json").exists(),
        "no connector file may appear for a refused declaration"
    );

    // A world-readable token location is refused, as serve refuses its own.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "telegram",
            "--ref",
            "gateway-connector/telegram/main",
            "--token-location",
            &format!("file:{}", loose.display()),
        ],
    );
    assert!(!ok, "a loose token file must be refused: {envelope}");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("owner"),
        "{envelope}"
    );

    // A missing token location is refused too.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "telegram",
            "--ref",
            "gateway-connector/telegram/main",
            "--token-location",
            &format!("file:{}", missing.display()),
        ],
    );
    assert!(!ok, "a missing token file must be refused: {envelope}");

    // The usable declaration lands, and list discloses the location, never
    // the material.
    let usable = token_file(home.path(), "usable.token", 0o600);
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "telegram",
            "--ref",
            "gateway-connector/telegram/main",
            "--token-location",
            &format!("file:{}", usable.display()),
            "--configuration-ref",
            "gateway-config/telegram/main",
        ],
    );
    assert!(ok, "the usable declaration must land: {envelope}");
    let (ok, envelope, stdout) = run(home.path(), &["gateway", "connector", "list"]);
    assert!(ok, "{envelope}");
    let connectors = envelope["data"]["connectors"].as_array().unwrap();
    assert_eq!(connectors.len(), 1);
    assert_eq!(
        connectors[0]["connector_ref"],
        "gateway-connector/telegram/main"
    );
    assert_eq!(connectors[0]["platform"], "telegram");
    assert_eq!(connectors[0]["implementation"], "telegram");
    assert_eq!(connectors[0]["enabled"], Value::Bool(true));
    assert_eq!(
        connectors[0]["token_location"],
        format!("file:{}", usable.display())
    );
    assert!(
        !stdout.contains("bot-token-value"),
        "the token material must never be listed: {stdout}"
    );
    let stored =
        std::fs::read_to_string(home.path().join("state/gateway-connectors.json")).unwrap();
    assert!(!stored.contains("bot-token-value"), "{stored}");

    // Disabled by flag; listed as disabled.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "telegram",
            "--ref",
            "gateway-connector/telegram/main",
            "--token-location",
            &format!("file:{}", usable.display()),
            "--disable",
        ],
    );
    assert!(ok, "{envelope}");
    let (_, envelope, _) = run(home.path(), &["gateway", "connector", "list"]);
    assert_eq!(
        envelope["data"]["connectors"][0]["enabled"],
        Value::Bool(false)
    );

    // Remove.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "remove",
            "--ref",
            "gateway-connector/telegram/main",
        ],
    );
    assert!(ok, "{envelope}");
    let (_, envelope, _) = run(home.path(), &["gateway", "connector", "list"]);
    assert_eq!(envelope["data"]["connectors"].as_array().unwrap().len(), 0);
}

#[test]
fn slack_connector_declaration_flows_through_the_service_config_plane() {
    let home = TempDir::new().unwrap();
    let loose = token_file(home.path(), "slack-loose.token", 0o644);

    // The telegram discipline covers slack: a world-readable token file is
    // refused at declaration time.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "slack",
            "--ref",
            "gateway-connector/slack/main",
            "--token-location",
            &format!("file:{}", loose.display()),
        ],
    );
    assert!(!ok, "a loose slack token file must be refused: {envelope}");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("owner"),
        "{envelope}"
    );

    // No token location: the slack validation names the requirement.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "slack",
            "--ref",
            "gateway-connector/slack/main",
        ],
    );
    assert!(!ok, "a slack declaration without a token location must be refused: {envelope}");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(message.contains("--token-location"), "{message}");

    // The usable declaration lands; the implementation defaults to the
    // platform name and list discloses the location, never the material.
    let usable = token_file(home.path(), "slack.token", 0o600);
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "slack",
            "--ref",
            "gateway-connector/slack/main",
            "--token-location",
            &format!("file:{}", usable.display()),
            "--configuration-ref",
            "gateway-config/slack/main",
        ],
    );
    assert!(ok, "the usable slack declaration must land: {envelope}");
    let (ok, envelope, stdout) = run(home.path(), &["gateway", "connector", "list"]);
    assert!(ok, "{envelope}");
    let connectors = envelope["data"]["connectors"].as_array().unwrap();
    assert_eq!(connectors.len(), 1);
    assert_eq!(connectors[0]["connector_ref"], "gateway-connector/slack/main");
    assert_eq!(connectors[0]["platform"], "slack");
    assert_eq!(connectors[0]["implementation"], "slack");
    assert_eq!(
        connectors[0]["token_location"],
        format!("file:{}", usable.display())
    );
    assert!(
        !stdout.contains("connector-bot-token-value"),
        "the token material must never be listed: {stdout}"
    );
    let stored =
        std::fs::read_to_string(home.path().join("state/gateway-connectors.json")).unwrap();
    assert!(!stored.contains("connector-bot-token-value"), "{stored}");

    // Remove leaves the plane clean.
    let (ok, envelope, _) = run(
        home.path(),
        &[
            "gateway",
            "connector",
            "remove",
            "--ref",
            "gateway-connector/slack/main",
        ],
    );
    assert!(ok, "{envelope}");
    let (_, envelope, _) = run(home.path(), &["gateway", "connector", "list"]);
    assert_eq!(envelope["data"]["connectors"].as_array().unwrap().len(), 0);
}

#[test]
fn an_unknown_implementation_stops_the_service_at_startup_naming_it() {
    let home = TempDir::new().unwrap();
    write_connectors_file(
        home.path(),
        json!([{
            "connector_ref": "gateway-connector/pigeon/main",
            "platform": "pigeon",
            "implementation": "carrier-pigeon"
        }]),
    );
    let (ok, envelope, _) = run(home.path(), &["gateway", "serve"]);
    assert!(
        !ok,
        "an unknown implementation must stop the serve: {envelope}"
    );
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(message.contains("carrier-pigeon"), "{message}");
    assert!(
        !home.path().join("state/gateway.sock").exists(),
        "no carrier may bind for a refused startup"
    );
}

#[test]
fn a_disabled_connector_is_declared_but_not_run() {
    let home = TempDir::new().unwrap();
    write_connectors_file(
        home.path(),
        json!([{
            "connector_ref": "gateway-connector/specimen/main",
            "platform": "specimen",
            "implementation": "stdio",
            "enabled": false,
            "program": specimen_program(&[
                "--connector-ref", "gateway-connector/specimen/main",
                "--emit-inbound", "should-never-arrive",
            ])
        }]),
    );
    let mut serve = spawn_serve(home.path());
    wait_for_socket(home.path());

    // The service is up, answers, and runs no connector.
    let (ok, protocol, _) = run(home.path(), &["gateway", "protocol"]);
    assert!(ok, "{protocol}");
    let snap = snapshot(home.path());
    assert_eq!(snap["connector_health"].as_array().unwrap().len(), 0);
    assert_eq!(snap["streams"].as_array().unwrap().len(), 0);

    shutdown(home.path());
    serve.wait().unwrap();
}

#[test]
fn the_specimen_binary_itself_speaks_the_wire_protocol() {
    // Direct child conformance: hello, scripted inbound echo, receipt marker,
    // shutdown. This is the same protocol the service drives over the pump.
    use std::process::Stdio as _;

    let mut child = Command::new(specimen_bin())
        .args([
            "--connector-ref",
            "gateway-connector/specimen/direct",
            "--emit-inbound",
            "direct-probe",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let hello: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(hello["type"], "hello", "{hello}");
    assert_eq!(
        hello["hello"]["descriptor"]["connector_ref"],
        "gateway-connector/specimen/direct"
    );
    let operations: Vec<&str> = hello["hello"]["descriptor"]["capabilities"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| op.as_str().unwrap())
        .collect();
    assert!(operations.contains(&"send"), "{hello}");
    assert!(operations.contains(&"typing"), "{hello}");
    assert!(
        !operations.contains(&"edit"),
        "edit must be undeclared: {hello}"
    );
    assert!(
        !operations.contains(&"react"),
        "react must be undeclared: {hello}"
    );

    line.clear();
    stdout.read_line(&mut line).unwrap();
    let health: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(health["type"], "health", "{health}");

    line.clear();
    stdout.read_line(&mut line).unwrap();
    let inbound: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(inbound["type"], "inbound", "{inbound}");
    assert_eq!(inbound["event"]["text"], "direct-probe");

    writeln!(
        stdin,
        "{}",
        json!({
            "type": "outbound",
            "operation": {
                "operation_ref": "gateway-operation/00000000000000000007",
                "connector_ref": "gateway-connector/specimen/direct",
                "address": {"platform": "specimen", "conversation_id": "main"},
                "operation": {"kind": "send", "text": "echo of direct-probe"}
            }
        })
    )
    .unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    let receipt: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(receipt["type"], "delivery-receipt", "{receipt}");
    assert_eq!(receipt["receipt"]["state"], "delivered");
    assert!(
        receipt["receipt"]["detail"]
            .as_str()
            .unwrap()
            .contains("echo of direct-probe"),
        "the receipt proves execution with the echoed text: {receipt}"
    );
    assert_eq!(
        receipt["receipt"]["native"]["specimen_marker"],
        "executed-1"
    );

    writeln!(
        stdin,
        "{}",
        json!({"type": "shutdown", "reason": "direct conformance"})
    )
    .unwrap();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "the specimen exits cleanly on shutdown: {status}"
    );
}
