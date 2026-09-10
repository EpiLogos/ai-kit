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

fn project_context(root: &Path, home: &Path, cwd: &Path) -> Output {
    // The public Context receipt lives on the native SessionSpace application
    // command. `method resolve` is not part of the current CLI grammar.
    Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
        .env("AIKIT_HOME", home)
        .env("CENTRAL_ROOT", root)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .current_dir(cwd)
        .arg("project-context")
        .output()
        .unwrap()
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
    succeeded(&bound);
    let optional = aikit(&root, &home, &project, &["compose"]);
    succeeded(&optional);
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
    succeeded(&composed);
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
    let first = project_context(&root, &home, &project);
    succeeded(&first);
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert!(first["context"]["reference"].is_string());
    let repeated = project_context(&root, &home, &project);
    succeeded(&repeated);
    let repeated: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(first, repeated, "unchanged native input has a stable receipt");

    // Change actual source bytes without changing the declared owner revision.
    // The next Context receipt must bind the new bytes, not merely the old label.
    let mut changed_bytes = fs::read(&source).unwrap();
    changed_bytes.push(b'\n');
    fs::write(&source, changed_bytes).unwrap();
    let changed = project_context(&root, &home, &project);
    succeeded(&changed);
    let changed: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_ne!(first["context"]["reference"], changed["context"]["reference"]);
    assert_eq!(
        first["context"]["basis"]["resolver_hash"],
        changed["context"]["basis"]["resolver_hash"]
    );
    assert_eq!(
        changed["context"]["basis"]["observed_source_resources"][0]["sources"][0]["revision"],
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
    assert!(
        !project_context(&root, &home, &project).status.success(),
        "the native Context receipt must also refuse broken source"
    );
}
