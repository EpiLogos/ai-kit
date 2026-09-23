//! A person's Stop reaches Pi as `abort`. When Pi acknowledges it and the
//! in-flight tool or request then ends with Pi's own "operation was aborted"
//! error, the turn was stopped — not failed by the provider.
use aikit_adapters::agent_connection::{
    AgentConnectionAdapter, CancelRequest, ConnectionSignalKind, PromptRequest, SessionOpenMode,
    SessionOpenRequest,
};
use aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter;
use aikit_core::ResourceRef;
use serde_json::json;

fn attached() -> PiRpcConnectionAdapter {
    let mut adapter = PiRpcConnectionAdapter::new(
        ResourceRef::parse("connection/test/pi-abort").unwrap(),
        "/tmp".into(),
        vec![],
    );
    let init = adapter.initialize().unwrap();
    adapter
        .ingest(json!({"type":"response","id":init.payload["id"].clone(),"command":"get_state","success":true,"data":{"sessionId":"native-pi","isStreaming":false,"isCompacting":false,"pendingMessageCount":0}}))
        .unwrap();
    let open = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: None,
            cwd: "/tmp".into(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(ResourceRef::parse("agent-session/pi-abort").unwrap()),
        })
        .unwrap();
    let id = open.payload["id"].clone();
    adapter
        .ingest(json!({"type":"response","id":id,"command":"get_state","success":true,"data":{"sessionId":"native-pi","isStreaming":false,"isCompacting":false,"pendingMessageCount":0}}))
        .unwrap();
    adapter
}

fn settle_after(
    adapter: &mut PiRpcConnectionAdapter,
    abort: bool,
    acknowledged_first: bool,
) -> ConnectionSignalKind {
    adapter
        .prompt(PromptRequest {
            native_session_id: "native-pi".into(),
            prompt: json!("sleep"),
        })
        .unwrap();
    let cancel = abort.then(|| {
        adapter
            .cancel(CancelRequest {
                native_session_id: "native-pi".into(),
            })
            .unwrap()
    });
    let ack = |adapter: &mut PiRpcConnectionAdapter| {
        if let Some(cancel) = &cancel {
            adapter
                .ingest(json!({"type":"response","id":cancel.payload["id"].clone(),"command":"abort","success":true}))
                .unwrap();
        }
    };
    if acknowledged_first {
        ack(adapter);
    }
    adapter
        .ingest(json!({"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"This operation was aborted"}}))
        .unwrap();
    let signals = adapter.ingest(json!({"type":"agent_settled"})).unwrap();
    if !acknowledged_first {
        ack(adapter);
    }
    signals.last().unwrap().kind.clone()
}

#[test]
fn an_error_after_an_acknowledged_abort_is_the_stop() {
    let mut adapter = attached();
    assert_eq!(
        settle_after(&mut adapter, true, true),
        ConnectionSignalKind::Cancelled
    );
}

#[test]
fn the_same_error_without_an_abort_stays_a_failure() {
    let mut adapter = attached();
    assert!(matches!(
        settle_after(&mut adapter, false, false),
        ConnectionSignalKind::Failed { reason } if reason == "This operation was aborted"
    ));
}

#[test]
fn an_abort_acknowledged_only_after_settling_still_ends_as_the_stop() {
    let mut adapter = attached();
    assert_eq!(
        settle_after(&mut adapter, true, false),
        ConnectionSignalKind::Cancelled
    );
}
