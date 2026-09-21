//! Real source/CLI/Service/stdio loop. No model or personal machine is used.
use aikit_cli::{app::Service, wiki_projection as projection};
use aikit_core::catalog::Catalog;
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::{CapsuleId, ContextId, TrustKey, TrustState};
use aikit_store::{AikitHome, TrustStore};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}
fn source(root: &Path) -> PathBuf {
    let file = root.join("ProjectCentral/agents/wiki/projection.md");
    write(
        &file,
        "# Working together\n\nKeep output brief by default.\n",
    );
    file
}
fn revised(file: &Path, body: &str) -> projection::ProjectionReading {
    let base = projection::read(file).unwrap();
    projection::update(
        file,
        &base.revision,
        body,
        "User explicitly requested substantive explanations for research",
    )
    .unwrap()
}
fn scene(root: &Path, selected: bool) -> (PathBuf, PathBuf, Service) {
    let root = root.canonicalize().unwrap();
    let home = root.join("home");
    let project = root.join("project");
    let cap = home.join("registries/personal/capsules/hook/continuity/wiki-projection");
    write(
        &cap.join("manifest.toml"),
        include_str!("../../../registry/capsules/hook/continuity/wiki-projection/manifest.toml"),
    );
    write(&cap.join("payload/wiki-projection"), "#!/bin/sh\nexit 0\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            cap.join("payload/wiki-projection"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let profile = if selected {
        "schema = 1\nenable = [\"hook/continuity/wiki-projection\"]\n[config.\"hook/continuity/wiki-projection\"]\nsources = [\"project:ProjectCentral/agents/wiki/projection.md\"]\n"
    } else {
        "schema = 1\n"
    };
    write(&project.join(".aikit/profile.toml"), profile);
    source(&project);
    let env = BTreeMap::from([(
        "AIKIT_CONTEXT_ID".to_string(),
        ContextId::generate().to_string(),
    )]);
    let mut service =
        Service::open(AikitHome::at(&home), &project, |k| env.get(k).cloned()).unwrap();
    let id = CapsuleId::parse(projection::CAPABILITY).unwrap();
    let capsule = service.snapshot().get(&id).unwrap();
    let key = TrustKey::new(
        capsule.source.clone().unwrap(),
        id,
        capsule.revision.clone().unwrap(),
    );
    TrustStore::new(service.index())
        .record(&key, TrustState::Trusted, Some("test review"))
        .unwrap();
    service.refresh().unwrap();
    (home, project, service)
}
fn prompt(project: &Path) -> HookEvent {
    HookEvent::new(
        "claude",
        HookEventKind::UserPromptSubmit,
        json!({"cwd":project}),
    )
    .in_cwd(project)
}

