//! Real git and native Workcell lifecycle, entirely inside disposable state.
//! The adapter reads the owner-created ledger; no registry JSON is fabricated.
use aikit_adapters::{runner::SystemRunner, workcell_run_intake};
use aikit_core::resource::{
    parse_or_search_expression, resolve_expression, MemoryResourceIndex, ResourceKind,
};
use serde_json::Value;
use std::{path::Path, process::Command, time::Duration};

fn command(binary: &str, args: &[&str], cwd: &Path) -> Value {
    let output = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_material_run_enters_the_existing_operator_field_without_inventing_an_agency() {
    let Some(binary) = std::env::var_os("AIKIT_TEST_WORKCELL_BIN") else {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_NATIVE_WORKCELL").is_none(),
            "AIKIT_TEST_WORKCELL_BIN is required"
        );
        eprintln!("native Workcell intake test requires AIKIT_TEST_WORKCELL_BIN");
        return;
    };
    let binary = binary.to_str().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let repo = temporary.path().join("repo");
    let state = temporary.path().join("workcell");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Native intake regression"]);
    git(
        &repo,
        &["config", "user.email", "native-intake@example.invalid"],
    );
    std::fs::write(repo.join("source.txt"), "native Workcell intake\n").unwrap();
    git(&repo, &["add", "source.txt"]);
    git(&repo, &["commit", "-m", "Initial native intake source"]);
    let started = command(
        binary,
        &[
            "--state-root",
            state.to_str().unwrap(),
            "--json",
            "--workspace-source",
            repo.to_str().unwrap(),
            "run",
            "start",
            "--run",
            "native-intake",
            "--extension",
            "branch_law=aikit",
            "--workspace",
            "writable",
        ],
        &repo,
    );
    assert_eq!(started["ok"], true);
    let records = workcell_run_intake::read(
        &SystemRunner::new()
            .with_cwd(&repo)
            .with_env("WORKCELL_HOME", state.display().to_string())
            .with_timeout(Duration::from_secs(5)),
        binary,
    )
    .unwrap();
    assert_eq!(
        records.len(),
        1,
        "an agency-less native run creates only its own Resource"
    );
    assert_eq!(records[0].descriptor.kind, ResourceKind::Run);
    assert_eq!(records[0].descriptor.id.as_str(), "run/native-intake");
    assert_eq!(
        records[0].descriptor.annotations["workcell.demand_ref"],
        started["run"]["demand_ref"].as_str().unwrap()
    );
    let mut index = MemoryResourceIndex::default();
    for record in records {
        index.insert(record);
    }
    let expression = parse_or_search_expression("@ run/native-intake").unwrap();
    let resolved = resolve_expression(&expression, &index, 10);
    assert!(resolved
        .candidates
        .iter()
        .any(|candidate| candidate.resource.as_str() == "run/native-intake"));
    let released = command(
        binary,
        &[
            "--state-root",
            state.to_str().unwrap(),
            "--json",
            "run",
            "release",
            "--run",
            "native-intake",
        ],
        &repo,
    );
    assert_eq!(released["ok"], true);
}
