//! The command runner is the seam every multiplexer adapter is built on.
//!
//! Two properties have to hold, and both are about *not* lying to the caller:
//!
//! * A non-zero exit is data, not an error. `tmux has-session` answers "no" with
//!   exit 1, and an adapter that treated that as a failure could never ask the
//!   question.
//! * A binary that is not there *is* an error, and it must be distinguishable
//!   from the command running and failing.

use std::path::PathBuf;

use aikit_adapters::runner::{CommandRunner, RecordingRunner, ScriptedRunner, SystemRunner};

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// The real thing
// ---------------------------------------------------------------------------

#[test]
fn the_system_runner_runs_a_real_process_and_captures_its_output() {
    let runner = SystemRunner::new();
    let out = runner.run(&argv(&["/bin/echo", "hello adapters"])).unwrap();

    assert_eq!(out.status, 0);
    assert!(out.ok());
    assert_eq!(out.stdout.trim(), "hello adapters");
    assert_eq!(out.stderr, "");
}

#[test]
fn a_non_zero_exit_is_reported_as_data_rather_than_an_error() {
    // `tmux has-session` answers a question with its exit code. An adapter that
    // could not see a non-zero status without an error would have to parse text.
    let runner = SystemRunner::new();
    let out = runner
        .run(&argv(&["/bin/sh", "-c", "echo out; echo err >&2; exit 3"]))
        .unwrap();

    assert_eq!(out.status, 3);
    assert!(!out.ok());
    assert_eq!(out.stdout.trim(), "out");
    assert_eq!(out.stderr.trim(), "err");
}

#[test]
fn a_missing_binary_is_an_error_with_a_stable_code() {
    let runner = SystemRunner::new();
    let error = runner
        .run(&argv(&["/definitely/not/a/binary/aikit-test"]))
        .unwrap_err();

    assert_eq!(error.code(), "mux.command_spawn_failed");
    assert!(
        error
            .message()
            .contains("/definitely/not/a/binary/aikit-test"),
        "the message must name the binary, got: {}",
        error.message()
    );
}

#[test]
fn an_empty_argv_is_refused_rather_than_spawning_something_surprising() {
    let error = SystemRunner::new().run(&[]).unwrap_err();
    assert_eq!(error.code(), "mux.empty_command");
}

#[test]
fn the_system_runner_can_be_given_a_working_directory_and_environment() {
    let dir = tempfile::tempdir().unwrap();
    let runner = SystemRunner::new()
        .with_cwd(dir.path())
        .with_env("AIKIT_TEST_MARKER", "present");

    let cwd = runner.run(&argv(&["/bin/sh", "-c", "pwd"])).unwrap();
    // macOS hands out /var/folders/... which is a symlink to /private/var/...
    let reported = PathBuf::from(cwd.stdout.trim());
    assert_eq!(
        reported.canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );

    let marker = runner
        .run(&argv(&[
            "/bin/sh",
            "-c",
            "printf %s \"$AIKIT_TEST_MARKER\"",
        ]))
        .unwrap();
    assert_eq!(marker.stdout, "present");
}

// ---------------------------------------------------------------------------
// Recording, for argv assertions against a real binary
// ---------------------------------------------------------------------------

#[test]
fn a_recording_runner_still_runs_the_real_command_and_remembers_the_argv() {
    let runner = RecordingRunner::new(SystemRunner::new());
    runner.run(&argv(&["/bin/echo", "one"])).unwrap();
    runner.run(&argv(&["/bin/echo", "two"])).unwrap();

    assert_eq!(
        runner.calls(),
        vec![argv(&["/bin/echo", "one"]), argv(&["/bin/echo", "two"])]
    );
}

// ---------------------------------------------------------------------------
// Scripting, for contract tests against recorded responses
// ---------------------------------------------------------------------------

#[test]
fn a_scripted_runner_answers_from_recorded_responses_and_records_what_was_asked() {
    let runner = ScriptedRunner::new()
        .on("version", "cmux 0.63.1")
        .on("list-workspaces", r#"{"workspaces":[]}"#);

    let version = runner.run(&argv(&["cmux", "version"])).unwrap();
    assert_eq!(version.stdout, "cmux 0.63.1");

    let list = runner.run(&argv(&["cmux", "list-workspaces"])).unwrap();
    assert_eq!(list.stdout, r#"{"workspaces":[]}"#);

    assert_eq!(
        runner.calls(),
        vec![
            argv(&["cmux", "version"]),
            argv(&["cmux", "list-workspaces"])
        ]
    );
}

#[test]
fn a_scripted_runner_matches_the_most_specific_recorded_pattern() {
    // `new-surface` and `new-surface --type browser` must be separable, or a
    // contract test cannot pin the difference between them.
    let runner = ScriptedRunner::new()
        .on("new-surface", r#"{"surface":"surface:1"}"#)
        .on("new-surface --type browser", r#"{"surface":"surface:9"}"#);

    let plain = runner.run(&argv(&["cmux", "new-surface"])).unwrap();
    assert_eq!(plain.stdout, r#"{"surface":"surface:1"}"#);

    let browser = runner
        .run(&argv(&["cmux", "new-surface", "--type", "browser"]))
        .unwrap();
    assert_eq!(browser.stdout, r#"{"surface":"surface:9"}"#);
}

#[test]
fn an_unscripted_command_fails_loudly_instead_of_returning_empty_output() {
    // Silently returning success for an unrecorded command is how a contract test
    // ends up asserting nothing at all.
    let runner = ScriptedRunner::new().on("version", "cmux 0.63.1");
    let error = runner.run(&argv(&["cmux", "list-workspaces"])).unwrap_err();

    assert_eq!(error.code(), "mux.unscripted_command");
    assert!(error.message().contains("list-workspaces"));
}

#[test]
fn a_scripted_sequence_answers_repeated_calls_with_successive_responses() {
    // Creating three panes issues three `split-window` calls that must come back
    // with three *different* pane ids, or an adapter's bookkeeping cannot be
    // tested at all.
    let runner = ScriptedRunner::new().sequence("split-window", &["%1", "%2", "%3"]);

    for expected in ["%1", "%2", "%3"] {
        let out = runner.run(&argv(&["tmux", "split-window", "-h"])).unwrap();
        assert_eq!(out.stdout, expected);
    }
    // Running past the end repeats the last response rather than failing: a test
    // that adds a pane should not have to re-record the whole sequence.
    let out = runner.run(&argv(&["tmux", "split-window", "-h"])).unwrap();
    assert_eq!(out.stdout, "%3");
}

#[test]
fn a_scripted_response_can_carry_a_failing_status_and_stderr() {
    let runner = ScriptedRunner::new().failing("capabilities", 1, "Error: Socket not found");
    let out = runner.run(&argv(&["cmux", "capabilities"])).unwrap();

    assert_eq!(out.status, 1);
    assert!(!out.ok());
    assert!(out.stderr.contains("Socket not found"));
}
