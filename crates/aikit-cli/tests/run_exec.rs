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
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("script-capture-")
        .tempdir_in(root)
        .unwrap()
}

fn script_capsule(dir: &std::path::Path, body: &str) -> Capsule {
    script_capsule_with_config(dir, body, "")
}

fn script_capsule_with_config(dir: &std::path::Path, body: &str, config: &str) -> Capsule {
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
    let mut c = Capsule::from_toml_str(&format!("{src}{config}")).unwrap();
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
fn actual_capture_preserves_both_streams_line_basis_and_shared_completed_result() {
    use std::sync::Arc;
    let tmp = native_tempdir();
    let capsule = script_capsule(
        tmp.path(),
        "#!/bin/sh\nprintf 'first\\r\\n\\nlast'\nprintf 'error-tail' >&2\nexit 7\n",
    );
    let plan = run::plan_script(&capsule, &[], None, tmp.path()).unwrap();
    let report = run::execute(&plan).unwrap();
    let captured = report.captured.as_ref().unwrap();
    assert_eq!(captured.status, 7);
    assert_eq!(captured.stdout, "first\r\n\nlast");
    assert_eq!(captured.stderr, "error-tail");
    assert_eq!(report.output, ["first", "", "last", "error-tail"]);
    let cloned = report.clone();
    assert!(Arc::ptr_eq(captured, cloned.captured.as_ref().unwrap()));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    run::emit_report_to(&report, &mut stdout, &mut stderr).unwrap();
    assert_eq!(stdout, captured.stdout.as_bytes());
    assert_eq!(stderr, captured.stderr.as_bytes());
    assert_eq!(
        run::method_output_digest(&report),
        blake3::hash(report.output.join("\n").as_bytes())
            .to_hex()
            .to_string()
    );
    let text_bytes: usize = report.output.iter().map(String::len).sum();
    assert!(text_bytes <= captured.stdout.len() + captured.stderr.len());
    assert!(report.output.len() <= 65_538);
}

#[test]
fn actual_manifest_capture_deadline_reaches_the_same_native_owner() {
    let tmp = native_tempdir();
    let capsule = script_capsule_with_config(
        tmp.path(),
        "#!/bin/sh\nprintf 'started'\nsleep 30\n",
        "timeout = \"1s\"\n",
    );
    let plan = run::plan_script(&capsule, &[], None, tmp.path()).unwrap();
    assert_eq!(plan.timeout, Some(std::time::Duration::from_secs(1)));
    let start = std::time::Instant::now();
    let failure = run::execute(&plan).unwrap_err();
    assert_eq!(failure.code(), "mux.command_timeout", "{failure:?}");
    assert_eq!(failure.details()["execution_started"], "true");
    assert_eq!(failure.details()["automatic_retry"], "false");
    assert_eq!(failure.details()["effects"], "unknown");
    assert_eq!(failure.details()["direct_child_reaped"], "true");
    assert_eq!(
        failure.details()["captured_stdout_bytes"],
        "started".len().to_string()
    );
    assert!(!failure.details().contains_key("captured_stdout"));
    assert!(start.elapsed() < std::time::Duration::from_secs(6));
}

#[test]
fn actual_capture_lf_boundary_is_shared_across_both_native_streams() {
    let tmp = native_tempdir();
    let capsule = script_capsule(tmp.path(),
        "#!/bin/sh\nawk 'BEGIN { for (i=0;i<32768;i++) print \"\" }'\nawk 'BEGIN { for (i=0;i<32768;i++) print \"\" }' >&2\n");
    let report = run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap();
    let captured = report.captured.as_ref().unwrap();
    assert_eq!(
        captured.stdout.bytes().filter(|b| *b == b'\n').count(),
        32_768
    );
    assert_eq!(
        captured.stderr.bytes().filter(|b| *b == b'\n').count(),
        32_768
    );
    assert_eq!(report.output.len(), 65_536);
    assert!(report.output.iter().all(String::is_empty));

    let capsule = script_capsule(tmp.path(),
        "#!/bin/sh\nawk 'BEGIN { for (i=0;i<32768;i++) print \"\" }'\nawk 'BEGIN { for (i=0;i<32769;i++) print \"\" }' >&2\n");
    let failure =
        run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap_err();
    assert_eq!(failure.code(), "mux.command_output_limit", "{failure:?}");
    assert_eq!(
        failure.details()["observation_stage"],
        "line_projection_capacity"
    );
    assert_eq!(failure.details()["line_feed_limit"], "65536");
    assert!(
        failure.details()["observed_line_feeds"]
            .parse::<usize>()
            .unwrap()
            > 65_536
    );
    assert_eq!(failure.details()["execution_started"], "true");
    assert_eq!(failure.details()["automatic_retry"], "false");
    let retained_bytes = failure.details()["captured_stdout_bytes"]
        .parse::<usize>()
        .unwrap()
        + failure.details()["captured_stderr_bytes"]
            .parse::<usize>()
            .unwrap();
    assert!(
        retained_bytes <= 65_536,
        "both streams contain only LF bytes in this real child"
    );
    assert!(!failure.details().contains_key("captured_stdout"));
    assert!(!failure.details().contains_key("captured_stderr"));
}

#[test]
fn actual_capture_large_single_line_keeps_the_existing_byte_capacity() {
    use std::io::Write;
    let tmp = native_tempdir();
    let data = tmp.path().join("large.txt");
    let mut file = fs::File::create(&data).unwrap();
    let block = [b'x'; 8192];
    for _ in 0..2048 {
        file.write_all(&block).unwrap();
    }
    file.sync_all().unwrap();
    let capsule = script_capsule(tmp.path(), "#!/bin/sh\ncat \"$1\"\n");
    let report = run::execute(
        &run::plan_script(
            &capsule,
            &[data.to_str().unwrap().to_owned()],
            None,
            tmp.path(),
        )
        .unwrap(),
    )
    .unwrap();
    let captured = report.captured.as_ref().unwrap();
    assert_eq!(captured.stdout.len(), 16 * 1024 * 1024);
    assert!(captured.stdout.bytes().all(|byte| byte == b'x'));
    assert!(captured.stderr.is_empty());
    assert_eq!(report.output.len(), 1);
    assert_eq!(report.output[0], captured.stdout);
}

#[test]
fn actual_capture_signal_status_matches_the_capsule_status_contract() {
    use aikit_adapters::runner::{CommandRunner, SystemRunner};
    let tmp = native_tempdir();
    let capsule = script_capsule(tmp.path(), "#!/bin/sh\nkill -TERM $$\n");
    let report = run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap();
    assert_eq!(report.status, 143);
    assert_eq!(report.captured.as_ref().unwrap().status, 143);
    let generic = SystemRunner::new()
        .run(&["/bin/sh".into(), "-c".into(), "kill -TERM $$".into()])
        .unwrap();
    assert_eq!(generic.status, -1, "generic status policy is unchanged");
}

#[test]
fn actual_capture_invalid_utf8_is_refused_without_a_semantic_report() {
    use std::error::Error;
    let tmp = native_tempdir();
    let capsule = script_capsule(tmp.path(), "#!/bin/sh\nprintf '\\377'\n");
    let failure =
        run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap_err();
    assert_eq!(failure.code(), "mux.command_utf8_invalid", "{failure:?}");
    assert_eq!(failure.details()["known_exit_status"], "0");
    assert_eq!(failure.details()["capture_cancelled"], "false");
    assert_eq!(
        failure
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    let capsule = script_capsule(tmp.path(), "#!/bin/sh\nprintf '\\357\\277\\275'\n");
    let report = run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap();
    assert_eq!(report.captured.as_ref().unwrap().stdout, "\u{fffd}");
}

#[test]
fn actual_closed_delivery_socket_preserves_completed_capture_and_original_io() {
    use std::error::Error;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    let tmp = native_tempdir();
    let capsule = script_capsule(
        tmp.path(),
        "#!/bin/sh\nprintf 'native-result'\nprintf 'native-diagnostic' >&2\nexit 7\n",
    );
    let report = run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap();
    let retained = Arc::clone(report.captured.as_ref().unwrap());
    let (mut writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let mut stderr = Vec::new();
    let failure = run::emit_report_to(&report, &mut writer, &mut stderr).unwrap_err();
    assert_eq!(failure.code(), "run.output_delivery_failed", "{failure:?}");
    assert_eq!(failure.details()["known_exit_status"], "7");
    assert_eq!(failure.details()["capture_complete"], "true");
    assert_eq!(failure.details()["automatic_retry"], "false");
    assert_eq!(
        failure.details()["captured_stdout_bytes"],
        "native-result".len().to_string()
    );
    assert_eq!(
        failure.details()["captured_stderr_bytes"],
        "native-diagnostic".len().to_string()
    );
    assert_eq!(failure.details()["delivery_unconfirmed"], "true");
    assert!(!failure.details().contains_key("captured_stdout"));
    assert!(!failure.details().contains_key("captured_stderr"));
    for diagnostic in [
        failure.to_string(),
        format!("{failure:?}"),
        aikit_cli::json::line(&aikit_cli::json::failure(&failure)),
    ] {
        assert!(
            !diagnostic.contains("native-result"),
            "completed stdout must not reroute to diagnostics"
        );
        assert!(
            !diagnostic.contains("native-diagnostic"),
            "completed stderr must not reroute to diagnostics"
        );
    }
    assert_eq!(retained.stdout, "native-result");
    assert_eq!(retained.stderr, "native-diagnostic");
    let cause = failure
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), std::io::ErrorKind::BrokenPipe);
    assert!(cause.raw_os_error().is_some());
    assert!(Arc::ptr_eq(&retained, report.captured.as_ref().unwrap()));
    assert!(
        stderr.is_empty(),
        "later stream is unattempted after failed stdout"
    );
}

#[test]
fn actual_unfinished_descendant_capture_is_not_a_completed_capsule_result() {
    let tmp = native_tempdir();
    let capsule = script_capsule(
        tmp.path(),
        "#!/bin/sh\nprintf 'leader-result'\n(sleep 30; printf 'unfinished-child') &\nexit 0\n",
    );
    let failure =
        run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap_err();
    match failure.code() {
        "mux.command_capture_cancelled" => {
            assert_eq!(failure.details()["stdout_eof"], "true");
            assert_eq!(failure.details()["stderr_eof"], "true");
        }
        "mux.command_capture_incomplete" => {
            assert!(
                failure.details()["stdout_eof"] == "false"
                    || failure.details()["stderr_eof"] == "false"
            );
        }
        other => panic!(
            "expected specific cancelled/incomplete native capture, got {other}: {failure:?}"
        ),
    }
    assert_eq!(failure.details()["known_exit_status"], "0");
    assert_eq!(failure.details()["capture_cancelled"], "true");
    assert_eq!(failure.details()["execution_started"], "true");
    assert_eq!(failure.details()["effects"], "unknown");
    assert_eq!(failure.details()["automatic_retry"], "false");
    assert_eq!(
        failure.details()["captured_stdout_bytes"],
        "leader-result".len().to_string()
    );
    assert!(!failure.details().contains_key("captured_stdout"));
}

#[test]
fn actual_empty_and_unterminated_capture_preserve_the_old_digest_basis() {
    let tmp = native_tempdir();
    for body in [
        "#!/bin/sh\nexit 0\n",
        "#!/bin/sh\nprintf 'one'\nprintf 'two' >&2\n",
        "#!/bin/sh\nprintf '\\n\\r\\n'\nprintf 'tail' >&2\n",
    ] {
        let capsule = script_capsule(tmp.path(), body);
        let report =
            run::execute(&run::plan_script(&capsule, &[], None, tmp.path()).unwrap()).unwrap();
        let captured = report.captured.as_ref().unwrap();
        let old_basis: Vec<String> = captured
            .stdout
            .lines()
            .chain(captured.stderr.lines())
            .map(str::to_owned)
            .collect();
        assert_eq!(report.output, old_basis);
        assert_eq!(
            run::method_output_digest(&report),
            blake3::hash(old_basis.join("\n").as_bytes())
                .to_hex()
                .to_string()
        );
        let handle = aikit_cli::app::RunHandle {
            capsule: capsule.id.clone(),
            report,
        };
        let mut hash = blake3::Hasher::new();
        hash.update(b"aikit.scoped-run-result/v1\0");
        hash.update(capsule.id.to_string().as_bytes());
        hash.update(b"\0");
        hash.update(handle.report.status.to_string().as_bytes());
        hash.update(b"\0joined");
        for line in &old_basis {
            hash.update(b"\0");
            hash.update(line.as_bytes());
        }
        assert_eq!(
            aikit_cli::scoped_invocation::run_result_digest(&handle),
            hash.finalize().to_hex().to_string()
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn actual_selected_capture_failure_keeps_partial_output_and_arguments_out_of_printers() {
    let root = native_tempdir();
    let capsule = script_capsule_with_config(
        root.path(),
        "#!/bin/sh\nprintf '%s' \"$1\"\nprintf 'private-stderr-canary' >&2\nsleep 30\n",
        "timeout = \"1s\"\n",
    );
    let failure = run::execute(
        &run::plan_script(
            &capsule,
            &["private-native-input-canary".to_owned()],
            None,
            root.path(),
        )
        .unwrap(),
    )
    .unwrap_err();
    assert_eq!(failure.code(), "mux.command_timeout", "{failure:?}");
    assert_eq!(failure.details()["execution_started"], "true");
    assert_eq!(failure.details()["effects"], "unknown");
    assert_eq!(failure.details()["automatic_retry"], "false");
    assert_eq!(failure.details()["direct_child_reaped"], "true");
    assert_eq!(
        failure.details()["captured_stdout_bytes"],
        "private-native-input-canary".len().to_string()
    );
    assert_eq!(
        failure.details()["captured_stderr_bytes"],
        "private-stderr-canary".len().to_string()
    );
    assert!(!failure.details().contains_key("command"));
    assert!(!failure.details().contains_key("captured_stdout"));
    assert!(!failure.details().contains_key("captured_stderr"));
    let capture = failure
        .native_capture()
        .expect("actual capsule timeout capture retained");
    assert_eq!(capture.stdout.as_slice(), b"private-native-input-canary");
    assert_eq!(capture.stderr.as_slice(), b"private-stderr-canary");
    assert_eq!(
        capture.status.map(|status| status.to_string()),
        failure
            .details()
            .get("known_exit_status")
            .or_else(|| failure.details().get("cleanup_exit_status"))
            .cloned()
    );
    assert!(!failure.details().contains_key("known_exit_status"));
    let cloned = failure.clone();
    let wrapped = aikit_core::error::AikitError::new(
        "mux.command_capture_failed",
        "Selected command refused",
    )
    .with_io_source_from(&failure);
    assert!(std::ptr::eq(capture, cloned.native_capture().unwrap()));
    assert!(std::ptr::eq(capture, wrapped.native_capture().unwrap()));
    for failure in [&failure, &cloned, &wrapped] {
        for diagnostic in [
            failure.to_string(),
            format!("{failure:?}"),
            aikit_cli::json::line(&aikit_cli::json::failure(failure)),
        ] {
            assert!(!diagnostic.contains("private-native-input-canary"));
            assert!(!diagnostic.contains("private-stderr-canary"));
        }
    }
}
