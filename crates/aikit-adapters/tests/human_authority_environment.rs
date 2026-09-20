//! Actual process owner from an isolated test subprocess: no environment races
//! with other tests, and no real credential or provider account is used.
use aikit_adapters::connection_process::{ConnectionProcess,ModelEnvironment};
use std::process::Command;
#[test]
#[cfg(unix)]
fn human_acceptance_authority_never_reaches_an_unscoped_harness() {
    const MARKER:&str="AIKIT_AUTHORITY_CHILD_TEST";
    if std::env::var_os(MARKER).is_none() {
        let output=Command::new(std::env::current_exe().unwrap()).args(["--exact","human_acceptance_authority_never_reaches_an_unscoped_harness","--nocapture"])
            .env(MARKER,"controlled").env("CENTRAL_NATIVE_TOKEN","CONTROLLED_NOT_A_REAL_CREDENTIAL").output().unwrap();
        assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stdout));
        return;
    }
    let argv=vec!["/bin/sh".into(),"-c".into(),"if [ -n \"${CENTRAL_NATIVE_TOKEN:-}\" ]; then echo '{\"human_token_present\":true}'; else echo '{\"human_token_present\":false}'; fi".into()];
    let mut child=ConnectionProcess::spawn(&argv,None).unwrap();assert_eq!(child.read_json().unwrap()["human_token_present"],false);
    let (_,mut reader,control)=ConnectionProcess::spawn_split_with_environment(&argv,None,Some(&ModelEnvironment::new())).unwrap();
    assert_eq!(reader.read_json().unwrap()["human_token_present"],false);
    drop(control);
}
