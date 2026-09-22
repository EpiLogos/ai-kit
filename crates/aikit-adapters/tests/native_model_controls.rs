//! Controlled protocol inputs to real adapters; no live-model claim.
use aikit_adapters::interactive_connection::{
    AcpStableConnectionAdapter, InteractiveAgentConnectionAdapter,
};
use aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter;
use aikit_adapters::{
    AgentConnectionAdapter, ConnectionSignalKind, SessionOpenMode, SessionOpenRequest,
};
use aikit_core::ResourceRef;
use serde_json::{json, Value};

fn r(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}
fn opened(result: Value) -> AcpStableConnectionAdapter {
    let mut adapter = AcpStableConnectionAdapter::new(r("connection/test/model-controls"), vec![]);
    let init = adapter.initialize().unwrap();
    adapter.ingest(json!({"jsonrpc":"2.0","id":init.payload["id"],"result":{"protocolVersion":1,"agentCapabilities":{}}})).unwrap();
    let open = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Create,
            native_session_id: None,
            cwd: "/tmp".into(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(r("agent-session/test/controls")),
        })
        .unwrap();
    adapter
        .ingest(json!({"jsonrpc":"2.0","id":open.payload["id"],"result":result}))
        .unwrap();
    adapter
}
#[test]
fn legacy_model_observation_does_not_invent_a_writable_config_selector() {
    let adapter = opened(
        json!({"sessionId":"native-test","models":{"currentModelId":"test/a","availableModels":[{"modelId":"test/a","name":"A"}]}}),
    );
    assert!(
        !adapter
            .session_model_controls("native-test")
            .model_selection
    );
}
#[test]
fn only_exact_advertised_acp_session_controls_are_writable() {
    let mut adapter = opened(json!({"sessionId":"native-test","configOptions":[
        {"id":"model","category":"model","type":"select","name":"Model","currentValue":"test/a","options":[{"value":"test/a","name":"A"},{"value":"test/b","name":"B"}]},
        {"id":"reasoning_effort","type":"select","name":"Reasoning","currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}
    ]}));
    let controls = adapter.session_model_controls("native-test");
    assert!(controls.model_selection && controls.reasoning_effort_selection);
    assert!(
        !adapter
            .session_model_controls("other-native")
            .model_selection
    );
    assert_eq!(
        adapter
            .set_session_model("other-native", "test/a")
            .unwrap_err()
            .code(),
        "connection.acp.model_selection_unsupported"
    );
    assert_eq!(
        adapter
            .set_session_model("native-test", "not-advertised")
            .unwrap_err()
            .code(),
        "connection.acp.model_not_advertised"
    );
    let command = adapter.set_session_model("native-test", "test/b").unwrap();
    assert_eq!(command.operation, "session/set_config_option");
    assert_eq!(
        command.payload["params"],
        json!({"sessionId":"native-test","configId":"model","value":"test/b"})
    );
}
fn open_pi(adapter: &mut PiRpcConnectionAdapter) -> Value {
    let init = adapter.initialize().unwrap();
    let response = match init.operation.as_str() {
        "get_available_models" => json!({
            "type":"response",
            "id":init.payload["id"],
            "command":"get_available_models",
            "success":true,
            "data":{"models":[
                {"provider":"provider-a","id":"same-id","name":"Model A"},
                {"provider":"provider-b","id":"same-id","name":"Model B"}
            ]}
        }),
        "get_state" => json!({
            "type":"response",
            "id":init.payload["id"],
            "command":"get_state",
            "success":true,
            "data":{
                "sessionId":"native-test",
                "isStreaming":false,
                "isCompacting":false,
                "pendingMessageCount":0,
                "model":{"provider":"provider-a","id":"same-id","name":"Model A"}
            }
        }),
        other => panic!("unexpected Pi initialization operation {other}"),
    };
    adapter.ingest(response).unwrap();
    let open = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: Some("native-test".into()),
            cwd: "/tmp".into(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(r("agent-session/test/pi-controls")),
        })
        .unwrap();
    let signals = adapter
        .ingest(json!({
            "type":"response",
            "id":open.payload["id"],
            "command":"get_state",
            "success":true,
            "data":{
                "sessionId":"native-test",
                "isStreaming":false,
                "isCompacting":false,
                "pendingMessageCount":0,
                "model":{"provider":"provider-a","id":"same-id","name":"Model A"}
            }
        }))
        .unwrap();
    match &signals[0].kind {
        ConnectionSignalKind::SessionOpened { binding } => {
            serde_json::to_value(binding.model_observation.as_ref().unwrap()).unwrap()
        }
        other => panic!("expected Pi SessionOpened, got {other:?}"),
    }
}

