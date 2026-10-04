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
//! receipt, journal or read model. The spawned `Command` receives it; bounded
//! private redaction values are released when that child's diagnostic drain
//! ends. This bounds their logical lifetime, without claiming secure erasure.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use aikit_core::credential::SecretValue;
use aikit_core::{AikitError, Result};
use serde_json::Value;

use crate::agent_connection::ConnectionCommand;

const STDERR_TAIL_BYTES: usize = 16_384;
const STDERR_DRAIN_BYTES: usize = 65_536;

/// Bounded diagnostic evidence, not process quiescence or a model result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStderrDiagnostic {
    pub schema: &'static str,
    pub tail: String,
    pub observed_bytes: u64,
    pub retained_bytes: usize,
    pub truncated: bool,
    /// True only when this pipe reached EOF; never a process-cleanup claim.
    pub complete: bool,
    pub redacted_credentials: u64,
    pub capture_error: Option<String>,
}

struct StderrState {
    pipe: Option<ChildStderr>,
    tail: VecDeque<u8>,
    pending: Vec<u8>,
    secrets: Vec<SecretValue>,
    observed_bytes: u64,
    discarded_bytes: u64,
    redacted_credentials: u64,
    complete: bool,
    ended: bool,
    capture_error: Option<String>,
    capture_disabled: bool,
}

impl StderrState {
    fn retain(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.tail.len() == STDERR_TAIL_BYTES {
                self.tail.pop_front();
                self.discarded_bytes = self.discarded_bytes.saturating_add(1);
            }
            self.tail.push_back(*byte);
        }
    }

    fn sanitize(&mut self, final_bytes: bool) {
        let lookbehind = self
            .secrets
            .iter()
            .map(|secret| secret.expose().len())
            .max()
            .unwrap_or(1)
            - 1;
        let limit = if final_bytes {
            self.pending.len()
        } else {
            self.pending.len().saturating_sub(lookbehind)
        };
        let mut cursor = 0;
        let mut safe = Vec::with_capacity(limit);
        while cursor < limit {
            if let Some(secret) = self
                .secrets
                .iter()
                .find(|secret| self.pending[cursor..].starts_with(secret.expose().as_bytes()))
            {
                cursor += secret.expose().len();
                safe.extend_from_slice(b"[redacted credential]");
                self.redacted_credentials = self.redacted_credentials.saturating_add(1);
            } else if final_bytes
                && self.secrets.iter().any(|secret| {
                    secret
                        .expose()
                        .as_bytes()
                        .starts_with(&self.pending[cursor..])
                })
            {
                // A stopped capture may end between credential fragments.
                // Withhold that suffix rather than returning private bytes.
                safe.extend_from_slice(b"[redacted credential fragment]");
                self.redacted_credentials = self.redacted_credentials.saturating_add(1);
                cursor = self.pending.len();
            } else {
                safe.push(self.pending[cursor]);
                cursor += 1;
            }
        }
        self.pending.drain(..cursor);
        self.retain(&safe);
    }

    fn end_capture(&mut self) {
        self.sanitize(true);
        self.release_private_capture();
    }

    /// Retained readers need only sanitized evidence after capture ends. Drop
    /// the opaque copies and any raw pending allocation, not just their length.
    /// This does not promise that allocator or OS memory has been erased.
    fn release_private_capture(&mut self) {
        self.secrets = Vec::new();
        self.pending = Vec::new();
        self.pipe.take();
        self.ended = true;
    }

    fn drain(&mut self) {
        if self.ended {
            return;
        }
        let mut buffer = [0; 4096];
        // A continuous stderr producer cannot monopolize a failure read or
        // cancellation. The next bounded pass continues from the same pipe.
        for _ in 0..STDERR_DRAIN_BYTES / buffer.len() {
            let Some(pipe) = self.pipe.as_mut() else {
                break;
            };
            match pipe.read(&mut buffer) {
                Ok(0) => {
                    self.complete = true;
                    self.end_capture();
                    break;
                }
                Ok(bytes) => {
                    self.observed_bytes = self.observed_bytes.saturating_add(bytes as u64);
                    if !self.capture_disabled {
                        self.pending.extend_from_slice(&buffer[..bytes]);
                        self.sanitize(false);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.capture_error = Some(error.to_string());
                    self.end_capture();
                    break;
                }
            }
        }
    }

    fn diagnostic(&self) -> ProcessStderrDiagnostic {
        let tail = self.tail.iter().copied().collect::<Vec<_>>();
        ProcessStderrDiagnostic {
            schema: "aikit.process-stderr-diagnostic/v1",
            tail: String::from_utf8_lossy(&tail).into_owned(),
            observed_bytes: self.observed_bytes,
            retained_bytes: tail.len(),
            truncated: self.discarded_bytes != 0,
            complete: self.complete,
            redacted_credentials: self.redacted_credentials,
            capture_error: self.capture_error.clone(),
        }
    }
}

