//! Real Unix IPC lifecycle and optional real installed ACP provider startup.
//! No simulated ACP transcript or model response is used.
#![cfg(unix)]
use aikit_cli::encounter_service::{
    request, serve, EncounterProvider, EncounterRequest, EncounterService,
};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::ResourceRef;
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use std::{
    fs,
    os::unix::net::UnixStream,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn launch(home: AikitHome, socket: &Path) -> (thread::JoinHandle<()>, mpsc::Receiver<()>) {
    let socket = socket.to_owned();
    let (done, finished) = mpsc::channel();
    let handle = thread::spawn(move || {
        serve(home, &socket).unwrap();
        done.send(()).unwrap();
    });
    (handle, finished)
}
fn ready(socket: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(value) = request(socket, &EncounterRequest::Health) {
            if value["ok"] == true {
                return value["data"]["pid"].as_u64().unwrap() as u32;
            }
        }
        assert!(
            Instant::now() < deadline,
            "real owner did not bind its socket"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
fn attach(home: &AikitHome) -> (SessionSpaceRef, ResourceRef) {
    let store = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/shutdown-acceptance").unwrap();
    let session = ResourceRef::parse("agent-session/shutdown-acceptance").unwrap();
    let create = store
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: None,
            },
        )
        .unwrap();
    store.apply(&create).unwrap();
    let attach = store
        .stage(
            Some(&space),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: session.clone(),
                    purpose: Some("Real owner lifecycle acceptance".into()),
                    provenance: vec!["temporary local acceptance".into()],
                },
            },
        )
        .unwrap();
    store.apply(&attach).unwrap();
    (space, session)
}

#[test]
fn explicit_owner_shutdown_acknowledges_then_releases_socket_and_lock() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let socket = temp.path().join("ipc/owner.sock");
    let (server, finished) = launch(home.clone(), &socket);
    let pid = ready(&socket);
    let wrong = request(
        &socket,
        &EncounterRequest::Shutdown {
            expected_pid: pid.wrapping_add(1),
        },
    )
    .unwrap();
    assert_eq!(wrong["ok"], false);
    assert_eq!(wrong["error"]["code"], "encounter.owner_changed");
    // An ordinary IPC detach must leave the owner alive.
    drop(UnixStream::connect(&socket).unwrap());
    assert_eq!(ready(&socket), pid);
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true);
    assert_eq!(ack["data"]["shutdown"], true);
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    assert!(!socket.exists());
    // A fresh owner can acquire the same native owner lock after clean exit.
    let (next, finished) = launch(home, &socket);
    let pid = ready(&socket);
    assert_eq!(
        request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap()["ok"],
        true
    );
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    next.join().unwrap();
}

#[test]
fn direct_service_shutdown_is_idempotent_and_denies_later_effects() {
    let temp = tempfile::tempdir().unwrap();
    let service = EncounterService::new(AikitHome::at(temp.path().join("aikit"))).unwrap();
    let shutdown = || EncounterRequest::Shutdown {
        expected_pid: std::process::id(),
    };
    let first = service.apply(shutdown()).unwrap();
    assert_eq!(service.apply(shutdown()).unwrap(), first);
    assert_eq!(
        service.apply(EncounterRequest::Health).unwrap_err().code(),
        "encounter.owner_stopped"
    );
}

