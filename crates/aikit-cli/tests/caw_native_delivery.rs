//! Source-built AIKit binaries, actual Actuation owner, controlled ACP/Pi child
//! processes. Replies are explicitly protocol FIXTURES, never real-model proof.
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
    fn attach(&self, id: &str, mode: &str) -> EncounterAgencyBinding {
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
        let context = self.temp.path().join(format!("{id}-context.md"));
        let text = format!("SELECTED_CONTEXT_{id}\n");
        fs::write(&context, &text).unwrap();
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
        self.cli(&["encounter-configure".into(),"--provider-json".into(),json!({"id":id,"label":"Controlled fixture, not installed harness proof","protocol":if mode=="pi"{"pi-rpc"}else{"acp"},"argv":["python3","-u",Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/caw_provider.py"),mode,self.temp.path().join(format!("{id}.log"))]}).to_string()]);
        binding
    }
    fn open(&self, id: &str, reconnect: bool) -> Value {
        self.request(json!({"action":if reconnect{"reconnect"}else{"open"},"space":format!("session-space/{id}"),"agent_session":format!("agent-session/{id}"),"provider":id,"cwd":self.temp.path()}))
    }
    fn turn(&self, id: &str, delivery: &str, text: &str) -> Value {
        json!({"action":"send","agent_session":format!("agent-session/{id}"),"turn":{"delivery_ref":format!("delivery/{delivery}"),"sender":"agent:sender","expected_binding_revision":"rev/1","packet":{"text":text,"source_refs":["source/shared"],"audience":[format!("agent:{id}")]}}})
    }
    fn returned(&self, id: &str, d: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let v=self.request(json!({"action":"delivery","agent_session":format!("agent-session/{id}"),"delivery_ref":format!("delivery/{d}")}));
            assert_eq!(v["ok"], true, "{v}");
            if matches!(
                v["data"]["phase"].as_str(),
                Some("returned" | "failed" | "cancelled")
            ) {
                return v;
            }
            assert!(
                Instant::now() < deadline,
                "native response did not settle: {v}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    fn prompts(&self, id: &str) -> Vec<Value> {
        fs::read_to_string(self.temp.path().join(format!("{id}.log")))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|v| v["method"] == "session/prompt" || v["type"] == "prompt")
            .collect()
    }
}
impl Drop for World {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
#[test]
#[ignore = "requires pinned AIKIT_CAW_ACTUATION_BIN; executed by mandatory CAW native gate"]
fn native_binary_selected_context_duplicate_denial_and_reconnect() {
    let mut w = World::new();
    let binding = w.attach("one", "acp");
    w.attach("two", "acp");
    w.start();
    let open = w.open("one", false);
    assert_eq!(open["ok"], true, "{open}");
    let message = w.turn("one", "first", "CONTROLLED_SLOW request");
    let sent = w.request(message.clone());
    assert_eq!(sent["ok"], true, "{sent}");
    assert_eq!(w.returned("one", "first")["data"]["phase"], "returned");
    assert_eq!(w.request(message.clone())["data"]["duplicate"], true);
    let p = w.prompts("one");
    assert_eq!(p.len(), 1);
    let prompt = p[0]["params"]["prompt"].to_string();
    assert!(prompt.contains("SELECTED_CONTEXT_one"));
    assert!(!prompt.contains("SELECTED_CONTEXT_two"));
    assert!(prompt.contains("agent:one"));
    let mut denied = w.turn("one", "denied", "not sent");
    denied["turn"]["packet"]["source_refs"] = json!(["source/private"]);
    assert_eq!(w.request(denied)["ok"], false);
    assert_eq!(w.prompts("one").len(), 1);
    w.stop();
    w.start();
    assert_eq!(
        w.request(message.clone())["data"]["duplicate"],
        true,
        "readback survives owner restart"
    );
    assert_eq!(
        w.open("one", false)["error"]["code"],
        "encounter.resume_required"
    );
    let again = w.open("one", true);
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(
        open["data"]["native_session_id"],
        again["data"]["native_session_id"]
    );
    let history = w.request(
        json!({"action":"read","agent_session":"agent-session/one","after":0,"limit":200}),
    );
    assert!(
        history.to_string().contains("FIXTURE_REPLAY_BEFORE_LOAD"),
        "load replay must survive the response boundary: {history}"
    );
    assert_eq!(w.request(w.turn("one", "second", "continued"))["ok"], true);
    assert_eq!(w.returned("one", "second")["data"]["phase"], "returned");
    fs::write(&binding.agency_source.path, b"revoked source bytes").unwrap();
    assert_eq!(w.request(w.turn("one", "revoked", "not sent"))["ok"], false);
    assert_eq!(w.prompts("one").len(), 2);
    w.stop();
}
#[test]
#[ignore = "requires pinned AIKIT_CAW_ACTUATION_BIN; executed by mandatory CAW native gate"]
fn native_group_preflight_privacy_actual_responses_and_pi_limits() {
    let mut w = World::new();
    w.attach("one", "acp");
    w.attach("two", "pi");
    w.start();
    for id in ["one", "two"] {
        let v = w.open(id, false);
        assert_eq!(v["ok"], true, "{v}");
    }
    let group = json!({"action":"send-group","delivery_ref":"delivery/group","sender":"agent:sender","packet":{"text":"same explicitly shared request","source_refs":["source/shared"],"audience":["agent:one","agent:two"]},"recipients":[{"agent_session":"agent-session/one","expected_binding_revision":"rev/1"},{"agent_session":"agent-session/two","expected_binding_revision":"rev/1"}]});
    let mut denied = group.clone();
    denied["packet"]["source_refs"] = json!(["source/private"]);
    assert_eq!(w.request(denied)["ok"], false);
    assert!(w.prompts("one").is_empty());
    assert!(w.prompts("two").is_empty());
    let v = w.request(group);
    assert_eq!(v["ok"], true, "{v}");
    for id in ["one", "two"] {
        assert_eq!(w.returned(id, "group")["data"]["phase"], "returned");
        assert_eq!(w.prompts(id).len(), 1);
    }
    let pi = w.prompts("two")[0]["message"].as_str().unwrap().to_owned();
    assert!(pi.contains("SELECTED_CONTEXT_two"));
    assert!(!pi.contains("SELECTED_CONTEXT_one"));
    w.stop();
    w.start();
    assert_eq!(
        w.open("two", true)["error"]["code"],
        "encounter.reconnect_unsupported"
    );
    w.stop();
}
#[test]
#[ignore = "requires pinned AIKIT_CAW_ACTUATION_BIN; executed by mandatory CAW native gate"]
fn native_owner_rejects_unauthorised_grant_and_stale_source() {
    let w = World::new();
    let binding = w.attach("one", "acp");
    let mut v: Value =
        serde_json::from_slice(&fs::read(&binding.agency_source.path).unwrap()).unwrap();
    v["metagency_grant"]["bounds_refs"] = json!(["bound:other"]);
    let bytes = serde_json::to_vec(&v).unwrap();
    fs::write(&binding.agency_source.path, &bytes).unwrap();
    assert_eq!(
        admit_agency(
            &SystemRunner::new(),
            &actuation().to_string_lossy(),
            &binding.agency_source,
            &binding.agent_ref,
            &binding.world_ref
        )
        .unwrap_err()
        .code(),
        "agency_admission.stale"
    );
    let mut fresh = binding.agency_source;
    fresh.content_digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    assert_eq!(
        admit_agency(
            &SystemRunner::new(),
            &actuation().to_string_lossy(),
            &fresh,
            &binding.agent_ref,
            &binding.world_ref
        )
        .unwrap_err()
        .code(),
        "agency_admission.denied"
    );
}

#[test]
#[ignore = "requires pinned AIKIT_CAW_ACTUATION_BIN; executed by mandatory CAW native gate"]
fn native_provider_denial_and_disconnect_do_not_authorise_replay() {
    let mut w = World::new();
    w.attach("one", "acp");
    w.start();
    assert_eq!(w.open("one", false)["ok"], true);
    let denied = w.turn("one", "provider-denial", "CONTROLLED_DENIAL");
    assert_eq!(w.request(denied.clone())["ok"], true);
    assert_eq!(
        w.returned("one", "provider-denial")["data"]["phase"],
        "failed"
    );
    assert_eq!(w.request(denied)["data"]["duplicate"], true);
    assert_eq!(w.prompts("one").len(), 1);
    let disconnected = w.turn("one", "lost", "CONTROLLED_DISCONNECT");
    assert_eq!(w.request(disconnected.clone())["ok"], true);
    let settled = w.returned("one", "lost");
    assert_ne!(settled["data"]["phase"], "returned");
    assert_eq!(w.request(disconnected.clone())["data"]["duplicate"], true);
    assert_eq!(w.prompts("one").len(), 2);
    assert_eq!(
        w.request(w.turn("one", "unsafe-retry", "must not reach failed transport"))["ok"],
        false
    );
    w.stop();
    w.start();
    assert_eq!(w.request(disconnected)["data"]["duplicate"], true);
    assert_eq!(w.prompts("one").len(), 2);
    w.stop();
}

#[test]
#[ignore = "requires pinned AIKIT_CAW_ACTUATION_BIN; executed by mandatory CAW native gate"]
fn native_root_agency_requires_no_project_profile_or_adoption() {
    let mut w = World::new();
    w.attach("root", "acp");
    w.start();
    assert_eq!(w.open("root", false)["ok"], true);
    assert_eq!(
        w.request(w.turn("root", "root-turn", "root-scoped request"))["ok"],
        true
    );
    assert_eq!(w.returned("root", "root-turn")["data"]["phase"], "returned");
    let text = w.prompts("root")[0]["params"]["prompt"].to_string();
    assert!(text.contains("World: central:root"));
    assert!(text.contains("Scope: scope:root"));
    assert!(!w.temp.path().join("ProjectCentral").exists());
    assert!(!w.temp.path().join("Control").exists());
    w.stop();
}