#[derive(Clone, Default)]
struct StderrReading(Option<Arc<Mutex<StderrState>>>);

impl StderrReading {
    fn diagnostic(&self) -> ProcessStderrDiagnostic {
        let Some(state) = &self.0 else {
            return ProcessStderrDiagnostic {
                schema: "aikit.process-stderr-diagnostic/v1",
                tail: String::new(),
                observed_bytes: 0,
                retained_bytes: 0,
                truncated: false,
                complete: false,
                redacted_credentials: 0,
                capture_error: Some(
                    "Nonblocking stderr capture unavailable on this platform".into(),
                ),
            };
        };
        match state.lock() {
            Ok(mut state) => {
                state.drain();
                state.diagnostic()
            }
            Err(_) => ProcessStderrDiagnostic {
                schema: "aikit.process-stderr-diagnostic/v1",
                tail: String::new(),
                observed_bytes: 0,
                retained_bytes: 0,
                truncated: false,
                complete: false,
                redacted_credentials: 0,
                capture_error: Some("stderr capture lock poisoned".into()),
            },
        }
    }

    fn failure(&self, error: AikitError) -> AikitError {
        error.with(
            "stderrDiagnostic",
            serde_json::to_string(&self.diagnostic()).expect("stderr diagnostic is serializable"),
        )
    }
}

struct StderrCapture {
    reading: StderrReading,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StderrCapture {
    #[cfg(unix)]
    fn start(pipe: ChildStderr, environment: Option<&ModelEnvironment>) -> Result<Self> {
        let flags = rustix::fs::fcntl_getfl(&pipe).map_err(|error| {
            AikitError::new(
                "connection.process.stderr_capture_failed",
                error.to_string(),
            )
        })?;
        rustix::fs::fcntl_setfl(&pipe, flags | rustix::fs::OFlags::NONBLOCK).map_err(|error| {
            AikitError::new(
                "connection.process.stderr_capture_failed",
                error.to_string(),
            )
        })?;
        let mut secrets = Vec::new();
        let mut capture_error = None;
        let mut total = 0usize;
        if let Some(environment) = environment {
            for (_, secret) in &environment.credentials {
                let bytes = secret.expose().as_bytes();
                total = total.saturating_add(bytes.len());
                if bytes.len() > STDERR_TAIL_BYTES
                    || total > STDERR_DRAIN_BYTES
                    || secrets.len() >= 128
                {
                    capture_error = Some("Delivered credential redaction exceeds bounded capture; stderr bytes withheld".into());
                    secrets.clear();
                    break;
                }
                if !bytes.is_empty() {
                    secrets.push(SecretValue::new(secret.expose().to_owned())?);
                }
            }
        }
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.expose().len()));
        let capture_disabled = capture_error.is_some();
        let state = Arc::new(Mutex::new(StderrState {
            pipe: Some(pipe),
            tail: VecDeque::new(),
            pending: Vec::new(),
            secrets,
            observed_bytes: 0,
            discarded_bytes: 0,
            redacted_credentials: 0,
            complete: false,
            ended: false,
            capture_error,
            capture_disabled,
        }));
        let reading = StderrReading(Some(Arc::clone(&state)));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("aikit-process-stderr".into())
            .spawn(move || loop {
                let complete = match state.lock() {
                    Ok(mut state) => {
                        state.drain();
                        if worker_stop.load(Ordering::Acquire)
                            && !state.ended
                            && state.capture_error.is_none()
                        {
                            state.capture_error = Some("Capture stopped before stderr EOF".into());
                        }
                        if state.ended || worker_stop.load(Ordering::Acquire) {
                            state.end_capture();
                        }
                        state.ended
                    }
                    Err(_) => true,
                };
                if complete || worker_stop.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            })
            .map_err(|error| {
                AikitError::new(
                    "connection.process.stderr_capture_failed",
                    error.to_string(),
                )
            })?;
        Ok(Self {
            reading,
            stop,
            thread: Some(thread),
        })
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        let failed = self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err());
        let Some(state) = &self.reading.0 else {
            return;
        };
        match state.lock() {
            Ok(mut state) => {
                if failed {
                    state.capture_error = Some("stderr capture thread failed".into());
                }
                state.end_capture();
            }
            Err(poisoned) => {
                // A panicked sanitizer cannot safely resume. Keep already
                // sanitized bytes private behind the poisoned read refusal,
                // but release its raw material and descriptor during teardown.
                let mut state = poisoned.into_inner();
                state.capture_error = Some("stderr capture lock poisoned".into());
                state.release_private_capture();
            }
        }
    }
}

