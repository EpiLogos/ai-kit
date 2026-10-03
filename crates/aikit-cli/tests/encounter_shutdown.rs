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
            body_ref: None,
            body_revision: None,
            from_profile: None,
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            required_context: None,
            now_context: None,
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

struct IdleFixture(Option<tempfile::TempDir>);
impl IdleFixture {
    fn path(&self) -> &Path { self.0.as_ref().unwrap().path() }
    fn finish(mut self) {
        let retained = self.0.take().unwrap().keep();
        fs::remove_dir_all(&retained)
            .unwrap_or_else(|failure| panic!("Owned retired fixture cleanup failed at {}: {failure:?}", retained.display()));
    }
}
impl Drop for IdleFixture {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            eprintln!("Retained actual native owner work/failure material: {}", directory.keep().display());
        }
    }
}
fn idle_fixture() -> IdleFixture {
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let scratch = product.join("ProjectCentral/now/tmp");
    fs::create_dir_all(&scratch).unwrap();
    IdleFixture(Some(tempfile::Builder::new().prefix("native-idle-ipc-").tempdir_in(scratch).unwrap()))
}

// This guard closes only its newly-created, exclusively owned test instance.
// It is not a migration mechanism for an unknown pre-existing owner.
struct OwnedServer {
    server: Option<thread::JoinHandle<()>>,
    finished: mpsc::Receiver<()>,
    socket: std::path::PathBuf,
    pid: u32,
}
impl OwnedServer {
    fn finish(mut self) {
        self.finished.recv_timeout(Duration::from_secs(10)).unwrap();
        self.server.take().unwrap().join().unwrap();
    }
}
impl Drop for OwnedServer {
    fn drop(&mut self) {
        if self.server.is_none() { return; }
        let response = request(&self.socket, &EncounterRequest::Shutdown { expected_pid: self.pid });
        if response.as_ref().is_ok_and(|value| value["ok"] == true)
            && self.finished.recv_timeout(Duration::from_secs(10)).is_ok()
        {
            if let Some(server) = self.server.take() { let _ = server.join(); }
        } else {
            eprintln!("Actual owned test-instance cleanup is unresolved: {response:?}");
        }
    }
}
fn idle_server(home: AikitHome, socket: &Path) -> OwnedServer {
    let (server, finished) = launch(home, socket);
    let mut owner = OwnedServer {
        server: Some(server), finished, socket: socket.to_owned(), pid: std::process::id(),
    };
    owner.pid = ready(socket);
    owner
}

fn occupancy(socket: &Path, pid: u32) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observed = request(socket, &EncounterRequest::OwnerOccupancy { expected_pid: pid }).unwrap();
        if observed["ok"] == true {
            assert_eq!(observed["data"]["schema"], "aikit.encounter-owner-occupancy/v1");
            assert_eq!(observed["data"]["complete"], true);
            return observed["data"].clone();
        }
        assert_eq!(observed["error"]["code"], "encounter.owner_busy", "{observed}");
        assert!(Instant::now() < deadline, "current native owner admission remained busy");
        thread::sleep(Duration::from_millis(10));
    }
}

