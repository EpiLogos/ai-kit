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
        Self::build(mode, None)
    }
    fn unopened(mode: &str) -> Self {
        Self::settings(mode, None, None, false)
    }
    fn build(mode: &str, default_modes: Option<serde_json::Value>) -> Self {
        Self::settings(mode, default_modes, None, true)
    }
    fn settings(
        mode: &str,
        default_modes: Option<serde_json::Value>,
        default_models: Option<serde_json::Value>,
        open: bool,
    ) -> Self {
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
                    temp.path().join("native-model.json").display().to_string(),
                ],
                body_ref: None,
                body_revision: None,
                from_profile: None,
                argv_fallback: Vec::new(),
                env: Default::default(),
                cwd: None,
                required_context: None,
                model_policy: None,
                now_context: None,
            },
        )
        .unwrap();
        if let Some(defaults) = default_modes {
            aikit_cli::permission_defaults::write(
                &home,
                &aikit_cli::permission_defaults::from_value(&defaults).unwrap(),
            )
            .unwrap();
        }
        if let Some(defaults) = default_models {
            aikit_cli::model_defaults::write(
                &home,
                &aikit_cli::model_defaults::from_value(&defaults).unwrap(),
            )
            .unwrap();
        }
        let service = Arc::new(EncounterService::new(home.clone()).unwrap());
        let rig = Self {
            temp,
            home,
            service,
            space,
            session,
        };
        if open {
            rig.service.apply(rig.open(false)).unwrap();
        }
        rig
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
    fn modes(&self) -> Value {
        self.service
            .apply(EncounterRequest::ModeRead {
                agent_session: self.session.clone(),
            })
            .unwrap()
    }
    fn select_mode(&self, mode: &str, native: &str) -> EncounterRequest {
        EncounterRequest::ModeSelect {
            agent_session: self.session.clone(),
            provider_mode_id: mode.into(),
            expected_native_session_id: Some(native.into()),
        }
    }
    fn journal(&self) -> Vec<Value> {
        self.service
            .apply(EncounterRequest::Read {
                agent_session: self.session.clone(),
                after: 0,
                limit: 256,
            })
            .unwrap()["events"]
            .as_array()
            .unwrap()
            .clone()
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
    let reservation = loop {
        let read = rig
            .service
            .apply(EncounterRequest::Read {
                agent_session: rig.session.clone(),
                after: 0,
                limit: 100,
            })
            .unwrap();
        if let Some(row) = read["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["event"]["kind"] == "native-open-reserved")
        {
            break row.clone();
        }
        assert!(
            Instant::now() < deadline,
            "opening was not journaled: {read}"
        );
        thread::sleep(Duration::from_millis(5));
    };
    // A reservation precedes effects; it is not a process-start receipt.
    let generation = reservation["event"]["connection_generation"]
        .as_str()
        .expect("the actual reservation retains its generation");
    assert!(!generation.is_empty());
    assert_eq!(
        reservation["event"]["owner_pid"].as_u64(),
        Some(u64::from(std::process::id()))
    );
    let observed_at = Instant::now();
    let view = rig.view();
    assert!(observed_at.elapsed() < Duration::from_millis(200));
    assert_eq!(view["connection"]["state"], "Opening");
    assert_eq!(
        rig.service.apply(rig.open(false)).unwrap_err().code(),
        "encounter.native_open_in_progress"
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
    let events = read["events"].as_array().unwrap();
    let reservations: Vec<_> = events
        .iter()
        .filter(|row| row["event"]["kind"] == "native-open-reserved")
        .collect();
    let bindings: Vec<_> = events
        .iter()
        .filter(|row| row["event"]["kind"] == "binding")
        .collect();
    assert_eq!(reservations.len(), 1);
    assert_eq!(bindings.len(), 1);
    assert_eq!(reservations[0], &reservation);
    assert_eq!(
        bindings[0]["event"]["connection_generation"].as_str(),
        Some(generation)
    );
    assert_eq!(
        bindings[0]["event"]["owner_pid"].as_u64(),
        Some(u64::from(std::process::id()))
    );
    assert!(bindings[0]["cursor"].as_u64().unwrap() > reservation["cursor"].as_u64().unwrap());
    rig.stop();
}

#[test]
fn native_modes_read_select_confirm_and_journal_with_owner_time() {
    let rig = Rig::new("normal");
    let before = rig.modes();
    assert_eq!(
        before,
        serde_json::json!({
            "agent_session": "agent-session/controlled",
            "native_session_id": "controlled-native",
            "mode_observation": {
                "current_mode_id": "default",
                "available_modes": [
                    {"id":"default","name":"Default","description":"Ask before edits."},
                    {"id":"accept_edits","name":"Accept Edits"},
                    {"id":"plan","name":"Plan"}
                ],
                "standing": "provider-reported-configuration-not-independent-selection-or-inference-proof"
            },
            "mode_controls": {"mode_selection": true, "reason": null},
            "standing": "provider-reported-configuration-not-independent-selection-or-inference-proof"
        })
    );
    // The View offers the read, gated like model-read.
    assert!(rig.view()["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["ref"] == "aikit.encounter.mode-read" && action["enabled"] == true));
    assert_eq!(
        rig.service
            .apply(rig.select_mode("accept_edits", "stale-native"))
            .unwrap_err()
            .code(),
        "encounter.stale_native_session"
    );
    assert_eq!(
        rig.service
            .apply(rig.select_mode("bypassPermissions", "controlled-native"))
            .unwrap_err()
            .code(),
        "encounter.mode_not_advertised"
    );
    assert_eq!(
        rig.service
            .apply(rig.select_mode(" ", "controlled-native"))
            .unwrap_err()
            .code(),
        "encounter.invalid_provider_mode_id"
    );
    let receipt = rig
        .service
        .apply(rig.select_mode("accept_edits", "controlled-native"))
        .unwrap();
    assert_eq!(receipt["selected"], true);
    assert_eq!(
        receipt["standing"],
        "provider-confirmed-native-session-mode; applies-to-the-next-action"
    );
    assert_eq!(
        receipt["previous_mode_observation"]["current_mode_id"],
        "default"
    );
    assert_eq!(
        receipt["mode_observation"]["current_mode_id"],
        "accept_edits"
    );
    assert_eq!(
        rig.modes()["mode_observation"]["current_mode_id"],
        "accept_edits"
    );

    let journal = rig.journal();
    // Every stored event carries the owner's observation time.
    assert!(journal.iter().all(|row| row["event"]["observed_at_ms"]
        .as_u64()
        .is_some_and(|ms| ms > 0)));
    let requested: Vec<_> = journal
        .iter()
        .filter(|row| row["event"]["kind"] == "native-mode-configuration-requested")
        .collect();
    let confirmed: Vec<_> = journal
        .iter()
        .filter(|row| row["event"]["kind"] == "native-mode-configuration-confirmed")
        .collect();
    assert_eq!(requested.len(), 1);
    assert_eq!(confirmed.len(), 1);
    assert_eq!(
        requested[0]["event"]["requested_provider_mode_id"],
        "accept_edits"
    );
    assert_eq!(requested[0]["event"]["provider"], "controlled");
    assert_eq!(
        requested[0]["event"]["authority"],
        "provider-advertised-session-mode; applies-to-the-next-action"
    );
    assert_eq!(
        confirmed[0]["event"]["receipt"]["current"]["current_mode_id"],
        "accept_edits"
    );
    assert_eq!(
        confirmed[0]["event"]["receipt"]["previous"]["current_mode_id"],
        "default"
    );
    assert!(requested[0]["cursor"].as_u64() < confirmed[0]["cursor"].as_u64());
    rig.stop();
}

#[test]
fn an_agent_changing_its_own_mode_is_reflected_in_the_read() {
    let rig = Rig::new("normal");
    rig.prompt("plan-yourself");
    rig.until(|v| {
        v["connection"]["state"] == "Resident" && v["blocks"].to_string().contains("plan.")
    });
    assert_eq!(rig.modes()["mode_observation"]["current_mode_id"], "plan");
    assert!(rig
        .journal()
        .iter()
        .any(|row| row["event"]["kind"] == "provider"
            && row["event"]["event"]["Signal"]["kind"]["kind"] == "mode-configured"));
    rig.stop();
}

#[test]
fn a_mode_change_waits_for_an_idle_session() {
    let rig = Rig::new("normal");
    rig.prompt("cancel");
    rig.until(|v| v["blocks"].to_string().contains("waiting for cancellation"));
    let read = rig.modes();
    assert_eq!(read["mode_controls"]["mode_selection"], false);
    assert!(read["mode_controls"]["reason"].is_string());
    assert_eq!(
        rig.service
            .apply(rig.select_mode("plan", "controlled-native"))
            .unwrap_err()
            .code(),
        "encounter.mode_selection_unavailable"
    );
    rig.service
        .apply(EncounterRequest::Cancel {
            agent_session: rig.session.clone(),
            reason: None,
        })
        .unwrap();
    rig.until(|v| v["connection"]["state"] == "Resident");
    rig.stop();
}

#[test]
fn lost_mode_ack_is_never_a_confirmed_receipt() {
    let rig = Rig::new("lost-mode-ack");
    assert!(rig
        .service
        .apply(rig.select_mode("plan", "controlled-native"))
        .is_err());
    let journal = rig.journal();
    let count = |kind: &str| {
        journal
            .iter()
            .filter(|row| row["event"]["kind"] == kind)
            .count()
    };
    assert_eq!(count("native-mode-configuration-requested"), 1);
    assert_eq!(count("native-mode-configuration-confirmed"), 0);
    rig.stop();
}

#[test]
fn the_configured_default_mode_is_requested_when_a_new_session_opens() {
    // Keyed by the provider's executable name: the controlled peer runs as python3.
    let rig = Rig::build(
        "normal",
        Some(serde_json::json!({"python3":"accept_edits","other":"plan"})),
    );
    assert_eq!(
        rig.modes()["mode_observation"]["current_mode_id"],
        "accept_edits"
    );
    let journal = rig.journal();
    let confirmed = journal
        .iter()
        .find(|row| row["event"]["kind"] == "native-mode-configuration-confirmed")
        .expect("default mode confirmed");
    assert_eq!(
        confirmed["event"]["origin"],
        serde_json::json!({"setting_ref":"ai-kit:permissions:permissions.default-mode","harness":"python3"})
    );
    // The binding records the mode the session opened in, before the default.
    let binding = journal
        .iter()
        .find(|row| row["event"]["kind"] == "binding")
        .unwrap();
    assert_eq!(
        binding["event"]["mode_observation"]["current_mode_id"],
        "default"
    );
    rig.stop();

    // A default the harness does not advertise is recorded, never forced.
    let rig = Rig::build(
        "normal",
        Some(serde_json::json!({"controlled":"bypassPermissions"})),
    );
    assert_eq!(
        rig.modes()["mode_observation"]["current_mode_id"],
        "default"
    );
    let journal = rig.journal();
    let refused = journal
        .iter()
        .find(|row| row["event"]["kind"] == "native-mode-default-not-applied")
        .expect("unadvertised default recorded");
    assert!(refused["event"]["reason"]
        .as_str()
        .unwrap()
        .contains("does not advertise mode bypassPermissions"));
    assert!(!journal
        .iter()
        .any(|row| row["event"]["kind"] == "native-mode-configuration-requested"));
    rig.stop();
}

#[test]
fn providers_and_views_name_the_harness_from_its_launch_not_its_label() {
    let rig = Rig::new("normal");
    let providers = rig.service.apply(EncounterRequest::Providers).unwrap();
    assert_eq!(
        providers,
        serde_json::json!([{
            "id": "controlled",
            "label": "Controlled protocol peer (not a model)",
            "protocol": "acp",
            "command": "python3",
            "entry": "agent_session_protocol.py",
            "sandboxed": false,
            "body_ref": null,
            "body_revision": null
        }])
    );
    let provider = &rig.view()["connection"]["provider"];
    assert_eq!(provider["command"], "python3");
    assert_eq!(provider["entry"], "agent_session_protocol.py");
    assert_eq!(provider["protocol"], "acp");
    assert_eq!(provider["sandboxed"], false);
    let status = rig
        .service
        .apply(EncounterRequest::Status {
            agent_session: rig.session.clone(),
        })
        .unwrap();
    assert_eq!(status["provider"]["command"], "python3");
    // No argv element other than basenames ever leaves.
    assert!(!providers.to_string().contains("tests/support"));
    rig.stop();
}

#[test]
fn launch_facts_see_through_a_declared_sandbox_and_expose_only_basenames() {
    use aikit_cli::encounter_service::{provider_launch_facts, EncounterProtocol};
    let argv: Vec<String> = [
        "/usr/bin/sandbox-exec",
        "-f",
        "/private/tmp/x/confinement.sb",
        "/Users/someone/.local/bin/pi",
        "--mode",
        "rpc",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        provider_launch_facts(EncounterProtocol::PiRpc, &argv),
        serde_json::json!({"protocol":"pi-rpc","command":"pi","entry":null,"sandboxed":true})
    );
    let node: Vec<String> = [
        "/opt/homebrew/bin/node",
        "/Users/someone/work/agent/index.js",
        "--token-file",
        "/secret/x",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        provider_launch_facts(EncounterProtocol::Acp, &node),
        serde_json::json!({"protocol":"acp","command":"node","entry":"index.js","sandboxed":false})
    );
}

#[test]
fn configured_model_default_selects_and_confirms_the_native_model() {
    let rig = Rig::settings(
        "normal",
        None,
        Some(serde_json::json!({"controlled":{"model_id":"test/b","model_name":"B"}})),
        false,
    );
    let opened = rig.service.apply(rig.open(false)).unwrap();
    assert_eq!(opened["model_observation"]["current_model_id"], "test/b");
    assert_eq!(
        rig.model()["model_observation"]["current_model_id"],
        "test/b"
    );
    assert!(rig
        .journal()
        .iter()
        .any(|row| row["event"]["kind"] == "native-model-default-confirmed"));
    aikit_cli::model_defaults::write(
        &rig.home,
        &aikit_cli::model_defaults::from_value(
            &serde_json::json!({"controlled":{"model_id":"test/a"}}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        rig.model()["model_observation"]["current_model_id"],
        "test/b",
        "changing a preference must not mutate the resident"
    );
    assert!(!rig
        .journal()
        .iter()
        .any(|row| row["event"]["kind"] == "prompt"));
    rig.stop();
}

#[test]
fn model_default_does_not_replace_the_model_on_native_resume() {
    let mut rig = Rig::settings(
        "retained-model",
        None,
        Some(serde_json::json!({"controlled":{"model_id":"test/b"}})),
        true,
    );
    assert_eq!(
        rig.model()["model_observation"]["current_model_id"],
        "test/b"
    );
    aikit_cli::model_defaults::write(
        &rig.home,
        &aikit_cli::model_defaults::from_value(
            &serde_json::json!({"controlled":{"model_id":"test/a"}}),
        )
        .unwrap(),
    )
    .unwrap();
    rig.stop();
    rig.service = Arc::new(EncounterService::new(rig.home.clone()).unwrap());
    let resumed = rig.service.apply(rig.open(true)).unwrap();
    assert_eq!(resumed["native_session_id"], "controlled-native");
    assert_eq!(
        rig.model()["model_observation"]["current_model_id"],
        "test/b"
    );
    assert_eq!(
        rig.journal()
            .iter()
            .filter(|row| row["event"]["kind"] == "native-model-default-confirmed")
            .count(),
        1
    );
    assert!(!rig
        .journal()
        .iter()
        .any(|row| row["event"]["kind"] == "prompt"));
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
        "encounter.native_open_recovery_required"
    );
    assert_eq!(
        service
            .apply(EncounterRequest::Shutdown {
                expected_pid: std::process::id(),
            })
            .unwrap_err()
            .code(),
        "encounter.shutdown_failed"
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
        "encounter.native_open_recovery_required"
    );
    let receipt = service
        .apply(EncounterRequest::ReconcileNativeOpen {
            agent_session: rig.session.clone(),
            expected_generation: "unfinished".into(),
            evidence_ref: ResourceRef::parse("evidence/native-cleanup").unwrap(),
            cleanup_confirmed: true,
        })
        .unwrap();
    assert_eq!(receipt["kind"], "native-open-reconciled");
    assert_eq!(receipt["connection_generation"], "unfinished");
    assert_eq!(receipt["evidence_ref"], "evidence/native-cleanup");
    assert_eq!(receipt["cleanup_confirmed"], true);
    assert_eq!(
        receipt["standing"],
        "operator-attestation; owner did not observe process exit"
    );
    for field in [
        "provider_success",
        "turn_replayed",
        "replacement_launched",
        "native_quiescence_observed",
        "verification_passed",
    ] {
        assert_eq!(receipt[field], false, "{field}: {receipt}");
    }
    let read = service
        .apply(EncounterRequest::Read {
            agent_session: rig.session.clone(),
            after: 0,
            limit: 100,
        })
        .unwrap();
    let events = read["events"].as_array().unwrap();
    let reservation = events
        .iter()
        .find(|row| row["event"]["kind"] == "native-open-reserved")
        .unwrap();
    let refusal = events
        .iter()
        .find(|row| row["event"]["kind"] == "native-open-refused")
        .unwrap();
    let reconciliations: Vec<_> = events
        .iter()
        .filter(|row| row["event"]["kind"] == "native-open-reconciled")
        .collect();
    assert_eq!(reconciliations.len(), 1);
    // Store adds its owner observation timestamp to the retained journal row.
    let mut retained_receipt = reconciliations[0]["event"].clone();
    let observed_at = retained_receipt
        .as_object_mut()
        .unwrap()
        .remove("observed_at_ms")
        .expect("the real journal adds its observation time");
    assert!(observed_at.as_i64().is_some());
    assert_eq!(retained_receipt, receipt);
    assert_eq!(reservation["event"]["connection_generation"], "unfinished");
    assert_eq!(refusal["event"]["connection_generation"], "unfinished");
    assert!(refusal["cursor"].as_u64().unwrap() > reservation["cursor"].as_u64().unwrap());
    assert!(reconciliations[0]["cursor"].as_u64().unwrap() > refusal["cursor"].as_u64().unwrap());
    let view = service
        .apply(EncounterRequest::View {
            agent_session: rig.session.clone(),
            before: None,
        })
        .unwrap();
    assert_eq!(view["connection"]["state"], "Disconnected");
}

#[test]
fn model_default_refuses_unadvertised_or_unconfirmed_selection() {
    for (mode, model) in [("normal", "not-advertised"), ("lost-model-ack", "test/b")] {
        let rig = Rig::settings(
            mode,
            None,
            Some(serde_json::json!({"controlled":{"model_id":model}})),
            false,
        );
        let failure = rig.service.apply(rig.open(false)).unwrap_err();
        assert!(!failure.message().is_empty());
        assert!(rig
            .journal()
            .iter()
            .any(|row| row["event"]["kind"] == "native-model-default-refused"
                && row["event"]["cleanup_confirmed"] == true));
        rig.stop();
    }
}

#[test]
#[ignore = "requires installed Pi; reads native state only, never sends a prompt"]
fn installed_pi_launch_default_is_confirmed_without_inference() {
    let model = std::env::var("AIKIT_TEST_PI_MODEL").expect("native observed Pi model ID");
    let provider = std::env::var("AIKIT_TEST_PI_PROVIDER").expect("native observed Pi provider");
    let executable = std::env::var("AIKIT_TEST_PI_BIN").unwrap_or_else(|_| "pi".into());
    let rig = Rig::settings(
        "normal",
        None,
        Some(serde_json::json!({"controlled":{"model_id":model,"native_provider":provider}})),
        false,
    );
    let native:EncounterProvider=serde_json::from_value(serde_json::json!({"id":"controlled","label":"Installed Pi","protocol":"pi-rpc","argv":[executable,"--mode","rpc","--no-session"]})).unwrap();
    EncounterService::configure(&rig.home, native).unwrap();
    let opened = rig.service.apply(rig.open(false)).unwrap();
    assert_eq!(opened["model_observation"]["current_model_id"], model);
    assert_eq!(opened["model_observation"]["native_provider"], provider);
    assert_eq!(opened["inference_observed"], false);
    let reading = rig.model();
    assert_eq!(
        reading["model_controls"]["model_selection"], false,
        "Pi is still launch-owned"
    );
    assert!(rig
        .journal()
        .iter()
        .any(|row| row["event"]["kind"] == "native-model-default-confirmed"));
    assert!(!rig
        .journal()
        .iter()
        .any(|row| row["event"]["kind"] == "prompt"));
    rig.stop();
}
