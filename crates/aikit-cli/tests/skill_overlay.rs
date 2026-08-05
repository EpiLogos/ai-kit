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

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
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
    assert!(reply["data"]["effects"].as_array().is_some_and(|effects| {
        effects.iter().any(|effect| effect["target"] == "codex")
            && effects.iter().any(|effect| effect["target"] == "claude-code")
    }));

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
fn a_review_pin_cannot_create_an_overlay_without_orientation() {
    let temp = tempfile::tempdir().unwrap();
    let (home, project) = scene(temp.path());
    let reviewed = "a".repeat(64);
    let (output, reply) = run(
        &home,
        &project,
        &[
            "skill",
            "overlay",
            "set",
            SKILL,
            "--scope",
            "user",
            "--reviewed-against",
            &reviewed,
        ],
    );
    assert!(!output.status.success());
    assert_eq!(reply["error"]["code"], "skill_overlay.empty");
    assert!(!home.join("scopes/global/profile.toml").exists());
}

#[test]
fn show_keeps_a_stale_review_pin_visible_until_the_user_reviews_the_update() {
    let temp = tempfile::tempdir().unwrap();
    let (home, project) = scene(temp.path());
    let stale = "0".repeat(64);
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
            "--guidance",
            "Orientation reviewed against an older payload.",
            "--reviewed-against",
            &stale,
        ],
    );
    assert!(set.status.success(), "set failed: {reply}");

    let (shown, reply) = run(
        &home,
        &project,
        &["skill", "overlay", "show", SKILL],
    );
    assert!(shown.status.success(), "show failed: {reply}");
    assert!(reply["warnings"].as_array().is_some_and(|warnings| {
        warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("review the augmentation against the updated source"))
        })
    }));

    let (status, reply) = run(&home, &project, &["status"]);
    assert!(status.status.success(), "status failed: {reply}");
    assert!(reply["warnings"].as_array().is_some_and(|warnings| {
        warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("review the augmentation against the updated source"))
        })
    }));
}

