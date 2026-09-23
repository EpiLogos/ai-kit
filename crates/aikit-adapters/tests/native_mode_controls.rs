//! Controlled ACP protocol inputs to the real adapters: session permission
//! modes are carried exactly as the agent advertises them. No live model.
use aikit_adapters::interactive_connection::{
    AcpStableConnectionAdapter, InteractiveAgentConnectionAdapter,
};
use aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter;
use aikit_adapters::{
    AgentConnectionAdapter, ConnectionSignalKind, NativeSessionBinding, SessionOpenMode,
    SessionOpenRequest,
};
use aikit_core::ResourceRef;
use serde_json::{json, Value};

fn r(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

/// The exact `modes` block Hermes ACP returns from session/new.
fn hermes_modes() -> Value {
    json!({"availableModes":[
        {"id":"default","name":"Default","description":"Ask before edits."},
        {"id":"accept_edits","name":"Accept Edits","description":"Auto-allow workspace and /tmp edits; still asks for sensitive paths."},
        {"id":"dont_ask","name":"Don't Ask","description":"Auto-allow file edits for this session except sensitive paths."}
    ],"currentModeId":"default"})
}

fn opened(result: Value) -> (AcpStableConnectionAdapter, NativeSessionBinding) {
    let mut adapter = AcpStableConnectionAdapter::new(r("connection/test/mode-controls"), vec![]);
    let init = adapter.initialize().unwrap();
    adapter.ingest(json!({"jsonrpc":"2.0","id":init.payload["id"],"result":{"protocolVersion":1,"agentCapabilities":{}}})).unwrap();
    let open = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Create,
            native_session_id: None,
            cwd: "/tmp".into(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(r("agent-session/test/modes")),
        })
        .unwrap();
    let signals = adapter
        .ingest(json!({"jsonrpc":"2.0","id":open.payload["id"],"result":result}))
        .unwrap();
    let binding = signals
        .into_iter()
        .find_map(|signal| match signal.kind {
            ConnectionSignalKind::SessionOpened { binding } => Some(binding),
            _ => None,
        })
        .expect("session opened");
    (adapter, binding)
}

#[test]
fn session_new_modes_are_carried_exactly_on_the_binding() {
    let (adapter, binding) = opened(json!({"sessionId":"native-test","modes":hermes_modes()}));
    let observation = binding.mode_observation.expect("modes observed");
    assert_eq!(observation.current_mode_id, "default");
    let ids: Vec<_> = observation
        .available_modes
        .iter()
        .map(|mode| mode.id.as_str())
        .collect();
    assert_eq!(ids, ["default", "accept_edits", "dont_ask"]);
    assert_eq!(
        observation.available_modes[0].description.as_deref(),
        Some("Ask before edits.")
    );
    // The option fields serialize exactly as the ACP wire names them.
    assert_eq!(
        serde_json::to_value(&observation.available_modes[1]).unwrap(),
        json!({"id":"accept_edits","name":"Accept Edits","description":"Auto-allow workspace and /tmp edits; still asks for sensitive paths."})
    );
    let controls = adapter.session_mode_controls("native-test");
    assert!(controls.mode_selection && controls.reason.is_none());
    assert!(!adapter.session_mode_controls("other-native").mode_selection);
}

#[test]
fn an_agent_without_modes_offers_no_mode_control() {
    let (mut adapter, binding) = opened(json!({"sessionId":"native-test"}));
    assert!(binding.mode_observation.is_none());
    let controls = adapter.session_mode_controls("native-test");
    assert!(!controls.mode_selection);
    assert!(controls.reason.is_some());
    assert_eq!(
        adapter
            .set_session_mode("native-test", "default")
            .unwrap_err()
            .code(),
        "connection.acp.mode_selection_unsupported"
    );
}

#[test]
fn a_malformed_modes_block_invents_nothing() {
    // currentModeId outside the advertised set.
    let (adapter, binding) = opened(json!({"sessionId":"native-test","modes":{
        "availableModes":[{"id":"default","name":"Default"}],"currentModeId":"plan"}}));
    assert!(binding.mode_observation.is_none());
    assert!(!adapter.session_mode_controls("native-test").mode_selection);
}

