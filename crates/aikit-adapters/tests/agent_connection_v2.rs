use aikit_adapters::{
    AcpV1ConnectionAdapter, AgentConnectionAdapter, ClassicProcessConnectionAdapter,
    ConnectionProtocolFamily, ConnectionSignalKind, SessionOpenMode, SessionOpenRequest,
    ACP_STABLE_PROTOCOL_VERSION, DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};
use aikit_core::resource::ResourceRef;
use serde_json::json;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn create_request() -> SessionOpenRequest {
    SessionOpenRequest {
        mode: SessionOpenMode::Create,
        native_session_id: None,
        cwd: "/workspace/project".into(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        agent_session: None,
    }
}

#[test]
fn official_stable_acp_v1_is_negotiated_by_protocol_version_and_capabilities() {
    let mut adapter = AcpV1ConnectionAdapter::new(
        r("connection/acp/test"),
        vec!["agentclientprotocol/agent-client-protocol:stable-v1".into()],
    );
    let init = adapter.initialize().unwrap();
    assert_eq!(init.operation, "initialize");
    assert_eq!(
        init.payload["params"]["protocolVersion"],
        ACP_STABLE_PROTOCOL_VERSION
    );

    let signals = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": true,
                    "sessionCapabilities": {
                        "resume": {},
                        "additionalDirectories": {}
                    }
                }
            }
        }))
        .unwrap();
    assert_eq!(signals.len(), 1);
    let capabilities = adapter.negotiated_capabilities();
    assert!(capabilities.supports(SessionOpenMode::Create));
    assert!(capabilities.supports(SessionOpenMode::Load));
    assert!(capabilities.supports(SessionOpenMode::Resume));
    assert!(!capabilities.supports(SessionOpenMode::Attach));
    assert!(capabilities.additional_directories);
}

#[test]
fn acp_native_session_id_never_becomes_agent_session_identity_without_explicit_binding() {
    let mut adapter = AcpV1ConnectionAdapter::new(
        r("connection/acp/deepseek"),
        vec![format!(
            "deepseek-ai/deepseek-harness@{DEEPSEEK_HARNESS_UPSTREAM_REVISION}"
        )],
    );
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {
                    "promptCapabilities": {
                        "image": false,
                        "audio": false,
                        "embeddedContext": false
                    }
                }
            }
        }))
        .unwrap();

    let command = adapter.open_session(create_request()).unwrap();
    assert_eq!(command.operation, "session/new");
    let opened = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "sessionId": "dsh-native-session-17" }
        }))
        .unwrap();
    let ConnectionSignalKind::SessionOpened { binding } = &opened[0].kind else {
        panic!("expected session binding");
    };
    assert_eq!(binding.native_session_id, "dsh-native-session-17");
    assert_eq!(binding.agent_session, None);

    let error = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Resume,
            native_session_id: Some("dsh-native-session-17".into()),
            ..create_request()
        })
        .unwrap_err();
    assert_eq!(error.code(), "connection.session_operation_unsupported");
}

#[test]
fn explicit_agent_session_binding_survives_acp_load_without_rewriting_native_identity() {
    let mut adapter = AcpV1ConnectionAdapter::new(r("connection/acp/load"), Vec::new());
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": { "loadSession": true }
            }
        }))
        .unwrap();

    let command = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Load,
            native_session_id: Some("native-abc".into()),
            agent_session: Some(r("agent-session/aikit-42")),
            ..create_request()
        })
        .unwrap();
    assert_eq!(command.operation, "session/load");
    let opened = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "sessionId": "native-abc" }
        }))
        .unwrap();
    let ConnectionSignalKind::SessionOpened { binding } = &opened[0].kind else {
        panic!("expected session binding");
    };
    assert_eq!(
        binding.agent_session.as_ref(),
        Some(&r("agent-session/aikit-42"))
    );
    assert_eq!(binding.native_session_id, "native-abc");
}

#[test]
fn acp_stream_permission_cancel_and_provenance_remain_ordered_and_distinct() {
    let mut adapter = AcpV1ConnectionAdapter::new(
        r("connection/acp/stream"),
        vec!["target/deepseek-acp".into()],
    );
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "protocolVersion": 1, "agentCapabilities": {} }
        }))
        .unwrap();

    let first = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "hello" }
                }
            }
        }))
        .unwrap();
    let permission = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 91,
            "method": "session/request_permission",
            "params": {
                "sessionId": "s-1",
                "toolCall": { "toolCallId": "tool-7" },
                "options": [
                    { "optionId": "allow-once", "name": "Allow once" },
                    { "optionId": "reject", "name": "Reject" }
                ]
            }
        }))
        .unwrap();
    let cancelled = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": "s-1" }
        }))
        .unwrap();

    assert!(first[0].sequence < permission[0].sequence);
    assert!(permission[0].sequence < cancelled[0].sequence);
    assert!(matches!(
        first[0].kind,
        ConnectionSignalKind::AgentMessageChunk { .. }
    ));
    let ConnectionSignalKind::PermissionRequested { request } = &permission[0].kind else {
        panic!("permission must remain a native permission request");
    };
    assert_eq!(request.native_request_id, "91");
    assert_eq!(request.tool_call_id.as_deref(), Some("tool-7"));
    assert_eq!(request.provenance, vec!["target/deepseek-acp"]);
    assert!(matches!(cancelled[0].kind, ConnectionSignalKind::Cancelled));
}

#[test]
fn classic_process_uses_same_connection_seam_without_acp_identity_or_permission_semantics() {
    let mut adapter = ClassicProcessConnectionAdapter::new(
        r("connection/classic/codex"),
        vec!["codex".into(), "exec".into()],
        vec!["aikit classic client fixture".into()],
    );
    let descriptor = adapter.descriptor();
    assert_eq!(
        descriptor.protocol.family,
        ConnectionProtocolFamily::ClassicProcess
    );
    assert!(descriptor.capabilities.supports(SessionOpenMode::Create));
    assert!(!descriptor.capabilities.permission_requests);
    assert!(!descriptor.capabilities.reconnect);

    assert_eq!(adapter.initialize().unwrap().operation, "launch");
    assert_eq!(
        adapter.open_session(create_request()).unwrap().operation,
        "create"
    );
    let error = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Resume,
            native_session_id: Some("classic-1".into()),
            ..create_request()
        })
        .unwrap_err();
    assert_eq!(error.code(), "connection.session_operation_unsupported");

    let first = adapter
        .ingest(json!({
            "kind": "text",
            "nativeSessionId": "classic-1",
            "text": "classic output"
        }))
        .unwrap();
    assert!(matches!(
        first[0].kind,
        ConnectionSignalKind::AgentMessageChunk { .. }
    ));
}
