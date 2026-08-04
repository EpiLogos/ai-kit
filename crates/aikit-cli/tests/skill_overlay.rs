//! Skill Usage Overlay authoring through real CLI processes and files.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

const SKILL: &str = "skill/test/wayfinder";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn scene(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    write(&project.join(".aikit/profile.toml"), "schema = 1\n# keep me\n");
    write(
        &home.join("registries/personal/capsules/skill/test/wayfinder/manifest.toml"),
        r#"schema = 1
id = "skill/test/wayfinder"
kind = "skill"
name = "wayfinder"
description = "Plans long work."

[skill]
root = "payload"
"#,
    );
    write(
        &home.join("registries/personal/capsules/skill/test/wayfinder/payload/SKILL.md"),
        "---\nname: wayfinder\ndescription: Plans long work.\ndisable-model-invocation: true\n---\n\n# Wayfinder\n",
    );
    (home, project)
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    (output, value)
}

#[test]
fn guidance_can_be_set_inspected_and_cleared_at_user_scope() {
    let temp = tempfile::tempdir().unwrap();
    let (home, project) = scene(temp.path());

    let (set, reply) = run(
        &home,
        &project,
        &[
            "skill",
            "overlay",
            "set",
            SKILL,
            "--scope",
            "user",
            "--description",
            "Prefer for work spanning agent sessions.",
            "--guidance",
            "Treat this as authoritative orienting augmentation.",
        ],
    );
    assert!(set.status.success(), "setting overlay failed: {reply}");
    assert_eq!(reply["data"]["scope"], "global");

    let global = fs::read_to_string(home.join("scopes/global/profile.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&global).unwrap();
    let overlay = &parsed["skill-overlays"][SKILL];
    assert_eq!(
        overlay["description"].as_str(),
        Some("Prefer for work spanning agent sessions.")
    );
    assert_eq!(
        overlay["guidance"].as_str(),
        Some("Treat this as authoritative orienting augmentation.")
    );

    let (shown, reply) = run(
        &home,
        &project,
        &["skill", "overlay", "show", SKILL],
    );
    assert!(shown.status.success(), "show failed: {reply}");
    assert_eq!(reply["data"]["overlays"].as_array().unwrap().len(), 1);
    assert_eq!(reply["data"]["overlays"][0]["scope"], "global");

    let (cleared, reply) = run(
        &home,
        &project,
        &["skill", "overlay", "clear", SKILL, "--scope", "user"],
    );
    assert!(cleared.status.success(), "clear failed: {reply}");
    let global = fs::read_to_string(home.join("scopes/global/profile.toml")).unwrap();
    assert!(!global.contains("skill/test/wayfinder"));
}

#[test]
fn a_project_overlay_is_format_preserving_and_can_reset_user_orientation() {
    let temp = tempfile::tempdir().unwrap();
    let (home, project) = scene(temp.path());

    let (set, reply) = run(
        &home,
        &project,
        &[
            "skill",
            "overlay",
            "set",
            SKILL,
            "--scope",
            "project",
            "--no-inherit",
            "--guidance",
            "Use the shared project convention.",
        ],
    );
    assert!(set.status.success(), "project overlay failed: {reply}");
    let profile = fs::read_to_string(project.join(".aikit/profile.toml")).unwrap();
    assert!(profile.contains("# keep me"));
    assert!(profile.contains("inherit = false"));
    assert!(profile.contains("Use the shared project convention."));
}

#[test]
fn reviewed_against_refuses_anything_but_an_exact_content_digest() {
    let temp = tempfile::tempdir().unwrap();
    let (home, project) = scene(temp.path());
    let (output, reply) = run(
        &home,
        &project,
        &[
            "skill",
            "overlay",
            "set",
            SKILL,
            "--scope",
            "global",
            "--guidance",
            "Reviewed orientation.",
            "--reviewed-against",
            "main",
        ],
    );
    assert!(!output.status.success());
    assert_eq!(reply["error"]["code"], "skill_overlay.invalid_revision");
    assert!(!home.join("scopes/global/profile.toml").exists());
}

#[test]
fn a_trusted_source_projects_the_same_effective_skill_to_codex_claude_and_broker_reads() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    fs::create_dir_all(&project).unwrap();
    write(&project.join(".aikit/profile.toml"), "schema = 1\n");
    let upstream = "---\nname: wayfinder\ndescription: Plans long work.\ndisable-model-invocation: true\n---\n\n# Wayfinder\n\nUpstream body.\n";
    write(&source.join("wayfinder/SKILL.md"), upstream);
    write(&source.join("wayfinder/references/tracker.md"), "Tracker reference.\n");

    for args in [
        vec![
            "source",
            "add-directory",
            "personal",
            source.to_str().unwrap(),
        ],
        vec!["source", "sync", "personal"],
        vec!["source", "promote", "personal", "--trust"],
    ] {
        let (output, reply) = run(&home, &project, &args);
        assert!(output.status.success(), "source command failed: {reply}");
    }

    let managed = "skill/personal/wayfinder";
    let (enabled, reply) = run(
        &home,
        &project,
        &["enable", managed, "--scope", "global"],
    );
    assert!(enabled.status.success(), "enable failed: {reply}");
    let (set, reply) = run(
        &home,
        &project,
        &[
            "skill",
            "overlay",
            "set",
            managed,
            "--scope",
            "global",
            "--description",
            "Prefer for work spanning agent sessions.",
            "--guidance",
            "Maps may carry execution when their Notes explicitly say so.",
        ],
    );
    assert!(set.status.success(), "overlay failed: {reply}");
    let context = reply["context"]["context_id"].as_str().unwrap();
    let current = home.join("state/contexts").join(context).join("current");
    let claude = fs::read_to_string(
        current.join("projections/claude/.claude/skills/wayfinder/SKILL.md"),
    )
    .unwrap();
    let codex = fs::read_to_string(
        current.join("projections/codex/.agents/skills/wayfinder/SKILL.md"),
    )
    .unwrap();
    assert_eq!(claude, codex);
    assert!(claude.contains("authoritative orienting augmentation"));
    assert!(claude.contains("Maps may carry execution"));
    assert!(claude.contains("disable-model-invocation: true"));
    assert_eq!(
        fs::read_to_string(
            current.join(
                "projections/claude/.claude/skills/wayfinder/references/tracker.md"
            )
        )
        .unwrap(),
        "Tracker reference.\n"
    );
    assert_eq!(fs::read_to_string(source.join("wayfinder/SKILL.md")).unwrap(), upstream);

    let (read, reply) = run(&home, &project, &["capabilities", "read", managed]);
    assert!(read.status.success(), "broker read failed: {reply}");
    assert_eq!(reply["data"]["instructions"].as_str(), Some(claude.as_str()));
}
