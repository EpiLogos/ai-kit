//! Explicit native owner integration, with all source and AIKit writes temporary.
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn ctrl(root: &Path, args: &[&str]) -> Value {
    let output = Command::new("ctrl")
        .args(["--json", "--root"])
        .arg(root)
        .args(args)
        .output()
        .expect("native ctrl required");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success() && value["ok"] == true, "{value}");
    value
}
fn aikit(root: &Path, home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_HOME", home)
        .env("CENTRAL_ROOT", root)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .current_dir(cwd)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}
#[test]
#[ignore = "requires installed native Central ctrl and Actuation discovery"]
fn explicit_compose_discloses_authored_basis_and_refuses_broken_source() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Central");
    let home = temp.path().join("aikit");
    ctrl(&root, &["init"]);
    let root = fs::canonicalize(root).unwrap();
    let project = root.join("Work/compose-proof");
    fs::create_dir_all(project.join("ProjectCentral/user")).unwrap();
    fs::write(project.join("ProjectCentral/project.json"),json!({
        "schema":"central.project/v1","project_id":"project:compose-proof","human_source":"ProjectCentral/user",
        "wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}
    }).to_string()).unwrap();
    let bound = aikit(
        &root,
        &home,
        &project,
        &[
            "project",
            "bind",
            "compose-proof",
            "--directory",
            project.to_str().unwrap(),
            "--no-default-skill-sets",
        ],
    );
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    let optional = aikit(&root, &home, &project, &["compose"]);
    assert!(
        optional.status.success(),
        "{}",
        String::from_utf8_lossy(&optional.stdout)
    );
    let profile = json!({"schema":"central.agent-profile/v1","ref":"profile/compose-proof","revision":"r1",
        "agent_ref":"agent/compose-proof","scope":"project","world_ref":"world/compose-proof",
        "ratified_world_refs":["world/compose-proof"],"governance_refs":["source/compose-proof/law"],
        "knowledge_source_refs":["source/compose-proof/intent"]});
    let saved = ctrl(
        &root,
        &[
            "action",
            "run",
            "agent-profile.save",
            &json!({"scope":"project","project":"compose-proof","profile":profile}).to_string(),
        ],
    );
    let composed = aikit(&root, &home, &project, &["compose"]);
    assert!(
        composed.status.success(),
        "{}",
        String::from_utf8_lossy(&composed.stdout)
    );
    let value: Value = serde_json::from_slice(&composed.stdout).unwrap();
    assert_eq!(
        value["data"]["composed_inputs"]["authored_basis"]["profile_source"]["revision"],
        "r1"
    );
    assert_eq!(
        value["data"]["composed_inputs"]["authored_basis"]["profile_source"]["governance_refs"],
        profile["governance_refs"]
    );
    assert_eq!(
        value["data"]["composed_inputs"]["authored_basis_standing"],
        "requested-source-not-effective-selection"
    );
    assert_eq!(value["data"]["plan"]["agent"]["state"], "resolved");
    let records = value["data"]["composed_inputs"]["source_resources"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["eligibility"]["state"], "undetermined");
    assert!(records[0]["providers"].as_array().unwrap().is_empty());
    assert_eq!(records[0]["descriptor"]["sources"][0]["revision"], "r1");
    let source = Path::new(saved["data"]["source_path"].as_str().unwrap());
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        project.join(source)
    };
    let method_path = project.join("method.json");
    fs::write(&method_path,json!({"id":"method/compose-proof","source":"central:source:project:compose-proof:method.json","name":"Observe commissioned Agent source","focus":["agent/compose-proof"]}).to_string()).unwrap();
    let method = aikit(
        &root,
        &home,
        &project,
        &[
            "method",
            "resolve",
            "--source",
            method_path.to_str().unwrap(),
            "--focus",
            "agent/compose-proof",
        ],
    );
    assert!(
        method.status.success(),
        "{}",
        String::from_utf8_lossy(&method.stdout)
    );
    let method: Value = serde_json::from_slice(&method.stdout).unwrap();
    assert_eq!(method["data"]["praxis"]["focus"][0], "agent/compose-proof");
    // Change actual source bytes without changing the declared owner revision.
    // The next Context receipt must bind the new bytes, not merely the old label.
    let old_bytes = fs::read(&source).unwrap();
    let mut changed_bytes = old_bytes.clone();
    changed_bytes.push(b'\n');
    fs::write(&source, changed_bytes).unwrap();
    let changed = aikit(
        &root,
        &home,
        &project,
        &[
            "method",
            "resolve",
            "--source",
            method_path.to_str().unwrap(),
            "--focus",
            "agent/compose-proof",
        ],
    );
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stdout)
    );
    let changed: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_ne!(
        method["data"]["context_resolution"]["reference"],
        changed["data"]["context_resolution"]["reference"]
    );
    assert_eq!(
        method["data"]["context_resolution"]["basis"]["resolver_hash"],
        changed["data"]["context_resolution"]["basis"]["resolver_hash"]
    );
    assert_eq!(
        changed["data"]["context_resolution"]["basis"]["observed_source_resources"][0]["sources"]
            [0]["revision"],
        "r1"
    );
    fs::write(source, b"malformed owner profile").unwrap();
    let refused = aikit(&root, &home, &project, &["compose"]);
    assert!(
        !refused.status.success(),
        "broken authored source must not become successful empty context"
    );
    let failure: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(failure["ok"], false);
}