#[test]
fn pi_routes_exact_provider_and_model_then_requires_confirmation() {
    let mut adapter = PiRpcConnectionAdapter::new(r("connection/test/pi"), "/tmp".into(), vec![]);
    let observation = open_pi(&mut adapter);
    assert_eq!(observation["current_model_id"], "provider-a/same-id");
    assert_eq!(
        observation["available_models"],
        json!([
            {"modelId":"provider-a/same-id","name":"Model A · provider-a"},
            {"modelId":"provider-b/same-id","name":"Model B · provider-b"}
        ])
    );
    let controls = adapter.session_model_controls("native-test");
    assert!(controls.model_selection);
    assert!(!controls.reasoning_effort_selection);
    assert!(controls.reason.is_none());
    assert_eq!(
        adapter
            .set_session_model("native-test", "same-id")
            .unwrap_err()
            .code(),
        "connection.pi_rpc.model_not_advertised"
    );
    let command = adapter
        .set_session_model("native-test", "provider-b/same-id")
        .unwrap();
    assert_eq!(command.operation, "set_model");
    assert_eq!(command.payload["provider"], "provider-b");
    assert_eq!(command.payload["modelId"], "same-id");
    let signals = adapter
        .ingest(json!({
            "type":"response",
            "id":command.payload["id"],
            "command":"set_model",
            "success":true,
            "data":{"provider":"provider-b","id":"same-id","name":"Model B"}
        }))
        .unwrap();
    match &signals[0].kind {
        ConnectionSignalKind::ModelConfigured { model_observation } => {
            assert_eq!(model_observation.current_model_id, "provider-b/same-id");
            assert_eq!(model_observation.available_models.len(), 2);
        }
        other => panic!("expected confirmed Pi ModelConfigured, got {other:?}"),
    }
}

#[test]
fn pi_rejects_a_set_model_response_for_a_different_exact_route() {
    let mut adapter =
        PiRpcConnectionAdapter::new(r("connection/test/pi-mismatch"), "/tmp".into(), vec![]);
    open_pi(&mut adapter);
    let command = adapter
        .set_session_model("native-test", "provider-b/same-id")
        .unwrap();
    let error = adapter
        .ingest(json!({
            "type":"response",
            "id":command.payload["id"],
            "command":"set_model",
            "success":true,
            "data":{"provider":"provider-a","id":"same-id","name":"Model A"}
        }))
        .unwrap_err();
    assert_eq!(
        error.code(),
        "connection.pi_rpc.model_configuration_unconfirmed"
    );
}

#[test]
fn pi_owner_pinned_model_keeps_raw_provider_id_and_stays_read_only() {
    let mut adapter =
        PiRpcConnectionAdapter::new(r("connection/test/pi-owner-pinned"), "/tmp".into(), vec![])
            .with_selected_model("provider-a", "same-id")
            .unwrap();
    let observation = open_pi(&mut adapter);
    assert_eq!(observation["current_model_id"], "same-id");
    assert_eq!(observation["available_models"][0]["modelId"], "same-id");
    let controls = adapter.session_model_controls("native-test");
    assert!(!controls.model_selection && !controls.reasoning_effort_selection);
    assert_eq!(
        adapter
            .set_session_model("native-test", "same-id")
            .unwrap_err()
            .code(),
        "connection.pi_rpc.model_selection_unsupported"
    );
    let verify = adapter.initialize().unwrap();
    assert_eq!(verify.operation, "get_state");
    let error = adapter
        .ingest(json!({
            "type":"response",
            "id":verify.payload["id"],
            "command":"get_state",
            "success":true,
            "data":{
                "sessionId":"native-test",
                "isStreaming":false,
                "isCompacting":false,
                "pendingMessageCount":0,
                "model":{"provider":"provider-b","id":"same-id","name":"Model B"}
            }
        }))
        .unwrap_err();
    assert_eq!(error.code(), "connection.pi_rpc.model_mismatch");
}

#[test]
fn pi_without_a_configured_native_model_attaches_but_does_not_advertise_a_selector() {
    let mut adapter =
        PiRpcConnectionAdapter::new(r("connection/test/pi-no-model"), "/tmp".into(), vec![]);
    let init = adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "type":"response",
            "id":init.payload["id"],
            "command":"get_available_models",
            "success":true,
            "data":{"models":[]}
        }))
        .unwrap();
    let open = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: None,
            cwd: "/tmp".into(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(r("agent-session/test/pi-no-model")),
        })
        .unwrap();
    let signals = adapter
        .ingest(json!({
            "type":"response",
            "id":open.payload["id"],
            "command":"get_state",
            "success":true,
            "data":{
                "sessionId":"native-no-model",
                "isStreaming":false,
                "isCompacting":false,
                "pendingMessageCount":0,
                "model":null
            }
        }))
        .unwrap();
    match &signals[0].kind {
        ConnectionSignalKind::SessionOpened { binding } => {
            assert!(binding.model_observation.is_none());
        }
        other => panic!("expected Pi SessionOpened, got {other:?}"),
    }
    let controls = adapter.session_model_controls("native-no-model");
    assert!(!controls.model_selection && !controls.reasoning_effort_selection);
    assert!(controls.reason.is_some());
}

#[test]
fn a_selector_without_the_required_model_category_stays_read_only() {
    let adapter = opened(
        json!({"sessionId":"native-test","configOptions":[{"id":"model","type":"select","name":"Model","currentValue":"test/a","options":[{"value":"test/a","name":"A"}]}]}),
    );
    assert!(
        !adapter
            .session_model_controls("native-test")
            .model_selection
    );
}
