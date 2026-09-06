//! Control-ground skill sources (OI-GIT-NORM K2): a machine-local source whose
//! skills may carry the sibling `central.skill/v1` contract (`skill.json`),
//! read at its published surface only.
//!
//! The acceptance criteria, one per test:
//!
//! * a retired member **never projects**, and `aikit set show` discloses it
//!   withheld-retired with the owner's own reason — not as missing, not as an
//!   error (honesty law);
//! * a contract violation is refused at `sync`, naming the file;
//! * a source that never opted in ignores the contract entirely;
//! * a ProjectCentral-ground source registers the same way and its projection
//!   eligibility follows the existing per-project enable scoping — no new
//!   mechanism.

use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::Command;
use serde_json::Value;

fn write_skill(root: &Path, name: &str) {
    fs::create_dir_all(root.join("references")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: The {name} skill for the Control ground test.\n---\n\n{name}\n"
        ),
    )
    .unwrap();
    fs::write(root.join("references/proof.md"), format!("{name}-proof\n")).unwrap();
}

fn write_manifest(root: &Path, body: &str) {
    fs::write(root.join("skill.json"), body).unwrap();
}

fn active_manifest(name: &str, scope: &str) -> String {
    format!(
        r#"{{"schema":"central.skill/v1","name":"{name}","scope":"{scope}","standing":"active","provenance":"human-authored"}}"#
    )
}

fn retired_manifest(name: &str, scope: &str, reason: &str) -> String {
    format!(
        r#"{{"schema":"central.skill/v1","name":"{name}","scope":"{scope}","standing":"retired","provenance":"human-authored","retirement":{{"retired_by":"owner","retired_at_unix_seconds":1788653215,"retirement_reason":"{reason}"}}}}"#
    )
}

fn aikit(home: &Path, cwd: &Path, args: &[&str]) -> Value {
    let output = run(home, cwd, args);
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .env("AIKIT_HOME", home)
        .env("HOME", home.join("user-home"))
        .env("AIKIT_CONTEXT_ID", "ctx_K2GROUND00000000000000")
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
    output
}

fn data(value: &Value) -> &Value {
    &value["data"]
}

/// One Control ground fixture: an active skill, a retired skill with its
/// retirement record, and a plain skill publishing no contract at all.
fn ground_fixture(temp: &Path) -> std::path::PathBuf {
    let ground = temp.join("ground");
    write_skill(&ground.join("keeper"), "keeper");
    write_manifest(
        &ground.join("keeper"),
        &active_manifest("keeper", "control-machine"),
    );
    write_skill(&ground.join("archived"), "archived");
    write_manifest(
        &ground.join("archived"),
        &retired_manifest(
            "archived",
            "control-machine",
            "Misroutes more than it helps.",
        ),
    );
    write_skill(&ground.join("plain"), "plain");
    ground
}

