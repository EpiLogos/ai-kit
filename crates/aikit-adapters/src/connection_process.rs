//! Real child-process transport for the existing connection adapter seam.
//!
//! `agent_connection` owns protocol semantics. This module owns only the OS
//! process and byte stream needed to exercise those semantics against a real
//! target. It deliberately does not create connection, AgentSession, Harness or
//! SessionSpace identity.
//!
//! It also owns the scoped final-child environment ([`ModelEnvironment`]): one
//! scrubbed allowlist plus the credential variables an encounter launch
//! delivers. Raw material is not serializable and is never stored in any
//! receipt, journal or read model — the environment exists only in the
//! spawned `Command`.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

use aikit_core::credential::SecretValue;
use aikit_core::{AikitError, Result};
use serde_json::Value;

use crate::agent_connection::ConnectionCommand;

/// Scoped final-child environment: the fixed allowlist a provider process
/// may see, plus the credential variables delivered at launch. `apply` clears
/// the environment first, so ambient variables — including unrelated keys —
/// never reach the child. Raw material is never serialized: the pairs live
/// only as [`SecretValue`] and land only in the spawned `Command`.
#[derive(Default)]
pub struct ModelEnvironment {
    credentials: Vec<(String, SecretValue)>,
    /// The exact installed Codex executable verified for a profile-derived
    /// ACP wrapper. Never taken from ambient CODEX_PATH.
    codex_path: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for ModelEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self
            .credentials
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        f.debug_struct("ModelEnvironment")
            .field("credentials", &names)
            .field("codex_path", &self.codex_path)
            .finish()
    }
}

impl ModelEnvironment {
    pub fn new() -> Self {
        Self::default()
    }

    /// One delivered credential. The variable name is re-checked against the
    /// shared shape law so no caller can bypass it by constructing the
    /// environment directly.
    pub fn with_credential(
        mut self,
        env_var: impl Into<String>,
        secret: SecretValue,
    ) -> Result<Self> {
        self.push_credential(env_var, secret)?;
        Ok(self)
    }

    /// Merge another environment's deliveries into this one. Every merged
    /// name passes the same shape law.
    pub fn extend(&mut self, other: ModelEnvironment) -> Result<()> {
        if let Some(path) = other.codex_path {
            self.set_codex_path(path)?;
        }
        for (env_var, secret) in other.credentials {
            self.push_credential(env_var, secret)?;
        }
        Ok(())
    }

    pub fn push_credential(
        &mut self,
        env_var: impl Into<String>,
        secret: SecretValue,
    ) -> Result<()> {
        let env_var = env_var.into();
        if env_var == "CODEX_PATH" {
            return Err(AikitError::new(
                "connection.codex_path_reserved",
                "CODEX_PATH is a native executable binding, not a credential delivery variable",
            ));
        }
        if !aikit_core::credential::valid_credential_variable(&env_var) {
            return Err(AikitError::new(
                "connection.credential_variable_invalid",
                "credential delivery needs a lawful credential variable name; environment \
                 control injection is refused",
            )
            .with("env_var", env_var));
        }
        self.credentials.push((env_var, secret));
        Ok(())
    }

