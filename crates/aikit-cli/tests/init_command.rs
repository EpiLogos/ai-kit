//! `aikit init --json` end to end through the real binary: it discovers the
//! foreign skill roots under `$HOME`, reports counts, and — the design point of
//! SPEC-III §4.4 — asks nothing and mutates nothing before producing output.

use std::fs;
use std::path::Path;

use serde_json::Value;
use sha2::Digest;
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

fn skills_cli_folder_hash(dir: &Path) -> String {
    let mut files = walkdir::WalkDir::new(dir)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry
                    .path()
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = sha2::Sha256::new();
    for (path, contents) in files {
        digest.update(path.as_bytes());
        digest.update(contents);
    }
    format!("{:x}", digest.finalize())
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

#[test]
fn init_reads_npx_skills_locks_as_foreign_provenance_without_mutating_them() {
    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();
    let project = home.path().join("work/payments");
    fs::create_dir_all(project.join(".agents/skills/project-review")).unwrap();
    skill(&project.join(".agents/skills"), "project-review");
    fs::create_dir_all(home.path().join(".agents/skills/global-review")).unwrap();
    skill(&home.path().join(".agents/skills"), "global-review");

    let global_lock = home.path().join(".agents/.skill-lock.json");
    let global_bytes = br#"{
      "version": 3,
      "skills": {
        "global-review": {
          "source": "acme/skills",
          "sourceType": "github",
          "sourceUrl": "https://github.com/acme/skills",
          "ref": "v2",
          "skillPath": "skills/global-review",
          "skillFolderHash": "sha256-global",
          "installedAt": "2026-07-01T00:00:00Z"
        }
      },
      "futureField": {"must": "survive"}
    }"#;
    fs::write(&global_lock, global_bytes).unwrap();
    let project_lock = project.join("skills-lock.json");
    let project_bytes = br#"{
      "version": 1,
      "skills": {
        "project-review": {
          "source": "acme/project-skills",
          "sourceType": "github",
          "ref": "main",
          "skillPath": "review",
          "computedHash": "sha256-project"
        }
      },
      "unknown": true
    }"#;
    fs::write(&project_lock, project_bytes).unwrap();

    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["init", "--json"])
        .env("HOME", home.path())
        .env("AIKIT_HOME", aikit_home.path())
        .current_dir(&project)
        .output()
        .expect("aikit init runs");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let locks = envelope["data"]["npx_skills"]["locks"]
        .as_array()
        .expect("npx lock reports");
    assert_eq!(locks.len(), 2, "{envelope}");
    let global = locks.iter().find(|lock| lock["scope"] == "global").unwrap();
    assert_eq!(global["version"], 3);
    assert_eq!(global["supported"], true);
    assert_eq!(global["entries"][0]["name"], "global-review");
    assert_eq!(global["entries"][0]["source"], "acme/skills");
    assert_eq!(global["entries"][0]["ref"], "v2");
    assert_eq!(global["entries"][0]["expected_hash"], "sha256-global");
    let local = locks
        .iter()
        .find(|lock| lock["scope"] == "project")
        .unwrap();
    assert_eq!(local["entries"][0]["name"], "project-review");
    assert_eq!(local["entries"][0]["expected_hash"], "sha256-project");

    let roots = envelope["data"]["roots"].as_array().unwrap();
    let project_skill_root = fs::canonicalize(project.join(".agents/skills")).unwrap();
    assert!(
        roots.iter().any(|root| {
            root["label"] == "@project-agents"
                && root["path"] == project_skill_root.display().to_string()
        }),
        "the npx project install root must be indexed: {roots:?}"
    );

    assert_eq!(fs::read(&global_lock).unwrap(), global_bytes);
    assert_eq!(fs::read(&project_lock).unwrap(), project_bytes);
    assert!(
        !project.join(".aikit").exists(),
        "foreign discovery remains read-only"
    );
}

#[test]
fn npx_project_skill_content_is_hashed_and_drift_is_reported_read_only() {
    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();
    let project = home.path().join("work/payments");
    let skills = project.join(".agents/skills");
    fs::create_dir_all(&skills).unwrap();
    skill(&skills, "review");
    let expected = skills_cli_folder_hash(&skills.join("review"));
    let lock = project.join("skills-lock.json");
    fs::write(
        &lock,
        format!(
            r#"{{"version":1,"skills":{{"review":{{"source":"acme/review","sourceType":"github","computedHash":"{expected}"}}}}}}"#
        ),
    )
    .unwrap();
    let original_lock = fs::read(&lock).unwrap();

    let inspect = || {
        let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
            .args(["init", "--json"])
            .env("HOME", home.path())
            .env("AIKIT_HOME", aikit_home.path())
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };

    let clean = inspect();
    let entry = &clean["data"]["npx_skills"]["locks"][0]["entries"][0];
    assert_eq!(entry["expected_hash"], expected);
    assert_eq!(entry["actual_hash"], expected);
    assert_eq!(entry["hash_matches"], true);

    fs::write(
        skills.join("review/SKILL.md"),
        "---\nname: review\ndescription: locally modified\n---\n",
    )
    .unwrap();
    let drifted = inspect();
    let entry = &drifted["data"]["npx_skills"]["locks"][0]["entries"][0];
    assert_eq!(entry["hash_matches"], false);
    assert_ne!(entry["actual_hash"], entry["expected_hash"]);
    assert_eq!(fs::read(&lock).unwrap(), original_lock);
}

