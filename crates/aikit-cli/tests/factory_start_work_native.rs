//! Native cross-product proof for AIKit's Factory-work entry operation.

use std::{path::PathBuf, process::Command};

use serde_json::Value;

#[test]
#[ignore = "run by the mandatory exact-Factory conformance CI job"]
fn public_cli_starts_and_idempotently_reopens_factory_owned_work() {
    let factory = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_REAL_BIN").expect("AIKIT_FACTORY_REAL_BIN is required"),
    );
    let request = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_COMMISSION_REQUEST")
            .expect("AIKIT_FACTORY_COMMISSION_REQUEST is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("developmental-state.json");

    let run = |request: &std::path::Path| {
        let output = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
            .args([
                "factory",
                "start-work",
                "--state",
                state.to_str().unwrap(),
                "--request-file",
                request.to_str().unwrap(),
                "--factory-bin",
                factory.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "AIKit must emit structured output ({error}); stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, value)
    };

    let (first_output, first) = run(&request);
    assert!(first_output.status.success(), "{first}");
    assert_eq!(first["data"]["contract"], "factory.commission-receipt/v1");
    assert_eq!(first["data"]["status"], "applied");
    assert_eq!(
        first["data"]["commission"]["request"]["rootAct"]["standing"],
        "commissioned-not-executed"
    );
    assert!(state.exists());

    let (replay_output, replay) = run(&request);
    assert!(replay_output.status.success(), "{replay}");
    assert_eq!(replay["data"]["status"], "already-applied");
    assert_eq!(replay["data"]["commission"], first["data"]["commission"]);

    let before = std::fs::read(&state).unwrap();
    let mut conflict: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
    conflict["purpose"] = Value::String("conflicting reuse must be refused".into());
    let conflict_path = directory.path().join("conflicting-request.json");
    std::fs::write(
        &conflict_path,
        serde_json::to_vec_pretty(&conflict).unwrap(),
    )
    .unwrap();
    let (conflict_output, conflict_result) = run(&conflict_path);
    assert!(!conflict_output.status.success(), "{conflict_result}");
    assert_eq!(conflict_result["ok"], false);
    assert_eq!(std::fs::read(&state).unwrap(), before);
}
