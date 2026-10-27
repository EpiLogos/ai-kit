//! A skill in the spool must reach an agent, or the apply says why not.
//!
//! The misses this guards, each of which used to be silent: a spool skill
//! enabled nowhere, an enabled skill withheld for want of a manifest, a
//! generated skill never linked into a harness root, and an apply that ran in a
//! context the harness root does not resolve through.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const CONTEXT: &str = "ctx_SKILLREACH0000000000000";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn skill(ground: &Path, name: &str, manifest: bool) {
    write(
        &ground.join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: The {name} skill.\n---\nBody.\n"),
    );
    if manifest {
        write(
            &ground.join(name).join("skill.json"),
            &format!(
                r#"{{"schema":"central.skill/v1","name":"{name}","scope":"control-user","standing":"active","provenance":"adopted"}}"#
            ),
        );
    }
}

fn run(home: &Path, project: &Path, context: &str, args: &[&str]) -> Output {
    Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env("AIKIT_CONTEXT_ID", context)
        .current_dir(project)
        .output()
        .unwrap()
}

fn ok(home: &Path, project: &Path, args: &[&str]) -> Value {
    let output = run(home, project, CONTEXT, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn warnings(envelope: &Value) -> Vec<String> {
    envelope["warnings"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|w| w.as_str().map(str::to_owned))
                .filter(|w| w.starts_with("skill-reach:"))
                .collect()
        })
        .unwrap_or_default()
}

fn promote(home: &Path, project: &Path, ground: &Path) {
    ok(
        home,
        project,
        &[
            "source",
            "add-directory",
            "personal",
            ground.to_str().unwrap(),
            "--control-ground",
        ],
    );
    ok(home, project, &["source", "sync", "personal"]);
    ok(home, project, &["source", "promote", "personal", "--trust"]);
}

#[test]
fn every_hand_off_that_loses_a_skill_is_announced_and_wired_roots_stay_complete() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let ground = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    skill(ground.path(), "alpha", true);
    skill(ground.path(), "beta", true); // authored, never enabled
    skill(ground.path(), "gamma", false); // enabled, but no manifest
    promote(home.path(), project.path(), ground.path());
    for id in ["skill/personal/alpha", "skill/personal/gamma"] {
        ok(
            home.path(),
            project.path(),
            &["enable", id, "--scope", "user"],
        );
    }

    // The apply names the two silent misses before any harness is wired.
    let first = ok(home.path(), project.path(), &["apply"]);
    let notes = warnings(&first).join("\n");
    assert!(
        notes.contains("enabled nowhere") && notes.contains("skill/personal/beta"),
        "unassigned spool skill must be announced: {notes}"
    );
    assert!(
        notes.contains("withheld") && notes.contains("skill/personal/gamma"),
        "withheld enabled skill must be announced: {notes}"
    );

    // First wiring of a harness root is the explicit, reviewed adoption.
    let tree = home
        .path()
        .join("state/contexts")
        .join(CONTEXT)
        .join("current/projections/codex/.agents/skills");
    let root = home.path().join(".agents/skills");
    fs::create_dir_all(&root).unwrap();
    let wire = [
        "adopt",
        root.to_str().unwrap(),
        "--projection",
        tree.to_str().unwrap(),
    ];
    let preview = ok(home.path(), project.path(), &wire);
    let mut confirm = wire.to_vec();
    confirm.extend([
        "--yes",
        "--expect-digest",
        preview["data"]["review_digest"].as_str().unwrap(),
    ]);
    ok(home.path(), project.path(), &confirm);
    assert!(root.join("alpha").is_symlink());

    // A skill enabled afterwards reaches the root with no further step.
    skill(ground.path(), "delta", true);
    ok(home.path(), project.path(), &["source", "sync", "personal"]);
    ok(
        home.path(),
        project.path(),
        &["source", "promote", "personal", "--trust"],
    );
    ok(
        home.path(),
        project.path(),
        &["enable", "skill/personal/delta", "--scope", "user"],
    );
    assert!(
        root.join("delta").is_symlink() && root.join("delta/SKILL.md").is_file(),
        "enabling a skill must link it into the wired root"
    );

    // The realistic miss: a skill is enabled from some other directory's
    // context, so the wired context is not re-applied. The toggle says so, and
    // the next apply in the wired context links it.
    skill(ground.path(), "epsilon", true);
    ok(home.path(), project.path(), &["source", "sync", "personal"]);
    ok(
        home.path(),
        project.path(),
        &["source", "promote", "personal", "--trust"],
    );
    let elsewhere = run(
        home.path(),
        project.path(),
        "ctx_ELSEWHERE00000000000",
        &["enable", "skill/personal/epsilon", "--scope", "user"],
    );
    assert!(elsewhere.status.success());
    let elsewhere: Value = serde_json::from_slice(&elsewhere.stdout).unwrap();
    assert!(
        warnings(&elsewhere)
            .join("\n")
            .contains(&format!("resolves through {CONTEXT}")),
        "an enable that did not reach the wired context must say so"
    );
    assert!(!root.join("epsilon").exists());
    let second = ok(home.path(), project.path(), &["apply"]);
    assert!(root.join("epsilon").is_symlink() && root.join("epsilon/SKILL.md").is_file());
    let wired = &second["data"]["skill_roots"][0];
    assert_eq!(wired["state"], "linked");
    assert_eq!(wired["linked"], 1);
    assert!(wired["undo"]
        .as_str()
        .unwrap()
        .starts_with("aikit procedure undo"));

    // A second apply has nothing left to do.
    let third = ok(home.path(), project.path(), &["apply"]);
    assert_eq!(third["data"]["skill_roots"][0]["state"], "satisfied");

    // An apply in some other context says the wired root was not refreshed.
    let other = run(
        home.path(),
        project.path(),
        "ctx_ELSEWHERE00000000000",
        &["apply"],
    );
    assert!(other.status.success());
    let other: Value = serde_json::from_slice(&other.stdout).unwrap();
    let notes = warnings(&other).join("\n");
    assert!(
        notes.contains(&format!("resolves through {CONTEXT}"))
            && notes.contains("AIKIT_CONTEXT_ID="),
        "an apply that did not refresh the wired context must say so: {notes}"
    );
}
