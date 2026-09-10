//! Real process conformance for `aikit.connection-adapter/v1`.
//!
//! The ACP test is provider-gated because CI must install a current pinned ACP
//! Agent. The classic fixture uses only Python's stdlib and is therefore an
//! always-on real non-ACP process test on Unix.

use aikit_adapters::{
    AcpV1ConnectionAdapter, AgentConnectionAdapter, CancelRequest,
    ClassicProcessConnectionAdapter, ConnectionProcess, ConnectionSignalKind,
    PromptRequest, SessionOpenMode, SessionOpenRequest,
};
use aikit_core::resource::ResourceRef;
use serde_json::json;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn open_request(agent_session: Option<ResourceRef>) -> SessionOpenRequest {
    SessionOpenRequest {
        mode: SessionOpenMode::Create,
        native_session_id: None,
        cwd: std::env::current_dir().unwrap().display().to_string(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        agent_session,
    }
}

fn require_real_acp_command() -> Option<Vec<String>> {
    match std::env::var("AIKIT_ACP_REAL_COMMAND") {
        Ok(raw) => Some(shell_words::split(&raw).expect("AIKIT_ACP_REAL_COMMAND must be shell words")),
        Err(_) if std::env::var_os("AIKIT_REQUIRE_ACP_REAL").is_some() => {
            panic!("AIKIT_REQUIRE_ACP_REAL is set but AIKIT_ACP_REAL_COMMAND is absent")
        }
        Err(_) => {
            eprintln!("SKIP real_acp_sdk_target_streams_and_preserves_identity_boundary: AIKIT_ACP_REAL_COMMAND is not configured");
            None
        }
    }
}

#[test]
fn real_acp_sdk_target_streams_and_preserves_identity_boundary() {
    let Some(argv) = require_real_acp_command() else {
        return;
    };
    let canonical = r("agent-session/acp-real");
    let mut adapter = AcpV1ConnectionAdapter::new(
        r("connection/acp/official-python-sdk"),
        vec!["agentclientprotocol/python-sdk pinned real target".into()],
    );
    let mut process = ConnectionProcess::spawn(&argv, None).unwrap();

    process.send_json(&adapter.initialize().unwrap()).unwrap();
    let init_signals = adapter.ingest(process.read_json().unwrap()).unwrap();
    assert!(init_signals.iter().any(|signal| matches!(
        signal.kind,
        ConnectionSignalKind::Status { .. }
    )));
    assert!(adapter.negotiated_capabilities().supports(SessionOpenMode::Create));

    process
        .send_json(&adapter.open_session(open_request(Some(canonical.clone()))).unwrap())
        .unwrap();
    let opened = adapter.ingest(process.read_json().unwrap()).unwrap();
    let binding = opened
        .iter()
        .find_map(|signal| match &signal.kind {
            ConnectionSignalKind::SessionOpened { binding } => Some(binding.clone()),
            _ => None,
        })
        .expect("real ACP target must return session/new identity");
    assert_eq!(binding.agent_session.as_ref(), Some(&canonical));
    assert!(!binding.native_session_id.is_empty());
    let first_native = binding.native_session_id.clone();

    process
        .send_json(
            &adapter
                .prompt(PromptRequest {
                    native_session_id: first_native.clone(),
                    prompt: json!([{ "type": "text", "text": "hello from AIKit" }]),
                })
                .unwrap(),
        )
        .unwrap();

    let mut saw_chunk = false;
    let mut saw_completed = false;
    for _ in 0..4 {
        let signals = adapter.ingest(process.read_json().unwrap()).unwrap();
        for signal in signals {
            match signal.kind {
                ConnectionSignalKind::AgentMessageChunk { text } => {
                    assert_eq!(text, "hello from AIKit");
                    saw_chunk = true;
                }
                ConnectionSignalKind::Completed { stop_reason } => {
                    assert_eq!(stop_reason, "end_turn");
                    saw_completed = true;
                }
                _ => {}
            }
        }
        if saw_chunk && saw_completed {
            break;
        }
    }
    assert!(saw_chunk, "real ACP target must emit session/update streaming");
    assert!(saw_completed, "real ACP target must complete the prompt");

    // The stable protocol carries cancel as a notification. The official echo
    // target has no long-running tool/permission phase, so successful delivery
    // is the truthful live acceptance here; permission semantics remain absent
    // from this specimen rather than fabricated.
    process
        .send_json(
            &adapter
                .cancel(CancelRequest {
                    native_session_id: first_native.clone(),
                })
                .unwrap(),
        )
        .unwrap();
    assert!(process.is_running().unwrap());

    process.terminate().unwrap();
    assert!(!process.is_running().unwrap());

    // Reconnecting the transport to this target is a new target process. The
    // echo Agent advertises no load/resume continuity, so AIKit must not infer
    // continuity from the canonical AgentSession or the connection ref.
    let mut second_adapter = AcpV1ConnectionAdapter::new(
        r("connection/acp/official-python-sdk"),
        vec!["agentclientprotocol/python-sdk restarted real target".into()],
    );
    let mut second = ConnectionProcess::spawn(&argv, None).unwrap();
    second
        .send_json(&second_adapter.initialize().unwrap())
        .unwrap();
    second_adapter.ingest(second.read_json().unwrap()).unwrap();
    assert!(!second_adapter
        .negotiated_capabilities()
        .supports(SessionOpenMode::Resume));
    second
        .send_json(
            &second_adapter
                .open_session(open_request(Some(canonical.clone())))
                .unwrap(),
        )
        .unwrap();
    let reopened = second_adapter.ingest(second.read_json().unwrap()).unwrap();
    let second_binding = reopened
        .iter()
        .find_map(|signal| match &signal.kind {
            ConnectionSignalKind::SessionOpened { binding } => Some(binding),
            _ => None,
        })
        .unwrap();
    assert_eq!(second_binding.agent_session.as_ref(), Some(&canonical));
    assert_ne!(
        second_binding.native_session_id, first_native,
        "provider restart must not counterfeit protocol-native session continuity"
    );
}

#[cfg(unix)]
#[test]
fn real_classic_process_streams_interrupts_and_does_not_claim_reconnect() {
    let script = r#"
import json, signal, sys
native = 'classic-native-1'
def interrupted(signum, frame):
    print(json.dumps({'kind':'cancelled','nativeSessionId':native}), flush=True)
signal.signal(signal.SIGINT, interrupted)
for line in sys.stdin:
    message = json.loads(line)
    if 'prompt' in message:
        print(json.dumps({'kind':'text','nativeSessionId':message.get('nativeSessionId', native),'text':message['prompt']}), flush=True)
"#;
    let argv = vec!["python3".into(), "-u".into(), "-c".into(), script.into()];
    let mut adapter = ClassicProcessConnectionAdapter::new(
        r("connection/classic/python-stdio"),
        argv.clone(),
        vec!["real classic stdio fixture".into()],
    );
    let descriptor = adapter.descriptor();
    assert!(!descriptor.capabilities.permission_requests);
    assert!(!descriptor.capabilities.reconnect);
    assert!(!descriptor.capabilities.supports(SessionOpenMode::Resume));
    assert_eq!(adapter.initialize().unwrap().operation, "launch");
    assert_eq!(
        adapter.open_session(open_request(Some(r("agent-session/classic-real")))).unwrap().operation,
        "create"
    );

    let mut process = ConnectionProcess::spawn(&argv, None).unwrap();
    let prompt = adapter
        .prompt(PromptRequest {
            native_session_id: "classic-native-1".into(),
            prompt: json!("classic hello"),
        })
        .unwrap();
    process.send_json(&prompt).unwrap();
    let streamed = adapter.ingest(process.read_json().unwrap()).unwrap();
    assert!(matches!(
        &streamed[0].kind,
        ConnectionSignalKind::AgentMessageChunk { text } if text == "classic hello"
    ));

    let cancel = adapter
        .cancel(CancelRequest {
            native_session_id: "classic-native-1".into(),
        })
        .unwrap();
    assert_eq!(cancel.operation, "interrupt");
    process.interrupt().unwrap();
    let cancelled = adapter.ingest(process.read_json().unwrap()).unwrap();
    assert!(matches!(cancelled[0].kind, ConnectionSignalKind::Cancelled));

    process.terminate().unwrap();
    assert!(!process.is_running().unwrap());
    let resume_error = adapter
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Resume,
            native_session_id: Some("classic-native-1".into()),
            ..open_request(Some(r("agent-session/classic-real")))
        })
        .unwrap_err();
    assert_eq!(resume_error.code(), "connection.session_operation_unsupported");

    // A new process can be launched, but that is process replacement, not a
    // protocol reconnect and not evidence that the prior native session exists.
    let mut replacement = ConnectionProcess::spawn(&argv, None).unwrap();
    assert!(replacement.is_running().unwrap());
    replacement.terminate().unwrap();
}
