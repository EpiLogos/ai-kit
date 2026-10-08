//! The real machine behind [`crate::gateway_upgrade`]: the gateway's own
//! carrier, the managed installer, the platform service manager, and the
//! detached worker an upgrade runs in.
//!
//! **Why the worker is not a child.** The gateway is what an upgrade
//! restarts, and an upgrade can be asked for through the gateway. A plain
//! child of the gateway dies with it: systemd's `KillMode=control-group`
//! signals every process in the unit's cgroup, and launchd stops a job's
//! remaining processes. So the worker is started under the *service manager*
//! as its own one-shot job — a LaunchAgent with no `KeepAlive`, or a transient
//! systemd unit — which a restart of the gateway does not touch. Hermes learned
//! the same lesson and records the updater's result *before* the disruptive
//! step; here the whole transaction is durable before every step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{
    gateway_command_within, GatewayCarrierTarget, GatewayCommand, GatewayLifecycle,
    GatewayResponse, OutboundOperationKind,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::{json, Value};

use crate::gateway_contact::three_part;
use crate::gateway_install::{
    start_installed_service, ServiceControl, ServiceEnvironment, ServicePlatform,
    SystemServiceControl,
};
use crate::gateway_ops::read_process_record;
use crate::gateway_upgrade::{
    default_plan, plan_line, plan_reading, write_atomic, CommandOutcome, DrainRecord, Driver,
    Identity, Installer, Mode, Running, StartAction, Store, Transaction, UpgradeEnv, UpgradeOrigin,
};

/// Start a worker as a plain detached process instead of under the service
/// manager (`process`). Only for development and tests, where no service
/// manager is stood up; the default is the manager.
pub const WORKER_MODE_ENV: &str = "AIKIT_UPGRADE_WORKER_MODE";

fn unix_socket_target(home: &AikitHome) -> GatewayCarrierTarget {
    GatewayCarrierTarget::UnixSocket(home.gateway_socket())
}

/// The real machine.
pub struct SystemEnv {
    pub home: AikitHome,
    pub home_dir: PathBuf,
    control: Box<dyn ServiceControl>,
}

impl SystemEnv {
    pub fn new(home: AikitHome) -> Result<Self> {
        let home_dir = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            AikitError::new(
                "gateway_upgrade.home_unresolved",
                "no HOME is set; the gateway service definition cannot be located",
            )
        })?;
        Ok(Self {
            home,
            home_dir,
            control: Box::new(SystemServiceControl),
        })
    }

    pub fn store(&self) -> Store {
        Store::new(&self.home.state())
    }

    /// The executable the service definition starts (what a supervisor would
    /// exec after a restart), else `aikit` on PATH.
    fn service_executable(&self) -> Option<PathBuf> {
        if let Ok(platform) = ServicePlatform::current() {
            if let Ok(definition) = std::fs::read_to_string(platform.unit_path(&self.home_dir)) {
                if let Some(path) = executable_named_by(&definition, platform) {
                    return Some(path);
                }
            }
        }
        crate::probe::which("aikit")
    }

    /// The pid recorded by the gateway's state lock, for a gateway that does
    /// not yet report its own (one built before build identity).
    fn pid_from_state_lock(&self) -> Option<u32> {
        let lock = self.home.gateway_state();
        let mut name = lock.file_name()?.to_os_string();
        name.push(".lock");
        let text = std::fs::read_to_string(lock.with_file_name(name)).ok()?;
        text.split_whitespace()
            .skip_while(|word| *word != "pid")
            .nth(1)
            .and_then(|pid| pid.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok())
    }
}

/// The executable a service definition runs: launchd's first
/// `ProgramArguments` string, systemd's `ExecStart` program.
pub fn executable_named_by(definition: &str, platform: ServicePlatform) -> Option<PathBuf> {
    match platform {
        ServicePlatform::LaunchAgent => {
            let after = definition.split("<key>ProgramArguments</key>").nth(1)?;
            let first = after.split("<string>").nth(1)?.split("</string>").next()?;
            Some(PathBuf::from(first.trim()))
        }
        ServicePlatform::SystemdUser => definition
            .lines()
            .find_map(|line| line.trim().strip_prefix("ExecStart="))
            .and_then(|command| command.split_whitespace().next())
            .map(|program| PathBuf::from(program.trim_matches('"'))),
    }
}

fn revision_from_version_line(line: &str) -> Option<String> {
    let open = line.find('(')?;
    let close = line[open..].find(')')? + open;
    let inside = line[open + 1..close].trim();
    (!inside.is_empty()).then(|| inside.to_owned())
}

/// Identify an executable: its real path, digest, and the revision its own
/// `--version` reports.
pub fn identify_executable(path: &Path) -> Option<Identity> {
    let resolved = std::fs::canonicalize(path).ok()?;
    let sha256 = aikit_adapters::sha256_of_file(&resolved);
    let revision = std::process::Command::new(&resolved)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(revision_from_version_line)
        })
        .unwrap_or_else(|| "unknown".into());
    Some(Identity {
        revision,
        executable_sha256: sha256,
        executable_path: Some(resolved.display().to_string()),
    })
}

fn running_from_posture(
    features: Vec<String>,
    posture: Option<crate::gateway_ops::GatewayProcessPosture>,
    fallback_pid: Option<u32>,
    installed_service: bool,
) -> Option<Running> {
    match posture {
        // The running process published its own posture (the serve arm writes
        // it, and refreshes it once the executable digest has been read): a
        // fact about the process, named by the process.
        Some(posture) => Some(Running {
            pid: posture.build.pid,
            started_at_unix_ms: posture.build.started_at_unix_ms,
            identity: Identity {
                revision: posture.build.revision.clone(),
                executable_sha256: posture.build.executable_sha256.clone(),
                executable_path: posture.build.executable_path.clone(),
            },
            lifecycle: posture.build.lifecycle,
            workcell_ref: posture.build.workcell_ref,
            features,
        }),
        // A gateway that has published no posture record (one built before the
        // record existed): it is running, it cannot say what, and its pid is
        // on its state lock. Its lifecycle is read from whether a service
        // definition stands behind it.
        None => fallback_pid.map(|pid| Running {
            pid,
            started_at_unix_ms: 0,
            identity: Identity {
                revision: "unknown".into(),
                executable_sha256: None,
                executable_path: None,
            },
            lifecycle: if installed_service {
                match ServicePlatform::current() {
                    Ok(ServicePlatform::LaunchAgent) => GatewayLifecycle::SupervisedLaunchd,
                    Ok(ServicePlatform::SystemdUser) => GatewayLifecycle::SupervisedSystemd,
                    Err(_) => GatewayLifecycle::Foreground,
                }
            } else {
                GatewayLifecycle::Foreground
            },
            workcell_ref: None,
            features,
        }),
    }
}

impl UpgradeEnv for SystemEnv {
    fn now_unix_ms(&self) -> u64 {
        aikit_adapters::gateway_posture::unix_ms_now()
    }

