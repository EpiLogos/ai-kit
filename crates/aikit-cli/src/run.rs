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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    })
}

/// What a finished run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// The child's exit status, or 128 + signal when it was killed by a signal.
    pub status: i32,
    /// Captured lines, populated only in [`ExecMode::Capture`].
    pub output: Vec<String>,
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
    let output = base_command(command)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| spawn_error(command, e))?;
    let mut lines: Vec<String> = Vec::new();
    for stream in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(stream).lines() {
            lines.push(line.to_string());
        }
    }
    Ok(RunReport {
        status: status_code(output.status),
        output: lines,
        detached: false,
    })
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
