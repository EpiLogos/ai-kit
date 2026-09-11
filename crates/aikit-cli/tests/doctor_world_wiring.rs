//! The wiring proof: the CLI backend runs the installation-health checks and
//! projects them as an owned disclosure the TUI can render, so the System pane
//! discloses real health where before `doctor` was absent from the TUI
//! entirely. Driven against a real home, not a fixture.

use aikit_cli::app::Service;
use aikit_core::doctor_world::{DoctorKnowledge, DoctorSeverity};
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use std::collections::BTreeMap;
use std::process::Command;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git is available in the test environment");
    assert!(status.success(), "git {args:?} failed");
}

fn project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join(".aikit")).unwrap();
    std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
    std::fs::write(
        root.join("ProjectCentral/project.json"),
        r#"{"schema":"central.project/v1","project_id":"project:probe","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
    )
    .unwrap();
    git(root, &["init", "--initial-branch=trunk"]);
    git(root, &["config", "user.email", "probe@example.invalid"]);
    git(root, &["config", "user.name", "probe"]);
    std::fs::write(root.join("README.md"), "probe\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "first"]);
}

fn service(home: &std::path::Path, root: &std::path::Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
}

#[test]
fn the_cli_backend_runs_the_health_checks_and_projects_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);

    let service = service(tmp.path(), &root);
    let disclosure = service
        .doctor_world()
        .expect("running the checks does not fail")
        .expect("a producer is attached — this is the wiring the TUI lacked");

    // The checks were run: an observed reading, not the `not_attempted` default.
    assert!(
        matches!(disclosure.knowledge, DoctorKnowledge::Observed { .. }),
        "the checks ran and produced an observed reading"
    );
    let findings = disclosure.findings().expect("observed");
    assert!(
        !findings.is_empty(),
        "the checks always report at least informational notes"
    );

    // Severity counts are answerable (not `None`) once the checks have run —
    // "zero errors observed" is a real fact, distinct from "not looked for".
    assert!(disclosure.count(DoctorSeverity::Error).is_some());

    // The gateway is one of the checks doctor runs, so surfacing doctor is also
    // how the gateway becomes visible in the TUI. On unix the `gateway.service`
    // check is always emitted.
    #[cfg(unix)]
    assert!(
        findings.iter().any(|f| f.check == "gateway.service"),
        "the gateway check is present so the System pane can answer the gateway row: {:?}",
        findings.iter().map(|f| &f.check).collect::<Vec<_>>()
    );

    // The native credential provider is probed too.
    assert!(
        findings.iter().any(|f| f.check == "credential.native-provider"),
        "the native credential provider check is present"
    );
}

#[test]
fn the_health_reading_is_cached_across_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);

    let service = service(tmp.path(), &root);
    let first = service.doctor_world().unwrap().unwrap();
    let second = service.doctor_world().unwrap().unwrap();
    // The checks spawn actuation several times; the reading must be reused, not
    // re-run, behind every world-changing action.
    assert_eq!(first, second);
}
