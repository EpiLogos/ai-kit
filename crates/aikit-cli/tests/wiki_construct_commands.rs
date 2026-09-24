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
    command
        .args(args)
        .arg("--json")
        .current_dir(cwd)
        .env("AIKIT_HOME", cwd.join(".isolated-aikit"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(body) = body {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.to_string().as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let output = child.wait_with_output().unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{args:?}: {e}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
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
    request(
        0,
        "action:create",
        json!([{"change":"create","anchor_ref":"wiki:whole",
        "title":"Counterpoint inquiry","inquiry":{"question":"How does Alpha qualify Beta?"},"space_refs":["wiki:project"]}]),
    )
}
fn save(cwd: &Path, path: &Path, request: &Value) -> (i32, Value) {
    command(
        cwd,
        &["wiki-construct", "apply", "--file", path.to_str().unwrap()],
        Some(&json!({"request":request})),
    )
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
    let (code, prepared) = command(
        temp.path(),
        &["wiki-construct", "apply"],
        Some(&json!({"document":initial(),"request":create()})),
    );
    assert_eq!(code, 0, "{prepared}");
    assert_eq!(prepared["data"]["state"], "prepared");
    assert_eq!(prepared["data"]["persisted"], false);
    assert_eq!(fs::read(&path).unwrap(), before);
    let (code, absent) = command(
        temp.path(),
        &[
            "wiki-construct",
            "inspect",
            "wiki:construction",
            "--file",
            path.to_str().unwrap(),
        ],
        None,
    );
    assert_ne!(code, 0, "removing native save must break reopen: {absent}");
}

#[test]
fn native_node_and_unconstructed_whole_facts_reopen_preserve_scope_and_refuse_stale_writes() {
    let (temp, path) = fixture();
    let mut document = initial();
    document["objects"][1]["native_extra"] = json!({"keep":"node property"});
    document["objects"][1]["provenance"] =
        json!([{"source_ref":"central:source:alpha.md","source_revision":"r1"}]);
    document["objects"].as_array_mut().unwrap().extend([
        json!({"object":"frame","profile":"okf-wiki/v1","ref":"wiki:plain-whole","revision":1,"member_refs":["wiki:alpha","wiki:beta"],"native_extra":{"keep":"whole property"}}),
        json!({"object":"frame","profile":"okf-wiki/v1","ref":"wiki:other-whole","revision":1,"member_refs":["wiki:alpha"]}),
    ]);
    fs::write(&path, document.to_string()).unwrap();
    let original = fs::read_to_string(&path).unwrap();
    let facts_request = |kind: &str,
                         reference: &str,
                         revision: u64,
                         operation: &str,
                         changes: Value| json!({"schema":"aikit.wiki-facts-action/v1","target":{"kind":kind,"ref":reference},"expected_revision":revision,"actor_ref":"human:author","operation_ref":operation,"changes":changes});
    let temporal = json!([{"kind":"occurrence","instant":"2024-01-02T03:04:05Z","precision":"minute","source_ref":"central:source:alpha.md"}]);
    let places = json!([{"place_ref":"place:declared","precision":"approximate","uncertainty":"The witness described the wider area, not an exact point.","source_ref":"central:source:alpha.md"}]);
    let node = facts_request(
        "node",
        "wiki:alpha",
        1,
        "action:node-facts",
        json!([{"change":"temporal_set","temporal":temporal},{"change":"place_set","places":places}]),
    );
    let apply = |request: &Value, basis: &str| {
        command(
            temp.path(),
            &[
                "wiki-construct",
                "facts-apply",
                "--file",
                path.to_str().unwrap(),
            ],
            Some(&json!({"request":request,"basis_content":basis})),
        )
    };
    let (code, saved) = apply(&node, &original);
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["data"]["revision"], 2);
    let first = fs::read_to_string(&path).unwrap();
    let (code, reopened) = command(
        temp.path(),
        &[
            "wiki-construct",
            "facts-inspect",
            "wiki:alpha",
            "--file",
            path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "{reopened}");
    let row = &reopened["data"]["reading"]["object"];
    assert_eq!(row["object"], "node");
    assert_eq!(row["revision"], 2);
    assert_eq!(row["aikit.techne-facet/v1"]["temporal"], temporal);
    assert_eq!(row["aikit.techne-facet/v1"]["spatial"], places);
    assert_eq!(row["native_extra"], document["objects"][1]["native_extra"]);
    assert_eq!(row["provenance"], document["objects"][1]["provenance"]);
    let whole = facts_request(
        "whole",
        "wiki:plain-whole",
        1,
        "action:whole-facts",
        json!([{"change":"temporal_set","temporal":[{"kind":"valid","interval":{"from":"1900-01-01T00:00:00Z","to":"1950-01-01T00:00:00Z"},"source_ref":"central:source:alpha.md"}]}]),
    );
    let (code, saved) = apply(&whole, &first);
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["data"]["reading"]["object"]["revision"], 2);
    assert!(
        saved["data"]["reading"]["object"]
            .get("aikit.constellation/v1")
            .is_none(),
        "no dummy construction is invented"
    );
    let after = fs::read_to_string(&path).unwrap();
    let actual: Value = serde_json::from_str(&after).unwrap();
    for index in [0, 2, 4] {
        let reference = &document["objects"][index]["ref"];
        let found = actual["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| &r["ref"] == reference)
            .unwrap();
        // Native rendering may materialise empty default arrays. Parse through
        // the owner model to compare semantic objects without string snapshots.
        assert_eq!(
            aikit_core::WikiObject::parse(found).unwrap(),
            aikit_core::WikiObject::parse(&document["objects"][index]).unwrap()
        );
    }
    let (code, replay) = apply(&node, &original);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["data"]["state"], "unchanged");
    assert_eq!(fs::read_to_string(&path).unwrap(), after);
    for mut bad in [node.clone(), whole.clone()] {
        bad["operation_ref"] = json!("action:stale");
        let (code, response) = apply(&bad, &after);
        assert_ne!(code, 0, "{response}");
        assert_eq!(fs::read_to_string(&path).unwrap(), after);
    }
    let mut conflict = node.clone();
    conflict["changes"][0]["temporal"] = json!([]);
    assert_ne!(
        apply(&conflict, &after).0,
        0,
        "same operation cannot name changed intent"
    );
    let mut wrong_kind = facts_request(
        "whole",
        "wiki:alpha",
        2,
        "action:wrong-kind",
        json!([{"change":"temporal_set","temporal":[]}]),
    );
    assert_ne!(apply(&wrong_kind, &after).0, 0);
    wrong_kind["target"]["kind"] = json!("node");
    wrong_kind["changes"][0]["temporal"] =
        json!([{"kind":"occurrence","instant":"2024-01-01T00:00:00Z"}]);
    assert_ne!(
        apply(&wrong_kind, &after).0,
        0,
        "source-free claim must refuse"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), after);
    let clear = facts_request(
        "node",
        "wiki:alpha",
        2,
        "action:clear-time",
        json!([{"change":"temporal_set","temporal":[]}]),
    );
    assert_ne!(
        apply(&clear, &original).0,
        0,
        "stale register bytes must refuse even with current object revision"
    );
    let (code, cleared) = apply(&clear, &after);
    assert_eq!(code, 0, "{cleared}");
    assert!(
        cleared["data"]["reading"]["object"]["aikit.techne-facet/v1"]
            .get("temporal")
            .is_none()
    );
    assert_eq!(
        cleared["data"]["reading"]["object"]["aikit.techne-facet/v1"]["spatial"],
        places
    );
}

