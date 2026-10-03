//! Actual native owner admission, SQLite and operating-system regressions.
//! No provider answer, native Session ID or successful model body is simulated.
use super::*;
use aikit_core::session_space_application::{SessionSpaceAgentAttachmentIntent, SessionSpaceMutation};
use std::{process::Command, sync::mpsc, thread};

// Retain the owned journal and OS evidence until the same native work has
// actually retired. Dropping a failed test or a JoinHandle is not that proof.
struct NativeIdleFixture(Option<tempfile::TempDir>);
impl NativeIdleFixture {
    fn path(&self) -> &Path { self.0.as_ref().unwrap().path() }
    fn finish(mut self) {
        let retained = self.0.take().unwrap().keep();
        std::fs::remove_dir_all(&retained)
            .unwrap_or_else(|failure| panic!("Owned retired fixture cleanup failed at {}: {failure:?}", retained.display()));
    }
}
impl Drop for NativeIdleFixture {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            eprintln!("Retained native owner work/failure material: {}", directory.keep().display());
        }
    }
}

fn fixture() -> NativeIdleFixture {
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let scratch = product.join("ProjectCentral/now/tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    NativeIdleFixture(Some(tempfile::Builder::new().prefix("native-owner-idle-").tempdir_in(scratch).unwrap()))
}

fn attach(home: &AikitHome) -> (SessionSpaceRef, ResourceRef) {
    let owner = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/native-idle-acceptance").unwrap();
    let session = ResourceRef::parse("agent-session/native-idle-acceptance").unwrap();
    let create = owner.stage(None, SessionSpaceMutation::Create { id: space.clone(), label: None }).unwrap();
    owner.apply(&create).unwrap();
    let attachment = owner.stage(Some(&space), SessionSpaceMutation::AttachAgentSession {
        attachment: SessionSpaceAgentAttachmentIntent {
            agent_session: session.clone(), purpose: Some("Real native owner lifecycle test".into()),
            provenance: vec!["owned native test material".into()],
        },
    }).unwrap();
    owner.apply(&attachment).unwrap();
    (space, session)
}

fn idle(service: &EncounterService) -> Result<Value> {
    service.apply(EncounterRequest::ShutdownIdle { expected_pid: std::process::id() })
}

#[test]
fn actual_prejournal_opening_refuses_idle_without_cancelling_the_open() {
    let directory = fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let (space, session) = attach(&home);
    let marker = directory.path().join("actual-provider-pid");
    EncounterService::configure(&home, EncounterProvider {
        protocol: EncounterProtocol::Acp, id: "actual-unresponsive-idle".into(),
        label: "Actual non-protocol startup".into(),
        argv: vec!["python3".into(), "-u".into(), "-c".into(),
            "import os,pathlib,sys,time; p=pathlib.Path(sys.argv[1]); q=p.with_suffix('.stage'); q.write_text(str(os.getpid())); os.replace(q,p); sys.stdin.readline(); time.sleep(60)".into(),
            marker.display().to_string()],
        argv_fallback: vec![], env: BTreeMap::new(), cwd: None, from_profile: None,
        required_context: None, model_policy: None, body_ref: None, body_revision: None, now_context: None,
    }).unwrap();
    let service = Arc::new(EncounterService::new(home).unwrap());
    let (reached, checkpoint) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    *service.native_validation_test_barrier.lock().unwrap() = Some(Arc::new(NativePublicationTestCheckpoint {
        reached, proceed: Mutex::new(proceed),
    }));
    let starting = service.clone();
    let worker_session = session.clone();
    let cwd = directory.path().to_owned();
    let (done, result) = mpsc::channel();
    let worker = thread::spawn(move || {
        // Identical lifecycle admission to public Open; only its actual native
        // deadline is shorter. The checkpoint precedes durable reservation.
        let _lease = starting.lifecycle.read().unwrap();
        done.send(starting.open_native_before(NativeOpenRequest {
            space, agent_session: worker_session, provider: "actual-unresponsive-idle".into(),
            cwd, reconnect: false, model_target: None,
        }, Instant::now() + Duration::from_secs(3))).unwrap();
    });
    checkpoint.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(service.openings.lock().unwrap().len(), 1);
    assert!(service.store.native_open_recovery(&session).unwrap().is_none());
    assert_eq!(idle(&service).unwrap_err().code(), "encounter.owner_busy");
    assert_eq!(service.apply(EncounterRequest::OwnerOccupancy { expected_pid: std::process::id() }).unwrap_err().code(), "encounter.owner_busy");
    assert!(!service.shutdown_requested.load(std::sync::atomic::Ordering::SeqCst));
    assert!(matches!(*service.lifecycle.read().unwrap(), Lifecycle::Running));
    release.send(()).unwrap();
    let received = result.recv_timeout(Duration::from_secs(8));
    // Join before interpreting an unexpected native answer; on timeout the
    // real fixture remains retained rather than detaching and deleting it.
    let joined = if received.is_ok() || worker.is_finished() { Some(worker.join()) } else { None };
    let native_result = received.unwrap();
    joined.expect("actual native opening thread retirement was not observed").unwrap();
    let failure = native_result.unwrap_err();
    assert_eq!(failure.code(), "agent_session_host.control_timeout");
    assert!(service.openings.lock().unwrap().is_empty());
    assert!(service.store.native_open_recovery(&session).unwrap().is_none());
    assert!(service.store.last_native_binding(&session).unwrap().is_none());
    let pid = std::fs::read_to_string(marker).unwrap();
    let mut command = Command::new("ps");
    command.args(["-p", pid.trim(), "-o", "stat="]);
    let observed = aikit_adapters::runner::SystemRunner::new().with_timeout(Duration::from_secs(2))
        .with_output_limit_bytes(4096).with_strict_utf8().capture_command(&mut command).unwrap();
    assert!(observed.stdout.trim().is_empty(), "the actual owned direct child was not reaped: {observed:?}");
    assert_eq!(idle(&service).unwrap()["idle_only"], true);
    drop(service);
    directory.finish();
}

#[test]
fn actual_background_worker_uses_the_same_idle_shutdown_fence() {
    let directory = fixture();
    let service = Arc::new(EncounterService::new(AikitHome::at(directory.path().join("owner"))).unwrap());
    let (reached, checkpoint) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    *service.native_worker_test_barrier.lock().unwrap() = Some(Arc::new(NativePublicationTestCheckpoint {
        reached, proceed: Mutex::new(proceed),
    }));
    agency::conversation::spawn_worker(&service);
    checkpoint.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(idle(&service).unwrap_err().code(), "encounter.owner_busy");
    assert!(!service.shutdown_requested.load(std::sync::atomic::Ordering::SeqCst));
    *service.native_worker_test_barrier.lock().unwrap() = None;
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let receipt = loop {
        match idle(&service) {
            Ok(receipt) => break receipt,
            Err(failure) => assert_eq!(failure.code(), "encounter.owner_busy"),
        }
        assert!(Instant::now() < deadline, "actual sweep did not release native admission");
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(receipt["idle_only"], true);
    assert_eq!(receipt["occupancy"]["complete"], true);
    // Dropping the application's strong reference must release this same real
    // worker after closure. No second worker/process authority is introduced.
    let weak = Arc::downgrade(&service);
    drop(service);
    let deadline = Instant::now() + Duration::from_secs(3);
    while weak.upgrade().is_some() {
        assert!(Instant::now() < deadline, "closed native worker retained the owner");
        thread::sleep(Duration::from_millis(10));
    }
    directory.finish();
}

#[test]
fn actual_queued_delivery_history_is_disclosed_and_retained_without_replay() {
    let directory = fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let service = EncounterService::new(home.clone()).unwrap();
    let session = ResourceRef::parse("agent-session/queued-native-idle").unwrap();
    let delivery = ResourceRef::parse("delivery/queued-native-idle").unwrap();
    let sender = ResourceRef::parse("agent/native-idle-test").unwrap();
    let requested = json!({"text":"Actual durable queue input; no transport sent"});
    service.store.queue_delivery(&session, &delivery, &sender, &requested).unwrap();
    let before = serde_json::to_value(service.store.events(&session, 0, 128).unwrap()).unwrap();
    let census = service.apply(EncounterRequest::OwnerOccupancy { expected_pid: std::process::id() }).unwrap();
    assert_eq!(census["native_idle"], true);
    assert_eq!(census["durable_work"]["nonterminal_delivery_rows"], 1);
    let receipt = idle(&service).unwrap();
    assert_eq!(receipt["durable_work_retained"], true);
    assert_eq!(receipt["native_session_continuity_inferred"], false);
    drop(service);
    let fresh = EncounterService::new(home).unwrap();
    assert_eq!(serde_json::to_value(fresh.store.events(&session, 0, 128).unwrap()).unwrap(), before);
    let retained = fresh.store.queued_deliveries(&session).unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].delivery_ref, delivery);
    assert_eq!(retained[0].request, requested);
    assert_eq!(retained[0].phase, "queued");
    assert!(fresh.store.last_native_binding(&session).unwrap().is_none());
    drop(fresh);
    directory.finish();
}

