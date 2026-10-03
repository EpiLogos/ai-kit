//! Executing a capability.
//!
//! AIKit never embeds a terminal emulator, so "run" is always a real child
//! process the OS schedules. The [`ExecMode`] decides the relationship between
//! that child and the current terminal:
//!
//! * **foreground** — the child inherits the terminal; AIKit waits and reports.
//! * **capture** — stdout and stderr are collected for a result panel.
//! * **background** — the child is detached and tracked, surfaced by `jobs`.
//! * **replace** — an exec-style handoff; on Unix AIKit's process *becomes* the
//!   child and never returns.
//! * **new-pane / new-view** — handed to the multiplexer adapter; those are
//!   planned in [`crate::app`], not here, because they need a mux binding.
//!
//! The planning step ([`plan_script`]) is deliberately separate from execution so
//! that the argv, working directory and environment can be inspected, redacted
//! and tested without spawning anything.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use aikit_adapters::runner::{Output, SystemRunner};

use aikit_core::capsule::{Capsule, ExecMode, WorkingDir};
use aikit_core::{AikitError, Result};

/// A fully-resolved command ready to run: no capsule lookups, no path
/// resolution and no environment guesswork left to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCommand {
    /// The program to spawn (an interpreter, or the entry script itself).
    pub program: String,
    /// Arguments after the program, including the entry path when an interpreter
    /// is used and the user's pass-through arguments.
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub mode: ExecMode,
    /// Actual manifest deadline. None preserves the no-declared-deadline policy.
    pub timeout: Option<Duration>,
}

impl ScriptCommand {
    /// The command as a single shell-ish line, for logs and `--json` echoes.
    pub fn display(&self) -> String {
        let mut parts = vec![self.program.clone()];
        parts.extend(self.argv.iter().cloned());
        parts.join(" ")
    }
}

/// Plan the execution of a **script** capsule.
///
/// Only scripts, tools and templates are runnable; asking to run a skill, hook or
/// guidance capsule is a category error, not a missing feature, and is refused
/// with `run.not_runnable` rather than silently doing nothing.
pub fn plan_script(
    capsule: &Capsule,
    user_args: &[String],
    project_root: Option<&Path>,
    invocation_cwd: &Path,
) -> Result<ScriptCommand> {
    let script = capsule.script().ok_or_else(|| {
        AikitError::new(
            "run.not_runnable",
            format!(
                "{} is a {} and cannot be run",
                capsule.id,
                capsule.kind.as_str()
            ),
        )
        .with("capability", capsule.id.to_string())
        .with("kind", capsule.kind.as_str())
    })?;

    let root = capsule.root.as_ref().ok_or_else(|| {
        AikitError::new(
            "run.source_missing",
            format!("{} has no payload on this machine", capsule.id),
        )
        .with("capability", capsule.id.to_string())
    })?;
    let entry = root.join(&script.entry);
    let entry_str = entry.to_string_lossy().to_string();

    // With an interpreter the program is the interpreter and the entry is its
    // first argument; without one the entry is the program and must itself be
    // executable. Either way the user's arguments come last, passed through
    // verbatim — `aikit run x -- --flag` should reach the script as `--flag`.
    let (program, mut argv) = match script.interpreter.split_first() {
        Some((program, rest)) => {
            let mut argv: Vec<String> = rest.to_vec();
            argv.push(entry_str);
            (program.clone(), argv)
        }
        None => (entry_str, Vec::new()),
    };
    argv.extend(user_args.iter().cloned());

    let cwd = match script.cwd {
        WorkingDir::Project => project_root.unwrap_or(invocation_cwd).to_path_buf(),
        WorkingDir::Cwd => invocation_cwd.to_path_buf(),
        WorkingDir::Capsule => root.clone(),
    };

    Ok(ScriptCommand {
        program,
        argv,
        cwd,
        env: script.env.clone(),
        mode: script.mode,
        timeout: script.timeout.map(|duration| duration.as_duration()),
    })
}

/// What a finished run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// The child's exit status, or 128 + signal when it was killed by a signal.
    pub status: i32,
    /// Captured lines, populated only in [`ExecMode::Capture`].
    pub output: Vec<String>,
    /// Exact completed native streams/status; only real capture populates this.
    pub captured: Option<Arc<Output>>,
    /// True when the process was left running (background).
    pub detached: bool,
}

/// Run a planned command according to its mode.
///
/// `new-pane`/`new-view` are rejected here with `run.needs_mux`: they cannot be
/// honoured without a multiplexer binding, and pretending otherwise (by running
/// the child in the current terminal) would be exactly the kind of silent
/// substitution the architecture forbids.
pub fn execute(command: &ScriptCommand) -> Result<RunReport> {
    match command.mode {
        ExecMode::Capture => execute_captured(command),
        ExecMode::Background => execute_background(command),
        ExecMode::Foreground | ExecMode::Replace => execute_foreground(command),
        ExecMode::NewPane | ExecMode::NewView => Err(AikitError::new(
            "run.needs_mux",
            format!(
                "{} needs a multiplexer to open a new pane or view",
                command.mode.as_str()
            ),
        )
        .with("mode", command.mode.as_str())),
    }
}

fn base_command(command: &ScriptCommand) -> Command {
    let mut cmd = Command::new(&command.program);
    cmd.args(&command.argv).current_dir(&command.cwd);
    for (key, value) in &command.env {
        cmd.env(key, value);
    }
    cmd
}

