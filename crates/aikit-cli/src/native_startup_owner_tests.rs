//! Proposed private owner regressions. These launch actual operating-system
//! processes which emit no provider response, session binding or model output.
//! The positive native protocol gate remains the installed Pi test elsewhere.
use super::*;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use std::{process::Command, sync::mpsc, thread};

fn attached(home: &AikitHome) -> (SessionSpaceRef, ResourceRef) {
    let owner = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/native-startup-owner-regression").unwrap();
    let session = ResourceRef::parse("agent-session/native-startup-owner-regression").unwrap();
    let create = owner
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: None,
            },
        )
        .unwrap();
    owner.apply(&create).unwrap();
    let attach = owner
        .stage(
            Some(&space),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: session.clone(),
                    purpose: Some("Real native startup failure; no worker/model success".into()),
                    provenance: vec!["isolated owner regression".into()],
                },
            },
        )
        .unwrap();
    owner.apply(&attach).unwrap();
    (space, session)
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
fn unresponsive_owned_startup_is_bounded_visible_and_cleans_its_exact_group() {
    let directory = tempfile::tempdir().unwrap();
    let home = AikitHome::at(directory.path().join("owner"));
    let (space, session) = attached(&home);
    let marker = directory.path().join("owned-pids.json");
    // An actual non-protocol process and descendant hold stdout. The test makes
    // no synthetic provider response, native session or successful worker.
    let program = r#"
import json, os, pathlib, subprocess, sys, time
child = subprocess.Popen(["/bin/sleep", "60"])
pathlib.Path(sys.argv[1]).write_text(json.dumps([os.getpid(), child.pid]))
sys.stdin.readline()
time.sleep(60)
"#;
    EncounterService::configure(
        &home,
        EncounterProvider {
            protocol: EncounterProtocol::Acp,
            id: "owned-unresponsive-startup".into(),
            label: "Actual startup process failure".into(),
            argv: vec![
                "python3".into(),
                "-u".into(),
                "-c".into(),
                program.into(),
                marker.display().to_string(),
            ],
            argv_fallback: vec![],
            env: BTreeMap::new(),
            cwd: None,
            from_profile: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            now_context: None,
        },
    )
    .unwrap();
    let service = Arc::new(EncounterService::new(home.clone()).unwrap());
    let starting = service.clone();
    let worker_space = space.clone();
    let worker_session = session.clone();
    let cwd = directory.path().to_owned();
    let worker_cwd = cwd.clone();
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        // Same owner lifecycle read lease used by public Open. Only the private
        // test deadline is shorter; no product env/config bypass is introduced.
        let _lease = starting.lifecycle.read().unwrap();
        sender
            .send(starting.open_native_before(
                NativeOpenRequest {
                    space: worker_space,
                    agent_session: worker_session,
                    provider: "owned-unresponsive-startup".into(),
                    cwd: worker_cwd,
                    reconnect: false,
                    model_target: None,
                },
                Instant::now() + Duration::from_secs(3),
            ))
            .unwrap();
    });
    let ready_by = Instant::now() + Duration::from_secs(5);
    let pids: Vec<u32> = loop {
        if let Ok(bytes) = std::fs::read(&marker) {
            if let Ok(pids) = serde_json::from_slice(&bytes) {
                break pids;
            }
        }
        assert!(
            Instant::now() < ready_by,
            "actual child did not reach startup"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(pids.len(), 2);
    let observing = Instant::now();
    assert!(service.apply(EncounterRequest::Health).is_ok());
    let view = service
        .apply(EncounterRequest::View {
            agent_session: session.clone(),
            before: None,
        })
        .unwrap();
    assert_eq!(view["connection"]["state"], "Opening");
    assert_eq!(view["connection"]["resident"], false);
    assert!(!view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["ref"] == "aikit.encounter.open")
        .unwrap()["enabled"]
        .as_bool()
        .unwrap());
    assert!(service
        .apply(EncounterRequest::Read {
            agent_session: session.clone(),
            after: 0,
            limit: 32,
        })
        .is_ok());
    assert_eq!(
        service
            .apply(EncounterRequest::Open {
                space,
                agent_session: session.clone(),
                provider: "owned-unresponsive-startup".into(),
                cwd,
            })
            .unwrap_err()
            .code(),
        "encounter.native_open_in_progress"
    );
    assert!(
        observing.elapsed() < Duration::from_secs(1),
        "queries waited on provider protocol"
    );
    let failure = receiver
        .recv_timeout(Duration::from_secs(8))
        .unwrap()
        .unwrap_err();
    worker.join().unwrap();
    assert_eq!(failure.code(), "agent_session_host.control_timeout");
    assert!(service
        .store
        .last_native_binding(&session)
        .unwrap()
        .is_none());
    assert!(service
        .store
        .native_open_recovery(&session)
        .unwrap()
        .is_none());
    let page = service.store.events(&session, 0, 128).unwrap();
    let reservation = page
        .events
        .iter()
        .find(|row| row.event["kind"] == "native-open-reserved")
        .unwrap();
    let refusal = page
        .events
        .iter()
        .find(|row| row.event["kind"] == "native-open-refused")
        .unwrap();
    assert_eq!(
        refusal.event["connection_generation"],
        reservation.event["connection_generation"]
    );
    assert_eq!(refusal.event["cleanup_confirmed"], true);
    assert_eq!(refusal.event["binding_recorded"], false);
    assert_eq!(refusal.event["turn_replayed"], false);
    assert!(
        process_state(pids[0]).is_none(),
        "owned leader was not reaped"
    );
    assert!(
        process_state(pids[1])
            .as_deref()
            .is_none_or(|state| state.starts_with('Z')),
        "owned descendant remains runnable"
    );
    // A fresh owner reads the exact retained failure rather than creating a
    // native binding or claiming worker success from a completed cleanup.
    drop(service);
    let fresh = EncounterService::new(home).unwrap();
    assert!(fresh.store.last_native_binding(&session).unwrap().is_none());
    assert!(fresh
        .store
        .native_open_recovery(&session)
        .unwrap()
        .is_none());
}

