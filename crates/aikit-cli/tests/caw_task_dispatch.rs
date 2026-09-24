//! Joined production owner path. Real disposable World, source-built native
//! owners and actual ACP process; no commercial-model or installed proof.
#![cfg(unix)]
use aikit_adapters::agency_admission::AgencySourceBasis;
use aikit_cli::encounter_service::{
    EncounterAgencyBinding, EncounterContextAdmission, EncounterRequiredSource,
};
use aikit_core::resource::{
    CredentialCondition, DeclaredRoute, ModelCatalogueEntry, ModelRouteKind, ProviderRef, SourceRef,
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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn rev(s: &str) -> SourceRevision {
    SourceRevision::parse(s).unwrap()
}
struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: AikitHome,
    socket: PathBuf,
    child: Option<Child>,
}
impl World {
    fn new(allowed: bool) -> Self {
        Self::with_model_action(allowed, false)
    }
    fn with_model_action(allowed: bool, model_action: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = AikitHome::at(root.join("home"));
        let socket = root.join("ipc/owner.sock");
        let world = Self {
            _temp: temp,
            root,
            home,
            socket,
            child: None,
        };
        for p in ["Control/user", "Control/relations", "Work/demo/src"] {
            fs::create_dir_all(world.root.join(p)).unwrap();
        }
        fs::write(world.root.join("Control/user/human.md"), "HUMAN_UNCHANGED").unwrap();
        let policy_path = "Control/user/placement.json";
        let policy_ref = format!("central:source:control:root:{policy_path}");
        fs::write(world.root.join(policy_path), json!({"schema":"central.work-placement-policy/v1",
            "scope_ref":"control:root", "writable":[{"path":"Work/demo","class":"repository"}], "protected":[],
            "enforcement":"material-filesystem", "required_coverage":["file-content","file-creation","file-removal","rename-link","truncate","descendant-processes"], "lease_seconds":300}).to_string()).unwrap();
        fs::write(world.root.join("Control/relations/source-relations.json"), json!({"schema":"central.control.ground-relations/v1",
            "project_id":"control:root", "relations":[{"ref":policy_ref,"path":policy_path,"roles":["work-placement-policy"],
            "provenance":"human-adopted","standing":"architecture-contract","treatment":"projectcentral-user",
            "recognition":"controlled-test-only","recorded_at_unix_seconds":1}]}).to_string()).unwrap();
        let space = SessionSpaceRef::parse("session-space/task").unwrap();
        let store = SessionSpaceApplicationStore::new(world.home.clone());
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
                                agent_session: r("agent-session/task"),
                                purpose: Some("Controlled task".into()),
                                provenance: vec!["native-test".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        let mut source: Value =
            serde_json::from_str(include_str!("fixtures/caw-agency-request.json")).unwrap();
        source["differentiated_binding"]["world_ref"] = json!("control:root");
        if allowed {
            source["determination"]["delegated_autonomy"]["allowed_action_refs"] = if model_action {
                json!([
                    "action/aikit/encounter-send",
                    "action/aikit/encounter-task",
                    "action/aikit/model-realise"
                ])
            } else {
                json!(["action/aikit/encounter-send", "action/aikit/encounter-task"])
            };
        }
        let path = world.root.join("agency.json");
        let bytes = serde_json::to_vec(&source).unwrap();
        fs::write(&path, &bytes).unwrap();
        let context = world.root.join("context.md");
        fs::write(&context, "SELECTED_CONTEXT").unwrap();
        let binding = EncounterAgencyBinding {
            revision: rev("rev/1"),
            active: true,
            agent_ref: r("agent:existing-1"),
            agency_ref: r("agency:project:delegation"),
            world_ref: r("control:root"),
            world_binding_ref: r("binding:project:delegation"),
            agency_source: AgencySourceBasis {
                source_ref: r("source/agency"),
                revision: rev("source/1"),
                path,
                content_digest: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
            },
            actuation_bin: PathBuf::from(
                std::env::var_os("AIKIT_CAW_ACTUATION_BIN").expect("native Actuation required"),
            ),
            allowed_senders: [r("agent:sender")].into(),
            allowed_packet_sources: [r("source/shared")].into(),
            context: Some(EncounterContextAdmission {
                sources: vec![EncounterRequiredSource {
                    source: r("source/context"),
                    revision: rev("source/1"),
                    path: context,
                    content_digest: format!(
                        "blake3:{}",
                        blake3::hash(b"SELECTED_CONTEXT").to_hex()
                    ),
                }],
                source_activations: vec![],
                projection: None,
                activation: None,
            }),
        };
        world.cli(&[
            "encounter-agency-configure".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
            "--binding-json".into(),
            serde_json::to_string(&binding).unwrap(),
        ]);
        world
    }
    fn command(&self, args: &[String]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
            .env("AIKIT_HOME", self.home.root())
            .env("WORKCELL_CONTROL_TOKEN", "controlled-caw-material-token")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap()
    }
    fn cli(&self, args: &[String]) -> Value {
        let o = self.command(args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
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
    fn prepare_input(&self) -> Value {
        json!({"central":{"ctrl_bin":std::env::var("AIKIT_CAW_CTRL_BIN").expect("native Central required"),"central_root":self.root,
            "project":null,"task_ref":"task:native-joined","purpose":"Native protected task","participant_refs":["agent:existing-1"],"source_refs":["source/agency"]},
            "provider":{"id":"controlled-task","label":"Controlled native, not model","protocol":"acp","argv":["python3","-u",Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/caw_task_provider.py"), self.root.join("Work/demo/src/protocol.log"),self.root.join("Control/user/human.md"), self.root.join("Work/loose.txt")]},
            "cwd":self.root.join("Work/demo/src"),"selected_directories":[self.root.join("Work/demo/src")],
            "workcell_boundary_bin":std::env::var("AIKIT_CAW_WORKCELL_BOUNDARY_BIN").expect("native Workcell required"),"authority_ref":"authority:project:delegation"})
    }
    fn prepare(&self) -> Value {
        self.cli(&[
            "encounter-task-configure".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
            "--request-json".into(),
            self.prepare_input().to_string(),
        ])
    }
    fn attach_second_session_with_same_native_agency(&self) {
        let space = SessionSpaceRef::parse("session-space/task").unwrap();
        let store = SessionSpaceApplicationStore::new(self.home.clone());
        store
            .apply(
                &store
                    .stage(
                        Some(&space),
                        SessionSpaceMutation::AttachAgentSession {
                            attachment: SessionSpaceAgentAttachmentIntent {
                                agent_session: r("agent-session/other"),
                                purpose: Some("Native shared-history boundary".into()),
                                provenance: vec!["native-test".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        let existing = self.home.state().join("encounter-agencies").join(format!(
            "{}.json",
            blake3::hash(b"agent-session/task").to_hex()
        ));
        let actual_binding: Value = serde_json::from_slice(&fs::read(existing).unwrap()).unwrap();
        self.cli(&[
            "encounter-agency-configure".into(),
            "--agent-session".into(),
            "agent-session/other".into(),
            "--binding-json".into(),
            actual_binding.to_string(),
        ]);
    }
    fn start(&mut self) {
        self.start_with_pi_config_ambient(None);
    }
    fn start_with_pi_config_ambient(&mut self, ambient_pi_config: Option<&Path>) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
        if let Some(path) = ambient_pi_config {
            command.env("PI_CODING_AGENT_DIR", path);
        }
        self.child = Some(
            command
                .env("AIKIT_HOME", self.home.root())
                .env("WORKCELL_CONTROL_TOKEN", "controlled-caw-material-token")
                .env("CENTRAL_NATIVE_TOKEN", "CONTROLLED_MUST_NOT_REACH_PROVIDER")
                .arg("-C")
                .arg(&self.root)
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
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn open(&self, record: &Value, cwd: &Path) -> Value {
        self.request(json!({"action":"open","space":"session-space/task","agent_session":"agent-session/task","provider":record["launcher"]["id"],"cwd":cwd}))
    }
    fn expected_task(&self) -> Value {
        let record = self.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
        ]);
        let source = fs::read(self.root.join("agency.json")).unwrap();
        json!({"revision":record["revision"],"task_ref":record["request"]["central"]["task_ref"],
            "now_ref":record["allocation"]["allocation"]["now_ref"],"now_revision":record["allocation"]["allocation"]["revision"]["revision"],
            "policy_revision":record["allocation"]["allocation"]["policy"]["revision"],"cwd":record["request"]["cwd"],
            "agent_ref":"agent:existing-1","agency_ref":"agency:project:delegation","world_binding_ref":"binding:project:delegation",
            "source_ref":"source/agency","source_revision":"source/1","source_digest":format!("blake3:{}",blake3::hash(&source).to_hex())})
    }
    fn send_expected(&self, id: &str, expected: Value) -> Value {
        self.request(json!({"action":"send","agent_session":"agent-session/task","turn":{
            "delivery_ref":format!("delivery/{id}"),"sender":"agent:sender","expected_binding_revision":"rev/1",
            "expected_task":expected,"packet":{"text":"Do the bounded native test","source_refs":["source/shared"],"audience":["agent:existing-1"]}}}))
    }
    fn send(&self, id: &str) -> Value {
        self.send_expected(id, self.expected_task())
    }
    fn returned(&self) -> Value {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            let v=self.request(json!({"action":"delivery","agent_session":"agent-session/task","delivery_ref":"delivery/one"}));
            if v["data"]["phase"] == "returned" {
                return v;
            }
            assert!(Instant::now() < end, "{v}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn prepare_actual_pi_task(world: &World) -> Value {
    let pi = PathBuf::from(std::env::var_os("AIKIT_CAW_PI_BIN").expect("actual Pi required"));
    assert_eq!(pi.file_name().and_then(|name| name.to_str()), Some("pi"));
    let entry = ModelCatalogueEntry {
        model: r("model:north-mini-code"),
        name: "North Mini Code free".into(),
        description: "Actual Pi selected-model startup without inference".into(),
        superseded_refs: Default::default(),
        routes: vec![DeclaredRoute {
            provider: ProviderRef::parse("provider:openrouter").unwrap(),
            kind: ModelRouteKind::ProviderNative,
            provider_native_ids: ["cohere/north-mini-code:free".to_string()].into(),
            endpoint: None,
            credential: CredentialCondition::NotRequired,
        }],
        source: SourceRef::parse("source/actual-pi-task-selection-test").unwrap(),
        freshness: None,
        book: None,
    };
    let catalogue = world
        .home
        .root()
        .join(aikit_store::model_catalogue::MODEL_CATALOGUE_DIR);
    fs::create_dir_all(&catalogue).unwrap();
    fs::write(
        catalogue.join("actual-pi-task.json"),
        serde_json::to_vec(&vec![&entry]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        aikit_store::model_catalogue::resolved_catalogue(&world.home)
            .0
            .get(&entry.model),
        Some(&entry)
    );
    let policy = json!({
        "schema":"aikit.model-dispatch-policy/v1",
        "agent_ref":"agent:existing-1",
        "world_ref":"control:root",
        "authority_ref":"authority:project:delegation",
        "bounds_refs":["bound:project:delegation"],
        "model_ref":"model:north-mini-code",
        "provider_ref":"provider:openrouter",
        "native_provider":"openrouter",
        "provider_native_id":"cohere/north-mini-code:free",
        "expires_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 300_000,
        "credential":null,
    });
    let policy_path = world.root.join("actual-pi-task-policy.json");
    let policy_bytes = serde_json::to_vec(&policy).unwrap();
    fs::write(&policy_path, &policy_bytes).unwrap();
    let mut request = world.prepare_input();
    request["provider"] = json!({
        "id":"native-pi-task",
        "label":"Actual Pi RPC protected task startup",
        "protocol":"pi-rpc",
        "argv":[pi,"--mode","rpc","--no-extensions","--session-dir",
            world.root.join("Work/demo/src/pi-sessions")],
        "model_policy":{
            "source":"source/actual-pi-task-policy",
            "revision":"rev/actual-pi-task-policy-1",
            "path":policy_path,
            "content_digest":format!("blake3:{}", blake3::hash(&policy_bytes).to_hex()),
        }
    });
    let prepared = world.cli(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        request.to_string(),
    ]);
    assert_eq!(prepared["ready"], true);
    world.cli(&[
        "encounter-configure".into(),
        "--provider-json".into(),
        request["provider"].to_string(),
    ]);
    prepared
}

#[test]
#[ignore = "requires source-built Central, Workcell, Actuation and actual pinned Pi; mandatory CAW lane"]
fn real_pi_task_startup_uses_allocated_now_instead_of_ambient_config() {
    let mut world = World::with_model_action(true, true);
    let ambient = world.root.join("ambient-outside-task");
    let prepared = prepare_actual_pi_task(&world);
    world.start_with_pi_config_ambient(Some(&ambient));
    let opened = world.open(&prepared, &world.root.join("Work/demo/src"));
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(opened["data"]["inference_observed"], false);
    assert_eq!(opened["data"]["protocol"], "pi-rpc");
    assert_eq!(opened["data"]["body_basis"]["harness_profile"], "pi");
    assert_eq!(
        opened["data"]["model_observation"]["current_model_id"], "cohere/north-mini-code:free",
        "the native Pi get_state must confirm the selected free model"
    );
    assert!(
        opened["data"]["native_session_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "the real Pi protocol must open an identified native session"
    );
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    assert!(prepared["requirements"]["writable_paths"]
        .as_array()
        .unwrap()
        .contains(&json!(now)));
    assert!(now.join("pi-agent").is_dir());
    assert!(
        !ambient.exists(),
        "ambient Pi config escaped the task grant"
    );
    println!("ACTUAL_PI_TASK_STARTUP_INSIDE_NATIVE_NOW_WORKCELL");
}

#[test]
#[ignore = "requires source-built Central, Workcell, Actuation and actual pinned Pi; mandatory CAW lane"]
fn real_pi_task_refuses_redirected_allocated_config_directory() {
    let mut world = World::with_model_action(true, true);
    let prepared = prepare_actual_pi_task(&world);
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let outside = world.root.join("outside-task-pi-config");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, now.join("pi-agent")).unwrap();
    world.start();
    let refused = world.open(&prepared, &world.root.join("Work/demo/src"));
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}
impl Drop for World {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Shut down via the native owner so it stops its protocol children.
            let _ = self.command(&[
                "encounter".into(),
                "--socket".into(),
                self.socket.display().to_string(),
                "--request-json".into(),
                json!({"action":"shutdown","expected_pid":child.id()}).to_string(),
            ]);
            let end = Instant::now() + Duration::from_secs(5);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
#[ignore = "requires source-built Central, Workcell with Landlock and Actuation; mandatory CAW lane"]
fn real_task_dispatch_confines_protocol_and_rechecks_source_without_duplicate_work() {
    let mut w = World::new(true);
    let prepared = w.prepare();
    assert_eq!(prepared["ready"], true);
    w.cli(&[
        "encounter-configure".into(),
        "--provider-json".into(),
        w.prepare_input()["provider"].to_string(),
    ]);
    w.start();
    let alternate = w.request(
        json!({"action":"open","space":"session-space/task","agent_session":"agent-session/task",
        "provider":"controlled-task","cwd":w.root.join("Work/demo/src")}),
    );
    assert_eq!(alternate["ok"], false);
    assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    assert_eq!(w.open(&prepared, &w.root.join("Work/demo/src"))["ok"], true);
    let before = fs::read(w.root.join("Work/demo/src/protocol.log")).unwrap();
    for field in [
        "revision",
        "task_ref",
        "now_ref",
        "now_revision",
        "policy_revision",
        "cwd",
        "agent_ref",
        "agency_ref",
        "world_binding_ref",
        "source_ref",
        "source_revision",
        "source_digest",
    ] {
        let mut wrong = w.expected_task();
        wrong[field] = json!("different/basis");
        assert_eq!(w.send_expected("one", wrong)["ok"], false, "{field}");
        assert_eq!(
            fs::read(w.root.join("Work/demo/src/protocol.log")).unwrap(),
            before
        );
    }
    assert_eq!(w.send("one")["ok"], true);
    let returned = w.returned();
    assert_eq!(
        returned["data"]["request"]["submission"]["turn"]["expected_task"],
        w.expected_task()
    );
    let evidence: Value =
        serde_json::from_slice(&fs::read(w.root.join("Work/demo/src/result.json")).unwrap())
            .unwrap();
    assert_eq!(evidence["denied"], json!([true, true]));
    assert_eq!(evidence["selected_context"], true);
    assert_eq!(evidence["central_token_present"], false);
    assert_eq!(evidence["cwd"], json!(w.root.join("Work/demo/src")));
    assert_eq!(
        fs::read_to_string(w.root.join("Control/user/human.md")).unwrap(),
        "HUMAN_UNCHANGED"
    );
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(now.join("return.txt")).unwrap(),
        "ACTUAL_TASK_RETURN"
    );
    let log = w.root.join("Work/demo/src/protocol.log");
    let before = fs::read(&log).unwrap();
    assert_eq!(w.send("one")["data"]["duplicate"], true);
    assert_eq!(fs::read(&log).unwrap(), before);
    fs::write(w.root.join("Control/user/placement.json"), "{}").unwrap();
    assert_eq!(w.send("two")["ok"], false);
    assert_eq!(fs::read(&log).unwrap(), before);
    assert_eq!(
        w.returned()["data"]["delivery_ref"],
        returned["data"]["delivery_ref"]
    );
    println!("TASK_NATIVE_PROTECTED_DISPATCH_EXECUTED");
}
#[test]
#[ignore = "requires exact native owners; mandatory CAW lane"]
fn wrong_actual_cwd_and_removed_now_cannot_launch_a_provider() {
    for remove in [false, true] {
        let mut w = World::new(true);
        let prepared = w.prepare();
        if remove {
            fs::remove_file(
                w.root.join(
                    prepared["allocation"]["allocation"]["source"]["path"]
                        .as_str()
                        .unwrap(),
                ),
            )
            .unwrap();
        }
        w.start();
        let cwd = if remove {
            w.root.join("Work/demo/src")
        } else {
            w.root.clone()
        };
        assert_eq!(w.open(&prepared, &cwd)["ok"], false);
        assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    }
}
#[test]
#[ignore = "requires exact native owners; mandatory CAW lane"]
fn missing_task_authority_refuses_before_now_allocation() {
    let w = World::new(false);
    let result = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        w.prepare_input().to_string(),
    ]);
    assert!(!result.status.success());
    assert!(!w.root.join("Control/agents/now").exists());
}

#[test]
#[ignore = "requires exact source-built Central, Workcell and Actuation; mandatory CAW lane"]
fn refused_central_source_amendment_keeps_ready_task_and_same_now() {
    let w = World::new(true);
    let ready = w.prepare();
    let before = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(before, ready);
    let mut amended = w.prepare_input();
    amended["central"]["source_refs"] = json!(["source/agency", "source/new-skill"]);
    let refused = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        amended.to_string(),
        "--expected-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("allocation identity is immutable"));
    let after = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(after, before, "refusal must not publish pending over ready");
    let retried = w.cli(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        w.prepare_input().to_string(),
        "--expected-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert_eq!(retried["ready"], true);
    assert_ne!(retried["revision"], ready["revision"]);
    assert_eq!(
        retried["allocation"]["allocation"]["now_ref"],
        ready["allocation"]["allocation"]["now_ref"]
    );
    assert_eq!(
        retried["allocation"]["allocation"]["record"]["task_ref"],
        ready["allocation"]["allocation"]["record"]["task_ref"]
    );
}

#[test]
#[ignore = "requires exact source-built Central, Workcell and Actuation; mandatory CAW lane"]
fn unhosted_pending_abort_revalidates_ready_with_fresh_revision_and_stale_cas_refuses() {
    let mut w = World::new(true);
    let ready = w.prepare();
    let mut invalid = w.prepare_input();
    invalid["selected_directories"] =
        json!([w.root.join("Work/demo/src/missing-native-directory")]);
    let failed = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        invalid.to_string(),
        "--expected-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(
        !failed.status.success(),
        "a nonexistent selected directory must fail the actual owner boundary"
    );
    let pending = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(pending["ready"], false);
    assert_eq!(
        pending["request"]["central"]["task_ref"],
        ready["request"]["central"]["task_ref"]
    );
    w.attach_second_session_with_same_native_agency();
    let other_ready = w.cli(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/other".into(),
        "--request-json".into(),
        w.prepare_input().to_string(),
    ]);
    let other_failed = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/other".into(),
        "--request-json".into(),
        invalid.to_string(),
        "--expected-revision".into(),
        other_ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!other_failed.status.success());
    let foreign_history = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        other_ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(
        !foreign_history.status.success(),
        "another AgentSession's real native ready record is not this session's recovery target"
    );
    assert_eq!(
        w.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into()
        ]),
        pending
    );
    let history = w.home.state().join("encounter-tasks/history");
    let same_revision_history: Vec<Value> = fs::read_dir(&history)
        .unwrap()
        .map(|item| serde_json::from_slice(&fs::read(item.unwrap().path()).unwrap()).unwrap())
        .filter(|record: &Value| record["revision"] == ready["revision"])
        .collect();
    assert_eq!(
        same_revision_history
            .iter()
            .filter(|record| record["ready"] == true)
            .count(),
        1,
        "exactly one ready history reading may be restored"
    );
    assert!(
        same_revision_history
            .iter()
            .any(|record| record["ready"] == false),
        "the native pending journal legitimately shares its revision with ready"
    );
    let entry = fs::read_dir(&history)
        .unwrap()
        .map(|item| item.unwrap().path())
        .find(|path| {
            serde_json::from_slice::<Value>(&fs::read(path).unwrap()).is_ok_and(|record| {
                record["revision"] == ready["revision"] && record["ready"] == true
            })
        })
        .expect("the actual earlier ready record is retained in native history");
    let original = fs::read(&entry).unwrap();
    fs::write(&entry, b"corrupt native history bytes").unwrap();
    let corrupted = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!corrupted.status.success());
    fs::remove_file(&entry).unwrap();
    std::os::unix::fs::symlink(w.root.join("agency.json"), &entry).unwrap();
    let redirected = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!redirected.status.success());
    fs::remove_file(&entry).unwrap();
    fs::write(&entry, original).unwrap();
    let stale = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        ready["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!stale.status.success());
    assert_eq!(
        w.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into()
        ]),
        pending
    );
    let restored = w.cli(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert_eq!(restored["status"], "restored");
    let resumed = &restored["record"];
    assert_eq!(resumed["ready"], true);
    assert_ne!(resumed["revision"], ready["revision"]);
    assert_ne!(resumed["revision"], pending["revision"]);
    assert_eq!(
        resumed["allocation"]["allocation"]["now_ref"],
        ready["allocation"]["allocation"]["now_ref"]
    );
    assert_eq!(
        resumed["request"]["central"]["task_ref"],
        ready["request"]["central"]["task_ref"]
    );
    assert_eq!(
        resumed["launcher"]["argv"].as_array().unwrap().last(),
        Some(&resumed["revision"])
    );
    w.start();
    assert_eq!(w.open(resumed, &w.root.join("Work/demo/src"))["ok"], true);
    let repeated = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(
        !repeated.status.success(),
        "stale abort cannot overwrite the restored live body"
    );
}

#[test]
#[ignore = "requires exact source-built Central, Workcell and Actuation; mandatory CAW lane"]
fn expired_unhosted_ready_is_reprepared_with_same_native_now_and_fresh_lease() {
    let mut w = World::new(true);
    let policy_path = w.root.join("Control/user/placement.json");
    let mut policy: Value = serde_json::from_slice(&fs::read(&policy_path).unwrap()).unwrap();
    policy["lease_seconds"] = json!(10);
    fs::write(&policy_path, policy.to_string()).unwrap();
    let ready = w.prepare();
    let old_expiry = ready["allocation"]["allocation"]["policy"]["expires_at_unix_seconds"]
        .as_u64()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        <= old_expiry
    {
        assert!(
            Instant::now() < deadline,
            "real short native lease did not expire"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut invalid = w.prepare_input();
    invalid["selected_directories"] =
        json!([w.root.join("Work/demo/src/missing-native-directory")]);
    let refused = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        invalid.to_string(),
        "--expected-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!refused.status.success());
    let pending = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(pending["ready"], false);
    let restored = w.cli(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    let resumed = &restored["record"];
    assert_eq!(resumed["ready"], true);
    assert_ne!(resumed["revision"], ready["revision"]);
    assert_eq!(
        resumed["allocation"]["allocation"]["now_ref"],
        ready["allocation"]["allocation"]["now_ref"]
    );
    assert_eq!(
        resumed["allocation"]["allocation"]["source"]["ref"],
        ready["allocation"]["allocation"]["source"]["ref"]
    );
    assert_eq!(
        resumed["allocation"]["allocation"]["policy"]["revision"],
        ready["allocation"]["allocation"]["policy"]["revision"]
    );
    assert!(
        resumed["allocation"]["allocation"]["policy"]["expires_at_unix_seconds"]
            .as_u64()
            .unwrap()
            > old_expiry,
        "recovery must obtain a fresh finite native lease"
    );
    for key in ["writable_paths", "protected_paths", "required_coverage"] {
        assert_eq!(resumed["requirements"][key], ready["requirements"][key]);
    }
    w.start();
    assert_eq!(w.open(resumed, &w.root.join("Work/demo/src"))["ok"], true);
}

#[test]
#[ignore = "requires exact source-built Central, Workcell and Actuation; mandatory CAW lane"]
fn pending_abort_refuses_changed_native_placement_policy_and_remains_recoverable() {
    let w = World::new(true);
    let ready = w.prepare();
    let mut invalid = w.prepare_input();
    invalid["selected_directories"] =
        json!([w.root.join("Work/demo/src/missing-native-directory")]);
    let refused = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        invalid.to_string(),
        "--expected-revision".into(),
        ready["revision"].as_str().unwrap().into(),
    ]);
    assert!(!refused.status.success());
    let pending = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    let policy_path = w.root.join("Control/user/placement.json");
    let original = fs::read(&policy_path).unwrap();
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["lease_seconds"] = json!(301);
    fs::write(&policy_path, changed.to_string()).unwrap();
    let command = |expected_revision: &str| {
        vec![
            "encounter-task-abort".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
            "--expected-revision".into(),
            expected_revision.into(),
            "--restore-revision".into(),
            ready["revision"].as_str().unwrap().into(),
        ]
    };
    let rejected = w.command(&command(pending["revision"].as_str().unwrap()));
    assert!(
        !rejected.status.success(),
        "changed policy cannot renew the old task authority"
    );
    let still_pending = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(still_pending["ready"], false);
    assert_ne!(still_pending["revision"], pending["revision"]);
    assert_eq!(
        still_pending["request"], ready["request"],
        "pending recovery is bound to the original request"
    );
    fs::write(&policy_path, original).unwrap();
    let stale = w.command(&command(pending["revision"].as_str().unwrap()));
    assert!(
        !stale.status.success(),
        "the first pending CAS was consumed by the recovery journal"
    );
    let recovered = w.cli(&command(still_pending["revision"].as_str().unwrap()));
    assert_eq!(recovered["record"]["ready"], true);
    assert_eq!(
        recovered["record"]["allocation"]["allocation"]["now_ref"],
        ready["allocation"]["allocation"]["now_ref"]
    );
}

#[test]
#[ignore = "requires exact source-built Central, Workcell and Actuation; mandatory CAW lane"]
fn first_pending_preparation_remains_bound_to_same_native_request() {
    let w = World::new(true);
    let directory = w.root.join("Work/demo/src/selected-after-failure");
    let mut request = w.prepare_input();
    request["selected_directories"] = json!([directory]);
    let failed = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        request.to_string(),
    ]);
    assert!(!failed.status.success());
    let pending = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(pending["ready"], false);
    let replacement = w.command(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        w.prepare_input().to_string(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
    ]);
    assert!(!replacement.status.success());
    let abort = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        "task-binding/nonexistent".into(),
    ]);
    assert!(
        !abort.status.success(),
        "no earlier ready body may be invented"
    );
    assert_eq!(
        w.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
        ]),
        pending
    );
    fs::create_dir(&directory).unwrap();
    let recovered = w.cli(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        request.to_string(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
    ]);
    assert_eq!(recovered["ready"], true);
    assert_eq!(
        recovered["request"]["central"],
        pending["request"]["central"]
    );
    assert_eq!(
        recovered["allocation"]["allocation"]["record"]["task_ref"],
        "task:native-joined"
    );
}

