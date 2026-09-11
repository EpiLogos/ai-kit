//! Native task -> selected Agency -> actual resident ACP process -> attributable
//! response, with real temporary Git worktrees and Workcell kernel confinement.
#![cfg(unix)]
use aikit_adapters::{agency_admission::AgencySourceBasis, central_work::CentralTask, NativeGitProvider};
use aikit_cli::encounter_service::{EncounterAgencyBinding, EncounterContextAdmission, EncounterRequiredSource,
    EncounterService, EncounterTaskBinding};
use aikit_core::{project::ProjectRef, resource::{CreateWorktreeRequest, VersionRevision, VersionedWorldProvider},
    session_space::SessionSpaceRef, session_space_application::{SessionSpaceAgentAttachmentIntent, SessionSpaceMutation},
    ResourceRef, SourceRevision};
use aikit_store::{encounter::EncounterStore, AikitHome, SessionSpaceApplicationStore};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};
fn r(s:&str)->ResourceRef { ResourceRef::parse(s).unwrap() }
fn rev(s:&str)->SourceRevision { SourceRevision::parse(s).unwrap() }
fn native(name:&str)->PathBuf { PathBuf::from(std::env::var_os(name).expect("exact native owner binary is required; no silent skip")) }
fn git(root:&Path,args:&[&str])->Vec<u8> {
    let out=Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr)); out.stdout
}
fn apply(service:&EncounterService,v:Value)->Value {service.apply(serde_json::from_value(v).unwrap()).unwrap()}
fn turn()->Value {json!({"action":"send","agent_session":"agent-session/joined","turn":{
    "delivery_ref":"delivery/first","sender":"agent:sender","expected_binding_revision":"rev/agency-1",
    "packet":{"text":"Make the bounded tracked change and return the exact basis.","source_refs":["source/shared"],"audience":["agent:joined"]}}})}
