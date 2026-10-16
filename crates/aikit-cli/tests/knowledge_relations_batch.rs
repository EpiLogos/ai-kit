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

/// The same request, streamed (v2): one line per item as it is answered,
/// each carrying exactly the envelope the v1 document carries for that item,
/// then the closing envelope. Returns the item lines by index and the
/// closing envelope.
fn stream(world: &World, request: &Value) -> (Vec<Option<Value>>, Value, i32) {
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
    let text = String::from_utf8(output.stdout).unwrap();
    let mut lines: Vec<Value> = text
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("every streamed line is whole JSON ({error}): {line}")
            })
        })
        .collect();
    let closing = lines.pop().expect("a closing envelope");
    let mut items = vec![None; request["items"].as_array().unwrap().len()];
    for line in lines {
        assert_eq!(line["schema"], "aikit.knowledge-relations-batch/v2");
        let index = line["index"].as_u64().unwrap() as usize;
        assert!(items[index].is_none(), "item {index} streamed once");
        items[index] = Some(line["answer"].clone());
    }
    (items, closing, output.status.code().unwrap())
}

#[test]
fn a_streamed_batch_answers_each_item_on_its_own_line_with_the_v1_envelope() {
    let world = world();
    let space = r#"{"kind":"wiki","value":"wiki:space:batch"}"#;
    let missing = r#"{"kind":"wiki","value":"wiki:node:absent"}"#;
    let items = json!([
        {"cwd": world.project, "address": space, "depth": 2, "max_nodes": 96, "max_edges": 192},
        {"cwd": world.project, "address": "not-an-address", "depth": 2, "max_nodes": 96, "max_edges": 192},
        {"cwd": world.project, "address": missing, "depth": 2, "max_nodes": 96, "max_edges": 192}
    ]);
    let (document, status) = batch(
        &world,
        &json!({"schema": "aikit.knowledge-relations-batch/v1", "items": items}),
    );
    assert_eq!(status, 0);
    let (streamed, closing, status) = stream(
        &world,
        &json!({"schema": "aikit.knowledge-relations-batch/v2", "items": items}),
    );
    assert_eq!(
        status, 0,
        "a refused item never sinks the stream: {closing}"
    );
    assert_eq!(closing["ok"], true);
    assert_eq!(
        closing["data"],
        json!({"schema": "aikit.knowledge-relations-batch/v2", "answered": 3})
    );
    let answers = document["data"]["answers"].as_array().unwrap();
    for (index, answer) in answers.iter().enumerate() {
        let line = streamed[index]
            .clone()
            .unwrap_or_else(|| panic!("item {index} was streamed"));
        assert_eq!(
            without_context_id(line),
            without_context_id(answer.clone()),
            "item {index}"
        );
    }
}

/// A Central world of two Projects whose native World owner is a stub that
/// records every action it is asked.
struct CentralWorld {
    world: World,
    root: std::path::PathBuf,
    ctrl: std::path::PathBuf,
    log: std::path::PathBuf,
}

fn central_world() -> CentralWorld {
    let world = world();
    let root = world._temp.path().join("central");
    for directory in ["Control", "Work"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    for member in ["cedar", "larch"] {
        let project = root.join("Work").join(member);
        fs::create_dir_all(project.join("ProjectCentral")).unwrap();
        fs::write(
            project.join("ProjectCentral/project.json"),
            json!({"schema": "central.project/v1", "project_id": format!("epilogos/{member}"),
                   "human_source": "ProjectCentral/user",
                   "wiki": {"profile": "okf-wiki/v1",
                            "source": "ProjectCentral/agents/wiki/wiki.json",
                            "adopted_sources": []}})
            .to_string(),
        )
        .unwrap();
        fs::create_dir_all(project.join("ProjectCentral/agents/wiki")).unwrap();
        fs::write(
            project.join("ProjectCentral/agents/wiki/wiki.json"),
            json!({"objects": [
                {"profile":"okf-wiki/v1","object":"space","ref":format!("wiki:space:{member}"),
                 "revision":1,"provenance":[],"title":member,"parent_space_refs":[],
                 "child_space_refs":[],"node_refs":[format!("wiki:node:{member}-alpha"),
                 format!("wiki:node:{member}-beta")]},
                {"profile":"okf-wiki/v1","object":"node","ref":format!("wiki:node:{member}-alpha"),
                 "revision":1,"provenance":[],"type":"Concept","title":"Alpha",
                 "space_refs":[format!("wiki:space:{member}")],"source_refs":[]},
                {"profile":"okf-wiki/v1","object":"node","ref":format!("wiki:node:{member}-beta"),
                 "revision":1,"provenance":[],"type":"Concept","title":"Beta",
                 "space_refs":[format!("wiki:space:{member}")],"source_refs":[]},
                {"profile":"okf-wiki/v1","object":"edge","ref":format!("wiki:edge:{member}"),
                 "revision":1,"provenance":[],"from_ref":format!("wiki:node:{member}-alpha"),
                 "to_ref":format!("wiki:node:{member}-beta"),"relation":"develops",
                 "origin":"authored"}
            ]})
            .to_string(),
        )
        .unwrap();
    }
    // Central's world map: the two Projects' wiki registers.
    let map = world._temp.path().join("world-map.json");
    let projects: Vec<Value> = ["cedar", "larch"]
        .iter()
        .map(|member| {
            json!({
                "name": member,
                "path": format!("Work/{member}"),
                "projectcentral": {"agent_wiki": {"wiki": {
                    "present": true,
                    "path": format!("Work/{member}/ProjectCentral/agents/wiki/wiki.json"),
                    "space_ref": format!("wiki:space:{member}")}}}
            })
        })
        .collect();
    fs::write(
        &map,
        json!({"ok": true, "data": {
            "schema": "central.world-map/v1",
            "root": root,
            "control": {"agent_wiki": {"wiki": {"present": false}}},
            "work": {"projects": projects}
        }})
        .to_string(),
    )
    .unwrap();
    let log = world._temp.path().join("ctrl.log");
    let ctrl = world._temp.path().join("bin/ctrl-stub");
    fs::create_dir_all(ctrl.parent().unwrap()).unwrap();
    fs::write(
        &ctrl,
        format!(
            r#"#!/bin/sh
# argv: --json --root <root> action run <action> '<input>'
action="$6"
input="$7"
printf '%s %s\n' "$action" "$input" >> '{log}'
case "$action" in
  central.world)
    cat '{map}'
    ;;
  central.world.here)
    project=$(printf '%s' "$input" | sed -n 's/.*"project":"\([^"]*\)".*/\1/p')
    printf '%s' "{{\"ok\":true,\"data\":{{\"schema\":\"central.world-here/v1\",\"project_world\":{{\"name\":\"$project\",\"state\":\"present\",\"ref\":\"project:$project\"}}}}}}"
    ;;
  central.world.effective-sources)
    world=$(printf '%s' "$input" | sed -n 's/.*"world_ref":"\([^"]*\)".*/\1/p')
    printf '%s' '{{"ok":true,"data":{{"world_ref":"'"$world"'","sources":[{{"ref":"central:source:control:root:Control","state":"available","effective_revision":"stub-rev-1","effective_source_world":"control:root","authority":"stub-fixture","source_treatment":"canonical","effective_treatment":"canonical","propagation_path":["control:root"],"provenance":[]}}]}}}}'
    ;;
  *)
    printf '%s' '{{"ok":false,"error":{{"code":"central.stub_unanswered","message":"the stub answers no other action"}}}}'
    exit 2
    ;;
