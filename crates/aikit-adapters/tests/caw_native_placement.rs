//! Controlled native-owner integration, not model/harness or personal adoption proof.
use aikit_adapters::central_placement::{CentralTaskRequest, NativeCentralPlacement};
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::ResourceRef;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn world() -> (tempfile::TempDir, CentralTaskRequest) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for path in [
        "Control/user",
        "Control/relations",
        "Work/demo/src",
        "Work/demo/ProjectCentral",
        "Work/sibling",
    ] {
        fs::create_dir_all(root.join(path)).unwrap();
    }
    let path = "Control/user/placement.json";
    let source = format!("central:source:control:root:{path}");
    fs::write(root.join(path), json!({
        "schema":"central.work-placement-policy/v1", "scope_ref":"control:root",
        "writable":[{"path":"Work/demo", "class":"repository"}],
        "protected":["Work/demo/ProjectCentral"],
        "enforcement":"material-filesystem", "required_coverage":["file-content", "file-creation", "file-removal", "rename-link", "truncate", "descendant-processes"],
        "lease_seconds":300
    }).to_string()).unwrap();
    fs::write(root.join("Control/relations/source-relations.json"), json!({
        "schema":"central.control.ground-relations/v1", "project_id":"control:root",
        "relations":[{"ref":source,"path":path,"roles":["work-placement-policy"],
            "provenance":"human-adopted", "standing":"architecture-contract",
            "treatment":"projectcentral-user", "recognition":"controlled-native-test-not-personal-adoption",
            "recorded_at_unix_seconds":1}]
    }).to_string()).unwrap();
    let request = CentralTaskRequest {
        ctrl_bin: PathBuf::from(
            std::env::var("AIKIT_CAW_CTRL_BIN").expect("exact source-built ctrl required"),
        ),
        central_root: root,
        project: None,
        task_ref: ResourceRef::parse("task:native-joined").unwrap(),
        purpose: "Controlled native joined placement".into(),
        participant_refs: vec![ResourceRef::parse("agent:no-profile").unwrap()],
        source_refs: vec![],
    };
    (dir, request)
}

#[test]
#[ignore = "requires exact source-built Central and Workcell; mandatory in CAW workflow"]
fn native_policy_now_validation_and_workcell_prepare_are_connected() {
    let (_dir, request) = world();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let task = owner.allocate(&request).unwrap();
    let again = owner.allocate(&request).unwrap();
    assert_eq!(task.allocation["now_ref"], again.allocation["now_ref"]);
    assert_eq!(task.allocation["revision"], again.allocation["revision"]);
    assert_eq!(again.allocation["created"], false);
    let source = request.central_root.join("Work/demo/src");
    let working_directory = request.central_root.join("Work/demo");
    assert!(owner.validate_write(&task, &working_directory).is_err());
    let working_anchor = owner
        .working_directory_anchor(&task, &working_directory)
        .unwrap();
    assert_eq!(
        working_anchor["schema"],
        "aikit.task-working-directory-anchor/v1"
    );
    assert_eq!(working_anchor["path"], json!(working_directory));
    for refused in [
        request.central_root.join("Work/demo/ProjectCentral"),
        request.central_root.join("Work/sibling"),
    ] {
        assert!(owner.working_directory_anchor(&task, &refused).is_err());
    }
    let result = owner
        .validate_write(&task, &source.join("answer.txt"))
        .unwrap();
    assert_eq!(result["allowed"], true);
    assert!(owner
        .validate_write(&task, &request.central_root.join("Work/loose.txt"))
        .is_err());
    let authority = ResourceRef::parse("authority:controlled-native-test").unwrap();
    let requirements = owner
        .write_boundary_requirements(&task, &authority, std::slice::from_ref(&source))
        .unwrap();
    assert_eq!(
        requirements["protected_paths"],
        task.allocation["policy"]["protected_paths"]
    );
    assert_eq!(
        requirements["required_coverage"],
        task.allocation["policy"]["required_coverage"]
    );
    assert_eq!(
        requirements["writable_paths"],
        json!([task.now_directory().unwrap(), source])
    );
    assert_eq!(task.storage_requirement().unwrap()["retention"], "preserve");
    assert_eq!(
        task.storage_declaration().unwrap()["directories"][0]["logical_ref"],
        task.allocation["now_ref"]
    );
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), requirements.to_string()).unwrap();
    let binary = std::env::var("AIKIT_CAW_WORKCELL_BOUNDARY_BIN").expect("exact Workcell required");
    let inspected = SystemRunner::new()
        .run(&[
            binary,
            "inspect".into(),
            file.path().display().to_string(),
            requirements["policy_revision"].as_str().unwrap().into(),
        ])
        .unwrap();
    assert!(inspected.ok(), "{} {}", inspected.stdout, inspected.stderr);
    let native: Value = serde_json::from_str(&inspected.stdout).unwrap();
    assert_eq!(native["schema"], "workcell.prepared-write-boundary/v1");
    assert_eq!(native["requirements"], requirements);
    assert_eq!(native["state"], "prepared-not-executed");
    println!("NATIVE_CENTRAL_WORKCELL_PREPARATION: actual owners, no installed/model claim");
}

