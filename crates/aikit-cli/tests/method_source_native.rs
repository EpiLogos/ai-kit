//! Real source catalog, Project binding and native Method/Praxis resolver.
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn call(home: &Path, work: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_HOME", home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .current_dir(work)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}
fn read(home: &Path, work: &Path, args: &[&str]) -> Value {
    let out = call(home, work, args);
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["data"].clone()
}
#[test]
fn source_method_resolves_actual_skills_and_refuses_missing_members_without_activation() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit");
    let work = temp.path().join("work");
    let source = temp.path().join("skills");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(source.join("check")).unwrap();
    fs::write(source.join("check/SKILL.md"),"---\nname: check\ndescription: Verify a changed artifact using its native owner checks and retain actual exit evidence.\n---\nRead the affected owner instructions, run the required check, and retain the actual exit result and artifact references.\n").unwrap();
    read(
        &home,
        &work,
        &[
            "source",
            "add-directory",
            "method-native",
            source.to_str().unwrap(),
        ],
    );
    read(&home, &work, &["source", "sync", "method-native"]);
    read(
        &home,
        &work,
        &["source", "promote", "method-native", "--trust"],
    );
    read(
        &home,
        &work,
        &[
            "project",
            "bind",
            "method-project",
            "--directory",
            work.to_str().unwrap(),
            "--no-default-skill-sets",
        ],
    );
    let path = work.join("method.json");
    let mut declaration = json!({"id":"method/native/check","source":"central:source:project:method-project:method.json","name":"Check actual artifact","skills":[{"skill":"skill/method-native/check"}],"verification":["skill/method-native/check"],"expected_return_forms":["exit-evidence"]});
    fs::write(&path, serde_json::to_vec(&declaration).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let resolved = read(
        &home,
        &work,
        &["method", "resolve", "--source", path.to_str().unwrap()],
    );
    assert_eq!(resolved["method"]["id"], declaration["id"]);
    assert_eq!(resolved["praxis"]["methods"].as_array().unwrap().len(), 1);
    assert!(resolved["praxis"]["warnings"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        resolved["skill_states"][0]["active"], false,
        "resolution does not activate a Skill"
    );
    assert_eq!(
        resolved["source_read"]["revision"],
        format!("blake3:{}", blake3::hash(&before))
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "situated resolution preserves source"
    );
    assert!(resolved["context_resolution"].is_object());
    let focus = resolved["praxis"]["focus"][0].as_str().unwrap();
    let focused = read(
        &home,
        &work,
        &[
            "method",
            "resolve",
            "--source",
            path.to_str().unwrap(),
            "--focus",
            focus,
        ],
    );
    assert_eq!(focused["method"]["focus"], json!([]));
    assert!(!call(
        &home,
        &work,
        &[
            "method",
            "resolve",
            "--source",
            path.to_str().unwrap(),
            "--focus",
            "project/absent"
        ]
    )
    .status
    .success());
    declaration["description"] = json!("Changed source requires a distinct exact revision.");
    fs::write(&path, serde_json::to_vec(&declaration).unwrap()).unwrap();
    let changed = read(
        &home,
        &work,
        &["method", "resolve", "--source", path.to_str().unwrap()],
    );
    assert_ne!(
        resolved["source_read"]["revision"],
        changed["source_read"]["revision"]
    );
    declaration["skills"][0]["skill"] = json!("skill/method-native/absent");
    fs::write(&path, serde_json::to_vec(&declaration).unwrap()).unwrap();
    let denied = call(
        &home,
        &work,
        &["method", "resolve", "--source", path.to_str().unwrap()],
    );
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stdout).contains("method.unresolved"));
}
