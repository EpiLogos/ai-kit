//! Controlled native-owner integration, not model/harness or personal adoption proof.
use aikit_adapters::central_placement::{CentralTaskRequest, NativeCentralPlacement};
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::ResourceRef;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

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

fn world_with_protected_index() -> (tempfile::TempDir, CentralTaskRequest) {
    let (directory, request) = world();
    // Controlled authored input precedes actual native admission; it is not
    // a fabricated owner response or a change to the personal World's law.
    let path = request.central_root.join("Control/user/placement.json");
    let mut source: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    source["protected"]
        .as_array_mut()
        .unwrap()
        .push(json!("Control/relations/source-relations.json"));
    fs::write(path, source.to_string()).unwrap();
    (directory, request)
}

fn native_inspection(requirements: &Value, path: &std::path::Path) -> Value {
    fs::write(path, requirements.to_string()).unwrap();
    let binary = std::env::var("AIKIT_CAW_WORKCELL_BOUNDARY_BIN").expect("exact Workcell required");
    let output = SystemRunner::new()
        .with_timeout(Duration::from_secs(15))
        .run(&[
            binary,
            "inspect".into(),
            path.display().to_string(),
            requirements["policy_revision"].as_str().unwrap().into(),
        ])
        .unwrap();
    assert!(output.ok(), "{} {}", output.stdout, output.stderr);
    let native: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(native["requirements"], *requirements);
    assert_eq!(native["state"], "prepared-not-executed");
    native
}

fn native_protocol_execution(
    prepared: &Value,
    path: &std::path::Path,
    body: &str,
    arguments: &[PathBuf],
) -> Value {
    fs::write(path, prepared.to_string()).unwrap();
    let binary = std::env::var("AIKIT_CAW_WORKCELL_BOUNDARY_BIN").expect("exact Workcell required");
    // This finite OS driver only captures the actual native exec. It supplies
    // pipes required by that protocol boundary and never constructs an owner
    // success reply, model, Agency or Human decision.
    let driver = r#"
import json, subprocess, sys
result = subprocess.run(sys.argv[1:], input=b'', capture_output=True, timeout=15)
print(json.dumps({'returncode': result.returncode,
                  'stdout': result.stdout.decode('utf-8'),
                  'stderr': result.stderr.decode('utf-8')}))
"#;
    let mut argv = vec![
        "python3".into(),
        "-B".into(),
        "-c".into(),
        driver.into(),
        binary,
        "exec".into(),
        path.display().to_string(),
        prepared["requirements"]["policy_revision"]
            .as_str()
            .unwrap()
            .into(),
        prepared["requirements_digest"].as_str().unwrap().into(),
        "--".into(),
        "python3".into(),
        "-B".into(),
        "-c".into(),
        body.into(),
    ];
    argv.extend(arguments.iter().map(|path| path.display().to_string()));
    let output = SystemRunner::new()
        .with_timeout(Duration::from_secs(20))
        .with_env_removed("CENTRAL_NATIVE_TOKEN")
        .with_env_removed("WORKCELL_CONTROL_TOKEN")
        .run(&argv)
        .unwrap();
    assert!(output.ok(), "{} {}", output.stdout, output.stderr);
    serde_json::from_str(&output.stdout).unwrap()
}