#[cfg(unix)]
#[test]
fn npx_hash_matches_the_reference_path_order_and_ignores_symlinks() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();
    let project = home.path().join("work/payments");
    let review = project.join(".agents/skills/review");
    fs::create_dir_all(review.join("scripts")).unwrap();
    skill(&project.join(".agents/skills"), "review");
    fs::write(review.join("scripts/run.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    let outside = home.path().join("outside-secret");
    fs::write(&outside, "must not be hashed").unwrap();
    symlink(&outside, review.join("linked-outside")).unwrap();

    // Produced by skills 1.5.10's exact
    // `relativePath.localeCompare` + SHA-256 algorithm.
    let expected = "c73ecec995df90ee5ee32d918a6abb810d15b5850dd18b3d457f3406102e5f73";
    fs::write(
        project.join("skills-lock.json"),
        format!(
            r#"{{"version":1,"skills":{{"review":{{"source":"acme/review","sourceType":"github","computedHash":"{expected}"}}}}}}"#
        ),
    )
    .unwrap();

    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["init", "--json"])
        .env("HOME", home.path())
        .env("AIKIT_HOME", aikit_home.path())
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entry = &envelope["data"]["npx_skills"]["locks"][0]["entries"][0];
    assert_eq!(entry["actual_hash"], expected);
    assert_eq!(entry["hash_matches"], true);
}

#[test]
fn npx_hash_uses_nodes_exact_unicode_collation() {
    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();
    let project = home.path().join("work/payments");
    let review = project.join(".agents/skills/review");
    skill(&project.join(".agents/skills"), "review");
    fs::write(review.join("ä.txt"), "umlaut\n").unwrap();
    fs::write(review.join("z.txt"), "zed\n").unwrap();
    fs::write(review.join("a"), "upper\n").unwrap();

    // Produced independently with skills 1.5.10's Node implementation. Its
    // default localeCompare order is a, ä.txt, SKILL.md, z.txt on this supported
    // runtime; lowercase-plus-byte and Unicode-scalar sorts disagree.
    let expected = "7fe976be2fd5aabd58e679d8c343b6bcd7059812d865a1d056df1759c7b13e35";
    fs::write(
        project.join("skills-lock.json"),
        format!(
            r#"{{"version":1,"skills":{{"review":{{"source":"acme/review","sourceType":"github","computedHash":"{expected}"}}}}}}"#
        ),
    )
    .unwrap();

    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["init", "--json"])
        .env("HOME", home.path())
        .env("AIKIT_HOME", aikit_home.path())
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entry = &envelope["data"]["npx_skills"]["locks"][0]["entries"][0];
    assert_eq!(entry["actual_hash"], expected);
    assert_eq!(entry["hash_matches"], true);
}

#[test]
fn unsupported_npx_lock_versions_are_reported_but_never_interpreted() {
    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".agents")).unwrap();
    fs::write(
        home.path().join(".agents/.skill-lock.json"),
        r#"{"version":99,"skills":{"do-not-guess":{"source":"unknown"}}}"#,
    )
    .unwrap();

    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["init", "--json"])
        .env("HOME", home.path())
        .env("AIKIT_HOME", aikit_home.path())
        .current_dir(home.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let lock = &envelope["data"]["npx_skills"]["locks"][0];
    assert_eq!(lock["version"], 99);
    assert_eq!(lock["supported"], false);
    assert_eq!(lock["entries"].as_array().unwrap().len(), 0);
    assert!(lock["note"].as_str().unwrap().contains("unsupported"));
}

#[test]
fn npx_global_provenance_honours_xdg_state_home() {
    let home = TempDir::new().unwrap();
    let aikit_home = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    fs::create_dir_all(state.path().join("skills")).unwrap();
    fs::create_dir_all(home.path().join(".agents")).unwrap();
    fs::write(
        state.path().join("skills/.skill-lock.json"),
        r#"{"version":3,"skills":{"from-xdg":{"source":"xdg/source"}}}"#,
    )
    .unwrap();
    fs::write(
        home.path().join(".agents/.skill-lock.json"),
        r#"{"version":3,"skills":{"legacy-home":{"source":"wrong/source"}}}"#,
    )
    .unwrap();

    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["init", "--json"])
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", state.path())
        .env("AIKIT_HOME", aikit_home.path())
        .current_dir(home.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let lock = &envelope["data"]["npx_skills"]["locks"][0];
    assert_eq!(
        lock["path"],
        state
            .path()
            .join("skills/.skill-lock.json")
            .display()
            .to_string()
    );
    assert_eq!(lock["entries"][0]["name"], "from-xdg");
    assert!(lock["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["name"] != "legacy-home"));
}
