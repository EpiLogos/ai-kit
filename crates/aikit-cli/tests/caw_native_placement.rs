//! Real native owner chain. No replacement policy, allocator or material runner.
use aikit_adapters::{central_work::{boundary_requirements, prepare_boundary, CentralPlacement, CentralTask}, runner::SystemRunner};
use aikit_core::ResourceRef;
use serde_json::{json, Value};
use std::{fs, io::Write, path::{Path, PathBuf}, process::{Command, Stdio}};

fn owner_env(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var(name).expect("native owner must be explicitly pinned"));
    assert!(path.is_absolute() && path.is_file());
    path
}
fn world(root: &Path) -> CentralTask {
    fs::create_dir_all(root.join("Control/user")).unwrap();
    fs::create_dir_all(root.join("Control/relations")).unwrap();
    fs::create_dir_all(root.join("Work/demo")).unwrap();
    let path = "Control/user/placement-policy.json";
    let source = format!("central:source:control:root:{path}");
    fs::write(root.join(path), json!({
        "schema":"central.work-placement-policy/v1","scope_ref":"control:root",
        "writable":[{"path":"Work/demo","class":"repository"}],"protected":[],
        "enforcement":"material-filesystem","lease_seconds":300,
        "required_coverage":["file-content","file-creation","file-removal","rename-link","truncate","descendant-processes"],
    }).to_string()).unwrap();
    fs::write(root.join("Control/relations/source-relations.json"), json!({
        "schema":"central.control.ground-relations/v1","project_id":"control:root",
        "relations":[{"ref":source,"path":path,"roles":["work-placement-policy"],
        "provenance":"human-adopted","standing":"architecture-contract","treatment":"projectcentral-user",
        "recognition":"controlled-native-test-not-personal-adoption","recorded_at_unix_seconds":1}],
    }).to_string()).unwrap();
    CentralTask {
        ctrl_bin: owner_env("AIKIT_CAW_CTRL_BIN"), root:root.to_owned(), project:None,
        task_ref:ResourceRef::parse("task:joined-native").unwrap(),
        purpose:"Exercise native owner connections".into(), participant_refs:vec![],source_refs:vec![],
    }
}
fn change_policy(root: &Path, edit: impl FnOnce(&mut Value)) {
    let path = root.join("Control/user/placement-policy.json");
    let mut policy: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut policy);
    fs::write(path, policy.to_string()).unwrap();
}

#[test]
#[ignore = "requires exact source-built Central and Workcell; executed by CAW workflow"]
fn central_to_aikit_to_workcell_preserves_paths_and_executes_protocol_under_real_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let task = world(temp.path());
    let owner = CentralPlacement::new(SystemRunner::new(), task.clone()).unwrap();
    let allocated = owner.allocate().unwrap();
    let repeated = owner.allocate().unwrap();
    assert_eq!(allocated.basis, repeated.basis);
    assert_eq!(allocated.now_revision, repeated.now_revision);
    for (path, allow) in [(temp.path().join("Work/demo/result.txt"),true),
        (allocated.basis.now.join("result.txt"),true), (temp.path().join("Work/scratch.txt"),false),
        (temp.path().join("Control/user/placement-policy.json"),false)] {
        assert_eq!(owner.validate(&allocated,temp.path(),&path).unwrap()["allowed"],allow);
    }
    let binary = owner_env("AIKIT_CAW_BOUNDARY_BIN");
    let receipt = prepare_boundary(&SystemRunner::new(), &binary, &allocated).unwrap();
    let requirements = boundary_requirements(&allocated).unwrap();
    assert_eq!(receipt["requirements"],requirements);
    assert_eq!(receipt["state"],"prepared-not-executed");
    let policy_path = temp.path().join("Control/user/placement-policy.json");
    let protected = fs::read(&policy_path).unwrap();
    let request_path = temp.path().join("boundary.json");
    fs::write(&request_path,requirements.to_string()).unwrap();
    let output_path = allocated.basis.now.join("protocol-output.txt");
    let script = "import sys,pathlib; line=sys.stdin.readline(); p=pathlib.Path(sys.argv[1]); denied=False\ntry: p.write_text('FORBIDDEN')\nexcept PermissionError: denied=True\nassert denied; pathlib.Path(sys.argv[2]).write_text(line); print('CONTROLLED_REPLY:'+line.strip(),flush=True)";
    let mut child=Command::new(&binary).args(["exec",request_path.to_str().unwrap(),allocated.basis.policy_revision.as_str(),receipt["requirements_digest"].as_str().unwrap(),"--","/usr/bin/python3","-c",script,policy_path.to_str().unwrap(),output_path.to_str().unwrap()])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"native-turn\n").unwrap();
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout,b"CONTROLLED_REPLY:native-turn\n");
    assert_eq!(fs::read(&output_path).unwrap(),b"native-turn\n");
    assert_eq!(fs::read(&policy_path).unwrap(),protected);
    println!("CAW_JOINED_PLACEMENT_EXECUTED: real Central allocation and decisions, AIKit native adapter, Workcell protocol exec, denial and permitted NOW write");
}

#[test]
#[ignore = "requires exact source-built owners; executed by CAW workflow"]
fn stale_private_missing_producer_and_unsupported_coverage_do_not_dispatch() {
    let temp=tempfile::tempdir().unwrap();
    let task=world(temp.path());
    let owner=CentralPlacement::new(SystemRunner::new(),task.clone()).unwrap();
    let allocated=owner.allocate().unwrap();
    change_policy(temp.path(),|p|p["protected"]=json!(["Work/demo"]));
    assert!(owner.validate(&allocated,temp.path(),&temp.path().join("Work/demo/result")).is_err());
    change_policy(temp.path(),|p| { p["protected"]=json!([]); p["required_coverage"]=json!(["read-confidentiality"]); });
    let unsupported=owner.allocate().unwrap();
    assert!(prepare_boundary(&SystemRunner::new(),&owner_env("AIKIT_CAW_BOUNDARY_BIN"),&unsupported).is_err());
    // Remove the actual Central producer, not a boolean in a simulated runner.
    let mut disconnected=task.clone();
    disconnected.ctrl_bin=temp.path().join("absent-ctrl");
    assert!(CentralPlacement::new(SystemRunner::new(),disconnected).unwrap().allocate().is_err());
    fs::write(temp.path().join("Control/user/.no-agent-retrieval"),b"private").unwrap();
    assert!(owner.allocate().is_err());
    assert!(!temp.path().join("Work/demo/result").exists());
}

#[test]
#[ignore = "requires exact source-built owners; executed by CAW workflow"]
fn public_prepare_uses_the_same_native_operations_without_claiming_execution() {
    let temp=tempfile::tempdir().unwrap();
    let task=world(temp.path());
    let output=Command::new(env!("CARGO_BIN_EXE_aikit-work-prepare"))
        .args(["--request-json",&serde_json::to_string(&task).unwrap(),"--workcell-boundary",owner_env("AIKIT_CAW_BOUNDARY_BIN").to_str().unwrap()])
        .output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let value:Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"],"aikit.native-task-preparation/v1");
    assert_eq!(value["executed"],false);
    assert_eq!(value["confinement_active"],false);
    assert_eq!(value["allocation"]["allocation"]["record"]["task_ref"],json!(task.task_ref));
    assert_eq!(value["validation"]["allowed"],true);
}