#[test]
fn saved_whole_reopens_in_another_process_and_return_is_idempotent() {
    let (temp, path) = fixture();
    let (code, saved) = save(temp.path(), &path, &create());
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["data"]["persisted"], true);
    assert_eq!(saved["data"]["revision"], 1);
    assert_eq!(saved["data"]["indexed_availability_proven"], false);
    let (code, edited) = save(
        temp.path(),
        &path,
        &request(1, "action:member", json!([member()])),
    );
    assert_eq!(code, 0, "{edited}");
    let (code, reopened) = command(
        temp.path(),
        &[
            "wiki-construct",
            "inspect",
            "wiki:construction",
            "--file",
            path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "{reopened}");
    assert_eq!(reopened["data"]["reading"], edited["data"]["reading"]);
    let before = fs::read(&path).unwrap();
    let (code, replay) = save(
        temp.path(),
        &path,
        &request(1, "action:member", json!([member()])),
    );
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["data"]["state"], "unchanged");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        serde_json::from_slice::<Value>(&before).unwrap()["keep_native_header"],
        true
    );
}

#[test]
fn a_late_agent_revision_cannot_overwrite_a_human_change() {
    let (temp, path) = fixture();
    assert_eq!(save(temp.path(), &path, &create()).0, 0);
    assert_eq!(
        save(
            temp.path(),
            &path,
            &request(1, "human:move", json!([member()]))
        )
        .0,
        0
    );
    let before = fs::read(&path).unwrap();
    let mut late = request(
        1,
        "agent:late",
        json!([{"change":"inquiry_set","title":"Old work","inquiry":{"question":"An obsolete question"}}]),
    );
    late["actor_ref"] = json!("agent:epii");
    let (code, refused) = save(temp.path(), &path, &late);
    assert_ne!(code, 0, "{refused}");
    assert_eq!(
        refused["error"]["code"],
        "knowledge.constellation_revision_conflict"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_readonly_or_malformed_action_never_changes_native_bytes() {
    let (temp, path) = fixture();
    assert_eq!(save(temp.path(), &path, &create()).0, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for object in document["objects"].as_array_mut().unwrap() {
        if object["ref"] == "wiki:construction" {
            object["shared_projection_ref"] = json!("projection:readonly");
        }
    }
    fs::write(&path, document.to_string()).unwrap();
    let before = fs::read(&path).unwrap();
    assert_ne!(
        save(
            temp.path(),
            &path,
            &request(1, "agent:forbidden", json!([member()]))
        )
        .0,
        0
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let (code, refused) = command(
        temp.path(),
        &["wiki-construct", "apply", "--file", path.to_str().unwrap()],
        Some(&json!({"document":initial(),"request":create()})),
    );
    assert_ne!(
        code, 0,
        "two competing native inputs must be refused: {refused}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn native_facts_and_construction_node_targets_refuse_readonly_source_objects() {
    for (flag, value) in [
        ("read_only", json!(true)),
        ("shared_projection_ref", json!("projection:shared")),
    ] {
        let (temp, path) = fixture();
        assert_eq!(save(temp.path(), &path, &create()).0, 0);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for row in document["objects"].as_array_mut().unwrap() {
            if row["ref"] == "wiki:alpha" || row["ref"] == "wiki:construction" {
                row[flag] = value.clone();
            }
        }
        fs::write(&path, document.to_string()).unwrap();
        let before = fs::read(&path).unwrap();
        for (kind, reference) in [("node", "wiki:alpha"), ("whole", "wiki:construction")] {
            let (code, response) = command(
                temp.path(),
                &[
                    "wiki-construct",
                    "facts-apply",
                    "--file",
                    path.to_str().unwrap(),
                ],
                Some(&json!({"request":{
                    "schema":"aikit.wiki-facts-action/v1","target":{"kind":kind,"ref":reference},"expected_revision":1,
                    "actor_ref":"human:author","operation_ref":"action:readonly-facts","changes":[{"change":"temporal_set","temporal":[]}]
                }})),
            );
            assert_ne!(code, 0, "{response}");
            assert!(
                response.to_string().contains("read-only shared material"),
                "{response}"
            );
            assert_eq!(fs::read(&path).unwrap(), before);
        }
        // Make only the containing frame writable. Its ordinary edit authority
        // must not grant authority over a separately protected source node.
        for row in document["objects"].as_array_mut().unwrap() {
            if row["ref"] == "wiki:construction" {
                row.as_object_mut().unwrap().remove(flag);
            }
        }
        fs::write(&path, document.to_string()).unwrap();
        let before = fs::read(&path).unwrap();
        let (code, response) = save(
            temp.path(),
            &path,
            &request(
                1,
                "action:readonly-node",
                json!([
                    {"change":"place_set","target":{"kind":"node","node_ref":"wiki:alpha","expected_revision":1},"places":[]}
                ]),
            ),
        );
        assert_ne!(code, 0, "{response}");
        assert!(
            response.to_string().contains("read-only shared material"),
            "{response}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn native_facts_refuse_register_growth_beyond_the_existing_read_budget() {
    let (temp, path) = fixture();
    let mut document = initial();
    document["retained_native_material"] = json!("x".repeat(9 * 1024 * 1024));
    fs::write(&path, document.to_string()).unwrap();
    let before = fs::read(&path).unwrap();
    let (code, response) = command(
        temp.path(),
        &[
            "wiki-construct",
            "facts-apply",
            "--file",
            path.to_str().unwrap(),
        ],
        Some(&json!({"request":{
            "schema":"aikit.wiki-facts-action/v1","target":{"kind":"node","ref":"wiki:alpha"},"expected_revision":1,
            "actor_ref":"human:author","operation_ref":"action:too-large","changes":[{"change":"place_set","places":[{
                "place_ref":"place:declared","precision":"unlocated","source_ref":"central:source:alpha.md","uncertainty":"y".repeat(8*1024*1024)
            }]}]
        }})),
    );
    assert_ne!(code, 0, "{response}");
    assert_eq!(response["error"]["code"], "knowledge.constellation_budget");
    assert!(
        response.to_string().contains("resulting Wiki register"),
        "must reach the output budget check, not reject its bounded input: {response}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let (code, reopened) = command(
        temp.path(),
        &[
            "wiki-construct",
            "facts-inspect",
            "wiki:alpha",
            "--file",
            path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "{reopened}");
    assert_eq!(reopened["data"]["reading"]["object"]["revision"], 1);
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