fn idle_request(socket: &Path, pid: u32) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observed = request(socket, &EncounterRequest::ShutdownIdle { expected_pid: pid }).unwrap();
        if observed["error"]["code"] != "encounter.owner_busy" {
            return observed;
        }
        // Each actual busy receipt proves this attempt made no closure. These
        // bounded test attempts are not an automatic production resend policy.
        assert!(Instant::now() < deadline, "current native owner admission remained busy");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn actual_idle_ipc_ack_releases_the_same_owner_socket_and_lock() {
    let directory = idle_fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let socket = directory.path().join("ipc/owner.sock");
    let owner = idle_server(home.clone(), &socket);
    let pid = owner.pid;
    let wrong = request(&socket, &EncounterRequest::ShutdownIdle { expected_pid: pid.wrapping_add(1) }).unwrap();
    assert_eq!(wrong["ok"], false);
    assert_eq!(wrong["error"]["code"], "encounter.owner_changed");
    assert!(wrong["error"].get("details").is_none());
    let wrong_full = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid.wrapping_add(1) }).unwrap();
    assert_eq!(wrong_full["ok"], false);
    assert_eq!(wrong_full["error"]["code"], "encounter.owner_changed");
    assert!(wrong_full["error"].get("details").is_none(), "existing error projection must remain code/message: {wrong_full}");
    let census = occupancy(&socket, pid);
    assert_eq!(census["native_idle"], true);
    assert_eq!(census["residents"], serde_json::json!([]));
    assert_eq!(census["openings"], serde_json::json!([]));
    assert_eq!(census["unresolved_native_startups"], serde_json::json!([]));
    let ack = idle_request(&socket, pid);
    assert_eq!(ack["ok"], true, "{ack}");
    assert_eq!(ack["data"]["idle_only"], true);
    assert_eq!(ack["data"]["stopped"], serde_json::json!([]));
    assert_eq!(ack["data"]["native_session_continuity_inferred"], false);
    owner.finish();
    assert!(!socket.exists());
    let next = idle_server(home, &socket);
    let next_pid = next.pid;
    assert_eq!(idle_request(&socket, next_pid)["ok"], true);
    next.finish();
    directory.finish();
}

