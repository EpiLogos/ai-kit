use std::fs;
use std::process::Command;

use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::runner::SystemRunner;
use aikit_core::knowledge_code::{CodeIndexProvider, GITNEXUS_TESTED_VERSION};
use aikit_core::resource::{SourceRef, SourceRevision};
use tempfile::tempdir;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git fixture command");
    assert!(status.success(), "git fixture command failed: {args:?}");
}

#[test]
fn real_gitnexus_current_cli_indexes_searches_contextualises_impacts_traces_changes_and_checks() {
    let dir = tempdir().expect("temporary GitNexus fixture");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/auth.ts"),
        r#"export function validate(token: string): boolean {
  return token.length > 3;
}

export function login(token: string): boolean {
  return validate(token);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"aikit-gitnexus-fixture"}"#,
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.email=aikit@example.invalid",
            "-c",
            "user.name=AIKit CI",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let revision_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(revision_output.status.success());
    let revision = String::from_utf8(revision_output.stdout).unwrap();

    let mut provider = GitNexusCodeIndexProvider::new(
        SystemRunner::new(),
        "aikit-gitnexus-fixture",
        SourceRef::parse("source:git/aikit-gitnexus-fixture").unwrap(),
        Some(SourceRevision::parse(&format!("git:{}", revision.trim())).unwrap()),
    );
    let before = provider.status();
    if !before.available {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_GITNEXUS_REAL").is_none(),
            "AIKIT_REQUIRE_GITNEXUS_REAL is set but GitNexus is unavailable: {}",
            before.detail
        );
        return;
    }
    assert_eq!(before.version.as_deref(), Some(GITNEXUS_TESTED_VERSION));
    assert!(before.capabilities.index);
    assert!(before.capabilities.search);
    assert!(before.capabilities.context);
    assert!(before.capabilities.impact);
    assert!(before.capabilities.trace);
    assert!(before.capabilities.detect_changes);
    assert!(before.capabilities.structural_check);
    assert!(before.capabilities.cypher);
    assert!(!before.capabilities.structured_output);

    let indexed = provider.index(root, true).expect("real GitNexus analyze");
    assert!(indexed.indexed);

    let login_hits = provider.search("login", 10).expect("real GitNexus query");
    let login = login_hits
        .iter()
        .find(|hit| hit.reference.symbol.as_deref() == Some("login"))
        .expect("query must expose canonical login code reference")
        .reference
        .clone();
    assert_eq!(login.source.as_str(), "source:git/aikit-gitnexus-fixture");
    assert!(login.path.ends_with("src/auth.ts"));

    let validate_hits = provider
        .search("validate", 10)
        .expect("real GitNexus validate query");
    let validate = validate_hits
        .iter()
        .find(|hit| hit.reference.symbol.as_deref() == Some("validate"))
        .expect("query must expose canonical validate code reference")
        .reference
        .clone();

    let context = provider.context(&login).expect("real GitNexus context");
    assert!(context.detail.is_object());
    let impact = provider
        .impact(&validate, "upstream")
        .expect("real GitNexus impact");
    assert!(impact.detail.is_object());
    let trace = provider
        .trace(&login, &validate)
        .expect("real GitNexus trace");
    assert!(trace.detail.is_object());

    fs::write(
        root.join("src/auth.ts"),
        r#"export function validate(token: string): boolean {
  return token.length > 3;
}

export function login(token: string): boolean {
  return validate(token);
}

export function logout(): boolean {
  return true;
}
"#,
    )
    .unwrap();
    let changes = provider
        .detect_changes("unstaged", None)
        .expect("real GitNexus detect-changes");
    assert!(changes.detail.is_string());
    assert!(!changes.detail.as_str().unwrap_or_default().trim().is_empty());
    let structural = provider
        .structural_check()
        .expect("real GitNexus cycle check");
    assert!(structural.detail.is_object());
}
