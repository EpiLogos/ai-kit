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

#[test]
fn acp_open_retains_reported_model_configuration_without_promoting_model_identity() {
    // A codec contract test; live provider acceptance is separately required.
    let mut adapter = AcpV1ConnectionAdapter::new(r("connection/acp/model-codec"), Vec::new());
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}))
        .unwrap();
    adapter.open_session(create_request()).unwrap();
    let opened = adapter
        .ingest(json!({"jsonrpc":"2.0","id":2,"result":{
        "sessionId":"native-codec-session","models":{"currentModelId":"provider/model",
        "availableModels":[{"modelId":"provider/model","name":"Provider Model"}]}}}))
        .unwrap();
    let ConnectionSignalKind::SessionOpened { binding } = &opened[0].kind else {
        panic!("expected opened binding")
    };
    let observation = binding.model_observation.as_ref().unwrap();
    assert_eq!(observation.current_model_id, "provider/model");
    assert_eq!(observation.available_models[0].model_id, "provider/model");
    assert!(observation.standing.contains("not-independent"));
    assert!(binding.agent.is_none() && binding.agent_session.is_none());
}

#[test]
fn load_and_resume_retain_requested_identity_and_reject_contradictions() {
    for mode in [SessionOpenMode::Load, SessionOpenMode::Resume] {
        for result in [
            serde_json::Value::Null,
            json!({}),
            json!({"sessionId":"native-kept"}),
        ] {
            let mut adapter = AcpV1ConnectionAdapter::new(r("connection/acp/continuation"), vec![]);
            adapter.initialize().unwrap();
            adapter.ingest(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}})).unwrap();
            let command = adapter
                .open_session(SessionOpenRequest {
                    mode,
                    native_session_id: Some("native-kept".into()),
                    agent_session: Some(r("agent-session/kept")),
                    ..create_request()
                })
                .unwrap();
            let signals = adapter
                .ingest(json!({"jsonrpc":"2.0","id":command.payload["id"],"result":result}))
                .unwrap();
            let ConnectionSignalKind::SessionOpened { binding } = &signals[0].kind else {
                panic!("expected load binding")
            };
            assert_eq!(binding.native_session_id, "native-kept");
            assert_eq!(binding.agent_session, Some(r("agent-session/kept")));
        }
        for result in [
            json!({"sessionId":"different"}),
            json!({"sessionId":""}),
            json!({"sessionId":null}),
            json!(7),
        ] {
            let mut adapter = AcpV1ConnectionAdapter::new(r("connection/acp/refusal"), vec![]);
            adapter.initialize().unwrap();
            adapter.ingest(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}})).unwrap();
            let command = adapter
                .open_session(SessionOpenRequest {
                    mode,
                    native_session_id: Some("native-kept".into()),
                    ..create_request()
                })
                .unwrap();
            assert!(adapter
                .ingest(json!({"jsonrpc":"2.0","id":command.payload["id"],"result":result}))
                .is_err());
        }
    }
}

fn resume_advertised_adapter(connection: &str) -> AcpV1ConnectionAdapter {
    let mut adapter = AcpV1ConnectionAdapter::new(r(connection), vec![]);
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": { "sessionCapabilities": { "resume": {}, "close": {} } }
            }
        }))
        .unwrap();
    adapter
}

#[test]
fn session_resume_advertised_sends_the_stabilized_session_resume_request_with_no_replay() {
    let mut adapter = resume_advertised_adapter("connection/acp/resume");
    let capabilities = adapter.negotiated_session_capabilities();
    assert!(capabilities.resume, "the negotiated resume fact is exposed");
    assert!(capabilities.close, "the negotiated close fact is exposed");

    let command = adapter
        .session_resume(
            "native-resume-9",
            "/workspace/project",
            vec![json!({ "name": "bimba", "command": "bimba-mcp", "args": [], "env": [] })],
        )
        .unwrap();
    assert_eq!(
        command.operation, "session/resume",
        "the stabilized resume operation rides the wire, not session/load with its replay"
    );
    assert_eq!(command.payload["params"]["sessionId"], "native-resume-9");
    assert_eq!(command.payload["params"]["cwd"], "/workspace/project");
    assert_eq!(
        command.payload["params"]["mcpServers"],
        json!([{ "name": "bimba", "command": "bimba-mcp", "args": [], "env": [] }]),
        "the request mirrors the session/new shape with the session id added"
    );

    // No replay events are fabricated: the response resolves through the same
    // SessionOpened binding path as session/new, and nothing else is emitted.
    let opened = adapter
        .ingest(json!({"jsonrpc":"2.0","id":command.payload["id"],"result":{}}))
        .unwrap();
    assert_eq!(opened.len(), 1);
    let ConnectionSignalKind::SessionOpened { binding } = &opened[0].kind else {
        panic!("expected opened binding");
    };
    assert_eq!(binding.native_session_id, "native-resume-9");
    assert_eq!(binding.opened_as, SessionOpenMode::Resume);
}

#[test]
fn resume_or_attach_without_the_advertised_capability_refuses_naming_sessioncapabilities_resume() {
    for mode in [SessionOpenMode::Resume, SessionOpenMode::Attach] {
        let mut adapter = AcpV1ConnectionAdapter::new(r("connection/acp/no-resume"), vec![]);
        adapter.initialize().unwrap();
        adapter
            .ingest(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "protocolVersion": 1, "agentCapabilities": {} }
            }))
            .unwrap();
        assert!(!adapter.negotiated_session_capabilities().resume);
        assert!(!adapter.negotiated_session_capabilities().close);

        let error = adapter
            .open_session(SessionOpenRequest {
                mode,
                native_session_id: Some("native-1".into()),
                ..create_request()
            })
            .unwrap_err();
        assert_eq!(error.code(), "connection.session_operation_unsupported");
        assert!(
            error.to_string().contains("sessionCapabilities.resume"),
            "the refusal names the missing capability, not a stale protocol claim: {error}"
        );
    }

    let mut adapter = AcpV1ConnectionAdapter::new(r("connection/acp/no-resume-typed"), vec![]);
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "protocolVersion": 1, "agentCapabilities": {} }
        }))
        .unwrap();
    let error = adapter
        .session_resume("native-1", "/workspace/project", Vec::new())
        .unwrap_err();
    assert!(
        error.to_string().contains("sessionCapabilities.resume"),
        "the typed resume route refuses before any wire message: {error}"
    );
}

#[test]
fn attach_rides_the_stabilized_resume_operation_where_it_is_advertised() {
    let mut adapter = resume_advertised_adapter("connection/acp/attach-resume");

    let command = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: Some("native-live".into()),
            ..create_request()
        })
        .unwrap();
    assert_eq!(
        command.operation, "session/resume",
        "attach has no dedicated ACP method; the advertised resume operation is the route"
    );
    assert_eq!(command.payload["params"]["sessionId"], "native-live");
    let opened = adapter
        .ingest(json!({"jsonrpc":"2.0","id":command.payload["id"],"result":{}}))
        .unwrap();
    let ConnectionSignalKind::SessionOpened { binding } = &opened[0].kind else {
        panic!("expected opened binding");
    };
    assert_eq!(binding.opened_as, SessionOpenMode::Attach);
    assert_eq!(binding.native_session_id, "native-live");
}
