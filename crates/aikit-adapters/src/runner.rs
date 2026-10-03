//! The command seam every external-process adapter is built on.
//!
//! Multiplexer adapters do not call [`std::process::Command`] directly. They call
//! a [`CommandRunner`], which buys two things that matter more than the
//! indirection costs:
//!
//! * a unit test can assert the *exact* argv an adapter produces, which is the
//!   only way to pin down flags like `display-popup -E -w 82% -h 70%` without a
//!   running server;
//! * an integration test can hand the same adapter a [`SystemRunner`] and drive
//!   the real binary, so the argv assertions are not asserting a fiction.
//!
//! ## A non-zero exit is data
//!
//! A nonzero exit remains ordinary status data when bounded capture and the
//! owned child lifecycle complete. Spawn failures and actual postlaunch
//! capacity, read, timeout or cleanup failures return `Err`; postlaunch errors
//! preserve possible effects and original IO causes, never a rollback claim.
//! `tmux has-session` still answers yes/no through its actual exit status.
//! Strict semantic capture also requires actual complete pipe EOF: cancelling
//! unfinished inherited output cannot create a successful semantic receipt.
//! Ordinary lossy capture keeps its existing idle-pipe cleanup behavior.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aikit_core::{AikitError, Result};

/// What a finished command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            status: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn failure(status: i32, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    pub fn ok(&self) -> bool {
        self.status == 0
    }

    /// stdout with the trailing newline removed, which is what every `-F`
    /// formatted tmux query and every one-line cmux answer actually means.
    pub fn line(&self) -> &str {
        self.stdout.trim_end_matches(['\n', '\r'])
    }

    /// Turn a failed command into an error, naming what was run.
    ///
    /// Callers that *ask a question* with the exit status must not use this.
    pub fn require(self, argv: &[String], code: &'static str) -> Result<Self> {
        if self.ok() {
            return Ok(self);
        }
        let detail = if self.stderr.trim().is_empty() {
            self.stdout.trim().to_string()
        } else {
            self.stderr.trim().to_string()
        };
        Err(AikitError::new(
            code,
            format!(
                "`{}` exited with status {}{}",
                argv.join(" "),
                self.status,
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            ),
        )
        .with("command", argv.join(" "))
        .with("status", self.status.to_string()))
    }
}

/// Runs one external command.
///
/// Object-safe: the stack adapter holds heterogeneous adapters behind `dyn`, and
/// each of those owns a runner.
pub trait CommandRunner {
    fn run(&self, argv: &[String]) -> Result<Output>;

    /// The runner's configured budget, when one is available. Optional providers
    /// may use this to narrow their own ceiling without changing the explicit
    /// `run_with_timeout` contract for other callers.
    fn configured_timeout(&self) -> Option<std::time::Duration> {
        None
    }

    /// Run with a wall-clock budget. A command that has not finished inside the
    /// budget is killed and answered as a runner error (`mux.command_timeout`):
    /// there is no status data to hand back.
    ///
    /// The default delegates to [`CommandRunner::run`] unchanged — an
    /// in-memory runner has nothing to kill — so only runners that spawn real
    /// processes need to, and can, enforce the budget.
    fn run_with_timeout(&self, argv: &[String], timeout: std::time::Duration) -> Result<Output> {
        let _ = timeout;
        self.run(argv)
    }

    /// Narrow one invocation to the remaining time and aggregate output
    /// allowance. The native capture reserves half the bytes for each stream;
    /// an unused half is not borrowed. A strict request must reject invalid
    /// UTF-8 before returning a semantic text receipt. Unknown runners refuse
    /// before execution rather than silently delegating an unenforced budget.
    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        let _ = (remaining_timeout, remaining_aggregate_bytes, strict_utf8);
        Err(AikitError::new("mux.command_limits_unsupported",
            "This runner cannot enforce per-invocation capture limits")
            .with("command", argv.join(" ")).with("execution_started", "false"))
    }
}

impl<T: CommandRunner + ?Sized> CommandRunner for Box<T> {
    fn run(&self, argv: &[String]) -> Result<Output> {
        (**self).run(argv)
    }

    fn run_with_timeout(&self, argv: &[String], timeout: std::time::Duration) -> Result<Output> {
        (**self).run_with_timeout(argv, timeout)
    }

    fn configured_timeout(&self) -> Option<std::time::Duration> {
        (**self).configured_timeout()
    }

    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        (**self).run_with_limits(argv, remaining_timeout, remaining_aggregate_bytes, strict_utf8)
    }
}

impl<T: CommandRunner + ?Sized> CommandRunner for &T {
    fn run(&self, argv: &[String]) -> Result<Output> {
        (**self).run(argv)
    }

    fn run_with_timeout(&self, argv: &[String], timeout: std::time::Duration) -> Result<Output> {
        (**self).run_with_timeout(argv, timeout)
    }

    fn configured_timeout(&self) -> Option<std::time::Duration> {
        (**self).configured_timeout()
    }

    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        (**self).run_with_limits(argv, remaining_timeout, remaining_aggregate_bytes, strict_utf8)
    }
}

/// A shared runner. The stack adapter owns its layers as `Box<dyn MuxAdapter>`,
/// which means a test cannot reach back into an adapter to see what it ran —
/// unless the runner itself is shared, which is what this makes possible.
impl<T: CommandRunner + ?Sized> CommandRunner for std::sync::Arc<T> {
    fn run(&self, argv: &[String]) -> Result<Output> {
        (**self).run(argv)
    }

    fn run_with_timeout(&self, argv: &[String], timeout: std::time::Duration) -> Result<Output> {
        (**self).run_with_timeout(argv, timeout)
    }

    fn configured_timeout(&self) -> Option<std::time::Duration> {
        (**self).configured_timeout()
    }

    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        (**self).run_with_limits(argv, remaining_timeout, remaining_aggregate_bytes, strict_utf8)
    }
}

// ---------------------------------------------------------------------------
// The real thing
// ---------------------------------------------------------------------------

/// Spawns real subprocesses.
#[derive(Debug, Default, Clone)]
pub struct SystemRunner {
    cwd: Option<PathBuf>,
    env: BTreeMap<String, String>,
    /// Environment keys explicitly withheld from the child. An external tool
    /// must answer the argv it was given, not a configuration file the parent
    /// shell happened to carry (e.g. `RIPGREP_CONFIG_PATH`).
    env_removed: Vec<String>,
    timeout: Option<std::time::Duration>,
    output_limit_bytes: Option<u64>,
    strict_utf8: bool,
    aggregate_output_limit_bytes: Option<usize>,
    capture_line_feed_limit: Option<usize>,
    unix_signal_status: bool,
    body_free_diagnostics: bool,
}

impl SystemRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// A runner bounded by the shared probe budget
    /// ([`aikit_core::probe::probe_budget`], default 10s, `AIKIT_PROBE_BUDGET_SECS`
    /// overrides). Read, status and probe surfaces construct this instead of an
    /// unbounded runner, so a hanging child — the gemini-with-expired-oauth
    /// class — is killed inside the budget and reported as `timed-out` rather
    /// than silently stalling the surface. An explicit [`with_timeout`] still
    /// wins: the probe budget is the default, never a ceiling on configuration.
    #[must_use]
    pub fn probe() -> Self {
        Self::new().with_timeout(aikit_core::probe::probe_budget())
    }

    /// The configured timeout, when this runner is bounded.
    pub fn timeout(&self) -> Option<std::time::Duration> {
        self.timeout
    }

    #[must_use]
    pub fn with_cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = Some(cwd.as_ref().to_path_buf());
        self
    }

    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_env_removed(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        self.env.retain(|existing, _| existing != &key);
        self.env_removed.push(key);
        self
    }

    /// Kill the child if it has not finished within the budget. A timed-out
    /// command is a runner error, not a failed command: there is no status
    /// data to hand back.
    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Captured raw bytes per stream. A larger transport is explicit; no output
    /// is truncated into a successful command result.
    pub fn output_limit_bytes(&self) -> u64 {
        self.output_limit_bytes.unwrap_or(DEFAULT_COMMAND_OUTPUT_LIMIT_BYTES)
    }

    /// Set a positive finite per-stream capacity. Validation precedes spawn.
    #[must_use]
    pub fn with_output_limit_bytes(mut self, limit: u64) -> Self {
        self.output_limit_bytes = Some(limit);
        self
    }

    /// Decode actual stdout and stderr strictly after the same native EOF and
    /// child-retirement checks. The default remains lossy for ordinary tools.
    #[must_use]
    pub fn with_strict_utf8(mut self) -> Self {
        self.strict_utf8 = true;
        self
    }

    /// Admit at most this many actual LF bytes across both captured streams.
    /// This opt-in capacity is checked before retaining each observed chunk;
    /// ordinary adapter capture keeps its existing byte-only defaults.
    #[must_use]
    pub fn with_capture_line_feed_limit(mut self, limit: usize) -> Self {
        self.capture_line_feed_limit = Some(limit);
        self
    }

    /// Report an actual Unix signal as 128 + signal before its ExitStatus is
    /// released. Ordinary adapter capture retains its existing -1 fallback.
    #[must_use]
    pub fn with_unix_signal_status(mut self) -> Self {
        self.unix_signal_status = true;
        self
    }

    /// Keep selected command failures body-free: no stream bodies, program,
    /// arguments, cwd or path values are copied into rendered diagnostics.
    /// Actual IO causes and lifecycle/status/count evidence remain available.
    /// Ordinary adapter diagnostics retain their existing default policy.
    #[must_use]
    pub fn with_body_free_diagnostics(mut self) -> Self {
        self.body_free_diagnostics = true;
        self
    }

    /// Capture an explicitly configured command through the same native
    /// lifecycle as run/run_with_timeout. Runner cwd/env overrides still apply.
    pub fn capture_command(&self, command: &mut std::process::Command) -> Result<Output> {
        let argv = std::iter::once(command.get_program())
            .chain(command.get_args()).map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        self.configure_command(command);
        self.spawn_bounded(command, &argv, self.timeout)
    }

    fn configure_command(&self, command: &mut std::process::Command) {
        if let Some(cwd) = &self.cwd { command.current_dir(cwd); }
        for (key, value) in &self.env { command.env(key, value); }
        for key in &self.env_removed { command.env_remove(key); }
    }

    fn spawn_bounded(
        &self,
        command: &mut std::process::Command,
        argv: &[String],
        budget: Option<std::time::Duration>,
    ) -> Result<Output> {
        let limit = self.output_limit_bytes();
        if limit == 0 || usize::try_from(limit).is_err() {
            return Err(AikitError::new("mux.command_output_limit_invalid",
                "Command output capacity must be positive and fit this platform")
                .with("execution_started", "false").with("output_limit_bytes", limit.to_string()));
        }
        if self.capture_line_feed_limit == Some(0) {
            return Err(AikitError::new("mux.command_output_limit_invalid",
                "Command LF capacity must be positive")
                .with("execution_started", "false").with("observation_stage", "line_projection_capacity")
                .with("line_feed_limit", "0"));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        { capture_native_command(command, argv, budget, CapturePolicy {
            limit, strict_utf8: self.strict_utf8, aggregate_limit: self.aggregate_output_limit_bytes,
            line_feed_limit: self.capture_line_feed_limit, unix_signal_status: self.unix_signal_status,
            body_free_diagnostics: self.body_free_diagnostics,
        }) }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (command, budget);
            let failure = AikitError::new("mux.command_capture_unsupported",
                "Bounded native command capture is unavailable on this platform")
                .with("execution_started", "false");
            Err(if self.body_free_diagnostics {
                failure.with("argument_count", argv.len().saturating_sub(1).to_string())
            } else { failure.with("command", argv.join(" ")) })
        }
    }

}