#[test]
fn a_retired_member_never_projects_and_set_show_discloses_the_reason() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let ground = ground_fixture(temp.path());

    let added = aikit(
        &home,
        &project,
        &[
            "source",
            "add-directory",
            "ground",
            ground.to_str().unwrap(),
            "--control-ground",
        ],
    );
    assert_eq!(data(&added)["control_ground"], Value::Bool(true));

    let synced = aikit(&home, &project, &["source", "sync", "ground"]);
    assert_eq!(data(&synced)["skills"], 3);

    aikit(&home, &project, &["source", "promote", "ground", "--trust"]);

    let shown = aikit(&home, &project, &["source", "show", "ground"]);
    assert_eq!(data(&shown)["control_ground"], Value::Bool(true));
    assert_eq!(data(&shown)["active_skills"], 3);
    assert_eq!(
        data(&shown)["active_retired_skills"],
        1,
        "the source discloses how many of its skills stand retired: {}",
        data(&shown)
    );

    // All three are members; all three are enabled and trusted. The retired one
    // still never projects — retirement is a ground fact, not a scope decision.
    fs::create_dir_all(project.join(".aikit")).unwrap();
    fs::write(
        project.join(".aikit/profile.toml"),
        "schema = 1\nenable = [\n  \"skill/ground/keeper\",\n  \"skill/ground/archived\",\n  \"skill/ground/plain\",\n]\n",
    )
    .unwrap();

    aikit(
        &home,
        &project,
        &[
            "set",
            "create",
            "control",
            "skill/ground/keeper",
            "skill/ground/archived",
            "skill/ground/plain",
        ],
    );
    let set = aikit(&home, &project, &["set", "show", "control"]);
    assert_eq!(data(&set)["members"], 3);
    assert_eq!(data(&set)["projected"].as_array().unwrap().len(), 2);
    assert_eq!(data(&set)["complete"], Value::Bool(false));

    let withheld = data(&set)["withheld"].as_array().unwrap();
    assert_eq!(withheld.len(), 1, "withheld, not dropped: {set}");
    assert_eq!(withheld[0]["capability"], "skill/ground/archived");
    let reason = withheld[0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("retired"),
        "the reply says retired, not missing, not error: {reason}"
    );
    assert!(
        reason.contains("Misroutes more than it helps."),
        "and quotes the owner's retirement reason verbatim: {reason}"
    );
    assert!(reason.contains("withheld-retired"));
    aikit(
        &home,
        &project,
        &[
            "project",
            "bind",
            "control",
            "--directory",
            project.to_str().unwrap(),
            "--set",
            "control",
        ],
    );
    let applied = aikit(&home, &project, &["apply"]);
    let current = home
        .join("state/contexts")
        .join(applied["context"]["context_id"].as_str().unwrap())
        .join("current");
    for relative in [
        "projections/codex/.agents/skills",
        "projections/claude/.claude/skills",
    ] {
        let projection = current.join(relative);
        let names: std::collections::BTreeSet<_> = fs::read_dir(&projection)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let mut expected: std::collections::BTreeSet<String> =
            ["keeper".to_string(), "plain".to_string()].into();
        // Claude receives the actor seed in its private generation. Codex's
        // shared project discovery path deliberately withholds that seed.
        if relative.contains("claude") {
            expected.insert("aikit-context".to_string());
        }
        assert_eq!(names, expected, "{relative}");
        for name in ["keeper", "plain"] {
            assert_eq!(
                fs::read(projection.join(name).join("SKILL.md")).unwrap(),
                fs::read(ground.join(name).join("SKILL.md")).unwrap()
            );
        }
        assert!(!projection.join("archived").exists());
    }

    // Restoration is the sibling's owner op; for ai-kit it is one more standing
    // change on the ground: re-sync, re-promote, project again.
    let first_digest = data(&shown)["active_snapshot"]
        .as_str()
        .unwrap()
        .to_string();
    write_manifest(
        &ground.join("archived"),
        &active_manifest("archived", "control-machine"),
    );
    aikit(&home, &project, &["source", "sync", "ground"]);
    let shown_again = aikit(&home, &project, &["source", "show", "ground"]);
    assert_ne!(
        data(&shown_again)["candidate_snapshot"],
        first_digest,
        "a standing change is a new snapshot, never a no-op on the promoted one"
    );
    aikit(&home, &project, &["source", "promote", "ground", "--trust"]);

    let restored = aikit(&home, &project, &["set", "show", "control"]);
    assert_eq!(
        data(&restored)["projected"].as_array().unwrap().len(),
        3,
        "restored standing projects again, through the same set membership: {restored}"
    );
    assert_eq!(data(&restored)["complete"], Value::Bool(true));
    let restored_apply = aikit(&home, &project, &["apply"]);
    assert!(
        current
            .join("projections/codex/.agents/skills/archived/SKILL.md")
            .is_file(),
        "restored preview: {restored}; applied: {restored_apply}; original: {applied}; current: {}",
        current.display()
    );
    assert_eq!(
        fs::read(current.join("projections/codex/.agents/skills/archived/SKILL.md")).unwrap(),
        fs::read(ground.join("archived/SKILL.md")).unwrap()
    );
    // Retiring a previously projected member must remove it from the next generation.
    write_manifest(
        &ground.join("archived"),
        &retired_manifest("archived", "control-machine", "Retired after restoration."),
    );
    aikit(&home, &project, &["source", "sync", "ground"]);
    aikit(&home, &project, &["source", "promote", "ground", "--trust"]);
    aikit(&home, &project, &["apply"]);
    for relative in [
        "projections/codex/.agents/skills/archived",
        "projections/claude/.claude/skills/archived",
    ] {
        assert!(!current.join(relative).exists());
    }
}

#[test]
fn contract_violations_are_refused_at_sync_naming_the_file() {
    let cases: [(&str, String); 4] = [
        (
            "wrong schema",
            r#"{"schema":"central.skill/v2","name":"keeper","scope":"control-machine","standing":"active"}"#.to_string(),
        ),
        (
            "retired without a retirement record",
            r#"{"schema":"central.skill/v1","name":"keeper","scope":"control-machine","standing":"retired"}"#.to_string(),
        ),
        (
            "unresolvable standing",
            r#"{"schema":"central.skill/v1","name":"keeper","scope":"control-machine","standing":"unresolved"}"#.to_string(),
        ),
        (
            "name does not match the directory",
            r#"{"schema":"central.skill/v1","name":"other","scope":"control-machine","standing":"active"}"#.to_string(),
        ),
    ];

    for (label, body) in cases {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("aikit-home");
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let ground = temp.path().join("ground");
        write_skill(&ground.join("keeper"), "keeper");
        write_manifest(&ground.join("keeper"), &body);

        aikit(
            &home,
            &project,
            &[
                "source",
                "add-directory",
                "ground",
                ground.to_str().unwrap(),
                "--control-ground",
            ],
        );
        let output = Command::cargo_bin("aikit")
            .unwrap()
            .env("AIKIT_HOME", &home)
            .env("HOME", home.join("user-home"))
            .env("AIKIT_CONTEXT_ID", "ctx_K2GROUND00000000000000")
            .current_dir(&project)
            .arg("--json")
            .args(["source", "sync", "ground"])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{label}: a contract violation must refuse the sync"
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            text.contains("skill.json"),
            "{label}: the refusal names the file: {text}"
        );
    }
}

