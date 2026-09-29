//! `aikit gateway hoist` and `aikit gateway --at`, through the real binary and
//! the real library: a posture packed on one AIKit home plans honestly, stages
//! through the ordered channel steps, and materialises on another home with
//! its journals, bindings, connectors, coexistence and agent providers intact
//! — token locations moving and token values never.

#[path = "support/serve_guard.rs"]
mod serve_guard;
use serve_guard::ServeGuard;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use aikit_adapters::{
    execute_against_state_file, AgencyGateway, CommuniqueDraft, CommuniqueState,
    ConnectorCapabilities, ConnectorDescriptor, ConversationAddress, GatewayBinding,
    GatewayCommand, GatewayIngressDecision, GatewayIngressPolicy, InboundEvent, InboundEventKind,
    SenderAttribution, SenderIdentity, SenderKind, GATEWAY_CONNECTOR_SDK_VERSION,
};
use aikit_core::resource::ResourceRef;
use aikit_store::home::AikitHome;
use serde_json::{json, Value};
use tempfile::TempDir;

fn bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

/// One `aikit` invocation against a home, expecting the JSON envelope.
fn run(home: &Path, args: &[&str]) -> (bool, Value, String) {
    let output = Command::new(bin())
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env_remove("AIKIT_GATEWAY_AT")
        .env_remove("AIKIT_GATEWAY_REF")
        .env_remove("AIKIT_WORKCELL_REF")
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

/// An owner-only token file; its VALUE is the one this suite must never find
/// in any hoist output.
const TOKEN_VALUE: &str = "hoist-fixture-token-value";

fn write_token(home: &Path, name: &str) -> PathBuf {
    let path = home.join(name);
    std::fs::write(&path, format!("{TOKEN_VALUE}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

/// Declare a telegram connector whose token lives at `token` (locations
/// only), optionally backed by a named agent provider.
fn connector_add(home: &Path, token: &Path, backing: Option<&str>) {
    let token_location = format!("file:{}", token.display());
    let mut args = vec![
        "gateway",
        "connector",
        "add",
        "--platform",
        "telegram",
        "--ref",
        "gateway-connector/telegram/main",
        "--token-location",
        token_location.as_str(),
    ];
    if let Some(backing) = backing {
        args.extend(["--agent-backing", backing]);
    }
    let (ok, envelope, _) = run(home, &args);
    assert!(ok, "connector add should succeed: {envelope}");
}

/// The coexistence document of a home, so the posture carries it.
fn coexistence_add(home: &Path) {
    let (ok, envelope, _) = run(home, &["gateway", "coexistence", "--policy", "coexist"]);
    assert!(ok, "coexistence policy should be settable: {envelope}");
}

/// An agent provider the connector's --agent-backing names, so the posture
/// carries the resolved registry entry. The registry is
/// `state/encounter-providers/<id>.json`, read the same way serve reads it.
fn provider_add(home: &Path) {
    let dir = home.join("state/encounter-providers");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("pi.json"),
        serde_json::to_vec_pretty(&json!({
            "protocol": "acp",
            "id": "pi",
            "label": "pi fixture",
            "argv": ["/bin/echo"],
        }))
        .unwrap(),
    )
    .unwrap();
}

/// A real semantic state for a source home: one connector descriptor, one
/// binding, a stream journal carrying events, and one Communique — built
/// through the kernel's own doors and persisted exactly as the service does.
fn seed_state(home: &Path) {
    let r = |raw: &str| ResourceRef::parse(raw).unwrap();
    let mut gateway = AgencyGateway::new(r("agency-gateway/local"));
    gateway
        .register_connector(ConnectorDescriptor {
            version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
            connector_ref: r("gateway-connector/telegram/main"),
            platform: "telegram".into(),
            implementation: "telegram".into(),
            capabilities: ConnectorCapabilities::default(),
            configuration_ref: None,
            provenance: vec!["gateway_hoist fixture".into()],
        })
        .unwrap();
    gateway
        .bind(GatewayBinding {
            binding_ref: r("gateway-binding/telegram/chat-42"),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(),
                scope_id: None,
                conversation_id: "chat-42".into(),
                thread_id: None,
            },
            agent_session_ref: r("agent-session/fixture"),
            agency_ref: r("agency/fixture"),
            actuation_ref: r("actuation/fixture"),
            actuation_stream_ref: r("actuation-stream/fixture"),
            agent_ref: None,
            harness_ref: None,
            surface_ref: None,
            forked_from: None,
            context_revision: 1,
            ingress: GatewayIngressPolicy {
                default: GatewayIngressDecision::Allow,
                sender_overrides: Default::default(),
            },
            provenance: vec![],
        })
        .unwrap();
    gateway
        .ingest(InboundEvent {
            event_ref: r("gateway-event/fixture-1"),
            connector_ref: r("gateway-connector/telegram/main"),
            address: ConversationAddress {
                platform: "telegram".into(),
                scope_id: None,
                conversation_id: "chat-42".into(),
                thread_id: None,
            },
            sender: SenderIdentity {
                native_sender_id: "sender-7".into(),
                kind: SenderKind::Human,
                display_name: Some("Fixture Sender".into()),
                metadata: Default::default(),
            },
            kind: InboundEventKind::Message,
            custom_kind: None,
            native_event_id: None,
            native_message_id: Some("native-1".into()),
            reply_to_native_message_id: None,
            text: Some("hello from the journal".into()),
            media: vec![],
            observed_at: None,
            native: Default::default(),
            provenance: vec![],
        })
        .unwrap();
    let state_file = home.join("state/gateway.json");
    std::fs::create_dir_all(state_file.parent().unwrap()).unwrap();
    std::fs::write(
        &state_file,
        serde_json::to_vec_pretty(&gateway.snapshot()).unwrap(),
    )
    .unwrap();
    // One Communique in the journal, appended through the offline seam the
    // contact verbs use, so the round-trip carries contact semantics too.
    execute_against_state_file(
        AgencyGateway::new(r("agency-gateway/local")),
        &state_file,
        GatewayCommand::SendCommunique {
            draft: Box::new(CommuniqueDraft {
                communique_ref: format!("{}fixture", aikit_adapters::COMMUNIQUE_REF_PREFIX),
                from_position_ref: Some("central:position:project:O-I:factory-guardian".into()),
                from_generation_ref: None,
                attribution: SenderAttribution::Claimed,
                attribution_basis: "fixture".into(),
                to_position_ref: "central:position:project:O-I:cradle-steward".into(),
                to_workcell_ref: None,
                to_instance: None,
                instance_hold: None,
                body: "carry me to the other Workcell".into(),
                sent_at_unix_ms: 1,
                state: CommuniqueState::Held,
                state_basis: "vacant; held for its next occupant".into(),
                reply_to: None,
                forward_to_workcell_ref: None,
                routing: None,
            }),
        },
        Duration::from_secs(2),
    )
    .unwrap();
}

fn hoist_args(to: &str) -> aikit_cli::gateway_hoist::HoistArgs {
    aikit_cli::gateway_hoist::HoistArgs {
        to: Some(to.to_owned()),
        apply: true,
        receive: false,
        force: false,
        yes: false,
        ssh: Some("fixture".into()),
        include_tokens: false,
        gateway_ref: None,
    }
}

/// The fixture channel: it runs no host, it records the ordered steps the ssh
/// channel would run, and answers each with fixed material.
/// One recorded channel call: the step name, its argv, and its stdin.
type RecordedCall = (&'static str, Vec<String>, Option<Vec<u8>>);

struct FixtureChannel {
    calls: std::sync::Mutex<Vec<RecordedCall>>,
}

impl FixtureChannel {
    fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn steps(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(step, _, _)| *step)
            .collect()
    }
}

impl aikit_cli::gateway_hoist::TargetChannel for FixtureChannel {
    fn run(
        &self,
        step: &'static str,
        argv: &[String],
        stdin: Option<&[u8]>,
    ) -> aikit_core::Result<Vec<u8>> {
        self.calls
            .lock()
            .unwrap()
            .push((step, argv.to_vec(), stdin.map(|bytes| bytes.to_vec())));
        match step {
            "resolve-target-home" => Ok(b"/home/frank\n".to_vec()),
            _ => Ok(Vec::new()),
        }
    }

    fn describe(&self) -> String {
        "fixture".into()
    }
}

// ---------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------

#[test]
fn plan_names_every_posture_item_and_never_a_token_value() {
    let source = TempDir::new().unwrap();
    let token = write_token(source.path(), "telegram.token");
    connector_add(source.path(), &token, Some("pi"));
    coexistence_add(source.path());
    provider_add(source.path());
    seed_state(source.path());

    let (ok, envelope, _) = run(
        source.path(),
        &["gateway", "hoist", "--to", "workcell:omarchy"],
    );
    assert!(ok, "plan should succeed: {envelope}");
    let plan = &envelope["data"];
    assert_eq!(plan["verb"], "plan");
    assert_eq!(plan["to"]["workcell_ref"], "workcell:omarchy");
    assert_eq!(plan["to"]["gateway_ref"], "agency-gateway/omarchy");

    // Every posture item is named.
    let connectors = plan["moves"]["connectors"].as_array().unwrap();
    assert_eq!(connectors.len(), 1);
    assert_eq!(
        connectors[0]["connector_ref"],
        "gateway-connector/telegram/main"
    );
    assert_eq!(connectors[0]["platform"], "telegram");
    assert!(
        connectors[0]["token_location"]
            .as_str()
            .unwrap()
            .ends_with("telegram.token"),
        "the token is named by location only: {connectors:?}"
    );
    assert_eq!(plan["moves"]["bindings"], 1);
    assert_eq!(plan["moves"]["stream_journals"], 1);
    assert_eq!(plan["moves"]["journal_events"], 1);
    assert_eq!(plan["moves"]["communiques"], 1);
    assert_eq!(plan["moves"]["coexistence_policy"], "coexist");
    let providers = plan["moves"]["agent_providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["id"], "pi");
    assert!(!providers[0]["argv"].as_array().unwrap().is_empty());

    // What the target must re-resolve, named as a location.
    let re_resolved = plan["re_resolved_on_target"].as_array().unwrap();
    assert_eq!(re_resolved.len(), 1);
    assert!(
        re_resolved[0]["expectation"]
            .as_str()
            .unwrap()
            .contains("workcell:omarchy"),
        "the expectation names the target: {re_resolved:?}"
    );

    // Identity law: material keeps its refs, the gateway becomes the target's.
    let identity = plan["identity"]["becomes"].as_str().unwrap();
    assert!(identity.contains("agency-gateway/omarchy"));
    assert!(identity.contains("workcell:omarchy"));
    let kept = plan["identity"]["kept"].as_str().unwrap();
    assert!(kept.contains("1 bindings"));
    assert!(kept.contains("1 Communiques"));

    // The token VALUE appears nowhere in the printed plan.
    let printed = format!("{envelope}");
    assert!(
        !printed.contains(TOKEN_VALUE),
        "no token value may ever move through a plan: {printed}"
    );
}

#[test]
fn plan_refuses_a_connector_backing_no_provider_names_the_fact() {
    let source = TempDir::new().unwrap();
    let token = write_token(source.path(), "telegram.token");
    let (ok, _, _) = run(
        source.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "telegram",
            "--ref",
            "gateway-connector/telegram/main",
            "--token-location",
            &format!("file:{}", token.display()),
            "--agent-backing",
            "pi",
        ],
    );
    assert!(ok);
    let (ok, envelope, _) = run(
        source.path(),
        &["gateway", "hoist", "--to", "workcell:omarchy"],
    );
    assert!(!ok, "a backing no provider answers must refuse: {envelope}");
    assert_eq!(
        envelope["error"]["code"],
        "gateway.hoist_backing_unresolved"
    );
}

// ---------------------------------------------------------------------------
// apply through the fixture channel — the ordered steps.
// ---------------------------------------------------------------------------

#[test]
fn apply_over_a_channel_runs_the_ordered_steps_and_stages_the_bundle() {
    let source = TempDir::new().unwrap();
    let token = write_token(source.path(), "telegram.token");
    connector_add(source.path(), &token, None);
    seed_state(source.path());
    let (ok, envelope, _) = run(
        source.path(),
        &[
            "gateway",
            "remote",
            "add",
            "--workcell",
            "workcell:omarchy",
            "--ws",
            "100.92.62.101:7800",
            "--token-location",
            &format!("file:{}", token.display()),
        ],
    );
    assert!(ok, "the remote must be declared first: {envelope}");

    let home = AikitHome::at(source.path());
    let channel = FixtureChannel::new();
    let out = aikit_cli::gateway_hoist::apply_over_channel(
        &home,
        &hoist_args("workcell:omarchy"),
        &channel,
    )
    .unwrap();

    // The ordered steps: home first, then the bundle; nothing after.
    assert_eq!(
        channel.steps(),
        vec!["resolve-target-home", "stage-bundle"],
        "steps run in order and stop there without --yes work: {:?}",
        channel.steps()
    );
    let calls = channel.calls.lock().unwrap();
    assert_eq!(calls[0].1, vec!["printenv".to_owned(), "HOME".to_owned()]);
    let (_, stage_argv, stage_stdin) = &calls[1];
    let stage_stdin = stage_stdin.as_ref().unwrap();
    let stage_shell = stage_argv[2].clone();
    assert!(
        stage_shell.contains("/home/frank/.aikit/state/gateway-hoist-pending.json"),
        "the bundle lands at the target home's staged name: {stage_shell}"
    );
    assert!(
        stage_shell.starts_with("umask 077"),
        "staged material is owner-only from birth: {stage_shell}"
    );
    let bundle: Value = serde_json::from_slice(stage_stdin).unwrap();
    assert_eq!(bundle["schema"], "aikit.gateway-hoist/v1");
    assert_eq!(bundle["target_workcell_ref"], "workcell:omarchy");
    assert_eq!(bundle["target_gateway_ref"], "agency-gateway/omarchy");
    assert_eq!(
        bundle["gateway_state"]["gateway_ref"], "agency-gateway/omarchy",
        "the packed state already carries the target identity"
    );
    let bundle_text = String::from_utf8(stage_stdin.clone()).unwrap();
    assert!(
        !bundle_text.contains(TOKEN_VALUE),
        "no token value may ever move through an apply"
    );
    drop(calls);

    // With --yes the same channel runs the receive and the service install
    // after staging, in that order.
    let yes_channel = FixtureChannel::new();
    let mut yes_args = hoist_args("workcell:omarchy");
    yes_args.yes = true;
    let yes_out =
        aikit_cli::gateway_hoist::apply_over_channel(&home, &yes_args, &yes_channel).unwrap();
    assert_eq!(
        yes_channel.steps(),
        vec![
            "resolve-target-home",
            "stage-bundle",
            "receive-on-target",
            "install-service-on-target",
        ],
        "--yes executes the remote steps in order: {:?}",
        yes_channel.steps()
    );
    assert_eq!(yes_out["flips"]["receive_on_target"]["executed"], true);
    assert_eq!(
        yes_out["flips"]["install_service_on_target"]["executed"],
        true
    );
    assert_eq!(
        yes_out["flips"]["remote_add_swap"]["executed"], false,
        "the declaration swap stays the operator's act on each machine"
    );

    // Without --yes the flips are printed, not executed.
    assert_eq!(out["flips"]["receive_on_target"]["executed"], false);
    assert!(
        out["flips"]["install_service_on_target"]["command"]
            .as_str()
            .unwrap()
            .contains("--ws 100.92.62.101:7800"),
        "the install command names the declared endpoint: {out}"
    );
    assert!(
        out["flips"]["remote_add_swap"]["command"]
            .as_str()
            .unwrap()
            .starts_with("aikit gateway remote add --workcell workcell:omarchy"),
        "the swap command is printed: {out}"
    );
    assert_eq!(
        out["target"]["bundle"],
        "/home/frank/.aikit/state/gateway-hoist-pending.json"
    );
}

#[test]
fn apply_refuses_an_undeclared_target_and_a_missing_token_location() {
    let source = TempDir::new().unwrap();
    let token = write_token(source.path(), "telegram.token");
    connector_add(source.path(), &token, None);
    let home = AikitHome::at(source.path());

    // No remote declared: there is no fabric fact to hoist against.
    let error = aikit_cli::gateway_hoist::apply_over_channel(
        &home,
        &hoist_args("workcell:omarchy"),
        &FixtureChannel::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "gateway.remote_undeclared", "{error}");

    // Declared, and --include-tokens names a token file that has vanished:
    // the refusal comes before any step runs.
    let (ok, envelope, _) = run(
        source.path(),
        &[
            "gateway",
            "remote",
            "add",
            "--workcell",
            "workcell:omarchy",
            "--ws",
            "100.92.62.101:7800",
            "--token-location",
            &format!("file:{}", token.display()),
        ],
    );
    assert!(ok, "{envelope}");
    std::fs::remove_file(&token).unwrap();
    let channel = FixtureChannel::new();
    let mut args = hoist_args("workcell:omarchy");
    args.yes = true;
    args.include_tokens = true;
    let error = aikit_cli::gateway_hoist::apply_over_channel(&home, &args, &channel).unwrap_err();
    assert_eq!(error.code(), "gateway.hoist_token_missing", "{error}");
    assert!(
        error.to_string().contains(&token.display().to_string()),
        "the refusal names the exact path: {error}"
    );
    assert!(
        channel.steps().is_empty(),
        "a missing token location stops the apply before anything moves"
    );
}

// ---------------------------------------------------------------------------
// pack → plan → receive, through the real binary on two homes.
// ---------------------------------------------------------------------------

#[test]
fn a_posture_round_trips_between_two_homes_with_journals_bindings_and_coexistence() {
    let source = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let token = write_token(source.path(), "telegram.token");
    connector_add(source.path(), &token, Some("pi"));
    coexistence_add(source.path());
    provider_add(source.path());
    seed_state(source.path());
    let (ok, envelope, _) = run(
        source.path(),
        &[
            "gateway",
            "remote",
            "add",
            "--workcell",
            "workcell:omarchy",
            "--ws",
            "100.92.62.101:7800",
            "--token-location",
            &format!("file:{}", token.display()),
        ],
    );
    assert!(ok, "{envelope}");

    // Apply stages locally (no ssh given): the bundle waits beside the exact
    // operator commands.
    let (ok, envelope, _) = run(
        source.path(),
        &["gateway", "hoist", "--to", "workcell:omarchy", "--apply"],
    );
    assert!(ok, "local apply should stage: {envelope}");
    let staged = envelope["data"]["target"]["bundle"].as_str().unwrap();
    let staged = PathBuf::from(staged);
    assert!(staged.exists(), "the bundle is staged: {staged:?}");
    assert!(
        !std::fs::read_to_string(&staged)
            .unwrap()
            .contains(TOKEN_VALUE),
        "the bundle carries locations, never token values"
    );

    // The operator carries it over: the bundle lands at the target's staged
    // name in the target home.
    let target_staged = target.path().join("state/gateway-hoist-pending.json");
    std::fs::create_dir_all(target_staged.parent().unwrap()).unwrap();
    std::fs::copy(&staged, &target_staged).unwrap();

    // Receive refuses a home with existing posture without --force …
    let (ok, envelope, _) = run(
        target.path(),
        &[
            "gateway",
            "connector",
            "add",
            "--platform",
            "slack",
            "--ref",
            "gateway-connector/slack/existing",
            "--token-location",
            &format!(
                "file:{}",
                write_token(target.path(), "slack.token").display()
            ),
        ],
    );
    assert!(ok, "{envelope}");
    let (ok, envelope, _) = run(target.path(), &["gateway", "hoist", "--receive"]);
    assert!(!ok, "receiving over posture must refuse: {envelope}");
    assert_eq!(envelope["error"]["code"], "gateway.hoist_receive_clobber");

    // … and lands with --force, keeping every semantic ref.
    let (ok, envelope, _) = run(target.path(), &["gateway", "hoist", "--receive", "--force"]);
    assert!(ok, "receive with --force should land: {envelope}");
    let data = &envelope["data"];
    assert_eq!(data["target"]["gateway_ref"], "agency-gateway/omarchy");
    assert_eq!(data["target"]["workcell_ref"], "workcell:omarchy");
    let landed = data["landed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap().to_owned())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(landed.contains("1 bindings"), "the binding moved: {landed}");
    assert!(
        landed.contains("1 stream journals"),
        "the journal moved: {landed}"
    );
    assert!(
        landed.contains("1 Communiques"),
        "the Communique moved: {landed}"
    );
    assert!(
        landed.contains("1 connector declarations"),
        "the packed connectors moved: {landed}"
    );
    assert!(
        landed.contains("agent provider pi"),
        "the provider moved: {landed}"
    );

    // The target's material is the source's semantics under the target's
    // identity, read back from the state the receive wrote.
    let state: Value =
        serde_json::from_slice(&std::fs::read(target.path().join("state/gateway.json")).unwrap())
            .unwrap();
    assert_eq!(state["gateway_ref"], "agency-gateway/omarchy");
    let bindings = state["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0]["binding_ref"],
        "gateway-binding/telegram/chat-42"
    );
    let streams = state["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0]["stream_ref"], "actuation-stream/fixture");
    assert_eq!(
        streams[0]["next_sequence"], 2,
        "the journal sequence arrived exactly"
    );
    let communiques = state["communiques"].as_array().unwrap();
    assert_eq!(communiques.len(), 1);
    assert_eq!(
        communiques[0]["communique_ref"],
        format!("{}fixture", aikit_adapters::COMMUNIQUE_REF_PREFIX)
    );
    // The receive replaced the posture it was forced over: the target's
    // connectors are the packed ones.
    let connectors: Value = serde_json::from_slice(
        &std::fs::read(target.path().join("state/gateway-connectors.json")).unwrap(),
    )
    .unwrap();
    let rows = connectors["connectors"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["connector_ref"], "gateway-connector/telegram/main");
    assert!(target
        .path()
        .join("state/gateway-coexistence.json")
        .exists());
    assert!(target
        .path()
        .join("state/encounter-providers/pi.json")
        .exists());
    assert!(
        !target_staged.exists(),
        "the staged bundle is consumed by the receive that unpacked it"
    );

    // Receiving with nothing staged refuses with the exact remedy.
    let (ok, envelope, _) = run(target.path(), &["gateway", "hoist", "--receive", "--force"]);
    assert!(!ok, "{envelope}");
    assert_eq!(envelope["error"]["code"], "gateway.hoist_receive_no_bundle");
}

// ---------------------------------------------------------------------------
// --at routing.
// ---------------------------------------------------------------------------

#[test]
fn at_routes_a_declared_remote_and_discloses_which_gateway_answered() {
    let source = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();

    // The remote gateway: its own token, its own ref, its WebSocket carrier.
    let target_token = write_token(target.path(), "gateway.token");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut serve = ServeGuard::new(
        Command::new(bin())
            .args([
                "gateway",
                "serve",
                "--ws",
                &format!("127.0.0.1:{port}"),
                "--ws-token-location",
                &format!("file:{}", target_token.display()),
            ])
            .env("AIKIT_HOME", target.path())
            .env("HOME", target.path())
            .env("AIKIT_GATEWAY_REF", "agency-gateway/omarchy")
            .current_dir(target.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the remote gateway should spawn"),
    );

    // The source declares that endpoint, holding its copy of the token.
    let source_copy = write_token(source.path(), "omarchy-gateway.token");
    let (ok, envelope, _) = run(
        source.path(),
        &[
            "gateway",
            "remote",
            "add",
            "--workcell",
            "workcell:omarchy",
            "--ws",
            &format!("127.0.0.1:{port}"),
            "--token-location",
            &format!("file:{}", source_copy.display()),
        ],
    );
    assert!(ok, "{envelope}");

    // Routing waits for the carrier to accept.
    let mut status = None;
    for _ in 0..100 {
        let (ok, envelope, _) = run(
            source.path(),
            &["gateway", "--at", "workcell:omarchy", "status"],
        );
        if ok {
            status = Some(envelope);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let envelope = status.expect("the --at status should reach the remote gateway");
    assert_eq!(
        envelope["data"]["status"]["gateway_ref"], "agency-gateway/omarchy",
        "the remote gateway answers, named as itself: {envelope}"
    );
    let warnings = envelope["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("workcell:omarchy")),
        "the envelope discloses the routing: {warnings:?}"
    );

    // An undeclared Workcell is a refusal, not a silent fallback.
    let (ok, envelope, _) = run(
        source.path(),
        &["gateway", "--at", "workcell:elsewhere", "status"],
    );
    assert!(!ok, "{envelope}");
    assert_eq!(envelope["error"]["code"], "gateway.remote_undeclared");

    // A verb with no carrier refuses --at instead of silently ignoring it.
    let (ok, envelope, _) = run(
        source.path(),
        &["gateway", "--at", "workcell:omarchy", "connector", "list"],
    );
    assert!(!ok, "{envelope}");
    assert_eq!(envelope["error"]["code"], "cli.usage");

    let _ = serve.kill();
    let _ = serve.wait();
}
