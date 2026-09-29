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
#[ignore = "requires OI_PI_BIN naming a real installed Pi 0.84 harness; discovery and setting the already-current model perform no inference"]
fn native_pi_discovers_and_confirms_the_exact_current_model_without_inference() {
    let executable = PathBuf::from(
        std::env::var_os("OI_PI_BIN").expect("OI_PI_BIN must name the actual Pi executable"),
    );
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let canonical = ResourceRef::parse("agent-session/native-pi-model-controls").unwrap();
    let adapter = PiRpcConnectionAdapter::new(
        ResourceRef::parse("connection/native-pi-model-controls").unwrap(),
        cwd.clone(),
        vec![
            "installed Pi; isolated temporary cwd; native model configuration; no inference".into(),
        ],
    );
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
    host.initialize().unwrap();
    let lane = host
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: None,
            cwd,
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(canonical),
        })
        .unwrap();
    let before = lane.binding().model_observation.clone().unwrap();
    assert!(!before.available_models.is_empty());
    assert!(before
        .available_models
        .iter()
        .any(|model| model.model_id == before.current_model_id));
    let controls = lane.model_controls().unwrap();
    assert!(controls.model_selection);
    assert!(!controls.reasoning_effort_selection);
    let unsupported = lane.set_reasoning_effort("high").unwrap_err();
    assert_eq!(
        unsupported.code(),
        "connection.pi_rpc.reasoning_effort_selection_unsupported"
    );
    assert_eq!(
        host.identity(lane.agent_session())
            .unwrap()
            .binding
            .model_observation,
        Some(before.clone()),
        "unsupported Pi reasoning must not mutate native model configuration"
    );
    let receipt = lane.set_model(&before.current_model_id).unwrap();
    assert_eq!(receipt.previous, before);
    assert_eq!(receipt.current.current_model_id, before.current_model_id);
    assert_eq!(
        host.identity(lane.agent_session())
            .unwrap()
            .binding
            .model_observation,
        Some(receipt.current)
    );
    host.shutdown().unwrap();
}

#[test]
#[ignore = "requires OI_PI_BIN naming a real installed Pi 0.84 harness; pinned-state refusal performs no inference"]
fn native_pinned_pi_initialization_refuses_an_unconfirmed_model_without_inference() {
    let executable = PathBuf::from(
        std::env::var_os("OI_PI_BIN").expect("OI_PI_BIN must name the actual Pi executable"),
    );
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--mode".into(),
        "rpc".into(),
        "--no-session".into(),
        "--no-tools".into(),
        "--no-extensions".into(),
    ];
    let observed_host = AgentSessionHost::launch(
        PiRpcConnectionAdapter::new(
            ResourceRef::parse("connection/native-pi-pinned-basis-read").unwrap(),
            cwd.clone(),
            vec!["installed Pi native state read; no inference".into()],
        ),
        &argv,
        Some(root.path()),
        AgentSessionHostLimits::default(),
    )
    .unwrap();
    observed_host.initialize().unwrap();
    let observed_lane = observed_host
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: None,
            cwd: cwd.clone(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(
                ResourceRef::parse("agent-session/native-pi-pinned-basis-read").unwrap(),
            ),
        })
        .unwrap();
    let current = observed_lane
        .binding()
        .model_observation
        .as_ref()
        .unwrap()
        .current_model_id
        .clone();
    let (provider, model_id) = current
        .split_once('/')
        .expect("Pi selection identity must preserve exact provider/model");
    let unconfirmed_model = format!("{model_id}-aikit-intentionally-unconfirmed");
    observed_host.shutdown().unwrap();

    let pinned = PiRpcConnectionAdapter::new(
        ResourceRef::parse("connection/native-pi-pinned-refusal").unwrap(),
        cwd,
        vec!["installed Pi pinned native state check; no inference".into()],
    )
    .with_selected_model(provider, &unconfirmed_model)
    .unwrap();
    let pinned_host = AgentSessionHost::launch(
        pinned,
        &argv,
        Some(root.path()),
        AgentSessionHostLimits::default(),
    )
    .unwrap();
    let error = pinned_host.initialize().unwrap_err();
    assert_eq!(error.code(), "agent_session_host.handshake_failed");
    assert_eq!(
        error.message(),
        "Pi native state does not confirm the selected provider/model; no default or fallback is admitted"
    );
    pinned_host.shutdown().unwrap();
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

#[test]
#[ignore = "requires OI_PI_BIN naming the installed Pi harness; read-only native model observation"]
fn native_pi_advertises_one_current_model_with_its_real_name() {
    let executable =
        PathBuf::from(std::env::var_os("OI_PI_BIN").expect("OI_PI_BIN names installed Pi"));
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let adapter = PiRpcConnectionAdapter::new(
        ResourceRef::parse("connection/native-pi-model-name").unwrap(),
        cwd.clone(),
        vec!["Pi native get_state".into()],
    );
    let host = AgentSessionHost::launch(
        adapter,
        &[
            executable.to_string_lossy().into_owned(),
            "--mode".into(),
            "rpc".into(),
            "--no-session".into(),
            "--no-tools".into(),
            "--no-extensions".into(),
        ],
        Some(root.path()),
        AgentSessionHostLimits::default(),
    )
    .unwrap();
    host.initialize().unwrap();
    let lane = host
        .open_session(SessionOpenRequest {
            mode: SessionOpenMode::Attach,
            native_session_id: None,
            cwd,
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(ResourceRef::parse("agent-session/native-pi-model-name").unwrap()),
        })
        .unwrap();
    let observation = lane
        .binding()
        .model_observation
        .as_ref()
        .expect("Pi discloses its configured model without a policy override");
    assert_eq!(observation.available_models.len(), 1);
    let model = &observation.available_models[0];
    assert_eq!(model.model_id, observation.current_model_id);
    let identity = model
        .roster_identity
        .as_ref()
        .expect("Pi supplies exact native roster coordinates");
    assert_eq!(
        identity.provider_ref,
        format!("provider:{}", observation.native_provider.as_ref().unwrap())
    );
    assert_eq!(identity.provider_native_id, model.model_id);
    assert_eq!(identity.harness_slug.as_deref(), Some("pi"));
    assert!(!model.name.trim().is_empty());
    assert_ne!(model.name, "harness");
    assert_ne!(model.name, "Unnamed model");
    println!(
        "native Pi current model: {} ({})",
        model.name, model.model_id
    );
}
