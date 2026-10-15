//! `aikit knowledge relations-batch` answers each item with exactly the
//! envelope its single `knowledge relations` invocation prints — the real
//! binary, a real project on disk, real processes — and one item's refusal
//! never sinks the others.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

struct World {
    _temp: tempfile::TempDir,
    project: std::path::PathBuf,
    user_home: std::path::PathBuf,
    aikit_home: std::path::PathBuf,
    bkmr: std::path::PathBuf,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let user_home = temp.path().join("user");
    let aikit_home = temp.path().join("aikit-home");
    let bkmr = temp.path().join("bkmr");
    for directory in [&project, &user_home, &aikit_home, &bkmr] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(
        project.join("semantic-wiki.json"),
        json!({"objects": [
            {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:batch","revision":2,
             "provenance":[],"title":"Batch","parent_space_refs":[],"child_space_refs":[],
             "node_refs":["wiki:node:alpha","wiki:node:beta"]},
            {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:alpha","revision":1,
             "provenance":[],"type":"Concept","title":"Alpha","space_refs":["wiki:space:batch"],
             "source_refs":[]},
            {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:beta","revision":1,
             "provenance":[],"type":"Concept","title":"Beta","space_refs":["wiki:space:batch"],
             "source_refs":[]},
            {"profile":"okf-wiki/v1","object":"edge","ref":"wiki:edge:alpha-beta","revision":1,
             "provenance":[],"from_ref":"wiki:node:alpha","to_ref":"wiki:node:beta",
             "relation":"develops","origin":"authored"}
        ]})
        .to_string(),
    )
    .unwrap();
    World {
        _temp: temp,
        project,
        user_home,
        aikit_home,
        bkmr,
    }
}

fn aikit(world: &World) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
    command
        .env("HOME", &world.user_home)
        .env("AIKIT_HOME", &world.aikit_home)
        .env("AIKIT_BKMR_CONFIG_DIR", &world.bkmr)
        .env_remove("CENTRAL_ROOT")
        .env_remove("AIKIT_CONTEXT_ID")
        .stdin(Stdio::null());
    command
}

/// The single form, exactly as O:I invokes it: one envelope line and a status.
fn single(world: &World, cwd: &Path, address: &str) -> (Value, i32) {
    let output = aikit(world)
        .args(["--json", "-C"])
        .arg(cwd)
        .args([
            "knowledge",
            "relations",
            "--depth",
            "2",
            "--max-nodes",
            "96",
            "--max-edges",
            "192",
            "--",
            address,
        ])
        .output()
        .unwrap();
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        output.status.code().unwrap(),
    )
}

fn batch(world: &World, request: &Value) -> (Value, i32) {
    let mut child = aikit(world)
        .args(["--json", "-C"])
        .arg(&world.project)
        .args(["knowledge", "relations-batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "batch printed no envelope ({error}): {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }),
        output.status.code().unwrap(),
    )
}

/// A fresh context id is minted per invocation; everything else is the answer.
fn without_context_id(mut envelope: Value) -> Value {
    if let Some(context) = envelope.get_mut("context") {
        context["context_id"] = Value::Null;
    }
    envelope
}

#[test]
fn each_batch_answer_is_the_envelope_its_single_invocation_prints() {
    let world = world();
    let space = r#"{"kind":"wiki","value":"wiki:space:batch"}"#;
    let missing = r#"{"kind":"wiki","value":"wiki:node:absent"}"#;
    let (space_single, space_status) = single(&world, &world.project, space);
    let (missing_single, missing_status) = single(&world, &world.project, missing);
    let (garbled_single, garbled_status) = single(&world, &world.project, "not-an-address");
    assert_eq!(space_status, 0);
    assert_eq!(space_single["ok"], true);
    assert!(
        space_single["data"]["edges"].as_array().unwrap().len() >= 3,
        "the space reaches its members and their authored edge: {space_single}"
    );
    assert_ne!(missing_status, 0);
    assert_eq!(missing_single["ok"], false);
    assert_ne!(garbled_status, 0);
    assert_eq!(garbled_single["error"]["code"], "knowledge.invalid_address");

    let request = json!({
        "schema": "aikit.knowledge-relations-batch/v1",
        "items": [
            {"cwd": world.project, "address": space, "depth": 2, "max_nodes": 96, "max_edges": 192},
            {"cwd": world.project, "address": "not-an-address", "depth": 2, "max_nodes": 96, "max_edges": 192},
            {"cwd": world.project, "address": missing, "depth": 2, "max_nodes": 96, "max_edges": 192},
            {"cwd": world.project, "address": {"kind":"wiki","value":"wiki:space:batch"},
             "depth": 2, "max_nodes": 96, "max_edges": 192}
        ]
    });
    let (envelope, status) = batch(&world, &request);
    assert_eq!(
        status, 0,
        "a refused item never sinks the batch: {envelope}"
    );
    assert_eq!(envelope["ok"], true);
    assert_eq!(
        envelope["data"]["schema"],
        "aikit.knowledge-relations-batch/v1"
    );
    let answers = envelope["data"]["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 4);
    assert_eq!(
        without_context_id(answers[0].clone()),
        without_context_id(space_single.clone())
    );
    assert_eq!(answers[1], garbled_single);
    assert_eq!(answers[2], missing_single);
    assert_eq!(
        without_context_id(answers[3].clone()),
        without_context_id(space_single),
        "the typed address object is the same ADDRESS"
    );
}

#[test]
fn a_malformed_batch_is_refused_whole_with_a_named_code() {
    let world = world();
    let (envelope, status) = batch(&world, &json!({"schema": "something-else", "items": []}));
    assert_ne!(status, 0);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "knowledge.relations_batch_input");
}