/// Requires an actual installed ACP command, e.g. the pinned Pi ACP bridge.
/// Starts and opens a real session but submits no prompt/model workload.
#[test]
#[ignore = "requires AIKIT_SHUTDOWN_NATIVE_ARGV naming an actual installed ACP provider"]
fn native_acp_owner_shutdown_reaps_provider_and_preserves_history() {
    let actual: Vec<String> = serde_json::from_str(
        &std::env::var("AIKIT_SHUTDOWN_NATIVE_ARGV").expect("explicit actual ACP argv required"),
    )
    .unwrap();
    assert!(!actual.is_empty());
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let (space, session) = attach(&home);
    let marker = temp.path().join("provider-pids");
    // A real OS launcher surrounds the real provider; it does not implement ACP.
    // Explicit stdin redirection prevents the shell's background /dev/null rule.
    let mut argv = vec![
        "/bin/sh".into(),
        "-c".into(),
        "marker=$1; shift; \"$@\" <&0 & echo \"$$ $!\" > \"$marker\"; wait".into(),
        "sh".into(),
        marker.display().to_string(),
    ];
    argv.extend(actual);
    EncounterService::configure(
        &home,
        EncounterProvider {
            model_policy: None,
            protocol: Default::default(),
            id: "native-shutdown".into(),
            label: "Actual installed ACP lifecycle".into(),
            argv,
            required_context: None,
        },
    )
    .unwrap();
    let socket = temp.path().join("ipc/owner.sock");
    let (server, finished) = launch(home.clone(), &socket);
    let pid = ready(&socket);
    let opened = request(
        &socket,
        &EncounterRequest::Open {
            space,
            agent_session: session.clone(),
            provider: "native-shutdown".into(),
            cwd: temp.path().to_owned(),
        },
    )
    .unwrap();
    assert_eq!(opened["ok"], true, "actual ACP setup failed: {opened}");
    let native = opened["data"]["native_session_id"].clone();
    let pids: Vec<u32> = fs::read_to_string(marker)
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    drop(UnixStream::connect(&socket).unwrap());
    assert_eq!(
        request(
            &socket,
            &EncounterRequest::Status {
                agent_session: session.clone()
            }
        )
        .unwrap()["data"]["native_session_id"],
        native
    );
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true, "native shutdown failed: {ack}");
    assert_eq!(ack["data"]["stopped"].as_array().unwrap().len(), 1);
    finished.recv_timeout(Duration::from_secs(10)).unwrap();
    server.join().unwrap();
    for pid in pids {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "actual provider survived owner shutdown: {pid} {state}"
        );
    }
    let store = aikit_store::encounter::EncounterStore::open(&home).unwrap();
    let events = serde_json::to_value(store.events(&session, 0, 128).unwrap()).unwrap();
    assert!(events["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["event"]["kind"] == "owner-shutdown-completed"));
    assert!(SessionSpaceApplicationStore::new(home)
        .load(&SessionSpaceRef::parse("session-space/shutdown-acceptance").unwrap())
        .unwrap()
        .agent_sessions
        .contains_key(&session));
}

#[test]
fn fragmented_request_and_large_response_cross_real_unix_socket_intact() {
    use std::io::{BufRead, BufReader, Write};
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let socket = temp.path().join("ipc/owner.sock");
    let (_, session) = attach(&home);
    let (server, finished) = launch(home, &socket);
    let pid = ready(&socket);
    let text = "source bytes ∆\n\"retained\" ".repeat(16_384);
    assert!(text.len() > 256 * 1024);
    let mut bytes = serde_json::to_vec(&EncounterRequest::Draft {
        agent_session: session,
        basis: 0,
        text: text.clone(),
    })
    .unwrap();
    bytes.push(b'\n');
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // The owner must wait for the rest of a frame after consuming its prefix.
    stream.write_all(&bytes[..17]).unwrap();
    thread::sleep(Duration::from_millis(100));
    for chunk in bytes[17..].chunks(4093) {
        stream.write_all(chunk).unwrap();
    }
    // Apply backpressure: the full response cannot fit the immediate socket buffer.
    thread::sleep(Duration::from_millis(100));
    let mut response = Vec::new();
    BufReader::new(stream)
        .read_until(b'\n', &mut response)
        .unwrap();
    assert!(response.len() > 256 * 1024);
    assert_eq!(response.last(), Some(&b'\n'));
    let value: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["text"], text);
    assert_eq!(value["data"]["revision"], 1);
    assert_eq!(
        request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap()["ok"],
        true
    );
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
}
