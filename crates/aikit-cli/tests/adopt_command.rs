//! Adoption moves authority through the real Procedure engine.
//!
//! These tests drive the binary against ordinary directories. They deliberately
//! inspect both sides of the move: the owned capsule must be loadable, and the
//! foreign tree must become a projection that can be undone.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn fixture() -> (TempDir, TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let foreign = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    write(
        &foreign.path().join("deep-review/SKILL.md"),
        "---\nname: deep-review\ndescription: Review the whole change.\n---\n\nRead every diff.\n",
    );
    write(
        &foreign.path().join("deep-review/references/checklist.md"),
        "# Checklist\n\n- Correctness\n",
    );
    (home, project, foreign)
}

fn run(home: &Path, project: &Path, args: &[&str]) -> Output {
    Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap()
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected one JSON envelope ({error}); stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn adoption_without_confirmation_is_a_real_diff_and_writes_nothing() {
    let (home, project, foreign) = fixture();
    let source = foreign.path().to_str().unwrap();
    let before = fs::read(foreign.path().join("deep-review/SKILL.md")).unwrap();

    let output = run(
        home.path(),
        project.path(),
        &["adopt", source, "--namespace", "claude"],
    );
    assert!(output.status.success(), "{:?}", envelope(&output));
    let body = envelope(&output);
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["applied"], false);
    assert!(
        body["data"]["diff"]
            .as_str()
            .unwrap()
            .contains("skill/claude/deep-review"),
        "{body}"
    );
    assert_eq!(
        fs::read(foreign.path().join("deep-review/SKILL.md")).unwrap(),
        before,
    );
    assert!(!home
        .path()
        .join("registries/personal/capsules/skill/claude/deep-review")
        .exists());
}

#[test]
fn confirmation_is_refused_when_the_source_changed_after_preview() {
    let (home, project, foreign) = fixture();
    let source = foreign.path().to_str().unwrap();
    let preview = run(
        home.path(),
        project.path(),
        &["adopt", source, "--namespace", "claude"],
    );
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    write(
        &foreign.path().join("deep-review/SKILL.md"),
        "---\nname: deep-review\ndescription: Changed after review.\n---\n",
    );

    let output = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            source,
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );

    assert!(!output.status.success());
    assert_eq!(
        envelope(&output)["error"]["code"],
        "procedure.review_mismatch"
    );
    assert!(!home
        .path()
        .join("registries/personal/capsules/skill/claude/deep-review")
        .exists());
}

