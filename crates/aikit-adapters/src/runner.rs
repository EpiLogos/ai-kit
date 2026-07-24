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
//! [`CommandRunner::run`] returns `Ok` for a command that ran and failed, and
//! `Err` only when the command could not be run at all. `tmux has-session`
//! answers a yes/no question with its exit status; an adapter that could not see
//! a non-zero status without an error would have to parse English instead.

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
}

impl<T: CommandRunner + ?Sized> CommandRunner for Box<T> {
    fn run(&self, argv: &[String]) -> Result<Output> {
        (**self).run(argv)
    }
}

impl<T: CommandRunner + ?Sized> CommandRunner for &T {
    fn run(&self, argv: &[String]) -> Result<Output> {
        (**self).run(argv)
    }
}

/// A shared runner. The stack adapter owns its layers as `Box<dyn MuxAdapter>`,
/// which means a test cannot reach back into an adapter to see what it ran —
/// unless the runner itself is shared, which is what this makes possible.
impl<T: CommandRunner + ?Sized> CommandRunner for std::sync::Arc<T> {
    fn run(&self, argv: &[String]) -> Result<Output> {
        (**self).run(argv)
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
}

impl SystemRunner {
    pub fn new() -> Self {
        Self::default()
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
}

impl CommandRunner for SystemRunner {
    fn run(&self, argv: &[String]) -> Result<Output> {
        let Some((program, args)) = argv.split_first() else {
            return Err(AikitError::new(
                "mux.empty_command",
                "an empty command was submitted to the runner",
            ));
        };

        let mut command = std::process::Command::new(program);
        command.args(args);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }

        let output = command.output().map_err(|e| {
            AikitError::new(
                "mux.command_spawn_failed",
                format!("could not run `{}`: {e}", argv.join(" ")),
            )
            .with("command", argv.join(" "))
            .with("program", program.clone())
        })?;

        Ok(Output {
            // A signalled child has no exit code. -1 is not a status any shell
            // produces, so it cannot be confused with a real one.
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
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
        self.calls().iter().any(|c| c.iter().any(|a| a == subcommand))
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