#[cfg(unix)]
#[test]
#[ignore = "requires exact native Central and Workcell174+; mandatory CAW workflow"]
fn native_explicit_exclusion_retains_policy_and_real_child_continuation() {
    use std::os::unix::fs::MetadataExt;
    let (_directory, request) = world_with_protected_index();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let task = owner.allocate(&request).unwrap();
    let authority = ResourceRef::parse("authority:controlled-native-test").unwrap();
    let relations = request.central_root.join("Control/relations");
    let index = relations.join("source-relations.json");
    let legacy = owner
        .write_boundary_requirements(&task, &authority, &[])
        .unwrap();
    let empty = owner
        .write_boundary_requirements_with_additional_protection(&task, &authority, &[], &[])
        .unwrap();
    assert_eq!(empty, legacy);
    assert_eq!(
        legacy["protected_paths"],
        task.allocation["policy"]["protected_paths"]
    );
    let requirements = owner
        .write_boundary_requirements_with_additional_protection(
            &task,
            &authority,
            &[],
            &[relations.clone(), relations.clone()],
        )
        .unwrap();
    let mut expected = legacy["protected_paths"].as_array().unwrap().clone();
    if !expected.contains(&json!(relations)) {
        expected.push(json!(relations));
    }
    assert_eq!(requirements["protected_paths"], json!(expected));
    for key in [
        "policy_ref",
        "policy_revision",
        "authority_ref",
        "required_coverage",
        "expires_at_unix_ms",
        "writable_paths",
    ] {
        assert_eq!(requirements[key], legacy[key], "{key}");
    }
    let legacy_prepared = native_inspection(&legacy, &request.central_root.join("legacy-req.json"));
    let prepared = native_inspection(
        &requirements,
        &request.central_root.join("augmented-req.json"),
    );
    assert_ne!(
        prepared["requirements_digest"],
        legacy_prepared["requirements_digest"]
    );
    let covered = prepared["protected_objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == json!(index))
        .unwrap();
    assert_eq!(covered["protection_basis"]["path"], json!(relations));
    let parent_identity = fs::metadata(&relations).unwrap().ino();
    let retained_index = fs::File::open(&index).unwrap();
    let old_index_identity = retained_index.metadata().unwrap().ino();
    let partial = task.now_directory().unwrap().join("partial-return.bin");
    fs::write(&partial, b"retained partial\0before").unwrap();
    let mut child_request = request.clone();
    child_request.task_ref = ResourceRef::parse("task:native-child-allocation").unwrap();
    child_request.purpose = "Real native allocation changes its registered source index".into();
    let native_child = SystemRunner::new()
        .with_timeout(Duration::from_secs(15))
        .run(&[
            request.ctrl_bin.display().to_string(),
            "--json".into(),
            "--root".into(),
            request.central_root.display().to_string(),
            "action".into(),
            "run".into(),
            "central.now.allocate".into(),
            json!({
                "task_ref": child_request.task_ref,
                "purpose": child_request.purpose,
                "participant_refs": child_request.participant_refs,
                "source_refs": child_request.source_refs,
                "parent_now_ref": task.allocation["now_ref"],
                "workcell_ref": "workcell:controlled-native-test",
                "expected_policy_revision": task.allocation["policy"]["revision"]
            })
            .to_string(),
        ])
        .unwrap();
    assert!(
        native_child.ok(),
        "{} {}",
        native_child.stdout,
        native_child.stderr
    );
    let native_child: Value = serde_json::from_str(&native_child.stdout).unwrap();
    assert_eq!(native_child["ok"], true);
    assert_eq!(native_child["data"]["created"], true);
    let child = owner.allocate(&child_request).unwrap();
    assert_eq!(child.allocation["created"], false);
    assert_eq!(
        child.allocation["record"]["parent_now_ref"],
        task.allocation["now_ref"]
    );
    assert_eq!(child.allocation["record"]["horizon"], "child");
    assert_ne!(fs::metadata(&index).unwrap().ino(), old_index_identity);
    assert_eq!(fs::metadata(&relations).unwrap().ino(), parent_identity);
    let index_after_owner_effect = fs::read(&index).unwrap();
    let retained = owner
        .write_boundary_requirements_with_additional_protection(
            &task,
            &authority,
            &[],
            &[relations],
        )
        .unwrap();
    assert_eq!(retained, requirements);
    assert_eq!(
        native_inspection(&retained, &request.central_root.join("after-req.json")),
        prepared
    );
    let marker = task.now_directory().unwrap().join("stale-exec-marker");
    let refused = native_protocol_execution(
        &legacy_prepared,
        &request.central_root.join("legacy-prepared.json"),
        "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('escaped stale admission')",
        std::slice::from_ref(&marker),
    );
    assert_ne!(refused["returncode"], 0);
    assert!(refused["stderr"]
        .as_str()
        .unwrap()
        .contains("protected_objects changed"));
    assert!(!marker.exists());
    let body = r#"
import json, os, pathlib, subprocess, sys
partial = pathlib.Path(sys.argv[1]); source = pathlib.Path(sys.argv[2])
with partial.open('ab') as stream: stream.write(b'-PARENT')
child = subprocess.run([sys.executable, '-B', '-c', '''
import json, os, pathlib, sys
with pathlib.Path(sys.argv[1]).open('ab') as stream: stream.write(b'-CHILD')
try: pathlib.Path(sys.argv[2]).write_text('forbidden child rewrite')
except PermissionError: print(json.dumps({'pid':os.getpid(),'denied':True}))
else: raise AssertionError('protected source escaped')
''', str(partial), str(source)], capture_output=True, text=True, timeout=10)
assert child.returncode == 0, child.stderr
print(json.dumps({'parent_pid':os.getpid(),'child':json.loads(child.stdout)}))
"#;
    let executed = native_protocol_execution(
        &prepared,
        &request.central_root.join("augmented-prepared.json"),
        body,
        &[partial.clone(), index.clone()],
    );
    assert_eq!(executed["returncode"], 0, "{executed}");
    let processes: Value = serde_json::from_str(executed["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(processes["child"]["denied"], true);
    assert_ne!(processes["parent_pid"], processes["child"]["pid"]);
    assert_eq!(
        fs::read(&partial).unwrap(),
        b"retained partial\0before-PARENT-CHILD"
    );
    assert_eq!(fs::read(&index).unwrap(), index_after_owner_effect);
    drop(retained_index);
    println!("NATIVE_EXPLICIT_EXCLUSION_CONTINUED: actual native owner index effect and confined OS parent/child; no model or Factory leg");
}

#[cfg(unix)]
#[test]
#[ignore = "requires exact native Central; mandatory CAW workflow"]
fn native_explicit_exclusions_refuse_aliases_overlap_and_merged_limit() {
    let (_directory, request) = world_with_protected_index();
    let owner = NativeCentralPlacement::new(SystemRunner::new());
    let task = owner.allocate(&request).unwrap();
    let authority = ResourceRef::parse("authority:controlled-native-test").unwrap();
    let now = task.now_directory().unwrap();
    let relations = request.central_root.join("Control/relations");
    let alias = request.central_root.join("relations-alias");
    std::os::unix::fs::symlink(&relations, &alias).unwrap();
    let writable_child = now.join("writable-child");
    fs::create_dir(&writable_child).unwrap();
    let ancestor_alias = request.central_root.join("root-alias");
    std::os::unix::fs::symlink(&request.central_root, &ancestor_alias).unwrap();
    let index = relations.join("source-relations.json");
    let original_index = fs::read(&index).unwrap();
    let original_now = fs::read(
        request
            .central_root
            .join(task.allocation["source"]["path"].as_str().unwrap()),
    )
    .unwrap();
    for refused in [
        request.central_root.join("absent"),
        index.clone(),
        alias,
        ancestor_alias.join("Control/relations"),
        now.clone(),
        writable_child,
        now.parent().unwrap().to_path_buf(),
    ] {
        assert!(
            owner
                .write_boundary_requirements_with_additional_protection(
                    &task,
                    &authority,
                    &[],
                    std::slice::from_ref(&refused)
                )
                .is_err(),
            "{}",
            refused.display()
        );
        assert_eq!(fs::read(&index).unwrap(), original_index);
    }
    let mut exclusions = Vec::new();
    for n in 0..64 {
        let path = request.central_root.join(format!("excluded-{n}"));
        fs::create_dir(&path).unwrap();
        exclusions.push(path);
    }
    let failure = owner
        .write_boundary_requirements_with_additional_protection(&task, &authority, &[], &exclusions)
        .unwrap_err();
    assert!(failure.message().contains("64 total"), "{failure}");
    assert_eq!(fs::read(&index).unwrap(), original_index);
    assert_eq!(
        fs::read(
            request
                .central_root
                .join(task.allocation["source"]["path"].as_str().unwrap())
        )
        .unwrap(),
        original_now
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires exact native Central and Workcell174+; mandatory CAW workflow"]
fn native_explicit_exclusion_preserves_hardlink_and_parent_identity_fences() {
    for change in ["hardlink", "parent"] {
        let (_directory, request) = world_with_protected_index();
        let owner = NativeCentralPlacement::new(SystemRunner::new());
        let task = owner.allocate(&request).unwrap();
        let authority = ResourceRef::parse("authority:controlled-native-test").unwrap();
        let relations = request.central_root.join("Control/relations");
        let index = relations.join("source-relations.json");
        let requirements = owner
            .write_boundary_requirements_with_additional_protection(
                &task,
                &authority,
                &[],
                std::slice::from_ref(&relations),
            )
            .unwrap();
        let prepared = native_inspection(
            &requirements,
            &request.central_root.join("requirements.json"),
        );
        let original_bytes = fs::read(&index).unwrap();
        if change == "hardlink" {
            fs::hard_link(&index, task.now_directory().unwrap().join("index-alias")).unwrap();
        } else {
            fs::rename(&relations, request.central_root.join("retained-relations")).unwrap();
            fs::create_dir(&relations).unwrap();
            fs::write(&index, &original_bytes).unwrap();
        }
        let marker = task.now_directory().unwrap().join("must-not-execute");
        let refused = native_protocol_execution(
            &prepared,
            &request.central_root.join("prepared.json"),
            "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('escaped admission')",
            std::slice::from_ref(&marker),
        );
        assert_ne!(refused["returncode"], 0, "{change}: {refused}");
        assert!(
            refused["stderr"]
                .as_str()
                .unwrap()
                .contains("protected_objects changed"),
            "{change}: {refused}"
        );
        assert!(!marker.exists(), "{change}");
        assert_eq!(fs::read(&index).unwrap(), original_bytes);
    }
}