    /// Bind the codex-acp wrapper to the same native Codex binary whose
    /// login was checked. This is a nonsecret executable path, not a caller
    /// supplied general environment override.
    pub fn set_codex_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if !path.is_absolute() || !path.is_file() || !is_executable_file(path) {
            return Err(AikitError::new(
                "connection.codex_path_invalid",
                "CODEX_PATH needs an absolute executable file selected by the native owner",
            ));
        }
        if self
            .codex_path
            .as_deref()
            .is_some_and(|existing| existing != path)
        {
            return Err(AikitError::new(
                "connection.codex_path_conflict",
                "Two different Codex executables cannot share one child environment",
            ));
        }
        self.codex_path = Some(path.to_path_buf());
        Ok(())
    }

    /// Whether any credential would be delivered. Callers may still apply an
    /// empty environment to scrub ambient API keys for native own-login.
    pub fn is_empty(&self) -> bool {
        self.credentials.is_empty() && self.codex_path.is_none()
    }

    pub fn apply(&self, command: &mut Command) {
        self.apply_with(command, |name| std::env::var_os(name));
    }

    fn apply_with(
        &self,
        command: &mut Command,
        mut ambient: impl FnMut(&str) -> Option<std::ffi::OsString>,
    ) {
        command.env_clear();
        for name in [
            "HOME",
            "CODEX_HOME",
            "PATH",
            "TERM",
            "LANG",
            "LC_ALL",
            "TZ",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "AIKIT_HOME",
            "AIKIT_CONTEXT_ID",
            "AIKIT_ISOLATION",
        ] {
            if let Some(value) = ambient(name) {
                command.env(name, value);
            }
        }
        for (name, value) in &self.credentials {
            command.env(name, value.expose());
        }
        if let Some(path) = &self.codex_path {
            command.env("CODEX_PATH", path);
        }
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

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
        let (child, stdin, stdout) = spawn_parts(argv, cwd, None)?;
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
        Self::spawn_split_with_environment(argv, cwd, None)
    }

    /// [`ConnectionProcess::spawn_split`] with a scoped launch environment:
    /// the child sees the scrubbed allowlist plus the delivered credential
    /// variables instead of the caller's full environment.
    pub fn spawn_split_with_environment(
        argv: &[String],
        cwd: Option<&Path>,
        environment: Option<&ModelEnvironment>,
    ) -> Result<(ConnectionWriter, ConnectionReader, ConnectionControl)> {
        let (child, stdin, stdout) = spawn_parts(argv, cwd, environment)?;
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
    environment: Option<&ModelEnvironment>,
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
    // The caller may request a scrubbed environment with no delivered key,
    // notably for Codex own-login. Absence of an environment alone means
    // ordinary inheritance.
    if let Some(environment) = environment {
        environment.apply(&mut command);
    }
    // Human native-action authority is not a provider credential. Even an
    // unscoped/login-backed harness must not inherit acceptance authority.
    command.env_remove("CENTRAL_NATIVE_TOKEN");
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
            let leader_exit_observed = self.poll_exit()?;
            match rustix::process::kill_process_group(self.pid(), rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                // A group whose leader is an unreaped zombie refuses SIGKILL
                // with EPERM on macOS even though nothing signalable remains;
                // live descendants keep the group signalable, so after the
                // leader's exit was observed an EPERM means the teardown work
                // is already done and reaping is what is left.
                Err(rustix::io::Errno::PERM) if leader_exit_observed.is_some() => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The delivered variable reaches the child under its declared name; the
    /// value itself is never printed, asserted on or persisted — presence is
    /// the fact under test.
    #[test]
    fn a_delivered_credential_reaches_the_child_and_ambient_variables_do_not() {
        let environment = ModelEnvironment::new()
            .with_credential(
                "PROBE_HARNESS_API_KEY",
                SecretValue::new("fixture-material").unwrap(),
            )
            .unwrap();
        let argv = vec![
            "sh".into(),
            "-c".into(),
            "if [ -n \"$PROBE_HARNESS_API_KEY\" ]; then echo delivered; else echo missing; fi; \
             if [ -n \"$UNRELATED_API_KEY\" ]; then echo leaked; else echo withheld; fi"
                .into(),
        ];
        let (writer, mut reader, control) =
            ConnectionProcess::spawn_split_with_environment(&argv, None, Some(&environment))
                .unwrap();
        drop(writer);
        let delivered = reader.read_line().unwrap();
        let unrelated = reader.read_line().unwrap();
        assert_eq!(delivered, "delivered");
        assert_eq!(unrelated, "withheld");
        // Drop terminates the exited child; a reaped group leader may refuse
        // an explicit signal in restricted environments, so no unwrap here.
        drop(control);
    }

    #[test]
    fn without_a_delivery_the_child_environment_is_inherited_unchanged() {
        let argv = vec![
            "sh".into(),
            "-c".into(),
            "printenv AIKIT_DELIVERY_PROBE >/dev/null && echo seen || echo absent".into(),
        ];
        let (writer, mut reader, control) =
            ConnectionProcess::spawn_split_with_environment(&argv, None, None).unwrap();
        drop(writer);
        assert_eq!(reader.read_line().unwrap(), "absent");
        // Drop terminates the exited child; a reaped group leader may refuse
        // an explicit signal in restricted environments, so no unwrap here.
        drop(control);
    }

    #[test]
    fn an_environment_with_no_credentials_is_distinct_from_no_environment() {
        assert!(ModelEnvironment::new().is_empty());
        let environment = ModelEnvironment::new()
            .with_credential(
                "PROBE_HARNESS_API_KEY",
                SecretValue::new("fixture-material").unwrap(),
            )
            .unwrap();
        assert!(!environment.is_empty());
    }

    #[test]
    fn empty_scrubbed_environment_keeps_codex_home_and_withholds_api_keys() {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "printf '%s\\n' \"$CODEX_HOME\"; if [ -n \"$OPENAI_API_KEY\" ]; then echo leaked; else echo withheld; fi",
            ])
            .env("OPENAI_API_KEY", "ambient-probe-key");
        ModelEnvironment::new().apply_with(&mut command, |name| {
            (name == "CODEX_HOME").then(|| "/isolated/native-codex-login".into())
        });
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "/isolated/native-codex-login\nwithheld\n"
        );
    }

    #[test]
    fn scoped_codex_path_reaches_a_real_child_without_ambient_override() {
        let executable = std::env::current_exe().unwrap();
        let mut environment = ModelEnvironment::new();
        environment.set_codex_path(&executable).unwrap();
        assert!(!environment.is_empty());
        let mut command = Command::new("sh");
        command
            .args(["-c", "printf '%s\\n' \"$CODEX_PATH\""])
            .env("CODEX_PATH", "/ambient/foreign-codex");
        environment.apply_with(&mut command, |_| None);
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            executable.to_str().unwrap()
        );
        assert!(ModelEnvironment::new()
            .push_credential("CODEX_PATH", SecretValue::new("not-a-path").unwrap())
            .is_err());
    }

    #[test]
    fn empty_environment_on_real_connection_process_scrubs_ambient_variables() {
        // This exercises spawn_parts, which used to drop Some(empty) and
        // inherit the full ambient environment despite an own-login plan.
        const NAME: &str = "AIKIT_CONNECTION_EMPTY_SCRUB_PROBE";
        let old = std::env::var_os(NAME);
        std::env::set_var(NAME, "ambient-present");
        let argv = vec![
            "sh".into(),
            "-c".into(),
            "if [ -n \"$AIKIT_CONNECTION_EMPTY_SCRUB_PROBE\" ]; then echo leaked; else echo withheld; fi".into(),
        ];
        let result = ConnectionProcess::spawn_split_with_environment(
            &argv,
            None,
            Some(&ModelEnvironment::new()),
        );
        match old {
            Some(value) => std::env::set_var(NAME, value),
            None => std::env::remove_var(NAME),
        }
        let (writer, mut reader, control) = result.unwrap();
        drop(writer);
        assert_eq!(reader.read_line().unwrap(), "withheld");
        drop(control);
    }

    #[test]
    fn an_unlawful_variable_name_is_refused_at_environment_construction() {
        let error = ModelEnvironment::new()
            .with_credential("PATH", SecretValue::new("fixture-material").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "connection.credential_variable_invalid");
    }

    #[test]
    fn the_environment_debug_render_names_variables_never_material() {
        let environment = ModelEnvironment::new()
            .with_credential(
                "PROBE_HARNESS_API_KEY",
                SecretValue::new("fixture-material").unwrap(),
            )
            .unwrap();
        let rendered = format!("{environment:?}");
        assert!(rendered.contains("PROBE_HARNESS_API_KEY"));
        assert!(!rendered.contains("fixture-material"));
    }
}