#[test]
#[ignore = "requires exact source-built Central; mandatory in CAW workflow"]
fn native_working_directory_anchor_refuses_removal_and_replacement() {
    let (_dir, request) = world();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let task = owner.allocate(&request).unwrap();
    let working_directory = request.central_root.join("Work/demo");
    owner
        .working_directory_anchor(&task, &working_directory)
        .unwrap();
    let moved = request.central_root.join("Work/demo-before");
    fs::rename(&working_directory, &moved).unwrap();
    assert!(owner
        .working_directory_anchor(&task, &working_directory)
        .is_err());
    fs::create_dir_all(working_directory.join("src")).unwrap();
    fs::create_dir_all(working_directory.join("ProjectCentral")).unwrap();
    assert!(owner
        .working_directory_anchor(&task, &working_directory)
        .is_err());
}

#[test]
#[ignore = "requires exact source-built Central; mandatory in CAW workflow"]
fn removing_native_policy_or_allocated_now_breaks_readmission() {
    for remove_policy in [true, false] {
        let (_dir, request) = world();
        let owner = NativeCentralPlacement::new(SystemRunner::new());
        let task = owner.allocate(&request).unwrap();
        let path = if remove_policy {
            request.central_root.join("Control/user/placement.json")
        } else {
            request
                .central_root
                .join(task.allocation["source"]["path"].as_str().unwrap())
        };
        fs::remove_file(path).unwrap();
        assert!(owner.revalidate(&task).is_err());
        assert!(owner
            .validate_write(&task, &task.now_directory().unwrap().join("return.txt"))
            .is_err());
    }
}

#[test]
#[ignore = "requires exact source-built Central; mandatory in CAW workflow"]
fn policy_changes_do_not_silently_rebase_or_renew_a_task() {
    let (_dir, request) = world();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let task = owner.allocate(&request).unwrap();
    let path = request.central_root.join("Control/user/placement.json");
    let mut policy: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    policy["protected"] = json!(["Work/demo/src"]);
    fs::write(&path, policy.to_string()).unwrap();
    assert!(owner.revalidate(&task).is_err());
    assert!(owner
        .write_boundary_requirements(&task, &ResourceRef::parse("authority:test").unwrap(), &[])
        .is_err());
    let next = owner.allocate(&request).unwrap();
    assert_eq!(task.allocation["now_ref"], next.allocation["now_ref"]);
    assert_ne!(
        task.allocation["policy"]["revision"],
        next.allocation["policy"]["revision"]
    );
    assert!(owner
        .validate_write(
            &next,
            &request.central_root.join("Work/demo/src/blocked.txt")
        )
        .is_err());
}

