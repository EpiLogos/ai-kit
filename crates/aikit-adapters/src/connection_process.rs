//! Real child-process transport for the existing connection adapter seam.
//!
//! `agent_connection` owns protocol semantics. This module owns only the OS
//! process and byte stream needed to exercise those semantics against a real
//! target. It deliberately does not create connection, AgentSession, Harness or
//! SessionSpace identity.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};

use aikit_core::{AikitError, Result};
use serde_json::Value;

use crate::agent_connection::ConnectionCommand;

/// A real stdio child process. ACP uses the JSON-line methods; classic targets
/// can use the text-line methods. Keeping both byte forms on one process owner is
/// what prevents a second connection stack from growing beside
/// `aikit.connection-adapter/v1`.
pub struct ConnectionProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    argv: Vec<String>,
}

impl ConnectionProcess {
    pub fn spawn(argv: &[String], cwd: Option<&Path>) -> Result<Self> {
        let Some((program, args)) = argv.split_first() else {
            return Err(AikitError::new(
                "connection.process.empty_argv",
                "cannot spawn a connection target from empty argv",
            ));
        };
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|error| {
            AikitError::new(
                "connection.process.spawn_failed",
                format!("could not spawn `{}`: {error}", argv.join(" ")),
            )
            .with("command", argv.join(" "))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            AikitError::new(
                "connection.process.stdin_unavailable",
                format!("`{}` did not expose stdin", argv.join(" ")),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AikitError::new(
                "connection.process.stdout_unavailable",
                format!("`{}` did not expose stdout", argv.join(" ")),
            )
        })?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            argv: argv.to_vec(),
        })
    }

    /// Execute one already-encoded ACP/JSON command on the real target.
    pub fn send_json(&mut self, command: &ConnectionCommand) -> Result<()> {
        let line = serde_json::to_string(&command.payload).map_err(|error| {
            AikitError::new(
                "connection.process.json_encode_failed",
                format!("could not encode {} command: {error}", command.operation),
            )
        })?;
        self.write_line(&line)
    }

    /// Read one complete JSON message from the target. Stable ACP stdio is
    /// newline-delimited JSON, so a line is the transport boundary, not a guess.
    pub fn read_json(&mut self) -> Result<Value> {
        let line = self.read_line()?;
        serde_json::from_str(&line).map_err(|error| {
            AikitError::new(
                "connection.process.invalid_json",
                format!("target `{}` emitted invalid JSON: {error}", self.argv.join(" ")),
            )
            .with("line", line)
        })
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        self.stdin.write_all(line.as_bytes()).map_err(|error| {
            AikitError::new(
                "connection.process.write_failed",
                format!("could not write to `{}`: {error}", self.argv.join(" ")),
            )
        })?;
        self.stdin.write_all(b"\n").map_err(|error| {
            AikitError::new(
                "connection.process.write_failed",
                format!("could not terminate line for `{}`: {error}", self.argv.join(" ")),
            )
        })?;
        self.stdin.flush().map_err(|error| {
            AikitError::new(
                "connection.process.flush_failed",
                format!("could not flush `{}` stdin: {error}", self.argv.join(" ")),
            )
        })
    }

    pub fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let bytes = self.stdout.read_line(&mut line).map_err(|error| {
            AikitError::new(
                "connection.process.read_failed",
                format!("could not read from `{}`: {error}", self.argv.join(" ")),
            )
        })?;
        if bytes == 0 {
            return Err(AikitError::new(
                "connection.process.disconnected",
                format!("target `{}` closed stdout", self.argv.join(" ")),
            ));
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    pub fn is_running(&mut self) -> Result<bool> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|error| {
                AikitError::new(
                    "connection.process.status_failed",
                    format!("could not inspect `{}`: {error}", self.argv.join(" ")),
                )
            })
    }

    /// Terminate the transport process. This says nothing about canonical
    /// AgentSession continuity; callers must use the connection capabilities and
    /// target evidence for that determination.
    pub fn terminate(&mut self) -> Result<Option<ExitStatus>> {
        if let Some(status) = self.child.try_wait().map_err(|error| {
            AikitError::new(
                "connection.process.status_failed",
                format!("could not inspect `{}`: {error}", self.argv.join(" ")),
            )
        })? {
            return Ok(Some(status));
        }
        self.child.kill().map_err(|error| {
            AikitError::new(
                "connection.process.terminate_failed",
                format!("could not terminate `{}`: {error}", self.argv.join(" ")),
            )
        })?;
        let status = self.child.wait().map_err(|error| {
            AikitError::new(
                "connection.process.wait_failed",
                format!("could not wait for `{}`: {error}", self.argv.join(" ")),
            )
        })?;
        Ok(Some(status))
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

impl Drop for ConnectionProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
