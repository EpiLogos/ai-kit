//! Controlled protocol inputs to real adapters; no live-model claim.
use aikit_adapters::interactive_connection::{
    AcpStableConnectionAdapter, InteractiveAgentConnectionAdapter,
};
use aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter;
use aikit_adapters::{AgentConnectionAdapter, SessionOpenMode, SessionOpenRequest};
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
#[test]
fn pi_resident_model_observation_never_promises_an_unimplemented_selector() {
    let adapter = PiRpcConnectionAdapter::new(r("connection/test/pi"), "/tmp".into(), vec![]);
    let controls = adapter.session_model_controls("native-test");
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