#[test]
fn actual_sqlite_census_failure_is_not_empty_idleness_or_shutdown() {
    let directory = fixture();
    let home = AikitHome::at(directory.path().join("owner"));
    let service = EncounterService::new(home.clone()).unwrap();
    let mut command = Command::new("python3");
    command.args(["-c", "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('DROP TABLE encounter_events'); c.commit(); c.close()"])
        .arg(home.state().join("encounters.sqlite3"));
    let result = aikit_adapters::runner::SystemRunner::new().with_timeout(Duration::from_secs(3))
        .with_output_limit_bytes(4096).with_strict_utf8().capture_command(&mut command).unwrap();
    assert_eq!(result.status, 0, "actual owned SQLite prerequisite failed: {result:?}");
    let oracle = service.store.native_open_recoveries().unwrap_err();
    assert_eq!(oracle.code(), "encounter.storage");
    let refused = idle(&service).unwrap_err();
    assert_eq!(refused.code(), oracle.code());
    assert_eq!(refused.message(), oracle.message());
    assert_eq!(refused.details(), oracle.details());
    assert!(!service.shutdown_requested.load(std::sync::atomic::Ordering::SeqCst));
    assert!(matches!(*service.lifecycle.read().unwrap(), Lifecycle::Running));
    drop(service);
    directory.finish();
}
