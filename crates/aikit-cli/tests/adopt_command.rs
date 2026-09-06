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

#[test]
fn control_adoption_stages_exact_ground_and_preserves_originals_until_cutover() {
    let (home, project, foreign) = fixture();
    let central = TempDir::new().unwrap();
    let ground = central.path().join("Control/machines/current/skills");
    fs::create_dir_all(&ground).unwrap();
    fs::create_dir(central.path().join(".central")).unwrap();
    let args = [
        "adopt",
        foreign.path().to_str().unwrap(),
        "--control-ground",
        ground.to_str().unwrap(),
    ];
    let preview = run(home.path(), project.path(), &args);
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    assert!(!ground.join("deep-review").exists());
    let review = envelope(&preview);
    let digest = review["data"]["review_digest"].as_str().unwrap();
    let mut confirmed = args.to_vec();
    confirmed.extend(["--yes", "--expect-digest", digest]);
    let applied = run(home.path(), project.path(), &confirmed);
    assert!(applied.status.success(), "{:?}", envelope(&applied));
    let body = envelope(&applied);
    assert_eq!(body["data"]["ownership"], "control-ground-staged");
    for relative in [
        "deep-review/SKILL.md",
        "deep-review/references/checklist.md",
    ] {
        assert_eq!(
            fs::read(foreign.path().join(relative)).unwrap(),
            fs::read(ground.join(relative)).unwrap()
        );
        assert!(!fs::symlink_metadata(foreign.path().join(relative))
            .unwrap()
            .file_type()
            .is_symlink());
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(ground.join("deep-review/skill.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema"], "central.skill/v1");
    assert_eq!(manifest["standing"], "active");
    assert_eq!(manifest["provenance"], "adopted");
    assert_eq!(
        manifest["adopted_from"],
        foreign
            .path()
            .join("deep-review")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    let staged = run(home.path(), project.path(), &args);
    assert!(staged.status.success(), "{:?}", envelope(&staged));
    let procedure = body["data"]["procedure"].as_str().unwrap();
    let undone = run(
        home.path(),
        project.path(),
        &["procedure", "undo", procedure],
    );
    assert!(undone.status.success(), "{:?}", envelope(&undone));
    assert!(!ground.join("deep-review/SKILL.md").exists());
    assert!(foreign.path().join("deep-review/SKILL.md").is_file());
}

#[test]
fn control_adoption_refuses_changed_review_and_conflicting_staged_bytes() {
    let (home, project, foreign) = fixture();
    let central = TempDir::new().unwrap();
    let ground = central.path().join("Control/machines/current/skills");
    fs::create_dir_all(&ground).unwrap();
    fs::create_dir(central.path().join(".central")).unwrap();
    let args = [
        "adopt",
        foreign.path().to_str().unwrap(),
        "--control-ground",
        ground.to_str().unwrap(),
    ];
    let preview = envelope(&run(home.path(), project.path(), &args));
    let digest = preview["data"]["review_digest"].as_str().unwrap();
    write(
        &foreign.path().join("deep-review/references/checklist.md"),
        "Changed after review\n",
    );
    let mut confirmed = args.to_vec();
    confirmed.extend(["--yes", "--expect-digest", digest]);
    let refused = run(home.path(), project.path(), &confirmed);
    assert!(!refused.status.success());
    assert!(!ground.join("deep-review").exists());
    write(
        &ground.join("deep-review/SKILL.md"),
        "---\nname: deep-review\ndescription: Existing owned skill.\n---\nKeep authored bytes.\n",
    );
    write(
        &ground.join("deep-review/skill.json"),
        r#"{"schema":"central.skill/v1","name":"deep-review","scope":"control-machine","standing":"active","provenance":"human-authored"}"#,
    );
    let before = fs::read(ground.join("deep-review/SKILL.md")).unwrap();
    assert!(!run(home.path(), project.path(), &args).status.success());
    assert_eq!(
        before,
        fs::read(ground.join("deep-review/SKILL.md")).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn control_adoption_preserves_retirement_and_refuses_external_links() {
    let (home, project, foreign) = fixture();
    let central = TempDir::new().unwrap();
    let ground = central.path().join("Control/machines/current/skills");
    fs::create_dir_all(&ground).unwrap();
    fs::create_dir(central.path().join(".central")).unwrap();
    for relative in [
        "deep-review/SKILL.md",
        "deep-review/references/checklist.md",
    ] {
        write(
            &ground.join(relative),
            &fs::read_to_string(foreign.path().join(relative)).unwrap(),
        );
    }
    let manifest = r#"{"schema":"central.skill/v1","name":"deep-review","scope":"control-machine","standing":"retired","provenance":"adopted","retirement":{"retired_by":"human","retired_at_unix_seconds":1788653215,"retirement_reason":"Already retired"}}"#;
    write(&ground.join("deep-review/skill.json"), manifest);
    let args = [
        "adopt",
        foreign.path().to_str().unwrap(),
        "--control-ground",
        ground.to_str().unwrap(),
    ];
    let preview = run(home.path(), project.path(), &args);
    assert!(preview.status.success(), "{:?}", envelope(&preview));
    assert_eq!(
        fs::read_to_string(ground.join("deep-review/skill.json")).unwrap(),
        manifest
    );
    let external = TempDir::new().unwrap();
    write(
        &external.path().join("SKILL.md"),
        "---\nname: external\ndescription: External authored skill.\n---\nStay here.\n",
    );
    std::os::unix::fs::symlink(external.path(), foreign.path().join("external")).unwrap();
    let refusal = run(home.path(), project.path(), &args);
    assert!(!refusal.status.success());
    assert!(!ground.join("external").exists());
    assert!(external.path().join("SKILL.md").is_file());
}

#[test]
fn personal_and_project_adoption_use_central_scopes_without_machine_attribution() {
    for (relative, scope, namespace) in [
        ("Control/user/skills", "control-user", "personal-ground"),
        (
            "Work/project/ProjectCentral/user/skills",
            "projectcentral-user",
            "project-ground",
        ),
    ] {
        let (home, project, foreign) = fixture();
        let central = TempDir::new().unwrap();
        let ground = central.path().join(relative);
        fs::create_dir_all(ground.parent().unwrap()).unwrap();
        fs::create_dir(central.path().join(".central")).unwrap();
        if scope == "projectcentral-user" {
            write(
                &central
                    .path()
                    .join("Work/project/ProjectCentral/project.json"),
                r#"{"schema":"central.project/v1","project_id":"project","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
            );
        }
        let args = [
            "adopt",
            foreign.path().to_str().unwrap(),
            "--control-ground",
            ground.to_str().unwrap(),
        ];
        let preview = run(home.path(), project.path(), &args);
        assert!(preview.status.success(), "{:?}", envelope(&preview));
        assert!(
            !ground.exists(),
            "preview must not create authored topology"
        );
        let body = envelope(&preview);
        assert_eq!(body["data"]["namespace"], namespace);
        let mut confirmed = args.to_vec();
        confirmed.extend([
            "--yes",
            "--expect-digest",
            body["data"]["review_digest"].as_str().unwrap(),
        ]);
        let applied = run(home.path(), project.path(), &confirmed);
        assert!(applied.status.success(), "{:?}", envelope(&applied));
        let manifest: Value =
            serde_json::from_slice(&fs::read(ground.join("deep-review/skill.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["scope"], scope);
        assert!(manifest.get("machine").is_none());
        assert_eq!(manifest["standing"], "active");
        assert_eq!(
            fs::read(ground.join("deep-review/SKILL.md")).unwrap(),
            fs::read(foreign.path().join("deep-review/SKILL.md")).unwrap()
        );
    }
}

#[test]
fn adoption_refuses_a_fourth_control_layout() {
    let (home, project, foreign) = fixture();
    let central = TempDir::new().unwrap();
    fs::create_dir_all(central.path().join("Control")).unwrap();
    fs::create_dir(central.path().join(".central")).unwrap();
    let ground = central.path().join("Control/skills");
    let result = run(
        home.path(),
        project.path(),
        &[
            "adopt",
            foreign.path().to_str().unwrap(),
            "--control-ground",
            ground.to_str().unwrap(),
        ],
    );
    assert!(!result.status.success());
    assert!(!ground.exists());
}

fn successful(home: &Path, project: &Path, args: &[&str]) -> Value {
    let output = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env("AIKIT_CONTEXT_ID", "ctx_CONTROLCUTOVER000000000")
        .current_dir(project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {:?}", envelope(&output));
    envelope(&output)
}

#[test]
fn control_cutover_uses_a_real_generation_and_undo_restores_the_original_tree() {
    let (home, project, original) = fixture();
    let foreign = home.path().join(".agents/skills");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::rename(original.path(), &foreign).unwrap();
    write(&foreign.as_path().join("retired/SKILL.md"), "---\nname: retired\ndescription: Retired human skill.\n---\nPreserve the authored content.\n");
    let central = TempDir::new().unwrap();
    let ground = central.path().join("Control/user/skills");
    fs::create_dir_all(ground.parent().unwrap()).unwrap();
    fs::create_dir(central.path().join(".central")).unwrap();
    let stage = [
        "adopt",
        foreign.as_path().to_str().unwrap(),
        "--control-ground",
        ground.to_str().unwrap(),
    ];
    let preview = successful(home.path(), project.path(), &stage);
    let mut accept = stage.to_vec();
    accept.extend([
        "--yes",
        "--expect-digest",
        preview["data"]["review_digest"].as_str().unwrap(),
    ]);
    successful(home.path(), project.path(), &accept);
    write(
        &ground.join("retired/skill.json"),
        r#"{"schema":"central.skill/v1","name":"retired","scope":"control-user","standing":"retired","provenance":"adopted","retirement":{"retired_by":"human","retired_at_unix_seconds":1788653215,"retirement_reason":"No longer selected for use"}}"#,
    );
    successful(
        home.path(),
        project.path(),
        &[
            "source",
            "add-directory",
            "personal",
            ground.to_str().unwrap(),
            "--control-ground",
        ],
    );
    successful(home.path(), project.path(), &["source", "sync", "personal"]);
    successful(
        home.path(),
        project.path(),
        &["source", "promote", "personal", "--trust"],
    );
    successful(
        home.path(),
        project.path(),
        &["enable", "skill/personal/deep-review", "--scope", "user"],
    );
    successful(
        home.path(),
        project.path(),
        &["enable", "skill/personal/retired", "--scope", "user"],
    );
    let applied = successful(home.path(), project.path(), &["apply"]);
    let projection = home
        .path()
        .join("state/contexts")
        .join(applied["context"]["context_id"].as_str().unwrap())
        .join("current/projections/codex/.agents/skills");
    let mut cutover = stage.to_vec();
    cutover.extend(["--projection", projection.to_str().unwrap()]);
    let preview = successful(home.path(), project.path(), &cutover);
    assert!(!foreign.as_path().is_symlink());
    let old_digest = preview["data"]["review_digest"].as_str().unwrap();
    // A changed source between preview and confirmation must remain untouched.
    write(
        &foreign.as_path().join("unaccounted.txt"),
        "Human content outside every skill.\n",
    );
    let mut confirm = cutover.clone();
    confirm.extend(["--yes", "--expect-digest", old_digest]);
    let refused = run(home.path(), project.path(), &confirm);
    assert!(!refused.status.success());
    assert!(envelope(&refused)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unaccounted"));
    assert!(!foreign.as_path().is_symlink());
    fs::remove_file(foreign.as_path().join("unaccounted.txt")).unwrap();
    // A newly restored skill requires a fresh source snapshot and generation.
    let retirement = fs::read(ground.join("retired/skill.json")).unwrap();
    write(
        &ground.join("retired/skill.json"),
        r#"{"schema":"central.skill/v1","name":"retired","scope":"control-user","standing":"active","provenance":"adopted"}"#,
    );
    assert!(!run(home.path(), project.path(), &confirm).status.success());
    assert!(!foreign.as_path().is_symlink());
    fs::write(ground.join("retired/skill.json"), retirement).unwrap();
    let confirmed = successful(home.path(), project.path(), &confirm);
    assert_eq!(confirmed["data"]["ownership"], "control-ground-projected");
    assert_eq!(
        fs::read_link(foreign.as_path()).unwrap(),
        home.path()
            .join("state/contexts")
            .canonicalize()
            .unwrap()
            .join(
                projection
                    .strip_prefix(home.path().join("state/contexts"))
                    .unwrap()
            )
    );
    let tree = successful(home.path(), project.path(), &["tree", "--expand", "registries"]);
    let agents = tree["data"]["rows"].as_array().unwrap().iter().find(|row| row["path"] == "registries/@agents").unwrap();
    assert!(agents["summary"].as_str().unwrap().starts_with("generated ·"));
    assert!(!foreign.as_path().join("retired/SKILL.md").exists());
    assert!(ground.join("retired/SKILL.md").is_file());
    let procedure = confirmed["data"]["procedure"].as_str().unwrap();
    assert_eq!(
        fs::read(foreign.as_path().join("deep-review/SKILL.md")).unwrap(),
        fs::read(ground.join("deep-review/SKILL.md")).unwrap()
    );
    // Native selection/application swaps the generation behind the stable link.
    successful(
        home.path(),
        project.path(),
        &["disable", "skill/personal/deep-review", "--scope", "user"],
    );
    successful(home.path(), project.path(), &["apply"]);
    assert!(!foreign.as_path().join("deep-review/SKILL.md").exists());
    successful(
        home.path(),
        project.path(),
        &["procedure", "undo", procedure],
    );
    assert!(!foreign.as_path().is_symlink());
    assert!(foreign.as_path().join("retired/SKILL.md").is_file());
    assert_eq!(
        fs::read(foreign.as_path().join("deep-review/SKILL.md")).unwrap(),
        fs::read(ground.join("deep-review/SKILL.md")).unwrap()
    );
}

#[test]
fn mixed_harness_cutover_recovers_links_preserves_host_files_and_undoes() {
    let (home, project, authored) = fixture();
    write(&authored.path().join("recovered/SKILL.md"), "---\nname: recovered\ndescription: Recovered local skill.\n---\nLocal content.\n");
    let root = home.path().join("harness/skills");
    fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(authored.path().join("deep-review"), root.join("deep-review")).unwrap();
    std::os::unix::fs::symlink("/old/moved/recovered", root.join("recovered")).unwrap();
    write(&root.join(".system/owner.txt"), "Harness-owned content");
    write(&root.join("notes.md"), "Human note");
    write(&root.join("unselected/SKILL.md"), "---\nname: unselected\ndescription: Harness-local skill.\n---\nKeep me.\n");
    successful(home.path(), project.path(), &["source", "add-directory", "local", authored.path().to_str().unwrap()]);
    successful(home.path(), project.path(), &["source", "sync", "local"]);
    successful(home.path(), project.path(), &["source", "promote", "local"]);
    for id in ["skill/local/deep-review", "skill/local/recovered"] {
        successful(home.path(), project.path(), &["enable", id, "--scope", "user"]);
    }
    let applied = successful(home.path(), project.path(), &["apply"]);
    let projection = home.path().join("state/contexts").join(applied["context"]["context_id"].as_str().unwrap()).join("current/projections/codex/.agents/skills");
    let pure = home.path().join(".agents/skills");
    fs::create_dir_all(&pure).unwrap();
    let pure_args = ["adopt", pure.to_str().unwrap(), "--projection", projection.to_str().unwrap()];
    let preview_pure = successful(home.path(), project.path(), &pure_args);
    let mut confirm_pure = pure_args.to_vec();
    confirm_pure.extend(["--yes", "--expect-digest", preview_pure["data"]["review_digest"].as_str().unwrap()]);
    successful(home.path(), project.path(), &confirm_pure);
    let tree = successful(home.path(), project.path(), &["tree", "--expand", "registries"]);
    let pure_row = tree["data"]["rows"].as_array().unwrap().iter().find(|r| r["path"] == "registries/@agents").unwrap();
    assert!(pure_row["summary"].as_str().unwrap().starts_with("generated ·"));
    let args = ["adopt", root.to_str().unwrap(), "--projection", projection.to_str().unwrap()];
    let preview = successful(home.path(), project.path(), &args);
    assert!(!root.join("recovered").exists());
    let mut confirm = args.to_vec();
    confirm.extend(["--yes", "--expect-digest", preview["data"]["review_digest"].as_str().unwrap()]);
    // Actual edits to a symlink's authored target invalidate the old review.
    let original = fs::read(authored.path().join("deep-review/SKILL.md")).unwrap();
    fs::write(authored.path().join("deep-review/SKILL.md"), "Changed human content").unwrap();
    assert!(!run(home.path(), project.path(), &confirm).status.success());
    fs::write(authored.path().join("deep-review/SKILL.md"), &original).unwrap();
    let accepted = successful(home.path(), project.path(), &confirm);
    assert_eq!(accepted["data"]["ownership"], "generation-projected");
    assert_eq!(fs::read(root.join("deep-review/SKILL.md")).unwrap(), original);
    assert!(root.join("recovered/SKILL.md").is_file());
    assert_eq!(fs::read_to_string(root.join(".system/owner.txt")).unwrap(), "Harness-owned content");
    assert_eq!(fs::read_to_string(root.join("notes.md")).unwrap(), "Human note");
    assert!(root.join("unselected/SKILL.md").is_file());
    assert_eq!(successful(home.path(), project.path(), &args)["data"]["skills"], 0);
    successful(home.path(), project.path(), &["procedure", "undo", accepted["data"]["procedure"].as_str().unwrap()]);
    assert_eq!(fs::read_link(root.join("recovered")).unwrap(), Path::new("/old/moved/recovered"));
    assert_eq!(fs::read_link(root.join("deep-review")).unwrap(), authored.path().join("deep-review"));
}
