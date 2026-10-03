//! Running a script capsule is a real subprocess against a real payload on disk,
//! not a mocked command. The capsule here is parsed from the same manifest text a
//! registry would hold, its payload is a genuine shell script, and the assertion
//! is on what that script actually printed.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use aikit_cli::run;
use aikit_core::capsule::{Capsule, ExecMode};
use tempfile::TempDir;

fn native_tempdir() -> TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new().prefix("script-baseline-").tempdir_in(root).unwrap()
}

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
    let tmp = native_tempdir();
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
    let tmp = native_tempdir();
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


#[test]
fn actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status() {
    let root = RetainedFixture::new();
    let mut capsule = script_capsule(root.path(), "#!/bin/sh\nprintf started\nsleep 3\n");
    let text = r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
[script]
entry = "payload/run.sh"
mode = "capture"
timeout = "1s"
"#;
    let payload_root = capsule.root.take();
    capsule = Capsule::from_toml_str(text).unwrap();
    capsule.root = payload_root;
    let plan = run::plan_script(&capsule, &[], None, root.path()).unwrap();
    root.prepare("actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status");
    let outcome = run::execute(&plan);
    let actual = observe_actual_native_outcome("actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status", &outcome);
    root.record_outcome("actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status", &actual);
    let failure = outcome.unwrap_err();
    assert_eq!(failure.code(), "mux.command_timeout", "{failure:?}");
    assert_eq!(failure.details()["execution_started"], "true");
    assert_eq!(failure.details()["direct_child_reaped"], "true");
    assert!(clean_inner_retirement(&actual), "Actual inner retirement is unavailable: {actual}");
    root.verify("actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status");
}

#[test]
fn actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal() {
    let root = RetainedFixture::new();
    let capsule = script_capsule(root.path(),
        "#!/bin/sh\nawk 'BEGIN { for (i=0;i<32768;i++) print \"\" }'\nawk 'BEGIN { for (i=0;i<32769;i++) print \"\" }' >&2\n");
    root.prepare("actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal");
    let outcome = run::execute(&run::plan_script(&capsule, &[], None, root.path()).unwrap());
    let actual = observe_actual_native_outcome("actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal", &outcome);
    root.record_outcome("actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal", &actual);
    let failure = outcome.unwrap_err();
    assert_eq!(failure.code(), "mux.command_output_limit", "{failure:?}");
    assert_eq!(failure.details()["observation_stage"], "line_projection_capacity");
    assert!(clean_inner_retirement(&actual), "Actual inner retirement is unavailable: {actual}");
    root.verify("actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal");
}