/// The observed result of signalling a child group created by this caller.
/// Neither result establishes that descendants were reaped or have retired.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
pub enum OwnedChildGroupSignal {
    Delivered,
    /// The actual syscall found no such group; retain its original OS error.
    AlreadyAbsent { cause: std::io::Error },
}

/// Signal only the group of a child spawned with `process_group(0)` by the
/// caller, which must exclusively own the still-unreaped Child. The native
/// non-consuming wait checks that ownership before using its numeric group ID.
/// Actual ECHILD after reap refuses signalling. Successful signalling is
/// distinct from confirmed reaping or retirement.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn signal_owned_child_group(child: &std::process::Child) -> std::io::Result<OwnedChildGroupSignal> {
    crate::connection_process::peek_owned_child_exit(child)?;
    let pid = rustix::process::Pid::from_raw(child.id() as i32).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "owned child has no valid process group ID")
    })?;
    match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
        Ok(()) => Ok(OwnedChildGroupSignal::Delivered),
        Err(error) if error == rustix::io::Errno::SRCH => {
            Ok(OwnedChildGroupSignal::AlreadyAbsent { cause: error.into() })
        }
        Err(error) => Err(error.into()),
    }
}

pub const DEFAULT_COMMAND_OUTPUT_LIMIT_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct OwnedCommand {
    child: std::process::Child,
    cleanup_attempted: bool,
    reaped_status: Option<std::process::ExitStatus>,
    ownership_lost: bool,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct CommandCleanup {
    signal: &'static str,
    absence: Option<std::io::Error>,
    error: Option<std::io::Error>,
    additional_errors: Vec<std::io::Error>,
    status: Option<std::process::ExitStatus>,
    reaped: bool,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl OwnedCommand {
    fn observe_exit(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let observed = crate::connection_process::peek_owned_child_exit(&self.child);
        if let Err(error) = &observed {
            if error.raw_os_error() == Some(rustix::io::Errno::CHILD.raw_os_error()) {
                self.ownership_lost = true;
            }
        }
        observed
    }

    fn no_signal(&self) -> CommandCleanup {
        CommandCleanup {
            signal: if self.ownership_lost { "ownership-lost" } else { "not-needed" },
            absence: None, error: None, additional_errors: Vec::new(),
            status: self.reaped_status, reaped: self.reaped_status.is_some(),
        }
    }

    fn finish_reap(&mut self, result: &mut CommandCleanup, deadline: std::time::Instant) {
        if self.ownership_lost { return; }
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    // Cache retirement before Drop or any later cleanup can act.
                    self.reaped_status = Some(status);
                    result.status = Some(status);
                    result.reaped = true;
                    break;
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline { break; }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => {
                    if error.raw_os_error() == Some(rustix::io::Errno::CHILD.raw_os_error()) {
                        self.ownership_lost = true;
                    }
                    if result.error.is_none() { result.error = Some(error); }
                    else { result.additional_errors.push(error); }
                    // A failed wait never authorises a subsequent numeric kill.
                    break;
                }
            }
        }
    }

    fn reap_without_signal(&mut self, deadline: std::time::Instant) -> CommandCleanup {
        self.cleanup_attempted = true;
        let mut result = self.no_signal();
        if self.reaped_status.is_none() { self.finish_reap(&mut result, deadline); }
        result
    }

    fn cleanup(&mut self, deadline: std::time::Instant) -> CommandCleanup {
        self.cleanup_attempted = true;
        if self.reaped_status.is_some() || self.ownership_lost { return self.no_signal(); }
        let (signal, absence, error) = match signal_owned_child_group(&self.child) {
            Ok(OwnedChildGroupSignal::Delivered) => ("delivered", None, None),
            Ok(OwnedChildGroupSignal::AlreadyAbsent { cause }) => ("already-absent", Some(cause), None),
            Err(error) => ("failed", None, Some(error)),
        };
        let mut result = CommandCleanup {
            signal, absence, error, additional_errors: Vec::new(), status: None, reaped: false,
        };
        if result.error.as_ref().is_some_and(|error|
            error.raw_os_error() == Some(rustix::io::Errno::CHILD.raw_os_error()))
        {
            self.ownership_lost = true;
            return result;
        }
        if result.error.is_some() {
            // A direct-child fallback is permitted only after the SAME native
            // ownership check still observes a live, unreaped leader.
            match self.observe_exit() {
                Ok(None) => {
                    if let Err(error) = self.child.kill() { result.additional_errors.push(error); }
                }
                Ok(Some(_)) => {},
                Err(error) => { result.additional_errors.push(error); }
            }
        }
        self.finish_reap(&mut result, deadline);
        result
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for OwnedCommand {
    fn drop(&mut self) {
        if !self.cleanup_attempted && self.reaped_status.is_none() && !self.ownership_lost {
            let _ = self.cleanup(std::time::Instant::now() + std::time::Duration::from_secs(2));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
enum CaptureReadFailure {
    Io(std::io::Error),
    Limit,
    LineFeedLimit { observed: Option<usize>, limit: usize },
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy)]
struct CapturePolicy {
    limit: u64,
    strict_utf8: bool,
    aggregate_limit: Option<usize>,
    line_feed_limit: Option<usize>,
    unix_signal_status: bool,
    body_free_diagnostics: bool,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_available(
    pipe: &mut impl std::io::Read, bytes: &mut Vec<u8>, eof: &mut bool, limit: u64,
    line_feeds: &mut usize, line_feed_limit: Option<usize>,
) -> std::result::Result<bool, CaptureReadFailure> {
    if *eof { return Ok(false); }
    let mut progress = false;
    // A fixed number of chunks returns control to the deadline/wait checks.
    let mut chunk = [0u8; 8192];
    for _ in 0..8 {
        match pipe.read(&mut chunk) {
            Ok(0) => { *eof = true; return Ok(progress); }
            Ok(count) => {
                let next = bytes.len().checked_add(count).ok_or(CaptureReadFailure::Limit)?;
                if next as u64 > limit { return Err(CaptureReadFailure::Limit); }
                if let Some(capacity) = line_feed_limit {
                    let observed = line_feeds.checked_add(chunk[..count].iter()
                        .filter(|byte| **byte == b'\n').count());
                    let next_line_feeds = observed.ok_or(CaptureReadFailure::LineFeedLimit {
                        observed, limit: capacity,
                    })?;
                    if next_line_feeds > capacity {
                        return Err(CaptureReadFailure::LineFeedLimit { observed, limit: capacity });
                    }
                    *line_feeds = next_line_feeds;
                }
                bytes.extend_from_slice(&chunk[..count]);
                progress = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(progress),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {},
            Err(error) => return Err(CaptureReadFailure::Io(error)),
        }
    }
    Ok(progress)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn capture_io(phase: &str, error: std::io::Error, body_free: bool) -> AikitError {
    let message = if body_free { format!("Command {phase} failed") }
        else { format!("Command {phase} failed: {error}") };
    let failure = AikitError::new("mux.command_capture_failed", message)
        .with("observation_stage", phase);
    let failure = if body_free {
        failure.with("io_kind", format!("{:?}", error.kind()))
            .with("raw_os_error", error.raw_os_error().map_or_else(|| "none".into(), |n| n.to_string()))
    } else { failure };
    failure.with_io_source(error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn capture_read_error(stream: &str, failure: CaptureReadFailure, limit: u64, body_free: bool) -> AikitError {
    match failure {
        CaptureReadFailure::Io(error) => capture_io(stream, error, body_free),
        CaptureReadFailure::Limit => AikitError::new("mux.command_output_limit",
            "Actual command output exceeded its capacity; no truncated success is returned")
            .with("stream", stream).with("output_limit_bytes", limit.to_string()),
        CaptureReadFailure::LineFeedLimit { observed, limit } => AikitError::new(
            "mux.command_output_limit",
            "Actual command LF output exceeded its line-projection capacity; no truncated success is returned")
            .with("observation_stage", "line_projection_capacity").with("stream", stream)
            .with("line_feed_limit", limit.to_string())
            .with("observed_line_feeds", observed.map_or_else(|| "counter_overflow".into(), |n| n.to_string())),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn io_observation(error: &std::io::Error, body_free: bool) -> String {
    let mut value = serde_json::json!({"kind":format!("{:?}", error.kind()),
        "raw_os_error":error.raw_os_error()});
    if !body_free { value["message"] = error.to_string().into(); }
    value.to_string()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn captured_exit_status(status: std::process::ExitStatus, unix_signal_status: bool) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if unix_signal_status {
        status.code().unwrap_or_else(|| status.signal().map_or(-1, |signal| 128 + signal))
    } else { status.code().unwrap_or(-1) }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn command_failure(
    mut failure: AikitError, argv: &[String], mut cleanup: CommandCleanup,
    stdout: Vec<u8>, stderr: Vec<u8>, known_status: Option<std::process::ExitStatus>, policy: CapturePolicy,
) -> AikitError {
    use std::error::Error;
    let observed_status = known_status.or(cleanup.status)
        .map(|status| captured_exit_status(status, policy.unix_signal_status));
    failure = failure.with("execution_started", "true").with("effects", "unknown")
        .with("automatic_retry", "false").with("group_signal", cleanup.signal)
        .with("direct_child_reaped", cleanup.reaped.to_string());
    failure = if policy.body_free_diagnostics {
        failure.with("argument_count", argv.len().saturating_sub(1).to_string())
            .with("captured_stdout_bytes", stdout.len().to_string())
            .with("captured_stderr_bytes", stderr.len().to_string())
    } else {
        failure.with("command", argv.join(" "))
            .with("captured_stdout", String::from_utf8_lossy(&stdout).into_owned())
            .with("captured_stderr", String::from_utf8_lossy(&stderr).into_owned())
    };
    if let Some(status) = known_status {
        failure = failure.with("known_exit_status", captured_exit_status(status, policy.unix_signal_status).to_string());
    } else if let Some(status) = cleanup.status {
        failure = failure.with("cleanup_exit_status", captured_exit_status(status, policy.unix_signal_status).to_string());
    }
    if let Some(cause) = cleanup.absence.as_ref() {
        failure = failure.with("group_absence_cause", io_observation(cause, policy.body_free_diagnostics));
    }
    if !cleanup.additional_errors.is_empty() {
        let causes: Vec<_> = cleanup.additional_errors.iter().map(|error| {
            let mut value = serde_json::json!({"kind":format!("{:?}", error.kind()),
                "raw_os_error":error.raw_os_error()});
            if !policy.body_free_diagnostics { value["message"] = error.to_string().into(); }
            value
        }).collect();
        failure = failure.with("additional_cleanup_causes", serde_json::Value::Array(causes).to_string());
    }
    if let Some(cause) = cleanup.error.take() {
        failure = failure.with("cleanup_cause", io_observation(&cause, policy.body_free_diagnostics));
        if failure.source().is_none() { failure = failure.with_io_source(cause); }
        else if policy.body_free_diagnostics {
            failure = failure.with_secondary_io_source_from(&capture_io("cleanup", cause, true));
        }
    }
    if policy.body_free_diagnostics {
        if let Some(cause) = cleanup.absence.take() {
            failure = failure.with_secondary_io_source_from(&capture_io("group_absence", cause, true));
        }
        for cause in cleanup.additional_errors {
            failure = failure.with_secondary_io_source_from(&capture_io("additional_cleanup", cause, true));
        }
        // Retain the actual bounded vectors privately. Their presence does not
        // certify complete EOF, a successful receipt or remote-effect absence.
        failure = failure.with_native_capture(observed_status, stdout, stderr);
    }
    failure
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn capture_native_command(
    command: &mut std::process::Command, argv: &[String], budget: Option<std::time::Duration>, policy: CapturePolicy,
) -> Result<Output> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let CapturePolicy { limit, strict_utf8, aggregate_limit, line_feed_limit, unix_signal_status, body_free_diagnostics } = policy;
    let mut line_feeds = 0usize;
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    let child = command.spawn().map_err(|error| {
        let failure = if body_free_diagnostics {
            AikitError::new("mux.command_spawn_failed", "Could not start selected command")
                .with("argument_count", argv.len().saturating_sub(1).to_string())
                .with("io_kind", format!("{:?}", error.kind()))
                .with("raw_os_error", error.raw_os_error().map_or_else(|| "none".into(), |n| n.to_string()))
        } else {
            AikitError::new("mux.command_spawn_failed", format!("could not run `{}`: {error}", argv.join(" ")))
                .with("command", argv.join(" ")).with("program", argv.first().cloned().unwrap_or_default())
        };
        failure.with("execution_started", "false").with_io_source(error)
    })?;
    let mut owned = OwnedCommand {
        child, cleanup_attempted: false, reaped_status: None, ownership_lost: false,
    };
    let mut stdout_pipe = owned.child.stdout.take().expect("stdout configured as piped");
    let mut stderr_pipe = owned.child.stderr.take().expect("stderr configured as piped");
    let mut stdout = Vec::new(); let mut stderr = Vec::new();
    let mut stdout_eof = false; let mut stderr_eof = false;
    let nonblocking = |fd| {
        rustix::fs::fcntl_getfl(fd).and_then(|flags| {
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
        }).map_err(std::io::Error::from)
    };
    // Borrow the actual held descriptors; never reopen a pipe by pathname.
    use std::os::fd::AsFd;
    if let Err(error) = nonblocking(stdout_pipe.as_fd()).and_then(|_| nonblocking(stderr_pipe.as_fd())) {
        let cleanup = owned.cleanup(Instant::now() + Duration::from_secs(2));
        return Err(command_failure(capture_io("nonblocking", error, body_free_diagnostics), argv, cleanup, stdout, stderr, None, policy));
    }
    let deadline = budget.and_then(|duration| Instant::now().checked_add(duration));
    if budget.is_some() && deadline.is_none() {
        let cleanup = owned.cleanup(Instant::now() + Duration::from_secs(2));
        return Err(command_failure(AikitError::new("mux.command_timeout_invalid", "Command deadline overflow"),
            argv, cleanup, stdout, stderr, None, policy));
    }
    let status = loop {
        let stdout_progress = match read_available(&mut stdout_pipe, &mut stdout, &mut stdout_eof, limit, &mut line_feeds, line_feed_limit) {
            Ok(progress) => progress,
            Err(failure) => {
                let cleanup = owned.cleanup(Instant::now() + Duration::from_secs(2));
                return Err(command_failure(capture_read_error("stdout", failure, limit, body_free_diagnostics)
                    .with("stdout_eof", stdout_eof.to_string()).with("stderr_eof", stderr_eof.to_string()),
                    argv, cleanup, stdout, stderr, None, policy));
            }
        };
        let stderr_progress = match read_available(&mut stderr_pipe, &mut stderr, &mut stderr_eof, limit, &mut line_feeds, line_feed_limit) {
            Ok(progress) => progress,
            Err(failure) => {
                let cleanup = owned.cleanup(Instant::now() + Duration::from_secs(2));
                return Err(command_failure(capture_read_error("stderr", failure, limit, body_free_diagnostics)
                    .with("stdout_eof", stdout_eof.to_string()).with("stderr_eof", stderr_eof.to_string()),
                    argv, cleanup, stdout, stderr, None, policy));
            }
        };
        match owned.observe_exit() {
            Ok(Some(status)) => break status,
            Ok(None) => {},
            Err(error) => {
                let cleanup = owned.cleanup(Instant::now() + Duration::from_secs(2));
                return Err(command_failure(capture_io("wait", error, body_free_diagnostics), argv, cleanup, stdout, stderr, None, policy));
            }
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let cleanup = owned.cleanup(Instant::now() + Duration::from_secs(2));
            return Err(command_failure(AikitError::new("mux.command_timeout",
                if body_free_diagnostics {
                    format!("Selected command did not finish within {:?}", budget.unwrap_or_default())
                } else { format!("`{}` did not finish within {:?}", argv.join(" "), budget.unwrap_or_default()) }),
                argv, cleanup, stdout, stderr, None, policy));
        }
        if stdout_progress || stderr_progress { std::thread::yield_now(); }
        else { std::thread::sleep(Duration::from_millis(5)); }
    };
    // Keep the exited leader unreaped while deciding whether live inherited
    // pipes require a group effect. Completed EOF needs only the native reap.
    let retirement_deadline = Instant::now() + Duration::from_secs(2);
    let retirement_failure = |failure: AikitError, stdout_eof: bool, stderr_eof: bool, cancelled: bool| {
        if strict_utf8 {
            failure.with("stdout_eof", stdout_eof.to_string()).with("stderr_eof", stderr_eof.to_string())
                .with("capture_cancelled", cancelled.to_string()).with("capture_encoding", "diagnostic-lossy")
        } else { failure }
    };
    let (cleanup, capture_cancelled) = loop {
        let stdout_progress = match read_available(&mut stdout_pipe, &mut stdout, &mut stdout_eof, limit, &mut line_feeds, line_feed_limit) {
            Ok(progress) => progress,
            Err(failure) => {
                let cleanup = owned.cleanup(retirement_deadline);
                return Err(command_failure(retirement_failure(capture_read_error("stdout", failure, limit, body_free_diagnostics),
                    stdout_eof, stderr_eof, true), argv, cleanup, stdout, stderr, Some(status), policy));
            }
        };
        let stderr_progress = match read_available(&mut stderr_pipe, &mut stderr, &mut stderr_eof, limit, &mut line_feeds, line_feed_limit) {
            Ok(progress) => progress,
            Err(failure) => {
                let cleanup = owned.cleanup(retirement_deadline);
                return Err(command_failure(retirement_failure(capture_read_error("stderr", failure, limit, body_free_diagnostics),
                    stdout_eof, stderr_eof, true), argv, cleanup, stdout, stderr, Some(status), policy));
            }
        };
        if stdout_eof && stderr_eof { break (owned.reap_without_signal(retirement_deadline), false); }
        // Strict semantic capture allows actual late bytes and natural EOF
        // through the SAME finite retirement interval. Generic lossy tools
        // retain their existing idle-pipe cleanup compatibility.
        if Instant::now() >= retirement_deadline || (!strict_utf8 && !stdout_progress && !stderr_progress) {
            break (owned.cleanup(retirement_deadline), true);
        }
        if stdout_progress || stderr_progress { std::thread::yield_now(); }
        else { std::thread::sleep(Duration::from_millis(5)); }
    };
    if cleanup.error.is_some() || !cleanup.reaped {
        return Err(command_failure(retirement_failure(AikitError::new("mux.command_cancellation_failed",
            "Actual command cleanup was not established"), stdout_eof, stderr_eof, capture_cancelled),
            argv, cleanup, stdout, stderr, Some(status), policy));
    }
    while !stdout_eof || !stderr_eof {
        let stdout_progress = match read_available(&mut stdout_pipe, &mut stdout, &mut stdout_eof, limit, &mut line_feeds, line_feed_limit) {
            Ok(progress) => progress,
            Err(failure) => return Err(command_failure(retirement_failure(capture_read_error("stdout", failure, limit, body_free_diagnostics),
                stdout_eof, stderr_eof, capture_cancelled), argv, cleanup, stdout, stderr, Some(status), policy)),
        };
        let stderr_progress = match read_available(&mut stderr_pipe, &mut stderr, &mut stderr_eof, limit, &mut line_feeds, line_feed_limit) {
            Ok(progress) => progress,
            Err(failure) => return Err(command_failure(retirement_failure(capture_read_error("stderr", failure, limit, body_free_diagnostics),
                stdout_eof, stderr_eof, capture_cancelled), argv, cleanup, stdout, stderr, Some(status), policy)),
        };
        if stdout_eof && stderr_eof { break; }
        if Instant::now() >= retirement_deadline {
            return Err(command_failure(retirement_failure(AikitError::new("mux.command_capture_incomplete",
                "Command inherited pipes did not retire within the finite capture bound"),
                stdout_eof, stderr_eof, capture_cancelled), argv, cleanup, stdout, stderr, Some(status), policy));
        }
        if stdout_progress || stderr_progress { std::thread::yield_now(); }
        else { std::thread::sleep(Duration::from_millis(5)); }
    }
    if strict_utf8 && capture_cancelled {
        return Err(command_failure(retirement_failure(AikitError::new("mux.command_capture_cancelled",
            "Unfinished inherited output required cancellation; no complete semantic receipt is returned"),
            stdout_eof, stderr_eof, true), argv, cleanup, stdout, stderr, Some(status), policy));
    }
    if strict_utf8 {
        // Utf8Error owns its observation; no stream borrow survives into the
        // failure path that transfers the original vectors to private evidence.
        let invalid = std::str::from_utf8(&stdout).err().map(|cause| ("stdout", cause))
            .or_else(|| std::str::from_utf8(&stderr).err().map(|cause| ("stderr", cause)));
        if let Some((stream, cause)) = invalid {
            let failure = AikitError::new("mux.command_utf8_invalid",
                "Actual command output is not valid UTF-8; no semantic text receipt is returned")
                .with("stream", stream).with("observation_stage", format!("{stream}_utf8"))
                .with("utf8_valid_up_to", cause.valid_up_to().to_string())
                .with("utf8_error_len", cause.error_len().map_or_else(|| "incomplete".into(), |n| n.to_string()))
                .with("capture_encoding", "diagnostic-lossy")
                .with("stdout_eof", "true").with("stderr_eof", "true").with("capture_cancelled", "false")
                .with_io_source(std::io::Error::new(std::io::ErrorKind::InvalidData, cause));
            return Err(command_failure(failure, argv, cleanup, stdout, stderr, Some(status), policy));
        }
    }
    // Also bound the returned String representation: default lossy decoding
    // can expand invalid raw bytes into three-byte replacement characters.
    let stdout_text = String::from_utf8_lossy(&stdout);
    let stderr_text = String::from_utf8_lossy(&stderr);
    if aggregate_limit.is_some_and(|capacity|
        stdout_text.len().checked_add(stderr_text.len()).is_none_or(|length| length > capacity))
    {
        let failure = AikitError::new("mux.command_output_limit",
            "Decoded command output exceeded the remaining aggregate capacity")
            .with("observation_stage", "decoded_output")
            .with("aggregate_output_limit_bytes", aggregate_limit.unwrap_or_default().to_string())
            .with("capture_encoding", "diagnostic-lossy");
        drop(stdout_text);
        drop(stderr_text);
        return Err(command_failure(failure, argv, cleanup, stdout, stderr, Some(status), policy));
    }
    let status = captured_exit_status(status, unix_signal_status);
    Ok(Output { status,
        stdout: stdout_text.into_owned(), stderr: stderr_text.into_owned() })
}

fn invocation_stream_limit(timeout: std::time::Duration, aggregate_bytes: usize) -> Result<u64> {
    if timeout.is_zero() || std::time::Instant::now().checked_add(timeout).is_none()
        || aggregate_bytes < 2
    {
        return Err(AikitError::new("mux.command_limits_invalid",
            "Invocation time must be positive and finite, with at least two aggregate output bytes")
            .with("execution_started", "false").with("remaining_timeout", format!("{timeout:?}"))
            .with("remaining_aggregate_bytes", aggregate_bytes.to_string()));
    }
    u64::try_from(aggregate_bytes / 2).map_err(|cause| {
        AikitError::new("mux.command_limits_invalid", "Invocation stream capacity does not fit this platform")
            .with("execution_started", "false").with("remaining_aggregate_bytes", aggregate_bytes.to_string())
            .with_io_source(std::io::Error::new(std::io::ErrorKind::InvalidInput, cause))
    })
}

impl CommandRunner for SystemRunner {
    fn configured_timeout(&self) -> Option<std::time::Duration> {
        self.timeout
    }

    fn run(&self, argv: &[String]) -> Result<Output> {
        let Some((program, args)) = argv.split_first() else {
            return Err(AikitError::new(
                "mux.empty_command",
                "an empty command was submitted to the runner",
            ));
        };

        let mut command = std::process::Command::new(program);
        command.args(args);
        self.configure_command(&mut command);
        self.spawn_bounded(&mut command, argv, self.timeout)
    }

    fn run_with_timeout(&self, argv: &[String], timeout: std::time::Duration) -> Result<Output> {
        let Some((program, args)) = argv.split_first() else {
            return Err(AikitError::new(
                "mux.empty_command",
                "an empty command was submitted to the runner",
            ));
        };

        let mut command = std::process::Command::new(program);
        command.args(args);
        self.configure_command(&mut command);
        // The caller's budget wins over the construction-time one: the request
        // knows how expensive this particular command is expected to be.
        self.spawn_bounded(&mut command, argv, Some(timeout))
    }

    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        let timeout = self.timeout.map_or(remaining_timeout, |configured| configured.min(remaining_timeout));
        let stream_limit = invocation_stream_limit(timeout, remaining_aggregate_bytes)?;
        let mut bounded = self.clone();
        bounded.timeout = Some(timeout);
        bounded.output_limit_bytes = Some(self.output_limit_bytes().min(stream_limit));
        bounded.aggregate_output_limit_bytes = Some(self.aggregate_output_limit_bytes
            .map_or(remaining_aggregate_bytes, |configured| configured.min(remaining_aggregate_bytes)));
        bounded.strict_utf8 |= strict_utf8;
        bounded.run(argv)
    }
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Passes every call through to an inner runner and remembers the argv.
///
/// This is what makes an argv assertion and a real-binary test the *same* test
/// rather than two tests that can drift apart.
pub struct RecordingRunner<R> {
    inner: R,
    calls: Mutex<Vec<Vec<String>>>,
}

impl<R> RecordingRunner<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        recorded(&self.calls)
    }

    /// Every recorded call rendered as one line, for readable assertions.
    pub fn call_lines(&self) -> Vec<String> {
        self.calls().iter().map(|c| c.join(" ")).collect()
    }

    /// Did any recorded call contain this subcommand?
    pub fn ran(&self, subcommand: &str) -> bool {
        self.calls()
            .iter()
            .any(|c| c.iter().any(|a| a == subcommand))
    }

    pub fn inner(&self) -> &R {
        &self.inner
    }
}

impl<R: CommandRunner> CommandRunner for RecordingRunner<R> {
    fn run(&self, argv: &[String]) -> Result<Output> {
        record(&self.calls, argv);
        self.inner.run(argv)
    }

    fn run_with_timeout(&self, argv: &[String], timeout: std::time::Duration) -> Result<Output> {
        record(&self.calls, argv);
        self.inner.run_with_timeout(argv, timeout)
    }

    fn configured_timeout(&self) -> Option<std::time::Duration> {
        self.inner.configured_timeout()
    }

    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        record(&self.calls, argv);
        self.inner.run_with_limits(argv, remaining_timeout, remaining_aggregate_bytes, strict_utf8)
    }
}

// ---------------------------------------------------------------------------
// Scripting
// ---------------------------------------------------------------------------

/// Answers from recorded responses, for contract tests against a binary that may
/// not be installed.
///
/// An *unscripted* command is an error rather than empty success: a contract test
/// whose adapter quietly got `""` back for a command nobody recorded is a test
/// that asserts nothing.
#[derive(Default)]
pub struct ScriptedRunner {
    responses: Vec<(String, Vec<Output>)>,
    /// How many times each recorded pattern has already answered, so a sequence
    /// can hand out successive values.
    consumed: Mutex<BTreeMap<usize, usize>>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl ScriptedRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful response for every command whose argv contains
    /// `pattern` as a contiguous run of arguments.
    #[must_use]
    pub fn on(mut self, pattern: &str, stdout: &str) -> Self {
        self.responses
            .push((pattern.to_string(), vec![Output::success(stdout)]));
        self
    }

    /// Record successive responses for repeated calls matching one pattern.
    ///
    /// Running past the end repeats the last response: adding a pane to a test
    /// fixture should not force every recorded sequence to be rewritten.
    #[must_use]
    pub fn sequence(mut self, pattern: &str, stdouts: &[&str]) -> Self {
        self.responses.push((
            pattern.to_string(),
            stdouts.iter().map(|s| Output::success(*s)).collect(),
        ));
        self
    }

    /// Record a failing response, including the stderr the real binary prints.
    #[must_use]
    pub fn failing(mut self, pattern: &str, status: i32, stderr: &str) -> Self {
        self.responses
            .push((pattern.to_string(), vec![Output::failure(status, stderr)]));
        self
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        recorded(&self.calls)
    }

    pub fn call_lines(&self) -> Vec<String> {
        self.calls().iter().map(|c| c.join(" ")).collect()
    }
}

impl CommandRunner for ScriptedRunner {
    fn run_with_limits(
        &self, argv: &[String], remaining_timeout: std::time::Duration,
        remaining_aggregate_bytes: usize, strict_utf8: bool,
    ) -> Result<Output> {
        let per_stream = invocation_stream_limit(remaining_timeout, remaining_aggregate_bytes)?;
        let _ = strict_utf8; // String responses are already valid UTF-8.
        let output = self.run(argv)?;
        if output.stdout.len() as u64 > per_stream || output.stderr.len() as u64 > per_stream {
            return Err(AikitError::new("mux.command_output_limit",
                "Recorded response exceeded its per-stream invocation capacity")
                .with("capture_kind", "scripted").with("execution_started", "false")
                .with("remaining_aggregate_bytes", remaining_aggregate_bytes.to_string()));
        }
        // This is explicit in-memory contract compatibility, not proof that a
        // real process deadline or capture lifecycle has been enforced.
        Ok(output)
    }

    fn run(&self, argv: &[String]) -> Result<Output> {
        record(&self.calls, argv);
        let line = argv.join(" ");

        // Longest pattern first, so `new-surface --type browser` beats
        // `new-surface` regardless of the order they were recorded in.
        let best = self
            .responses
            .iter()
            .enumerate()
            .filter(|(_, (pattern, _))| line.contains(pattern.as_str()))
            .max_by_key(|(_, (pattern, _))| pattern.len());

        match best {
            Some((index, (_, outputs))) => {
                let mut consumed = self.consumed.lock().unwrap_or_else(|e| e.into_inner());
                let seen = consumed.entry(index).or_insert(0);
                let step = (*seen).min(outputs.len().saturating_sub(1));
                *seen += 1;
                outputs.get(step).cloned().ok_or_else(|| {
                    AikitError::new(
                        "mux.unscripted_command",
                        format!("the recorded response for `{line}` is empty"),
                    )
                })
            }
            None => Err(AikitError::new(
                "mux.unscripted_command",
                format!("no recorded response for `{line}`"),
            )
            .with("command", line)),
        }
    }
}

// ---------------------------------------------------------------------------
// Shared recording plumbing
// ---------------------------------------------------------------------------

/// A poisoned recording mutex means a test thread panicked while holding it. The
/// call log is append-only data, so recovering it is strictly better than turning
/// one failure into a cascade of unrelated ones.
fn record(log: &Mutex<Vec<Vec<String>>>, argv: &[String]) {
    log.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(argv.to_vec());
}

fn recorded(log: &Mutex<Vec<Vec<String>>>) -> Vec<Vec<String>> {
    log.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn native_command_tempdir() -> tempfile::TempDir {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ProjectCentral/now/tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        tempfile::Builder::new().prefix("runner-owned-").tempdir_in(&scratch).unwrap()
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_script_lf_capacity_refuses_zero_before_spawn_and_keeps_generic_default() {
        let root = native_command_tempdir();
        let sentinel = root.path().join("must-not-execute");
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "printf launched > \"$1\"", "script-capacity"])
            .arg(&sentinel);
        let failure = SystemRunner::new().with_capture_line_feed_limit(0)
            .capture_command(&mut command).unwrap_err();
        assert_eq!(failure.code(), "mux.command_output_limit_invalid");
        assert_eq!(failure.details()["execution_started"], "false");
        assert_eq!(failure.details()["observation_stage"], "line_projection_capacity");
        assert!(!sentinel.exists());
        let output = SystemRunner::new().with_strict_utf8().run(&[
            "/bin/sh".into(), "-c".into(),
            "awk 'BEGIN { for (i=0;i<65537;i++) print \"\" }'".into(),
        ]).unwrap();
        assert_eq!(output.status, 0);
        assert_eq!(output.stdout.bytes().filter(|byte| *byte == b'\n').count(), 65_537,
            "generic capture has no script LF policy");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_script_policy_survives_limited_runner_clone_and_status_mapping() {
        let runner = SystemRunner::new().with_capture_line_feed_limit(1)
            .with_unix_signal_status().with_strict_utf8().with_body_free_diagnostics();
        let output = runner.run_with_limits(&[
            "/bin/sh".into(), "-c".into(), "printf 'out'; printf '\\n' >&2; exit 7".into(),
        ], std::time::Duration::from_secs(5), 1024, true).unwrap();
        assert_eq!(output.stdout, "out"); assert_eq!(output.stderr, "\n");
        assert_eq!(output.status, 7);
        let failure = runner.run_with_limits(&[
            "/bin/sh".into(), "-c".into(), "printf '\\n'; printf '\\n' >&2".into(),
        ], std::time::Duration::from_secs(5), 1024, true).unwrap_err();
        assert_eq!(failure.code(), "mux.command_output_limit", "{failure:?}");
        assert_eq!(failure.details()["observation_stage"], "line_projection_capacity");
        assert_eq!(failure.details()["observed_line_feeds"], "2");
        assert!(!failure.details().contains_key("captured_stdout"));
        assert!(!failure.details().contains_key("command"));
        assert!(failure.details().contains_key("captured_stdout_bytes"));
        let capture = failure.native_capture().expect("actual retained LF capture");
        assert_eq!(capture.stdout.len() + capture.stderr.len(), 1);
        assert_eq!(capture.stdout.iter().chain(&capture.stderr).copied().collect::<Vec<_>>().as_slice(), b"\n");
        assert_eq!(failure.details()["captured_stdout_bytes"], capture.stdout.len().to_string());
        assert_eq!(failure.details()["captured_stderr_bytes"], capture.stderr.len().to_string());
        assert_eq!(capture.status.map(|status| status.to_string()),
            failure.details().get("known_exit_status").or_else(|| failure.details().get("cleanup_exit_status")).cloned());
        let cloned = failure.clone();
        let wrapped = AikitError::new("mux.command_capture_failed", "Selected command refused")
            .with_io_source_from(&failure);
        assert!(std::ptr::eq(capture, cloned.native_capture().unwrap()));
        assert!(std::ptr::eq(capture, wrapped.native_capture().unwrap()));
        let output = runner.run_with_limits(&[
            "/bin/sh".into(), "-c".into(), "kill -TERM $$".into(),
        ], std::time::Duration::from_secs(5), 1024, true).unwrap();
        assert_eq!(output.status, 143);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_body_free_diagnostics_keep_generic_compatibility_and_original_decoder_cause() {
        use std::error::Error;
        let argv = vec!["/bin/sh".into(), "-c".into(),
            "printf '%s' \"$1\"; printf 'private-stderr-canary' >&2; sleep 30".into(),
            "native-diagnostic-test".into(), "private-native-input-canary".into()];
        let generic = SystemRunner::new().with_strict_utf8()
            .with_timeout(std::time::Duration::from_secs(1)).run(&argv).unwrap_err();
        assert_eq!(generic.code(), "mux.command_timeout");
        assert_eq!(generic.details()["captured_stdout"], "private-native-input-canary");
        assert_eq!(generic.details()["captured_stderr"], "private-stderr-canary");
        assert!(generic.to_string().contains("private-native-input-canary"));
        let selected = SystemRunner::new().with_strict_utf8().with_body_free_diagnostics()
            .with_timeout(std::time::Duration::from_secs(1)).run(&argv).unwrap_err();
        assert_eq!(selected.code(), generic.code());
        assert_eq!(selected.details()["captured_stdout_bytes"], "private-native-input-canary".len().to_string());
        assert_eq!(selected.details()["captured_stderr_bytes"], "private-stderr-canary".len().to_string());
        assert!(!selected.details().contains_key("command"));
        assert!(!selected.to_string().contains("private-native-input-canary"));
        assert!(!format!("{selected:?}").contains("private-stderr-canary"));
        let capture = selected.native_capture().expect("actual timed-out stream observation");
        assert_eq!(capture.stdout.as_slice(), b"private-native-input-canary");
        assert_eq!(capture.stderr.as_slice(), b"private-stderr-canary");
        assert_eq!(capture.status.map(|status| status.to_string()),
            selected.details().get("known_exit_status").or_else(|| selected.details().get("cleanup_exit_status")).cloned());
        assert!(!selected.details().contains_key("known_exit_status"),
            "live-leader timeout must not fabricate a pre-cleanup successful status");
        let selected_clone = selected.clone();
        let selected_wrapped = AikitError::new("mux.command_capture_failed", "Selected command refused")
            .with_io_source_from(&selected);
        assert!(std::ptr::eq(capture, selected_clone.native_capture().unwrap()));
        assert!(std::ptr::eq(capture, selected_wrapped.native_capture().unwrap()));
        for failure in [&selected, &selected_clone, &selected_wrapped] {
            for diagnostic in [failure.to_string(), format!("{failure:?}"),
                serde_json::json!({"code":failure.code(),"message":failure.message(),
                    "details":failure.details()}).to_string()] {
                assert!(!diagnostic.contains("private-native-input-canary"));
                assert!(!diagnostic.contains("private-stderr-canary"));
            }
        }
        let invalid = SystemRunner::new().with_strict_utf8().with_body_free_diagnostics().run(&[
            "/bin/sh".into(), "-c".into(), "printf 'private-body\\377'".into(),
        ]).unwrap_err();
        assert_eq!(invalid.code(), "mux.command_utf8_invalid", "{invalid:?}");
        assert_eq!(invalid.details()["known_exit_status"], "0");
        assert_eq!(invalid.details()["captured_stdout_bytes"], "13");
        assert_eq!(invalid.details()["capture_cancelled"], "false");
        assert_eq!(invalid.source().unwrap().downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::InvalidData);
        assert!(!invalid.to_string().contains("private-body"));
        let cloned = invalid.clone();
        assert!(std::ptr::eq(invalid.source().unwrap(), cloned.source().unwrap()));
        let raw = invalid.native_capture().expect("actual completed invalid bytes");
        assert_eq!(raw.stdout.as_slice(), b"private-body\xff");
        assert!(raw.stderr.is_empty());
        assert_eq!(raw.status, Some(0));
        let wrapped = AikitError::new("mux.command_capture_failed", "Selected command refused")
            .with_io_source_from(&invalid);
        assert!(std::ptr::eq(raw, cloned.native_capture().unwrap()));
        assert!(std::ptr::eq(raw, wrapped.native_capture().unwrap()));
        assert!(std::ptr::eq(invalid.source().unwrap(), wrapped.source().unwrap()));
        for failure in [&invalid, &cloned, &wrapped] {
            for diagnostic in [failure.to_string(), format!("{failure:?}"),
                serde_json::json!({"code":failure.code(),"message":failure.message(),
                    "details":failure.details()}).to_string()] {
                assert!(!diagnostic.contains("private-body"));
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_body_free_spawn_diagnostics_omit_private_program_cwd_and_arguments() {
        use std::error::Error;
        let root = native_command_tempdir();
        let missing_program = root.path().join("private-selected-program-canary");
        let missing_cwd = root.path().join("private-selected-cwd-canary");
        let cases = [
            (SystemRunner::new().with_cwd(root.path()),
                vec![missing_program.to_str().unwrap().to_owned(), "private-argument-canary".into()]),
            (SystemRunner::new().with_cwd(&missing_cwd),
                vec!["/bin/sh".into(), "private-argument-canary".into()]),
        ];
        for (runner, argv) in cases {
            let failure = runner.with_body_free_diagnostics().run(&argv).unwrap_err();
            assert_eq!(failure.code(), "mux.command_spawn_failed", "{failure:?}");
            assert_eq!(failure.details()["execution_started"], "false");
            assert!(failure.native_capture().is_none(), "no invented capture before spawn");
            assert_eq!(failure.details()["argument_count"], "1");
            let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
            assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
            assert!(cause.raw_os_error().is_some());
            assert_eq!(failure.details()["io_kind"], "NotFound");
            for diagnostic in [failure.to_string(), format!("{failure:?}"),
                serde_json::json!({"code":failure.code(),"message":failure.message(),
                    "details":failure.details()}).to_string()]
            {
                assert!(!diagnostic.contains("private-selected-program-canary"));
                assert!(!diagnostic.contains("private-selected-cwd-canary"));
                assert!(!diagnostic.contains("private-argument-canary"));
                assert!(!diagnostic.contains(root.path().to_str().unwrap()));
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn reap_owned_child(child: &mut std::process::Child) -> std::process::ExitStatus {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(status) = child.try_wait().unwrap() { return status; }
            if std::time::Instant::now() >= deadline {
                let _ = signal_owned_child_group(child);
                panic!("owned child did not reap within the actual cancellation bound");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_owned_group_signal_and_direct_child_reap_are_separate_results() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("30").process_group(0)
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn().unwrap();
        let signal = signal_owned_child_group(&child).unwrap();
        assert!(matches!(signal, OwnedChildGroupSignal::Delivered));
        let status = reap_owned_child(&mut child);
        assert!(!status.success(), "actual killed child must not report success");
        let refusal = signal_owned_child_group(&child).unwrap_err();
        assert_eq!(refusal.raw_os_error(), Some(rustix::io::Errno::CHILD.raw_os_error()),
            "reaped ownership cannot authorise a numeric process-group signal");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn an_actual_reaped_child_refuses_group_signalling_with_original_echild() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "exit 7"]).process_group(0)
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn().unwrap();
        assert_eq!(reap_owned_child(&mut child).code(), Some(7));
        for _ in 0..2 {
            let refusal = signal_owned_child_group(&child).unwrap_err();
            assert_eq!(refusal.raw_os_error(), Some(rustix::io::Errno::CHILD.raw_os_error()));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_peek_repeatedly_observes_exit_without_reaping_or_reopening_ownership() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "exit 17"]).process_group(0);
        let child = command.spawn().unwrap();
        let mut owned = OwnedCommand {
            child, cleanup_attempted: false, reaped_status: None, ownership_lost: false,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let observed = loop {
            if let Some(status) = owned.observe_exit().unwrap() { break status; }
            assert!(std::time::Instant::now() < deadline, "actual child did not exit");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(observed.code(), Some(17));
        assert_eq!(owned.observe_exit().unwrap(), Some(observed));
        assert_eq!(owned.observe_exit().unwrap(), Some(observed));
        let reaped = owned.reap_without_signal(deadline);
        assert_eq!(reaped.status, Some(observed));
        assert!(reaped.reaped);
        assert_eq!(reaped.signal, "not-needed");
        let repeated = owned.cleanup(deadline);
        assert_eq!(repeated.status, Some(observed));
        assert_eq!(repeated.signal, "not-needed");
        let refusal = signal_owned_child_group(&owned.child).unwrap_err();
        assert_eq!(refusal.raw_os_error(), Some(rustix::io::Errno::CHILD.raw_os_error()));
        // Drop sees the cached reap and must not signal this released numeric ID.
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn repeated_actual_fast_exit_capture_keeps_real_status_and_both_streams() {
        for _ in 0..64 {
            let output = SystemRunner::new().with_timeout(std::time::Duration::from_secs(2))
                .run(&["/bin/sh".into(), "-c".into(),
                    "printf out; printf err >&2; exit 19".into()]).unwrap();
            assert_eq!(output.status, 19);
            assert_eq!(output.stdout, "out");
            assert_eq!(output.stderr, "err");
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_completed_eof_preserves_a_background_child_with_closed_streams() {
        struct StopOnDrop { stop: std::path::PathBuf, stopped: std::path::PathBuf }
        impl Drop for StopOnDrop {
            fn drop(&mut self) {
                let _ = std::fs::write(&self.stop, b"stop");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while !self.stopped.exists() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
        fn await_file(path: &std::path::Path) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !path.exists() {
                assert!(std::time::Instant::now() < deadline, "actual background control missing: {}", path.display());
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let fixture = native_command_tempdir();
        let stop = fixture.path().join("stop");
        let ready = fixture.path().join("ready");
        let request = fixture.path().join("request");
        let acknowledgment = fixture.path().join("acknowledgment");
        let stopped = fixture.path().join("stopped");
        let _control = StopOnDrop { stop: stop.clone(), stopped: stopped.clone() };
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", r#"(
            exec >/dev/null 2>/dev/null
            printf ready > "$2"
            while [ ! -e "$1" ]; do
                if [ -e "$3" ]; then printf alive > "$4"; fi
                sleep 0.02
            done
            printf stopped > "$5"
        ) &
        while [ ! -e "$2" ]; do sleep 0.02; done
        printf leader
        exit 9"#, "owned-background-fixture"])
            .arg(&stop).arg(&ready).arg(&request).arg(&acknowledgment).arg(&stopped);
        let output = SystemRunner::new().with_timeout(std::time::Duration::from_secs(5))
            .capture_command(&mut command).unwrap();
        assert_eq!(output.status, 9);
        assert_eq!(output.stdout, "leader");
        assert_eq!(output.stderr, "");
        await_file(&ready);
        assert!(!stopped.exists());
        std::fs::write(&request, b"respond after native capture returned").unwrap();
        await_file(&acknowledgment);
        assert_eq!(std::fs::read(&acknowledgment).unwrap(), b"alive");
        std::fs::write(&stop, b"stop").unwrap();
        await_file(&stopped);
        // The descendant acknowledged after capture; no post-reap PGID signal
        // is used even for fixture cleanup. Its explicit stop control owns exit.
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_external_reap_marks_ownership_lost_and_drop_preserves_live_background_group() {
        use std::os::unix::process::CommandExt;
        struct StopOnDrop { stop: std::path::PathBuf, stopped: std::path::PathBuf }
        impl Drop for StopOnDrop {
            fn drop(&mut self) {
                let _ = std::fs::write(&self.stop, b"stop");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while !self.stopped.exists() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
        fn await_file(path: &std::path::Path) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !path.exists() {
                assert!(std::time::Instant::now() < deadline, "actual background response missing");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let fixture = native_command_tempdir();
        let stop = fixture.path().join("stop");
        let ready = fixture.path().join("ready");
        let request = fixture.path().join("request");
        let acknowledgment = fixture.path().join("acknowledgment");
        let stopped = fixture.path().join("stopped");
        let _control = StopOnDrop { stop: stop.clone(), stopped: stopped.clone() };
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", r#"(
            exec >/dev/null 2>/dev/null
            printf ready > "$2"
            while [ ! -e "$1" ]; do
                if [ -e "$3" ]; then printf alive > "$4"; fi
                sleep 0.02
            done
            printf stopped > "$5"
        ) &
        while [ ! -e "$2" ]; do sleep 0.02; done
        exit 23"#, "owned-lost-ownership-fixture"])
            .arg(&stop).arg(&ready).arg(&request).arg(&acknowledgment).arg(&stopped)
            .process_group(0).stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let mut owned = OwnedCommand {
            child: command.spawn().unwrap(), cleanup_attempted: false,
            reaped_status: None, ownership_lost: false,
        };
        // Actual OS reap through std Child deliberately bypasses the wrapper's
        // cached ownership. waitid must return the kernel's ECHILD, not a stub.
        assert_eq!(reap_owned_child(&mut owned.child).code(), Some(23));
        assert!(owned.reaped_status.is_none());
        let original = owned.observe_exit().unwrap_err();
        assert_eq!(original.raw_os_error(), Some(rustix::io::Errno::CHILD.raw_os_error()));
        assert!(owned.ownership_lost);
        let cleanup = owned.cleanup(std::time::Instant::now() + std::time::Duration::from_secs(2));
        assert_eq!(cleanup.signal, "ownership-lost");
        assert!(cleanup.error.is_none() && cleanup.additional_errors.is_empty());
        assert!(!cleanup.reaped, "wrapper must not fabricate its own reap");
        drop(owned);
        std::fs::write(&request, b"respond after lost-ownership cleanup and Drop").unwrap();
        await_file(&acknowledgment);
        assert_eq!(std::fs::read(&acknowledgment).unwrap(), b"alive");
        std::fs::write(&stop, b"stop").unwrap();
        await_file(&stopped);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_leader_exit_does_not_wait_for_descendant_held_pipes() {
        let started = std::time::Instant::now();
        let output = SystemRunner::new().with_timeout(std::time::Duration::from_secs(10))
            .run(&["/bin/sh".into(), "-c".into(),
                "sleep 30 & printf 'leader-output\n'; printf 'leader-error\n' >&2; exit 3".into()]).unwrap();
        assert_eq!(output.status, 3);
        assert_eq!(output.stdout, "leader-output\n");
        assert_eq!(output.stderr, "leader-error\n");
        assert!(started.elapsed() < std::time::Duration::from_secs(5),
            "real descendant pipes cannot extend the command to their natural30s completion");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_output_capacity_failure_retains_possible_execution_effects_for_both_streams() {
        for (stream, script) in [("stdout", "printf '0123456789abcdef'"),
            ("stderr", "printf '0123456789abcdef' >&2")]
        {
            let error = SystemRunner::new().with_timeout(std::time::Duration::from_secs(2))
                .with_output_limit_bytes(8)
                .run(&["/bin/sh".into(), "-c".into(), script.into()]).unwrap_err();
            assert_eq!(error.code(), "mux.command_output_limit");
            assert_eq!(error.details().get("stream").map(String::as_str), Some(stream));
            assert_eq!(error.details().get("execution_started").map(String::as_str), Some("true"));
            assert_eq!(error.details().get("effects").map(String::as_str), Some("unknown"));
            assert_eq!(error.details().get("automatic_retry").map(String::as_str), Some("false"));
            assert_eq!(error.details().get("direct_child_reaped").map(String::as_str), Some("true"));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn invalid_capture_capacity_refuses_before_actual_command_side_effect() {
        let owned = native_command_tempdir();
        let marker = owned.path().join("must-not-be-created");
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", r#"printf effect > "$1""#, "owned-fixture"]).arg(&marker);
        let error = SystemRunner::new().with_output_limit_bytes(0).capture_command(&mut command).unwrap_err();
        assert_eq!(error.code(), "mux.command_output_limit_invalid");
        assert_eq!(error.details().get("execution_started").map(String::as_str), Some("false"));
        assert!(!marker.exists());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_transport_override_preserves_an_escaped_sixteen_mebibyte_source() {
        let owned = native_command_tempdir();
        let body = "\"".repeat(16 * 1024 * 1024);
        let transport = serde_json::to_string(&serde_json::json!({"body":body})).unwrap();
        let path = owned.path().join("actual-escaped-body.json");
        std::fs::write(&path, &transport).unwrap();
        let argv = ["/bin/cat".to_owned(), path.display().to_string()];
        let default = SystemRunner::new().with_timeout(std::time::Duration::from_secs(20)).run(&argv).unwrap_err();
        assert_eq!(default.code(), "mux.command_output_limit");
        let admitted = SystemRunner::new().with_timeout(std::time::Duration::from_secs(20))
            .with_output_limit_bytes(128 * 1024 * 1024).run(&argv).unwrap();
        assert_eq!(admitted.status, 0);
        assert_eq!(admitted.stdout, transport);
        assert_eq!(std::fs::read(&path).unwrap(), transport.as_bytes());
        // This is a real physical command-transport proof, not a native source
        // owner response or a Team/Public audience grant.
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_spawn_failure_keeps_original_io_and_no_execution() {
        use std::error::Error;
        let owned = native_command_tempdir();
        let missing = owned.path().join("absent-program");
        let error = SystemRunner::new().run(&[missing.display().to_string()]).unwrap_err();
        assert_eq!(error.code(), "mux.command_spawn_failed");
        assert_eq!(error.details().get("execution_started").map(String::as_str), Some("false"));
        let cause = error.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        assert!(cause.raw_os_error().is_some());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_strict_stdout_and_stderr_refusal_keep_decoding_cause_and_completed_lifecycle() {
        use std::error::Error;
        for (stream, script) in [("stdout", r#"printf '\377'; exit 17"#),
            ("stderr", r#"printf '\377' >&2; exit 17"#)]
        {
            let failure = SystemRunner::new().run_with_limits(
                &["/bin/sh".into(), "-c".into(), script.into()],
                std::time::Duration::from_secs(2), 64, true).unwrap_err();
            assert_eq!(failure.code(), "mux.command_utf8_invalid");
            assert_eq!(failure.details().get("stream").map(String::as_str), Some(stream));
            assert_eq!(failure.details().get("known_exit_status").map(String::as_str), Some("17"));
            assert_eq!(failure.details().get("direct_child_reaped").map(String::as_str), Some("true"));
            assert_eq!(failure.details().get("group_signal").map(String::as_str), Some("not-needed"));
            assert_eq!(failure.details().get("execution_started").map(String::as_str), Some("true"));
            assert_eq!(failure.details().get("effects").map(String::as_str), Some("unknown"));
            assert_eq!(failure.details().get("automatic_retry").map(String::as_str), Some("false"));
            assert_eq!(failure.details().get("capture_encoding").map(String::as_str), Some("diagnostic-lossy"));
            let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
            assert_eq!(cause.kind(), std::io::ErrorKind::InvalidData);
            let decoding = cause.get_ref().unwrap().downcast_ref::<std::str::Utf8Error>().unwrap();
            assert_eq!(decoding.valid_up_to(), 0);
            assert_eq!(decoding.error_len(), Some(1));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn strict_capture_builder_and_false_request_preserve_strictness_and_real_replacement_text() {
        let runner = SystemRunner::new().with_strict_utf8().with_timeout(std::time::Duration::from_secs(2));
        let valid = runner.run_with_limits(
            &["/bin/sh".into(), "-c".into(), r#"printf '\357\277\275'; exit 7"#.into()],
            std::time::Duration::from_secs(3), 16, false).unwrap();
        assert_eq!(valid.status, 7);
        assert_eq!(valid.stdout, "\u{fffd}");
        let invalid = runner.run_with_limits(
            &["/bin/sh".into(), "-c".into(), r#"printf '\377'"#.into()],
            std::time::Duration::from_secs(2), 16, false).unwrap_err();
        assert_eq!(invalid.code(), "mux.command_utf8_invalid");
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", r#"printf '\377'; exit 11"#]);
        let capture = runner.capture_command(&mut command).unwrap_err();
        assert_eq!(capture.code(), "mux.command_utf8_invalid");
        assert_eq!(capture.details().get("known_exit_status").map(String::as_str), Some("11"));
        let lossy = SystemRunner::new().with_timeout(std::time::Duration::from_secs(2))
            .run(&["/bin/sh".into(), "-c".into(), r#"printf '\377'"#.into()]).unwrap();
        assert_eq!(lossy.stdout, "\u{fffd}", "ordinary default capture remains compatible");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_strict_decoding_waits_for_the_same_held_descendant_pipe_retirement() {
        let failure = SystemRunner::new().run_with_limits(
            &["/bin/sh".into(), "-c".into(), r#"sleep 30 & printf '\377'; exit 23"#.into()],
            std::time::Duration::from_secs(2), 64, true).unwrap_err();
        match failure.code() {
            "mux.command_capture_cancelled" => {
                assert_eq!(failure.details().get("stdout_eof").map(String::as_str), Some("true"));
                assert_eq!(failure.details().get("stderr_eof").map(String::as_str), Some("true"));
            }
            "mux.command_capture_incomplete" => {
                assert!(failure.details().get("stdout_eof").map(String::as_str) == Some("false")
                    || failure.details().get("stderr_eof").map(String::as_str) == Some("false"),
                    "incomplete requires actual unclosed output at the same deadline: {failure:?}");
            }
            _ => panic!("unfinished strict capture must preserve its exact cancellation/EOF disposition: {failure:?}"),
        }
        assert_eq!(failure.details().get("known_exit_status").map(String::as_str), Some("23"));
        assert_eq!(failure.details().get("direct_child_reaped").map(String::as_str), Some("true"));
        assert_eq!(failure.details().get("group_signal").map(String::as_str), Some("delivered"));
        assert_eq!(failure.details().get("capture_cancelled").map(String::as_str), Some("true"));
        assert_eq!(failure.details().get("effects").map(String::as_str), Some("unknown"));
        assert_eq!(failure.details().get("automatic_retry").map(String::as_str), Some("false"));
        // A pre-exit invalid byte is diagnostic only once unfinished output
        // required cancellation; it cannot become a completed decode receipt.
        assert_eq!(failure.details().get("captured_stdout").map(String::as_str), Some("\u{fffd}"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn strict_capture_admits_actual_gated_late_eof_and_refuses_completed_late_invalid_text() {
        use std::error::Error;
        use rustix::process::{waitid, WaitId, WaitIdOptions};
        for invalid in [false, true] {
            let fixture = native_command_tempdir();
            let ready = fixture.path().join("ready");
            let leader = fixture.path().join("leader-pid");
            let release = fixture.path().join("release-after-real-exit");
            let payload = fixture.path().join("late-payload");
            let bytes: &[u8] = if invalid { &[0xff] } else { b"genuine-late-output" };
            std::fs::write(&payload, bytes).unwrap();
            let retained_bytes = std::fs::read(&payload).unwrap();
            let leader_for_observer = leader.clone();
            let release_for_observer = release.clone();
            let observer = std::thread::spawn(move || -> std::io::Result<()> {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let raw = loop {
                    match std::fs::read_to_string(&leader_for_observer) {
                        Ok(raw) if !raw.is_empty() => break raw,
                        Ok(_) => {},
                        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {},
                        Err(cause) => return Err(cause),
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "actual leader PID not observed"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                };
                let raw_pid = raw.parse::<i32>().map_err(|cause|
                    std::io::Error::new(std::io::ErrorKind::InvalidData, cause))?;
                let pid = rustix::process::Pid::from_raw(raw_pid).ok_or_else(||
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "fixture leader PID is not positive"))?;
                loop {
                    // Test-only non-consuming observation of the real child
                    // spawned by public capture. It never signals/reaps or
                    // infers an owner receipt from this numeric fixture fact.
                    if waitid(WaitId::Pid(pid), WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT)
                        .map_err(std::io::Error::from)?.is_some()
                    {
                        // The delay is an adverse schedule after actual exit,
                        // not a substitute for the native exit observation.
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        return std::fs::write(release_for_observer, b"release actual retained pipes");
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "actual leader exit not observed"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            });
            let mut command = std::process::Command::new("/bin/sh");
            command.args(["-c", r#"(
                printf ready > "$1"
                while [ ! -e "$3" ]; do sleep 0.02; done
                /bin/cat "$4"
            ) &
            while [ ! -e "$1" ]; do sleep 0.02; done
            printf '%s' "$$" > "$2.tmp"
            /bin/mv "$2.tmp" "$2"
            exit 7"#, "actual-late-eof-fixture"])
                .arg(&ready).arg(&leader).arg(&release).arg(&payload);
            let actual = SystemRunner::new().with_strict_utf8().with_timeout(std::time::Duration::from_secs(5))
                .capture_command(&mut command);
            // Join the finite actual observer even if capture refused, before
            // the owned fixture can disappear or the result assertion panics.
            observer.join().unwrap().unwrap();
            assert_eq!(std::fs::read(&payload).unwrap(), retained_bytes);
            if invalid {
                let failure = actual.unwrap_err();
                assert_eq!(failure.code(), "mux.command_utf8_invalid");
                assert_eq!(failure.details().get("known_exit_status").map(String::as_str), Some("7"));
                assert_eq!(failure.details().get("group_signal").map(String::as_str), Some("not-needed"));
                assert_eq!(failure.details().get("stdout_eof").map(String::as_str), Some("true"));
                assert_eq!(failure.details().get("stderr_eof").map(String::as_str), Some("true"));
                assert_eq!(failure.details().get("capture_cancelled").map(String::as_str), Some("false"));
                let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
                assert_eq!(cause.kind(), std::io::ErrorKind::InvalidData);
                assert!(cause.get_ref().unwrap().is::<std::str::Utf8Error>());
            } else {
                let output = actual.unwrap();
                assert_eq!(output.status, 7);
                assert_eq!(output.stdout.as_bytes(), retained_bytes);
                assert_eq!(output.stderr, "");
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_remaining_output_capacity_reserves_each_half_and_checks_lossy_expansion() {
        let runner = SystemRunner::new();
        let exact = runner.run_with_limits(
            &["/bin/sh".into(), "-c".into(), "printf 1234; printf 5678 >&2; exit 5".into()],
            std::time::Duration::from_secs(2), 8, true).unwrap();
        assert_eq!(exact.status, 5);
        assert_eq!(exact.stdout, "1234");
        assert_eq!(exact.stderr, "5678");
        let half = runner.run_with_limits(
            &["/bin/sh".into(), "-c".into(), "printf 12345".into()],
            std::time::Duration::from_secs(2), 8, true).unwrap_err();
        assert_eq!(half.code(), "mux.command_output_limit");
        assert_eq!(half.details().get("stream").map(String::as_str), Some("stdout"));
        assert_eq!(half.details().get("output_limit_bytes").map(String::as_str), Some("4"));
        let expanded = runner.run_with_limits(
            &["/bin/sh".into(), "-c".into(), r#"printf '\377\377'; exit 13"#.into()],
            std::time::Duration::from_secs(2), 4, false).unwrap_err();
        assert_eq!(expanded.code(), "mux.command_output_limit");
        assert_eq!(expanded.details().get("observation_stage").map(String::as_str), Some("decoded_output"));
        assert_eq!(expanded.details().get("known_exit_status").map(String::as_str), Some("13"));
        assert_eq!(expanded.details().get("direct_child_reaped").map(String::as_str), Some("true"));
        assert_eq!(expanded.details().get("effects").map(String::as_str), Some("unknown"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_invalid_invocation_limits_refuse_before_child_side_effects() {
        let fixture = native_command_tempdir();
        let marker = fixture.path().join("not-executed");
        let argv = ["/bin/sh".into(), "-c".into(), r#"printf changed > "$1""#.into(),
            "bounded-fixture".into(), marker.to_str().unwrap().to_owned()];
        for (timeout, bytes) in [(std::time::Duration::from_secs(2), 0),
            (std::time::Duration::from_secs(2), 1), (std::time::Duration::ZERO, 64),
            (std::time::Duration::MAX, 64)]
        {
            let refusal = SystemRunner::new().run_with_limits(&argv, timeout, bytes, true).unwrap_err();
            assert_eq!(refusal.code(), "mux.command_limits_invalid");
            assert_eq!(refusal.details().get("execution_started").map(String::as_str), Some("false"));
            assert!(!marker.exists());
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_remaining_limits_preserve_smaller_configuration_and_invocation_coordinates() {
        let fixture = native_command_tempdir();
        let runner = SystemRunner::new().with_cwd(fixture.path())
            .with_env("RUNNER_BOUNDED_KEEP", "retained")
            .with_env("RUNNER_BOUNDED_REMOVE", "withheld").with_env_removed("RUNNER_BOUNDED_REMOVE")
            .with_timeout(std::time::Duration::from_secs(2)).with_output_limit_bytes(1024);
        let output = runner.run_with_limits(
            &["/bin/sh".into(), "-c".into(),
                r#"pwd -P; printf '%s|%s' "$RUNNER_BOUNDED_KEEP" "${RUNNER_BOUNDED_REMOVE-unset}""#.into()],
            std::time::Duration::from_secs(5), 4096, true).unwrap();
        assert_eq!(output.stdout, format!("{}\nretained|unset", fixture.path().canonicalize().unwrap().display()));
        let capacity = SystemRunner::new().with_output_limit_bytes(3).run_with_limits(
            &["/bin/sh".into(), "-c".into(), "printf 1234".into()],
            std::time::Duration::from_secs(2), 64, true).unwrap_err();
        assert_eq!(capacity.code(), "mux.command_output_limit");
        assert_eq!(capacity.details().get("output_limit_bytes").map(String::as_str), Some("3"));
        for (configured, remaining) in [(std::time::Duration::from_millis(80), std::time::Duration::from_secs(2)),
            (std::time::Duration::from_secs(2), std::time::Duration::from_millis(80))]
        {
            let failure = SystemRunner::new().with_timeout(configured).run_with_limits(
                &["/bin/sleep".into(), "30".into()], remaining, 64, true).unwrap_err();
            assert_eq!(failure.code(), "mux.command_timeout");
            assert_eq!(failure.details().get("direct_child_reaped").map(String::as_str), Some("true"));
            assert_eq!(failure.details().get("execution_started").map(String::as_str), Some("true"));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn all_forwarding_layers_preserve_actual_strict_output_and_time_requests() {
        let boxed: Box<dyn CommandRunner + Send + Sync> = Box::new(SystemRunner::new());
        let shared = std::sync::Arc::new(boxed);
        let reference = &shared;
        let recording = RecordingRunner::new(reference);
        let object: &dyn CommandRunner = &recording;
        let invalid = ["/bin/sh".into(), "-c".into(), r#"printf '\377'"#.into()];
        assert_eq!(object.run_with_limits(&invalid, std::time::Duration::from_secs(2), 64, true)
            .unwrap_err().code(), "mux.command_utf8_invalid");
        let too_large = ["/bin/sh".into(), "-c".into(), "printf 12345".into()];
        assert_eq!(object.run_with_limits(&too_large, std::time::Duration::from_secs(2), 8, false)
            .unwrap_err().code(), "mux.command_output_limit");
        let runaway = ["/bin/sleep".into(), "30".into()];
        assert_eq!(object.run_with_limits(&runaway, std::time::Duration::from_millis(80), 64, true)
            .unwrap_err().code(), "mux.command_timeout");
        assert_eq!(recording.calls(), vec![invalid.to_vec(), too_large.to_vec(), runaway.to_vec()]);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn unsupported_runner_limits_do_not_delegate_to_an_actual_effectful_run() {
        struct LegacyNative(SystemRunner);
        impl CommandRunner for LegacyNative {
            fn run(&self, argv: &[String]) -> Result<Output> { self.0.run(argv) }
        }
        let fixture = native_command_tempdir();
        let marker = fixture.path().join("must-not-run");
        let argv = ["/bin/sh".into(), "-c".into(), r#"printf effect > "$1""#.into(),
            "legacy-native-fixture".into(), marker.to_str().unwrap().to_owned()];
        let actual = LegacyNative(SystemRunner::new().with_timeout(std::time::Duration::from_secs(2)));
        let refusal = actual.run_with_limits(&argv, std::time::Duration::from_secs(2), 128, true).unwrap_err();
        assert_eq!(refusal.code(), "mux.command_limits_unsupported");
        assert_eq!(refusal.details().get("execution_started").map(String::as_str), Some("false"));
        assert!(!marker.exists());
        assert!(actual.run(&argv).unwrap().ok(), "control uses the actual legacy native implementation");
        assert_eq!(std::fs::read(&marker).unwrap(), b"effect");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_limits_preserve_missing_program_io_cause_before_execution() {
        use std::error::Error;
        let fixture = native_command_tempdir();
        let missing = fixture.path().join("missing-program");
        let failure = SystemRunner::new().run_with_limits(&[missing.to_str().unwrap().to_owned()],
            std::time::Duration::from_secs(2), 64, true).unwrap_err();
        assert_eq!(failure.code(), "mux.command_spawn_failed");
        assert_eq!(failure.details().get("execution_started").map(String::as_str), Some("false"));
        let cause = failure.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        assert!(cause.raw_os_error().is_some());
    }

    #[test]
    fn scripted_limits_are_explicit_string_compatibility_not_native_execution_proof() {
        let runner = ScriptedRunner::new().on("query", "1234");
        let output = runner.run_with_limits(&["query".into()], std::time::Duration::from_secs(2), 8, true).unwrap();
        assert_eq!(output.stdout, "1234");
        let refused = runner.run_with_limits(&["query".into()], std::time::Duration::from_secs(2), 6, false).unwrap_err();
        assert_eq!(refused.code(), "mux.command_output_limit");
        assert_eq!(refused.details().get("capture_kind").map(String::as_str), Some("scripted"));
    }

    #[test]
    fn a_wall_clock_budget_kills_a_runaway_child_as_a_runner_error() {
        let runner = SystemRunner::new().with_timeout(std::time::Duration::from_millis(120));
        let error = runner
            .run(&["sleep".into(), "30".into()])
            .expect_err("a 30s sleep cannot finish inside a 120ms budget");
        assert_eq!(error.code(), "mux.command_timeout");
    }

    #[test]
    fn the_probe_constructor_rides_the_shared_budget_without_dropping_the_explicit_one() {
        let probe = SystemRunner::probe();
        assert_eq!(
            probe.timeout(),
            Some(aikit_core::probe::DEFAULT_PROBE_BUDGET),
            "probe() is bounded by the shared budget, never unbounded"
        );
        let explicit = SystemRunner::probe().with_timeout(std::time::Duration::from_millis(120));
        assert_eq!(
            explicit.timeout(),
            Some(std::time::Duration::from_millis(120)),
            "an explicit budget wins over the probe default"
        );
        assert_eq!(SystemRunner::new().timeout(), None);
    }

    #[test]
    fn a_child_that_finishes_inside_the_budget_keeps_its_real_status() {
        let runner = SystemRunner::new().with_timeout(std::time::Duration::from_secs(10));
        let output = runner
            .run(&["sh".into(), "-c".into(), "echo bounded && exit 3".into()])
            .expect("a fast command runs inside the budget");
        assert_eq!(output.status, 3);
        assert_eq!(output.stdout.trim_end(), "bounded");
    }

    #[test]
    fn a_withheld_environment_key_never_reaches_the_child() {
        let runner = SystemRunner::new()
            .with_env("SENTINEL", "absent")
            .with_env_removed("SENTINEL");
        let output = runner
            .run(&[
                "sh".into(),
                "-c".into(),
                "printenv SENTINEL || echo withheld".into(),
            ])
            .expect("printenv runs");
        assert_eq!(output.stdout.trim_end(), "withheld");
    }
}
