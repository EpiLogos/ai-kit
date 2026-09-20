//! Real native processes and temporary files. Preparation is not save; a
//! routing receipt cannot satisfy reopen. No provider, model or user data.
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn command(cwd: &Path, args: &[&str], body: Option<&Value>) -> (i32, Value) {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("aikit"));
    command.args(args).arg("--json").current_dir(cwd)
        .env("AIKIT_HOME", cwd.join(".isolated-aikit"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(body) = body {
        child.stdin.take().unwrap().write_all(body.to_string().as_bytes()).unwrap();
    } else {
        drop(child.stdin.take());
    }
    let output = child.wait_with_output().unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!("{args:?}: {e}: {} / {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    });
    (output.status.code().unwrap_or(-1), reply)
}
fn initial() -> Value {
    json!({"profile":"okf-wiki/v1","keep_native_header":true,"objects":[
        {"object":"space","profile":"okf-wiki/v1","ref":"wiki:project","revision":1,"node_refs":["wiki:alpha","wiki:beta"]},
        {"object":"node","profile":"okf-wiki/v1","ref":"wiki:alpha","revision":1,"type":"Document","title":"Alpha","space_refs":["wiki:project"]},
        {"object":"node","profile":"okf-wiki/v1","ref":"wiki:beta","revision":1,"type":"Document","title":"Beta","space_refs":["wiki:project"]}
    ]})
}
fn fixture() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("wiki.json");
    fs::write(&path, initial().to_string()).unwrap();
    (temp, path)
}
fn request(revision: u64, operation: &str, changes: Value) -> Value {
    json!({"schema":"aikit.constellation-action/v1","frame_ref":"wiki:construction","expected_revision":revision,
        "operation_ref":operation,"actor_ref":"human:author","changes":changes})
}
fn create() -> Value {
    request(0, "action:create", json!([{"change":"create","anchor_ref":"wiki:whole",
        "title":"Counterpoint inquiry","inquiry":{"question":"How does Alpha qualify Beta?"},"space_refs":["wiki:project"]}]))
}
fn save(cwd: &Path, path: &Path, request: &Value) -> (i32, Value) {
    command(cwd, &["wiki-construct", "apply", "--file", path.to_str().unwrap()], Some(&json!({"request":request})))
}
fn member() -> Value {
    json!({"change":"member_add","member":{"subject_ref":"wiki:alpha","participation":{
        "participation_ref":"participation:alpha","sources":[{"source_ref":"central:source:alpha.md","source_revision":"r1",
        "aikit.techne-facet/v1":{"contract":"aikit.techne-facet/v1","selector":{"unit":"text_span","start":3,"end":21}}}]}}})
}

#[test]
fn prepare_without_native_save_cannot_reopen_or_change_the_file() {
    let (temp, path) = fixture();
    let before = fs::read(&path).unwrap();
    let (code, prepared) = command(temp.path(), &["wiki-construct", "apply"], Some(&json!({"document":initial(),"request":create()})));
    assert_eq!(code, 0, "{prepared}");
    assert_eq!(prepared["data"]["state"], "prepared");
    assert_eq!(prepared["data"]["persisted"], false);
    assert_eq!(fs::read(&path).unwrap(), before);
    let (code, absent) = command(temp.path(), &["wiki-construct", "inspect", "wiki:construction", "--file", path.to_str().unwrap()], None);
    assert_ne!(code, 0, "removing native save must break reopen: {absent}");
}

#[test]
fn saved_whole_reopens_in_another_process_and_return_is_idempotent() {
    let (temp, path) = fixture();
    let (code, saved) = save(temp.path(), &path, &create());
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["data"]["persisted"], true);
    assert_eq!(saved["data"]["revision"], 1);
    assert_eq!(saved["data"]["indexed_availability_proven"], false);
    let (code, edited) = save(temp.path(), &path, &request(1, "action:member", json!([member()])));
    assert_eq!(code, 0, "{edited}");
    let (code, reopened) = command(temp.path(), &["wiki-construct", "inspect", "wiki:construction", "--file", path.to_str().unwrap()], None);
    assert_eq!(code, 0, "{reopened}");
    assert_eq!(reopened["data"]["reading"], edited["data"]["reading"]);
    let before = fs::read(&path).unwrap();
    let (code, replay) = save(temp.path(), &path, &request(1, "action:member", json!([member()])));
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["data"]["state"], "unchanged");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(serde_json::from_slice::<Value>(&before).unwrap()["keep_native_header"], true);
}

#[test]
fn a_late_agent_revision_cannot_overwrite_a_human_change() {
    let (temp, path) = fixture();
    assert_eq!(save(temp.path(), &path, &create()).0, 0);
    assert_eq!(save(temp.path(), &path, &request(1, "human:move", json!([member()]))).0, 0);
    let before = fs::read(&path).unwrap();
    let mut late = request(1, "agent:late", json!([{"change":"inquiry_set","title":"Old work","inquiry":{"question":"An obsolete question"}}]));
    late["actor_ref"] = json!("agent:epii");
    let (code, refused) = save(temp.path(), &path, &late);
    assert_ne!(code, 0, "{refused}");
    assert_eq!(refused["error"]["code"], "knowledge.constellation_revision_conflict");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_readonly_or_malformed_action_never_changes_native_bytes() {
    let (temp, path) = fixture();
    assert_eq!(save(temp.path(), &path, &create()).0, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for object in document["objects"].as_array_mut().unwrap() {
        if object["ref"] == "wiki:construction" { object["shared_projection_ref"] = json!("projection:readonly"); }
    }
    fs::write(&path, document.to_string()).unwrap();
    let before = fs::read(&path).unwrap();
    assert_ne!(save(temp.path(), &path, &request(1, "agent:forbidden", json!([member()]))).0, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
    let (code, refused) = command(temp.path(), &["wiki-construct", "apply", "--file", path.to_str().unwrap()], Some(&json!({"document":initial(),"request":create()})));
    assert_ne!(code, 0, "two competing native inputs must be refused: {refused}");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlink_file_and_symlink_lock_are_refused_without_touching_target() {
    use std::os::unix::fs::symlink;
    let (temp, path) = fixture();
    let before = fs::read(&path).unwrap();
    let alias = temp.path().join("alias.json");
    symlink(&path, &alias).unwrap();
    assert_ne!(save(temp.path(), &alias, &create()).0, 0);
    symlink(&path, temp.path().join(".wiki.json.construction.lock")).unwrap();
    assert_ne!(save(temp.path(), &path, &create()).0, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
}