#[cfg(unix)]
#[test]
fn confirmed_adoption_moves_authority_and_undo_restores_the_foreign_tree() {
    use std::os::unix::fs::PermissionsExt;

    let (home, project, foreign) = fixture();
    let source = foreign.path().to_str().unwrap();
    let skill_file = foreign.path().join("deep-review/SKILL.md");
    let reference = foreign.path().join("deep-review/references/checklist.md");
    let original_skill = fs::read(&skill_file).unwrap();
    let original_reference = fs::read(&reference).unwrap();
    fs::set_permissions(&reference, fs::Permissions::from_mode(0o755)).unwrap();

    let preview = run(
        home.path(),
        project.path(),
        &["adopt", source, "--namespace", "claude"],
    );
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let output = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            source,
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(output.status.success(), "{:?}", envelope(&output));
    let body = envelope(&output);
    assert_eq!(body["data"]["applied"], true);
    assert_eq!(body["data"]["skills"], 1);
    assert_eq!(body["data"]["ownership"], "adopted");
    let procedure = body["data"]["procedure"].as_str().unwrap();

    let capsule = home
        .path()
        .join("registries/personal/capsules/skill/claude/deep-review");
    assert!(capsule.join("manifest.toml").is_file());
    assert_eq!(
        fs::read(capsule.join("payload/SKILL.md")).unwrap(),
        original_skill
    );
    assert_eq!(
        fs::read(capsule.join("payload/references/checklist.md")).unwrap(),
        original_reference
    );
    assert_eq!(
        fs::metadata(capsule.join("payload/references/checklist.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "executable payload fidelity is preserved"
    );
    assert!(
        skill_file.is_symlink(),
        "the foreign file is now a projection"
    );
    assert!(
        reference.is_symlink(),
        "the complete skill tree is projected"
    );

    let loaded = aikit_store::registry::load_registry(
        &home.path().join("registries/personal"),
        aikit_core::RegistrySource::personal(),
    )
    .unwrap();
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    assert_eq!(
        aikit_core::catalog::Catalog::capsules(&loaded.catalog).len(),
        1
    );

    let undo = run(
        home.path(),
        project.path(),
        &["procedure", "undo", procedure],
    );
    assert!(undo.status.success(), "{:?}", envelope(&undo));
    assert_eq!(envelope(&undo)["data"]["undone"], 6);
    assert!(!skill_file.is_symlink());
    assert!(!reference.is_symlink());
    assert_eq!(fs::read(&skill_file).unwrap(), original_skill);
    assert_eq!(fs::read(&reference).unwrap(), original_reference);
    assert_eq!(
        fs::metadata(&reference).unwrap().permissions().mode() & 0o777,
        0o755,
        "undo restores the original mode as well as the bytes"
    );
    assert!(
        !capsule.join("manifest.toml").exists(),
        "undo removes the owned files it created"
    );
}

#[cfg(unix)]
#[test]
fn adoption_moves_a_linked_skill_without_mutating_its_external_target() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let foreign = TempDir::new().unwrap();
    let external = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    let external_skill = external.path().join("shared-review");
    let original = "---\nname: shared-review\ndescription: Shared review instructions.\n---\n\nExternal source.\n";
    write(&external_skill.join("SKILL.md"), original);
    let projected = foreign.path().join("shared-review");
    symlink(&external_skill, &projected).unwrap();

    let preview = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "claude",
        ],
    );
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let output = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(output.status.success(), "{:?}", envelope(&output));
    let body = envelope(&output);
    let procedure = body["data"]["procedure"].as_str().unwrap();
    let owned_payload = home
        .path()
        .join("registries/personal/capsules/skill/claude/shared-review/payload");

    assert_eq!(
        fs::read_link(&projected).unwrap(),
        owned_payload,
        "the foreign directory entry now projects the AIKit-owned payload"
    );
    assert_eq!(
        fs::read_to_string(external_skill.join("SKILL.md")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(projected.join("SKILL.md")).unwrap(),
        original
    );

    let undo = run(
        home.path(),
        project.path(),
        &["procedure", "undo", procedure],
    );
    assert!(undo.status.success(), "{:?}", envelope(&undo));
    assert_eq!(
        fs::read_link(&projected).unwrap(),
        external_skill,
        "undo restores the original link target rather than copying its bytes"
    );
    assert_eq!(
        fs::read_to_string(projected.join("SKILL.md")).unwrap(),
        original
    );
}

#[cfg(unix)]
#[test]
fn linked_skill_confirmation_is_refused_when_only_the_link_target_changes() {
    use std::os::unix::fs::symlink;

    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let foreign = TempDir::new().unwrap();
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    let identical = "---\nname: shared\ndescription: Identical bytes, different authority.\n---\n";
    write(&first.path().join("shared/SKILL.md"), identical);
    write(&second.path().join("shared/SKILL.md"), identical);
    let projected = foreign.path().join("shared");
    symlink(first.path().join("shared"), &projected).unwrap();

    let preview = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "claude",
        ],
    );
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    fs::remove_file(&projected).unwrap();
    symlink(second.path().join("shared"), &projected).unwrap();

    let applied = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(!applied.status.success());
    assert_eq!(
        envelope(&applied)["error"]["code"],
        "procedure.review_mismatch"
    );
    assert_eq!(
        fs::read_link(&projected).unwrap(),
        second.path().join("shared")
    );
}

