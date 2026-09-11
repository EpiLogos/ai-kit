//! Joined production owner path. Real disposable World, source-built native
//! owners and actual ACP process; no commercial-model or installed proof.
#![cfg(unix)]
use aikit_cli::encounter_service::{EncounterAgencyBinding, EncounterContextAdmission, EncounterRequiredSource};
use aikit_adapters::agency_admission::AgencySourceBasis;
use aikit_core::{ResourceRef, SourceRevision};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{SessionSpaceAgentAttachmentIntent, SessionSpaceMutation};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, process::{Child, Command, Stdio}, time::{Duration, Instant}};
fn r(s: &str) -> ResourceRef { ResourceRef::parse(s).unwrap() }
fn rev(s: &str) -> SourceRevision { SourceRevision::parse(s).unwrap() }
struct World { _temp: tempfile::TempDir, root: PathBuf, home: AikitHome, socket: PathBuf, child: Option<Child> }
impl World {
    fn new(allowed: bool) -> Self {
        let temp = tempfile::tempdir().unwrap(); let root = temp.path().canonicalize().unwrap();
        let home = AikitHome::at(root.join("home")); let socket = root.join("ipc/owner.sock");
        let world = Self { _temp: temp, root, home, socket, child: None };
        for p in ["Control/user", "Control/relations", "Work/demo/src"] { fs::create_dir_all(world.root.join(p)).unwrap(); }
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
        store.apply(&store.stage(None, SessionSpaceMutation::Create { id: space.clone(), label: None }).unwrap()).unwrap();
        store.apply(&store.stage(Some(&space), SessionSpaceMutation::AttachAgentSession {
            attachment: SessionSpaceAgentAttachmentIntent { agent_session:r("agent-session/task"), purpose:Some("Controlled task".into()), provenance:vec!["native-test".into()] }
        }).unwrap()).unwrap();
        let mut source: Value = serde_json::from_str(include_str!("fixtures/caw-agency-request.json")).unwrap();
        source["differentiated_binding"]["world_ref"] = json!("control:root");
        if allowed { source["determination"]["delegated_autonomy"]["allowed_action_refs"] = json!(["action/aikit/encounter-send","action/aikit/encounter-task"]); }
        let path = world.root.join("agency.json"); let bytes = serde_json::to_vec(&source).unwrap(); fs::write(&path,&bytes).unwrap();
        let context = world.root.join("context.md"); fs::write(&context,"SELECTED_CONTEXT").unwrap();
        let binding = EncounterAgencyBinding { revision:rev("rev/1"), active:true, agent_ref:r("agent:existing-1"),
            agency_ref:r("agency:project:delegation"), world_ref:r("control:root"), world_binding_ref:r("binding:project:delegation"),
            agency_source:AgencySourceBasis { source_ref:r("source/agency"), revision:rev("source/1"), path,
                content_digest:format!("blake3:{}",blake3::hash(&bytes).to_hex()) },
            actuation_bin:PathBuf::from(std::env::var_os("AIKIT_CAW_ACTUATION_BIN").expect("native Actuation required")),
            allowed_senders:[r("agent:sender")].into(), allowed_packet_sources:[r("source/shared")].into(),
            context:Some(EncounterContextAdmission { sources:vec![EncounterRequiredSource { source:r("source/context"), revision:rev("source/1"), path:context,
                content_digest:format!("blake3:{}",blake3::hash(b"SELECTED_CONTEXT").to_hex()) }], source_activations:vec![], projection:None, activation:None }) };
        world.cli(&["encounter-agency-configure".into(),"--agent-session".into(),"agent-session/task".into(),"--binding-json".into(),serde_json::to_string(&binding).unwrap()]);
        world
    }
    fn command(&self, args: &[String]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_aikit-session-space")).env("AIKIT_HOME",self.home.root())
            .arg("-C").arg(&self.root).args(args).output().unwrap()
    }
    fn cli(&self, args: &[String]) -> Value {
        let o=self.command(args); assert!(o.status.success(),"{}",String::from_utf8_lossy(&o.stderr)); serde_json::from_slice(&o.stdout).unwrap()
    }
    fn request(&self,v:Value)->Value { self.cli(&["encounter".into(),"--socket".into(),self.socket.display().to_string(),"--request-json".into(),v.to_string()]) }
    fn prepare_input(&self)->Value {
        json!({"central":{"ctrl_bin":std::env::var("AIKIT_CAW_CTRL_BIN").expect("native Central required"),"central_root":self.root,
            "project":null,"task_ref":"task:native-joined","purpose":"Native protected task","participant_refs":["agent:existing-1"],"source_refs":["source/agency"]},
            "provider":{"id":"controlled-task","label":"Controlled native, not model","protocol":"acp","argv":["python3","-u",Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/caw_task_provider.py"), self.root.join("Work/demo/src/protocol.log"),self.root.join("Control/user/human.md"), self.root.join("Work/loose.txt")]},
            "cwd":self.root.join("Work/demo/src"),"selected_directories":[self.root.join("Work/demo/src")],
            "workcell_boundary_bin":std::env::var("AIKIT_CAW_WORKCELL_BOUNDARY_BIN").expect("native Workcell required"),"authority_ref":"authority:project:delegation"})
    }
    fn prepare(&self)->Value { self.cli(&["encounter-task-configure".into(),"--agent-session".into(),"agent-session/task".into(),"--request-json".into(),self.prepare_input().to_string()]) }
    fn start(&mut self) {
        self.child=Some(Command::new(env!("CARGO_BIN_EXE_aikit-session-space")).env("AIKIT_HOME",self.home.root())
            .env("CENTRAL_NATIVE_TOKEN","CONTROLLED_MUST_NOT_REACH_PROVIDER")
            .arg("-C").arg(&self.root).args(["encounter-serve","--socket"]).arg(&self.socket)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
        let deadline=Instant::now()+Duration::from_secs(15);
        while !self.socket.exists() { assert!(Instant::now()<deadline); std::thread::sleep(Duration::from_millis(20)); }
    }
    fn open(&self,record:&Value,cwd:&Path)->Value { self.request(json!({"action":"open","space":"session-space/task","agent_session":"agent-session/task","provider":record["launcher"]["id"],"cwd":cwd})) }
    fn send(&self,id:&str)->Value { self.request(json!({"action":"send","agent_session":"agent-session/task","turn":{"delivery_ref":format!("delivery/{id}"),"sender":"agent:sender","expected_binding_revision":"rev/1","packet":{"text":"Do the bounded native test","source_refs":["source/shared"],"audience":["agent:existing-1"]}}})) }
    fn returned(&self)->Value {
        let end=Instant::now()+Duration::from_secs(15);
        loop { let v=self.request(json!({"action":"delivery","agent_session":"agent-session/task","delivery_ref":"delivery/one"}));
            if v["data"]["phase"]=="returned" { return v; }
            assert!(Instant::now()<end,"{v}"); std::thread::sleep(Duration::from_millis(20)); }
    }
}
impl Drop for World {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Shut down via the native owner so it stops its protocol children.
            let _ = self.command(&["encounter".into(), "--socket".into(), self.socket.display().to_string(),
                "--request-json".into(), json!({"action":"shutdown","expected_pid":child.id()}).to_string()]);
            let end = Instant::now() + Duration::from_secs(5);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < end { std::thread::sleep(Duration::from_millis(20)); }
            let _ = child.kill(); let _ = child.wait();
        }
    }
}

#[test]
#[ignore="requires source-built Central, Workcell with Landlock and Actuation; mandatory CAW lane"]
fn real_task_dispatch_confines_protocol_and_rechecks_source_without_duplicate_work() {
    let mut w=World::new(true); let prepared=w.prepare(); assert_eq!(prepared["ready"],true);
    w.cli(&["encounter-configure".into(), "--provider-json".into(), w.prepare_input()["provider"].to_string()]);
    w.start();
    let alternate = w.request(json!({"action":"open","space":"session-space/task","agent_session":"agent-session/task",
        "provider":"controlled-task","cwd":w.root.join("Work/demo/src")}));
    assert_eq!(alternate["ok"],false);
    assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    assert_eq!(w.open(&prepared,&w.root.join("Work/demo/src"))["ok"],true);
    assert_eq!(w.send("one")["ok"],true); let returned=w.returned();
    let evidence:Value=serde_json::from_slice(&fs::read(w.root.join("Work/demo/src/result.json")).unwrap()).unwrap();
    assert_eq!(evidence["denied"],json!([true,true])); assert_eq!(evidence["selected_context"],true);
    assert_eq!(evidence["central_token_present"],false); assert_eq!(evidence["cwd"],json!(w.root.join("Work/demo/src")));
    assert_eq!(fs::read_to_string(w.root.join("Control/user/human.md")).unwrap(),"HUMAN_UNCHANGED");
    let now=PathBuf::from(prepared["allocation"]["allocation"]["writable_destination"].as_str().unwrap());
    assert_eq!(fs::read_to_string(now.join("return.txt")).unwrap(),"ACTUAL_TASK_RETURN");
    let log=w.root.join("Work/demo/src/protocol.log"); let before=fs::read(&log).unwrap();
    assert_eq!(w.send("one")["data"]["duplicate"],true); assert_eq!(fs::read(&log).unwrap(),before);
    fs::write(w.root.join("Control/user/placement.json"),"{}").unwrap();
    assert_eq!(w.send("two")["ok"],false); assert_eq!(fs::read(&log).unwrap(),before);
    assert_eq!(w.returned()["data"]["delivery_ref"],returned["data"]["delivery_ref"]);
    println!("TASK_NATIVE_PROTECTED_DISPATCH_EXECUTED");
}
#[test]
#[ignore="requires exact native owners; mandatory CAW lane"]
fn wrong_actual_cwd_and_removed_now_cannot_launch_a_provider() {
    for remove in [false,true] {
        let mut w=World::new(true); let prepared=w.prepare();
        if remove { fs::remove_file(w.root.join(prepared["allocation"]["allocation"]["source"]["path"].as_str().unwrap())).unwrap(); }
        w.start(); let cwd=if remove { w.root.join("Work/demo/src") } else { w.root.clone() };
        assert_eq!(w.open(&prepared,&cwd)["ok"],false);
        assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    }
}
#[test]
#[ignore="requires exact native owners; mandatory CAW lane"]
fn missing_task_authority_refuses_before_now_allocation() {
    let w=World::new(false); let result=w.command(&["encounter-task-configure".into(),"--agent-session".into(),"agent-session/task".into(),"--request-json".into(),w.prepare_input().to_string()]);
    assert!(!result.status.success()); assert!(!w.root.join("Control/agents/now").exists());
}
