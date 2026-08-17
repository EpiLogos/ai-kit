use aikit_adapters::{
    AcpStableConnectionAdapter, AgentConnectionAdapter, CancelRequest,
    InteractiveAgentConnectionAdapter, PermissionDecision, SessionOpenMode,
};
use aikit_core::resource::ResourceRef;
use serde_json::json;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn negotiate(adapter: &mut AcpStableConnectionAdapter) {
    let init = adapter.initialize().unwrap();
    assert_eq!(init.payload["id"], 1);
    adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {
                    "sessionCapabilities": {
                        "resume": {},
                        "close": {},
                        "list": {}
                    }
                }
            }
        }))
        .unwrap();
}

#[test]
fn stable_acp_wrapper_preserves_string_permission_ids_and_exact_selected_outcome() {
    let mut adapter = AcpStableConnectionAdapter::new(
        r("connection/acp/stable"),
        vec!["agentclientprotocol/schema/v1".into()],
    );
    negotiate(&mut adapter);

    let signals = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": "permission-7",
            "method": "session/request_permission",
            "params": {
                "sessionId": "native-session",
                "toolCall": { "toolCallId": "tool-1" },
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "reject", "name": "Reject", "kind": "reject_once" }
                ]
            }
        }))
        .unwrap();
    let aikit_adapters::ConnectionSignalKind::PermissionRequested { request } = &signals[0].kind
    else {
        panic!("expected permission request");
    };
    assert_eq!(request.native_request_id, "s:permission-7");
    assert_eq!(request.native_session_id, "native-session");

    let response = adapter
        .respond_permission(
            request,
            PermissionDecision::Selected {
                option_id: "allow".into(),
            },
        )
        .unwrap();
    assert_eq!(response.payload["id"], "permission-7");
    assert_eq!(
        response.payload["result"]["outcome"]["outcome"],
        "selected"
    );
    assert_eq!(
        response.payload["result"]["outcome"]["optionId"],
        "allow"
    );
}

#[test]
fn invalid_permission_choice_does_not_consume_the_pending_request() {
    let mut adapter =
        AcpStableConnectionAdapter::new(r("connection/acp/permission-retry"), Vec::new());
    negotiate(&mut adapter);

    let signals = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": "permission-retry",
            "method": "session/request_permission",
            "params": {
                "sessionId": "native-session",
                "toolCall": { "toolCallId": "tool-2" },
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                ]
            }
        }))
        .unwrap();
    let aikit_adapters::ConnectionSignalKind::PermissionRequested { request } = &signals[0].kind
    else {
        panic!("expected permission request");
    };

    let error = adapter
        .respond_permission(
            request,
            PermissionDecision::Selected {
                option_id: "not-offered".into(),
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "connection.acp.permission_option_unknown");

    let response = adapter
        .respond_permission(
            request,
            PermissionDecision::Selected {
                option_id: "allow".into(),
            },
        )
        .unwrap();
    assert_eq!(response.payload["id"], "permission-retry");
    assert_eq!(
        response.payload["result"]["outcome"]["optionId"],
        "allow"
    );
}

#[test]
fn prompt_cancel_answers_pending_permissions_cancelled_before_session_cancel() {
    let mut adapter = AcpStableConnectionAdapter::new(r("connection/acp/cancel"), Vec::new());
    negotiate(&mut adapter);
    let signals = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": 88,
            "method": "session/request_permission",
            "params": {
                "sessionId": "s-1",
                "toolCall": { "toolCallId": "tool-9" },
                "options": [{ "optionId": "yes", "name": "Yes", "kind": "allow_once" }]
            }
        }))
        .unwrap();
    assert_eq!(signals.len(), 1);

    let commands = adapter
        .coordinated_cancel(CancelRequest {
            native_session_id: "s-1".into(),
        })
        .unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].payload["id"], 88);
    assert_eq!(
        commands[0].payload["result"]["outcome"]["outcome"],
        "cancelled"
    );
    assert_eq!(commands[1].operation, "session/cancel");
}

#[test]
fn resume_close_and_transport_disconnect_remain_distinct_lifecycle_operations() {
    let mut adapter = AcpStableConnectionAdapter::new(r("connection/acp/lifecycle"), Vec::new());
    negotiate(&mut adapter);

    assert!(adapter
        .descriptor()
        .capabilities
        .supports(SessionOpenMode::Resume));
    assert!(adapter.negotiated_session_capabilities().close);
    assert!(adapter.negotiated_session_capabilities().list);

    let close = adapter.close_native_session("native-42").unwrap();
    assert_eq!(close.operation, "session/close");
    assert_eq!(close.payload["params"]["sessionId"], "native-42");
    let close_id = close.payload["id"].clone();
    let close_signals = adapter
        .ingest(json!({
            "jsonrpc": "2.0",
            "id": close_id,
            "result": {}
        }))
        .unwrap();
    assert_eq!(close_signals.len(), 1);
    assert_eq!(
        close_signals[0].native_session_id.as_deref(),
        Some("native-42")
    );
    assert!(matches!(
        close_signals[0].kind,
        aikit_adapters::ConnectionSignalKind::Status { .. }
    ));

    let disconnect = adapter.disconnect().unwrap();
    assert_eq!(disconnect.operation, "disconnect-transport");

    let reconnect = adapter.reconnect().unwrap_err();
    assert_eq!(reconnect.code(), "connection.reconnect_unsupported");
}
