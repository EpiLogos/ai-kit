//! Exact owned idle-body cleanup and explicit fresh successor admission. All
//! durable facts use the existing encounter journal; a release is not a Return.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReleasedPredecessor {
    pub expected_native_session_id: String,
    pub expected_generation: String,
    pub release_cursor: u64,
}

pub(super) fn validate_predecessor(
    store: &EncounterStore,
    session: &ResourceRef,
    current_binding: Option<&Value>,
    expected: &NativeReleasedPredecessor,
    owned_successor: bool,
) -> Result<()> {
    let receipt = store
        .native_release_receipt(
            session,
            &expected.expected_native_session_id,
            &expected.expected_generation,
        )?
        .ok_or_else(|| {
            AikitError::new(
                "encounter.released_predecessor_absent",
                "An actual owner-observed native release is required for a fresh successor",
            )
        })?;
    if receipt["receipt"]["cleanup_confirmed"] != true
        || receipt["terminal_cursor"].as_u64() != Some(expected.release_cursor)
    {
        return Err(AikitError::new(
            "encounter.released_predecessor_changed",
            "Exact predecessor cleanup is uncertain or its retained receipt changed",
        ));
    }
    let Some(binding) = current_binding else {
        return Err(AikitError::new(
            "encounter.released_predecessor_changed",
            "The retained native predecessor binding is absent",
        ));
    };
    let original = binding["native_session_id"].as_str()
        == Some(expected.expected_native_session_id.as_str())
        && binding["connection_generation"].as_str() == Some(expected.expected_generation.as_str());
    let successor = owned_successor
        && binding["continuation"] == "fresh-native-successor"
        && binding["released_predecessor"] == serde_json::to_value(expected).map_err(error)?;
    if !original && !successor {
        return Err(AikitError::new("encounter.released_predecessor_changed", "Another native generation owns the retained session; stale replacement performs no effect"));
    }
    Ok(())
}

impl EncounterService {
    pub(super) fn release_native(
        &self,
        session: ResourceRef,
        native: String,
        generation: String,
        deadline: Instant,
    ) -> Result<Value> {
        // Cleanup belongs to this exact owned body even if its semantic space
        // was later detached. Replacement separately requires current attachment.
        // An exact retry returns the immutable old cleanup receipt even if a
        // successor is now resident. It never stops that successor.
        if let Some(receipt) = self
            .store
            .native_release_receipt(&session, &native, &generation)?
        {
            return if receipt["receipt"]["cleanup_confirmed"] == true {
                Ok(receipt)
            } else {
                Err(AikitError::new(
                    "encounter.native_release_uncertain",
                    "The retained exact release did not confirm cleanup",
                )
                .with("native_release", receipt.to_string()))
            };
        }
        let held = self.resident(&session)?;
        let operation = native_control_lease(&held, deadline)?;
        if held.generation != generation || held.lane.binding().native_session_id != native {
            return Err(AikitError::new(
                "encounter.native_release_basis",
                "Native session or connection generation changed; no process stopped",
            ));
        }
        if held.host.identity(&session)?.state != aikit_adapters::SessionLaneState::Resident {
            return Err(AikitError::new("encounter.native_release_busy", "An active or interrupted turn must settle through its ordinary lifecycle before idle release"));
        }
        if self
            .permissions
            .lock()
            .map_err(error)?
            .get(&session)
            .is_some_and(|requests| !requests.is_empty())
        {
            return Err(AikitError::new(
                "encounter.native_release_permission_pending",
                "Actual native consent requests must settle before idle-body release",
            ));
        }
        // Reuse the existing startup lease so cleanup and startup cannot race.
        // There is no new ownership registry and no map lock during native IO.
        let mut lease =
            self.begin_native_lease(&session, generation.clone(), "native-release-validation")?;
        lease.terminal_recorded = true;
        let removed = {
            let mut residents = self.residents.lock().map_err(error)?;
            if !residents
                .get(&session)
                .is_some_and(|current| Arc::ptr_eq(current, &held))
            {
                return Err(AikitError::new(
                    "encounter.native_release_basis",
                    "Resident changed before exact cleanup",
                ));
            }
            residents.remove(&session).expect("checked owned resident")
        };
        drop(operation);
        drop(held);
        let resident = match Arc::try_unwrap(removed) {
            Ok(resident) => resident,
            Err(held) => {
                self.residents.lock().map_err(error)?.insert(session, held);
                return Err(AikitError::new(
                    "encounter.resident_in_use",
                    "The body is borrowed by another owner operation; no cleanup occurred",
                ));
            }
        };
        if let Err(failure) = self
            .store
            .reserve_native_release(&session, &native, &generation)
        {
            self.residents
                .lock()
                .map_err(error)?
                .insert(session, Arc::new(resident));
            return Err(failure);
        }
        // Late channel files stay recoverable. Stop and join its owned reader
        // before dropping the body; the journal's generation fence is atomic.
        let watcher = resident.stop_child_messages();
        let cleanup = resident.host.shutdown();
        let cleanup_confirmed = watcher.is_ok() && cleanup.is_ok();
        let cleanup_error = watcher
            .err()
            .map(|e| e.to_string())
            .into_iter()
            .chain(cleanup.as_ref().err().map(ToString::to_string))
            .collect::<Vec<_>>();
        let receipt = self.store.finish_native_release(
            &session, &native, &generation, cleanup_confirmed,
            cleanup.as_ref().ok().and_then(|status|status.as_ref().map(ToString::to_string)),
            (!cleanup_error.is_empty()).then(||cleanup_error.join("; ")),
        ).map_err(|failure| AikitError::new("encounter.native_release_outcome_uncertain", format!("Owned cleanup was attempted but its outcome could not be retained: {failure}")).with("cleanup_confirmed",cleanup_confirmed.to_string()))?;
        self.permissions.lock().map_err(error)?.remove(&session);
        if !cleanup_confirmed {
            return Err(AikitError::new(
                "encounter.native_release_uncertain",
                "Exact owned cleanup is not fully confirmed; replacement remains fenced",
            )
            .with("native_release", receipt.to_string()));
        }
        Ok(receipt)
    }
}

