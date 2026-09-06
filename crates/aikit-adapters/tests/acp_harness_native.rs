//! Explicit live acceptance of the existing generic ACP host with a configured
//! real harness. No fake process or transcript can satisfy this test.
//! The launcher must isolate destructive tools; this test only requests text.
use aikit_adapters::{
    agent_connection::{ConnectionSignalKind, SessionOpenMode, SessionOpenRequest},
    agent_session_host::{
        AgentSessionHost, AgentSessionHostLimits, HostEvent, SessionLane, TurnStop,
    },
    interactive_connection::AcpStableConnectionAdapter,
};
use aikit_core::ResourceRef;
use serde_json::json;
use std::time::{Duration, Instant};

fn receive(lane: &SessionLane, deadline: Instant) -> HostEvent {
    lane.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .expect("real ACP provider must deliver a bounded native event or terminal result")
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
#[ignore = "requires AIKIT_ACP_NATIVE_ARGV JSON argv for a real configured ACP harness; explicit provider acceptance"]
fn native_acp_stream_interrupt_and_resident_identity_survive_view_handle_drop() {
    let argv: Vec<String> = serde_json::from_str(
        &std::env::var("AIKIT_ACP_NATIVE_ARGV")
            .expect("Explicit real ACP harness argv is required"),
    )
    .unwrap();
    assert!(!argv.is_empty());
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let connection = ResourceRef::parse("connection/native-acp-acceptance").unwrap();
    let canonical = ResourceRef::parse("agent-session/native-acp-acceptance").unwrap();
    let adapter = AcpStableConnectionAdapter::new(
        connection,
        vec!["Real configured ACP harness; bounded provider acceptance".into()],
    );
    let host = AgentSessionHost::launch(
        adapter,
        &argv,
        Some(root.path()),
        // Reasoning harnesses may emit thousands of real thinking updates even
        // for a short answer. Use the host's explicit configurable bound.
        AgentSessionHostLimits {
            max_signals_per_turn: 16_384,
        },
    )
    .unwrap();
    let descriptor = host.initialize().unwrap();
    assert!(descriptor.capabilities.supports(SessionOpenMode::Create));
    let request = SessionOpenRequest {
        mode: SessionOpenMode::Create,
        native_session_id: None,
        cwd,
        additional_directories: vec![],
        mcp_servers: vec![],
        agent_session: Some(canonical.clone()),
    };
    let lane = host.open_session(request.clone()).unwrap();
    assert_eq!(lane.agent_session(), &canonical);
    assert_ne!(lane.binding().native_session_id, canonical.as_str());
    let native = lane.binding().native_session_id.clone();
    // A real harness may emit startup information after session/new, before
    // any prompt. Observe it separately; never mistake it for a turn response.
    let startup_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < startup_deadline,
            "Provider startup did not settle"
        );
        match lane.recv_timeout(Duration::from_millis(500)) {
            Ok(HostEvent::Signal(_)) => {}
            Ok(HostEvent::TurnEnded(_)) => panic!("No prompt was started"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(error) => panic!("Provider disconnected during startup: {error}"),
        }
    }

    let view_handle = lane
        .prompt(json!([{"type":"text","text":"Reply with exactly OI_NATIVE_STREAM_OK. Do not use tools."}]))
        .unwrap();
    drop(view_handle);
    assert_eq!(
        completed_text(&lane).trim(),
        "OI_NATIVE_STREAM_OK",
        "dropping a view/turn handle must not interrupt the native provider"
    );

    let turn = lane.prompt(json!([{"type":"text","text":"Write the integers from 1 to 20000, separated by spaces. Begin immediately. Do not use tools."}])).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match receive(&lane, deadline) {
            HostEvent::Signal(signal)
                if matches!(&signal.kind, ConnectionSignalKind::AgentMessageChunk { .. })
                    || matches!(&signal.kind, ConnectionSignalKind::Status {message} if message == "ACP session update: agent_thought_chunk") =>
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
        .any(|command| command == "session/cancel"));
    loop {
        if let HostEvent::TurnEnded(record) = receive(&lane, deadline) {
            assert_eq!(
                record.stop,
                TurnStop::Cancelled,
                "only an observed provider cancellation may finish this interruption"
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
        .prompt(json!([{"type":"text","text":"Reply with exactly OI_NATIVE_STILL_RESIDENT. Do not use tools."}]))
        .unwrap();
    assert_eq!(completed_text(&lane).trim(), "OI_NATIVE_STILL_RESIDENT");
    host.shutdown().unwrap();
}