#[test]
fn a_source_that_never_opted_in_ignores_the_contract_entirely() {
    // Backward compatibility: an ordinary directory source never opens
    // skill.json. A would-be retirement there is just payload bytes, and the
    // skill projects like any other — AIKit must not read a neighbour's
    // contract into a source that did not declare it.
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let ground = temp.path().join("skills");
    write_skill(&ground.join("archived"), "archived");
    write_manifest(
        &ground.join("archived"),
        &retired_manifest("archived", "control-machine", "Irrelevant here."),
    );

    let added = aikit(
        &home,
        &project,
        &["source", "add-directory", "plain", ground.to_str().unwrap()],
    );
    assert_eq!(data(&added)["control_ground"], Value::Bool(false));
    aikit(&home, &project, &["source", "sync", "plain"]);
    aikit(&home, &project, &["source", "promote", "plain", "--trust"]);

    let shown = aikit(&home, &project, &["source", "show", "plain"]);
    assert_eq!(data(&shown)["active_retired_skills"], 0);

    fs::create_dir_all(project.join(".aikit")).unwrap();
    fs::write(
        project.join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"skill/plain/archived\"]\n",
    )
    .unwrap();
    aikit(
        &home,
        &project,
        &["set", "create", "plain", "skill/plain/archived"],
    );
    let set = aikit(&home, &project, &["set", "show", "plain"]);
    assert_eq!(
        data(&set)["projected"][0],
        "skill/plain/archived",
        "an ordinary source projects its skill regardless of any skill.json"
    );
}

#[test]
fn a_projectcentral_ground_source_routes_per_project_through_existing_bindings() {
    // The project scope registers exactly like the machine scopes. Whether its
    // skills project is decided by the enable scoping that already exists —
    // bound here, absent there. No new routing mechanism.
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_b).unwrap();
    let ground = project_a.join("ProjectCentral/user/skills");
    write_skill(&ground.join("project-skill"), "project-skill");
    write_manifest(
        &ground.join("project-skill"),
        &active_manifest("project-skill", "projectcentral-user"),
    );

    aikit(
        &home,
        &project_a,
        &[
            "source",
            "add-directory",
            "projground",
            ground.to_str().unwrap(),
            "--control-ground",
        ],
    );
    aikit(&home, &project_a, &["source", "sync", "projground"]);
    aikit(
        &home,
        &project_a,
        &["source", "promote", "projground", "--trust"],
    );

    // The project's own scope enables its skill: inside the project root it
    // projects. The binding is the ordinary one every project uses.
    run(
        &home,
        &project_a,
        &[
            "project",
            "bind",
            "fixture-a",
            "--directory",
            project_a.to_str().unwrap(),
        ],
    );
    run(
        &home,
        &project_a,
        &[
            "enable",
            "skill/projground/project-skill",
            "--scope",
            "project",
        ],
    );
    aikit(
        &home,
        &project_a,
        &[
            "set",
            "create",
            "project-scope",
            "skill/projground/project-skill",
        ],
    );
    let inside = aikit(&home, &project_a, &["set", "show", "project-scope"]);
    assert_eq!(
        data(&inside)["projected"][0],
        "skill/projground/project-skill",
        "inside the project root the ProjectCentral skill projects: {inside}"
    );
    aikit(
        &home,
        &project_a,
        &[
            "project",
            "bind",
            "fixture-a",
            "--directory",
            project_a.to_str().unwrap(),
            "--set",
            "project-scope",
        ],
    );
    let applied = aikit(&home, &project_a, &["apply"]);
    let current = home
        .join("state/contexts")
        .join(applied["context"]["context_id"].as_str().unwrap())
        .join("current");
    for relative in [
        "projections/codex/.agents/skills",
        "projections/claude/.claude/skills",
    ] {
        assert_eq!(
            fs::read(current.join(relative).join("project-skill/SKILL.md")).unwrap(),
            fs::read(ground.join("project-skill/SKILL.md")).unwrap()
        );
    }

    // Elsewhere the same set asks for the same member and is told, in the
    // resolver's own words, that no scope enables it there. The set is shared;
    // only the answer is contextual.
    let outside = aikit(&home, &project_b, &["set", "show", "project-scope"]);
    assert!(
        data(&outside)["projected"].as_array().unwrap().is_empty(),
        "outside the project root nothing projects: {outside}"
    );
    let reason = data(&outside)["withheld"][0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("no scope enables it"),
        "eligibility follows the existing enable scoping, nothing new: {reason}"
    );
    let applied = aikit(&home, &project_b, &["apply"]);
    let current = home
        .join("state/contexts")
        .join(applied["context"]["context_id"].as_str().unwrap())
        .join("current");
    for relative in [
        "projections/codex/.agents/skills",
        "projections/claude/.claude/skills",
    ] {
        assert!(!current.join(relative).join("project-skill").exists());
    }
    assert!(!project_b.join(".agents/skills/project-skill").exists());
}