struct OwnedCli(std::process::Child, std::path::PathBuf, Option<std::process::ExitStatus>);
struct CliRetirement {
    status: Option<std::process::ExitStatus>,
    errors: Vec<(&'static str, std::io::Error)>,
    kill_attempted: bool,
    timed_out: bool,
}
impl OwnedCli {
    fn retirement(&mut self, terminate: bool) -> CliRetirement {
        let mut actual = CliRetirement { status: self.2, errors: Vec::new(), kill_attempted: false, timed_out: false };
        if actual.status.is_some() { return actual; }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.0.try_wait() {
                Ok(Some(status)) => { self.2 = Some(status); actual.status = Some(status); return actual; }
                Ok(None) if terminate && !actual.kill_attempted => {
                    // Only a same-Child current running observation admits a
                    // kill attempt. A wait error supplies no numeric-PID grant.
                    actual.kill_attempted = true;
                    if let Err(failure) = self.0.kill() { actual.errors.push(("kill", failure)); }
                }
                Ok(None) => {}
                Err(failure) => actual.errors.push(("try_wait", failure)),
            }
            if Instant::now() >= deadline { actual.timed_out = true; return actual; }
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn record(&self, phase: &str, actual: &CliRetirement) -> std::io::Result<()> {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let value = serde_json::json!({
            "owned_child_pid": self.0.id(), "phase": phase,
            "same_child_status_observed": actual.status.is_some(),
            "status": actual.status.map(|status| status.to_string()),
            "exit_code": actual.status.and_then(|status| status.code()),
            "kill_attempted": actual.kill_attempted, "deadline_expired": actual.timed_out,
            "errors": actual.errors.iter().map(|(operation, failure)| serde_json::json!({
                "operation": operation, "kind": format!("{:?}", failure.kind()),
                "raw_os_error": failure.raw_os_error(), "message": failure.to_string(),
            })).collect::<Vec<_>>(),
            "standing": "same owned Child observations; errors are not retirement; unresolved fixture retained"
        });
        let bytes = serde_json::to_vec_pretty(&value).map_err(std::io::Error::other)?;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
            .open(self.1.join(format!("owner-retirement-{phase}.json")))?;
        file.write_all(&bytes)?;
        file.sync_all()
    }
    fn finish_retirement(&mut self, terminate: bool, phase: &str) -> std::process::ExitStatus {
        let actual = self.retirement(terminate);
        let recorded = self.record(phase, &actual);
        assert!(recorded.is_ok(),
            "Actual owned CLI cleanup evidence failed at {}: {recorded:?}; actual errors {:?}; observed status {:?}; kill attempted {}; deadline expired {}",
            self.1.display(), actual.errors, actual.status, actual.kill_attempted, actual.timed_out);
        assert!(actual.errors.is_empty() && !actual.timed_out,
            "Actual owned CLI cleanup failure retained at {}: {:?}; observed status {:?}", self.1.display(), actual.errors, actual.status);
        actual.status.expect("same owned CLI retirement remains unconfirmed; fixture retained")
    }
    fn stop(&mut self) -> std::process::ExitStatus { self.finish_retirement(true, "stop") }
    fn wait_for_exit(&mut self) -> std::process::ExitStatus { self.finish_retirement(false, "wait") }
}
impl Drop for OwnedCli {
    fn drop(&mut self) {
        if self.2.is_some() { return; }
        let actual = self.retirement(true);
        let recorded = self.record("drop", &actual);
        if actual.status.is_none() || !actual.errors.is_empty() || actual.timed_out || recorded.is_err() {
            let failure = format!("Owned CLI cleanup unconfirmed/failed; retain {}: actual status {:?}, kill {}, deadline {}, IO {:?}, evidence {:?}",
                self.1.display(), actual.status, actual.kill_attempted, actual.timed_out, actual.errors, recorded);
            if thread::panicking() { eprintln!("{failure}"); } else { panic!("{failure}"); }
        }
    }
}

fn cli_owner(binary: &Path, home: &AikitHome, directory: &Path, socket: &Path) -> OwnedCli {
    use std::os::unix::fs::OpenOptionsExt;
    let stdout = fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
        .open(directory.join("owner-stdout.raw")).unwrap();
    let stderr = fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
        .open(directory.join("owner-stderr.raw")).unwrap();
    OwnedCli(std::process::Command::new(binary).arg("-C").arg(directory)
        .args(["session-space", "encounter-serve", "--socket"]).arg(socket)
        .env("AIKIT_HOME", home.root()).stdin(std::process::Stdio::null())
        .stdout(stdout).stderr(stderr).spawn().unwrap(), directory.to_owned(), None)
}

#[test]
fn actual_owner_interruption_retains_unresolved_startup_and_refuses_idle() {
    use aikit_adapters::runner::SystemRunner;
    let directory = idle_fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let (space, session) = attach(&home);
    let marker = directory.path().join("provider-pid");
    let exited = directory.path().join("provider-exited");
    let program = "import os,pathlib,sys; p=pathlib.Path(sys.argv[1]); q=p.with_suffix('.stage'); q.write_text(str(os.getpid())); os.replace(q,p); sys.stdin.buffer.read(); p=pathlib.Path(sys.argv[2]); q=p.with_suffix('.stage'); q.write_text('actual-stdin-eof'); os.replace(q,p)";
    EncounterService::configure(&home, EncounterProvider {
        model_policy: None, protocol: Default::default(), id: "actual-interrupted-idle".into(),
        label: "Actual provider process without protocol response".into(),
        argv: vec!["python3".into(), "-u".into(), "-c".into(), program.into(),
            marker.display().to_string(), exited.display().to_string()],
        body_ref: None, body_revision: None, from_profile: None, argv_fallback: vec![],
        env: Default::default(), cwd: None, required_context: None, now_context: None,
    }).unwrap();
    let socket = directory.path().join("ipc/owner.sock");
    let mut owner = cli_owner(Path::new(env!("CARGO_BIN_EXE_aikit")), &home, directory.path(), &socket);
    let pid = ready(&socket);
    assert_eq!(pid, owner.0.id());
    let open_socket = socket.clone();
    let open_cwd = directory.path().to_owned();
    let open_session = session.clone();
    let (done, result) = mpsc::channel();
    let caller = thread::spawn(move || {
        done.send(request(&open_socket, &EncounterRequest::Open {
            space, agent_session: open_session, provider: "actual-interrupted-idle".into(), cwd: open_cwd,
        })).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the actual native startup did not invoke its owned process");
        thread::sleep(Duration::from_millis(10));
    }
    let journal = aikit_store::encounter::EncounterStore::open(&home).unwrap();
    let reservation = journal.native_open_recovery(&session).unwrap().unwrap();
    assert_eq!(reservation["state"], "RecoveryRequired");
    let generation = reservation["opening"]["connection_generation"].as_str().unwrap().to_owned();
    assert_eq!(reservation["opening"]["owner_pid"], pid);
    assert!(journal.last_native_binding(&session).unwrap().is_none());
    let status = owner.stop();
    assert!(!status.success());
    assert!(result.recv_timeout(Duration::from_secs(5)).unwrap().is_err());
    caller.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !exited.exists() {
        assert!(Instant::now() < deadline, "the actual inherited provider stdin did not reach EOF");
        thread::sleep(Duration::from_millis(10));
    }
    let provider_pid = fs::read_to_string(marker).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut ps = std::process::Command::new("ps");
        ps.args(["-o", "stat=", "-p", provider_pid.trim()]);
        let actual = SystemRunner::new().with_timeout(Duration::from_secs(2))
            .with_output_limit_bytes(4096).with_strict_utf8().capture_command(&mut ps).unwrap();
        if actual.stdout.trim().is_empty() || actual.stdout.trim().starts_with('Z') { break; }
        assert!(Instant::now() < deadline, "the actual provider remained runnable: {actual:?}");
        thread::sleep(Duration::from_millis(10));
    }
    // No manufactured phase: the actual startup owner died after reservation
    // and process invocation, before it could record a generation terminal.
    let before = serde_json::to_value(journal.events(&session, 0, 128).unwrap()).unwrap();
    let fresh = EncounterService::new(home).unwrap();
    let census = fresh.apply(EncounterRequest::OwnerOccupancy { expected_pid: std::process::id() }).unwrap();
    assert_eq!(census["native_idle"], false);
    assert_eq!(census["residents"], serde_json::json!([]));
    assert_eq!(census["unresolved_native_startups"][0]["agent_session"], serde_json::json!(session));
    assert_eq!(census["unresolved_native_startups"][0]["connection_generation"], generation);
    let failure = fresh.apply(EncounterRequest::ShutdownIdle { expected_pid: std::process::id() }).unwrap_err();
    assert_eq!(failure.code(), "encounter.owner_not_idle");
    assert_eq!(serde_json::to_value(journal.events(&session, 0, 128).unwrap()).unwrap(), before);
    assert!(fresh.apply(EncounterRequest::Health).is_ok());
    drop(fresh);
    drop(journal);
    directory.finish();
}

#[test]
#[ignore = "requires actual installed AIKIT_SHUTDOWN_NATIVE_ARGV; no prompt or inference"]
fn actual_resident_and_detached_resident_refuse_idle_without_stopping_provider() {
    let actual: Vec<String> = serde_json::from_str(&std::env::var("AIKIT_SHUTDOWN_NATIVE_ARGV").expect("actual native ACP argv required")).unwrap();
    assert!(!actual.is_empty());
    let directory = idle_fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let (space, session) = attach(&home);
    EncounterService::configure(&home, EncounterProvider {
        model_policy: None, protocol: Default::default(), id: "actual-resident-idle".into(),
        label: "Actual installed native resident".into(), argv: actual,
        body_ref: None, body_revision: None, from_profile: None, argv_fallback: vec![],
        env: Default::default(), cwd: None, required_context: None, now_context: None,
    }).unwrap();
    let socket = directory.path().join("ipc/owner.sock");
    let server = idle_server(home.clone(), &socket);
    let pid = server.pid;
    let opened = request(&socket, &EncounterRequest::Open {
        space: space.clone(), agent_session: session.clone(), provider: "actual-resident-idle".into(),
        cwd: directory.path().to_owned(),
    }).unwrap();
    assert_eq!(opened["ok"], true, "actual native provider startup failed: {opened}");
    let native = opened["data"]["native_session_id"].as_str().unwrap().to_owned();
    let attached_census = occupancy(&socket, pid);
    assert_eq!(attached_census["native_idle"], false);
    assert_eq!(attached_census["residents"][0]["native_session_id"], native);
    let refused = idle_request(&socket, pid);
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["code"], "encounter.owner_not_idle");
    let refused_census: serde_json::Value = serde_json::from_str(refused["error"]["details"]["occupancy"].as_str().unwrap()).unwrap();
    assert_eq!(refused_census["schema"], "aikit.encounter-owner-occupancy/v1");
    assert_eq!(refused_census["pid"], pid);
    assert_eq!(refused_census["residents"][0]["native_session_id"], native);
    assert_eq!(refused["error"]["details"].as_object().unwrap().len(), 1);
    let owner = SessionSpaceApplicationStore::new(home.clone());
    let detach = owner.stage(Some(&space), SessionSpaceMutation::DetachAgentSession { agent_session: session.clone() }).unwrap();
    owner.apply(&detach).unwrap();
    assert!(!owner.load(&space).unwrap().agent_sessions.contains_key(&session));
    let detached = occupancy(&socket, pid);
    assert_eq!(detached["native_idle"], false);
    assert_eq!(detached["session_space_attachment_required"], false);
    assert_eq!(detached["residents"].as_array().unwrap().len(), 1);
    assert_eq!(detached["residents"][0]["agent_session"], serde_json::json!(session));
    assert_eq!(detached["residents"][0]["native_session_id"], native);
    let refused = idle_request(&socket, pid);
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["code"], "encounter.owner_not_idle");
    let reattach = owner.stage(Some(&space), SessionSpaceMutation::AttachAgentSession {
        attachment: SessionSpaceAgentAttachmentIntent { agent_session: session.clone(), purpose: None, provenance: vec![] },
    }).unwrap();
    owner.apply(&reattach).unwrap();
    let status = request(&socket, &EncounterRequest::Status { agent_session: session.clone() }).unwrap();
    assert_eq!(status["ok"], true);
    assert_eq!(status["data"]["native_session_id"], native);
    let journal = aikit_store::encounter::EncounterStore::open(&home).unwrap();
    assert!(!journal.events(&session, 0, 128).unwrap().events.iter()
        .any(|event| event.event["kind"] == "owner-shutdown-requested"));
    // This fixture's separately authorised full shutdown remains legitimate;
    // idle refusal must not alter its original real cleanup/history oracle.
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true, "{ack}");
    assert_eq!(ack["data"]["stopped"].as_array().unwrap().len(), 1);
    server.finish();
    directory.finish();
}

