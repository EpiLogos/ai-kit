//! Real child-process transport for the existing connection adapter seam.
//!
//! `agent_connection` owns protocol semantics. This module owns only the OS
//! process and byte stream needed to exercise those semantics against a real
//! target. It deliberately does not create connection, AgentSession, Harness or
//! SessionSpace identity.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

use aikit_core::{AikitError, Result};
use serde_json::Value;

use crate::agent_connection::ConnectionCommand;

/// A real stdio child process. ACP uses the JSON-line methods; classic targets
/// can use the text-line methods. Keeping both byte forms on one process owner is
/// what prevents a second connection stack from growing beside
/// `aikit.connection-adapter/v1`.
pub struct ConnectionProcess {
    child: OwnedChild,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    argv: Vec<String>,
}

impl ConnectionProcess {
    pub fn spawn(argv: &[String], cwd: Option<&Path>) -> Result<Self> {
        let (child, stdin, stdout) = spawn_parts(argv, cwd)?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            argv: argv.to_vec(),
        })
    }

    /// Spawn and split in one step: the write half may be shared across session
    /// threads, the read half belongs to one demultiplexing reader, and the
    /// control half keeps the ordinary process mechanisms. One process owner,
    /// two halves, no second connection stack.
    pub fn spawn_split(
        argv: &[String],
        cwd: Option<&Path>,
    ) -> Result<(ConnectionWriter, ConnectionReader, ConnectionControl)> {
        let (child, stdin, stdout) = spawn_parts(argv, cwd)?;
        let argv: Arc<Vec<String>> = Arc::new(argv.to_vec());
        Ok((
            ConnectionWriter {
                stdin: Mutex::new(stdin),
                argv: Arc::clone(&argv),
            },
            ConnectionReader {
                stdout: BufReader::new(stdout),
                argv: Arc::clone(&argv),
            },
            ConnectionControl {
                child: Arc::new(Mutex::new(child)),
                argv,
            },
        ))
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
                format!(
                    "target `{}` emitted invalid JSON: {error}",
                    self.argv.join(" ")
                ),
            )
            .with("line", line)
        })
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        write_line_to(&mut self.stdin, line, &self.argv)
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
            .poll_exit()
            .map(|status| status.is_none())
            .map_err(|error| {
                AikitError::new(
                    "connection.process.status_failed",
                    format!("could not inspect `{}`: {error}", self.argv.join(" ")),
                )
            })
    }

    /// Interrupt a real classic child without changing connection semantics into
    /// process semantics. The adapter decides that a command means `interrupt`;
    /// this transport maps that command to the host's ordinary SIGINT mechanism.
    #[cfg(unix)]
    pub fn interrupt(&mut self) -> Result<()> {
        if !self.is_running()? {
            return Err(AikitError::new(
                "connection.process.disconnected",
                format!("target `{}` is not running", self.argv.join(" ")),
            ));
        }
        let status = Command::new("kill")
            .arg("-INT")
            .arg(self.child.id().to_string())
            .status()
            .map_err(|error| {
                AikitError::new(
                    "connection.process.interrupt_failed",
                    format!("could not signal `{}`: {error}", self.argv.join(" ")),
                )
            })?;
        if !status.success() {
            return Err(AikitError::new(
                "connection.process.interrupt_failed",
                format!("SIGINT for `{}` exited with {status}", self.argv.join(" ")),
            ));
        }
        Ok(())
    }

    /// Terminate the transport process. This says nothing about canonical
    /// AgentSession continuity; callers must use the connection capabilities and
    /// target evidence for that determination.
    pub fn terminate(&mut self) -> Result<Option<ExitStatus>> {
        self.child.terminate().map(Some).map_err(|error| {
            AikitError::new(
                "connection.process.terminate_failed",
                format!("could not terminate `{}`: {error}", self.argv.join(" ")),
            )
        })
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// Spawn a connection target and return its raw parts, so [`ConnectionProcess`]
/// and [`ConnectionProcess::spawn_split`] share one spawn path.
fn spawn_parts(
    argv: &[String],
    cwd: Option<&Path>,
) -> Result<(OwnedChild, ChildStdin, ChildStdout)> {
    let Some((program, args)) = argv.split_first() else {
        return Err(AikitError::new(
            "connection.process.empty_argv",
            "cannot spawn a connection target from empty argv",
        ));
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    // A private group contains the adapter and ordinary inherited descendants.
    // It is a lifetime boundary, not a sandbox: deliberate setsid/setpgid escape
    // requires a stronger execution provider.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
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
    Ok((
        OwnedChild {
            child,
            terminated: None,
        },
        stdin,
        stdout,
    ))
}

/// The write half of a split [`ConnectionProcess`]. Every session thread writes
/// through the same serialized stdin, so interleaved sessions never interleave
/// *bytes*.
pub struct ConnectionWriter {
    stdin: Mutex<ChildStdin>,
    argv: Arc<Vec<String>>,
}

impl ConnectionWriter {
    pub fn send_json(&self, command: &ConnectionCommand) -> Result<()> {
        let line = serde_json::to_string(&command.payload).map_err(|error| {
            AikitError::new(
                "connection.process.json_encode_failed",
                format!("could not encode {} command: {error}", command.operation),
            )
        })?;
        self.write_line(&line)
    }

    pub fn write_line(&self, line: &str) -> Result<()> {
        let mut stdin = self.stdin.lock().map_err(|_| poisoned("write"))?;
        write_line_to(&mut stdin, line, &self.argv)
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// The read half of a split [`ConnectionProcess`]. Not cloneable: exactly one
/// reader consumes the target's stdout so observed wire order stays total.
pub struct ConnectionReader {
    stdout: BufReader<ChildStdout>,
    argv: Arc<Vec<String>>,
}

impl ConnectionReader {
    pub fn read_json(&mut self) -> Result<Value> {
        let line = self.read_line()?;
        serde_json::from_str(&line).map_err(|error| {
            AikitError::new(
                "connection.process.invalid_json",
                format!(
                    "target `{}` emitted invalid JSON: {error}",
                    self.argv.join(" ")
                ),
            )
            .with("line", line)
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

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// Process control for a split [`ConnectionProcess`]. Signalling and termination
/// stay host mechanisms; they say nothing about canonical AgentSession
/// continuity.
pub struct ConnectionControl {
    child: Arc<Mutex<OwnedChild>>,
    argv: Arc<Vec<String>>,
}

impl ConnectionControl {
    pub fn is_running(&self) -> Result<bool> {
        let mut child = self.child.lock().map_err(|_| poisoned("status"))?;
        child
            .poll_exit()
            .map(|status| status.is_none())
            .map_err(|error| {
                AikitError::new(
                    "connection.process.status_failed",
                    format!("could not inspect `{}`: {error}", self.argv.join(" ")),
                )
            })
    }

    /// Interrupt a real classic child without changing connection semantics into
    /// process semantics. The adapter decides that a command means `interrupt`;
    /// this transport maps that command to the host's ordinary SIGINT mechanism.
    #[cfg(unix)]
    pub fn interrupt(&self) -> Result<()> {
        // Keep ownership locked through signalling: termination must not reap
        // and release this PID between the status check and SIGINT.
        let mut child = self.child.lock().map_err(|_| poisoned("interrupt"))?;
        if child
            .poll_exit()
            .map_err(|error| {
                AikitError::new(
                    "connection.process.status_failed",
                    format!("could not inspect `{}`: {error}", self.argv.join(" ")),
                )
            })?
            .is_some()
        {
            return Err(AikitError::new(
                "connection.process.disconnected",
                format!("target `{}` is not running", self.argv.join(" ")),
            ));
        }
        let id = child.id();
        let status = Command::new("kill")
            .arg("-INT")
            .arg(id.to_string())
            .status()
            .map_err(|error| {
                AikitError::new(
                    "connection.process.interrupt_failed",
                    format!("could not signal `{}`: {error}", self.argv.join(" ")),
                )
            })?;
        if !status.success() {
            return Err(AikitError::new(
                "connection.process.interrupt_failed",
                format!("SIGINT for `{}` exited with {status}", self.argv.join(" ")),
            ));
        }
        Ok(())
    }

    pub fn terminate(&self) -> Result<Option<ExitStatus>> {
        let mut child = self.child.lock().map_err(|_| poisoned("terminate"))?;
        child.terminate().map(Some).map_err(|error| {
            AikitError::new(
                "connection.process.terminate_failed",
                format!("could not terminate `{}`: {error}", self.argv.join(" ")),
            )
        })
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

fn poisoned(operation: &str) -> AikitError {
    AikitError::new(
        "connection.process.lock_poisoned",
        format!("connection {operation} lock was poisoned by a failed session thread"),
    )
}

fn write_line_to(stdin: &mut ChildStdin, line: &str, argv: &[String]) -> Result<()> {
    stdin.write_all(line.as_bytes()).map_err(|error| {
        AikitError::new(
            "connection.process.write_failed",
            format!("could not write to `{}`: {error}", argv.join(" ")),
        )
    })?;
    stdin.write_all(b"\n").map_err(|error| {
        AikitError::new(
            "connection.process.write_failed",
            format!("could not terminate line for `{}`: {error}", argv.join(" ")),
        )
    })?;
    stdin.flush().map_err(|error| {
        AikitError::new(
            "connection.process.flush_failed",
            format!("could not flush `{}` stdin: {error}", argv.join(" ")),
        )
    })
}

/// Retains an exited group leader until the group has been terminated. Keeping
/// it unreaped reserves its PID/PGID, so a later Drop cannot signal a reused ID.
struct OwnedChild {
    child: Child,
    terminated: Option<ExitStatus>,
}

impl OwnedChild {
    fn id(&self) -> u32 {
        self.child.id()
    }

    fn poll_exit(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if let Some(status) = self.terminated {
            return Ok(Some(status));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use rustix::process::{waitid, WaitId, WaitIdOptions};
            use std::os::unix::process::ExitStatusExt;
            let observed = waitid(
                WaitId::Pid(self.pid()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )?;
            Ok(observed.map(|status| {
                if let Some(code) = status.exit_status() {
                    ExitStatus::from_raw(code << 8)
                } else {
                    ExitStatus::from_raw(status.terminating_signal().unwrap_or(0))
                }
            }))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            self.child.try_wait()
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn pid(&self) -> rustix::process::Pid {
        rustix::process::Pid::from_raw(self.child.id() as i32).expect("OS child PID is positive")
    }

    fn terminate(&mut self) -> std::io::Result<ExitStatus> {
        if let Some(status) = self.terminated {
            return Ok(status);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            // Confirm the leader is still our unreaped child before using its
            // group identity. ECHILD refuses signalling if ownership was lost.
            self.poll_exit()?;
            match rustix::process::kill_process_group(self.pid(), rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => return Err(error.into()),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            if self.child.try_wait()?.is_none() {
                self.child.kill()?;
            }
        }
        let status = self.child.wait()?;
        // Clear group ownership only after signalling and reaping; repeated
        // terminate/Drop then cannot accidentally signal a recycled PGID.
        self.terminated = Some(status);
        Ok(status)
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}