#[test]
#[ignore = "requires exact native Central; mandatory CAW workflow"]
fn existing_child_task_replays_native_ancestry_without_amending_client_request() {
    let (_dir, request) = world();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let mut parent_request = request.clone();
    parent_request.task_ref = ResourceRef::parse("task:native-parent").unwrap();
    parent_request.purpose = "Existing native parent".into();
    let parent = owner.allocate(&parent_request).unwrap();
    // Central authors the child record through its actual public operation.
    // The legacy AIKit request intentionally has no relationship fields.
    let input = json!({
        "task_ref": request.task_ref, "purpose": request.purpose,
        "participant_refs": request.participant_refs, "source_refs": request.source_refs,
        "parent_now_ref": parent.allocation["now_ref"], "workcell_ref":"workcell:native-test",
        "expected_policy_revision": parent.allocation["policy"]["revision"],
    });
    let out = SystemRunner::new()
        .run(&[
            request.ctrl_bin.display().to_string(),
            "--json".into(),
            "--root".into(),
            request.central_root.display().to_string(),
            "action".into(),
            "run".into(),
            "central.now.allocate".into(),
            input.to_string(),
        ])
        .unwrap();
    assert!(out.ok(), "{} {}", out.stdout, out.stderr);
    let allocated: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(allocated["ok"], true);
    let native = &allocated["data"];
    let record_path = request
        .central_root
        .join(native["source"]["path"].as_str().unwrap());
    let original_record = fs::read(&record_path).unwrap();
    let partial =
        PathBuf::from(native["writable_destination"].as_str().unwrap()).join("partial-return.bin");
    fs::write(&partial, b"retained partial output\0before recovery\n").unwrap();
    let original_request = serde_json::to_vec(&request).unwrap();
    let task = owner.allocate(&request).unwrap();
    assert_eq!(task.allocation["created"], false);
    assert_eq!(task.allocation["now_ref"], native["now_ref"]);
    assert_eq!(task.allocation["revision"], native["revision"]);
    assert_eq!(
        task.allocation["record"]["parent_now_ref"],
        parent.allocation["now_ref"]
    );
    assert_eq!(
        task.allocation["record"]["workcell_ref"],
        "workcell:native-test"
    );
    assert_eq!(task.allocation["record"]["horizon"], "child");
    assert_eq!(serde_json::to_vec(&task.request).unwrap(), original_request);
    assert_eq!(fs::read(&record_path).unwrap(), original_record);
    assert_eq!(
        fs::read(&partial).unwrap(),
        b"retained partial output\0before recovery\n"
    );
    // Cleanup may never close a preexisting child after failed preparation.
    assert_eq!(owner.close_new_allocation(&task).unwrap(), None);
    assert_eq!(
        owner.revalidate(&task).unwrap()["record"]["lifecycle"],
        "active"
    );
    let again = owner.allocate(&request).unwrap();
    assert_eq!(again.allocation["created"], false);
    assert_eq!(again.allocation["revision"], native["revision"]);
}

#[test]
#[ignore = "requires exact native Central; mandatory CAW workflow"]
fn existing_task_intent_mismatch_refuses_without_changing_native_bytes() {
    let (_dir, request) = world();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let task = owner.allocate(&request).unwrap();
    let path = request
        .central_root
        .join(task.allocation["source"]["path"].as_str().unwrap());
    let before = fs::read(&path).unwrap();
    for change in 0..3 {
        let mut wrong = request.clone();
        match change {
            0 => wrong.purpose.push_str(" changed intent"),
            1 => wrong
                .participant_refs
                .push(ResourceRef::parse("agent:other").unwrap()),
            2 => wrong
                .source_refs
                .push(ResourceRef::parse("source:other").unwrap()),
            _ => unreachable!(),
        }
        assert!(owner.allocate(&wrong).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    assert_eq!(
        owner.revalidate(&task).unwrap()["record"]["lifecycle"],
        "active"
    );
}
