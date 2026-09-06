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
        AgentSessionHostLimits::default(),
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
    let mut observed = 0;
    let mut thoughts = 0;
    let mut previous = 0;
    loop {
        match receive(&lane, deadline) {
            HostEvent::Signal(signal) => {
                assert!(signal.sequence > previous, "actual ACP ordering must be preserved");
                previous=signal.sequence;
                observed+=1;
                if let ConnectionSignalKind::AgentThoughtChunk { text, content } = &signal.kind {
                    assert_eq!(content["content"]["text"].as_str(), Some(text.as_str()), "exposed thinking bytes must survive owner normalization");
                    if !text.is_empty(){thoughts+=1;}
                }
                if observed > 512 && thoughts > 0 {break;}
            }
            HostEvent::TurnEnded(record) => panic!("reasoning-heavy regression ended before crossing the former ceiling: {:?}; events={observed}, thoughts={thoughts}", record.stop),
        }
    }
    eprintln!("Actual ACP streamed {observed} ordered events including {thoughts} preserved thinking updates before explicit cancellation");
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

#[test]
#[ignore = "requires explicit actual ACP argv; tests deliberately configured operational policy"]
fn actual_acp_operational_limit_is_distinct_and_session_can_continue() {
    let argv:Vec<String>=serde_json::from_str(&std::env::var("AIKIT_ACP_NATIVE_ARGV").expect("Explicit real ACP argv required")).unwrap();
    let root=tempfile::tempdir().unwrap();
    let adapter=AcpStableConnectionAdapter::new(ResourceRef::parse("connection/native-operational-policy").unwrap(),vec!["Explicit actual ACP operational policy acceptance".into()]);
    let mut limits=AgentSessionHostLimits::default();limits.max_signals_per_turn=64;
    let host=AgentSessionHost::launch(adapter,&argv,Some(root.path()),limits).unwrap();host.initialize().unwrap();
    let lane=host.open_session(SessionOpenRequest{mode:SessionOpenMode::Create,native_session_id:None,cwd:root.path().to_string_lossy().into_owned(),additional_directories:vec![],mcp_servers:vec![],agent_session:Some(ResourceRef::parse("agent-session/native-operational-policy").unwrap())}).unwrap();
    while lane.recv_timeout(Duration::from_millis(500)).is_ok(){}
    let native=lane.binding().native_session_id.clone();
    let turn=lane.prompt(json!([{"type":"text","text":"Write the integers from 1 to 20000, separated by spaces. Begin immediately. Do not use tools."}])).unwrap();
    let deadline=Instant::now()+Duration::from_secs(120);
    loop {if let HostEvent::TurnEnded(record)=receive(&lane,deadline){assert_eq!(record.stop,TurnStop::OperationalLimit{max_signals:64});assert!(record.signals>64);break;}}
    drop(turn);assert!(host.transport_error().is_none());
    let continuation=lane.prompt(json!([{"type":"text","text":"Reply exactly OK. Do not use tools."}])).unwrap();
    loop {if let HostEvent::TurnEnded(record)=receive(&lane,Instant::now()+Duration::from_secs(120)){assert!(matches!(record.stop,TurnStop::Completed{..}|TurnStop::OperationalLimit{max_signals:64}),"Configured policy remains active on continuation: {:?}",record.stop);assert!(record.signals>0);break;}}
    drop(continuation);assert!(host.transport_error().is_none());
    assert_eq!(lane.binding().native_session_id,native);
}