fn returned(service:&EncounterService,delivery:&str)->Value {
    let until=Instant::now()+Duration::from_secs(15);
    loop {
        let value=apply(service,json!({"action":"delivery","agent_session":"agent-session/joined","delivery_ref":delivery}));
        if matches!(value["phase"].as_str(),Some("returned"|"failed"|"cancelled")){return value;}
        assert!(Instant::now()<until,"provider did not settle: {value}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn prompts(path:&Path)->Vec<Value> {
    fs::read_to_string(path).unwrap().lines().map(|l|serde_json::from_str::<Value>(l).unwrap())
        .filter(|v|v["method"]=="session/prompt").collect()
}
#[test]
#[ignore="requires exact Central, Workcell and Actuation; run by maintained CAW native workflow"]
fn task_admission_reaches_resident_response_and_reconnect_without_touching_human_checkout() {
    let temp=tempfile::tempdir().unwrap();let root=temp.path();
    let home=AikitHome::at(root.join("home"));
    let human=root.join("Work/demo");fs::create_dir_all(&human).unwrap();
    git(&human,&["init","-q"]);git(&human,&["config","user.name","Native test"]);git(&human,&["config","user.email","test@example.invalid"]);
    fs::write(human.join("tracked.txt"),"exact base\n").unwrap();git(&human,&["add","tracked.txt"]);git(&human,&["commit","-qm","base"]);
    let head=String::from_utf8(git(&human,&["rev-parse","HEAD"])).unwrap().trim().to_owned();
    fs::write(human.join("tracked.txt"),"human staged\n").unwrap();git(&human,&["add","tracked.txt"]);
    fs::write(human.join("tracked.txt"),"human unstaged\n").unwrap();fs::write(human.join("untracked.txt"),"keep this\n").unwrap();
    let staged=git(&human,&["diff","--cached","--binary"]);let dirty=fs::read(human.join("tracked.txt")).unwrap();
    let git_provider=NativeGitProvider::new().unwrap();let project=ProjectRef::parse("project:joined").unwrap();
    let cwd=human.join("candidates/one");let other=human.join("candidates/two");
    fs::create_dir_all(human.join("candidates")).unwrap();
    for path in [&cwd,&other] {
        git_provider.create_worktree(&project,human.to_str().unwrap(),&CreateWorktreeRequest{
            path:path.to_string_lossy().into_owned(),base:VersionRevision::new(&head),branch:None}).unwrap();
    }
    let basis=git_provider.development_field_basis(&project,cwd.to_str().unwrap(),Some(VersionRevision::new(&head)),1024*1024).unwrap();
    fs::create_dir_all(root.join("Control/user")).unwrap();fs::create_dir_all(root.join("Control/relations")).unwrap();
    let policy_path=root.join("Control/user/placement-policy.json");
    let source_ref="central:source:control:root:Control/user/placement-policy.json";
    fs::write(&policy_path,json!({"schema":"central.work-placement-policy/v1","scope_ref":"control:root",
        "writable":[{"path":"Work/demo/candidates/one","class":"worktree"}],"protected":[],
        "enforcement":"material-filesystem","lease_seconds":600,
        "required_coverage":["file-content","file-creation","file-removal","rename-link","truncate","descendant-processes"]}).to_string()).unwrap();
    let original_policy=fs::read(&policy_path).unwrap();
    fs::write(root.join("Control/relations/source-relations.json"),json!({"schema":"central.control.ground-relations/v1","project_id":"control:root",
        "relations":[{"ref":source_ref,"path":"Control/user/placement-policy.json","roles":["work-placement-policy"],
        "provenance":"human-adopted","standing":"architecture-contract","treatment":"projectcentral-user",
        "recognition":"controlled disposable test, not personal adoption","recorded_at_unix_seconds":1}]}).to_string()).unwrap();
    let session=r("agent-session/joined");let space=SessionSpaceRef::parse("session-space/joined").unwrap();
    let spaces=SessionSpaceApplicationStore::new(home.clone());
    spaces.apply(&spaces.stage(None,SessionSpaceMutation::Create{id:space.clone(),label:None}).unwrap()).unwrap();
    spaces.apply(&spaces.stage(Some(&space),SessionSpaceMutation::AttachAgentSession{attachment:SessionSpaceAgentAttachmentIntent{
        agent_session:session.clone(),purpose:Some("Controlled joined task".into()),provenance:vec!["native fixture".into()]}}).unwrap()).unwrap();
    let mut agency:Value=serde_json::from_str(include_str!("fixtures/caw-agency-request.json")).unwrap();
    agency["differentiated_binding"]["agent_ref"]=json!("agent:joined");
    agency["differentiated_binding"]["world_ref"]=json!("world:root-composition");
    let agency_path=root.join("agency.json");let bytes=serde_json::to_vec(&agency).unwrap();fs::write(&agency_path,&bytes).unwrap();
    let context_path=root.join("selected.md");let context_text="SELECTED_JOINED_CONTEXT\n";fs::write(&context_path,context_text).unwrap();
    let agency_binding=EncounterAgencyBinding{revision:rev("rev/agency-1"),active:true,agent_ref:r("agent:joined"),
        agency_ref:r("agency:project:delegation"),world_ref:r("world:root-composition"),world_binding_ref:r("binding:project:delegation"),
        agency_source:AgencySourceBasis{source_ref:r("source/native-agency"),revision:rev("rev/native-1"),path:agency_path,
            content_digest:format!("blake3:{}",blake3::hash(&bytes).to_hex())},
        actuation_bin:native("AIKIT_CAW_ACTUATION_BIN"),allowed_senders:[r("agent:sender")].into(),allowed_packet_sources:[r("source/shared")].into(),
        context:Some(EncounterContextAdmission{sources:vec![EncounterRequiredSource{source:r("source/selected"),revision:rev("rev/source-1"),path:context_path.clone(),
            content_digest:format!("blake3:{}",blake3::hash(context_text.as_bytes()).to_hex())}],source_activations:vec![],projection:None,activation:None})};
    EncounterService::configure_agency(&home,&session,&agency_binding,None).unwrap();
    let log=cwd.join("protocol.log");
    let provider=json!({"id":"joined","label":"Controlled protocol, not model acceptance","protocol":"acp",
        "argv":["/usr/bin/python3","-u",Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/caw_provider.py"),"placement",log,
            human.join("tracked.txt"),cwd.join("tracked.txt")]});
    EncounterService::configure(&home,serde_json::from_value(provider).unwrap()).unwrap();
    let task=EncounterTaskBinding{revision:rev("rev/task-1"),active:true,
        task:CentralTask{ctrl_bin:native("AIKIT_CAW_CTRL_BIN"),root:root.into(),project:None,task_ref:r("task:joined"),purpose:"Bounded native worktree change".into(),
            participant_refs:vec![r("agent:joined")],source_refs:vec![r("source/selected")]},
        world_ref:r("world:root-composition"),central_scope_ref:r("control:root"),cwd:cwd.clone(),provider:"joined".into(),
        workcell_boundary_bin:native("AIKIT_CAW_BOUNDARY_BIN"),workcell_bin:native("AIKIT_CAW_WORKCELL_BIN"),workcell_ref:r("workcell:controlled"),
        return_ref:r("return-relation:project:delegation"),working_copy:Some(basis)};
    // Exercise the new public CLI operation, not only a struct written into state.
    let configured=Command::new(env!("CARGO_BIN_EXE_aikit-session-space")).env("AIKIT_HOME",home.root()).arg("-C").arg(root)
        .args(["encounter-task-configure","--agent-session",session.as_str(),"--binding-json",&serde_json::to_string(&task).unwrap()]).output().unwrap();
    assert!(configured.status.success(),"{}",String::from_utf8_lossy(&configured.stderr));
    let service=EncounterService::new(home.clone()).unwrap();
    for wrong in [&human,&other] {
        assert!(service.apply(serde_json::from_value(json!({"action":"open","space":space,"agent_session":session,"provider":"joined","cwd":wrong})).unwrap()).is_err());
    }
    assert!(!log.exists());
    let opened=apply(&service,json!({"action":"open","space":space,"agent_session":session,"provider":"joined","cwd":cwd}));
    let sent=apply(&service,turn());assert_eq!(sent["transport_accepted"],true);
    assert_eq!(returned(&service,"delivery/first")["phase"],"returned");
    assert_eq!(apply(&service,turn())["duplicate"],true);
    assert_eq!(prompts(&log).len(),1);
    let prompt=prompts(&log)[0]["params"]["prompt"].to_string();
    assert!(prompt.contains("SELECTED_JOINED_CONTEXT") && prompt.contains(&head) && prompt.contains("native-task-basis"));
    let durable=EncounterStore::open(&home).unwrap();
    let before=durable.last_native_binding(&session).unwrap().unwrap();
    assert_eq!(before["task"]["storage"]["observation"]["reading"]["ok"],true);
    assert_eq!(before["task"]["storage"]["receipt_world"]["subjects"]["now"],before["task"]["allocation"]["basis"]["allocation_ref"]);
    let now=PathBuf::from(before["task"]["allocation"]["basis"]["now"].as_str().unwrap());
    assert!(now.is_dir());
    assert_eq!(fs::read(human.join("tracked.txt")).unwrap(),dirty);
    assert_eq!(git(&human,&["diff","--cached","--binary"]),staged);
    assert_eq!(fs::read(human.join("untracked.txt")).unwrap(),b"keep this\n");
    assert_eq!(fs::read(other.join("tracked.txt")).unwrap(),b"exact base\n");
    assert_ne!(fs::read(cwd.join("tracked.txt")).unwrap(),b"exact base\n");
    let mut later=turn();later["turn"]["delivery_ref"]=json!("delivery/later");
    fs::write(&context_path,"changed private/source basis").unwrap();
    assert!(service.apply(serde_json::from_value(later.clone()).unwrap()).is_err());
    fs::write(&context_path,context_text).unwrap();
    let mut changed:Value=serde_json::from_slice(&original_policy).unwrap();changed["protected"]=json!(["Work/demo/candidates/one"]);
    fs::write(&policy_path,changed.to_string()).unwrap();
    assert!(service.apply(serde_json::from_value(later.clone()).unwrap()).is_err());
    assert_eq!(prompts(&log).len(),1);
    fs::write(&policy_path,&original_policy).unwrap();
    apply(&service,json!({"action":"shutdown","expected_pid":std::process::id()}));drop(service);
    let restarted=EncounterService::new(home.clone()).unwrap();
    let reopened=apply(&restarted,json!({"action":"reconnect","space":space,"agent_session":session,"provider":"joined","cwd":cwd}));
    assert_eq!(reopened["native_session_id"],opened["native_session_id"]);
    assert_eq!(apply(&restarted,turn())["duplicate"],true);
    apply(&restarted,later);assert_eq!(returned(&restarted,"delivery/later")["phase"],"returned");
    let after=durable.last_native_binding(&session).unwrap().unwrap();
    assert_eq!(after["task"]["binding"]["return_ref"],before["task"]["binding"]["return_ref"]);
    assert_eq!(after["task"]["storage"]["receipt_world"]["world_ref"],before["task"]["storage"]["receipt_world"]["world_ref"]);
    assert_eq!(prompts(&log).len(),2);
    // Removing the actual owner binding cannot convert historical task work to Direct.
    let config=home.state().join("encounter-tasks").join(format!("{}.json",blake3::hash(session.as_str().as_bytes()).to_hex()));
    fs::remove_file(config).unwrap();
    let mut refused=turn();refused["turn"]["delivery_ref"]=json!("delivery/missing-task");
    assert!(restarted.apply(serde_json::from_value(refused).unwrap()).is_err());
    apply(&restarted,json!({"action":"shutdown","expected_pid":std::process::id()}));
    assert_eq!(fs::read(&policy_path).unwrap(),original_policy);
    assert_eq!(fs::read(human.join("tracked.txt")).unwrap(),dirty);
    println!("CAW_TASK_RESIDENT_EXECUTED: native owners, selected Agency, real worktree/NOW/storage, confined provider turn, dedup, source/policy refusal and native reconnect");
}
