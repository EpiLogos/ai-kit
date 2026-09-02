//! `aikit set` end to end.
//!
//! The acceptance criterion: **a skill-set withholds an unreviewed member and says
//! so.** `aikit set show` is the command that earns its place in the surface — it
//! lists members *and* the members that would not project here, with the reason.
//! A set is a request; this is the reply.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A skill capsule with a real Agent Skill payload.
fn skill(home: &Path, id: &str) {
    let leaf = id.rsplit('/').next().unwrap();
    let base = home.join(format!("registries/personal/capsules/{id}"));
    write(
        &base.join("manifest.toml"),
        &format!(
            r#"schema = 1
id = "{id}"
kind = "skill"
name = "{leaf}"
description = "The {leaf} skill for the set test."

[skill]
root = "payload"
"#
        ),
    );
    write(
        &base.join("payload/SKILL.md"),
        &format!("---\nname: {leaf}\ndescription: The {leaf} skill.\n---\n\n# {leaf}\n"),
    );
}

/// Review a capsule, so it can activate. A skill needs review before it projects;
/// that gate is the point, so the tests have to pass through it honestly rather
/// than around it.
fn review(home_path: &Path, project: &Path, id: &str) {
    use aikit_core::catalog::Catalog;
    use aikit_core::trust::{TrustKey, TrustState};
    use aikit_store::home::AikitHome;
    use aikit_store::index::Index;
    use aikit_store::trust::TrustStore;

    let home = AikitHome::at(home_path);
    home.ensure_layout().unwrap();
    let load = aikit_cli::app::load_catalog(&home, Some(project)).unwrap();
    let cid = aikit_core::CapsuleId::parse(id).unwrap();
    let capsule = Catalog::get(&load.catalog, &cid).expect("capsule is catalogued");
    let key = TrustKey::new(
        capsule.source.clone().unwrap(),
        capsule.id.clone(),
        capsule.revision.clone().unwrap(),
    );
    let index = Index::open(&home.database()).unwrap();
    TrustStore::new(&index)
        .record(&key, TrustState::Reviewed, None)
        .unwrap();
}

fn aikit(home: &Path, project: &Path, args: &[&str]) -> (bool, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?}: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit JSON; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

#[test]
fn a_set_withholds_an_unreviewed_member_and_says_why() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    skill(home.path(), "skill/rust/review");
    skill(home.path(), "skill/rust/unsafe-audit");

    // BOTH are enabled, but only `review` has been reviewed. The withholding is
    // therefore genuinely about trust — which is the property that matters, since
    // a set must not be able to launder it.
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"skill/rust/review\", \"skill/rust/unsafe-audit\"]\n",
    );
    review(home.path(), project.path(), "skill/rust/review");

    let (ok, _) = aikit(
        home.path(),
        project.path(),
        &[
            "set",
            "create",
            "rust-review",
            "skill/rust/review",
            "skill/rust/unsafe-audit",
        ],
    );
    assert!(ok, "set create should succeed");

    let (ok, envelope) = aikit(home.path(), project.path(), &["set", "show", "rust-review"]);
    assert!(ok);
    let data = &envelope["data"];

    assert_eq!(data["members"], Value::from(2));
    assert_eq!(
        data["complete"],
        Value::Bool(false),
        "the set did not get everything it asked for: {data}"
    );

    let withheld = data["withheld"].as_array().expect("withheld array");
    assert_eq!(withheld.len(), 1, "one member was withheld: {data}");
    assert_eq!(withheld[0]["capability"], "skill/rust/unsafe-audit");
    assert!(
        withheld[0]["reason"]
            .as_str()
            .is_some_and(|r| r.contains("review")),
        "and the set SAYS WHY, in the resolver's own words: {data}"
    );
    assert_eq!(
        data["projected"][0], "skill/rust/review",
        "the reviewed member still projects: withholding is per-member"
    );

    // The one-line summary a status bar, a screen reader and an agent all get.
    let summary = data["summary"].as_str().unwrap();
    assert!(summary.contains("2 members"), "{summary}");
    assert!(summary.contains("1 projected"), "{summary}");
    assert!(summary.contains("1 withheld"), "{summary}");
}

#[test]
fn a_glob_expands_at_authoring_time_and_never_matches_dynamically() {
    // SPEC-III §1.5: if sets matched dynamically, syncing a registry would silently
    // change what a harness sees. So the glob expands NOW, and a capsule catalogued
    // later is proposed rather than joined.
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    skill(home.path(), "skill/rust/review");
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"skill/rust/review\"]\n",
    );
    review(home.path(), project.path(), "skill/rust/review");

    let (ok, envelope) = aikit(
        home.path(),
        project.path(),
        &["set", "create", "rusty", "--match", "skill/rust/*"],
    );
    assert!(ok, "{envelope}");
    assert_eq!(envelope["data"]["members"], Value::from(1));

    // A capsule catalogued AFTER authoring matches the retained pattern…
    skill(home.path(), "skill/rust/unsafe-audit");

    let (_, envelope) = aikit(home.path(), project.path(), &["set", "show", "rusty"]);
    assert_eq!(
        envelope["data"]["members"],
        Value::from(1),
        "…but membership is unchanged: a new match is proposed, never joined"
    );
    assert_eq!(
        envelope["data"]["patterns"][0], "skill/rust/*",
        "the pattern is retained as provenance so the proposal can be made"
    );
}

#[test]
fn removing_from_a_set_never_deletes_the_capability() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    skill(home.path(), "skill/rust/review");
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"skill/rust/review\"]\n",
    );

    aikit(
        home.path(),
        project.path(),
        &["set", "create", "s", "skill/rust/review"],
    );
    let (ok, envelope) = aikit(
        home.path(),
        project.path(),
        &["set", "remove", "s", "skill/rust/review"],
    );
    assert!(ok);
    assert_eq!(envelope["data"]["members"], Value::from(0));

    // The capsule is untouched on disk — a set is a view, not an owner.
    assert!(
        home.path()
            .join("registries/personal/capsules/skill/rust/review/manifest.toml")
            .is_file(),
        "removing from a set must never delete the capability"
    );
}

#[test]
fn a_set_made_with_mkdir_and_a_text_file_is_a_real_set() {
    // The concept's whole claim: `mkdir` is a legitimate way to create a set. If
    // this test needed the CLI, the concept would have failed.
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    skill(home.path(), "skill/rust/review");
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"skill/rust/review\"]\n",
    );

    review(home.path(), project.path(), "skill/rust/review");

    let hand_made = home.path().join("skillsets/by-hand");
    fs::create_dir_all(&hand_made).unwrap();
    fs::write(
        hand_made.join("members"),
        "# a comment, and a blank line follow\n\nskill/rust/review\n",
    )
    .unwrap();

    let (ok, envelope) = aikit(home.path(), project.path(), &["set", "show", "by-hand"]);
    assert!(ok, "{envelope}");
    assert_eq!(envelope["data"]["members"], Value::from(1));
    assert_eq!(envelope["data"]["projected"][0], "skill/rust/review");
}
