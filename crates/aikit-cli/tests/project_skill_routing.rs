use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn skill(root: &Path, name: &str) {
    fs::create_dir_all(root.join("references")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Real routed skill.\n---\n\n{name}\n"),
    )
    .unwrap();
    fs::write(root.join("references/proof.md"), format!("{name}-proof\n")).unwrap();
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .env("AIKIT_HOME", home)
        .env("HOME", home.join("user-home"))
        .env("AIKIT_CONTEXT_ID", "ctx_REALTMUX00000000000000")
        .env("AIKIT_ISOLATION", "worktree")
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

fn git(cwd: &Path, args: &[&str]) {
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
}

#[test]
fn repository_identity_preserves_host_and_the_full_nested_namespace() {
    let one = aikit_cli::projects::normalize_repository_identity(
        "https://gitlab.one/group/subgroup/agent-platform.git",
    )
    .unwrap();
    let two = aikit_cli::projects::normalize_repository_identity(
        "git@gitlab.two:group/subgroup/agent-platform.git",
    )
    .unwrap();

    assert_eq!(one, "gitlab.one/group/subgroup/agent-platform");
    assert_eq!(two, "gitlab.two/group/subgroup/agent-platform");
    assert_ne!(one, two, "repository identity must never discard the host");
}

#[test]
fn project_binding_routes_only_selected_sets_to_codex_and_claude() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let source = temp.path().join("skills");
    let project_a = temp.path().join("project-a");
    let project_b = temp.path().join("project-b");
    fs::create_dir_all(&project_a).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    skill(&source.join("wayfinder"), "wayfinder");
    skill(&source.join("grilling"), "grilling");

    run(
        &home,
        &project_a,
        &[
            "source",
            "add-directory",
            "mattpocock",
            source.to_str().unwrap(),
        ],
    );
    run(&home, &project_a, &["source", "sync", "mattpocock"]);
    run(
        &home,
        &project_a,
        &["source", "promote", "mattpocock", "--trust"],
    );
    run(
        &home,
        &project_a,
        &["set", "create", "foundation", "skill/mattpocock/wayfinder"],
    );

    let binding = run(
        &home,
        &project_a,
        &[
            "project",
            "bind",
            "demo",
            "--directory",
            project_a.to_str().unwrap(),
            "--set",
            "foundation",
        ],
    );
    assert_eq!(binding["data"]["project"], "demo");

    run(
        &home,
        &project_a,
        &["enable", "skill/mattpocock/wayfinder", "--scope", "project"],
    );
    let applied = run(
        &home,
        &project_a,
        &["enable", "skill/mattpocock/grilling", "--scope", "project"],
    );
    assert_eq!(
        applied["context"]["project_root"],
        project_a.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    let context = applied["context"]["context_id"].as_str().unwrap();
    let current = home.join("state/contexts").join(context).join("current");
    let codex = current.join("projections/codex/.agents/skills");
    let claude = current.join("projections/claude/.claude/skills");

    assert!(codex.join("wayfinder/SKILL.md").is_file());
    assert!(claude.join("wayfinder/SKILL.md").is_file());
    assert!(!codex.join("grilling").exists());
    assert!(!claude.join("grilling").exists());
    assert_eq!(
        fs::read_to_string(codex.join("wayfinder/references/proof.md")).unwrap(),
        "wayfinder-proof\n"
    );
    assert!(
        project_a
            .join(".agents/skills/wayfinder/SKILL.md")
            .is_file(),
        "Codex must receive the routed set through its native upward-discovery path"
    );
    assert!(!project_a.join(".agents/skills/grilling").exists());

    let client_status = run(&home, &project_a, &["client", "status", "codex"]);
    assert_eq!(
        client_status["data"]["clients"][0]["items"], 1,
        "client status must describe the Skill Set-filtered projection, not the wider active view"
    );

    let launch = run(&home, &project_a, &["client", "launch", "claude"]);
    let command = launch["data"]["command"].as_array().unwrap();
    assert_eq!(command[0], "claude");
    assert_eq!(command[1], "--add-dir");
    assert_eq!(
        PathBuf::from(command[2].as_str().unwrap()),
        current.join("projections/claude")
    );
    assert!(
        PathBuf::from(command[2].as_str().unwrap()).is_dir(),
        "Claude must be launched against the stable, materialized current projection"
    );

    let other = run(&home, &project_b, &["status"]);
    assert_eq!(other["context"]["project_root"], Value::Null);
    assert_eq!(other["data"]["active"].as_array().unwrap().len(), 0);

    let shown = run(&home, &project_a, &["project", "show"]);
    assert_eq!(shown["data"]["project"], "demo");
    assert_eq!(shown["data"]["skill_sets"][0], "foundation");
    assert_eq!(
        PathBuf::from(shown["data"]["root"].as_str().unwrap()),
        project_a.canonicalize().unwrap()
    );

    run(
        &home,
        &project_a,
        &[
            "project",
            "bind",
            "demo",
            "--directory",
            project_a.to_str().unwrap(),
            "--no-default-skill-sets",
        ],
    );
    run(&home, &project_a, &["apply"]);
    assert!(
        !project_a.join(".agents/skills/wayfinder").exists(),
        "an explicit empty routing set must clear the previous Codex projection"
    );
    let empty_current = home.join("state/contexts/ctx_REALTMUX00000000000000/current");
    assert!(
        !empty_current
            .join("projections/claude/.claude/skills/wayfinder")
            .exists(),
        "a project that opts out of defaults must not leak active skills into Claude"
    );

    run(
        &home,
        &project_b,
        &[
            "project",
            "bind",
            "demo",
            "--directory",
            project_b.to_str().unwrap(),
            "--no-default-skill-sets",
        ],
    );
    assert!(
        fs::symlink_metadata(project_a.join(".agents/skills")).is_err(),
        "reassigning a Project Specification must remove its AIKit-owned link from the old directory"
    );
}

#[test]
fn repository_binding_matches_transport_independently_and_uses_the_git_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let repo = temp.path().join("clone");
    let nested = repo.join("packages/app");
    fs::create_dir_all(&nested).unwrap();
    git(&repo, &["init", "--quiet"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:Acme/Agent-Platform.git",
        ],
    );

    run(
        &home,
        &nested,
        &[
            "project",
            "bind",
            "agent-platform",
            "--repository",
            "https://github.com/acme/agent-platform.git",
            "--set",
            "repository-foundation",
        ],
    );
    run(
        &home,
        &nested,
        &[
            "project",
            "bind",
            "local-specialization",
            "--directory",
            repo.to_str().unwrap(),
            "--set",
            "directory-writing",
        ],
    );
    let shown = run(&home, &nested, &["project", "show"]);
    assert_eq!(shown["data"]["project"], "local-specialization");
    assert_eq!(shown["data"]["matched_by"], "directory");
    assert_eq!(
        shown["data"]["skill_sets"],
        serde_json::json!(["repository-foundation", "directory-writing"]),
        "less-specific repository defaults must compose with the directory specialization"
    );
    assert_eq!(
        PathBuf::from(shown["data"]["root"].as_str().unwrap()),
        repo.canonicalize().unwrap()
    );
}