#[test]
fn correction_changes_the_next_dispatch_without_restarting_service() {
    let temp = tempfile::tempdir().unwrap();
    let (_, project, service) = scene(temp.path(), true);
    let file = project.join("ProjectCentral/agents/wiki/projection.md");
    let governance = project.join("ProjectCentral/agents/governance/style.md");
    write(&governance, "Authoritative human source remains untouched.");
    let before = service.dispatch_hook(&prompt(&project)).unwrap();
    assert!(before.injected_text().contains("Keep output brief"));
    let changed = revised(
        &file,
        "# Working together\n\nFor research, explain the argument fully.\n",
    );
    let after = service.dispatch_hook(&prompt(&project)).unwrap();
    assert!(after.injected_text().contains("explain the argument fully"));
    assert!(after.injected_text().contains(&changed.revision));
    assert!(!after.injected_text().contains("Keep output brief"));
    let wire = aikit_cli::hook::translate_decision("claude", "UserPromptSubmit", false, &after);
    let payload: Value = serde_json::from_str(wire.stdout.as_deref().unwrap()).unwrap();
    assert!(payload["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains(&changed.revision));

    assert_eq!(
        std::fs::read_to_string(governance).unwrap(),
        "Authoritative human source remains untouched."
    );
}

#[test]
fn unchanged_reading_is_not_redelivered_but_a_change_reaches_the_next_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let (_, project, service) = scene(temp.path(), true);
    let file = project.join("ProjectCentral/agents/wiki/projection.md");
    let start = HookEvent::new(
        "claude",
        HookEventKind::SessionStart,
        json!({"cwd": project}),
    )
    .in_cwd(&project);
    // Session start always delivers: the fresh session holds nothing.
    assert!(service
        .dispatch_hook(&start)
        .unwrap()
        .injected_text()
        .contains("Keep output brief"));
    // The same reading at the next prompt is silence, not a repeat.
    assert!(!service
        .dispatch_hook(&prompt(&project))
        .unwrap()
        .injected_text()
        .contains("Keep output brief"));
    // A correction changes the composition, so the next prompt receives it.
    revised(&file, "# Working together\n
For research, explain the argument fully.\n");
    let after = service.dispatch_hook(&prompt(&project)).unwrap();
    assert!(after.injected_text().contains("explain the argument fully"));
    // And it is not repeated again while unchanged.
    assert!(!service
        .dispatch_hook(&prompt(&project))
        .unwrap()
        .injected_text()
        .contains("explain the argument fully"));
}

#[test]
fn a_newly_unavailable_source_is_delivered_not_silently_dropped() {
    let temp = tempfile::tempdir().unwrap();
    let (_, project, service) = scene(temp.path(), true);
    assert!(service
        .dispatch_hook(&prompt(&project))
        .unwrap()
        .injected_text()
        .contains("Keep output brief"));
    std::fs::remove_file(file_of(&project)).unwrap();
    // The reading became unavailable: that change itself reaches the next act
    // as an explicit notice rather than silence or a cached projection.
    let decision = service.dispatch_hook(&prompt(&project)).unwrap();
    assert!(decision
        .injected_text()
        .contains("Do not substitute a cached projection"));
}

fn file_of(project: &Path) -> PathBuf {
    project.join("ProjectCentral/agents/wiki/projection.md")
}

#[test]
fn stale_update_is_refused_and_an_empty_body_clears_explicitly() {
    let temp = tempfile::tempdir().unwrap();
    let file = source(&temp.path().canonicalize().unwrap());
    let first = projection::read(&file).unwrap();
    let second = revised(&file, "new reading");
    let e = projection::update(&file, &first.revision, "stale overwrite", "stale")
        .unwrap_err();
    assert_eq!(e.code(), "wiki_projection.conflict");
    assert_eq!(projection::read(&file).unwrap().revision, second.revision);
    // The source never accumulates embedded provenance: body in, body out.
    let cleared = revised(&file, "");
    assert!(cleared.body.is_empty());
    let raw = std::fs::read_to_string(&file).unwrap();
    assert_eq!(raw, "");
}

#[test]
fn source_is_not_operative_merely_because_it_exists() {
    let temp = tempfile::tempdir().unwrap();
    let (_, project, service) = scene(temp.path(), false);
    assert!(!service
        .dispatch_hook(&prompt(&project))
        .unwrap()
        .injected_text()
        .contains("Keep output brief"));
}

#[test]
fn root_and_project_addresses_do_not_fall_back_to_a_different_workcell() {
    let a = Path::new("/one/Central");
    let b = Path::new("/two/Central");
    let address = "central:Control/agents/wiki/user.md";
    assert_eq!(
        projection::address_path(address, None, Some(a)).unwrap(),
        a.join("Control/agents/wiki/user.md")
    );
    assert_eq!(
        projection::address_path(address, None, Some(b)).unwrap(),
        b.join("Control/agents/wiki/user.md")
    );
    assert!(projection::address_path(address, Some(a), None).is_err());
    assert!(
        projection::address_path("project:../Control/agents/wiki/user.md", Some(a), Some(b))
            .is_err()
    );
    assert!(projection::address_path("project:/tmp/agents/wiki/user.md", Some(a), None).is_err());
}

#[test]
fn sibling_projects_read_their_own_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let a = root.join("a");
    let b = root.join("b");
    let af = source(&a);
    let bf = source(&b);
    revised(&af, "Alpha only");
    revised(&bf, "Beta only");
    let config: toml::value::Table =
        toml::from_str("sources = [\"project:ProjectCentral/agents/wiki/projection.md\"]").unwrap();
    let (blocks, _) = projection::context_blocks(&config, Some(&a), None).unwrap();
    assert!(blocks.join("\n").contains("Alpha only"));
    assert!(!blocks.join("\n").contains("Beta only"));
}

#[test]
fn denied_or_missing_sources_never_reuse_the_old_projection() {
    let temp = tempfile::tempdir().unwrap();
    let (_, project, service) = scene(temp.path(), true);
    let folder = project.join("ProjectCentral/agents/wiki");
    write(&folder.join(".no-agent-retrieval"), "");
    let decision = service.dispatch_hook(&prompt(&project)).unwrap();
    assert!(!decision.injected_text().contains("Keep output brief"));
    assert!(decision.injected_text().contains("wiki_projection.denied"));
    assert!(projection::read(&folder.join("projection.md")).is_err());
    std::fs::remove_file(folder.join(".no-agent-retrieval")).unwrap();
    std::fs::remove_file(folder.join("projection.md")).unwrap();
    let decision = service.dispatch_hook(&prompt(&project)).unwrap();
    assert!(!decision.warnings.is_empty());
    assert!(decision
        .injected_text()
        .contains("Do not substitute a cached projection"));
}

#[test]
fn governance_is_not_an_update_target() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("Control/agents/governance/instructions.md");
    write(&path, "human source");
    assert_eq!(
        projection::read(&path).unwrap_err().code(),
        "wiki_projection.not_agent_wiki"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "human source");
}