// This derives only actual common old/current owner observations. The exact
// expected branches attach every cleanup IO to these structured facts; this is
// not a claim that the old Core API exposes its successor's typed secondary API.
fn observed_io_fields(value: &serde_json::Value) -> serde_json::Value {
    let kind = value["kind"].as_str().expect("Actual native IO kind");
    assert!(kind.len() <= 80 && kind.bytes().all(|b| b.is_ascii_alphanumeric()),
        "Native IO kind must be a bounded scalar, not diagnostic prose");
    let errno = &value["raw_os_error"];
    assert!(errno.is_null() || errno.as_i64().is_some(), "Actual errno must be integer/null");
    serde_json::json!({"kind":kind,"raw_os_error":errno})
}
fn observed_cause(failure: &aikit_core::AikitError, key: &str) -> Option<serde_json::Value> {
    failure.details().get(key).map(|encoded| {
        assert!(encoded.len() <= 4096, "Native cleanup scalar evidence exceeds its profile");
        let value: serde_json::Value = serde_json::from_str(encoded).expect("Actual native cleanup observation");
        observed_io_fields(&value)
    })
}
fn observed_additional_causes(failure: &aikit_core::AikitError) -> Option<Vec<serde_json::Value>> {
    failure.details().get("additional_cleanup_causes").map(|encoded| {
        assert!(encoded.len() <= 4096, "Native cleanup array evidence exceeds its profile");
        let value: serde_json::Value = serde_json::from_str(encoded).expect("Actual native additional cleanup observations");
        value.as_array().expect("Actual cleanup cause array").iter().map(observed_io_fields).collect()
    })
}
fn observed_status(failure: &aikit_core::AikitError, key: &str) -> Option<i32> {
    failure.details().get(key).map(|actual| actual.parse().expect("Actual native integer exit status"))
}
fn observed_eof(failure: &aikit_core::AikitError, key: &str) -> Option<bool> {
    failure.details().get(key).map(|actual| match actual.as_str() {
        "true" => true, "false" => false, _ => panic!("Actual native EOF must be boolean"),
    })
}
fn clean_inner_retirement(actual: &serde_json::Value) -> bool {
    if actual["native_outcome"] != "refused" || actual["execution_started"] != "true"
        || actual["direct_child_reaped"] != "true" || actual["original_source_present"] != false
        || !actual["cleanup_cause"].is_null() || !actual["additional_cleanup_causes"].is_null() {
        return false;
    }
    let known = actual["known_exit_status"].as_i64();
    let cleanup = actual["cleanup_exit_status"].as_i64();
    if known.is_some() == cleanup.is_some() || known.or(cleanup).is_none_or(|status| status < 0) {
        return false;
    }
    match actual["group_signal"].as_str() {
        Some("delivered") => actual["group_absence_cause"].is_null(),
        // Native AlreadyAbsent is a real non-failure observation. It still
        // needs its original cause, successful actual reap and exit status.
        Some("already-absent") => actual["group_absence_cause"].is_object(),
        Some("not-needed") => actual["group_absence_cause"].is_null()
            && actual["stdout_eof"] == true && actual["stderr_eof"] == true,
        _ => false,
    }
}
fn observe_actual_native_outcome(case: &str, outcome: &aikit_core::Result<run::RunReport>) -> serde_json::Value {
    use std::error::Error;
    let actual = match outcome {
        Ok(report) => {
            let known_control_matches = match case {
                "actual_old_api_manifest_deadline_requires_native_timeout_not_completed_status" =>
                    report.output.len() == 1 && report.output[0] == "started",
                "actual_old_api_two_stream_lf_overflow_requires_native_capacity_refusal" =>
                    report.output.len() == 65_537 && report.output.iter().all(String::is_empty),
                "actual_native_spawn_failure_keeps_fixture_across_assertion_unwind" => false,
                _ => panic!("Unknown controlled characterization case"),
            };
            serde_json::json!({"case":case,"native_outcome":"completed",
                "status":report.status,"detached":report.detached,
                "output_rows":report.output.len(),"known_control_matches":known_control_matches})
        }
        Err(failure) => {
            let source = failure.source();
            let original_io = source.and_then(|actual| actual.downcast_ref::<std::io::Error>())
                .map(|actual| serde_json::json!({"kind":format!("{:?}",actual.kind()),"raw_os_error":actual.raw_os_error()}));
            serde_json::json!({"case":case,"native_outcome":"refused","code":failure.code(),
                "execution_started":failure.details().get("execution_started"),
                "direct_child_reaped":failure.details().get("direct_child_reaped"),
                "observation_stage":failure.details().get("observation_stage"),
                "group_signal":failure.details().get("group_signal"),
                "known_exit_status":observed_status(failure,"known_exit_status"),
                "cleanup_exit_status":observed_status(failure,"cleanup_exit_status"),
                "stdout_eof":observed_eof(failure,"stdout_eof"),"stderr_eof":observed_eof(failure,"stderr_eof"),
                "capture_cancelled":observed_eof(failure,"capture_cancelled"),
                "original_source_present":source.is_some(),"original_io":original_io,
                "cleanup_cause":observed_cause(failure,"cleanup_cause"),
                "group_absence_cause":observed_cause(failure,"group_absence_cause"),
                "additional_cleanup_causes":observed_additional_causes(failure),
                "cause_observation_contract":"expected_command_failure_original_and_structured_cleanup/v2",
                "eof_observation_scope":"actual_refusal_snapshot_or_unpublished; not_post_cleanup_EOF",
                "typed_secondary_api_scope":"not_available_on_shared_old_API; exact_expected_branch_attachment_census"})
        }
    };
    eprintln!("AIKIT_CAPTURE_NATIVE_OBSERVATION {actual}");
    actual
}