#[test]
fn reconciliation_discovery_and_generic_request_refuse_missing_native_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let home = AikitHome::at(directory.path().join("owner"));
    let (_, session) = attached(&home);
    let service = EncounterService::new(home).unwrap();
    // Real typed generic encounter request dispatch, not a parallel command.
    let value = json!({"action":"reconcile-native-open", "agent_session":session,
        "expected_generation":"exact-retained-generation-required",
        "cleanup_confirmed":true, "evidence_ref":"evidence/native-owner-cleanup"});
    let request: EncounterRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&request).unwrap(), value);
    assert_eq!(
        service.apply(request).unwrap_err().code(),
        "encounter.native_open_recovery_absent"
    );
    let view = service
        .apply(EncounterRequest::View {
            agent_session: session,
            before: None,
        })
        .unwrap();
    let action = view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["ref"] == "aikit.encounter.reconcile-native-open")
        .unwrap();
    assert_eq!(action["enabled"], false);
    assert!(action["reason"].as_str().unwrap().contains("attestation"));
}

/// Retained owner state for opt-in actual Pi gates. Required diagnostics and
/// native cleanup are explicitly finalized before the caller can pass. Drop is
/// only best-effort on unwind, and never replaces the original panic/refusal.
#[derive(Default)]
struct NativePiEvidenceJournal {
    writes: Mutex<()>,
    failures: Mutex<Vec<Value>>,
}
struct RetainedPiEvidence {
    directory: PathBuf,
    started: Instant,
    service: Option<Arc<EncounterService>>,
    sessions: Vec<ResourceRef>,
    in_flight: Arc<std::sync::atomic::AtomicBool>,
    diagnostics: Arc<NativePiEvidenceJournal>,
    passed: bool,
}
impl RetainedPiEvidence {
    fn new(name: &str) -> Self {
        let declared = std::env::var_os("OI_NATIVE_PI_TEST_EVIDENCE_DIR")
            .expect("explicit fresh retained native Pi evidence root required");
        Self::at(Path::new(&declared), name)
    }
    fn at(root: &Path, name: &str) -> Self {
        assert!(
            root.is_absolute(),
            "native Pi evidence root must be absolute"
        );
        assert!(
            std::fs::symlink_metadata(root).unwrap().is_dir(),
            "native Pi evidence root must be an existing real directory"
        );
        let directory = root.canonicalize().unwrap().join(name);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .expect("each native Pi test evidence directory must be fresh");
        let evidence = Self {
            directory,
            started: Instant::now(),
            service: None,
            sessions: Vec::new(),
            in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            diagnostics: Arc::new(NativePiEvidenceJournal::default()),
            passed: false,
        };
        evidence.mark(
            "test-entry",
            json!({"native_owner_budget_ms":NATIVE_STARTUP_TIMEOUT.as_millis(),
            "checkpoint_observation_budget_ms":20000,"checkpoint_release_budget_ms":15000,
            "model_prompt":false,"factory_verification":false}),
        );
        evidence
    }
    fn path(&self) -> &Path {
        &self.directory
    }
    fn observe(&mut self, service: &Arc<EncounterService>, sessions: &[ResourceRef]) {
        self.service = Some(service.clone());
        self.sessions = sessions.to_vec();
        self.mark(
            "owner-attached",
            json!({"sessions":sessions,"owner_pid":std::process::id(),
            "owner_state":service.home.state()}),
        );
    }
    fn mark(&self, phase: &str, details: Value) {
        retained_pi_mark(
            &self.directory,
            self.started,
            &self.diagnostics,
            phase,
            details,
        );
    }
    fn snapshot(&self, phase: &str) {
        let Some(service) = &self.service else {
            retained_pi_diagnostic_failure(
                &self.diagnostics,
                phase,
                json!({"code":"owner-not-attached"}),
            );
            return;
        };
        let mut sessions = Vec::new();
        for session in &self.sessions {
            let mut events = Vec::new();
            let mut after = 0;
            let mut error = None;
            let mut truncated = false;
            for _ in 0..8 {
                match service.store.events(session, after, 128) {
                    Ok(page) => {
                        for row in &page.events {
                            let bytes = serde_json::to_vec(&row.event).unwrap();
                            let mut safe = json!({"cursor":row.cursor,"kind":row.event["kind"],
                                "payload_blake3":blake3::hash(&bytes).to_hex().to_string(),"payload_bytes":bytes.len()});
                            for field in [
                                "connection_generation",
                                "native_session_id",
                                "owner_pid",
                                "error_code",
                                "cleanup_confirmed",
                                "binding_recorded",
                                "process_started",
                                "turn_replayed",
                            ] {
                                if let Some(value) = row.event.get(field) {
                                    safe[field] = value.clone();
                                }
                            }
                            if let Some(event) = row.event.get("event").and_then(Value::as_object) {
                                safe["host_event_variant"] =
                                    json!(event.keys().collect::<Vec<_>>());
                            }
                            events.push(safe);
                        }
                        after = page.next_cursor;
                        truncated = page.more;
                        if !page.more {
                            break;
                        }
                    }
                    Err(failure) => {
                        let safe = retained_pi_error(&failure);
                        retained_pi_diagnostic_failure(&self.diagnostics, phase, safe.clone());
                        error = Some(safe);
                        break;
                    }
                }
            }
            if truncated {
                retained_pi_diagnostic_failure(
                    &self.diagnostics,
                    phase,
                    json!({"code":"required-journal-summary-truncated","agent_session":session}),
                );
            }
            sessions.push(json!({"agent_session":session,"events":events,"read_error":error,"truncated":truncated}));
        }
        let value = json!({"schema":"aikit.native-pi-safe-owner-snapshot/v1","phase":phase,
            "sessions":sessions,"owner_state":service.home.state(),"raw_owner_state_retained_locally":true,
            "raw_provider_contents_exported":false,"model_prompt":false,"factory_verification":false});
        let path = self.directory.join(format!("{phase}.json"));
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        if let Err(failure) = std::fs::write(&path, &bytes) {
            retained_pi_diagnostic_failure(
                &self.diagnostics,
                phase,
                json!({"code":"snapshot-persistence","path":path,"io_kind":format!("{:?}",failure.kind())}),
            );
            return;
        }
        match std::fs::read(&path) {
            Ok(retained) if retained == bytes => {}
            Ok(_) => retained_pi_diagnostic_failure(
                &self.diagnostics,
                phase,
                json!({"code":"snapshot-readback-mismatch","path":path}),
            ),
            Err(failure) => retained_pi_diagnostic_failure(
                &self.diagnostics,
                phase,
                json!({"code":"snapshot-readback","path":path,"io_kind":format!("{:?}",failure.kind())}),
            ),
        }
    }
    fn cleanup(&self) {
        let Some(service) = &self.service else {
            retained_pi_diagnostic_failure(
                &self.diagnostics,
                "actual-native-owner-cleanup",
                json!({"code":"owner-not-attached"}),
            );
            return;
        };
        let result = service.apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        });
        let safe = match &result {
            Ok(value) => {
                if value["shutdown"] != true || value["canonical_sessions_retained"] != true {
                    retained_pi_diagnostic_failure(
                        &self.diagnostics,
                        "actual-native-owner-cleanup",
                        json!({"code":"required-native-cleanup-ack-missing"}),
                    );
                }
                json!({"outcome":"ok","shutdown":value["shutdown"],"stopped":value["stopped"],
                    "canonical_sessions_retained":value["canonical_sessions_retained"]})
            }
            Err(failure) => {
                let safe = retained_pi_error(failure);
                retained_pi_diagnostic_failure(
                    &self.diagnostics,
                    "actual-native-owner-cleanup",
                    safe.clone(),
                );
                json!({"outcome":"err","error":safe})
            }
        };
        self.mark("actual-native-owner-cleanup", safe);
    }
    fn validate_timeline(&self) {
        let path = self.directory.join("timeline.jsonl");
        let outcome = (|| -> std::result::Result<(), String> {
            let bytes = std::fs::read(&path).map_err(|failure| format!("{:?}", failure.kind()))?;
            if bytes.len() > 1024 * 1024 {
                return Err("required-timeline-over-limit".into());
            }
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| "required-timeline-invalid-utf8".to_owned())?;
            let mut rows = 0;
            for line in text.lines() {
                let _: Value = serde_json::from_str(line)
                    .map_err(|_| "required-timeline-invalid-json".to_owned())?;
                rows += 1;
            }
            if rows == 0 {
                return Err("required-timeline-empty".into());
            }
            Ok(())
        })();
        if let Err(code) = outcome {
            retained_pi_diagnostic_failure(
                &self.diagnostics,
                "timeline-readback",
                json!({"code":code,"path":path}),
            );
        }
    }
    fn finish_success(&mut self) -> Result<()> {
        if self.in_flight.load(std::sync::atomic::Ordering::SeqCst) {
            retained_pi_diagnostic_failure(
                &self.diagnostics,
                "success-finalization",
                json!({"code":"native-open-still-outstanding"}),
            );
        } else {
            self.snapshot("before-test-cleanup");
            self.cleanup();
            self.snapshot("after-test-cleanup");
        }
        self.mark("required-evidence-finalized",json!({"success_marker":false,
            "standing":"native test result remains uncommitted until all required writes and readback succeed"}));
        self.validate_timeline();
        let failures = self
            .diagnostics
            .failures
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        if !failures.is_empty() {
            return Err(AikitError::new(
                "test.native_pi_required_evidence_missing",
                format!(
                    "Required native test diagnostics or cleanup refused: {}",
                    json!(failures)
                ),
            ));
        }
        // Only actual caller success after required evidence/readback. Drop has
        // no further required writes and cannot turn late persistence refusal green.
        self.passed = true;
        Ok(())
    }
}
fn retained_pi_diagnostic_failure(diagnostics: &NativePiEvidenceJournal, phase: &str, safe: Value) {
    diagnostics
        .failures
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(json!({"phase":phase,"failure":safe}));
}
fn retained_pi_error(failure: &AikitError) -> Value {
    json!({"code":failure.code(),"message_blake3":blake3::hash(failure.message().as_bytes()).to_hex().to_string(),
        "message_bytes":failure.message().len(),"message_and_details_exported":false})
}
fn retained_pi_outcome(outcome: &Result<Value>) -> Value {
    match outcome {
        Ok(value) => {
            json!({"outcome":"ok","native_session_id":value["native_session_id"],"resident":value["resident"],"inference_observed":false})
        }
        Err(failure) => json!({"outcome":"err","error":retained_pi_error(failure)}),
    }
}
fn retained_pi_mark(
    directory: &Path,
    started: Instant,
    diagnostics: &NativePiEvidenceJournal,
    phase: &str,
    details: Value,
) {
    use std::io::Write;
    let _writer = diagnostics
        .writes
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let row = json!({"elapsed_ms":started.elapsed().as_millis(),"phase":phase,"details":details});
    let path = directory.join("timeline.jsonl");
    let outcome = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{}", row)?;
        file.sync_data()
    })();
    if let Err(failure) = outcome {
        retained_pi_diagnostic_failure(
            diagnostics,
            phase,
            json!({"code":"timeline-persistence","path":path,"io_kind":format!("{:?}",failure.kind())}),
        );
    }
}
struct RetainedPiInFlight(Arc<std::sync::atomic::AtomicBool>);
impl Drop for RetainedPiInFlight {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
impl Drop for RetainedPiEvidence {
    fn drop(&mut self) {
        if self.passed {
            return;
        }
        let outstanding = self.in_flight.load(std::sync::atomic::Ordering::SeqCst);
        self.mark("test-return",json!({"test_passed":false,"panicking":std::thread::panicking(),
            "native_open_outstanding":outstanding,"native_quiescence_observed":false,"factory_verification":false}));
        if outstanding {
            self.mark(
                "cleanup-not-admitted",
                json!({"reason":"actual native open still outstanding",
                "owner_state_retained":true,"native_quiescence_observed":false}),
            );
            return;
        }
        // Best effort during unwind only. All required successful-path work
        // belongs to finish_success; retention errors never double-panic here.
        self.snapshot("before-test-cleanup");
        self.cleanup();
        self.snapshot("after-test-cleanup");
    }
}

/// Real admitted native resident and owner control path. No prompt, fake
/// handshake, provider transcript, model output or Factory attempt is created.
#[test]
#[ignore = "requires OI_PI_BIN and allocated OI_NATIVE_PI_TEST_EVIDENCE_DIR; actual native controls, no prompt/inference"]
fn actual_pi_resident_controls_share_a_finite_owner_budget_without_losing_identity() {
    let binary = std::env::var("OI_PI_BIN").expect("actual installed Pi executable required");
    let mut directory = RetainedPiEvidence::new("resident-controls");
    let home = AikitHome::at(directory.path().join("owner"));
    let (space, session) = attached(&home);
    EncounterService::configure(
        &home,
        EncounterProvider {
            protocol: EncounterProtocol::PiRpc,
            id: "native-resident-controls".into(),
            label: "Actual Pi native control acceptance".into(),
            argv: vec![
                binary,
                "--mode".into(),
                "rpc".into(),
                "--no-session".into(),
                "--no-tools".into(),
                "--no-extensions".into(),
            ],
            argv_fallback: vec![],
            env: BTreeMap::new(),
            cwd: None,
            from_profile: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            now_context: None,
        },
    )
    .unwrap();
    let service = Arc::new(EncounterService::new(home).unwrap());
    directory.observe(&service, std::slice::from_ref(&session));
    directory.mark("first-open-call", Value::Null);
    let opened = service
        .apply(EncounterRequest::Open {
            space,
            agent_session: session.clone(),
            provider: "native-resident-controls".into(),
            cwd: directory.path().to_path_buf(),
        })
        .unwrap();
    directory.mark(
        "first-open-returned",
        retained_pi_outcome(&Ok(opened.clone())),
    );
    let native = opened["native_session_id"].clone();
    let read = service
        .apply(EncounterRequest::ModelRead {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(read["native_session_id"], native);
    let modes = service
        .apply(EncounterRequest::ModeRead {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(modes["native_session_id"], native);
    assert_eq!(
        modes["mode_controls"]["mode_selection"], false,
        "Pi does not advertise an ACP permission-mode selector"
    );
    assert_eq!(
        service
            .apply(EncounterRequest::ModeSelect {
                agent_session: session.clone(),
                provider_mode_id: "not-advertised-by-pi".into(),
                expected_native_session_id: native.as_str().map(str::to_owned),
            })
            .unwrap_err()
            .code(),
        "encounter.mode_selection_unavailable"
    );
    if read["model_controls"]["model_selection"] == true {
        let current = read["model_observation"]["current_model_id"]
            .as_str()
            .expect("actual advertised current model")
            .to_owned();
        let selected = service
            .apply(EncounterRequest::ModelSelect {
                agent_session: session.clone(),
                provider_model_id: current.clone(),
                provider_reasoning_effort: None,
                expected_native_session_id: native.as_str().map(str::to_owned),
            })
            .unwrap();
        assert_eq!(selected["native_session_id"], native);
        assert_eq!(selected["model_observation"]["current_model_id"], current);
        // A real accepted model leg followed by an actually unsupported Pi
        // reasoning leg must remain a retained partial effect, not success.
        let refused = service
            .apply(EncounterRequest::ModelSelect {
                agent_session: session.clone(),
                provider_model_id: current,
                provider_reasoning_effort: Some(
                    "deliberately-unadvertised-native-reasoning-value".into(),
                ),
                expected_native_session_id: native.as_str().map(str::to_owned),
            })
            .unwrap_err();
        assert_eq!(
            refused.code(),
            "connection.reasoning_effort_selection_unsupported"
        );
        let mut after = 0;
        let mut partial = None;
        loop {
            let page = service.store.events(&session, after, 256).unwrap();
            for row in &page.events {
                if row.event["kind"] == "native-model-configuration-partial-confirmed" {
                    partial = Some(row.event.clone());
                }
            }
            after = page.next_cursor;
            if !page.more {
                break;
            }
        }
        let partial =
            partial.expect("actual native model effect retained despite reasoning refusal");
        assert_eq!(partial["overall_selection_confirmed"], false);
        assert_eq!(partial["receipt"]["native_session_id"], native);
    } else {
        assert!(read["model_controls"]["reason"].as_str().is_some());
        eprintln!("Actual Pi offered no model selection; verified genuine model/mode reads and unsupported-mode refusal, no selection success claimed");
    }
    // Hold the real admitted resident's operation lease. The owner must refuse
    // by its original budget, rather than begin a control when that lease is
    // eventually released. Other canonical queries remain usable throughout.
    let resident = service.resident(&session).unwrap();
    let held = resident.operations.lock().unwrap();
    let waiting = service.clone();
    let waiting_session = session.clone();
    let (sender, receiver) = mpsc::channel();
    let started = Instant::now();
    let worker = thread::spawn(move || {
        sender
            .send(waiting.apply_with_native_control_deadline(
                EncounterRequest::ModelRead {
                    agent_session: waiting_session,
                },
                Instant::now() + Duration::from_millis(200),
            ))
            .unwrap();
    });
    assert!(service.apply(EncounterRequest::Health).is_ok());
    assert_eq!(
        service
            .apply(EncounterRequest::Status {
                agent_session: session.clone()
            })
            .unwrap()["native_session_id"],
        native
    );
    assert_eq!(
        service
            .apply(EncounterRequest::View {
                agent_session: session.clone(),
                before: None
            })
            .unwrap()["connection"]["native_session_id"],
        native
    );
    assert!(service
        .apply(EncounterRequest::Read {
            agent_session: session.clone(),
            after: 0,
            limit: 32
        })
        .is_ok());
    let bounded = receiver.recv_timeout(Duration::from_secs(2));
    drop(held);
    worker.join().unwrap();
    assert_eq!(
        bounded.unwrap().unwrap_err().code(),
        "encounter.native_control_deadline_elapsed"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(
        service
            .apply(EncounterRequest::Status {
                agent_session: session.clone()
            })
            .unwrap()["state"],
        "Resident",
        "an unadmitted waiting control cannot terminate another operation's native host"
    );
    drop(resident);
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], true);
    assert_eq!(shutdown["stopped"][0]["native_session_id"], native);
    directory.finish_success().unwrap();
}

/// The real SQLite dispatch callback resolves the first actual resident while
/// a second actual Pi startup is trying to journal its binding. The checkpoint
/// only orders those real operations; it invents no native binding or prompt.
#[test]
#[ignore = "requires OI_PI_BIN and allocated OI_NATIVE_PI_TEST_EVIDENCE_DIR; two actual native startups/SQLite rollback, no prompt/inference"]
fn actual_pi_publication_does_not_hold_the_resident_map_while_waiting_for_the_journal() {
    let binary = std::env::var("OI_PI_BIN").expect("actual installed Pi executable required");
    let mut directory = RetainedPiEvidence::new("publication-journal");
    let home = AikitHome::at(directory.path().join("owner"));
    let (space, first) = attached(&home);
    let second = ResourceRef::parse("agent-session/native-publication-second").unwrap();
    let owner = SessionSpaceApplicationStore::new(home.clone());
    let attach = owner
        .stage(
            Some(&space),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: second.clone(),
                    purpose: Some(
                        "Genuine native publication/journal lock regression; no prompt".into(),
                    ),
                    provenance: vec!["actual installed Pi native startup".into()],
                },
            },
        )
        .unwrap();
    owner.apply(&attach).unwrap();
    EncounterService::configure(
        &home,
        EncounterProvider {
            protocol: EncounterProtocol::PiRpc,
            id: "native-publication-lock".into(),
            label: "Actual Pi publication lock acceptance".into(),
            argv: vec![
                binary,
                "--mode".into(),
                "rpc".into(),
                "--no-session".into(),
                "--no-tools".into(),
                "--no-extensions".into(),
            ],
            argv_fallback: vec![],
            env: BTreeMap::new(),
            cwd: None,
            from_profile: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            now_context: None,
        },
    )
    .unwrap();
    let service = Arc::new(EncounterService::new(home).unwrap());
    directory.observe(&service, &[first.clone(), second.clone()]);
    directory.mark("first-open-call", Value::Null);
    let first_open = service
        .apply(EncounterRequest::Open {
            space: space.clone(),
            agent_session: first.clone(),
            provider: "native-publication-lock".into(),
            cwd: directory.path().to_path_buf(),
        })
        .unwrap();
    directory.mark(
        "first-open-returned",
        retained_pi_outcome(&Ok(first_open.clone())),
    );
    let draft = service
        .store
        .set_draft(
            &first,
            0,
            "Retained draft: regression will refuse before native prompt",
        )
        .unwrap();
    let (entered, reached) = mpsc::channel();
    let (proceed, go) = mpsc::channel();
    let checkpoint = Arc::new(NativePublicationTestCheckpoint {
        reached: entered,
        proceed: Mutex::new(go),
    });
    *service.native_publication_test_barrier.lock().unwrap() = Some(checkpoint);
    let starting = service.clone();
    let cwd = directory.path().to_path_buf();
    let second_for_open = second.clone();
    let (sender, receiver) = mpsc::channel();
    let worker_evidence = directory.path().to_path_buf();
    let worker_started = directory.started;
    let worker_diagnostics = directory.diagnostics.clone();
    let in_flight = directory.in_flight.clone();
    in_flight.store(true, std::sync::atomic::Ordering::SeqCst);
    let worker = thread::spawn(move || {
        let _in_flight = RetainedPiInFlight(in_flight);
        retained_pi_mark(
            &worker_evidence,
            worker_started,
            &worker_diagnostics,
            "second-open-call",
            Value::Null,
        );
        let outcome = starting.apply(EncounterRequest::Open {
            space,
            agent_session: second_for_open,
            provider: "native-publication-lock".into(),
            cwd,
        });
        retained_pi_mark(
            &worker_evidence,
            worker_started,
            &worker_diagnostics,
            "second-open-returned",
            retained_pi_outcome(&outcome),
        );
        sender.send(outcome).unwrap();
    });
    // The second provider has genuinely negotiated/opened and passed source
    // checks. It is paused immediately before canonical binding journal I/O.
    let checkpoint_reached = reached.recv_timeout(Duration::from_secs(20));
    directory.mark("checkpoint-observation", json!({"reached":checkpoint_reached.is_ok(),
        "checkpoint_elapsed_ms":checkpoint_reached.as_ref().ok().map(|at|at.duration_since(directory.started).as_millis()),
        "observation_refusal":checkpoint_reached.as_ref().err().map(|error| match error {
            mpsc::RecvTimeoutError::Timeout=>"timeout", mpsc::RecvTimeoutError::Disconnected=>"disconnected"
        })}));
    if checkpoint_reached.is_err() {
        // This is a test-induced cancellation, not native transport EOF. A
        // late actual startup sees the closed private release channel.
        directory.mark(
            "test-checkpoint-release-dropped",
            json!({"native_transport_exit_observed":false}),
        );
        drop(proceed);
        let outcome = receiver.recv_timeout(Duration::from_secs(90));
        let late = reached.try_recv();
        directory.mark("post-cancellation-owner-observation", json!({
            "owner_outcome":outcome.as_ref().ok().map(retained_pi_outcome),
            "owner_wait_refusal":outcome.as_ref().err().map(|error|match error {
                mpsc::RecvTimeoutError::Timeout=>"timeout",mpsc::RecvTimeoutError::Disconnected=>"disconnected"
            }),
            "late_checkpoint_observed":late.is_ok(),
            "late_checkpoint_elapsed_ms":late.as_ref().ok().map(|at|at.duration_since(directory.started).as_millis())
        }));
        if outcome.is_ok() {
            worker.join().unwrap();
        }
        panic!("actual second native startup did not reach publication in 20s; retained safe owner evidence at {}",directory.path().display());
    }
    let begun = Instant::now();
    let refusal = service.store.submit_context(&first, draft.revision, None, |_| {
        directory.mark("actual-sqlite-dispatch-entered",Value::Null);
        proceed.send(()).map_err(error)?;
        // Probe without an unbounded test hang if a regression reintroduces
        // the old map->journal cycle. Then use the real resident() owner path.
        match service.residents.try_lock() {
            Ok(map) => drop(map),
            Err(_) => return Err(AikitError::new("test.native_publication_map_held", "Native publication retained the map while waiting for this real SQLite dispatch transaction")),
        }
        let actual = service.resident(&first)?;
        assert_eq!(actual.host.identity(&first)?.binding.native_session_id, first_open["native_session_id"].as_str().unwrap());
        // Roll back the real transaction before dispatch. No provider prompt,
        // user-message success or synthetic model output is manufactured.
        Err(AikitError::new("test.native_submission_not_dispatched", "Real resident resolved; intentional no-prompt rollback"))
    }).unwrap_err();
    directory.mark(
        "actual-sqlite-dispatch-returned",
        retained_pi_error(&refusal),
    );
    let second_open = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
    worker.join().unwrap();
    *service.native_publication_test_barrier.lock().unwrap() = None;
    assert_eq!(refusal.code(), "test.native_submission_not_dispatched");
    let second_open = second_open.unwrap();
    assert!(begun.elapsed() < Duration::from_secs(15));
    assert_eq!(
        service.store.view(&first, None).unwrap()["draft"]["text"],
        draft.text
    );
    assert_eq!(
        service.store.view(&first, None).unwrap()["draft"]["revision"],
        draft.revision
    );
    assert_eq!(
        service
            .apply(EncounterRequest::Status {
                agent_session: first.clone()
            })
            .unwrap()["native_session_id"],
        first_open["native_session_id"]
    );
    assert_eq!(
        service
            .apply(EncounterRequest::Status {
                agent_session: second.clone()
            })
            .unwrap()["native_session_id"],
        second_open["native_session_id"]
    );
    assert_ne!(
        first_open["native_session_id"],
        second_open["native_session_id"]
    );
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(
        shutdown["stopped"].as_array().unwrap().len(),
        2,
        "both genuine native residents cleaned"
    );
    directory.finish_success().unwrap();
}

#[test]
fn retained_native_pi_diagnostics_preserve_actual_owner_bytes_on_unwind() {
    let root = tempfile::tempdir().unwrap();
    let owner_path = root.path().join("unwind-owner/owner");
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut evidence = RetainedPiEvidence::at(root.path(), "unwind-owner");
        let home = AikitHome::at(evidence.path().join("owner"));
        let (_, session) = attached(&home);
        let service = Arc::new(EncounterService::new(home).unwrap());
        evidence.observe(&service, std::slice::from_ref(&session));
        service
            .store
            .set_draft(&session, 0, "controlled-private-draft-do-not-export")
            .unwrap();
        evidence.mark(
            "real-unwind-before-provider",
            json!({"provider_started":false}),
        );
        panic!("controlled real test unwind before any provider");
    }));
    assert!(panic.is_err());
    assert!(owner_path.join("state/encounters.sqlite3").is_file());
    let safe =
        std::fs::read_to_string(root.path().join("unwind-owner/after-test-cleanup.json")).unwrap();
    assert!(!safe.contains("controlled-private-draft-do-not-export"));
    assert!(!safe.contains("provider_success"));
    let fresh = EncounterService::new(AikitHome::at(owner_path)).unwrap();
    let session = ResourceRef::parse("agent-session/native-startup-owner-regression").unwrap();
    assert_eq!(
        fresh.store.view(&session, None).unwrap()["draft"]["text"],
        "controlled-private-draft-do-not-export"
    );
    let timeline =
        std::fs::read_to_string(root.path().join("unwind-owner/timeline.jsonl")).unwrap();
    assert!(timeline.contains("\"test_passed\":false"));
    assert!(timeline.contains("\"panicking\":true"));
}

