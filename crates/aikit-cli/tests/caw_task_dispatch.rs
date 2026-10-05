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
    selected_native_driver: Option<PathBuf>,
    native_entry_point: aikit_cli::SessionSpaceEntryPoint,
    native_evidence: Option<PathBuf>,
    native_capture_sequence: std::cell::Cell<u32>,
}
impl World {
    fn new(allowed: bool) -> Self {
        Self::with_model_action(allowed, false)
    }
    fn with_model_action(allowed: bool, model_action: bool) -> Self {
        Self::with_model_action_and_driver(allowed, model_action, None)
    }
    fn with_model_action_and_driver(
        allowed: bool,
        model_action: bool,
        selected_native_driver: Option<PathBuf>,
    ) -> Self {
        Self::with_model_action_driver_and_entry_point(
            allowed,
            model_action,
            selected_native_driver,
            aikit_cli::SessionSpaceEntryPoint::Standalone,
        )
    }
    fn with_model_action_driver_and_entry_point(
        allowed: bool,
        model_action: bool,
        selected_native_driver: Option<PathBuf>,
        native_entry_point: aikit_cli::SessionSpaceEntryPoint,
    ) -> Self {
        Self::with_model_action_driver_entry_point_and_evidence(
            allowed,
            model_action,
            selected_native_driver,
            native_entry_point,
            None,
        )
    }
    fn with_model_action_driver_entry_point_and_evidence(
        allowed: bool,
        model_action: bool,
        selected_native_driver: Option<PathBuf>,
        native_entry_point: aikit_cli::SessionSpaceEntryPoint,
        native_evidence: Option<&Path>,
    ) -> Self {
        let mut temp = match native_evidence {
            Some(evidence) => tempfile::Builder::new()
                .prefix("world-")
                .tempdir_in(evidence)
                .unwrap(),
            None => tempfile::tempdir().unwrap(),
        };
        // Disable fixture deletion before the first native setup call, so an
        // unwind with an unknown native lifetime leaves actual bytes intact.
        if native_evidence.is_some() {
            temp.disable_cleanup(true);
        }
        let root = temp.path().canonicalize().unwrap();
        let home = AikitHome::at(root.join("home"));
        let socket = root.join("ipc/owner.sock");
        let world = Self {
            _temp: temp,
            root,
            home,
            socket,
            child: None,
            selected_native_driver,
            native_entry_point,
            native_evidence: native_evidence.map(Path::to_path_buf),
            native_capture_sequence: std::cell::Cell::new(0),
        };
        for p in [
            "Control/user",
            "Control/relations",
            "Work/demo/src",
            "Work/demo/ProjectCentral",
            "Work/sibling",
        ] {
            fs::create_dir_all(world.root.join(p)).unwrap();
        }
        fs::write(world.root.join("Control/user/human.md"), "HUMAN_UNCHANGED").unwrap();
        let policy_path = "Control/user/placement.json";
        let policy_ref = format!("central:source:control:root:{policy_path}");
        fs::write(world.root.join(policy_path), json!({"schema":"central.work-placement-policy/v1",
            "scope_ref":"control:root", "writable":[{"path":"Work/demo","class":"repository"}],
            "protected":["Work/demo/ProjectCentral"],
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
    fn native_driver(&self) -> &Path {
        self.selected_native_driver
            .as_deref()
            .unwrap_or_else(|| Path::new(env!("CARGO_BIN_EXE_aikit-session-space")))
    }
    fn command(&self, args: &[String]) -> std::process::Output {
        Command::new(self.native_driver())
            .env("AIKIT_HOME", self.home.root())
            .env("WORKCELL_CONTROL_TOKEN", "controlled-caw-material-token")
            .arg("-C")
            .arg(&self.root)
            .args(self.native_entry_point.verb_prefix())
            .args(args)
            .output()
            .unwrap()
    }
    fn cli(&self, args: &[String]) -> Value {
        if let Some(evidence) = &self.native_evidence {
            let sequence = self.native_capture_sequence.get().checked_add(1).unwrap();
            self.native_capture_sequence.set(sequence);
            let mut command = Command::new(self.native_driver());
            command
                .env("AIKIT_HOME", self.home.root())
                .env("WORKCELL_CONTROL_TOKEN", "controlled-caw-material-token")
                .arg("-C")
                .arg(&self.root)
                .args(self.native_entry_point.verb_prefix())
                .args(args);
            let output = entrypoint_capture(
                &mut command,
                evidence,
                &format!("native-call-{sequence}"),
                Duration::from_secs(60),
            );
            assert_eq!(
                output.status,
                0,
                "native refusal retained: {}",
                evidence.display()
            );
            return serde_json::from_str(&output.stdout).unwrap();
        }
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
            "cwd":self.root.join("Work/demo"),"selected_directories":[self.root.join("Work/demo/src")],
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
    let opened = world.open(&prepared, &world.root.join("Work/demo"));
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
    let refused = world.open(&prepared, &world.root.join("Work/demo"));
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
    assert_eq!(w.open(&prepared, &w.root.join("Work/demo"))["ok"], true);
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
    assert_eq!(evidence["cwd"], json!(w.root.join("Work/demo")));
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
            w.root.join("Work/demo")
        } else {
            w.root.clone()
        };
        assert_eq!(w.open(&prepared, &cwd)["ok"], false);
        assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    }
}

#[test]
#[ignore = "requires exact native owners; mandatory CAW lane"]
fn protected_or_sibling_working_directory_refuses_before_provider_start() {
    for relative in ["Work/demo/ProjectCentral", "Work/sibling"] {
        let w = World::new(true);
        let mut input = w.prepare_input();
        input["cwd"] = json!(w.root.join(relative));
        let result = w.command(&[
            "encounter-task-configure".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
            "--request-json".into(),
            input.to_string(),
        ]);
        assert!(!result.status.success(), "{relative}");
        assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    }
}

#[test]
#[ignore = "requires exact native owners; mandatory CAW lane"]
fn removed_or_replaced_working_directory_refuses_at_task_continuation() {
    for replace in [false, true] {
        let mut w = World::new(true);
        let prepared = w.prepare();
        let working_directory = w.root.join("Work/demo");
        let moved = w.root.join("Work/demo-before");
        fs::rename(&working_directory, &moved).unwrap();
        if replace {
            fs::create_dir_all(working_directory.join("src")).unwrap();
            fs::create_dir_all(working_directory.join("ProjectCentral")).unwrap();
        }
        w.start();
        assert_eq!(w.open(&prepared, &working_directory)["ok"], false);
        assert!(!working_directory.join("src/protocol.log").exists());
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
    assert_eq!(w.open(resumed, &w.root.join("Work/demo"))["ok"], true);
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
fn profile_derived_codex_task_keeps_its_body_inside_the_workcell_launcher() {
    let w = World::new(true);
    let mut request = w.prepare_input();
    request["provider"] = json!({
        "id":"codex-task-body",
        "label":"Codex ACP body selected through its native profile",
        "protocol":"acp",
        "from_profile":"codex",
    });
    let prepared = w.cli(&[
        "encounter-task-configure".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
        "--request-json".into(),
        request.to_string(),
    ]);
    assert_eq!(prepared["ready"], true);
    assert_eq!(prepared["request"]["provider"]["from_profile"], "codex");
    assert_eq!(prepared["launcher"]["from_profile"], Value::Null);
    assert!(prepared["launcher"]["argv_fallback"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert!(prepared["launcher"]["argv"]
        .as_array()
        .is_some_and(|argv| argv.iter().any(|arg| arg == "encounter-task-exec")));
    let reread = w.cli(&[
        "encounter-task-read".into(),
        "--agent-session".into(),
        "agent-session/task".into(),
    ]);
    assert_eq!(reread["revision"], prepared["revision"]);
    assert_eq!(reread["request"]["provider"]["from_profile"], "codex");
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
    assert_eq!(w.open(resumed, &w.root.join("Work/demo"))["ok"], true);
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

#[path = "support/caw_task_npm_runtime.rs"]
mod npm_runtime;

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
    // Exercise native publication through the activated primary executable.
    // Its companion remains beside the canonical owner, outside this PATH.
    // An unrelated, real executable with the same name must not select it.
    let activated = w.root.join("activated-owner");
    let unrelated = w.root.join("unrelated-owner");
    fs::create_dir_all(&activated).unwrap();
    fs::create_dir_all(&unrelated).unwrap();
    let boundary_binary = binary.parent().unwrap().join("workcell-write-boundary");
    assert!(boundary_binary.is_file(), "native companion required");
    fs::copy(&boundary_binary, unrelated.join("workcell-write-boundary")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&binary, activated.join("workcell")).unwrap();
    #[cfg(not(unix))]
    fs::copy(&binary, activated.join("workcell")).unwrap();
    let mut paths = vec![activated, unrelated];
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
    assert_eq!(
        record["prepared_run"]["boundary_executable"],
        json!(boundary_binary.canonicalize().unwrap()),
        "native preparation must retain the selected owner's actual sibling"
    );
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

// Native entrypoint regressions: actual selected compiler images and native
// Central / Workcell / Encounter. Every new case retains its owned Run space
// before a native call. Unknown retirement never deletes its image or World.
fn entrypoint_evidence(name: &str) -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("WORKCELL_TEST_ARTIFACT_ROOT")
            .expect("select the admitted absolute native Run artifact root"),
    );
    assert!(root.is_absolute() && root.canonicalize().unwrap() == root);
    assert!(fs::symlink_metadata(&root).unwrap().is_dir());
    tempfile::Builder::new()
        .prefix(name)
        .tempdir_in(root)
        .unwrap()
        .keep()
}

// This is an evidence consumer of the existing production native capture,
// not a subprocess supervisor. Failed/partial private bytes and actual cleanup
// causes are retained before the error reaches the test assertion.
fn entrypoint_capture(
    command: &mut Command,
    evidence: &Path,
    label: &str,
    timeout: Duration,
) -> aikit_adapters::runner::Output {
    use std::error::Error;
    fs::write(evidence.join(format!("{label}-argv.json")),
        serde_json::to_vec_pretty(&json!({"program":command.get_program().to_string_lossy(),
            "args":command.get_args().map(|arg|arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "state":"pending", "timeoutMs":timeout.as_millis(),
            "outputLimitPerStream":1024 * 1024, "automaticRetry":false})).unwrap()).unwrap();
    let result = aikit_adapters::runner::SystemRunner::new()
        .with_strict_utf8()
        .with_body_free_diagnostics()
        .with_timeout(timeout)
        .with_output_limit_bytes(1024 * 1024)
        .capture_command(command);
    let account = match &result {
        Ok(output) => {
            fs::write(
                evidence.join(format!("{label}.stdout")),
                output.stdout.as_bytes(),
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{label}.stderr")),
                output.stderr.as_bytes(),
            )
            .unwrap();
            json!({"state":"captured", "status":output.status,
                "standing":"actual completed native capture; status is not semantic success"})
        }
        Err(error) => {
            if let Some(raw) = error.native_capture() {
                fs::write(evidence.join(format!("{label}.stdout")), &raw.stdout).unwrap();
                fs::write(evidence.join(format!("{label}.stderr")), &raw.stderr).unwrap();
            }
            json!({"state":"refused", "code":error.code(), "message":error.message(),
                "details":error.details(), "cause":error.source().map(ToString::to_string),
                "secondaryCauses":error.secondary_io_sources().map(ToString::to_string).collect::<Vec<_>>(),
                "nativeCaptureStatus":error.native_capture().and_then(|raw|raw.status),
                "privatePartialBytesRetained":error.native_capture().is_some(),
                "standing":"actual failure; descendant effects and retirement remain their observed basis"})
        }
    };
    fs::write(
        evidence.join(format!("{label}-outcome.json")),
        serde_json::to_vec_pretty(&account).unwrap(),
    )
    .unwrap();
    result.unwrap_or_else(|error| {
        panic!(
            "native capture refused; evidence retained at {}: {}",
            evidence.display(),
            error.code()
        )
    })
}

fn copied_entrypoint_binary(selector: &str, name: &str, evidence: &Path) -> (PathBuf, String) {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let source =
        PathBuf::from(std::env::var_os(selector).expect("qualified native image required"));
    assert!(source.is_absolute() && source.canonicalize().unwrap() == source);
    let named = fs::symlink_metadata(&source).unwrap();
    let limit = 629_145_600u64;
    assert!(
        named.is_file()
            && !named.file_type().is_symlink()
            && named.len() > 0
            && named.len() <= limit
            && named.mode() & 0o111 != 0
    );
    let flags = if cfg!(target_os = "linux") {
        0o400000 | 0o4000
    } else if cfg!(target_os = "macos") {
        0x100 | 0x4
    } else {
        panic!("held native image custody requires Linux or macOS")
    };
    let basis = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.mode(),
            m.nlink(),
        )
    };
    let mut input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(&source)
        .unwrap();
    let held = input.metadata().unwrap();
    assert!(held.is_file());
    assert_eq!(basis(&held), basis(&named));
    let renamed = evidence.join(name);
    fs::write(evidence.join("native-image-custody.json"),
        serde_json::to_vec_pretty(&json!({"state":"pending", "selector":selector,
            "source":source,"destination":renamed,"sourceDevice":held.dev(),
            "sourceInode":held.ino(),"bytes":held.len(),"byteLimit":limit,
            "timeoutSeconds":360,"partialDestination":"retain on failure; never admitted by presence"})).unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(360);
    let hash = |file: &mut fs::File| {
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65_536];
        let mut bytes = 0u64;
        loop {
            assert!(Instant::now() < deadline, "native image custody deadline");
            let n = file.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            bytes = bytes.checked_add(n as u64).unwrap();
            assert!(bytes <= held.len() && bytes <= limit);
            digest.update(&buffer[..n]);
        }
        assert_eq!(bytes, held.len());
        format!("{:x}", digest.finalize())
    };
    let before = hash(&mut input);
    input.seek(SeekFrom::Start(0)).unwrap();
    let mut output = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o700)
        .custom_flags(flags)
        .open(&renamed)
        .unwrap();
    let mut copied = 0u64;
    let mut buffer = [0u8; 65_536];
    loop {
        assert!(Instant::now() < deadline, "native image copy deadline");
        let n = input.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        copied = copied.checked_add(n as u64).unwrap();
        assert!(copied <= held.len() && copied <= limit);
        output.write_all(&buffer[..n]).unwrap();
    }
    output.flush().unwrap();
    assert_eq!(copied, held.len());
    output
        .set_permissions(fs::Permissions::from_mode(0o700))
        .unwrap();
    let copied_basis = output.metadata().unwrap();
    assert!(copied_basis.is_file());
    assert_eq!(hash(&mut output), before);
    assert_eq!(hash(&mut input), before);
    assert_eq!(basis(&input.metadata().unwrap()), basis(&held));
    assert_eq!(basis(&fs::symlink_metadata(&source).unwrap()), basis(&held));
    assert_eq!(basis(&output.metadata().unwrap()), basis(&copied_basis));
    assert_eq!(
        basis(&fs::symlink_metadata(&renamed).unwrap()),
        basis(&copied_basis)
    );
    fs::write(
        evidence.join("native-image-custody-completed.json"),
        serde_json::to_vec_pretty(&json!({"selector":selector,"source":source,
            "destination":renamed,"sha256":before,"bytes":copied,
            "sourceDevice":held.dev(),"sourceInode":held.ino(),
            "destinationDevice":copied_basis.dev(),"destinationInode":copied_basis.ino(),
            "heldNamedBeforeAfterEqual":true,"state":"captured",
            "standing":"actual image bytes; compiler Source qualification supplied separately"}))
        .unwrap(),
    )
    .unwrap();
    (renamed, before)
}

struct NativeEntrypointOwner {
    socket: PathBuf,
    pid: u32,
    birth: String,
}
impl NativeEntrypointOwner {
    // Linux observes the kernel start tick through a held bounded /proc file.
    // macOS observes ps's actual start stamp. Its second-level precision is
    // conservative: an indistinguishable reused PID refuses retirement rather
    // than authoring a finer birth witness. No signal is sent by this observer.
    fn observe(world: &World, pid: u32, label: &str) -> Option<String> {
        if cfg!(target_os = "linux") {
            use std::io::Read;
            use std::os::unix::fs::OpenOptionsExt;
            let path = PathBuf::from(format!("/proc/{pid}/stat"));
            let mut file = match fs::OpenOptions::new()
                .read(true)
                .custom_flags(0o400000 | 0o4000)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
                Err(error) => panic!("actual process identity refused: {error}"),
            };
            assert!(file.metadata().unwrap().is_file());
            let mut bytes = Vec::new();
            (&mut file).take(4097).read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() <= 4096);
            let text = String::from_utf8(bytes).unwrap();
            let (head, fields) = text.rsplit_once(") ").unwrap();
            assert_eq!(head.split_once(' ').unwrap().0.parse::<u32>().unwrap(), pid);
            let birth = fields
                .split_whitespace()
                .nth(19)
                .unwrap()
                .parse::<u64>()
                .unwrap();
            Some(format!("linux-proc-start-tick:{birth}"))
        } else if cfg!(target_os = "macos") {
            let mut command = Command::new("/bin/ps");
            command.args(["-p", &pid.to_string(), "-o", "pid=", "-o", "lstart="]);
            let result = entrypoint_capture(
                &mut command,
                world.native_evidence.as_ref().unwrap(),
                label,
                Duration::from_secs(2),
            );
            assert!(result.stderr.is_empty());
            if result.status == 1 && result.stdout.trim().is_empty() {
                return None;
            }
            assert_eq!(result.status, 0);
            assert_eq!(result.stdout.lines().count(), 1);
            let line = result.stdout.trim();
            assert_eq!(
                line.split_whitespace()
                    .next()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap(),
                pid
            );
            Some(format!("macos-ps-start-stamp:{line}"))
        } else {
            panic!("native retirement observation requires Linux or macOS")
        }
    }
    fn close(&self, world: &World) {
        let evidence = world.native_evidence.as_ref().unwrap();
        assert_eq!(
            Self::observe(world, self.pid, "owner-preclose-observation").as_ref(),
            Some(&self.birth)
        );
        let reply = world.cli(&[
            "encounter".into(),
            "--socket".into(),
            self.socket.display().to_string(),
            "--request-json".into(),
            json!({"action":"shutdown","expected_pid":self.pid}).to_string(),
        ]);
        assert_eq!(reply["ok"], true, "{reply}");
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut sample = 0;
        let terminal = loop {
            sample += 1;
            let current = Self::observe(world, self.pid, &format!("owner-retirement-{sample}"));
            fs::write(
                evidence.join("owner-retirement-latest.json"),
                serde_json::to_vec_pretty(&json!({"pid":self.pid,"birth":self.birth,
                    "currentObservedBirth":current,"socketPresent":self.socket.exists(),
                    "shutdownAcknowledged":true,"state":"awaiting actual retirement"}))
                .unwrap(),
            )
            .unwrap();
            if current.as_ref() != Some(&self.birth) && !self.socket.exists() {
                break current;
            }
            assert!(
                Instant::now() < deadline,
                "native owner retirement unconfirmed; World/image retained"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        fs::write(evidence.join("owner-retirement-completed.json"),
            serde_json::to_vec_pretty(&json!({"pid":self.pid,"birth":self.birth,
                "currentObservedBirth":terminal,"shutdownAcknowledged":true,
                "socketRemoved":true,"observedOriginalProcessAbsent":true,
                "standing":"observed native owner retirement; detached child was not reaped by this test"})).unwrap()).unwrap();
    }
}
// No Drop shutdown or deletion: on unwind the actual pending call, World,
// image, native journal and any observed PID/birth remain for native recovery.

fn exercise_native_entrypoint(
    selector: &str,
    name: &str,
    entry_point: aikit_cli::SessionSpaceEntryPoint,
) {
    let images = entrypoint_evidence("native-entrypoint-");
    let (renamed, image_sha256) = copied_entrypoint_binary(selector, name, &images);
    let alias = images.join("native-owner-alias");
    std::os::unix::fs::symlink(&renamed, &alias).unwrap();
    for invoked in [&renamed, &alias] {
        let evidence = tempfile::Builder::new()
            .prefix("case-")
            .tempdir_in(&images)
            .unwrap()
            .keep();
        let world = World::with_model_action_driver_entry_point_and_evidence(
            true,
            false,
            Some(invoked.clone()),
            entry_point,
            Some(&evidence),
        );
        let mut request = world.prepare_input();
        request["provider"] = json!({
            "id":"native-codex-entrypoint-body",
            "label":"Actual Codex profile held behind native Task CAS",
            "protocol":"acp", "from_profile":"codex",
        });
        let configure = |expected: Option<&str>| {
            let mut args = vec![
                "encounter-task-configure".into(),
                "--agent-session".into(),
                "agent-session/task".into(),
                "--request-json".into(),
                request.to_string(),
            ];
            if let Some(revision) = expected {
                args.extend(["--expected-revision".into(), revision.into()]);
            }
            world.cli(&args)
        };
        let first = configure(None);
        assert_eq!(first["ready"], true, "{first}");
        let second = configure(Some(first["revision"].as_str().unwrap()));
        assert_eq!(second["ready"], true, "{second}");
        assert_ne!(first["revision"], second["revision"]);
        assert_eq!(
            first["allocation"]["allocation"]["now_ref"],
            second["allocation"]["allocation"]["now_ref"]
        );
        assert_eq!(first["request"]["provider"], second["request"]["provider"]);
        assert_eq!(first["launcher"]["id"], second["launcher"]["id"]);
        let reread = world.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
        ]);
        assert_eq!(reread, second);
        let argv = first["launcher"]["argv"].as_array().unwrap();
        assert_eq!(
            Path::new(argv[0].as_str().unwrap()).canonicalize().unwrap(),
            renamed
        );
        let verb_index = if entry_point == aikit_cli::SessionSpaceEntryPoint::Main {
            2
        } else {
            1
        };
        if verb_index == 2 {
            assert_eq!(argv[1], "session-space");
        }
        assert_eq!(argv[verb_index], "encounter-task-exec");
        let mut stale = Command::new(argv[0].as_str().unwrap());
        stale
            .env("AIKIT_HOME", world.home.root())
            .current_dir(world.root.join("Work/demo"))
            .args(argv[1..].iter().map(|arg| arg.as_str().unwrap()))
            .stdin(Stdio::null());
        let refused = entrypoint_capture(
            &mut stale,
            &evidence,
            "actual-stale-launcher",
            Duration::from_secs(60),
        );
        assert_ne!(refused.status, 0);
        assert!(
            refused.stderr.contains(
                "Task revision or actual process cwd differs from the prepared execution basis"
            ),
            "stale native launcher did not reach its owner guard: {}",
            refused.stderr
        );
        assert_eq!(
            world.cli(&[
                "encounter-task-read".into(),
                "--agent-session".into(),
                "agent-session/task".into()
            ]),
            second
        );
        assert!(!world.root.join("Work/demo/src/protocol.log").exists());

        let started = world.cli(&["encounter-start".into()]);
        assert_eq!(started["ok"], true, "{started}");
        let socket = aikit_cli::encounter_service::socket_path(&world.home);
        let pid = started["data"]["pid"].as_u64().unwrap().try_into().unwrap();
        let birth = NativeEntrypointOwner::observe(&world, pid, "owner-admission-observation")
            .expect("native owner must be actually observed before shutdown");
        let owner = NativeEntrypointOwner {
            socket: socket.clone(),
            pid,
            birth,
        };
        fs::write(
            evidence.join("owner-admission.json"),
            serde_json::to_vec_pretty(&json!({"pid":owner.pid,"birth":owner.birth,
                "socket":socket,"state":"pending explicit native shutdown and retirement",
                "World":world.root,"image":renamed,"retention":"all outcomes"}))
            .unwrap(),
        )
        .unwrap();
        let health = world.cli(&[
            "encounter".into(),
            "--socket".into(),
            socket.display().to_string(),
            "--request-json".into(),
            json!({"action":"health"}).to_string(),
        ]);
        assert_eq!(health["ok"], true, "{health}");
        assert_eq!(health["data"]["pid"], started["data"]["pid"]);
        assert_eq!(
            world.cli(&["encounter-start".into()])["data"]["pid"],
            started["data"]["pid"],
            "repeated startup may not create another owner"
        );
        owner.close(&world);
        println!(
            "{}",
            json!({"schema":"aikit.native-entrypoint-regression/v2", "entry_point":format!("{entry_point:?}"), "invoked":invoked, "image_sha256":image_sha256, "prior_task_revision":first["revision"], "current_task_revision":second["revision"], "now_ref":second["allocation"]["allocation"]["now_ref"], "native_owner_pid":owner.pid, "native_owner_birth":owner.birth, "stale_launcher_refused":true, "native_owner_retirement_observed":true, "provider_opened":false,"evidence":evidence})
        );
    }
}

#[test]
#[ignore = "requires qualified native AIKIT_MAIN_BINARY and Central/Workcell/Actuation plus admitted WORKCELL_TEST_ARTIFACT_ROOT; real Task CAS and owner IPC"]
fn renamed_and_aliased_main_keeps_native_task_and_owner_grammar() {
    exercise_native_entrypoint(
        "AIKIT_MAIN_BINARY",
        "renamed-main",
        aikit_cli::SessionSpaceEntryPoint::Main,
    );
}

#[test]
#[ignore = "requires qualified AIKIT_SESSION_SPACE_BINARY and Central/Workcell/Actuation plus admitted WORKCELL_TEST_ARTIFACT_ROOT; real Task CAS and owner IPC"]
fn standalone_named_aikit_and_alias_keep_native_task_and_owner_grammar() {
    exercise_native_entrypoint(
        "AIKIT_SESSION_SPACE_BINARY",
        "aikit",
        aikit_cli::SessionSpaceEntryPoint::Standalone,
    );
}