fn spawn_error(command: &ScriptCommand, e: std::io::Error) -> AikitError {
    AikitError::new(
        "run.spawn_failed",
        format!("could not run `{}`: {e}", command.display()),
    )
    .with("program", command.program.clone())
}

fn execute_captured(command: &ScriptCommand) -> Result<RunReport> {
    // One native owner retains the pipes, deadline, exit identity and effects.
    // LF admission occurs during that capture, before any line allocation.
    let mut runner = SystemRunner::new().with_strict_utf8().with_body_free_diagnostics()
        .with_capture_line_feed_limit(65_536).with_unix_signal_status();
    if let Some(timeout) = command.timeout { runner = runner.with_timeout(timeout); }
    let captured = Arc::new(runner.capture_command(&mut base_command(command))?);
    let row_count = captured.stdout.lines().count() + captured.stderr.lines().count();
    let mut lines = Vec::with_capacity(row_count);
    lines.extend(captured.stdout.lines().chain(captured.stderr.lines()).map(str::to_owned));
    Ok(RunReport {
        status: captured.status,
        output: lines,
        captured: Some(captured),
        detached: false,
    })
}

/// Deliver completed native streams without rejoining lines or inventing a
/// newline. A delivery failure retains the completed result and actual IO
/// cause; an already written prefix is possible, so it must not be retried.
pub fn emit_report_to(
    report: &RunReport, stdout: &mut dyn Write, stderr: &mut dyn Write,
) -> Result<()> {
    let delivery_failure = |stage: &str, cause: std::io::Error| {
        let kind = format!("{:?}", cause.kind());
        let errno = cause.raw_os_error().map_or_else(|| "none".into(), |code| code.to_string());
        let mut failure = AikitError::new("run.output_delivery_failed",
            format!("Completed script {stage} delivery failed"))
            .with("observation_stage", stage)
            .with("execution_started", if report.captured.is_some() { "true" } else { "unknown" })
            .with("effects", "unknown").with("automatic_retry", "false")
            .with("capture_complete", report.captured.is_some().to_string())
            .with("known_exit_status", report.status.to_string())
            .with("delivery_unconfirmed", "true").with("io_kind", kind).with("raw_os_error", errno)
            .with_io_source(cause);
        if let Some(captured) = &report.captured {
            // Completed bodies remain in the caller's borrowed Arc. Error
            // printers have a different sink and receive only actual counts.
            failure = failure.with("captured_stdout_bytes", captured.stdout.len().to_string())
                .with("captured_stderr_bytes", captured.stderr.len().to_string());
        }
        failure
    };
    if let Some(captured) = &report.captured {
        stdout.write_all(captured.stdout.as_bytes()).map_err(|cause| delivery_failure("stdout", cause))?;
        stdout.flush().map_err(|cause| delivery_failure("stdout_flush", cause))?;
        stderr.write_all(captured.stderr.as_bytes()).map_err(|cause| delivery_failure("stderr", cause))?;
        stderr.flush().map_err(|cause| delivery_failure("stderr_flush", cause))?;
    } else if !report.output.is_empty() {
        // Existing manually constructed noncapture reports retain their line
        // projection. This path does not assert a completed native capture.
        for line in &report.output {
            writeln!(stdout, "{line}").map_err(|cause| delivery_failure("stdout_lines", cause))?;
        }
        stdout.flush().map_err(|cause| delivery_failure("stdout_flush", cause))?;
    }
    Ok(())
}

/// Use the actual CLI output streams, including multicall export delivery.
pub fn emit_report(report: &RunReport) -> Result<()> {
    emit_report_to(report, &mut std::io::stdout().lock(), &mut std::io::stderr().lock())
}

/// The unchanged v1 Method digest basis: stdout lines, then stderr lines,
/// separated by one LF and without an invented terminal LF. This is a line
/// view digest, not a claim about the exact native byte streams.
pub fn method_output_digest(report: &RunReport) -> String {
    let mut hash = blake3::Hasher::new();
    for (index, line) in report.output.iter().enumerate() {
        if index != 0 { hash.update(b"\n"); }
        hash.update(line.as_bytes());
    }
    hash.finalize().to_hex().to_string()
}

fn execute_foreground(command: &ScriptCommand) -> Result<RunReport> {
    // The child inherits the real terminal (the default stdio), which is what a
    // foreground run is for. Replace mode is handled by the caller before this,
    // via `exec_replace`; if it reaches here the platform lacks `exec` and a
    // waited foreground run is the honest degradation.
    let status = base_command(command)
        .status()
        .map_err(|e| spawn_error(command, e))?;
    Ok(RunReport {
        status: status_code(status),
        output: Vec::new(),
        captured: None,
        detached: false,
    })
}

fn execute_background(command: &ScriptCommand) -> Result<RunReport> {
    base_command(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| spawn_error(command, e))?;
    Ok(RunReport {
        status: 0,
        output: Vec::new(),
        captured: None,
        detached: true,
    })
}

/// Replace the current process image with the command (Unix `exec`).
///
/// Returns only on failure to exec: on success control never comes back, which
/// is the whole point of `replace` mode — the terminal, signals and exit status
/// all belong to the child directly with no AIKit wrapper in the way.
#[cfg(unix)]
pub fn exec_replace(command: &ScriptCommand) -> AikitError {
    use std::os::unix::process::CommandExt;
    let e = base_command(command).exec();
    spawn_error(command, e)
}

fn status_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(code) = status.code() {
            return code;
        }
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}