#[path = "support/caw_task_material.rs"]
mod material;

#[test]
#[ignore = "requires exact native Central, Actuation and Workcell executables; mandatory prepared-run lane"]
fn native_prepared_run_preserves_authority_and_existing_worktree() {
    use sha2::{Digest, Sha256};
    let w = World::new(true);
    let recognise_cleanup_authority = |world: &World| {
        let path = world.root.join("Control/relations/source-relations.json");
        let mut source: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        source["relations"].as_array_mut().unwrap().push(json!({
            "ref":"central:source:control:root:Control/user/native-action-authority.json",
            "path":"Control/user/native-action-authority.json", "roles":["native-action-authority"],
            "provenance":"human-adopted", "standing":"architecture-contract", "treatment":"projectcentral-user",
            "recognition":"controlled-test-only", "recorded_at_unix_seconds":1
        }));
        fs::write(path, source.to_string()).unwrap();
    };
    recognise_cleanup_authority(&w);
    let binary = PathBuf::from(
        std::env::var_os("AIKIT_CAW_WORKCELL_BIN").expect("native Workcell required"),
    );
    let state = w.root.join("Work/demo/material");
    let repository = w.root.join("Work/demo/src");
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "--template="]);
    git(&["config", "user.name", "Native task test"]);
    git(&["config", "user.email", "native@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    fs::write(repository.join("readme"), "native material\n").unwrap();
    git(&["add", "readme"]);
    git(&["commit", "-m", "native material"]);
    let workcell = |args: &[String]| {
        let out = Command::new(&binary)
            .arg("--state-root")
            .arg(&state)
            .arg("--json")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let started = workcell(&[
        "--workspace-source".into(),
        repository.display().to_string(),
        "run".into(),
        "start".into(),
        "--run".into(),
        "native-task".into(),
        "--extension".into(),
        "branch_law=aikit".into(),
        "--workspace".into(),
        "writable".into(),
    ]);
    struct Release {
        binary: PathBuf,
        state: PathBuf,
    }
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = Command::new(&self.binary)
                .arg("--state-root")
                .arg(&self.state)
                .args(["--json", "run", "release", "--run", "native-task"])
                .output();
        }
    }
    let _release = Release {
        binary: binary.clone(),
        state: state.clone(),
    };
    let worktree = started["run"]["material_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|r| r.as_str()?.strip_prefix("workspace:git-worktree:"))
        .map(|key| state.join("workspaces").join(key))
        .unwrap()
        .canonicalize()
        .unwrap();
    fs::write(w.root.join("Control/user/native-action-authority.json"),json!({"schema":"central.native-action-authority/v1","scope_ref":"control:root","grants":[{"principal_ref":"agent:existing-1","actor_kind":"agent","token_sha256":format!("{:x}",Sha256::digest(b"native-run-cleanup-authority-proof-20260924")),"scope_refs":["control:root"],"actions":["central.now.lifecycle"],"expires_at_unix_seconds":u64::MAX}]}).to_string()).unwrap();
    let mut request = w.prepare_input();
    request["cwd"] = json!(worktree);
    request["selected_directories"] = json!([worktree]);
    request["prepared_run_scope"] =
        json!({"run_slug":"native-task","expected_demand_digest":started["run"]["demand_digest"]});
    request
        .as_object_mut()
        .unwrap()
        .remove("workcell_boundary_bin");
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let configure = |world: &World, request: &Value| {
        Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
            .env("AIKIT_HOME", world.home.root())
            .env("WORKCELL_HOME", &state)
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .env(
                "OI_ACTUATION_BIN",
                std::env::var_os("AIKIT_CAW_ACTUATION_BIN").unwrap(),
            )
            .env(
                "CENTRAL_NATIVE_TOKEN",
                "native-run-cleanup-authority-proof-20260924",
            )
            .arg("-C")
            .arg(&world.root)
            .args([
                "encounter-task-configure",
                "--agent-session",
                "agent-session/task",
                "--request-json",
            ])
            .arg(request.to_string())
            .output()
            .unwrap()
    };
    let mut stale = request.clone();
    stale["prepared_run_scope"]["expected_demand_digest"] = json!("sha256:stale");
    assert!(
        !configure(&w, &stale).status.success(),
        "stale demand must refuse before allocation"
    );
    // A real boundary refusal after allocation closes only that new NOW.
    let other = World::new(true);
    recognise_cleanup_authority(&other);
    fs::copy(
        w.root.join("Control/user/native-action-authority.json"),
        other.root.join("Control/user/native-action-authority.json"),
    )
    .unwrap();
    let mut wrong = other.prepare_input();
    wrong["prepared_run_scope"] = request["prepared_run_scope"].clone();
    wrong
        .as_object_mut()
        .unwrap()
        .remove("workcell_boundary_bin");
    let refusal = configure(&other, &wrong);
    assert!(!refusal.status.success());
    let retained = other.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(retained["ready"], false);
    assert_eq!(retained["cleanup"]["state"], "confirmed", "{retained}");
    assert_eq!(
        retained["cleanup"]["receipt"]["record"]["lifecycle"],
        "closed"
    );
    assert!(
        worktree.is_dir(),
        "failure cleanup must not delete a pre-existing run worktree"
    );
    let out = configure(&w, &request);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["ready"], true);
    assert_eq!(record["request"]["cwd"], json!(worktree));
    assert_eq!(
        record["prepared_run"]["scope"]["prepared_write_boundary"],
        record["inspection"]
    );
    assert_eq!(
        record["prepared_run"]["scope"]["agency"]["admission"]["status"],
        "actualised"
    );
    assert_eq!(
        record["prepared_run"]["scope"]["agency"]["admission"]["differentiated_binding"]
            ["agency_ref"],
        "agency:project:delegation"
    );
    assert_eq!(
        record["requirements"]["authority_ref"],
        "authority:project:delegation"
    );
    assert_eq!(
        record["requirements"]["writable_paths"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "only native NOW and selected existing worktree"
    );
    assert_eq!(
        workcell(&["run".into(), "list".into(), "--full".into()])["runs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Revoke the exact source after preparation: even an unchanged run name
    // cannot authorize another provider launch.
    let agency_before_revocation = fs::read(w.root.join("agency.json")).unwrap();
    fs::write(w.root.join("agency.json"), "{}").unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
        .env("AIKIT_HOME", w.home.root())
        .current_dir(&worktree)
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            record["revision"].as_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "changed Agency must refuse before provider execution"
    );
    assert!(!w.root.join("Work/demo/src/protocol.log").exists());

    // Restore the exact admitted test source, then make a real failed
    // amendment to this ready prepared-run task. The generic unhosted abort
    // must not restore around Workcell's retained run authority, even though
    // the requested ready revision exists in this same native task history.
    fs::write(w.root.join("agency.json"), agency_before_revocation).unwrap();
    let run_before = workcell(&[
        "run".into(),
        "show".into(),
        "--run".into(),
        "native-task".into(),
    ]);
    let mut missing = request.clone();
    missing["selected_directories"] = json!([worktree.join("missing-native-directory")]);
    let amendment = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
        .env("AIKIT_HOME", w.home.root())
        .env("WORKCELL_HOME", &state)
        .env("PATH", std::env::join_paths(&paths).unwrap())
        .env(
            "OI_ACTUATION_BIN",
            std::env::var_os("AIKIT_CAW_ACTUATION_BIN").unwrap(),
        )
        .env(
            "CENTRAL_NATIVE_TOKEN",
            "native-run-cleanup-authority-proof-20260924",
        )
        .arg("-C")
        .arg(&w.root)
        .args([
            "encounter-task-configure",
            "--agent-session",
            "agent-session/task",
            "--request-json",
        ])
        .arg(missing.to_string())
        .args(["--expected-revision", record["revision"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !amendment.status.success(),
        "the actual missing directory must refuse after journalling pending"
    );
    let pending = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(pending["ready"], false);
    assert!(pending["prepared_run"].is_object());
    assert_eq!(
        pending["request"]["prepared_run_scope"],
        request["prepared_run_scope"]
    );
    let aborted = w.command(&[
        "encounter-task-abort".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--expected-revision".into(),
        pending["revision"].as_str().unwrap().into(),
        "--restore-revision".into(),
        record["revision"].as_str().unwrap().into(),
    ]);
    assert!(!aborted.status.success());
    assert!(String::from_utf8_lossy(&aborted.stderr)
        .contains("Prepared-run preparation may have uncertain effects"));
    assert_eq!(
        w.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into()
        ]),
        pending,
        "abort must retain the exact pending task for native Workcell recovery"
    );
    let run_after = workcell(&[
        "run".into(),
        "show".into(),
        "--run".into(),
        "native-task".into(),
    ]);
    assert_eq!(run_after["run"], run_before["run"]);
    assert_eq!(run_after["run_revision"], run_before["run_revision"]);
    assert!(worktree.is_dir());
    assert!(!w.root.join("Work/demo/src/protocol.log").exists());
}
