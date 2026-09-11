//! Additional cases in the maintained native task campaign. The host is the
//! actual Workcell control service; the managed service is actual AIKit, not a
//! TCP sleeper labelled "session". Provider replies remain controlled ACP.
use super::*;
use aikit_adapters::central_placement::{CentralTaskRequest, NativeCentralPlacement};
use aikit_adapters::runner::SystemRunner;
use std::net::{TcpListener, TcpStream};

const TOKEN: &str = "controlled-caw-material-token";

struct NativeHost {
    root: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    state: PathBuf,
    endpoint: String,
    child: Option<Child>,
    managed_pid: Option<u32>,
}
impl NativeHost {
    fn new(w: &World, declare_storage: bool, managed_owner: bool) -> Self {
        let request: CentralTaskRequest = serde_json::from_value(w.prepare_input()["central"].clone()).unwrap();
        let task = NativeCentralPlacement::new(SystemRunner::new()).allocate(&request).unwrap();
        let state = w.root.join("material-host"); fs::create_dir_all(&state).unwrap();
        // Controlled owner configuration, using the actual Central allocation.
        // AIKit's production consumer never writes the host's declarations.
        if declare_storage { fs::write(state.join("storage.json"), task.storage_declaration().unwrap().to_string()).unwrap(); }
        if managed_owner {
            fs::write(state.join("services.json"), json!({"schema":"workcell.service-declaration/v1", "services":[{
                "logical_ref":"service:caw-native-encounter", "endpoint":format!("unix://{}",w.socket.display()),
                "lifetime":"provider-process-scoped", "program":env!("CARGO_BIN_EXE_aikit-session-space"),
                "args":["-C",w.root.to_str().unwrap(),"encounter-serve","--socket",w.socket.to_str().unwrap()],
                "cwd":w.root, "env":{"AIKIT_HOME":w.home.root(), "WORKCELL_CONTROL_TOKEN":TOKEN,
                    "CENTRAL_NATIVE_TOKEN":"CONTROLLED_MUST_NOT_REACH_PROVIDER"}
            }]}).to_string()).unwrap();
        }
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint=listener.local_addr().unwrap().to_string(); drop(listener);
        let mut host=Self { root:w.root.clone(),home:w.home.root().to_path_buf(),socket:w.socket.clone(),
            state,endpoint,child:None,managed_pid:None };
        host.start(); host
    }
    fn start(&mut self) {
        self.child=Some(Command::new(std::env::var_os("AIKIT_CAW_WORKCELL_SERVICE_BIN").expect("native Workcell control service required"))
            .env("WORKCELL_CONTROL_TOKEN",TOKEN)
            .args(["--state-root",self.state.to_str().unwrap(),"--workcell-ref","workcell:caw-task-material","--listen",&self.endpoint])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
        let end=Instant::now()+Duration::from_secs(10);
        while TcpStream::connect(&self.endpoint).is_err() {
            assert!(self.child.as_mut().unwrap().try_wait().unwrap().is_none(),"native host exited");
            assert!(Instant::now()<end,"native host did not listen"); std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop(&mut self) {
        if let Some(mut child)=self.child.take() { let _=child.kill(); child.wait().unwrap(); }
    }
    fn input(&self,w:&World,managed:bool)->Value {
        let mut input=w.prepare_input();
        input["material_host"]=json!({"workcell_bin":std::env::var("AIKIT_CAW_WORKCELL_BIN").expect("native workcell CLI required"),
            "endpoint":self.endpoint,"workcell_ref":"workcell:caw-task-material","demand_ref":"demand:caw-task-material-attempt-1",
            "required_services":if managed { vec!["service:caw-native-encounter"] } else { vec![] },
            "encounter_service":if managed { Some("service:caw-native-encounter") } else { None }});
        input
    }
    fn prepare(&mut self,w:&World,managed:bool)->Value {
        let result=w.cli(&["encounter-task-configure".into(),"--agent-session".into(),"agent-session/task".into(),
            "--request-json".into(),self.input(w,managed).to_string()]);
        if managed {
            self.managed_pid=Some(result["material"]["world"]["binding_graph"]["bindings"].as_array().unwrap().iter()
                .find(|b|b["port"]=="service").unwrap()["properties"]["pid"].as_str().unwrap().parse().unwrap());
        }
        result
    }
    fn workcell(&self,operation:&str,world:&Value)->Value {
        let receipt=self.root.join("captured-material-receipt.json"); fs::write(&receipt,world.to_string()).unwrap();
        let out=Command::new(std::env::var_os("AIKIT_CAW_WORKCELL_BIN").unwrap()).env("WORKCELL_CONTROL_TOKEN",TOKEN)
            .args(["--endpoint",&self.endpoint,"--json","--receipt",receipt.to_str().unwrap(),operation]).output().unwrap();
        assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr)); serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for NativeHost {
    fn drop(&mut self) {
        if let Some(pid)=self.managed_pid.take() {
            let _=Command::new(env!("CARGO_BIN_EXE_aikit-session-space")).env("AIKIT_HOME",&self.home)
                .arg("-C").arg(&self.root).args(["encounter","--socket"]).arg(&self.socket)
                .arg("--request-json").arg(json!({"action":"shutdown","expected_pid":pid}).to_string())
                .stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        self.stop();
    }
}
fn await_delivery(w:&World,id:&str) {
    let end=Instant::now()+Duration::from_secs(15);
    loop {
        let v=w.request(json!({"action":"delivery","agent_session":"agent-session/task","delivery_ref":format!("delivery/{id}")}));
        if v["data"]["phase"]=="returned" { break; }
        assert!(Instant::now()<end,"{v}"); std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore="requires source-built native owners and positive Landlock; mandatory CAW lane"]
fn persistent_storage_reentry_and_release_govern_real_turns_without_duplicate_work() {
    let mut w=World::new(true); let mut host=NativeHost::new(&w,true,false);
    let prepared=host.prepare(&w,false); let material=&prepared["material"]["world"];
    assert_eq!(prepared["ready"],true);
    assert_eq!(material["subjects"]["now"],prepared["allocation"]["allocation"]["now_ref"]);
    assert_eq!(material["subjects"]["agent_session"],"agent-session/task");
    assert_eq!(material["binding_graph"]["bindings"][0]["properties"]["path"],prepared["allocation"]["allocation"]["writable_destination"]);
    w.start(); assert_eq!(w.open(&prepared,&w.root.join("Work/demo/src"))["ok"],true);
    assert_eq!(w.send("one")["ok"],true); await_delivery(&w,"one");
    let log=w.root.join("Work/demo/src/protocol.log"); let before=fs::read(&log).unwrap();
    host.stop(); host.start();
    assert_eq!(host.workcell("inspect",material)["world_ref"],material["world_ref"]);
    assert_eq!(w.send("one")["data"]["duplicate"],true); assert_eq!(fs::read(&log).unwrap(),before);
    assert_eq!(w.send("two")["ok"],true); await_delivery(&w,"two");
    let before=fs::read(&log).unwrap(); host.workcell("release",material);
    assert_eq!(w.send("three")["ok"],false); assert_eq!(fs::read(&log).unwrap(),before);
    assert_eq!(w.returned()["data"]["phase"],"returned");
    assert_eq!(fs::read_to_string(w.root.join("Control/user/human.md")).unwrap(),"HUMAN_UNCHANGED");
    let now=PathBuf::from(prepared["allocation"]["allocation"]["writable_destination"].as_str().unwrap());
    assert_eq!(fs::read_to_string(now.join("return.txt")).unwrap(),"ACTUAL_TASK_RETURN");
    let evidence:Value=serde_json::from_slice(&fs::read(w.root.join("Work/demo/src/result.json")).unwrap()).unwrap();
    assert_eq!(evidence["workcell_token_present"],false);
    println!("TASK_NATIVE_PERSISTENT_STORAGE_EXECUTED");
}

#[test]
#[ignore="requires exact native owners and positive Landlock; mandatory CAW lane"]
fn workcell_really_hosts_the_native_encounter_owner_and_its_protected_response() {
    let w=World::new(true); let mut host=NativeHost::new(&w,true,true);
    let prepared=host.prepare(&w,true); assert!(host.managed_pid.is_some());
    let deadline=Instant::now()+Duration::from_secs(10);
    while !w.socket.exists() { assert!(Instant::now()<deadline); std::thread::sleep(Duration::from_millis(20)); }
    assert_eq!(w.open(&prepared,&w.root.join("Work/demo/src"))["ok"],true);
    assert_eq!(w.send("one")["ok"],true); await_delivery(&w,"one");
    let before=fs::read(w.root.join("Work/demo/src/protocol.log")).unwrap();
    assert_eq!(w.send("one")["data"]["duplicate"],true);
    assert_eq!(before,fs::read(w.root.join("Work/demo/src/protocol.log")).unwrap());
    let read=w.cli(&["encounter-task-read".into(),"--agent-session".into(),"agent-session/task".into()]);
    assert_eq!(read["material"]["world"]["world_ref"],prepared["material"]["world"]["world_ref"]);
    assert_eq!(host.workcell("observe",&prepared["material"]["world"])["observations"].as_array().unwrap().len(),2);
    let evidence:Value=serde_json::from_slice(&fs::read(w.root.join("Work/demo/src/result.json")).unwrap()).unwrap();
    assert_eq!(evidence["denied"],json!([true,true])); assert_eq!(evidence["workcell_token_present"],false);
    assert_eq!(evidence["central_token_present"],false); assert_eq!(evidence["selected_context"],true);
    assert_eq!(evidence["cwd"],json!(w.root.join("Work/demo/src")));
    println!("TASK_NATIVE_WORKCELL_HOSTED_ENCOUNTER_EXECUTED");
}

#[test]
#[ignore="requires native Central, Workcell and Actuation; mandatory CAW lane"]
fn absent_attachment_foreign_host_and_dropped_requirement_cannot_start_work() {
    for declared in [false,true] {
        let w=World::new(true); let host=NativeHost::new(&w,declared,false);
        let mut input=host.input(&w,false);
        if declared { input["material_host"]["workcell_ref"]=json!("workcell:wrong-owner"); }
        let out=w.command(&["encounter-task-configure".into(),"--agent-session".into(),"agent-session/task".into(),"--request-json".into(),input.to_string()]);
        assert!(!out.status.success()); assert!(!w.socket.exists());
        assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    }
    let w=World::new(true); let mut host=NativeHost::new(&w,true,false); let prepared=host.prepare(&w,false);
    let out=w.command(&["encounter-task-configure".into(),"--agent-session".into(),"agent-session/task".into(),
        "--expected-revision".into(),prepared["revision"].as_str().unwrap().into(),"--request-json".into(),w.prepare_input().to_string()]);
    assert!(!out.status.success());
    let current=w.cli(&["encounter-task-read".into(),"--agent-session".into(),"agent-session/task".into()]);
    assert_eq!(current,prepared); assert!(!w.root.join("Work/demo/src/protocol.log").exists());
}

#[test]
#[ignore="requires exact native owners; mandatory CAW lane"]
fn a_healthy_unrelated_process_cannot_satisfy_encounter_hosting() {
    let mut w=World::new(true); let mut host=NativeHost::new(&w,true,true);
    host.stop();
    let path=host.state.join("services.json");
    let mut declaration:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["services"][0]["program"]=json!("/bin/sleep");
    declaration["services"][0]["args"]=json!(["60"]);
    fs::write(&path,declaration.to_string()).unwrap(); host.start();
    let prepared=host.prepare(&w,true);
    w.start();
    assert_ne!(host.managed_pid,Some(w.child.as_ref().unwrap().id()));
    assert_eq!(w.open(&prepared,&w.root.join("Work/demo/src"))["ok"],false);
    assert!(!w.root.join("Work/demo/src/protocol.log").exists());
    // The fake service is only a negative witness; do not send it an owner shutdown.
    host.managed_pid=None;
}
