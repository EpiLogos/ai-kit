//! Controlled peer, real native store/handler/ACP/stdio. This is not live inference.
#![cfg(unix)]
use aikit_adapters::interactive_connection::PermissionDecision;
use aikit_cli::encounter_service::{EncounterProvider, EncounterRequest, EncounterService};
use aikit_core::{
    session_space::SessionSpaceRef,
    session_space_application::{SessionSpaceAgentAttachmentIntent, SessionSpaceMutation},
    ResourceRef,
};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::Value;
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

struct Rig {
    temp: tempfile::TempDir,
    home: AikitHome,
    service: EncounterService,
    space: SessionSpaceRef,
    session: ResourceRef,
}
impl Rig {
    fn new(mode: &str) -> Self {
        Self::build(mode, None)
    }
    fn build(mode: &str, default_modes: Option<serde_json::Value>) -> Self {
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
        let service = EncounterService::new(home.clone()).unwrap();
        let rig = Self {
            temp,
            home,
            service,
            space,
            session,
        };
        rig.service.apply(rig.open(false)).unwrap();
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
    rig.service = EncounterService::new(rig.home.clone()).unwrap();
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
            "sandboxed": false
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
