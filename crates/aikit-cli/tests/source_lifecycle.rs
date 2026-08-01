use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn skill(root: &Path, name: &str, marker: &str) {
    fs::create_dir_all(root.join("references")).unwrap();
    fs::create_dir_all(root.join("agents")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Exercises the real source lifecycle.\ndisable-model-invocation: true\n---\n\n{marker}\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("references/rules.md"),
        format!("rules:{marker}\n"),
    )
    .unwrap();
    fs::write(
        root.join("agents/openai.yaml"),
        "interface:\n  display_name: Real\n",
    )
    .unwrap();
}

fn aikit(home: &Path, cwd: &Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .env("AIKIT_HOME", home)
        .env("HOME", home.join("user-home"))
        .current_dir(cwd)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "aikit {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn data(value: &Value) -> &Value {
    &value["data"]
}

#[test]
fn git_sources_require_an_exact_commit_and_a_contained_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let cwd = temp.path().join("project");
    fs::create_dir_all(&cwd).unwrap();

    for arguments in [
        vec![
            "source",
            "add-git",
            "moving-branch",
            "https://example.invalid/skills.git",
            "--revision",
            "main",
        ],
        vec![
            "source",
            "add-git",
            "escaping-root",
            "https://example.invalid/skills.git",
            "--revision",
            "0123456789abcdef0123456789abcdef01234567",
            "--root",
            "../outside",
        ],
    ] {
        Command::cargo_bin("aikit")
            .unwrap()
            .env("AIKIT_HOME", &home)
            .env("HOME", home.join("user-home"))
            .current_dir(&cwd)
            .arg("--json")
            .args(arguments)
            .assert()
            .failure();
    }

    let secret = "never-print-this-token";
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .env("AIKIT_HOME", &home)
        .env("HOME", home.join("user-home"))
        .current_dir(&cwd)
        .arg("--json")
        .args([
            "source",
            "add-git",
            "credentialed",
            &format!("https://{secret}@example.invalid/skills.git"),
            "--revision",
            "0123456789abcdef0123456789abcdef01234567",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[test]
fn directory_source_sync_is_candidate_only_then_promotes_an_immutable_complete_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let source = temp.path().join("writing-guidance-tools");
    let cwd = temp.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    skill(&source, "writing-guidance-tools", "version-one");

    let added = aikit(
        &home,
        &cwd,
        &[
            "source",
            "add-directory",
            "writing-guidance-tools",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(data(&added)["kind"], "directory");
    assert_eq!(data(&added)["portable"], false);

    let synced = aikit(&home, &cwd, &["source", "sync", "writing-guidance-tools"]);
    assert_eq!(data(&synced)["skills"], 1);
    assert_eq!(data(&synced)["active_snapshot"], Value::Null);
    let candidate = data(&synced)["candidate_snapshot"]
        .as_str()
        .unwrap()
        .to_string();

    let before = aikit(&home, &cwd, &["status", "--all"]);
    let unavailable = data(&before)["unavailable"].as_array().unwrap();
    assert!(unavailable
        .iter()
        .all(|row| { row["id"] != "skill/writing-guidance-tools/writing-guidance-tools" }));

    let promoted = aikit(
        &home,
        &cwd,
        &["source", "promote", "writing-guidance-tools", "--trust"],
    );
    assert_eq!(data(&promoted)["active_snapshot"], candidate);
    assert_eq!(data(&promoted)["trusted_skills"], 1);

    let shown = aikit(&home, &cwd, &["source", "show", "writing-guidance-tools"]);
    let registry = PathBuf::from(data(&shown)["active_registry"].as_str().unwrap());
    let payload =
        registry.join("capsules/skill/writing-guidance-tools/writing-guidance-tools/payload");
    assert_eq!(
        fs::read_to_string(payload.join("references/rules.md")).unwrap(),
        "rules:version-one\n"
    );
    assert_eq!(
        fs::read_to_string(payload.join("agents/openai.yaml")).unwrap(),
        "interface:\n  display_name: Real\n"
    );
    assert!(
        fs::read_to_string(payload.join("SKILL.md"))
            .unwrap()
            .contains("disable-model-invocation: true"),
        "invocation policy must survive ingestion byte-for-byte"
    );

    skill(&source, "writing-guidance-tools", "version-two");
    let second = aikit(&home, &cwd, &["source", "sync", "writing-guidance-tools"]);
    assert_ne!(data(&second)["candidate_snapshot"], candidate);
    assert_eq!(data(&second)["active_snapshot"], candidate);
    assert_eq!(
        fs::read_to_string(payload.join("references/rules.md")).unwrap(),
        "rules:version-one\n",
        "sync must not mutate the promoted snapshot"
    );

    aikit(
        &home,
        &cwd,
        &["source", "promote", "writing-guidance-tools", "--trust"],
    );
    let version_two = aikit(&home, &cwd, &["source", "show", "writing-guidance-tools"]);
    let version_two_digest = data(&version_two)["active_snapshot"]
        .as_str()
        .unwrap()
        .to_string();
    let second_registry = PathBuf::from(data(&version_two)["active_registry"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(
            second_registry.join(
                "capsules/skill/writing-guidance-tools/writing-guidance-tools/payload/references/rules.md"
            )
        )
        .unwrap(),
        "rules:version-two\n"
    );

    skill(&source, "writing-guidance-tools", "version-three");
    aikit(&home, &cwd, &["source", "sync", "writing-guidance-tools"]);
    aikit(
        &home,
        &cwd,
        &["source", "promote", "writing-guidance-tools", "--trust"],
    );
    let rolled_to_two = aikit(
        &home,
        &cwd,
        &["source", "rollback", "writing-guidance-tools"],
    );
    assert_eq!(data(&rolled_to_two)["active_snapshot"], version_two_digest);
    let rolled = aikit(
        &home,
        &cwd,
        &["source", "rollback", "writing-guidance-tools"],
    );
    assert_eq!(data(&rolled)["active_snapshot"], candidate);
    let after_rollback = aikit(&home, &cwd, &["source", "show", "writing-guidance-tools"]);
    assert_eq!(
        PathBuf::from(data(&after_rollback)["active_registry"].as_str().unwrap()),
        registry
    );

    fs::create_dir_all(cwd.join(".aikit")).unwrap();
    aikit(
        &home,
        &cwd,
        &[
            "enable",
            "skill/writing-guidance-tools/writing-guidance-tools",
            "--scope",
            "project",
        ],
    );
    let rollback_status = aikit(&home, &cwd, &["status", "--all"]);
    assert!(
        data(&rollback_status)["active"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "skill/writing-guidance-tools/writing-guidance-tools"),
        "rollback must restore the trust previously reviewed for that immutable revision"
    );
}

#[cfg(unix)]
#[test]
fn directory_snapshot_identity_includes_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let source = temp.path().join("permission-sensitive-skill");
    let cwd = temp.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    skill(&source, "permission-sensitive-skill", "same-bytes");

    aikit(
        &home,
        &cwd,
        &[
            "source",
            "add-directory",
            "permission-source",
            source.to_str().unwrap(),
        ],
    );
    let first = aikit(&home, &cwd, &["source", "sync", "permission-source"]);
    let first_digest = data(&first)["candidate_snapshot"].as_str().unwrap();

    let script = source.join("references/rules.md");
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).unwrap();

    let second = aikit(&home, &cwd, &["source", "sync", "permission-source"]);
    assert_ne!(
        data(&second)["candidate_snapshot"].as_str().unwrap(),
        first_digest,
        "a permission-only change must create a distinct immutable snapshot"
    );
}

#[test]
fn git_source_resolves_an_exact_commit_and_ignores_later_worktree_changes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let repository = temp.path().join("matt-skills");
    let cwd = temp.path().join("project");
    fs::create_dir_all(repository.join("skills/engineering")).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    git(&repository, &["init", "--quiet"]);
    git(&repository, &["config", "user.name", "AIKit test"]);
    git(
        &repository,
        &["config", "user.email", "aikit@example.invalid"],
    );
    skill(
        &repository.join("skills/engineering/wayfinder"),
        "wayfinder",
        "pinned-version",
    );
    skill(
        &repository.join("skills/engineering/grilling"),
        "grilling",
        "pinned-version",
    );
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "--quiet", "-m", "fixture"]);
    let commit = git(&repository, &["rev-parse", "HEAD"]);

    let added = aikit(
        &home,
        &cwd,
        &[
            "source",
            "add-git",
            "mattpocock",
            repository.to_str().unwrap(),
            "--revision",
            &commit,
            "--root",
            "skills",
        ],
    );
    assert_eq!(data(&added)["kind"], "git");
    assert_eq!(data(&added)["portable"], true);

    fs::write(
        repository.join("skills/engineering/wayfinder/references/rules.md"),
        "uncommitted mutation\n",
    )
    .unwrap();

    let synced = aikit(&home, &cwd, &["source", "sync", "mattpocock"]);
    assert_eq!(data(&synced)["skills"], 2);
    assert_eq!(data(&synced)["git_commit"], commit);
    let promoted = aikit(
        &home,
        &cwd,
        &[
            "source",
            "promote",
            "mattpocock",
            "--trust-skill",
            "skill/mattpocock/engineering/wayfinder",
        ],
    );
    assert_eq!(data(&promoted)["trusted_skills"], 1);
    let shown = aikit(&home, &cwd, &["source", "show", "mattpocock"]);
    let registry = PathBuf::from(data(&shown)["active_registry"].as_str().unwrap());
    assert!(
        !registry.parent().unwrap().join("checkout").exists(),
        "the mutable Git checkout must not be retained in the immutable snapshot"
    );
    assert_eq!(
        fs::read_to_string(
            registry.join(
                "capsules/skill/mattpocock/engineering/wayfinder/payload/references/rules.md"
            )
        )
        .unwrap(),
        "rules:pinned-version\n",
        "the immutable snapshot must come from the exact commit, not the mutable checkout"
    );

    fs::write(
        repository.join("skills/engineering/wayfinder/references/rules.md"),
        "rules:second-commit\n",
    )
    .unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "--quiet", "-m", "second fixture"]);
    let second_commit = git(&repository, &["rev-parse", "HEAD"]);

    let pinned = aikit(
        &home,
        &cwd,
        &["source", "set-revision", "mattpocock", &second_commit],
    );
    assert_eq!(data(&pinned)["revision"], second_commit);
    let update = aikit(&home, &cwd, &["source", "sync", "mattpocock"]);
    let second_digest = data(&update)["candidate_snapshot"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(data(&update)["git_commit"], second_commit);
    assert_eq!(
        data(&update)["active_snapshot"],
        data(&shown)["active_snapshot"],
        "advancing a pin and syncing must not silently promote it"
    );
    aikit(&home, &cwd, &["source", "promote", "mattpocock", "--trust"]);
    let updated = aikit(&home, &cwd, &["source", "show", "mattpocock"]);
    let updated_registry = PathBuf::from(data(&updated)["active_registry"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(
            updated_registry.join(
                "capsules/skill/mattpocock/engineering/wayfinder/payload/references/rules.md"
            )
        )
        .unwrap(),
        "rules:second-commit\n"
    );
    let rolled = aikit(&home, &cwd, &["source", "rollback", "mattpocock"]);
    assert_eq!(
        data(&rolled)["active_snapshot"],
        data(&shown)["active_snapshot"],
        "rollback must restore the previous immutable Git snapshot"
    );

    git(
        &repository,
        &["commit", "--quiet", "--allow-empty", "-m", "same tree"],
    );
    let same_tree_commit = git(&repository, &["rev-parse", "HEAD"]);
    aikit(
        &home,
        &cwd,
        &["source", "set-revision", "mattpocock", &same_tree_commit],
    );
    let same_tree = aikit(&home, &cwd, &["source", "sync", "mattpocock"]);
    assert_eq!(data(&same_tree)["git_commit"], same_tree_commit);
    assert_ne!(
        data(&same_tree)["candidate_snapshot"].as_str().unwrap(),
        second_digest,
        "two exact commits with identical skill bytes need distinct provenance snapshots"
    );
}