#[cfg(unix)]
#[test]
fn adoption_refuses_a_skill_tree_that_escapes_through_a_directory_symlink() {
    let (home, project, foreign) = fixture();
    let outside = TempDir::new().unwrap();
    write(
        &outside.path().join("secret.txt"),
        "outside the requested authority root\n",
    );
    std::os::unix::fs::symlink(
        outside.path(),
        foreign.path().join("deep-review/references/escape"),
    )
    .unwrap();

    let output = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            foreign.path().to_str().unwrap(),
            "--namespace",
            "claude",
            "--yes",
        ],
    );

    assert!(!output.status.success());
    assert_eq!(
        envelope(&output)["error"]["code"],
        "adopt.symlink_not_supported"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("secret.txt")).unwrap(),
        "outside the requested authority root\n"
    );
}

#[cfg(unix)]
#[test]
fn the_tree_reports_an_adopted_root_as_adopted_not_foreign() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    let root = home.path().join(".claude/skills");
    write(
        &root.join("deep-review/SKILL.md"),
        "---\nname: deep-review\ndescription: Review the whole change.\n---\n",
    );

    let preview = run(
        home.path(),
        project.path(),
        &["adopt", root.to_str().unwrap(), "--namespace", "claude"],
    );
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let adopted = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            root.to_str().unwrap(),
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(adopted.status.success(), "{:?}", envelope(&adopted));

    let tree = run(
        home.path(),
        project.path(),
        &["tree", "--expand", "registries"],
    );
    assert!(tree.status.success(), "{:?}", envelope(&tree));
    let rows = envelope(&tree)["data"]["rows"].clone();
    let claude = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == "registries/@claude")
        .expect("the default Claude root is present");
    assert!(
        claude["summary"].as_str().unwrap().contains("adopted"),
        "authority state must not regress to foreign after adoption: {claude}"
    );
    assert!(!claude["summary"].as_str().unwrap().contains("foreign"));
}

#[test]
fn authority_record_refuses_a_journal_from_an_unrelated_procedure_kind() {
    let (home, project, foreign) = fixture();
    let source = foreign.path().to_str().unwrap();
    let preview = run(
        home.path(),
        project.path(),
        &["adopt", source, "--namespace", "claude"],
    );
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let adopted = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            source,
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(adopted.status.success(), "{:?}", envelope(&adopted));
    let procedure = envelope(&adopted)["data"]["procedure"]
        .as_str()
        .unwrap()
        .to_string();
    let metadata = home
        .path()
        .join("state/procedures")
        .join(procedure)
        .join("procedure.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    value["kind"] = serde_json::json!({"kind": "doctor-fix", "checks": []});
    fs::write(&metadata, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let tree = run(
        home.path(),
        project.path(),
        &["tree", "--expand", "registries"],
    );
    assert!(!tree.status.success());
    assert_eq!(envelope(&tree)["error"]["code"], "adopt.record_unreadable");
}

#[test]
fn authority_record_cannot_claim_a_source_root_the_procedure_did_not_move() {
    let (home, project, foreign) = fixture();
    let other_source = TempDir::new().unwrap();
    let source = foreign.path().to_str().unwrap();
    let preview = run(
        home.path(),
        project.path(),
        &["adopt", source, "--namespace", "claude"],
    );
    let digest = envelope(&preview)["data"]["review_digest"]
        .as_str()
        .unwrap()
        .to_string();
    let adopted = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            source,
            "--namespace",
            "claude",
            "--yes",
            "--expect-digest",
            &digest,
        ],
    );
    assert!(adopted.status.success(), "{:?}", envelope(&adopted));

    let record = home.path().join("state/adoptions/claude.toml");
    let mut value: toml::Value = toml::from_str(&fs::read_to_string(&record).unwrap()).unwrap();
    value["source"] = toml::Value::String(
        fs::canonicalize(other_source.path())
            .unwrap()
            .display()
            .to_string(),
    );
    fs::write(&record, toml::to_string_pretty(&value).unwrap()).unwrap();

    let tree = run(
        home.path(),
        project.path(),
        &["tree", "--expand", "registries"],
    );
    assert!(!tree.status.success());
    assert_eq!(envelope(&tree)["error"]["code"], "adopt.record_unreadable");
}
