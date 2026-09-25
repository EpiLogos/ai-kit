//! Real operating-system process failures through the production session host.
//! No provider responses or model output are manufactured by these cases.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use aikit_adapters::{
    pi_rpc_connection::PiRpcConnectionAdapter, AcpStableConnectionAdapter, AgentSessionHost,
    AgentSessionHostLimits, SessionOpenMode, SessionOpenRequest,
};
use aikit_core::ResourceRef;
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn launch(argv: &[String]) -> AgentSessionHost {
    AgentSessionHost::launch(
        AcpStableConnectionAdapter::new(
            ResourceRef::parse("connection/native-startup-process-failure").unwrap(),
            vec!["real process lifecycle; no provider or inference claim".into()],
        ),
        argv,
        None,
        AgentSessionHostLimits::default(),
    )
    .unwrap()
}

fn process_state(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    (!state.is_empty()).then_some(state)
}

#[test]
fn unresponsive_startup_terminates_and_reaps_the_owned_process_group() {
    let directory = tempfile::tempdir().unwrap();
    let pids_path = directory.path().join("owned-pids.json");
    // This actual process accepts no protocol. Its child keeps inherited pipes
    // open, so killing only the leader would leave host cleanup hanging.
    let program = r#"
import json, os, pathlib, subprocess, sys, time
child = subprocess.Popen(["/bin/sleep", "60"])
pathlib.Path(sys.argv[1]).write_text(json.dumps([os.getpid(), child.pid]))
sys.stdin.readline()
time.sleep(60)
"#;
    let host = launch(&[
        "python3".into(),
        "-u".into(),
        "-c".into(),
        program.into(),
        pids_path.to_string_lossy().into_owned(),
    ]);
    let ready_by = Instant::now() + Duration::from_secs(5);
    let pids: Vec<u32> = loop {
        if let Ok(bytes) = std::fs::read(&pids_path) {
            if let Ok(pids) = serde_json::from_slice(&bytes) {
                break pids;
            }
        }
        assert!(
            Instant::now() < ready_by,
            "owned child did not become ready"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|pid| process_state(*pid).is_some()));

    let started = Instant::now();
    let failure = host
        .initialize_before(started + Duration::from_millis(250))
        .unwrap_err();
    assert_eq!(failure.code(), "agent_session_host.control_timeout");
    assert_eq!(
        failure
            .details()
            .get("cleanup_confirmed")
            .map(String::as_str),
        Some("true")
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    let exit = host.shutdown().unwrap().expect("owned leader was reaped");
    assert!(!exit.success());
    assert!(process_state(pids[0]).is_none(), "leader was not reaped");
    let child_state = process_state(pids[1]);
    assert!(
        child_state
            .as_deref()
            .is_none_or(|state| state.starts_with('Z')),
        "owned descendant remains runnable: {child_state:?}"
    );
}

#[test]
fn actual_child_exit_is_reported_as_transport_failure_not_timeout() {
    let host = launch(&["/bin/sh".into(), "-c".into(), "read request; exit 7".into()]);
    let failure = host
        .initialize_before(Instant::now() + Duration::from_secs(5))
        .unwrap_err();
    assert_eq!(failure.code(), "agent_session_host.transport_closed");
    assert_eq!(host.shutdown().unwrap().unwrap().code(), Some(7));
}

#[test]
#[ignore = "requires OI_PI_BIN naming the installed Pi harness; actual protocol startup only, no inference"]
fn actual_pi_session_open_does_not_renew_an_elapsed_startup_deadline() {
    let binary = std::env::var("OI_PI_BIN").expect("set OI_PI_BIN to the installed Pi harness");
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().to_string_lossy().into_owned();
    let host = AgentSessionHost::launch(
        PiRpcConnectionAdapter::new(
            ResourceRef::parse("connection/native-pi-startup-deadline").unwrap(),
            cwd.clone(),
            vec!["actual installed Pi protocol; no prompt or inference".into()],
        ),
        &[
            binary,
            "--mode".into(),
            "rpc".into(),
            "--no-session".into(),
            "--no-tools".into(),
            "--no-extensions".into(),
        ],
        Some(directory.path()),
        AgentSessionHostLimits::default(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    host.initialize_before(deadline).unwrap();
    thread::sleep(deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(20));
    let started = Instant::now();
    let failure = host
        .open_session_before(
            SessionOpenRequest {
                mode: SessionOpenMode::Attach,
                native_session_id: None,
                cwd,
                additional_directories: vec![],
                mcp_servers: vec![],
                agent_session: Some(
                    ResourceRef::parse("agent-session/native-pi-startup-deadline").unwrap(),
                ),
            },
            deadline,
        )
        .expect_err("an elapsed startup deadline must not open a session");
    assert_eq!(failure.code(), "agent_session_host.control_timeout");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        host.shutdown().unwrap().is_some(),
        "Pi child must be reaped"
    );
}
