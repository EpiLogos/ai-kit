//! `aikit init --json` end to end through the real binary: it discovers the
//! foreign skill roots under `$HOME`, reports counts, and — the design point of
//! SPEC-III §4.4 — asks nothing and mutates nothing before producing output.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn skill(dir: &Path, name: &str) {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A real skill named {name}.\n---\n\n# {name}\n"),
    )
    .unwrap();
}

#[test]
fn init_discovers_foreign_roots_and_speaks_the_json_envelope() {
    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();

    let claude = home.path().join(".claude/skills");
    fs::create_dir_all(&claude).unwrap();
    skill(&claude, "pdf");
    skill(&claude, "docx");

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .arg("init")
        .arg("--json")
        .env("HOME", home.path())
        .env("AIKIT_HOME", aikit_home.path())
        .current_dir(home.path())
        .output()
        .expect("aikit init runs");

    assert!(
        output.status.success(),
        "init must succeed; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let envelope: Value = serde_json::from_slice(&output.stdout).expect("init emits JSON");
    assert_eq!(envelope["ok"], Value::Bool(true), "envelope: {envelope}");
    assert_eq!(envelope["schema"], Value::from(1));

    let data = &envelope["data"];
    assert_eq!(data["total_skills"], Value::from(2), "found pdf and docx");

    let roots = data["roots"].as_array().expect("roots array");
    let claude_root = roots
        .iter()
        .find(|r| r["label"] == "@claude")
        .expect("the @claude root is reported");
    assert_eq!(claude_root["skills"], Value::from(2));

    // Discovery is read-only: it created nothing under the foreign root.
    assert!(
        !home.path().join(".claude/skills/.aikit").exists(),
        "init must not write into the foreign root"
    );
}