    fn read_running(&self) -> Result<Option<Running>> {
        let target = unix_socket_target(&self.home);
        match gateway_command_within(
            &target,
            GatewayCommand::Protocol,
            None,
            Duration::from_secs(3),
        ) {
            Ok(GatewayResponse::Protocol { features, .. }) => Ok(running_from_posture(
                features,
                read_process_record(&self.home),
                self.pid_from_state_lock(),
                crate::gateway_install::is_installed(&self.home_dir),
            )),
            Ok(_) => Ok(None),
            // Nothing is listening (refused, no socket) or it did not answer
            // in time: no gateway is answering.
            Err(_) => Ok(None),
        }
    }

    fn read_running_strict(&self) -> Result<Option<Running>> {
        let target = unix_socket_target(&self.home);
        let mut last = String::new();
        for attempt in 0..4 {
            match gateway_command_within(
                &target,
                GatewayCommand::Protocol,
                None,
                Duration::from_secs(3),
            ) {
                Ok(GatewayResponse::Protocol { features, .. }) => {
                    return Ok(running_from_posture(
                        features,
                        read_process_record(&self.home),
                        self.pid_from_state_lock(),
                        crate::gateway_install::is_installed(&self.home_dir),
                    ))
                }
                Ok(_) => return Ok(None),
                Err(error) => {
                    let text = error.to_string();
                    // Nothing is listening: no socket, or a stale one.
                    let not_listening = !self.home.gateway_socket().exists()
                        || text.contains("Connection refused")
                        || text.contains("No such file")
                        || text.contains("os error 61")
                        || text.contains("os error 111")
                        || text.contains("os error 2)");
                    if not_listening {
                        return Ok(None);
                    }
                    last = text;
                    if attempt < 3 {
                        std::thread::sleep(Duration::from_secs(2));
                    }
                }
            }
        }
        Err(AikitError::new(
            "gateway_upgrade.gateway_unresponsive",
            format!(
                "a gateway holds this home's socket and did not answer in time (it may be \
                 busy or stuck): {last}."
            ),
        ))
    }

    fn installed_identity(&self) -> Result<Option<Identity>> {
        Ok(self
            .service_executable()
            .and_then(|path| identify_executable(&path)))
    }

    fn run_installer(
        &self,
        argv: &[String],
        timeout: Duration,
        log: &Path,
    ) -> Result<CommandOutcome> {
        use std::io::Write;
        let Some((program, arguments)) = argv.split_first() else {
            return Err(AikitError::new(
                "gateway_upgrade.installer_empty",
                "the installer command is empty",
            ));
        };
        let log_file = std::fs::File::create(log).map_err(|error| {
            AikitError::new(
                "gateway_upgrade.io",
                format!("create {}: {error}", log.display()),
            )
        })?;
        let mut command = std::process::Command::new(program);
        command
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .stdout(log_file.try_clone().map_err(|error| {
                AikitError::new("gateway_upgrade.io", format!("duplicate the log: {error}"))
            })?)
            .stderr(log_file);
        let mut child = command.spawn().map_err(|error| {
            AikitError::new(
                "gateway_upgrade.installer_spawn",
                format!("could not run {program}: {error}"),
            )
        })?;
        let started = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let mut note = std::fs::OpenOptions::new().append(true).open(log).ok();
                    if let Some(file) = note.as_mut() {
                        let _ = writeln!(file, "\n[killed after {} ms]", timeout.as_millis());
                    }
                    return Ok(CommandOutcome {
                        success: false,
                        detail: format!(
                            "{program} did not finish within {} ms",
                            timeout.as_millis()
                        ),
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                Err(error) => {
                    return Err(AikitError::new(
                        "gateway_upgrade.installer_wait",
                        format!("waiting for {program}: {error}"),
                    ))
                }
            }
        };
        let tail = std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(240)
            .collect::<String>();
        Ok(CommandOutcome {
            success: status.success(),
            detail: if status.success() {
                format!("{program} exited 0 ({})", log.display())
            } else {
                format!(
                    "{program} exited {} — {tail} ({})",
                    status
                        .code()
                        .map_or("by signal".to_owned(), |c| c.to_string()),
                    log.display()
                )
            },
        })
    }