#[test]
fn oversized_guidance_is_withheld_whole_not_cut_mid_rule() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = source(&root);
    revised(&file, &"never silently truncate ".repeat(600));
    let config: toml::value::Table =
        toml::from_str("sources = [\"project:ProjectCentral/agents/wiki/projection.md\"]").unwrap();
    let (blocks, warnings) = projection::context_blocks(&config, Some(&root), None).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(!blocks.join("\n").contains("never silently truncate"));
}

#[cfg(unix)]
#[test]
fn symlinked_sources_and_lock_targets_are_refused() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = source(&root);
    let alias = file.with_file_name("alias.md");
    symlink(&file, &alias).unwrap();
    assert_eq!(
        projection::read(&alias).unwrap_err().code(),
        "wiki_projection.alias"
    );
    symlink(&file, file.with_file_name(".projection.md.projection-lock")).unwrap();
    let current = projection::read(&file).unwrap();
    assert_eq!(
        projection::update(&file, &current.revision, "new", "reason")
            .unwrap_err()
            .code(),
        "wiki_projection.alias"
    );
}

#[test]
fn real_cli_update_and_plain_hook_stdout_form_the_same_loop() {
    let temp = tempfile::tempdir().unwrap();
    let (home, project, _) = scene(temp.path(), true);
    let file = project.join("ProjectCentral/agents/wiki/projection.md");
    let before = projection::read(&file).unwrap();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["--json", "wiki", "projection", "update", "--file"])
        .arg(&file)
        .args([
            "--expected-revision",
            &before.revision,
            "--evidence",
            "dialogue:user:17",
            "--actor",
            "agent:guardian",
            "--reason",
            "Explicit correction",
        ])
        .env("AIKIT_HOME", &home)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"For research, give a connected, substantial argument.")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stored: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stored["data"]["state"], "stored");
    assert_eq!(stored["data"]["harness_loaded"], false);
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(["hook", "dispatch", "claude", "UserPromptSubmit"])
        .env("AIKIT_HOME", &home)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json!({"cwd":project}).to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire: Value = serde_json::from_slice(&output.stdout).unwrap();
    let context = wire["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("connected, substantial argument"));
    assert!(context.contains(stored["data"]["projection"]["revision"].as_str().unwrap()));
}

#[test]
fn concurrent_writers_cannot_both_commit_the_same_basis() {
    let temp = tempfile::tempdir().unwrap();
    let file = source(&temp.path().canonicalize().unwrap());
    let revision = projection::read(&file).unwrap().revision;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|body| {
            let file = file.clone();
            let revision = revision.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                projection::update(&file, &revision, body, "correction")
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let winner = results.into_iter().find(|r| r.is_ok()).unwrap().unwrap();
    assert_eq!(projection::read(&file).unwrap().body, winner.body);
}

#[test]
fn unavailable_statuses_share_the_same_total_budget_as_guidance() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = source(&root);
    write(&file, &"guidance ".repeat(850));
    let addresses: Vec<toml::Value> =
        std::iter::once("project:ProjectCentral/agents/wiki/projection.md".to_string())
            .chain((1..8).map(|i| format!("project:ProjectCentral/agents/wiki/missing-{i}.md")))
            .map(toml::Value::String)
            .collect();
    let config =
        toml::value::Table::from_iter([("sources".to_string(), toml::Value::Array(addresses))]);
    let (blocks, warnings) = projection::context_blocks(&config, Some(&root), None).unwrap();
    assert_eq!(blocks.len(), 8);
    assert!(!warnings.is_empty());
    assert!(aikit_core::estimate_tokens(&blocks.join("\n\n")) <= 2048);
    assert!(blocks
        .iter()
        .skip(1)
        .all(|s| s.contains("Do not substitute a cached projection")));
}

#[test]
fn source_address_overflow_is_explicit_not_unbounded_context() {
    let addresses: Vec<_> = (0..8)
        .map(|i| toml::Value::String(format!("project:{}-{i}.md", "x".repeat(4096))))
        .collect();
    let config =
        toml::value::Table::from_iter([("sources".to_string(), toml::Value::Array(addresses))]);
    let error = projection::context_blocks(&config, None, None).unwrap_err();
    assert_eq!(error.code(), "wiki_projection.source_budget");
}

#[test]
fn update_preserves_yaml_frontmatter_and_exact_body_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = source(&root);
    let body = "---\ntags: [guidance]\naliases: [Working together]\n---\n\n# Intent\n\nExplain research fully.\n";
    let changed = revised(&file, body);
    // The file is exactly the body: no appended history, no hidden machinery.
    let raw = std::fs::read_to_string(&file).unwrap();
    assert_eq!(raw, body);
    assert_eq!(projection::read(&file).unwrap().body, body);
    assert_eq!(projection::read(&file).unwrap().revision, changed.revision);
}
