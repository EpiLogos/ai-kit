//! Running a script capsule is a real subprocess against a real payload on disk,
//! not a mocked command. The capsule here is parsed from the same manifest text a
//! registry would hold, its payload is a genuine shell script, and the assertion
//! is on what that script actually printed.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use aikit_cli::run;
use aikit_core::capsule::{Capsule, ExecMode};
use tempfile::TempDir;

fn script_capsule(dir: &std::path::Path, body: &str) -> Capsule {
    let payload = dir.join("payload");
    fs::create_dir_all(&payload).unwrap();
    let entry = payload.join("run.sh");
    fs::write(&entry, body).unwrap();
    let mut perms = fs::metadata(&entry).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&entry, perms).unwrap();

    let src = r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "Prints a greeting for the test."

[script]
entry = "payload/run.sh"
mode = "capture"
"#;
    let mut c = Capsule::from_toml_str(src).unwrap();
    c.root = Some(dir.to_path_buf());
    c
}

#[test]
fn a_captured_script_run_returns_the_scripts_real_output_and_status() {
    let tmp = TempDir::new().unwrap();
    let capsule = script_capsule(tmp.path(), "#!/bin/sh\necho \"hello $1\"\nexit 0\n");

    let plan = run::plan_script(&capsule, &["world".to_string()], None, tmp.path()).unwrap();
    assert_eq!(plan.mode, ExecMode::Capture);

    let report = run::execute(&plan).unwrap();
    assert_eq!(report.status, 0);
    let out = report.output.join("\n");
    assert!(out.contains("hello world"), "captured output was: {out:?}");
}

#[test]
fn a_failing_script_reports_its_nonzero_status() {
    let tmp = TempDir::new().unwrap();
    let capsule = script_capsule(tmp.path(), "#!/bin/sh\necho oops 1>&2\nexit 7\n");

    let plan = run::plan_script(&capsule, &[], None, tmp.path()).unwrap();
    let report = run::execute(&plan).unwrap();
    assert_eq!(report.status, 7);
}

#[test]
fn a_capsule_that_is_not_a_script_cannot_be_run() {
    let src = r#"schema = 1
id = "skill/demo/thing"
kind = "skill"
name = "thing"
description = "A skill, which is not runnable."

[skill]
root = "payload"
"#;
    let mut c = Capsule::from_toml_str(src).unwrap();
    c.root = Some(std::env::temp_dir());
    let err = run::plan_script(&c, &[], None, &std::env::temp_dir()).unwrap_err();
    assert_eq!(err.code(), "run.not_runnable");
}