    fn drain(
        &self,
        expected_pid: u32,
        reason: &str,
        grace: Duration,
        exit: bool,
    ) -> Result<DrainRecord> {
        let _ = (reason, grace);
        // The platform's own stop, exactly what `launchctl bootout` and
        // `systemctl stop` do: a SIGTERM to the process. The gateway's serve
        // signal handler answers it with a drained, persisted, clean exit —
        // the drain is measured by the gateway that meets it, and every turn
        // it could not finish is journaled on its own stream. What this
        // transaction records is that the stop was asked of the right process;
        // its counts are the predecessor's own knowledge, not this receipt's.
        let wrong_process = self
            .pid_from_state_lock()
            .is_some_and(|pid| pid != expected_pid);
        if wrong_process {
            return Err(AikitError::new(
                "gateway_upgrade.drain_wrong_process",
                format!(
                    "the process on the state lock is not the gateway this upgrade read ({expected_pid}); \
                     it was not stopped"
                ),
            ));
        }
        if !exit {
            return Err(AikitError::new(
                "gateway_upgrade.drain_without_exit",
                "a stop that does not end the process cannot be asked of a gateway",
            ));
        }
        let output = self
            .control
            .run("kill", &["-TERM".to_owned(), expected_pid.to_string()])?;
        if !output.status.success() {
            return Err(AikitError::new(
                "gateway_upgrade.stop_refused",
                format!(
                    "the stop of pid {expected_pid} was refused: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(DrainRecord {
            measured: false,
            reason: format!(
                "{reason} (the predecessor drains itself for its stop; its own reading is on \
                 its stderr and its journal)"
            ),
            ..DrainRecord::default()
        })
    }

    fn start_service(&self, lifecycle: GatewayLifecycle) -> Result<StartAction> {
        if !lifecycle.restarts_itself() {
            return Ok(StartAction::OperatorMustStart(
                "start the gateway with the command you run it with (`aikit gateway serve …`)"
                    .into(),
            ));
        }
        match start_installed_service(self.control.as_ref(), &self.home_dir) {
            Ok(what) => Ok(StartAction::Requested(what)),
            Err(error) => Ok(StartAction::OperatorMustStart(error.to_string())),
        }
    }

    fn backup_state(&self, into: &Path) -> Result<Vec<String>> {
        let mut copied = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.home.state()) else {
            return Ok(copied);
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_gateway_state =
                name.starts_with("gateway") && name.ends_with(".json") && !name.ends_with(".lock");
            if is_gateway_state && entry.path().is_file() {
                let destination = into.join(&name);
                std::fs::copy(entry.path(), &destination).map_err(|error| {
                    AikitError::new(
                        "gateway_upgrade.io",
                        format!("copy {} for recovery: {error}", entry.path().display()),
                    )
                })?;
                copied.push(destination.display().to_string());
            }
        }
        Ok(copied)
    }

    fn announce(&self, origin: &UpgradeOrigin, text: &str) -> Result<()> {
        let binding_ref = ResourceRef::parse(&origin.binding_ref).map_err(|error| {
            AikitError::new(
                "gateway_upgrade.origin_invalid",
                format!("origin binding {}: {error}", origin.binding_ref),
            )
        })?;
        // The conversation's own connector is the delivery path: an outbound
        // send prepared on the origin binding reaches the conversation through
        // the pump that serves it (and its delivery receipt shows in the
        // gateway's snapshot). This is the current engine's surface for a line
        // that arrives from outside a turn.
        gateway_command_within(
            &unix_socket_target(&self.home),
            GatewayCommand::PrepareOperation {
                binding_ref,
                operation: OutboundOperationKind::Send {
                    text: Some(text.to_owned()),
                    media: Vec::new(),
                    reply_to_native_message_id: None,
                },
            },
            None,
            Duration::from_secs(10),
        )
        .map(|_| ())
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

// ---------------------------------------------------------------------------
// Installer resolution
// ---------------------------------------------------------------------------

/// The managed installer for this machine: `oi update --apply` for the
/// gateway's own product, with `oi update --rollback` as the supported way
/// back. `None` when `oi` is not installed (an upgrade can still restart onto
/// an already-installed build).
pub fn resolve_installer(channel: Option<&str>, candidate: Option<&str>) -> Option<Installer> {
    let oi = crate::probe::which("oi")?;
    let mut install = vec![
        oi.display().to_string(),
        "update".to_owned(),
        "--apply".to_owned(),
    ];
    if let Some(channel) = channel {
        install.extend(["--channel".to_owned(), channel.to_owned()]);
    }
    if let Some(candidate) = candidate {
        install.extend(["--candidate".to_owned(), format!("ai-kit={candidate}")]);
    }
    install.push("ai-kit".to_owned());
    Some(Installer {
        install,
        rollback: vec![
            oi.display().to_string(),
            "update".into(),
            "--rollback".into(),
        ],
        timeout_ms: 45 * 60 * 1_000,
    })
}

// ---------------------------------------------------------------------------
// The detached worker
// ---------------------------------------------------------------------------

fn worker_short_id(id: &str) -> String {
    id.trim_start_matches("upg-").chars().take(12).collect()
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A LaunchAgent plist for a one-shot worker: runs once at load, is never
/// kept alive, and logs beside its transaction.
pub fn render_worker_plist(
    label: &str,
    arguments: &[String],
    environment: &BTreeMap<String, String>,
    log: &Path,
) -> String {
    let arguments = arguments
        .iter()
        .map(|argument| format!("        <string>{}</string>", xml_escape(argument)))
        .collect::<Vec<_>>()
        .join("\n");
    let environment = environment
        .iter()
        .map(|(name, value)| {
            format!(
                "      <key>{}</key>\n      <string>{}</string>",
                xml_escape(name),
                xml_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
{arguments}
    </array>
    <key>EnvironmentVariables</key>
    <dict>
{environment}
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
  </dict>
</plist>
"#,
        label = xml_escape(label),
        log = xml_escape(&log.display().to_string()),
    )
}

/// What the worker needs in its environment: the gateway service's own
/// relation to its owners, this home, and the service instance it manages.
fn worker_environment(home: &AikitHome, home_dir: &Path) -> Result<BTreeMap<String, String>> {
    // The service's own owner relation when it can be discovered (a Central
    // root with its ctrl/actuation/factory); otherwise just what the worker
    // itself needs — it drains, restarts and verifies a gateway, it does not
    // run Routines. Never a credential.
    let mut values = ServiceEnvironment::discover(home_dir, home)
        .map(|environment| environment.values)
        .unwrap_or_default();
    values.insert("HOME".into(), home_dir.display().to_string());
    values.insert("AIKIT_HOME".into(), home.root().display().to_string());
    for name in [
        crate::gateway_install::SERVICE_INSTANCE_ENV,
        crate::gateway_contact::WORKCELL_ENV,
        crate::gateway_install::GATEWAY_REF_ENV,
        WORKER_MODE_ENV,
    ] {
        if let Ok(value) = std::env::var(name) {
            values.insert(name.to_owned(), value);
        }
    }
    // The installer (`oi`) and the toolchain it builds with must be findable
    // by an unattended job: keep the directories of every tool the upgrade
    // will run, beside the service's own.
    if let Some(path) = std::env::var_os("PATH") {
        let existing = values.get("PATH").cloned().unwrap_or_default();
        let mut directories: Vec<String> = existing
            .split(':')
            .filter(|d| !d.is_empty())
            .map(str::to_owned)
            .collect();
        for directory in std::env::split_paths(&path) {
            let directory = directory.display().to_string();
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
        values.insert("PATH".into(), directories.join(":"));
    }
    Ok(values)
}

/// Start the worker for `id` where the gateway's restart cannot reach it.
/// Returns a description of where it runs.
pub fn spawn_worker(home: &AikitHome, id: &str) -> Result<String> {
    let aikit = std::env::current_exe().map_err(|error| {
        AikitError::new(
            "gateway_upgrade.executable_unknown",
            format!("cannot locate this executable to start the upgrade worker: {error}"),
        )
    })?;
    // Run the worker on the managed path a restart will also resolve, not on
    // whichever build this process happens to be.
    let aikit = crate::probe::which("aikit").unwrap_or(aikit);
    let store = Store::new(&home.state());
    let log = store.dir(id).join("worker.log");
    std::fs::create_dir_all(store.dir(id)).map_err(|error| {
        AikitError::new(
            "gateway_upgrade.io",
            format!("create the upgrade dir: {error}"),
        )
    })?;
    let arguments = vec![
        aikit.display().to_string(),
        "gateway".into(),
        "upgrade".into(),
        "worker".into(),
        "--txn".into(),
        id.to_owned(),
    ];
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AikitError::new("gateway_upgrade.home_unresolved", "no HOME is set"))?;
    let environment = worker_environment(home, &home_dir)?;
    let short = worker_short_id(id);

    if std::env::var(WORKER_MODE_ENV).as_deref() == Ok("process") {
        return spawn_detached_process(&arguments, &environment, &log);
    }
    match ServicePlatform::current()? {
        ServicePlatform::LaunchAgent => {
            let label = format!("ai.aikit.gateway-upgrade.{short}");
            let plist_path = home_dir
                .join("Library/LaunchAgents")
                .join(format!("{label}.plist"));
            if let Some(parent) = plist_path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    AikitError::new(
                        "gateway_upgrade.io",
                        format!("create LaunchAgents: {error}"),
                    )
                })?;
            }
            write_atomic(
                &plist_path,
                render_worker_plist(&label, &arguments, &environment, &log).as_bytes(),
            )?;
            let control = SystemServiceControl;
            let uid = control
                .run("id", &["-u".to_owned()])
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .and_then(|text| text.trim().parse::<u32>().ok())
                .unwrap_or(501);
            let domain = format!("gui/{uid}");
            let loaded = control.run(
                "launchctl",
                &[
                    "bootstrap".into(),
                    domain.clone(),
                    plist_path.display().to_string(),
                ],
            )?;
            if !loaded.status.success() {
                let _ = std::fs::remove_file(&plist_path);
                return Err(three_part(
                    "gateway_upgrade.worker_start_failed",
                    format!(
                        "launchctl could not start the upgrade worker: {}",
                        String::from_utf8_lossy(&loaded.stderr).trim()
                    ),
                    "The upgrade was recorded but nothing is running it.",
                    format!(
                        "Resume it in the foreground: `aikit gateway upgrade resume {id} --foreground`"
                    ),
                ));
            }
            Ok(format!("launchd job {label} ({domain})"))
        }
        ServicePlatform::SystemdUser => {
            let unit = format!("aikit-gateway-upgrade-{short}");
            let mut command: Vec<String> = vec![
                "--user".into(),
                format!("--unit={unit}"),
                "--collect".into(),
                "--quiet".into(),
            ];
            for (name, value) in &environment {
                command.push(format!("--setenv={name}={value}"));
            }
            command.push("--".into());
            command.extend(arguments.iter().cloned());
            let started = SystemServiceControl.run("systemd-run", &command)?;
            if started.status.success() {
                Ok(format!("systemd transient unit {unit}"))
            } else {
                Err(three_part(
                    "gateway_upgrade.worker_start_failed",
                    format!(
                        "systemd-run could not start the upgrade worker: {}",
                        String::from_utf8_lossy(&started.stderr).trim()
                    ),
                    "The upgrade was recorded but nothing is running it.",
                    format!(
                        "Resume it in the foreground: `aikit gateway upgrade resume {id} --foreground`"
                    ),
                ))
            }
        }
    }
}

#[cfg(unix)]
fn spawn_detached_process(
    arguments: &[String],
    environment: &BTreeMap<String, String>,
    log: &Path,
) -> Result<String> {
    use std::os::unix::process::CommandExt;
    let log_file = std::fs::File::create(log).map_err(|error| {
        AikitError::new(
            "gateway_upgrade.io",
            format!("create {}: {error}", log.display()),
        )
    })?;
    let (program, rest) = arguments.split_first().expect("a worker command");
    let child = std::process::Command::new(program)
        .args(rest)
        .envs(environment)
        .stdin(std::process::Stdio::null())
        .stdout(log_file.try_clone().map_err(|error| {
            AikitError::new("gateway_upgrade.io", format!("duplicate the log: {error}"))
        })?)
        .stderr(log_file)
        // Its own process group: a signal or a hang-up aimed at the gateway
        // or the terminal does not reach it.
        .process_group(0)
        .spawn()
        .map_err(|error| {
            AikitError::new(
                "gateway_upgrade.worker_start_failed",
                format!("could not start the upgrade worker: {error}"),
            )
        })?;
    Ok(format!("detached process {}", child.id()))
}

#[cfg(not(unix))]
fn spawn_detached_process(
    _arguments: &[String],
    _environment: &BTreeMap<String, String>,
    _log: &Path,
) -> Result<String> {
    Err(AikitError::new(
        "gateway_upgrade.worker_unsupported",
        "a detached upgrade worker needs a unix platform",
    ))
}

/// The worker's last act: remove its own one-shot job. A leftover definition
/// is harmless (RunAtLoad fires once per login) but untidy, and is named by
/// `upgrade status`.
pub fn retire_worker(home: &AikitHome, id: &str) {
    if std::env::var(WORKER_MODE_ENV).as_deref() == Ok("process") {
        return;
    }
    let short = worker_short_id(id);
    let Some(home_dir) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let _ = home;
    if let Ok(ServicePlatform::LaunchAgent) = ServicePlatform::current() {
        let label = format!("ai.aikit.gateway-upgrade.{short}");
        let plist = home_dir
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"));
        let _ = std::fs::remove_file(&plist);
        let control = SystemServiceControl;
        let uid = control
            .run("id", &["-u".to_owned()])
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|text| text.trim().parse::<u32>().ok())
            .unwrap_or(501);
        // Last: this boots the job this process is running in.
        let _ = control.run(
            "launchctl",
            &["bootout".into(), format!("gui/{uid}/{label}")],
        );
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// `upgrade plan`: what runs, what is installed, what an apply would do.
pub fn plan_command(
    home: &AikitHome,
    channel: Option<&str>,
    candidate: Option<&str>,
    install: bool,
) -> Result<Value> {
    if let Some(candidate) = candidate {
        validate_candidate(candidate)?;
    }
    let env = SystemEnv::new(home.clone())?;
    let installer = install
        .then(|| resolve_installer(channel, candidate))
        .flatten();
    let peers = declared_remote_readings(home);
    plan_reading(&env, installer.as_ref(), peers)
}

/// A candidate names a revision to build: a commit id or a branch-like name, never
/// anything that could be read as an option or carry shell syntax.
pub fn validate_candidate(candidate: &str) -> Result<()> {
    let ok = !candidate.is_empty()
        && !candidate.starts_with('-')
        && candidate.len() <= 128
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    if ok {
        Ok(())
    } else {
        Err(three_part(
            "gateway_upgrade.candidate_invalid",
            format!("`{candidate}` is not a revision the installer can build."),
            "Nothing was changed.",
            "Name a commit (`git rev-parse <branch>`) or a branch: letters, digits, `.`, `_`, `-`, `/`.",
        ))
    }
}

/// Free kibibytes on the volume holding `path` (`df -Pk`); `None` when it cannot
/// be read.
pub fn free_kib(path: &Path) -> Option<u64> {
    let output = std::process::Command::new("df")
        .args(["-Pk"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse::<u64>()
        .ok()
}

/// A managed install builds the suite: it needs room, and a full disk is how a
/// build (and a gateway state write) fails halfway. Checked before anything changes.
pub const INSTALL_MIN_FREE_KIB: u64 = 3 * 1024 * 1024;

/// The floor in use: `AIKIT_INSTALL_MIN_FREE_MIB` overrides the default for an
/// operator who builds elsewhere, or for a rehearsal on a small volume.
pub fn install_min_free_kib() -> u64 {
    std::env::var("AIKIT_INSTALL_MIN_FREE_MIB")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|mib| mib.saturating_mul(1024))
        .unwrap_or(INSTALL_MIN_FREE_KIB)
}

pub struct ApplyOptions {
    pub install: bool,
    pub channel: Option<String>,
    pub candidate: Option<String>,
    pub origin: Option<UpgradeOrigin>,
    pub requested_by: String,
    pub auto_rollback: bool,
    pub drain_grace_secs: Option<u64>,
    pub verify_timeout_secs: Option<u64>,
    pub exit_wait_secs: Option<u64>,
    /// Drive the transaction in this process instead of a detached worker.
    pub foreground: bool,
    /// Wait (bounded) for the worker and return the finished receipt.
    pub wait: bool,
}

/// `upgrade apply`: create the transaction, then drive it — detached by
/// default, because the gateway this restarts may be what asked.
pub fn apply_command(home: &AikitHome, options: ApplyOptions) -> Result<Value> {
    let env = SystemEnv::new(home.clone())?;
    let store = env.store();
    if let Some(candidate) = options.candidate.as_deref() {
        validate_candidate(candidate)?;
    }
    if options.install {
        let at = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.state());
        if let Some(free) = free_kib(&at) {
            let floor = install_min_free_kib();
            if free < floor {
                return Err(three_part(
                    "gateway_upgrade.disk_low",
                    format!(
                        "{} MiB are free where the managed install builds ({}); it needs at least {} MiB (AIKIT_INSTALL_MIN_FREE_MIB changes the floor).",
                        free / 1024,
                        at.display(),
                        floor / 1024
                    ),
                    "Nothing was changed; the running gateway was not touched.",
                    "Free space (build caches of retired work are the usual cause), then run it again; or restart onto the installed build with `aikit gateway upgrade apply`.",
                ));
            }
        }
    }
    let installer = if options.install {
        Some(resolve_installer(options.channel.as_deref(), options.candidate.as_deref()).ok_or_else(|| {
            three_part(
                "gateway_upgrade.installer_absent",
                "--install needs the managed installer, and `oi` is not on PATH.",
                "Nothing was changed.",
                "Install O:I (`oi`), or restart onto an already-installed build with `aikit gateway upgrade apply --restart-only`.",
            )
        })?)
    } else {
        None
    };
    let mode = if installer.is_some() {
        Mode::InstallThenRestart
    } else {
        Mode::RestartOnly
    };
    let mut plan = default_plan(mode, installer, None);
    plan.auto_rollback = options.auto_rollback;
    if let Some(secs) = options.drain_grace_secs {
        plan.drain_grace_ms = secs * 1_000;
    }
    if let Some(secs) = options.verify_timeout_secs {
        plan.verify_timeout_ms = secs * 1_000;
    }
    if let Some(secs) = options.exit_wait_secs {
        plan.exit_wait_ms = secs * 1_000;
    }
    let driver = Driver {
        env: &env,
        store: &store,
    };
    let mut transaction = driver.create(options.requested_by, options.origin, plan)?;
    if options.foreground {
        let _lock = lock_driver(&store, &transaction.id)?;
        driver.drive(&mut transaction)?;
        return Ok(json!({
            "upgrade": transaction.id,
            "phase": transaction.phase,
            "outcome": transaction.outcome,
            "receipt": store.dir(&transaction.id).join("receipt.md").display().to_string(),
        }));
    }
    let worker = spawn_worker(home, &transaction.id)?;
    let mut reading = json!({
        "upgrade": transaction.id,
        "started": true,
        "worker": worker,
        "status": "aikit gateway upgrade status",
        "receipt": store.dir(&transaction.id).join("receipt.md").display().to_string(),
    });
    if options.wait {
        let waited = wait_for(&store, &transaction.id, Duration::from_secs(45 * 60 + 300));
        reading["phase"] = json!(waited.phase);
        reading["outcome"] = json!(waited.outcome);
    }
    Ok(reading)
}

fn wait_for(store: &Store, id: &str, limit: Duration) -> Transaction {
    let started = std::time::Instant::now();
    loop {
        if let Ok(transaction) = store.load(id) {
            if transaction.phase.is_terminal() || started.elapsed() >= limit {
                return transaction;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn lock_driver(store: &Store, id: &str) -> Result<aikit_adapters::GatewayStateLock> {
    aikit_adapters::acquire_gateway_state_lock(
        &store.dir(id).join("driver"),
        Duration::from_secs(2),
        "gateway upgrade worker",
    )
}

/// `upgrade worker --txn`: the detached finaliser. Drives the transaction to
/// a terminal phase and retires its own job.
pub fn worker_command(home: &AikitHome, id: &str) -> Result<Value> {
    let env = SystemEnv::new(home.clone())?;
    let store = env.store();
    let _lock = lock_driver(&store, id)?;
    let mut transaction = store.load(id)?;
    let driver = Driver {
        env: &env,
        store: &store,
    };
    let result = driver.drive(&mut transaction);
    let reading = json!({
        "upgrade": transaction.id,
        "phase": transaction.phase,
        "outcome": transaction.outcome,
    });
    drop(_lock);
    if transaction.phase.is_terminal() {
        retire_worker(home, id);
    }
    result.map(|()| reading)
}

/// `upgrade resume ID`: another driver finishes what a stopped worker left.
pub fn resume_command(home: &AikitHome, id: Option<&str>, foreground: bool) -> Result<Value> {
    let env = SystemEnv::new(home.clone())?;
    let store = env.store();
    let transaction = match id {
        Some(id) => store.load(id)?,
        None => store.in_flight().ok_or_else(|| {
            AikitError::new(
                "gateway_upgrade.nothing_to_resume",
                "no upgrade is in flight; `aikit gateway upgrade status` lists them",
            )
        })?,
    };
    if transaction.phase.is_terminal() && transaction.receipt_delivered {
        return Ok(json!({
            "upgrade": transaction.id,
            "phase": transaction.phase,
            "note": "already finished and its receipt delivered",
        }));
    }
    if foreground {
        return worker_command(home, &transaction.id);
    }
    let worker = spawn_worker(home, &transaction.id)?;
    Ok(json!({"upgrade": transaction.id, "resumed": true, "worker": worker}))
}

/// `upgrade status [ID]`: the transaction, its steps and its receipt path.
pub fn status_command(home: &AikitHome, id: Option<&str>) -> Result<Value> {
    let store = Store::new(&home.state());
    let transaction = match id {
        Some(id) => Some(store.load(id)?),
        None => store.in_flight().or_else(|| store.latest()),
    };
    let Some(transaction) = transaction else {
        return Ok(json!({"upgrades": [], "note": "no upgrade has run on this home"}));
    };
    Ok(json!({
        "upgrade": transaction.id,
        "phase": transaction.phase,
        "terminal": transaction.phase.is_terminal(),
        "outcome": transaction.outcome,
        "before": transaction.before,
        "after": transaction.after,
        "steps": transaction.steps,
        "receipt_delivered": transaction.receipt_delivered,
        "receipt": store.dir(&transaction.id).join("receipt.md").display().to_string(),
        "all": store.list().iter().map(|t| json!({"id": t.id, "phase": t.phase})).collect::<Vec<_>>(),
    }))
}

/// `upgrade rollback ID`: restore the previous build of a finished or stuck
/// upgrade and verify it runs.
pub fn rollback_command(home: &AikitHome, id: &str) -> Result<Value> {
    let env = SystemEnv::new(home.clone())?;
    let store = env.store();
    let mut transaction = store.load(id)?;
    if transaction
        .plan
        .installer
        .as_ref()
        .is_none_or(|installer| installer.rollback.is_empty())
    {
        return Err(three_part(
            "gateway_upgrade.no_rollback",
            format!("Upgrade {id} recorded no installer rollback."),
            "Nothing was changed.",
            "Restore the previous build with your installer, then `aikit gateway upgrade apply --restart-only`.",
        ));
    }
    require_latest(&store, id)?;
    let _lock = lock_driver(&store, id)?;
    transaction.phase = crate::gateway_upgrade::Phase::RollingBack;
    transaction.rollback_requested = true;
    transaction.outcome = None;
    transaction.receipt_delivered = false;
    store.save(&transaction)?;
    let driver = Driver {
        env: &env,
        store: &store,
    };
    driver.drive(&mut transaction)?;
    Ok(json!({
        "upgrade": transaction.id,
        "phase": transaction.phase,
        "outcome": transaction.outcome,
    }))
}

/// Whether this upgrade left a newly installed build in place: it ran the installer,
/// the install took effect, and it was not undone. A restart-only upgrade, a
/// `no-change`, an install that failed before changing anything and one already rolled
/// back changed no installed set that the installer's rollback would restore.
fn installed_a_build(transaction: &crate::gateway_upgrade::Transaction) -> bool {
    use crate::gateway_upgrade::Phase;
    transaction.plan.installer.is_some()
        && transaction
            .steps
            .iter()
            .any(|step| step.phase == Phase::Installed && step.ok)
        && !matches!(
            transaction.phase,
            Phase::FailedBeforeChange | Phase::RolledBack
        )
        && transaction
            .outcome
            .as_ref()
            .is_none_or(|outcome| outcome.status != "no-change")
}

/// `oi update --rollback` restores the previous set of the LATEST update that changed
/// the installed build. For an older install that is a different build than the one
/// that transaction would verify, so it is refused rather than ending in a confusing
/// needs-operator. A restart-only or `no-change` upgrade run since does not count: it
/// changed no installed set.
fn require_latest(store: &Store, id: &str) -> Result<()> {
    match store.list().into_iter().rev().find(installed_a_build) {
        Some(latest) if latest.id == id => Ok(()),
        Some(latest) => Err(three_part(
            "gateway_upgrade.rollback_not_latest",
            format!(
                "Upgrade {id} is not the latest upgrade that changed the installed build ({}): the installer's rollback restores the previous set of that one.",
                latest.id
            ),
            "Nothing was changed.",
            format!("Roll back that one (`aikit gateway upgrade rollback {}`), or restore the build you want with your installer and run `aikit gateway upgrade apply`.", latest.id),
        )),
        None => Err(three_part(
            "gateway_upgrade.nothing_to_roll_back",
            format!("No upgrade has left a newly installed build to roll back (asked for {id})."),
            "Nothing was changed.",
            "Restore the build you want with your installer, then `aikit gateway upgrade apply --restart-only`.",
        )),
    }
}

/// `upgrade abandon`: give up on a transaction whose worker is gone and cannot be
/// resumed (a step that fails every time). Refused while a worker holds it; it
/// changes nothing on disk or in the running gateway.
pub fn abandon_command(home: &AikitHome, id: Option<&str>, reason: &str) -> Result<Value> {
    let env = SystemEnv::new(home.clone())?;
    let store = env.store();
    let mut transaction = match id {
        Some(id) => store.load(id)?,
        None => store.in_flight().ok_or_else(|| {
            three_part(
                "gateway_upgrade.nothing_to_abandon",
                "No upgrade is in flight.",
                "Nothing was changed.",
                "`aikit gateway upgrade status` lists the upgrades this home has run.",
            )
        })?,
    };
    if transaction.phase.is_terminal() {
        return Err(three_part(
            "gateway_upgrade.already_finished",
            format!(
                "Upgrade {} already finished ({:?}); there is nothing to abandon.",
                transaction.id, transaction.phase
            ),
            "Nothing was changed.",
            "`aikit gateway upgrade status` shows its receipt.",
        ));
    }
    let _lock = lock_driver(&store, &transaction.id).map_err(|_| {
        three_part(
            "gateway_upgrade.worker_alive",
            format!("A worker is driving upgrade {} right now.", transaction.id),
            "Nothing was changed.",
            "Let it finish, or stop it first; abandon is for a transaction nothing is driving.",
        )
    })?;
    let driver = Driver {
        env: &env,
        store: &store,
    };
    driver.abandon(&mut transaction, reason)?;
    Ok(json!({
        "upgrade": transaction.id,
        "phase": transaction.phase,
        "outcome": transaction.outcome,
    }))
}

/// The upgrade owner behind a conversation's ask: the plan on request, and on
/// `apply` a transaction whose receipt returns to that conversation. The
/// current engine keeps no `/upgrade` parse arm, so nothing wires this today;
/// the trait stands as the seam a future engine re-wiring implements (the ask
/// names its origin at the CLI meanwhile: `upgrade apply --origin-binding`).
pub trait GatewayUpgradeLauncher: Send + Sync + 'static {
    /// The plan, as data and one plain-words line. Changes nothing.
    fn plan(&self) -> Result<(Value, String)>;
    /// Start the managed upgrade in a detached worker; the receipt returns to
    /// `origin` once the new build is verified running.
    fn start(&self, origin: UpgradeOrigin) -> Result<(Value, String)>;
}

/// The upgrade owner behind a conversation's `/upgrade`: the plan on request,
/// and on `apply` a transaction whose receipt returns to that conversation.
pub struct ConversationUpgradeLauncher {
    pub home: AikitHome,
}

impl GatewayUpgradeLauncher for ConversationUpgradeLauncher {
    fn plan(&self) -> Result<(Value, String)> {
        let plan = plan_command(&self.home, None, None, false)?;
        let line = plan_line(&plan);
        Ok((plan, line))
    }

    fn start(&self, origin: UpgradeOrigin) -> Result<(Value, String)> {
        let binding = origin.binding_ref.clone();
        let started = apply_command(
            &self.home,
            ApplyOptions {
                install: false,
                channel: None,
                candidate: None,
                origin: Some(origin),
                requested_by: format!("conversation:{binding}"),
                auto_rollback: true,
                drain_grace_secs: None,
                verify_timeout_secs: None,
                exit_wait_secs: None,
                foreground: false,
                wait: false,
            },
        )?;
        let id = started["upgrade"].as_str().unwrap_or("?").to_owned();
        let line = format!(
            "upgrade {id} started in a worker that outlives this gateway: it drains the \
             gateway, restarts it on the installed build, verifies the new process and reports \
             here"
        );
        Ok((started, line))
    }
}

/// A non-terminal transaction whose worker is gone: the *new* gateway notices
/// on its tick and starts a resume worker, so a dead worker never strands an
/// upgrade or its receipt. A transaction whose driver lock is held has a live
/// worker and is left alone, as is one touched in the last half minute.
pub fn adopt_orphans(home: &AikitHome) -> Result<Option<String>> {
    adopt_orphans_with(home, |home, id| spawn_worker(home, id).ok())
}

/// [`adopt_orphans`] with the worker spawn behind a seam, so the rule — who is
/// adopted, and when — can be tested without starting a process.
pub fn adopt_orphans_with(
    home: &AikitHome,
    spawn: impl Fn(&AikitHome, &str) -> Option<String>,
) -> Result<Option<String>> {
    let store = Store::new(&home.state());
    let Some(transaction) = store.in_flight() else {
        // A finished upgrade whose receipt could not be announced yet.
        let undelivered = store
            .list()
            .into_iter()
            .rev()
            .find(|t| t.phase.is_terminal() && !t.receipt_delivered && t.origin.is_some());
        return Ok(match undelivered {
            Some(transaction) if quiet_for(&transaction, 30_000) => {
                match lock_driver(&store, &transaction.id) {
                    Ok(lock) => {
                        drop(lock);
                        spawn(home, &transaction.id)
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        });
    };
    if !quiet_for(&transaction, 30_000) {
        return Ok(None);
    }
    match lock_driver(&store, &transaction.id) {
        Ok(lock) => {
            drop(lock);
            Ok(spawn(home, &transaction.id))
        }
        Err(_) => Ok(None),
    }
}

fn quiet_for(transaction: &Transaction, millis: u64) -> bool {
    aikit_adapters::gateway_posture::unix_ms_now().saturating_sub(transaction.updated_at_unix_ms)
        >= millis
}

/// Each declared remote gateway, asked what build it runs and which protocol
/// features it supports. A peer that does not answer is named, not omitted: a
/// mixed-version pair is a fact about the fleet an upgrade should show.
pub fn declared_remote_readings(home: &AikitHome) -> Vec<Value> {
    let Ok(remotes) = crate::gateway_contact::load_remotes(home) else {
        return Vec::new();
    };
    remotes.remotes.iter().map(probe_remote).collect()
}

/// Ask one declared remote gateway what it is: which Workcell it says it
/// serves, which build, which features. The declared Workcell is what claims
/// and relays are recorded against, so the answered one is compared to it by
/// the caller — a gateway declared as one Workcell that serves another is an
/// identity fault, not a detail.
pub fn probe_remote(remote: &crate::gateway_contact::GatewayRemote) -> Value {
    let token = crate::secret_location::SecretLocation::parse(&remote.token_location)
        .and_then(|location| location.resolve());
    let mut reading = json!({
        "workcell_ref": remote.workcell_ref,
        "endpoint": remote.websocket_bind,
        "listener_class": aikit_adapters::ListenerClass::classify_bind(&remote.websocket_bind),
    });
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            reading["reachable"] = json!(false);
            reading["detail"] = json!(format!("its token could not be read: {error}"));
            return reading;
        }
    };
    let target = GatewayCarrierTarget::WebSocket {
        bind: remote.websocket_bind.clone(),
        path: remote.websocket_path.clone(),
        bearer_token: token.expose().to_owned(),
    };
    match gateway_command_within(
        &target,
        GatewayCommand::Protocol,
        None,
        Duration::from_secs(3),
    ) {
        Ok(GatewayResponse::Protocol { features, .. }) => {
            let missing: Vec<&str> = aikit_adapters::GATEWAY_PROTOCOL_FEATURES
                .iter()
                .copied()
                .filter(|wanted| !features.iter().any(|f| f == wanted))
                .collect();
            reading["reachable"] = json!(true);
            reading["features"] = json!(features);
            reading["missing_features"] = json!(missing);
        }
        Ok(_) => {
            reading["reachable"] = json!(false);
            reading["detail"] = json!("it answered, but not with a protocol reading");
        }
        Err(error) => {
            reading["reachable"] = json!(false);
            reading["detail"] = json!(error.to_string());
        }
    }
    reading
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction_in(store: &Store, id: &str, phase: crate::gateway_upgrade::Phase, updated: u64) {
        let mut transaction: crate::gateway_upgrade::Transaction =
            serde_json::from_value(serde_json::json!({
                "schema": crate::gateway_upgrade::TRANSACTION_SCHEMA,
                "id": id,
                "created_at_unix_ms": updated,
                "updated_at_unix_ms": updated,
                "phase": serde_json::to_value(phase).unwrap(),
                "requested_by": "test",
                "plan": {
                    "mode": "restart-only",
                    "drain_grace_ms": 1000,
                    "exit_wait_ms": 1000,
                    "verify_timeout_ms": 1000,
                    "auto_rollback": true
                }
            }))
            .unwrap();
        transaction.receipt_delivered = false;
        store.save(&transaction).unwrap();
    }

    /// An upgrade that ran the installer: `installed` says whether the install took
    /// effect; `status` is its outcome.
    fn install_in(
        store: &Store,
        id: &str,
        phase: crate::gateway_upgrade::Phase,
        updated: u64,
        installed: bool,
        status: Option<&str>,
    ) {
        let mut transaction: crate::gateway_upgrade::Transaction =
            serde_json::from_value(serde_json::json!({
                "schema": crate::gateway_upgrade::TRANSACTION_SCHEMA,
                "id": id,
                "created_at_unix_ms": updated,
                "updated_at_unix_ms": updated,
                "phase": serde_json::to_value(phase).unwrap(),
                "requested_by": "test",
                "plan": {
                    "mode": "install-then-restart",
                    "installer": {
                        "install": ["oi", "update", "--apply", "ai-kit"],
                        "rollback": ["oi", "update", "--rollback"],
                        "timeout_ms": 1000
                    },
                    "drain_grace_ms": 1000,
                    "exit_wait_ms": 1000,
                    "verify_timeout_ms": 1000,
                    "auto_rollback": true
                },
                "steps": if installed {
                    serde_json::json!([{
                        "at_unix_ms": updated,
                        "phase": serde_json::to_value(crate::gateway_upgrade::Phase::Installed).unwrap(),
                        "ok": true,
                        "detail": "installed"
                    }])
                } else {
                    serde_json::json!([])
                },
                "outcome": status.map(|status| serde_json::json!({
                    "status": status,
                    "summary": "test",
                    "operator_steps": []
                }))
            }))
            .unwrap();
        transaction.receipt_delivered = true;
        store.save(&transaction).unwrap();
    }

    #[test]
    fn only_the_latest_upgrade_that_changed_the_installed_build_can_be_rolled_back() {
        use crate::gateway_upgrade::Phase;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        // Nothing installed anything yet: there is nothing to roll back.
        transaction_in(&store, "upg-000", Phase::Completed, 1);
        assert_eq!(
            require_latest(&store, "upg-000").unwrap_err().code(),
            "gateway_upgrade.nothing_to_roll_back"
        );
        install_in(
            &store,
            "upg-001",
            Phase::Completed,
            2,
            true,
            Some("completed"),
        );
        install_in(
            &store,
            "upg-002",
            Phase::Completed,
            3,
            true,
            Some("completed"),
        );
        let error = require_latest(&store, "upg-001").unwrap_err();
        assert_eq!(error.code(), "gateway_upgrade.rollback_not_latest");
        assert!(error.message().contains("upg-002"), "{}", error.message());
        assert!(require_latest(&store, "upg-002").is_ok());
        // Upgrades since that changed no installed set do not displace it: a
        // restart-only run, a no-change, an install that failed before changing
        // anything, and one that was already rolled back.
        transaction_in(&store, "upg-003", Phase::Completed, 4);
        install_in(
            &store,
            "upg-004",
            Phase::Completed,
            5,
            true,
            Some("no-change"),
        );
        install_in(
            &store,
            "upg-005",
            Phase::FailedBeforeChange,
            6,
            false,
            Some("failed-before-change"),
        );
        install_in(
            &store,
            "upg-006",
            Phase::RolledBack,
            7,
            true,
            Some("rolled-back"),
        );
        assert!(require_latest(&store, "upg-002").is_ok());
        let error = require_latest(&store, "upg-001").unwrap_err();
        assert!(error.message().contains("upg-002"), "{}", error.message());
        // A later install does displace it.
        install_in(
            &store,
            "upg-007",
            Phase::Completed,
            8,
            true,
            Some("completed"),
        );
        assert_eq!(
            require_latest(&store, "upg-002").unwrap_err().code(),
            "gateway_upgrade.rollback_not_latest"
        );
        assert!(require_latest(&store, "upg-007").is_ok());
    }

    #[test]
    fn a_dead_workers_transaction_is_adopted_once_it_has_been_quiet_and_never_while_it_is_fresh_or_held(
    ) {
        use std::cell::RefCell;
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        std::fs::create_dir_all(home.state()).unwrap();
        let store = Store::new(&home.state());
        let now = aikit_adapters::gateway_posture::unix_ms_now();
        let spawned = RefCell::new(Vec::<String>::new());
        let spawn = |_: &AikitHome, id: &str| {
            spawned.borrow_mut().push(id.to_owned());
            Some(format!("worker for {id}"))
        };
        // Nothing in flight: nothing adopted.
        assert_eq!(adopt_orphans_with(&home, spawn).unwrap(), None);
        // In flight but touched a moment ago: a live worker may be about to write.
        transaction_in(
            &store,
            "upg-001",
            crate::gateway_upgrade::Phase::Draining,
            now,
        );
        assert_eq!(adopt_orphans_with(&home, spawn).unwrap(), None);
        // Quiet for over 30 s and nobody holds its driver lock: adopted.
        transaction_in(
            &store,
            "upg-001",
            crate::gateway_upgrade::Phase::Draining,
            now - 60_000,
        );
        assert_eq!(
            adopt_orphans_with(&home, spawn).unwrap().as_deref(),
            Some("worker for upg-001")
        );
        // Quiet, but a worker holds the driver lock: left alone.
        let held = lock_driver(&store, "upg-001").unwrap();
        assert_eq!(adopt_orphans_with(&home, spawn).unwrap(), None);
        drop(held);
        // A finished upgrade whose receipt was never announced is adopted too (to
        // deliver it) — but only if it names a conversation.
        transaction_in(
            &store,
            "upg-001",
            crate::gateway_upgrade::Phase::Completed,
            now - 60_000,
        );
        assert_eq!(
            adopt_orphans_with(&home, spawn).unwrap(),
            None,
            "no origin: nothing to tell"
        );
        let mut told = store.load("upg-001").unwrap();
        told.origin = Some(crate::gateway_upgrade::UpgradeOrigin {
            binding_ref: "gateway-binding/x".into(),
            connector_ref: None,
            in_reply_to_sequence: None,
        });
        store.save(&told).unwrap();
        assert_eq!(
            adopt_orphans_with(&home, spawn).unwrap().as_deref(),
            Some("worker for upg-001"),
            "an unannounced receipt for a conversation is delivered by the next gateway"
        );
        assert_eq!(spawned.borrow().len(), 2);
        // A finished upgrade's receipt is not chased while it is fresh (its worker may
        // be about to announce it), nor while a worker holds its lock.
        let mut fresh = store.load("upg-001").unwrap();
        fresh.updated_at_unix_ms = now;
        store.save(&fresh).unwrap();
        assert_eq!(
            adopt_orphans_with(&home, spawn).unwrap(),
            None,
            "a fresh finished upgrade is left to its worker"
        );
        fresh.updated_at_unix_ms = now - 60_000;
        store.save(&fresh).unwrap();
        let held = lock_driver(&store, "upg-001").unwrap();
        assert_eq!(
            adopt_orphans_with(&home, spawn).unwrap(),
            None,
            "a finished upgrade whose lock is held is left alone"
        );
        drop(held);
        assert_eq!(spawned.borrow().len(), 2);
    }

    #[test]
    fn the_executable_a_supervisor_runs_is_read_from_its_definition() {
        let plist = r#"<key>ProgramArguments</key>
    <array>
        <string>/Users/x/.local/bin/aikit</string>
        <string>gateway</string>
    </array>"#;
        assert_eq!(
            executable_named_by(plist, ServicePlatform::LaunchAgent),
            Some(PathBuf::from("/Users/x/.local/bin/aikit"))
        );
        let unit = "[Service]\nExecStart=/home/frank/.local/bin/aikit gateway serve --unix\nRestart=always\n";
        assert_eq!(
            executable_named_by(unit, ServicePlatform::SystemdUser),
            Some(PathBuf::from("/home/frank/.local/bin/aikit"))
        );
        assert_eq!(executable_named_by("", ServicePlatform::LaunchAgent), None);
    }

    #[test]
    fn a_revision_is_read_from_the_version_line() {
        assert_eq!(
            revision_from_version_line("aikit 0.1.0 (64d4e12fd9)"),
            Some("64d4e12fd9".to_owned())
        );
        assert_eq!(revision_from_version_line("aikit 0.1.0"), None);
    }

    #[test]
    fn the_worker_is_a_one_shot_job_that_is_never_kept_alive_and_runs_the_named_transaction() {
        let mut environment = BTreeMap::new();
        environment.insert("AIKIT_HOME".to_owned(), "/home/x/.aikit".to_owned());
        let plist = render_worker_plist(
            "ai.aikit.gateway-upgrade.abc",
            &[
                "/bin/aikit".into(),
                "gateway".into(),
                "upgrade".into(),
                "worker".into(),
                "--txn".into(),
                "upg-abc".into(),
            ],
            &environment,
            Path::new("/tmp/worker.log"),
        );
        assert!(plist.contains("<key>KeepAlive</key>\n    <false/>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.contains("<string>upg-abc</string>"));
        assert!(plist.contains("<key>AIKIT_HOME</key>"));
        assert!(plist.contains("ai.aikit.gateway-upgrade.abc"));
    }

    #[test]
    fn a_gateway_that_predates_build_identity_is_still_a_running_process_with_a_pid() {
        let running = running_from_posture(
            vec!["communique-exact-instance".into()],
            None,
            Some(4242),
            false,
        )
        .expect("a running process");
        assert_eq!(running.pid, 4242);
        assert_eq!(running.identity.revision, "unknown");
        assert_eq!(running.lifecycle, GatewayLifecycle::Foreground);
        assert!(running_from_posture(vec![], None, None, true).is_none());
    }
}