impl Resident {
    pub(super) fn stop_child_messages(&self) -> Result<()> {
        if let Some(mut watcher) = self.child_messages.lock().map_err(error)?.take() {
            watcher.stop()?;
        }
        Ok(())
    }
}

/// The child channel is owned by one resident generation. A stop is bounded;
/// if a filesystem call stalls, its later journal attempt is still fenced by
/// canonical release intent and cannot consume files for a successor.
pub(super) struct OwnedChildWatcher {
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl OwnedChildWatcher {
    pub(super) fn start(
        store: Arc<EncounterStore>,
        session: ResourceRef,
        generation: String,
        dir: PathBuf,
    ) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stopped);
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(400));
                if worker_stop.load(Ordering::Acquire) {
                    break;
                }
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    break;
                };
                let mut files: Vec<_> = entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                    .collect();
                files.sort();
                for path in files {
                    if worker_stop.load(Ordering::Acquire) {
                        return;
                    }
                    let event = (|| -> Result<Value> {
                        use std::io::Read;
                        let mut bytes = Vec::new();
                        std::fs::File::open(&path)
                            .map_err(error)?
                            .take(64 * 1024 + 1)
                            .read_to_end(&mut bytes)
                            .map_err(error)?;
                        if bytes.len() > 64 * 1024 {
                            return Err(error("child message exceeds the 64 KiB channel bound"));
                        }
                        let value: Value = serde_json::from_slice(&bytes).map_err(error)?;
                        if value["schema"] != "actuation.child-message/v1" {
                            return Err(error("unrecognised child message schema"));
                        }
                        Ok(
                            json!({"kind":"child-message","from":value["from"],"receiver_role":value["receiver_role"],"text":value["text"],"text_sha256":value["text_sha256"],"file":path.file_name().map(|name|name.to_string_lossy().to_string())}),
                        )
                    })();
                    if worker_stop.load(Ordering::Acquire) {
                        return;
                    }
                    let result = event.and_then(|event| {
                        store.append_child_message(&session, &generation, &event)
                    });
                    match result {
                        Ok(_) => {
                            if let Err(failure) = std::fs::remove_file(&path) {
                                let _=store.append_child_message(&session,&generation,&json!({"kind":"child-message-file-retained","file":path.file_name().map(|name|name.to_string_lossy().to_string()),"reason":failure.to_string()}));
                            }
                        }
                        Err(failure)
                            if failure.code() == "encounter.child_message_generation_changed" =>
                        {
                            return
                        }
                        Err(failure) => {
                            // Reject only after that exact generation retains its
                            // rejection. Late/stale bytes remain at their source.
                            if store.append_child_message(&session,&generation,&json!({"kind":"child-message-rejected","file":path.file_name().map(|name|name.to_string_lossy().to_string()),"reason":failure.to_string()})).is_ok() {
                                let _=std::fs::rename(&path,path.with_extension("json.rejected"));
                            } else { return; }
                        }
                    }
                }
            }
        });
        Self {
            stopped,
            worker: Some(worker),
        }
    }
    fn stop(&mut self) -> Result<()> {
        self.stopped.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.thread().unpark();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if !worker.is_finished() {
            return Err(AikitError::new(
                "encounter.child_watcher_cleanup_uncertain",
                "The generation-fenced child watcher did not stop within its cleanup budget",
            ));
        }
        worker.join().map_err(|_| {
            AikitError::new(
                "encounter.child_watcher_failed",
                "The child watcher panicked during cleanup",
            )
        })
    }
}
impl Drop for OwnedChildWatcher {
    fn drop(&mut self) {
        if let Err(failure) = self.stop() {
            eprintln!("Native child watcher cleanup: {failure}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::session_space_application::{
        SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
    };
    use std::process::Command;
    #[test]
    #[ignore = "requires exact installed AIKIT_CAW_PI_BIN; actual Pi plus OS child-channel writer, never prompts"]
    fn actual_pi_release_joins_owned_reader_and_retains_late_channel_bytes() {
        let pi = PathBuf::from(
            std::env::var_os("AIKIT_CAW_PI_BIN").expect("exact installed Pi is required"),
        );
        assert_eq!(pi.file_name().and_then(|name| name.to_str()), Some("pi"));
        let directory = tempfile::tempdir().unwrap();
        let home = AikitHome::at(directory.path().join("home"));
        let space = SessionSpaceRef::parse("session-space/owned-native-release-reader").unwrap();
        let session = ResourceRef::parse("agent-session/owned-native-release-reader").unwrap();
        let authored = SessionSpaceApplicationStore::new(home.clone());
        authored
            .apply(
                &authored
                    .stage(
                        None,
                        SessionSpaceMutation::Create {
                            id: space.clone(),
                            label: None,
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        authored.apply(&authored.stage(Some(&space),SessionSpaceMutation::AttachAgentSession{attachment:SessionSpaceAgentAttachmentIntent{agent_session:session.clone(),purpose:Some("Actual native idle-body/channel lifetime regression; no worker success".into()),provenance:vec!["isolated native Pi + real OS file writer".into()]}}).unwrap()).unwrap();
        EncounterService::configure(
            &home,
            EncounterProvider {
                protocol: EncounterProtocol::PiRpc,
                id: "actual-pi-release-reader".into(),
                label: "Actual installed Pi without inference".into(),
                argv: vec![
                    pi.display().to_string(),
                    "--mode".into(),
                    "rpc".into(),
                    "--no-extensions".into(),
                    "--session-dir".into(),
                    directory
                        .path()
                        .join("native-pi-sessions")
                        .display()
                        .to_string(),
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
        let service = EncounterService::new(home.clone()).unwrap();
        let opened = service
            .apply(EncounterRequest::Open {
                space,
                agent_session: session.clone(),
                provider: "actual-pi-release-reader".into(),
                cwd: directory.path().to_path_buf(),
            })
            .unwrap();
        let generation = opened["connection_generation"].as_str().unwrap().to_owned();
        let dir = directory
            .path()
            .join("actual-child-channel")
            .join(&generation);
        std::fs::create_dir_all(&dir).unwrap();
        let native_resident = service.resident(&session).unwrap();
        *native_resident.child_messages.lock().unwrap() = Some(OwnedChildWatcher::start(
            Arc::clone(&service.store),
            session.clone(),
            generation.clone(),
            dir.clone(),
        ));
        drop(native_resident);
        // An actual OS child emits channel words. It supplies no provider
        // protocol, fabricated native identity, model response or Factory effect.
        let mut writer=Command::new("python3").args(["-c",r#"
import hashlib,json,os,pathlib,sys,time
root=pathlib.Path(sys.argv[1]); ready=pathlib.Path(sys.argv[2])
def put(name,text):
 value={'schema':'actuation.child-message/v1','from':'native-process/'+str(os.getpid()),'receiver_role':'parent','text':text,'text_sha256':hashlib.sha256(text.encode('utf-8')).hexdigest()}
 (root/name).write_text(json.dumps(value))
put('before.json','actual OS words before owned release');ready.write_text('first actual file written')
while not (root/'release-gate').exists():time.sleep(.01)
put('late.json','actual OS bytes after owner-observed release')
"#,dir.to_str().unwrap(),directory.path().join("writer-ready").to_str().unwrap()]).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if service
                .store
                .events(&session, 0, 256)
                .unwrap()
                .events
                .iter()
                .any(|event| event.event["kind"] == "child-message")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual child-channel reader did not retain the real first file"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let released = service
            .apply(EncounterRequest::ReleaseNative {
                agent_session: session.clone(),
                expected_native_session_id: opened["native_session_id"].as_str().unwrap().into(),
                expected_generation: generation.clone(),
            })
            .unwrap();
        assert_eq!(released["receipt"]["cleanup_confirmed"], true);
        let before = serde_json::to_value(service.store.events(&session, 0, 256).unwrap()).unwrap();
        std::fs::write(dir.join("release-gate"), b"actual owner release complete").unwrap();
        let exit_by = Instant::now() + Duration::from_secs(5);
        let writer_status = loop {
            if let Some(status) = writer.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < exit_by,
                "actual owned writer did not finish after release gate"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(writer_status.success());
        let late_bytes = std::fs::read(dir.join("late.json")).unwrap();
        let late: Value = serde_json::from_slice(&late_bytes).unwrap();
        assert_eq!(late["text"], "actual OS bytes after owner-observed release");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(std::fs::read(dir.join("late.json")).unwrap(), late_bytes);
        assert_eq!(
            serde_json::to_value(service.store.events(&session, 0, 256).unwrap()).unwrap(),
            before
        );
        let current = service
            .apply(EncounterRequest::View {
                agent_session: session,
                before: None,
            })
            .unwrap();
        assert_eq!(current["connection"]["state"], "Released");
        assert_eq!(current["connection"]["resident"], false);
    }
}
