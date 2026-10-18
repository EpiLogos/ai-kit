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

#[test]
fn acp_lifecycle_updates_are_kept_verbatim_and_usage_rides_the_completed_turn() {
    use aikit_adapters::{PromptRequest, PROVIDER_PAYLOAD_LIMIT};

    let mut adapter = AcpV1ConnectionAdapter::new(
        r("connection/acp/lifecycle"),
        vec!["target/codex-acp".into()],
    );
    adapter.initialize().unwrap();
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "protocolVersion": 1, "agentCapabilities": {} }
        }))
        .unwrap();

    // The kinds codex-acp 1.13.1 emits beside text and tools, plus a kind no
    // AIKit release knows: each is kept as the provider sent it.
    let updates = [
        json!({"sessionUpdate": "usage_update", "used": 18342, "size": 272000}),
        json!({"sessionUpdate": "plan", "entries": [
            {"content": "Read the gateway", "priority": "high", "status": "in_progress"}
        ]}),
        json!({"sessionUpdate": "compaction_update", "status": "completed",
               "_meta": {"tokensBefore": 240000, "tokensAfter": 31000}}),
        json!({"sessionUpdate": "session_info_update", "title": "Gateway outage"}),
        json!({"sessionUpdate": "a_kind_from_next_year", "anything": [1, 2, 3]}),
    ];
    for update in &updates {
        let signals = adapter
            .ingest(json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": { "sessionId": "s-1", "update": update }
            }))
            .unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].native_session_id.as_deref(), Some("s-1"));
        let ConnectionSignalKind::ProviderUpdate {
            session_update,
            payload,
            truncated_from_bytes,
        } = &signals[0].kind
        else {
            panic!(
                "{update} must be kept as a provider update: {:?}",
                signals[0].kind
            );
        };
        assert_eq!(session_update, update["sessionUpdate"].as_str().unwrap());
        assert_eq!(payload, update, "the update object is kept as sent");
        assert_eq!(*truncated_from_bytes, None);
    }

    // A runaway update is bounded, and the bound is named.
    let huge = json!({"sessionUpdate": "plan", "entries": "x".repeat(PROVIDER_PAYLOAD_LIMIT * 2)});
    let signals = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": "s-1", "update": huge }
        }))
        .unwrap();
    let ConnectionSignalKind::ProviderUpdate {
        payload,
        truncated_from_bytes,
        ..
    } = &signals[0].kind
    else {
        panic!("an oversized update is still kept");
    };
    assert!(payload.as_str().unwrap().len() <= PROVIDER_PAYLOAD_LIMIT);
    assert_eq!(*truncated_from_bytes, Some(huge.to_string().len()));

    // The prompt result's usage rides the Completed signal.
    let prompt = adapter
        .prompt(PromptRequest {
            native_session_id: "s-1".into(),
            prompt: json!([{"type": "text", "text": "go"}]),
        })
        .unwrap();
    let id = prompt.payload["id"].as_u64().unwrap();
    let usage = json!({"inputTokens": 1200, "outputTokens": 340, "totalTokens": 1540,
                       "cachedReadTokens": 800});
    let completed = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "stopReason": "end_turn", "usage": usage }
        }))
        .unwrap();
    assert_eq!(
        completed[0].kind,
        ConnectionSignalKind::Completed {
            stop_reason: "end_turn".into(),
            usage: Some(usage),
        }
    );

    // A provider that reports no usage gets none invented.
    let prompt = adapter
        .prompt(PromptRequest {
            native_session_id: "s-1".into(),
            prompt: json!([{"type": "text", "text": "again"}]),
        })
        .unwrap();
    let id = prompt.payload["id"].as_u64().unwrap();
    let completed = adapter
        .ingest(json!({"jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" }}))
        .unwrap();
    assert_eq!(
        completed[0].kind,
        ConnectionSignalKind::Completed {
            stop_reason: "end_turn".into(),
            usage: None,
        }
    );
}
