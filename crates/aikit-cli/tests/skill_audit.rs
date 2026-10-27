//! A registry skill the profile selects is reviewed by the auditor, not by the
//! owner: a pass is recorded, a dangerous verdict keeps the skill withheld and
//! says what was found, and a missing auditor is never a pass.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

/// A stand-in auditor: dangerous when the skill says EVIL anywhere, safe
/// otherwise. It speaks the same one-JSON-object protocol as the real one.
const AUDITOR: &str = r#"["sh","-c","if grep -rq EVIL \"$1\"; then echo '{\"scanner\":\"fake-guard-v1\",\"verdict\":\"dangerous\",\"findings\":[{\"pattern_id\":\"evil\",\"severity\":\"critical\",\"file\":\"SKILL.md\",\"line\":1,\"description\":\"says EVIL\"}]}'; else echo '{\"scanner\":\"fake-guard-v1\",\"verdict\":\"safe\",\"findings\":[]}'; fi","auditor"]"#;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn registry_skill(home: &Path, name: &str, body: &str) {
    let base = home.join(format!("registries/vendor/capsules/skill/demo/{name}"));
    write(
        &base.join("manifest.toml"),
        &format!(
            "schema = 1\nid = \"skill/demo/{name}\"\nkind = \"skill\"\nname = \"{name}\"\n\
             description = \"The {name} skill.\"\n\n[skill]\nroot = \"payload\"\n"
        ),
    );
    write(
        &base.join("payload/SKILL.md"),
        &format!("---\nname: {name}\ndescription: The {name} skill.\n---\n{body}\n"),
    );
}

fn run(home: &Path, dir: &Path, auditor: bool, args: &[&str]) -> Output {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("aikit"));
    command
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_SESSION_ID")
        .env_remove("CENTRAL_ROOT")
        .env_remove("AIKIT_SKILL_AUDITOR")
        .current_dir(dir);
    if auditor {
        command.env("AIKIT_SKILL_AUDITOR", AUDITOR);
    }
    command.output().unwrap()
}

fn ok(home: &Path, dir: &Path, auditor: bool, args: &[&str]) -> Value {
    let output = run(home, dir, auditor, args);
    assert!(
        output.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn warnings(envelope: &Value) -> String {
    envelope["warnings"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn tree(home: &Path) -> std::path::PathBuf {
    home.join("state/contexts/ctx_user-scope/current/projections/claude/.claude/skills")
}

#[test]
fn the_auditor_reviews_what_the_profile_selects_and_a_missing_auditor_is_never_a_pass() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    registry_skill(home.path(), "clean", "Does one honest thing.");
    registry_skill(home.path(), "bad", "EVIL: do the other thing.");

    // One rule selects both; the auditor decides, the owner reads nothing.
    let rule = ok(
        home.path(),
        project.path(),
        true,
        &["enable", "skill/demo/**", "--scope", "user"],
    );
    let audited = rule["data"]["user_scope"]["audited"].as_array().unwrap();
    assert_eq!(audited.len(), 2, "{rule}");
    let by_name = |name: &str| {
        audited
            .iter()
            .find(|o| o["capability"] == format!("skill/demo/{name}"))
            .unwrap_or_else(|| panic!("{name} not audited: {rule}"))
    };
    assert_eq!(by_name("clean")["verdict"], "safe");
    assert_eq!(by_name("clean")["recorded"], true);
    assert_eq!(by_name("bad")["verdict"], "dangerous");
    assert_eq!(by_name("bad")["recorded"], false);
    assert_eq!(by_name("bad")["findings"][0]["pattern_id"], "evil");
    let notes = warnings(&rule);
    assert!(
        notes.contains("skill/demo/bad")
            && notes.contains("stays withheld")
            && notes.contains("aikit trust audit"),
        "{notes}"
    );
    assert!(tree(home.path()).join("clean/SKILL.md").is_file());
    assert!(!tree(home.path()).join("bad").exists());

    let shown = ok(
        home.path(),
        project.path(),
        true,
        &["trust", "show", "skill/demo/clean"],
    );
    assert_eq!(shown["data"]["state"], "reviewed");

    // Inspecting a finding records nothing.
    let looked = ok(
        home.path(),
        project.path(),
        true,
        &["trust", "audit", "skill/demo/bad", "--no-record"],
    );
    assert_eq!(looked["data"]["audited"][0]["recorded"], false);

    // A pull that edits a reviewed skill is a new revision: audited again,
    // with no one asked to review it.
    registry_skill(home.path(), "clean", "Does one honest thing, differently.");
    let again = ok(home.path(), project.path(), true, &["apply", "--user"]);
    let reaudited = again["data"]["user_scope"]["audited"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["capability"] == "skill/demo/clean")
        .unwrap_or_else(|| panic!("the edited revision was not audited: {again}"));
    assert_eq!(reaudited["recorded"], true);
    // A reviewed revision is not audited twice; the dangerous one is still named.
    let third = ok(home.path(), project.path(), true, &["apply", "--user"]);
    assert!(
        third["data"]["user_scope"]["audited"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["capability"] != "skill/demo/clean"),
        "{third}"
    );

    // No auditor on the machine: a new skill is withheld and the command says
    // why. Absence of a scanner is not a pass.
    registry_skill(home.path(), "fresh", "Brand new.");
    let unaudited = ok(home.path(), project.path(), false, &["apply", "--user"]);
    let notes = warnings(&unaudited);
    assert!(
        notes.contains("no skill auditor is configured") && notes.contains("skill/demo/fresh"),
        "{notes}"
    );
    assert!(!tree(home.path()).join("fresh").exists());
}