#[test]
fn actual_required_diagnostic_filesystem_refusal_cannot_certify_native_test_success() {
    let root = tempfile::tempdir().unwrap();
    for (name, target) in [
        ("snapshot-refusal", "before-test-cleanup.json"),
        ("timeline-refusal", "timeline.jsonl"),
    ] {
        let mut evidence = RetainedPiEvidence::at(root.path(), name);
        let home = AikitHome::at(evidence.path().join("owner"));
        let (_, session) = attached(&home);
        let service = Arc::new(EncounterService::new(home).unwrap());
        evidence.observe(&service, std::slice::from_ref(&session));
        let occupied = evidence.path().join(target);
        if occupied.exists() {
            std::fs::rename(
                &occupied,
                evidence.path().join("retained-initial-timeline.jsonl"),
            )
            .unwrap();
        }
        std::fs::create_dir(&occupied).unwrap();
        std::fs::write(occupied.join("retained.txt"), b"preexisting bytes").unwrap();
        let refusal = evidence.finish_success().unwrap_err();
        assert_eq!(refusal.code(), "test.native_pi_required_evidence_missing");
        assert!(!evidence.passed);
        assert_eq!(
            std::fs::read(occupied.join("retained.txt")).unwrap(),
            b"preexisting bytes"
        );
        assert!(evidence
            .path()
            .join("owner/state/encounters.sqlite3")
            .is_file());
    }
}