// Keep custody before an operation. Dropping the held descriptor closes a
// handle; it deliberately never removes this retained native fixture. Failed
// assertions cannot discard material while inner retirement is unconfirmed.
struct RetainedFixture {
    root: std::path::PathBuf,
    held: fs::File,
    tuple: (u64, u64),
}
impl RetainedFixture {
    fn new() -> Self {
        use std::os::unix::fs::MetadataExt;
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&scratch).unwrap();
        let root = tempfile::Builder::new().prefix("characterization-retained-")
            .tempdir_in(scratch).unwrap().keep().canonicalize().unwrap();
        let held = fs::File::open(&root).unwrap();
        let metadata = held.metadata().unwrap();
        assert!(metadata.is_dir());
        Self { root, held, tuple: (metadata.dev(), metadata.ino()) }
    }
    fn path(&self) -> &std::path::Path { &self.root }
    fn material(&self, case: &str) -> serde_json::Value {
        use std::os::unix::fs::MetadataExt;
        let named = fs::symlink_metadata(&self.root).unwrap();
        let held = self.held.metadata().unwrap();
        assert!(named.is_dir() && held.is_dir());
        assert_eq!((named.dev(), named.ino()), self.tuple);
        assert_eq!((held.dev(), held.ino()), self.tuple);
        let payload = self.root.join("payload/run.sh");
        let metadata = fs::symlink_metadata(&payload).unwrap();
        assert!(metadata.is_file() && metadata.nlink() == 1);
        let bytes = fs::read(&payload).unwrap();
        serde_json::json!({"schema":"aikit.characterization-owned-fixture/v2","case":case,
            "root_path":self.root,"source_manifest_dir":env!("CARGO_MANIFEST_DIR"),
            "root_dev":self.tuple.0,"root_ino":self.tuple.1,
            "payload_dev":metadata.dev(),"payload_ino":metadata.ino(),"payload_mode":metadata.mode(),
            "payload_bytes":bytes.len(),"payload_blake3":blake3::hash(&bytes).to_hex().to_string(),
            "custody":"retained before native effect; no automatic cleanup on return/unwind"})
    }
    fn write(&self, name: &str, value: &serde_json::Value) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
            .open(self.root.join(name)).unwrap();
        file.write_all(&serde_json::to_vec_pretty(value).unwrap()).unwrap();
        file.sync_all().unwrap();
        self.held.sync_all().unwrap();
    }
    fn prepare(&self, case: &str) { self.write("characterization-before.json", &self.material(case)); }
    fn verify(&self, case: &str) {
        let before: serde_json::Value = serde_json::from_slice(&fs::read(self.root.join("characterization-before.json")).unwrap()).unwrap();
        assert_eq!(before, self.material(case), "Actual retained native fixture changed");
    }
    fn record_outcome(&self, case: &str, actual: &serde_json::Value) {
        self.verify(case);
        self.write("characterization-outcome.json", actual);
    }
}

#[test]
fn actual_native_spawn_failure_keeps_fixture_across_assertion_unwind() {
    use std::error::Error;
    let case = "actual_native_spawn_failure_keeps_fixture_across_assertion_unwind";
    let root = RetainedFixture::new();
    let capsule = script_capsule(root.path(), "#!/bin/sh\nprintf retained-unselected-control\n");
    let mut plan = run::plan_script(&capsule, &[], None, root.path()).unwrap();
    let missing = root.path().join("genuinely-missing-executable");
    let oracle = fs::File::open(&missing).unwrap_err();
    assert_eq!(oracle.kind(), std::io::ErrorKind::NotFound);
    plan.program = missing.to_str().expect("Owned native fixture coordinate").to_owned();
    root.prepare(case);
    let outcome = run::execute(&plan);
    let actual = observe_actual_native_outcome(case, &outcome);
    root.record_outcome(case, &actual);
    let failure = outcome.as_ref().unwrap_err();
    assert_eq!(failure.code(), "mux.command_spawn_failed");
    assert_eq!(failure.details()["execution_started"], "false");
    let native = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(native.kind(), oracle.kind());
    assert_eq!(native.raw_os_error(), oracle.raw_os_error());
    assert!(!clean_inner_retirement(&actual), "Actual failure must not qualify current positive");
    // This is a genuine failed assertion on the actual native Err, not a
    // synthetic error, fake Output or replacement owner response.
    let retained_path = root.root.clone();
    let retained_tuple = root.tuple;
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let held_fixture = root;
        let report = outcome.unwrap();
        drop(held_fixture);
        report
    }));
    assert!(unwind.is_err());
    // The original fixture owner was moved INTO the actual failing assertion;
    // its descriptor was dropped by unwind. Reopen only that same retained
    // physical object, never create a replacement or cleanup by guessed path.
    let held = fs::File::open(&retained_path).unwrap();
    let root = RetainedFixture { root: retained_path, held, tuple: retained_tuple };
    root.verify(case);
    root.write("characterization-unwind.json", &serde_json::json!({
        "case":case,"actual_assertion_unwind_observed":unwind.is_err(),
        "native_kind":format!("{:?}",oracle.kind()),"native_errno":oracle.raw_os_error(),
        "fixture_verified_after_unwind":true,"scope":"real spawn failure/custody; not forced cleanup syscall failure"}));
}
