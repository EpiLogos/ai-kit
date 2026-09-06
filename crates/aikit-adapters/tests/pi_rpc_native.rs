//! Explicit live acceptance against the installed Pi harness and its configured
//! provider. No fake process or transcript can satisfy this test.
use aikit_adapters::{
    agent_connection::{ConnectionSignalKind, SessionOpenMode, SessionOpenRequest},
    agent_session_host::{
        AgentSessionHost, AgentSessionHostLimits, HostEvent, SessionLane, TurnStop,
    },
    pi_rpc_connection::PiRpcConnectionAdapter,
};
use aikit_core::ResourceRef;
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn receive(lane: &SessionLane, deadline: Instant) -> HostEvent {
    lane.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .expect("real Pi must deliver a bounded native event or terminal result")
}

fn completed_text(lane: &SessionLane) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut text = String::new();
    loop {
        match receive(lane, deadline) {
            HostEvent::Signal(signal) => {
                if let ConnectionSignalKind::AgentMessageChunk { text: delta } = signal.kind {
                    text.push_str(&delta);
                }
            }
            HostEvent::TurnEnded(record) => {
                assert!(
                    matches!(record.stop, TurnStop::Completed { .. }),
                    "native turn did not complete: {:?}",
                    record.stop
                );
                return text;
            }
        }
    }
}

#[test]
#[ignore = "requires OI_PI_BIN naming a real installed Pi 0.84 harness and configured provider; run explicitly for native acceptance"]
fn native_pi_stream_interrupt_and_resident_identity_survive_view_handle_drop() {
    let executable = PathBuf::from(
        std::env::var_os("OI_PI_BIN").expect("OI_PI_BIN must name the actual Pi executable"),
    );
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let connection = ResourceRef::parse("connection/native-pi-acceptance").unwrap();
    let canonical = ResourceRef::parse("agent-session/native-pi-acceptance").unwrap();
    let adapter = PiRpcConnectionAdapter::new(connection, cwd.clone(), vec!["installed Pi; isolated temporary cwd; native configured provider; tools and extensions disabled".into()]);
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--mode".into(),
        "rpc".into(),
        "--no-session".into(),
        "--no-tools".into(),
        "--no-extensions".into(),
    ];
    let host = AgentSessionHost::launch(
        adapter,
        &argv,
        Some(root.path()),
        AgentSessionHostLimits::default(),
    )
    .unwrap();
    let descriptor = host.initialize().unwrap();
    assert!(descriptor.capabilities.supports(SessionOpenMode::Attach));
    assert!(!descriptor.capabilities.supports(SessionOpenMode::Resume));
    assert!(!descriptor.capabilities.permission_requests);
    let request = SessionOpenRequest {
        mode: SessionOpenMode::Attach,
        native_session_id: None,
        cwd,
        additional_directories: vec![],
        mcp_servers: vec![],
        agent_session: Some(canonical.clone()),
    };
    let lane = host.open_session(request.clone()).unwrap();
    assert_eq!(lane.agent_session(), &canonical);
    assert_ne!(lane.binding().native_session_id, canonical.as_str());
    let mut another = request;
    another.agent_session = Some(ResourceRef::parse("agent-session/another-native-pi").unwrap());
    assert!(
        host.open_session(another).is_err(),
        "one Pi process must not masquerade as concurrent resident sessions"
    );
    let native = lane.binding().native_session_id.clone();

    let view_handle = lane
        .prompt(json!(
            "Reply with exactly OI_NATIVE_STREAM_OK. Do not use tools."
        ))
        .unwrap();
    drop(view_handle);
    assert_eq!(
        completed_text(&lane).trim(),
        "OI_NATIVE_STREAM_OK",
        "dropping a view/turn handle must not interrupt the native provider"
    );

    let turn = lane.prompt(json!("Write the integers from 1 to 20000, separated by spaces. Begin immediately. Do not use tools.")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match receive(&lane, deadline) {
            HostEvent::Signal(signal)
                if matches!(signal.kind, ConnectionSignalKind::AgentMessageChunk { .. }) =>
            {
                break
            }
            HostEvent::TurnEnded(record) => panic!(
                "native provider stopped before the mid-stream interrupt: {:?}",
                record.stop
            ),
            _ => {}
        }
    }
    let receipt = turn
        .interrupt(Some("native integration acceptance".into()))
        .unwrap();
    assert!(receipt
        .commands
        .iter()
        .any(|command| command == "clear_queue"));
    assert!(receipt.commands.iter().any(|command| command == "abort"));
    loop {
        if let HostEvent::TurnEnded(record) = receive(&lane, deadline) {
            assert_eq!(
                record.stop,
                TurnStop::Cancelled,
                "only an observed Pi cancellation may finish this interruption"
            );
            assert_eq!(record.agent_session, canonical);
            break;
        }
    }
    assert_eq!(
        host.identity(&canonical).unwrap().binding.native_session_id,
        native
    );
    let _turn = lane
        .prompt(json!(
            "Reply with exactly OI_NATIVE_STILL_RESIDENT. Do not use tools."
        ))
        .unwrap();
    assert_eq!(completed_text(&lane).trim(), "OI_NATIVE_STILL_RESIDENT");
    host.shutdown().unwrap();
}