#[test]
fn a_trusted_source_projects_the_same_effective_skill_to_codex_claude_and_broker_reads() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let sibling = temp.path().join("sibling");
    let source = temp.path().join("matt-skills");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    write(&project.join(".aikit/profile.toml"), "schema = 1\n");
    write(&sibling.join(".aikit/profile.toml"), "schema = 1\n");
    let upstream = "---\nname: wayfinder\ndescription: Plans long work.\ndisable-model-invocation: true\n---\n\n# Wayfinder\n\nUpstream body.\n";
    let skill_root = source.join("skills/engineering/wayfinder");
    write(&skill_root.join("SKILL.md"), upstream);
    write(&skill_root.join("references/tracker.md"), "Tracker reference.\n");
    write(&skill_root.join("scripts/check.sh"), "#!/bin/sh\nexit 0\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(skill_root.join("scripts/check.sh"))
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(skill_root.join("scripts/check.sh"), permissions).unwrap();
    }
    git(&source, &["init", "--quiet"]);
    git(&source, &["config", "user.name", "AIKit test"]);
    git(
        &source,
        &["config", "user.email", "aikit@example.invalid"],
    );
    git(&source, &["add", "."]);
    git(&source, &["commit", "--quiet", "-m", "wayfinder v1"]);
    let first_commit = git(&source, &["rev-parse", "HEAD"]);

    for args in [
        vec![
            "source",
            "add-git",
            "mattpocock",
            source.to_str().unwrap(),
            "--revision",
            &first_commit,
            "--root",
            "skills",
        ],
        vec!["source", "sync", "mattpocock"],
    ] {
        let (output, reply) = run(&home, &project, &args);
        assert!(output.status.success(), "source command failed: {reply}");
    }

    let managed = "skill/mattpocock/engineering/wayfinder";
    let (promoted, reply) = run(
        &home,
        &project,
        &[
            "source",
            "promote",
            "mattpocock",
            "--trust-skill",
            managed,
        ],
    );
    assert!(promoted.status.success(), "promotion failed: {reply}");
    let (enabled, reply) = run(
        &home,
        &project,
        &["enable", managed, "--scope", "global"],
    );
    assert!(enabled.status.success(), "enable failed: {reply}");
    let (_, explained) = run(&home, &project, &["explain", managed]);
    let first_revision = explained["data"]["revision"].as_str().unwrap();
    assert_eq!(first_revision.len(), 64);

    let (set_global, global_reply) = run(
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
            "Use the user's issue tracker as the shared map.",
            "--reviewed-against",
            first_revision,
        ],
    );
    assert!(set_global.status.success(), "global overlay failed: {global_reply}");
    let global_generation = global_reply["data"]["generation"].as_str().unwrap();
    assert!(global_reply["data"]["effects"]
        .as_array()
        .is_some_and(|effects| !effects.is_empty()));

    let (set_project, project_reply) = run(
        &home,
        &project,
        &[
            "skill",
            "overlay",
            "set",
            managed,
            "--scope",
            "project",
            "--no-inherit",
            "--guidance",
            "This project's maps may carry execution when Notes explicitly say so.",
        ],
    );
    assert!(set_project.status.success(), "project overlay failed: {project_reply}");
    assert_ne!(
        project_reply["data"]["generation"].as_str().unwrap(),
        global_generation,
        "a changed Effective Skill must change the generation identity"
    );
    let context = project_reply["context"]["context_id"].as_str().unwrap();
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
    assert!(claude.contains("This project's maps may carry execution"));
    assert!(!claude.contains("Use the user's issue tracker"));
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let projected = current
            .join("projections/claude/.claude/skills/wayfinder/scripts/check.sh");
        assert_ne!(fs::metadata(projected).unwrap().permissions().mode() & 0o111, 0);
    }
    assert_eq!(fs::read_to_string(skill_root.join("SKILL.md")).unwrap(), upstream);

    let (_, explanation) = run(&home, &project, &["explain", managed]);
    let overlays = explanation["data"]["skill_usage_overlays"]
        .as_array()
        .unwrap();
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0]["scope"], "project");
    assert!(overlays[0]["origin"]
        .to_string()
        .contains(".aikit/profile.toml"));

    let (read, reply) = run(&home, &project, &["capabilities", "read", managed]);
    assert!(read.status.success(), "broker read failed: {reply}");
    assert_eq!(reply["data"]["instructions"].as_str(), Some(claude.as_str()));

    let (applied, sibling_reply) = run(&home, &sibling, &["apply"]);
    assert!(applied.status.success(), "sibling apply failed: {sibling_reply}");
    let sibling_context = sibling_reply["context"]["context_id"].as_str().unwrap();
    let sibling_current = home
        .join("state/contexts")
        .join(sibling_context)
        .join("current");
    let sibling_skill = fs::read_to_string(
        sibling_current.join("projections/codex/.agents/skills/wayfinder/SKILL.md"),
    )
    .unwrap();
    assert!(sibling_skill.contains("Use the user's issue tracker"));
    assert!(!sibling_skill.contains("This project's maps may carry execution"));

    write(
        &skill_root.join("SKILL.md"),
        &upstream.replace("Upstream body.", "Updated upstream body."),
    );
    git(&source, &["add", "."]);
    git(&source, &["commit", "--quiet", "-m", "wayfinder v2"]);
    let second_commit = git(&source, &["rev-parse", "HEAD"]);
    for args in [
        vec!["source", "set-revision", "mattpocock", &second_commit],
        vec!["source", "sync", "mattpocock"],
        vec![
            "source",
            "promote",
            "mattpocock",
            "--trust-skill",
            managed,
        ],
    ] {
        let (output, reply) = run(&home, &sibling, &args);
        assert!(output.status.success(), "source update failed: {reply}");
    }
    let (shown, reply) = run(&home, &sibling, &["skill", "overlay", "show", managed]);
    assert!(shown.status.success(), "updated overlay show failed: {reply}");
    assert!(reply["warnings"].as_array().is_some_and(|warnings| {
        warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("review the augmentation against the updated source"))
        })
    }));
}
