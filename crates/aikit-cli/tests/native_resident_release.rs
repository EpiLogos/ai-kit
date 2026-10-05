//! Actual installed Pi and Actuation owners in disposable ground. No prompt,
//! simulated provider reply or synthetic native/session binding is used.
#![cfg(unix)]
use aikit_adapters::agency_admission::{admit_agency, AgencySourceBasis};
use aikit_adapters::runner::SystemRunner;
use aikit_cli::encounter_service::{
    EncounterAgencyBinding, EncounterContextAdmission, EncounterRequiredSource,
};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::{ResourceRef, SourceRevision};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use aikit_core::resource::{
    CredentialCondition, DeclaredRoute, ModelCatalogueEntry, ModelRouteKind, ProviderRef, SourceRef,
};
use aikit_store::encounter::EncounterStore;
use std::time::{SystemTime, UNIX_EPOCH};
fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn rev(s: &str) -> SourceRevision {
    SourceRevision::parse(s).unwrap()
}
fn actuation() -> PathBuf {
    PathBuf::from(
        std::env::var_os("AIKIT_CAW_ACTUATION_BIN")
            .expect("Run with the pinned source-built native Actuation executable; no silent skip"),
    )
}
struct World {
    temp: tempfile::TempDir,
    home: AikitHome,
    socket: PathBuf,
    child: Option<Child>,
}
impl World {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("home"));
        let socket = temp.path().join("ipc/owner.sock");
        Self {
            temp,
            home,
            socket,
            child: None,
        }
    }
    fn cli(&self, args: &[String]) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
            .env("AIKIT_HOME", self.home.root())
            .arg("-C")
            .arg(self.temp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn request(&self, v: Value) -> Value {
        self.cli(&[
            "encounter".into(),
            "--socket".into(),
            self.socket.display().to_string(),
            "--request-json".into(),
            v.to_string(),
        ])
    }
    fn start(&mut self) {
        self.child = Some(
            Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
                .env("AIKIT_HOME", self.home.root())
                .arg("-C")
                .arg(self.temp.path())
                .args(["encounter-serve", "--socket"])
                .arg(&self.socket)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.socket.exists() {
            assert!(Instant::now() < deadline, "native owner did not start");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let ack = self.request(json!({"action":"shutdown","expected_pid":child.id()}));
            assert_eq!(ack["ok"], true, "{ack}");
            assert!(child.wait().unwrap().success());
        }
    }
    fn attach(&self, id: &str, _mode: &str) -> EncounterAgencyBinding {
        let space = SessionSpaceRef::parse(&format!("session-space/{id}")).unwrap();
        let session = r(&format!("agent-session/{id}"));
        let store = SessionSpaceApplicationStore::new(self.home.clone());
        store
            .apply(
                &store
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
        store
            .apply(
                &store
                    .stage(
                        Some(&space),
                        SessionSpaceMutation::AttachAgentSession {
                            attachment: SessionSpaceAgentAttachmentIntent {
                                agent_session: session.clone(),
                                purpose: Some("Controlled native test".into()),
                                provenance: vec!["explicit fixture".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        let path = self.temp.path().join(format!("{id}-agency.json"));
        let mut source: Value =
            serde_json::from_str(include_str!("fixtures/caw-agency-request.json")).unwrap();
        source["differentiated_binding"]["agent_ref"] = json!(format!("agent:{id}"));
        source["differentiated_binding"]["agency_ref"] = json!(format!("agency:{id}"));
        source["differentiated_binding"]["binding_ref"] = json!(format!("binding:{id}"));
        source["determination"]["differentiated_agency_ref"] = json!(format!("agency:{id}"));
        source["determination"]["world_binding_ref"] = json!(format!("binding:{id}"));
        let world_ref = if id == "root" {
            "central:root"
        } else {
            "central:project:Example"
        };
        source["differentiated_binding"]["world_ref"] = json!(world_ref);
        if id == "root" {
            source["differentiated_binding"]["scope_ref"] = json!("scope:root");
        }
        let bytes = serde_json::to_vec(&source).unwrap();
        fs::write(&path, &bytes).unwrap();
        let path = path.canonicalize().unwrap();
        let context = self.temp.path().join(format!("{id}-context.md"));
        let text = format!("SELECTED_CONTEXT_{id}\n");
        fs::write(&context, &text).unwrap();
        let context = context.canonicalize().unwrap();
        let binding = EncounterAgencyBinding {
            revision: rev("rev/1"),
            active: true,
            agent_ref: r(&format!("agent:{id}")),
            agency_ref: r(&format!("agency:{id}")),
            world_ref: r(world_ref),
            world_binding_ref: r(&format!("binding:{id}")),
            agency_source: AgencySourceBasis {
                source_ref: r(&format!("source/{id}")),
                revision: rev("rev/native-1"),
                path,
                content_digest: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
            },
            actuation_bin: actuation(),
            allowed_senders: [r("human:owner"), r("agent:sender")].into(),
            allowed_packet_sources: [r("source/shared")].into(),
            context: Some(EncounterContextAdmission {
                sources: vec![EncounterRequiredSource {
                    source: r(&format!("source/{id}-context")),
                    revision: rev("rev/context-1"),
                    path: context,
                    content_digest: format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex()),
                }],
                source_activations: vec![],
                projection: None,
                activation: None,
            }),
        };
        self.cli(&[
            "encounter-agency-configure".into(),
            "--agent-session".into(),
            session.to_string(),
            "--binding-json".into(),
            serde_json::to_string(&binding).unwrap(),
        ]);
        binding
    }
}
impl Drop for World {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let response = aikit_cli::encounter_service::request(
                &self.socket,
                &aikit_cli::encounter_service::EncounterRequest::Shutdown {
                    expected_pid: child.id(),
                },
            );
            if response.as_ref().is_ok_and(|value| value["ok"] == true) {
                let _ = child.wait();
            } else {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
fn publish_catalogue_fixture(w: &World, entry: &ModelCatalogueEntry) {
    // Actual documented authored catalogue source, loaded by the production
    // catalogue owner. This is not a hand-seeded Wiki/detection identity.
    let directory = w
        .home
        .root()
        .join(aikit_store::model_catalogue::MODEL_CATALOGUE_DIR);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("controlled.json"),
        serde_json::to_vec(&vec![entry]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        aikit_store::model_catalogue::resolved_catalogue(&w.home)
            .0
            .get(&entry.model),
        Some(entry)
    );
}

fn actual_pi_setup(w: &World) -> Value {
    let pi = std::env::var_os("AIKIT_CAW_PI_BIN")
        .map(PathBuf::from)
        .expect("Run with the exact installed Pi executable");
    assert_eq!(pi.file_name().and_then(|name| name.to_str()), Some("pi"));
    let mut binding = w.attach("root", "pi");
    let mut source: Value =
        serde_json::from_slice(&fs::read(&binding.agency_source.path).unwrap()).unwrap();
    source["determination"]["delegated_autonomy"]["allowed_action_refs"] =
        json!(["action/aikit/encounter-send", "action/aikit/model-realise"]);
    let bytes = serde_json::to_vec(&source).unwrap();
    fs::write(&binding.agency_source.path, &bytes).unwrap();
    binding.revision = rev("rev/2");
    binding.agency_source.revision = rev("rev/native-2");
    binding.agency_source.content_digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    w.cli(&[
        "encounter-agency-configure".into(),
        "--agent-session".into(),
        "agent-session/root".into(),
        "--binding-json".into(),
        serde_json::to_string(&binding).unwrap(),
        "--expected-revision".into(),
        "rev/1".into(),
    ]);
    publish_catalogue_fixture(
        w,
        &ModelCatalogueEntry {
            model: r("model:deepseek-v4-pro"),
            name: "DeepSeek V4 Pro".into(),
            description: "Exact native Pi selection receipt proof".into(),
            superseded_refs: Default::default(),
            routes: vec![DeclaredRoute {
                provider: ProviderRef::parse("provider:openrouter").unwrap(),
                kind: ModelRouteKind::ProviderNative,
                provider_native_ids: ["deepseek/deepseek-v4-pro".to_string()].into(),
                endpoint: None,
                credential: CredentialCondition::NotRequired,
            }],
            source: SourceRef::parse("source/actual-pi-selection-test").unwrap(),
            freshness: None,
            book: None,
        },
    );
    let policy = json!({
        "schema":"aikit.model-dispatch-policy/v1",
        "agent_ref":"agent:root",
        "world_ref":"central:root",
        "authority_ref":"authority:project:delegation",
        "bounds_refs":["bound:project:delegation"],
        "model_ref":"model:deepseek-v4-pro",
        "provider_ref":"provider:openrouter",
        "native_provider":"openrouter",
        "provider_native_id":"deepseek/deepseek-v4-pro",
        "expires_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 300_000,
        "credential":null,
    });
    let policy_path = w.temp.path().join("actual-pi-model-policy.json");
    let policy_bytes = serde_json::to_vec(&policy).unwrap();
    fs::write(&policy_path, &policy_bytes).unwrap();
    let provider = json!({
        "id":"root",
        "label":"Actual installed Pi, no inference",
        "protocol":"pi-rpc",
        "argv":[pi,"--mode","rpc","--no-extensions","--session-dir",w.temp.path().join("pi-sessions")],
        "model_policy":{
            "source":"source/actual-pi-model-policy",
            "revision":"rev/model-1",
            "path":policy_path,
            "content_digest":format!("blake3:{}",blake3::hash(&policy_bytes).to_hex()),
        }
    });
    w.cli(&[
        "encounter-configure".into(),
        "--provider-json".into(),
        provider.to_string(),
    ]);
    let admitted = admit_agency(
        &SystemRunner::new(),
        actuation().to_str().unwrap(),
        &binding.agency_source,
        &binding.agent_ref,
        &binding.world_ref,
    )
    .unwrap();
    json!({"action":"open-model","request":{
        "space":"session-space/root",
        "agent_session":"agent-session/root",
        "cwd":w.temp.path(),
        "model_ref":"model:deepseek-v4-pro",
        "provider_ref":"provider:openrouter",
        "body":"root",
        "expected_agency":admitted,
    }})
}

fn history(store: &EncounterStore) -> Value {
    let mut events = Vec::new();
    let mut cursor = 0;
    loop {
        let page = store.events(&r("agent-session/root"), cursor, 256).unwrap();
        cursor = page.next_cursor;
        events.extend(page.events);
        if !page.more {
            break;
        }
    }
    serde_json::to_value(events).unwrap()
}
fn owned_native_pids(w: &World) -> Vec<u32> {
    let parent = w.child.as_ref().expect("actual owner child").id();
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse().ok()?;
            let ppid: u32 = words.next()?.parse().ok()?;
            (ppid == parent).then_some(pid)
        })
        .collect()
}
fn assert_reaped(pids: &[u32]) {
    for pid in pids {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).trim().is_empty(),
            "exact owned native process {pid} remains after release"
        );
    }
}
fn value_ok(value: Value) -> Value {
    assert_eq!(value["ok"], true, "{value}");
    value["data"].clone()
}
fn release(_w: &World, opened: &Value) -> Value {
    json!({"action":"release-native","agent_session":"agent-session/root","expected_native_session_id":opened["native_session_id"],"expected_generation":opened["connection_generation"]})
}
fn predecessor(opened: &Value, receipt: &Value) -> Value {
    json!({"expected_native_session_id":opened["native_session_id"],"expected_generation":opened["connection_generation"],"release_cursor":receipt["terminal_cursor"]})
}
fn replace(target: &Value, predecessor: Value) -> Value {
    json!({"action":"replace-model","request":target["request"],"released_predecessor":predecessor})
}
fn hash(path: &Path) -> String {
    blake3::hash(&fs::read(path).unwrap()).to_hex().to_string()
}

#[test]
#[ignore = "requires exact installed Pi and pinned actual Actuation; no model prompt"]
fn actual_pi_idle_release_and_same_session_fresh_successor_preserve_history_and_retry() {
    let mut w = World::new();
    let target = actual_pi_setup(&w);
    w.start();
    let first = value_ok(w.request(target.clone()));
    assert_eq!(first["resident"], true);
    assert_eq!(first["inference_observed"], false);
    assert!(first["connection_generation"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    let bad = json!({"action":"release-native","agent_session":"agent-session/root","expected_native_session_id":first["native_session_id"],"expected_generation":"different-actual-generation-assertion"});
    assert_eq!(w.request(bad)["ok"], false);
    let store = EncounterStore::open(&w.home).unwrap();
    let retained_first = store
        .last_native_binding(&r("agent-session/root"))
        .unwrap()
        .unwrap();
    assert_eq!(
        retained_first["native_session_id"],
        first["native_session_id"]
    );
    let immutable_source = hash(&PathBuf::from(
        target["request"]["expected_agency"]["basis"]["path"]
            .as_str()
            .unwrap(),
    ));
    let pids = owned_native_pids(&w);
    assert_eq!(
        pids.len(),
        1,
        "actual isolated owner must have one admitted native body"
    );
    let request = release(&w, &first);
    let receipt = value_ok(w.request(request.clone()));
    assert_eq!(receipt["state"], "Released");
    assert_reaped(&pids);
    assert_eq!(receipt["receipt"]["cleanup_confirmed"], true);
    assert_eq!(receipt["receipt"]["native_resume"], false);
    assert_eq!(receipt["receipt"]["inference_observed"], false);
    let view = value_ok(w.request(json!({"action":"view","agent_session":"agent-session/root"})));
    assert_eq!(view["connection"]["state"], "Released");
    assert_eq!(view["connection"]["resident"], false);
    assert_eq!(
        w.request(target.clone())["ok"],
        false,
        "ordinary OpenModel semantics must not change"
    );
    let successor_request = replace(&target, predecessor(&first, &receipt));
    let second = value_ok(w.request(successor_request.clone()));
    assert_ne!(second["native_session_id"], first["native_session_id"]);
    assert_ne!(
        second["connection_generation"],
        first["connection_generation"]
    );
    assert_eq!(second["agent_session"], first["agent_session"]);
    assert_eq!(second["inference_observed"], false);
    let retained_second = store
        .last_native_binding(&r("agent-session/root"))
        .unwrap()
        .unwrap();
    assert_eq!(retained_second["continuation"], "fresh-native-successor");
    assert_eq!(
        retained_second["released_predecessor"],
        predecessor(&first, &receipt)
    );
    let before = history(&store);
    assert_eq!(
        value_ok(w.request(request.clone())),
        receipt,
        "old exact retry may not stop the successor"
    );
    assert_eq!(history(&store), before);
    let retry = value_ok(w.request(successor_request));
    assert_eq!(retry["native_session_id"], second["native_session_id"]);
    assert_eq!(
        retry["connection_generation"],
        second["connection_generation"]
    );
    let current =
        value_ok(w.request(json!({"action":"status","agent_session":"agent-session/root"})));
    assert_eq!(current["native_session_id"], second["native_session_id"]);
    assert_eq!(current["state"], "Resident");
    assert_eq!(
        hash(&PathBuf::from(
            target["request"]["expected_agency"]["basis"]["path"]
                .as_str()
                .unwrap()
        )),
        immutable_source
    );
    // Actual old-generation child output is refused atomically by the owner;
    // no fabricated return or current-generation success is appended.
    let before_late = history(&store);
    let late = store
        .append_child_message(
            &r("agent-session/root"),
            first["connection_generation"].as_str().unwrap(),
            &json!({"kind":"child-message","text":"late query-only negative assertion"}),
        )
        .unwrap_err();
    assert_eq!(late.code(), "encounter.child_message_generation_changed");
    assert_eq!(history(&store), before_late);
    let second_release = value_ok(w.request(release(&w, &second)));
    w.stop();
    w.start();
    assert_eq!(
        value_ok(w.request(request)),
        receipt,
        "restart readback retains old exact receipt"
    );
    let third = value_ok(w.request(replace(&target, predecessor(&second, &second_release))));
    assert_ne!(third["native_session_id"], second["native_session_id"]);
    assert_eq!(third["agent_session"], first["agent_session"]);
    assert_eq!(third["inference_observed"], false);
    value_ok(w.request(release(&w, &third)));
    w.stop();
}

#[test]
#[ignore = "requires exact installed Pi and pinned actual Actuation; actual pending native store work, no prompt"]
fn actual_pi_pending_work_prevents_release_and_an_unreleased_restart_cannot_recreate_successor() {
    let mut w = World::new();
    let target = actual_pi_setup(&w);
    w.start();
    let first = value_ok(w.request(target.clone()));
    let store = EncounterStore::open(&w.home).unwrap();
    // Exercise the actual durable queue owner, independently of any model
    // output. This reserves no Factory attempt and asserts no addressed ACK.
    store
        .queue_delivery(
            &r("agent-session/root"),
            &r("delivery/held-native-release-test"),
            &r("agent:sender"),
            &json!({"standing":"native-store queue admission regression; never dispatched"}),
        )
        .unwrap();
    let blocked = w.request(release(&w, &first));
    assert_eq!(blocked["ok"], false);
    let state =
        value_ok(w.request(json!({"action":"status","agent_session":"agent-session/root"})));
    assert_eq!(state["native_session_id"], first["native_session_id"]);
    store
        .refuse_queued_delivery(
            &r("agent-session/root"),
            &r("delivery/held-native-release-test"),
            "test.native_never_dispatched",
            "Controlled queue was never dispatched; retain actual refusal",
        )
        .unwrap();
    let first_release = value_ok(w.request(release(&w, &first)));
    let second = value_ok(w.request(replace(&target, predecessor(&first, &first_release))));
    w.stop();
    w.start();
    let before = history(&store);
    let refusal = w.request(replace(&target, predecessor(&first, &first_release)));
    assert_eq!(
        refusal["ok"], false,
        "journal alone must not reconstruct body ownership"
    );
    assert_eq!(
        store
            .last_native_binding(&r("agent-session/root"))
            .unwrap()
            .unwrap()["native_session_id"],
        second["native_session_id"]
    );
    assert_eq!(history(&store), before);
    w.stop();
}

#[test]
#[ignore = "requires exact installed Pi and pinned actual Actuation; concurrent public clients without prompts"]
fn concurrent_native_release_clients_retain_one_exact_cleanup_receipt() {
    let mut w = World::new();
    let target = actual_pi_setup(&w);
    w.start();
    let opened = value_ok(w.request(target.clone()));
    let request: aikit_cli::encounter_service::EncounterRequest =
        serde_json::from_value(release(&w, &opened)).unwrap();
    let socket = w.socket.clone();
    let bytes = serde_json::to_value(&request).unwrap();
    let other_socket = socket.clone();
    let worker = std::thread::spawn(move || {
        aikit_cli::encounter_service::request(&other_socket, &request).unwrap()
    });
    let first = w.request(bytes.clone());
    let second = worker.join().unwrap();
    assert!(
        first["ok"] == true || second["ok"] == true,
        "neither exact native release client completed: {first} / {second}"
    );
    let retained = value_ok(w.request(bytes));
    assert_eq!(retained["receipt"]["cleanup_confirmed"], true);
    let store = EncounterStore::open(&w.home).unwrap();
    let records = history(&store);
    let events = records.as_array().unwrap();
    let requested = events
        .iter()
        .filter(|event| event["event"]["kind"] == "native-release-requested")
        .count();
    let completed = events
        .iter()
        .filter(|event| event["event"]["kind"] == "native-release-completed")
        .count();
    assert_eq!(requested, 1);
    assert_eq!(completed, 1);
    let replaced = value_ok(w.request(replace(&target, predecessor(&opened, &retained))));
    assert_eq!(value_ok(w.request(release(&w, &opened))), retained);
    let current =
        value_ok(w.request(json!({"action":"status","agent_session":"agent-session/root"})));
    assert_eq!(current["native_session_id"], replaced["native_session_id"]);
    value_ok(w.request(release(&w, &replaced)));
    w.stop();
}