esac
"#,
            log = log.display(),
            map = map.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ctrl, fs::Permissions::from_mode(0o755)).unwrap();
    }
    CentralWorld {
        world,
        root,
        ctrl,
        log,
    }
}

fn central_aikit(central: &CentralWorld) -> Command {
    let mut command = aikit(&central.world);
    command
        .env("CENTRAL_ROOT", &central.root)
        .env("CENTRAL_CTRL_BIN", &central.ctrl)
        .env_remove("OI_CENTRAL_CTRL_BIN");
    command
}

/// How many times the stub was asked `central.world.here` for each Project
/// since the log was last cleared, and clear it.
fn world_here_reads(central: &CentralWorld) -> std::collections::BTreeMap<String, usize> {
    let log = fs::read_to_string(&central.log).unwrap_or_default();
    let _ = fs::remove_file(&central.log);
    let mut reads = std::collections::BTreeMap::new();
    for line in log.lines() {
        if let Some(input) = line.strip_prefix("central.world.here ") {
            let input: Value = serde_json::from_str(input).unwrap();
            *reads
                .entry(input["project"].as_str().unwrap().to_owned())
                .or_insert(0) += 1;
        }
    }
    reads
}

#[test]
fn a_batch_reads_each_projects_world_facet_once_and_answers_as_its_singles_do() {
    let central = central_world();
    let space = |member: &str| json!({"kind": "wiki", "value": format!("wiki:space:{member}")});
    let mut singles = Vec::new();
    for member in ["cedar", "larch"] {
        let output = central_aikit(&central)
            .args(["--json", "-C"])
            .arg(central.root.join("Work").join(member))
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
                &space(member).to_string(),
            ])
            .output()
            .unwrap();
        let single = serde_json::from_slice::<Value>(&output.stdout).unwrap();
        assert_eq!(single["ok"], true, "{member}: {single}");
        singles.push(single);
        assert_eq!(
            world_here_reads(&central),
            std::collections::BTreeMap::from([(member.to_string(), 2)]),
            "a single read observes its Project's facet before and after its binding"
        );
    }
    let items: Vec<Value> = ["cedar", "larch"]
        .iter()
        .map(|member| {
            json!({"cwd": central.root.join("Work").join(member), "address": space(member),
                   "depth": 2, "max_nodes": 96, "max_edges": 192})
        })
        .collect();
    for schema in [
        "aikit.knowledge-relations-batch/v1",
        "aikit.knowledge-relations-batch/v2",
    ] {
        let mut child = central_aikit(&central)
            .args(["--json", "-C"])
            .arg(&central.root)
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
            .write_all(
                json!({"schema": schema, "items": items})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        let mut answers = vec![Value::Null; 2];
        for line in text.lines() {
            let line: Value = serde_json::from_str(line).unwrap();
            if let Some(index) = line["index"].as_u64() {
                answers[index as usize] = line["answer"].clone();
            } else if line["data"]["answers"].is_array() {
                answers = line["data"]["answers"].as_array().unwrap().clone();
            } else {
                assert_eq!(line["ok"], true, "{schema}: {line}");
            }
        }
        for (index, member) in ["cedar", "larch"].iter().enumerate() {
            assert_eq!(
                without_context_id(answers[index].clone()),
                without_context_id(singles[index].clone()),
                "{schema}: {member}"
            );
        }
        assert_eq!(
            world_here_reads(&central),
            std::collections::BTreeMap::from([("cedar".to_string(), 1), ("larch".to_string(), 1)]),
            "{schema}: one reading of each Project's facet serves its binding observation"
        );
    }
}