#[test]
fn actual_native_journal_unwind_and_cleanup_refusal_cannot_certify_success() {
    let root = tempfile::tempdir().unwrap();
    let mut evidence = RetainedPiEvidence::at(root.path(), "native-read-cleanup-refusal");
    let home = AikitHome::at(evidence.path().join("owner"));
    let (_, session) = attached(&home);
    let service = Arc::new(EncounterService::new(home).unwrap());
    evidence.observe(&service, std::slice::from_ref(&session));
    let draft = service
        .store
        .set_draft(&session, 0, "controlled retained draft")
        .unwrap();
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = service
            .store
            .submit_context(&session, draft.revision, None, |_| -> Result<()> {
                panic!("controlled actual SQLite transaction unwind; no native prompt");
            });
    }));
    assert!(unwind.is_err());
    let refusal = evidence.finish_success().unwrap_err();
    assert_eq!(refusal.code(), "test.native_pi_required_evidence_missing");
    assert!(!evidence.passed);
    let failures = evidence.diagnostics.failures.lock().unwrap().clone();
    assert!(failures
        .iter()
        .any(|failure| failure["phase"] == "actual-native-owner-cleanup"));
    assert!(failures
        .iter()
        .any(|failure| failure["phase"] == "before-test-cleanup"));
    assert!(evidence
        .path()
        .join("owner/state/encounters.sqlite3")
        .is_file());
}
