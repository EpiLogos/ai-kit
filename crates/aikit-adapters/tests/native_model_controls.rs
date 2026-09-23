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

#[test]
fn native_selector_deduplicates_routes_without_merging_distinct_models() {
    let mut adapter = opened(json!({"sessionId":"native-test","configOptions":[
        {"id":"model","category":"model","type":"select","currentValue":"provider/a","options":[
            {"value":"provider/a","name":"Model A"}, {"value":"provider/a","name":"Model A"},
            {"value":"provider/b","name":"Model B"}
        ]}
    ]}));
    let request = adapter
        .set_session_model("native-test", "provider/b")
        .unwrap();
    let signals = adapter.ingest(json!({"jsonrpc":"2.0","id":request.payload["id"],"result":{"configOptions":[
        {"id":"model","category":"model","type":"select","currentValue":"provider/b","options":[
            {"value":"provider/a","name":"Model A"}, {"value":"provider/a","name":"Model A"},
            {"value":"provider/b","name":"Model B"}
        ]}
    ]}})).unwrap();
    let aikit_adapters::agent_connection::ConnectionSignalKind::ModelConfigured {
        model_observation,
    } = &signals[0].kind
    else {
        panic!("native configuration receipt required");
    };
    assert_eq!(model_observation.available_models.len(), 2);
    assert_eq!(model_observation.available_models[0].name, "Model A");
    assert_eq!(model_observation.available_models[1].name, "Model B");
    assert_eq!(model_observation.current_model_id, "provider/b");
}

#[test]
fn pi_advertises_its_observed_model_name_without_an_aikit_policy() {
    let mut adapter = PiRpcConnectionAdapter::new(r("connection/test/pi"), "/tmp".into(), vec![]);
    let state = json!({"sessionId":"native-pi","isStreaming":false,"isCompacting":false,"pendingMessageCount":0,
        "model":{"provider":"anthropic","id":"claude-sonnet-4-5","name":"Claude Sonnet 4.5"}});
    let initialize = adapter.initialize().unwrap();
    adapter
        .ingest(
            json!({"type":"response","id":initialize.payload["id"],"success":true,"data":state}),
        )
        .unwrap();
    let attach = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: None,
            cwd: "/tmp".into(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(r("agent-session/test/pi-model")),
        })
        .unwrap();
    let signals = adapter
        .ingest(json!({"type":"response","id":attach.payload["id"],"success":true,"data":state}))
        .unwrap();
    let aikit_adapters::agent_connection::ConnectionSignalKind::SessionOpened { binding } =
        &signals[0].kind
    else {
        panic!("native binding required");
    };
    let models = &binding.model_observation.as_ref().unwrap().available_models;
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name, "Claude Sonnet 4.5");
    assert_eq!(models[0].model_id, "claude-sonnet-4-5");
}