impl Drop for StderrCapture {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Scoped final-child environment: the fixed allowlist a provider process
/// may see, plus the credential variables delivered at launch. `apply` clears
/// the environment first, so ambient variables — including unrelated keys —
/// never reach the child. Raw material is never serialized: the pairs remain
/// [`SecretValue`], and bounded private copies redact the child's diagnostics.
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
    stderr: StderrReading,
}

impl ConnectionProcess {
    pub fn spawn(argv: &[String], cwd: Option<&Path>) -> Result<Self> {
        let (child, stdin, stdout, stderr) = spawn_parts(argv, cwd, None)?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            argv: argv.to_vec(),
            stderr,
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
        let (child, stdin, stdout, stderr) = spawn_parts(argv, cwd, environment)?;
        let argv: Arc<Vec<String>> = Arc::new(argv.to_vec());
        Ok((
            ConnectionWriter {
                stdin: Mutex::new(stdin),
                argv: Arc::clone(&argv),
                stderr: stderr.clone(),
            },
            ConnectionReader {
                stdout: BufReader::new(stdout),
                argv: Arc::clone(&argv),
                stderr: stderr.clone(),
            },
            ConnectionControl {
                child: Arc::new(Mutex::new(child)),
                argv,
                stderr,
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
            self.stderr.failure(
                AikitError::new(
                    "connection.process.invalid_json",
                    format!(
                        "target `{}` emitted invalid JSON: {error}",
                        self.argv.join(" ")
                    ),
                )
                .with("line", line),
            )
        })
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        write_line_to(&mut self.stdin, line, &self.argv).map_err(|error| self.stderr.failure(error))
    }

    pub fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let bytes = self.stdout.read_line(&mut line).map_err(|error| {
            self.stderr.failure(AikitError::new(
                "connection.process.read_failed",
                format!("could not read from `{}`: {error}", self.argv.join(" ")),
            ))
        })?;
        if bytes == 0 {
            return Err(self.stderr.failure(AikitError::new(
                "connection.process.disconnected",
                format!("target `{}` closed stdout", self.argv.join(" ")),
            )));
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

    pub fn stderr_diagnostic(&self) -> ProcessStderrDiagnostic {
        self.stderr.diagnostic()
    }
}

/// Spawn a connection target and return its raw parts, so [`ConnectionProcess`]
/// and [`ConnectionProcess::spawn_split`] share one spawn path.
fn spawn_parts(
    argv: &[String],
    cwd: Option<&Path>,
    environment: Option<&ModelEnvironment>,
) -> Result<(OwnedChild, ChildStdin, ChildStdout, StderrReading)> {
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
    #[cfg(unix)]
    command.stderr(Stdio::piped());
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
    let child = command.spawn().map_err(|error| {
        AikitError::new(
            "connection.process.spawn_failed",
            format!("could not spawn `{}`: {error}", argv.join(" ")),
        )
        .with("command", argv.join(" "))
    })?;
    // Own the child immediately: any subsequent pipe/capture setup failure
    // uses the same group teardown and reaping guard as ordinary shutdown.
    let mut child = OwnedChild {
        child,
        terminated: None,
        stderr_capture: None,
    };
    let stdin = child.child.stdin.take().ok_or_else(|| {
        AikitError::new(
            "connection.process.stdin_unavailable",
            format!("`{}` did not expose stdin", argv.join(" ")),
        )
    })?;
    let stdout = child.child.stdout.take().ok_or_else(|| {
        AikitError::new(
            "connection.process.stdout_unavailable",
            format!("`{}` did not expose stdout", argv.join(" ")),
        )
    })?;
    #[cfg(unix)]
    let stderr = {
        let pipe = child.child.stderr.take().ok_or_else(|| {
            AikitError::new(
                "connection.process.stderr_unavailable",
                "Target did not expose stderr",
            )
        })?;
        let capture = StderrCapture::start(pipe, environment)?;
        let reading = capture.reading.clone();
        child.stderr_capture = Some(capture);
        reading
    };
    #[cfg(not(unix))]
    let stderr = StderrReading::default();
    Ok((child, stdin, stdout, stderr))
}

/// The write half of a split [`ConnectionProcess`]. Every session thread writes
/// through the same serialized stdin, so interleaved sessions never interleave
/// *bytes*.
pub struct ConnectionWriter {
    stdin: Mutex<ChildStdin>,
    argv: Arc<Vec<String>>,
    stderr: StderrReading,
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
        write_line_to(&mut stdin, line, &self.argv).map_err(|error| self.stderr.failure(error))
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
    stderr: StderrReading,
}

impl ConnectionReader {
    pub fn read_json(&mut self) -> Result<Value> {
        let line = self.read_line()?;
        serde_json::from_str(&line).map_err(|error| {
            self.stderr.failure(
                AikitError::new(
                    "connection.process.invalid_json",
                    format!(
                        "target `{}` emitted invalid JSON: {error}",
                        self.argv.join(" ")
                    ),
                )
                .with("line", line),
            )
        })
    }

    pub fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let bytes = self.stdout.read_line(&mut line).map_err(|error| {
            self.stderr.failure(AikitError::new(
                "connection.process.read_failed",
                format!("could not read from `{}`: {error}", self.argv.join(" ")),
            ))
        })?;
        if bytes == 0 {
            return Err(self.stderr.failure(AikitError::new(
                "connection.process.disconnected",
                format!("target `{}` closed stdout", self.argv.join(" ")),
            )));
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
    stderr: StderrReading,
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

    pub fn stderr_diagnostic(&self) -> ProcessStderrDiagnostic {
        self.stderr.diagnostic()
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

/// Observe this exclusively owned, unreaped child without releasing its PID.
/// ECHILD is an actual loss of ownership, never permission to signal its ID.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn peek_owned_child_exit(child: &Child) -> std::io::Result<Option<ExitStatus>> {
    use rustix::process::{waitid, WaitId, WaitIdOptions};
    use std::os::unix::process::ExitStatusExt;
    let pid = rustix::process::Pid::from_raw(child.id() as i32).expect("OS child PID is positive");
    let observed = waitid(
        WaitId::Pid(pid),
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

/// How long a group refused with EPERM is given for its mid-exit leader to
/// become waitable. The transition was measured at up to ~10ms on a heavily
/// loaded Mac; this bound only caps the wait for a leader that is not
/// actually exiting.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const LEADER_EXIT_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Retains an exited group leader until the group has been terminated. Keeping
/// it unreaped reserves its PID/PGID, so a later Drop cannot signal a reused ID.
struct OwnedChild {
    child: Child,
    terminated: Option<ExitStatus>,
    stderr_capture: Option<StderrCapture>,
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
            peek_owned_child_exit(&self.child)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            self.child.try_wait()
        }
    }

    /// Observe the leader until its exit is waitable or `grace` elapses.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn await_leader_exit(
        &mut self,
        grace: std::time::Duration,
    ) -> std::io::Result<Option<ExitStatus>> {
        let deadline = std::time::Instant::now() + grace;
        loop {
            if let Some(status) = self.poll_exit()? {
                return Ok(Some(status));
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
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
                // Keep the unreaped leader's identity reserved across the
                // bounded EOF/mid-exit window. ECHILD from either observation
                // remains an error and never authorises another signal.
                Err(rustix::io::Errno::PERM) if leader_exit_observed.is_some() => {}
                Err(rustix::io::Errno::PERM)
                    if self.await_leader_exit(LEADER_EXIT_GRACE)?.is_some() => {}
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
        if let Some(capture) = &mut self.stderr_capture {
            capture.finish();
        }
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

    /// A target that exits on its own closes stdout before the kernel makes
    /// it a waitable zombie; in that window macOS refuses the group signal
    /// with EPERM. Teardown right after the reader sees EOF — exactly what a
    /// host does when a harness dies mid-request — must confirm, not report
    /// an uncertain cleanup. The window is narrow, so the race is repeated.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn teardown_right_after_a_self_exit_eof_is_confirmed() {
        let argv = vec!["sh".into(), "-c".into(), "exit 0".into()];
        for attempt in 0..200 {
            let (writer, mut reader, control) =
                ConnectionProcess::spawn_split_with_environment(&argv, None, None).unwrap();
            assert!(reader.read_line().is_err(), "the target wrote nothing");
            if let Err(failure) = control.terminate() {
                panic!("attempt {attempt}: teardown of a self-exited target failed: {failure}");
            }
            drop(writer);
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn repeated_fast_exit_cleanup_retains_the_owned_child_and_cached_result() {
        for _ in 0..64 {
            let argv = vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'not-json\\n'; exit 49".into(),
            ];
            let mut process = ConnectionProcess::spawn(&argv, None).unwrap();
            assert_eq!(
                process.read_json().unwrap_err().code(),
                "connection.process.invalid_json"
            );
            let status = process.terminate().unwrap().unwrap();
            // Cleanup may win the race and kill the leader. Either outcome
            // must be the actual OS status, retained without signalling again.
            use std::os::unix::process::ExitStatusExt;
            assert!(status.code() == Some(49) || status.signal() == Some(9));
            assert_eq!(process.terminate().unwrap(), Some(status));
            assert!(!process.is_running().unwrap());
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn exited_leader_with_a_live_group_member_still_closes_the_native_pipe() {
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "sleep 30 & printf 'READY\\n'; exit 50".into(),
        ];
        let (writer, mut reader, control) = ConnectionProcess::spawn_split(&argv, None).unwrap();
        drop(writer);
        assert_eq!(reader.read_line().unwrap(), "READY");
        let started = std::time::Instant::now();
        control.terminate().unwrap();
        assert_eq!(
            reader.read_line().unwrap_err().code(),
            "connection.process.disconnected"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!control.is_running().unwrap());
    }

    #[cfg(unix)]
    fn stderr_detail(error: &AikitError) -> Value {
        serde_json::from_str(error.details().get("stderrDiagnostic").unwrap()).unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn a_real_child_startup_failure_retains_stderr_and_exit_status() {
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf 'native launcher refused its source seat\\n' >&2; exec 2>&-; exit 42".into(),
        ];
        let mut process = ConnectionProcess::spawn(&argv, None).unwrap();
        let error = process.read_json().unwrap_err();
        assert_eq!(error.code(), "connection.process.disconnected");
        let diagnostic = stderr_detail(&error);
        assert!(diagnostic["tail"]
            .as_str()
            .unwrap()
            .contains("native launcher refused its source seat"));
        assert_eq!(diagnostic["complete"], true);
        assert_eq!(diagnostic["captureError"], Value::Null);
        let status = process.terminate().unwrap().unwrap();
        assert_eq!(status.code(), Some(42));
        assert!(!process.is_running().unwrap());
        // Reading diagnostic bytes never changes the failed process result.
        assert_eq!(process.terminate().unwrap().unwrap().code(), Some(42));
    }

    #[test]
    #[cfg(unix)]
    fn stderr_is_drained_beyond_pipe_capacity_and_only_a_bounded_tail_is_retained() {
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "head -c 131072 /dev/zero | tr '\\000' x >&2; printf 'END_OF_NATIVE_DIAGNOSTIC\\n' >&2; exec 2>&-; exit 43".into(),
        ];
        let (writer, mut reader, control) = ConnectionProcess::spawn_split(&argv, None).unwrap();
        drop(writer);
        let error = reader.read_line().unwrap_err();
        let diagnostic = stderr_detail(&error);
        assert_eq!(error.code(), "connection.process.disconnected");
        assert!(diagnostic["observedBytes"].as_u64().unwrap() > 131_072);
        assert_eq!(
            diagnostic["retainedBytes"].as_u64(),
            Some(STDERR_TAIL_BYTES as u64)
        );
        assert_eq!(diagnostic["truncated"], true);
        assert!(diagnostic["tail"]
            .as_str()
            .unwrap()
            .ends_with("END_OF_NATIVE_DIAGNOSTIC\n"));
        assert_eq!(control.terminate().unwrap().unwrap().code(), Some(43));
    }

    #[test]
    #[cfg(unix)]
    fn delivered_credential_fragments_are_redacted_from_actual_child_failure() {
        const SECRET: &str = "native-credential-redaction-probe";
        let environment = ModelEnvironment::new()
            .with_credential("PROBE_HARNESS_API_KEY", SecretValue::new(SECRET).unwrap())
            .unwrap();
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%.7s' \"$PROBE_HARNESS_API_KEY\" >&2; sleep 0.02; printf '%s' \"${PROBE_HARNESS_API_KEY#???????}\" >&2; printf '\\nSOURCE_DENIED\\n' >&2; exec 2>&-; exit 44".into(),
        ];
        let (writer, mut reader, control) =
            ConnectionProcess::spawn_split_with_environment(&argv, None, Some(&environment))
                .unwrap();
        drop(writer);
        let error = reader.read_line().unwrap_err();
        let diagnostic = stderr_detail(&error);
        assert_eq!(diagnostic["redactedCredentials"], 1);
        assert!(diagnostic["tail"]
            .as_str()
            .unwrap()
            .contains("[redacted credential]"));
        assert!(diagnostic["tail"]
            .as_str()
            .unwrap()
            .contains("SOURCE_DENIED"));
        assert!(!error.to_string().contains(SECRET));
        assert!(!serde_json::to_string(&control.stderr_diagnostic())
            .unwrap()
            .contains(SECRET));
        assert_eq!(control.terminate().unwrap().unwrap().code(), Some(44));
    }

    #[test]
    #[cfg(unix)]
    fn retained_reader_preserves_sanitized_eof_without_private_redaction_copies() {
        const SECRET: &str = "controlled-retained-reader-credential";
        let environment = ModelEnvironment::new()
            .with_credential("PROBE_HARNESS_API_KEY", SecretValue::new(SECRET).unwrap())
            .unwrap();
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s\\n' \"$PROBE_HARNESS_API_KEY\" >&2; printf 'RETAINED_SAFE_DIAGNOSTIC\\n' >&2; exec 2>&-; exit 47".into(),
        ];
        let (writer, mut reader, control) =
            ConnectionProcess::spawn_split_with_environment(&argv, None, Some(&environment))
                .unwrap();
        let retained = reader.stderr.clone();
        drop(writer);
        let error = reader.read_line().unwrap_err();
        assert_eq!(error.code(), "connection.process.disconnected");
        let before = retained.diagnostic();
        assert!(before.complete);
        assert!(before.tail.contains("[redacted credential]"));
        assert!(before.tail.contains("RETAINED_SAFE_DIAGNOSTIC"));
        assert!(!before.tail.contains(SECRET));
        {
            let state = retained.0.as_ref().unwrap().lock().unwrap();
            assert!(state.ended);
            assert!(state.pipe.is_none());
            assert!(state.secrets.is_empty());
            assert!(state.pending.is_empty());
        }
        assert_eq!(control.terminate().unwrap().unwrap().code(), Some(47));
        drop(control);
        drop(reader);
        let after = retained.diagnostic();
        assert_eq!(after.tail, before.tail);
        assert!(after.complete);
        let state = retained.0.as_ref().unwrap().lock().unwrap();
        assert!(state.secrets.is_empty());
        assert!(state.pending.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn unsafe_redaction_size_withholds_real_stderr_instead_of_leaking_it() {
        let secret = "private-redaction-limit-probe-".repeat(1024);
        let environment = ModelEnvironment::new()
            .with_credential("PROBE_HARNESS_API_KEY", SecretValue::new(&secret).unwrap())
            .unwrap();
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s' \"$PROBE_HARNESS_API_KEY\" >&2; exec 2>&-; exit 46".into(),
        ];
        let (writer, mut reader, control) =
            ConnectionProcess::spawn_split_with_environment(&argv, None, Some(&environment))
                .unwrap();
        drop(writer);
        let error = reader.read_line().unwrap_err();
        let diagnostic = stderr_detail(&error);
        assert_eq!(diagnostic["tail"], "");
        assert_eq!(
            diagnostic["observedBytes"].as_u64(),
            Some(secret.len() as u64)
        );
        assert!(diagnostic["captureError"]
            .as_str()
            .unwrap()
            .contains("stderr bytes withheld"));
        assert!(!error.to_string().contains(&secret));
        assert_eq!(control.terminate().unwrap().unwrap().code(), Some(46));
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_of_a_real_continuous_stderr_producer_joins_capture() {
        let environment = ModelEnvironment::new()
            .with_credential(
                "PROBE_HARNESS_API_KEY",
                SecretValue::new("controlled-cancelled-capture-credential").unwrap(),
            )
            .unwrap();
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s\\n' \"$PROBE_HARNESS_API_KEY\" >&2; printf 'READY\\n'; while :; do printf 'continuous stderr\\n' >&2; done".into(),
        ];
        let (writer, mut reader, control) =
            ConnectionProcess::spawn_split_with_environment(&argv, None, Some(&environment))
                .unwrap();
        assert_eq!(reader.read_line().unwrap(), "READY");
        let capture = control.stderr.0.as_ref().unwrap().clone();
        let start = std::time::Instant::now();
        control.terminate().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(!control.is_running().unwrap());
        assert_eq!(
            reader.read_line().unwrap_err().code(),
            "connection.process.disconnected"
        );
        let state = capture.lock().unwrap();
        assert!(
            state.pipe.is_none(),
            "terminated capture must release its descriptor even while readers retain diagnostics"
        );
        assert!(state.secrets.is_empty());
        assert!(state.pending.is_empty());
        assert!(state.tail.len() <= STDERR_TAIL_BYTES);
        drop(state);
        drop(writer);
        drop(control);
    }

    #[test]
    #[cfg(unix)]
    fn actual_invalid_stdout_keeps_stderr_failure_basis() {
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf 'stderr protocol setup refused\\n' >&2; printf 'not-json\\n'; exit 45".into(),
        ];
        let mut process = ConnectionProcess::spawn(&argv, None).unwrap();
        let error = process.read_json().unwrap_err();
        assert_eq!(error.code(), "connection.process.invalid_json");
        assert!(stderr_detail(&error)["tail"]
            .as_str()
            .unwrap()
            .contains("stderr protocol setup refused"));
        process.terminate().unwrap();
    }

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