#[test]
#[ignore = "requires actual qualified AIKIT_IDLE_LEGACY_BIN retaining original cdc8 owner Source"]
fn actual_legacy_owner_refuses_new_idle_tag_and_keeps_its_existing_lifecycle() {
    let binary = std::path::PathBuf::from(std::env::var_os("AIKIT_IDLE_LEGACY_BIN").expect("actual legacy CompilerArtifact required"));
    let directory = idle_fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let socket = directory.path().join("ipc/owner.sock");
    let mut owner = cli_owner(&binary, &home, directory.path(), &socket);
    let pid = ready(&socket);
    assert_eq!(pid, owner.0.id());
    let refused = request(&socket, &EncounterRequest::ShutdownIdle { expected_pid: pid }).unwrap();
    assert_eq!(refused["ok"], false, "legacy owner must not adopt a new idle flag: {refused}");
    assert_eq!(refused["error"]["code"], "encounter.runtime");
    assert_eq!(ready(&socket), pid);
    // This newly-created exclusive fixture never admitted a provider. Full
    // closure here is not authority to drain the Original running owner.
    assert_eq!(request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap()["ok"], true);
    assert!(owner.wait_for_exit().success(), "legacy owner did not exit successfully after its legitimate full ACK");
    assert!(!socket.exists());
    directory.finish();
}
