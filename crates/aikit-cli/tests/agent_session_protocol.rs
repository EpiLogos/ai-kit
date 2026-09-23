//! Controlled peer, real native store/handler/ACP/stdio. This is not live inference.
#![cfg(unix)]
use aikit_adapters::agency_admission::AgencySourceBasis;
use aikit_adapters::interactive_connection::PermissionDecision;
use aikit_cli::encounter_service::{
    EncounterAgencyBinding, EncounterProvider, EncounterRequest, EncounterService,
};
use aikit_core::{
    session_space::SessionSpaceRef,
    session_space_application::{SessionSpaceAgentAttachmentIntent, SessionSpaceMutation},
    ResourceRef, SourceRevision,
};
use aikit_store::{encounter::EncounterStore, AikitHome, SessionSpaceApplicationStore};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

struct Rig {
    temp: tempfile::TempDir,
    home: AikitHome,
    service: Arc<EncounterService>,
    space: SessionSpaceRef,
    session: ResourceRef,
}
impl Rig {
    fn new(mode: &str) -> Self {
        let rig = Self::unopened(mode);
        rig.service.apply(rig.open(false)).unwrap();
        rig
    }
    fn unopened(mode: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        let store = SessionSpaceApplicationStore::new(home.clone());
        let space = SessionSpaceRef::parse("session-space/controlled").unwrap();
        let session = ResourceRef::parse("agent-session/controlled").unwrap();
        store
            .apply(
                &store
                    .stage(
                        None,
                        SessionSpaceMutation::Create {
                            id: space.clone(),
                            label: Some("Controlled session".into()),
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        store
            .apply(
                &store
                    .stage(
                        Some(&space),
                        SessionSpaceMutation::AttachAgentSession {
                            attachment: SessionSpaceAgentAttachmentIntent {
                                agent_session: session.clone(),
                                purpose: Some("Controlled protocol regression".into()),
                                provenance: vec!["test-only".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/agent_session_protocol.py");
        EncounterService::configure(
            &home,
            EncounterProvider {
                protocol: Default::default(),
                id: "controlled".into(),
                label: "Controlled protocol peer (not a model)".into(),
                argv: vec![
                    "python3".into(),
                    "-u".into(),
                    script.display().to_string(),
                    mode.into(),
                ],
                required_context: None,
                model_policy: None,
            },
        )
        .unwrap();
        let service = Arc::new(EncounterService::new(home.clone()).unwrap());
        Self {
            temp,
            home,
            service,
            space,
            session,
        }
    }
    fn open(&self, resume: bool) -> EncounterRequest {
        if resume {
            EncounterRequest::Reconnect {
                space: self.space.clone(),
                agent_session: self.session.clone(),
                provider: "controlled".into(),
                cwd: self.temp.path().into(),
            }
        } else {
            EncounterRequest::Open {
                space: self.space.clone(),
                agent_session: self.session.clone(),
                provider: "controlled".into(),
                cwd: self.temp.path().into(),
            }
        }
    }
    fn view(&self) -> Value {
        self.service
            .apply(EncounterRequest::View {
                agent_session: self.session.clone(),
                before: None,
            })
            .unwrap()
    }
    fn model(&self) -> Value {
        self.service
            .apply(EncounterRequest::ModelRead {
                agent_session: self.session.clone(),
            })
            .unwrap()
    }
    fn select(&self, native: &str) -> EncounterRequest {
        EncounterRequest::ModelSelect {
            agent_session: self.session.clone(),
            provider_model_id: "test/b".into(),
            provider_reasoning_effort: Some("high".into()),
            expected_native_session_id: Some(native.into()),
        }
    }
    fn prompt(&self, text: &str) {
        let basis = self.view()["draft"]["revision"].as_u64().unwrap();
        let draft = self
            .service
            .apply(EncounterRequest::Draft {
                agent_session: self.session.clone(),
                basis,
                text: text.into(),
            })
            .unwrap();
        self.service
            .apply(EncounterRequest::Prompt {
                agent_session: self.session.clone(),
                draft_revision: draft["revision"].as_u64().unwrap(),
            })
            .unwrap();
    }
    fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let view = self.view();
            if predicate(&view) {
                return view;
            }
            assert!(
                Instant::now() < deadline,
                "native observation timeout: {view}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn stop(&self) {
        self.service
            .apply(EncounterRequest::Shutdown {
                expected_pid: std::process::id(),
            })
            .unwrap();
    }
}
#[test]
fn native_selector_read_confirm_and_stale_identity_guard() {
    let rig = Rig::new("normal");
    let before = rig.model();
    assert_eq!(before["model_controls"]["model_selection"], true);
    assert_eq!(
        rig.service
            .apply(rig.select("stale-native"))
            .unwrap_err()
            .code(),
        "encounter.stale_native_session"
    );
    assert_eq!(
        rig.model()["model_observation"]["current_model_id"],
        "test/a"
    );
    let receipt = rig.service.apply(rig.select("controlled-native")).unwrap();
    assert_eq!(receipt["selected"], true);
    assert_eq!(receipt["inference_observed"], false);
    assert_eq!(receipt["model_observation"]["current_model_id"], "test/b");
    assert_eq!(
        receipt["model_observation"]["reasoning_effort"]["currentValue"],
        "high"
    );
    let events = rig
        .service
        .apply(EncounterRequest::Read {
            agent_session: rig.session.clone(),
            after: 0,
            limit: 100,
        })
        .unwrap();
    let confirmed = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["event"]["kind"].as_str())
        .filter(|kind| kind.ends_with("configuration-confirmed"))
        .collect::<Vec<_>>();
    assert_eq!(
        confirmed,
        vec![
            "native-model-configuration-confirmed",
            "native-reasoning-effort-configuration-confirmed"
        ],
        "each provider-confirmed sequential write must be retained in order"
    );
    rig.stop();
}

#[test]
fn unadvertised_reasoning_is_rejected_before_any_model_mutation_or_request_record() {
    let rig = Rig::new("normal");
    let before = rig.model();
    let error = rig
        .service
        .apply(EncounterRequest::ModelSelect {
            agent_session: rig.session.clone(),
            provider_model_id: "test/b".into(),
            provider_reasoning_effort: Some("not-advertised".into()),
            expected_native_session_id: Some("controlled-native".into()),
        })
        .unwrap_err();
    assert_eq!(error.code(), "encounter.reasoning_effort_not_advertised");
    let after = rig.model();
    assert_eq!(
        after["model_observation"], before["model_observation"],
        "a rejected reasoning option must not change the model or reasoning effort"
    );
    let events = rig
        .service
        .apply(EncounterRequest::Read {
            agent_session: rig.session.clone(),
            after: 0,
            limit: 100,
        })
        .unwrap();
    assert_eq!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["event"]["kind"] == "native-model-configuration-requested")
            .count(),
        0
    );
    rig.stop();
}
#[test]
fn production_handlers_collect_partial_streams_and_reopen_the_same_session() {
    let mut rig = Rig::new("normal");
    rig.prompt("partial");
    let view = rig.until(|v| {
        v["connection"]["state"] == "Resident" && v["blocks"].to_string().contains("then final.")
    });
    assert!(view["blocks"].to_string().contains("First partial"));
    let original = view["connection"]["native_session_id"].clone();
    rig.stop();
    rig.service = Arc::new(EncounterService::new(rig.home.clone()).unwrap());
    assert_eq!(
        rig.service.apply(rig.open(false)).unwrap_err().code(),
        "encounter.resume_required"
    );
    rig.service.apply(rig.open(true)).unwrap();
    assert_eq!(rig.view()["connection"]["native_session_id"], original);
    rig.prompt("partial");
    rig.until(|v| {
        v["connection"]["state"] == "Resident" && v["blocks"].to_string().contains("then final.")
    });
    rig.stop();
}
#[test]
fn native_permission_denial_and_cancellation_are_not_agent_work_success() {
    let rig = Rig::new("normal");
    rig.prompt("deny");
    let view = rig.until(|v| {
        v["permissions"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
    });
    let id = view["permissions"][0]["native_request_id"]
        .as_str()
        .unwrap()
        .to_string();
    rig.service
        .apply(EncounterRequest::Permission {
            agent_session: rig.session.clone(),
            request_id: id,
            decision: PermissionDecision::Selected {
                option_id: "deny".into(),
            },
        })
        .unwrap();
    rig.until(|v| {
        v["connection"]["state"] == "Resident"
            && v["blocks"].to_string().contains("Permission denied")
    });
    rig.prompt("cancel");
    rig.until(|v| v["blocks"].to_string().contains("waiting for cancellation"));
    assert_eq!(rig.model()["model_controls"]["model_selection"], false);
    rig.service
        .apply(EncounterRequest::Cancel {
            agent_session: rig.session.clone(),
            reason: Some("controlled user stop".into()),
        })
        .unwrap();
    rig.until(|v| v["connection"]["state"] == "Resident");
    rig.stop();
}
#[test]
fn disconnect_preserves_partial_work_and_does_not_offer_controls() {
    let rig = Rig::new("normal");
    rig.prompt("disconnect");
    let view = rig.until(|v| v["connection"]["error"].is_string());
    assert!(view["blocks"].to_string().contains("Partial answer"));
    assert_eq!(rig.model()["model_controls"]["model_selection"], false);
    rig.stop();
}
#[test]
fn lost_configuration_ack_is_never_a_selected_receipt_or_a_replayed_write() {
    let rig = Rig::new("lost-model-ack");
    assert!(rig.service.apply(rig.select("controlled-native")).is_err());
    let events = rig
        .service
        .apply(EncounterRequest::Read {
            agent_session: rig.session.clone(),
            after: 0,
            limit: 100,
        })
        .unwrap();
    let events = events["events"].as_array().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|row| row["event"]["kind"] == "native-model-configuration-requested")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|row| row["event"]["kind"] == "native-model-configuration-confirmed")
            .count(),
        0
    );
    rig.stop();
}

#[test]
fn opening_keeps_views_responsive_and_refuses_a_duplicate_process_launch() {
    let rig = Rig::unopened("slow-initialize");
    let service = Arc::clone(&rig.service);
    let request = rig.open(false);
    let opening = thread::spawn(move || service.apply(request));
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let read = rig
            .service
            .apply(EncounterRequest::Read {
                agent_session: rig.session.clone(),
                after: 0,
                limit: 100,
            })
            .unwrap();
        if read["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["event"]["kind"] == "native-open-attempted")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "opening was not journaled: {read}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let observed_at = Instant::now();
    let view = rig.view();
    assert!(observed_at.elapsed() < Duration::from_millis(200));
    assert_eq!(view["connection"]["state"], "Opening");
    assert_eq!(
        rig.service.apply(rig.open(false)).unwrap_err().code(),
        "encounter.open_in_progress"
    );

    let other = ResourceRef::parse("agent-session/independent-view").unwrap();
    let store = SessionSpaceApplicationStore::new(rig.home.clone());
    store
        .apply(
            &store
                .stage(
                    Some(&rig.space),
                    SessionSpaceMutation::AttachAgentSession {
                        attachment: SessionSpaceAgentAttachmentIntent {
                            agent_session: other.clone(),
                            purpose: Some("Independent view during startup".into()),
                            provenance: vec!["native-test".into()],
                        },
                    },
                )
                .unwrap(),
        )
        .unwrap();
    let observed_at = Instant::now();
    let other_view = rig
        .service
        .apply(EncounterRequest::View {
            agent_session: other,
            before: None,
        })
        .unwrap();
    assert!(observed_at.elapsed() < Duration::from_millis(200));
    assert_eq!(other_view["connection"]["state"], "Disconnected");

    assert_eq!(opening.join().unwrap().unwrap()["resident"], true);
    let read = rig
        .service
        .apply(EncounterRequest::Read {
            agent_session: rig.session.clone(),
            after: 0,
            limit: 100,
        })
        .unwrap();
    assert_eq!(
        read["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["event"]["kind"] == "native-open-attempted")
            .count(),
        1
    );
    rig.stop();
}

#[test]
fn idempotent_open_rechecks_a_withdrawn_agency_before_returning_resident() {
    let rig = Rig::new("normal");
    let binding = EncounterAgencyBinding {
        revision: SourceRevision::parse("rev/withdrawn-1").unwrap(),
        active: false,
        agent_ref: ResourceRef::parse("agent:controlled").unwrap(),
        agency_ref: ResourceRef::parse("agency:controlled").unwrap(),
        world_ref: ResourceRef::parse("control:root").unwrap(),
        world_binding_ref: ResourceRef::parse("binding:controlled").unwrap(),
        agency_source: AgencySourceBasis {
            source_ref: ResourceRef::parse("source/controlled-agency").unwrap(),
            revision: SourceRevision::parse("rev/source-1").unwrap(),
            path: rig.temp.path().join("withdrawn-agency.json"),
            content_digest: format!("blake3:{}", blake3::hash(b"withdrawn").to_hex()),
        },
        actuation_bin: rig.temp.path().join("unused-actuation"),
        allowed_senders: [ResourceRef::parse("human:owner").unwrap()].into(),
        allowed_packet_sources: Default::default(),
        context: None,
    };
    EncounterService::configure_agency(&rig.home, &rig.session, &binding, None).unwrap();
    let failure = rig.service.apply(rig.open(false)).unwrap_err();
    assert_eq!(failure.code(), "encounter.participant_withdrawn");
    assert_eq!(rig.view()["connection"]["resident"], true);
    rig.stop();
}

#[test]
fn restart_projects_unfinished_or_uncertain_native_open_from_the_real_journal() {
    let rig = Rig::unopened("normal");
    let store = EncounterStore::open(&rig.home).unwrap();
    store
        .append(
            &rig.session,
            &serde_json::json!({"kind":"native-open-reserved","connection_generation":"unfinished","owner_pid":999,"space":rig.space,"provider":"controlled","cwd":rig.temp.path()}),
        )
        .unwrap();
    let service = EncounterService::new(rig.home.clone()).unwrap();
    let view = service
        .apply(EncounterRequest::View {
            agent_session: rig.session.clone(),
            before: None,
        })
        .unwrap();
    assert_eq!(view["connection"]["state"], "RecoveryRequired");
    assert_eq!(
        service.apply(rig.open(false)).unwrap_err().code(),
        "encounter.open_recovery_required"
    );
    assert_eq!(
        service
            .apply(EncounterRequest::Shutdown {
                expected_pid: std::process::id(),
            })
            .unwrap_err()
            .code(),
        "encounter.cleanup_uncertain"
    );
    store
        .append(
            &rig.session,
            &serde_json::json!({"kind":"native-open-refused","connection_generation":"unfinished","reason":"owned child cleanup not confirmed","cleanup_confirmed":false}),
        )
        .unwrap();
    let service = EncounterService::new(rig.home.clone()).unwrap();
    let view = service
        .apply(EncounterRequest::View {
            agent_session: rig.session.clone(),
            before: None,
        })
        .unwrap();
    assert_eq!(view["connection"]["state"], "CleanupUncertain");
    assert_eq!(
        service.apply(rig.open(false)).unwrap_err().code(),
        "encounter.cleanup_uncertain"
    );
    let receipt = service
        .apply(EncounterRequest::ReconcileNativeOpen {
            agent_session: rig.session.clone(),
            expected_generation: "unfinished".into(),
            evidence_ref: ResourceRef::parse("evidence/native-cleanup").unwrap(),
            cleanup_confirmed: true,
        })
        .unwrap();
    assert_eq!(receipt["reconciled"], true);
    let view = service
        .apply(EncounterRequest::View {
            agent_session: rig.session.clone(),
            before: None,
        })
        .unwrap();
    assert_eq!(view["connection"]["state"], "Disconnected");
}