#[test]
fn set_mode_sends_the_acp_request_and_confirms_on_the_provider_reply() {
    let (mut adapter, _) = opened(json!({"sessionId":"native-test","modes":hermes_modes()}));
    assert_eq!(
        adapter
            .set_session_mode("native-test", "plan")
            .unwrap_err()
            .code(),
        "connection.acp.mode_not_advertised"
    );
    let command = adapter
        .set_session_mode("native-test", "accept_edits")
        .unwrap();
    assert_eq!(command.operation, "session/set_mode");
    assert_eq!(command.payload["method"], "session/set_mode");
    assert_eq!(
        command.payload["params"],
        json!({"sessionId":"native-test","modeId":"accept_edits"})
    );
    // ACP answers session/set_mode with an empty result.
    let signals = adapter
        .ingest(json!({"jsonrpc":"2.0","id":command.payload["id"],"result":{}}))
        .unwrap();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].native_session_id.as_deref(), Some("native-test"));
    match &signals[0].kind {
        ConnectionSignalKind::ModeConfigured { mode_observation } => {
            assert_eq!(mode_observation.current_mode_id, "accept_edits");
            assert_eq!(mode_observation.available_modes.len(), 3);
        }
        other => panic!("expected mode-configured, got {other:?}"),
    }
    // The kebab wire name of the new signal.
    assert_eq!(
        serde_json::to_value(&signals[0].kind).unwrap()["kind"],
        "mode-configured"
    );
}

#[test]
fn a_refused_set_mode_is_degraded_not_confirmed() {
    let (mut adapter, _) = opened(json!({"sessionId":"native-test","modes":hermes_modes()}));
    let command = adapter.set_session_mode("native-test", "dont_ask").unwrap();
    let signals = adapter
        .ingest(json!({"jsonrpc":"2.0","id":command.payload["id"],"error":{"code":-32602,"message":"no"}}))
        .unwrap();
    assert!(matches!(
        signals[0].kind,
        ConnectionSignalKind::Degraded { .. }
    ));
}

#[test]
fn current_mode_update_from_the_agent_moves_the_current_mode() {
    let (mut adapter, _) = opened(json!({"sessionId":"native-test","modes":hermes_modes()}));
    for spelling in ["currentModeId", "modeId"] {
        let signals = adapter
            .ingest(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"native-test",
                "update":{"sessionUpdate":"current_mode_update", spelling:"dont_ask"}}}))
            .unwrap();
        match &signals[0].kind {
            ConnectionSignalKind::ModeConfigured { mode_observation } => {
                assert_eq!(mode_observation.current_mode_id, "dont_ask")
            }
            other => panic!("expected mode-configured for {spelling}, got {other:?}"),
        }
    }
    // A mode the session never advertised stays a plain status.
    let signals = adapter
        .ingest(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"native-test",
            "update":{"sessionUpdate":"current_mode_update","currentModeId":"plan"}}}))
        .unwrap();
    assert!(matches!(
        signals[0].kind,
        ConnectionSignalKind::Status { .. }
    ));
}

#[test]
fn pi_publishes_no_permission_modes() {
    let mut adapter = PiRpcConnectionAdapter::new(r("connection/test/pi"), "/tmp".into(), vec![]);
    let controls = adapter.session_mode_controls("native-test");
    assert!(!controls.mode_selection);
    assert!(controls.reason.unwrap().contains("Pi RPC"));
    assert_eq!(
        adapter
            .set_session_mode("native-test", "default")
            .unwrap_err()
            .code(),
        "connection.pi_rpc.mode_selection_unsupported"
    );
}

#[test]
fn old_bindings_without_modes_still_parse() {
    let binding: NativeSessionBinding =
        serde_json::from_value(json!({"native_session_id":"n","opened_as":"create"})).unwrap();
    assert!(binding.mode_observation.is_none());
}