#[test]
fn configurable_default_skill_sets_are_inherited_unless_a_project_opts_out() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("aikit-home");
    let inherited = temp.path().join("inherited");
    let inherited_again = temp.path().join("inherited-again");
    let isolated = temp.path().join("isolated");
    fs::create_dir_all(&inherited).unwrap();
    fs::create_dir_all(&inherited_again).unwrap();
    fs::create_dir_all(&isolated).unwrap();

    run(
        &home,
        &inherited,
        &[
            "project",
            "defaults",
            "--set",
            "foundation",
            "--set",
            "writing",
            "--set",
            "foundation",
        ],
    );
    run(
        &home,
        &inherited,
        &[
            "project",
            "bind",
            "inherits",
            "--directory",
            inherited.to_str().unwrap(),
        ],
    );
    run(
        &home,
        &inherited_again,
        &[
            "project",
            "bind",
            "inherits-again",
            "--directory",
            inherited_again.to_str().unwrap(),
        ],
    );
    run(
        &home,
        &isolated,
        &[
            "project",
            "bind",
            "isolated",
            "--directory",
            isolated.to_str().unwrap(),
            "--no-default-skill-sets",
        ],
    );

    let inherited_show = run(&home, &inherited, &["project", "show"]);
    assert_eq!(
        inherited_show["data"]["skill_sets"],
        serde_json::json!(["foundation", "writing"])
    );
    let inherited_again_show = run(&home, &inherited_again, &["project", "show"]);
    assert_eq!(
        inherited_again_show["data"]["skill_sets"],
        serde_json::json!(["foundation", "writing"]),
        "the same reusable sets must compose into more than one project"
    );
    let isolated_show = run(&home, &isolated, &["project", "show"]);
    assert_eq!(isolated_show["data"]["skill_sets"], serde_json::json!([]));
}
