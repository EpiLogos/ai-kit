//! `aikit gateway install-service | uninstall-service` — the user service that
//! keeps the gateway alive across restart, sleep and reboot, and with it the
//! Routine dispatcher's 30-second tick and the Communique relay pass.
//!
//! Two platforms, one posture:
//!
//! ```text
//! macOS   ~/Library/LaunchAgents/ai.aikit.gateway.plist         launchctl bootstrap gui/<uid>
//! Linux   ~/.config/systemd/user/aikit-gateway.service           systemctl --user enable --now
//! ```
//!
//! The service always serves this home's Unix socket (`serve --unix`), so
//! local inbox, send and turn-boundary delivery reach it. With `--ws ADDR
//! --ws-token-location LOC` it also serves the authenticated WebSocket
//! carrier that other Workcells relay through and ask for occupancy. The
//! token is named by location only; the unit file never carries it, and a
//! `file:` location that is group- or world-readable is refused before
//! anything is written. Workcell and gateway identity travel in the service
//! environment as `AIKIT_WORKCELL_REF` / `AIKIT_GATEWAY_REF`.
//!
//! The service also carries the native-owner relation the Routine dispatcher
//! and the relay pass need ([`ServiceEnvironment`]: HOME, AIKIT_HOME, the
//! Central root, the `ctrl`/`factory`/`actuation` executables and a PATH of
//! only their directories plus system paths), never credentials.
//!
//! KeepAlive (launchd) and `Restart=always` (systemd) restart the process if
//! it exits. A proven-stale home socket left by an exited gateway is cleared
//! under the gateway state lock; a live one is refused. Uninstall is the exact
//! inverse and refuses when nothing is installed. `launchctl` and `systemctl`
//! sit behind [`ServiceControl`] so both renderings and both call sequences
//! are testable on any host.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::gateway_contact::{three_part, WORKCELL_ENV};
use crate::secret_location::SecretLocation;

pub const LAUNCH_AGENT_LABEL: &str = "ai.aikit.gateway";
pub const SYSTEMD_UNIT_NAME: &str = "aikit-gateway.service";
pub const SERVICE_VERSION: &str = "aikit.gateway-service-install/v1";
pub const GATEWAY_REF_ENV: &str = "AIKIT_GATEWAY_REF";

/// Which user-service manager keeps the gateway alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePlatform {
    /// A macOS user LaunchAgent in the GUI domain.
    LaunchAgent,
    /// A Linux systemd user unit, wanted by `default.target`.
    SystemdUser,
}

impl ServicePlatform {
    /// The platform this binary runs on, or a refusal naming the fact.
    pub fn current() -> Result<Self> {
        if cfg!(target_os = "macos") {
            Ok(Self::LaunchAgent)
        } else if cfg!(target_os = "linux") {
            Ok(Self::SystemdUser)
        } else {
            Err(three_part(
                "gateway.service_install_unsupported",
                format!(
                    "This platform ({}) has neither a launchd user domain nor a systemd user manager AIKit installs into.",
                    std::env::consts::OS
                ),
                "Nothing was installed.",
                "Run `aikit gateway serve --unix` under this platform's own service manager.",
            ))
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LaunchAgent => "launchd-user-agent",
            Self::SystemdUser => "systemd-user-unit",
        }
    }

    /// The service definition file under `home_dir`.
    pub fn unit_path(self, home_dir: &Path) -> PathBuf {
        match self {
            Self::LaunchAgent => home_dir
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCH_AGENT_LABEL}.plist")),
            Self::SystemdUser => home_dir
                .join(".config/systemd/user")
                .join(SYSTEMD_UNIT_NAME),
        }
    }

    /// Where the service's output goes: a log file for launchd, the user
    /// journal for systemd.
    pub fn log_location(self, home_dir: &Path) -> String {
        match self {
            Self::LaunchAgent => log_path(home_dir).display().to_string(),
            Self::SystemdUser => format!("journalctl --user -u {SYSTEMD_UNIT_NAME}"),
        }
    }
}

/// The LaunchAgent plist path under the given home directory.
pub fn plist_path(home_dir: &Path) -> PathBuf {
    ServicePlatform::LaunchAgent.unit_path(home_dir)
}

/// The systemd user unit path under the given home directory.
pub fn systemd_unit_path(home_dir: &Path) -> PathBuf {
    ServicePlatform::SystemdUser.unit_path(home_dir)
}

/// The gateway's LaunchAgent log file under the given home directory.
pub fn log_path(home_dir: &Path) -> PathBuf {
    home_dir
        .join("Library/Logs")
        .join(format!("{LAUNCH_AGENT_LABEL}.log"))
}

/// The only process material carried into the service manager. Credentials are resolved
/// later per admitted Routine action, never copied from the installing shell.
#[derive(Debug, Clone)]
pub struct ServiceEnvironment {
    pub values: BTreeMap<String, String>,
}

fn owner_binary(names: &[&str], fallback: &str) -> Result<Option<PathBuf>> {
    let configured = names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let requested = configured.as_deref().unwrap_or(fallback);
    let found = crate::probe::which(requested);
    if configured.is_some() && found.is_none() {
        return Err(AikitError::new(
            "gateway.service_owner_unresolved",
            format!("configured native owner {requested} is not executable"),
        ));
    }
    found
        .map(|path| {
            if path.is_absolute() {
                Ok(path)
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .map_err(|error| {
                        AikitError::new(
                            "gateway.service_owner_unresolved",
                            format!("could not anchor native owner {requested}: {error}"),
                        )
                    })
            }
        })
        .transpose()
}

impl ServiceEnvironment {
    /// Capture only the installed native-owner relation this service needs.
    /// The user's interactive PATH may contain unrelated, mutable directories;
    /// the service receives only discovered owner directories and system paths.
    pub fn discover(home_dir: &Path, home: &aikit_store::AikitHome) -> Result<Self> {
        if !home_dir.is_absolute() || !home.root().is_absolute() {
            return Err(AikitError::new(
                "gateway.service_home_unresolved",
                "HOME and AIKIT_HOME must be absolute for a resident gateway",
            ));
        }
        let central_root = crate::routine_dispatch::CtrlOccurrenceSource::discover()?;
        if !central_root.is_absolute() {
            return Err(AikitError::new(
                "gateway.service_central_root_unresolved",
                "the resident gateway's Central root must be absolute",
            ));
        }
        let ctrl = owner_binary(&["CENTRAL_CTRL_BIN", "OI_CENTRAL_CTRL_BIN"], "ctrl")?.ok_or_else(
            || {
                AikitError::new(
                    "gateway.service_owner_unresolved",
                    "ctrl is required for the resident Routine scheduler",
                )
            },
        )?;
        let factory = owner_binary(
            &["FACTORY_BIN", "OI_FACTORY_BIN", "AIKIT_FACTORY_BIN"],
            "factory",
        )?;
        let actuation = owner_binary(&["ACTUATION_BIN", "OI_ACTUATION_BIN"], "actuation")?;
        let gh = owner_binary(&[], "gh")?;
        let mut paths = Vec::new();
        for binary in [
            Some(&ctrl),
            gh.as_ref(),
            factory.as_ref(),
            actuation.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(parent) = binary.parent() {
                if !paths.contains(&parent.to_path_buf()) {
                    paths.push(parent.to_path_buf());
                }
            }
        }
        for system in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
            let path = PathBuf::from(system);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        let path = std::env::join_paths(paths).map_err(|error| {
            AikitError::new(
                "gateway.service_path_invalid",
                format!("native owner search path cannot be carried to the service: {error}"),
            )
        })?;
        let mut values = BTreeMap::from([
            ("HOME".into(), home_dir.display().to_string()),
            ("AIKIT_HOME".into(), home.root().display().to_string()),
            (
                "AIKIT_CENTRAL_ROOT".into(),
                central_root.display().to_string(),
            ),
            ("CENTRAL_CTRL_BIN".into(), ctrl.display().to_string()),
            ("PATH".into(), path.to_string_lossy().into_owned()),
        ]);
        if let Some(factory) = factory {
            values.insert("FACTORY_BIN".into(), factory.display().to_string());
            values.insert("AIKIT_FACTORY_BIN".into(), factory.display().to_string());
        }
        if let Some(actuation) = actuation {
            values.insert("ACTUATION_BIN".into(), actuation.display().to_string());
        }
        Ok(Self { values })
    }
}

/// What the installed service serves and as whom.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceOptions {
    /// Also serve the authenticated WebSocket carrier at `HOST:PORT`.
    pub websocket_bind: Option<String>,
    /// Where the WebSocket bearer token lives (`file:/abs/path` or a declared
    /// secret ref). Required with `websocket_bind`.
    pub token_location: Option<String>,
    /// `AIKIT_GATEWAY_REF` for the service.
    pub gateway_ref: Option<String>,
    /// `AIKIT_WORKCELL_REF` for the service.
    pub workcell_ref: Option<String>,
}

fn nothing_installed() -> &'static str {
    "Nothing was installed and no service definition was written."
}

impl ServiceOptions {
    /// Check everything the service will need before anything is written: a
    /// WebSocket carrier and its token location come together, a `file:`
    /// token is owner-only and readable now, and the refs parse the way
    /// `serve` will parse them.
    pub fn validate(&self) -> Result<()> {
        match (&self.websocket_bind, &self.token_location) {
            (Some(_), None) => {
                return Err(three_part(
                    "gateway.service_ws_token_required",
                    "--ws was given without --ws-token-location; a network carrier must authenticate its peers.",
                    nothing_installed(),
                    "Add --ws-token-location file:/ABSOLUTE/PATH/TO/TOKEN (an owner-only file, chmod 600).",
                ))
            }
            (None, Some(_)) => {
                return Err(three_part(
                    "gateway.service_ws_required",
                    "--ws-token-location was given without --ws; there is no WebSocket carrier for it to guard.",
                    nothing_installed(),
                    "Add --ws HOST:PORT, or drop --ws-token-location to serve the Unix socket only.",
                ))
            }
            (Some(bind), Some(location)) => {
                if bind.trim().is_empty() || bind.trim() != bind {
                    return Err(three_part(
                        "gateway.service_ws_invalid",
                        format!("--ws {bind:?} is not a HOST:PORT address."),
                        nothing_installed(),
                        "Pass the address the other Workcells reach, e.g. --ws 100.109.102.82:7800.",
                    ));
                }
                let parsed = SecretLocation::parse(location).map_err(|error| {
                    three_part(
                        "gateway.service_token_location_invalid",
                        format!("--ws-token-location {location}: {error}"),
                        nothing_installed(),
                        "Name the token by location: file:/ABSOLUTE/PATH (chmod 600) or a keychain/pass/op/varlock ref.",
                    )
                })?;
                // A file token is read by an unattended service; prove now
                // that it will be accepted, rather than at the first restart.
                if let SecretLocation::File(_) = parsed {
                    parsed.resolve().map_err(|error| {
                        three_part(
                            "gateway.service_token_unusable",
                            format!("The token at {location} cannot be used: {error}"),
                            nothing_installed(),
                            format!("Make it an owner-only, non-empty file (chmod 600 {}), then install again.",
                                location.trim_start_matches("file:")),
                        )
                    })?;
                }
            }
            (None, None) => {}
        }
        if let Some(gateway_ref) = &self.gateway_ref {
            aikit_core::resource::ResourceRef::parse(gateway_ref).map_err(|error| {
                three_part(
                    "gateway.service_gateway_ref_invalid",
                    format!("--gateway-ref {gateway_ref:?} is not a ref: {error}"),
                    nothing_installed(),
                    "Pass a ref such as agency-gateway/omarchy.",
                )
            })?;
        }
        if let Some(workcell_ref) = &self.workcell_ref {
            if workcell_ref.trim().is_empty()
                || workcell_ref.trim() != workcell_ref
                || workcell_ref.chars().any(char::is_whitespace)
            {
                return Err(three_part(
                    "gateway.service_workcell_ref_invalid",
                    format!("--workcell-ref {workcell_ref:?} is not a Workcell ref."),
                    nothing_installed(),
                    "Pass a ref such as workcell:omarchy.",
                ));
            }
        }
        Ok(())
    }

    /// `aikit gateway serve …` arguments after the binary.
    pub fn serve_arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "gateway".to_owned(),
            "serve".to_owned(),
            "--unix".to_owned(),
        ];
        if let (Some(bind), Some(location)) = (&self.websocket_bind, &self.token_location) {
            arguments.extend([
                "--ws".to_owned(),
                bind.clone(),
                "--ws-token-location".to_owned(),
                location.clone(),
            ]);
        }
        arguments
    }

    /// The service environment: identity only, never material.
    pub fn environment(&self) -> Vec<(&'static str, String)> {
        let mut environment = Vec::new();
        if let Some(workcell_ref) = &self.workcell_ref {
            environment.push((WORKCELL_ENV, workcell_ref.clone()));
        }
        if let Some(gateway_ref) = &self.gateway_ref {
            environment.push((GATEWAY_REF_ENV, gateway_ref.clone()));
        }
        environment
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render the LaunchAgent plist for a bare `serve --unix` with this owner
/// environment (no WebSocket, no identity).
pub fn render_plist(binary: &Path, log: &Path, environment: &ServiceEnvironment) -> String {
    render_plist_for(binary, log, environment, &ServiceOptions::default())
}

/// Render the LaunchAgent plist. `binary` is the exact executable launchd
/// runs.
pub fn render_plist_for(
    binary: &Path,
    log: &Path,
    environment: &ServiceEnvironment,
    options: &ServiceOptions,
) -> String {
    let mut program_arguments = vec![binary.display().to_string()];
    program_arguments.extend(options.serve_arguments());
    let arguments = program_arguments
        .iter()
        .map(|argument| format!("        <string>{}</string>", xml_escape(argument)))
        .collect::<Vec<_>>()
        .join("\n");
    let environment = service_environment(environment, options);
    let environment = if environment.is_empty() {
        String::new()
    } else {
        format!(
            "    <key>EnvironmentVariables</key>\n    <dict>\n{}\n    </dict>\n",
            environment
                .iter()
                .map(|(name, value)| format!(
                    "      <key>{}</key>\n      <string>{}</string>",
                    xml_escape(name),
                    xml_escape(value)
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
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
{environment}    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
  </dict>
</plist>
"#,
        label = LAUNCH_AGENT_LABEL,
        arguments = arguments,
        environment = environment,
        log = xml_escape(&log.display().to_string()),
    )
}

/// The service's whole environment: the native-owner relation, then this
/// service's identity.
fn service_environment(
    environment: &ServiceEnvironment,
    options: &ServiceOptions,
) -> Vec<(String, String)> {
    let mut values: BTreeMap<String, String> = environment.values.clone();
    for (name, value) in options.environment() {
        values.insert(name.to_owned(), value);
    }
    values.into_iter().collect()
}

/// One systemd word: quoted when it holds whitespace or quoting characters,
/// with `%` and `$` escaped so systemd substitutes nothing into it.
fn systemd_word(word: &str) -> String {
    let escaped = word.replace('%', "%%").replace('$', "$$");
    if escaped.is_empty()
        || escaped
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | ';'))
    {
        format!("\"{}\"", escaped.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        escaped
    }
}

/// Render the systemd user unit. `binary` is the exact executable systemd
/// runs.
pub fn render_systemd_unit(
    binary: &Path,
    environment: &ServiceEnvironment,
    options: &ServiceOptions,
) -> String {
    let mut command = vec![systemd_word(&binary.display().to_string())];
    command.extend(options.serve_arguments().iter().map(|a| systemd_word(a)));
    let environment = service_environment(environment, options)
        .iter()
        .map(|(name, value)| format!("Environment={}\n", systemd_word(&format!("{name}={value}"))))
        .collect::<String>();
    format!(
        "# Written by `aikit gateway install-service`; remove with `aikit gateway uninstall-service`.\n\
[Unit]\n\
Description=AIKit gateway (Communique contact, relay pass and Routine dispatcher)\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={command}\n\
{environment}\
Restart=always\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        command = command.join(" "),
        environment = environment,
    )
}

/// The service manager's command line, behind a seam tests stub.
pub trait ServiceControl {
    fn run(&self, program: &str, arguments: &[String]) -> Result<std::process::Output>;
}

/// The real `launchctl` / `systemctl` (and `id -u` for the launchd domain).
pub struct SystemServiceControl;

impl ServiceControl for SystemServiceControl {
    fn run(&self, program: &str, arguments: &[String]) -> Result<std::process::Output> {
        std::process::Command::new(program)
            .args(arguments)
            .output()
            .map_err(|error| {
                AikitError::new(
                    "gateway.service_manager_unavailable",
                    format!("could not run {program}: {error}"),
                )
            })
    }
}

fn run(
    control: &dyn ServiceControl,
    program: &str,
    arguments: &[&str],
) -> Result<std::process::Output> {
    let arguments: Vec<String> = arguments.iter().map(|a| (*a).to_owned()).collect();
    control.run(program, &arguments)
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

/// The launchd GUI domain for this user (`gui/<uid>`).
fn gui_domain(control: &dyn ServiceControl) -> String {
    let uid = run(control, "id", &["-u"])
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(501);
    format!("gui/{uid}")
}

/// Clear a socket left by an exited default Gateway before launchd starts its
/// replacement. The Gateway's native state lock excludes owners using this
/// state file, and a live Unix connection also protects custom-state owners.
/// Other files at this path are never treated as stale sockets.
#[cfg(unix)]
fn clear_stale_gateway_socket(home: &aikit_store::AikitHome) -> Result<bool> {
    use std::os::unix::{fs::FileTypeExt, fs::MetadataExt, net::UnixStream};

    let socket = home.gateway_socket();
    let metadata = match std::fs::symlink_metadata(&socket) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(AikitError::new(
                "gateway.service_install_socket_unreadable",
                format!("inspect Gateway socket {}: {error}", socket.display()),
            ));
        }
    };
    if !metadata.file_type().is_socket() {
        return Err(AikitError::new(
            "gateway.service_install_conflict",
            format!(
                "refusing to replace a non-socket Gateway path at {}",
                socket.display()
            ),
        ));
    }

    // A resident Gateway using this state file holds the lock for its
    // lifetime. Holding it here excludes that native owner starting between
    // the lstat and unlink; the connection probe covers custom-state owners.
    let _lock = aikit_adapters::acquire_gateway_state_lock(
        &home.gateway_state(),
        Duration::from_millis(200),
        "Gateway LaunchAgent socket inspection",
    )
    .map_err(|error| {
        AikitError::new(
            "gateway.service_install_conflict",
            format!("Gateway owner state is active; refusing socket cleanup: {error}"),
        )
    })?;
    match UnixStream::connect(&socket) {
        Ok(_) => {
            return Err(AikitError::new(
                "gateway.service_install_conflict",
                format!(
                    "a Gateway is answering at {}; stop it before installation",
                    socket.display()
                ),
            ));
        }
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => {}
        Err(error) => {
            return Err(AikitError::new(
                "gateway.service_install_conflict",
                format!(
                    "cannot prove Gateway socket {} is stale: {error}",
                    socket.display()
                ),
            ));
        }
    }
    let current = std::fs::symlink_metadata(&socket).map_err(|error| {
        AikitError::new(
            "gateway.service_install_conflict",
            format!(
                "Gateway socket {} changed during inspection: {error}",
                socket.display()
            ),
        )
    })?;
    if !current.file_type().is_socket()
        || current.dev() != metadata.dev()
        || current.ino() != metadata.ino()
    {
        return Err(AikitError::new(
            "gateway.service_install_conflict",
            format!(
                "Gateway socket {} changed during inspection",
                socket.display()
            ),
        ));
    }
    std::fs::remove_file(&socket).map_err(|error| {
        AikitError::new(
            "gateway.service_install_socket_cleanup_failed",
            format!(
                "remove proven stale Gateway socket {}: {error}",
                socket.display()
            ),
        )
    })?;
    Ok(true)
}

/// Is the gateway service installed for this platform?
pub fn is_installed(home_dir: &Path) -> bool {
    ServicePlatform::current()
        .map(|platform| platform.unit_path(home_dir).exists())
        .unwrap_or(false)
}

/// Install on this platform with the real service manager.
pub fn install(
    home_dir: &Path,
    home: &aikit_store::AikitHome,
    options: &ServiceOptions,
) -> Result<Value> {
    let platform = ServicePlatform::current()?;
    options.validate()?;
    let running = std::env::current_exe().map_err(|error| {
        AikitError::new(
            "gateway.service_install_binary_unresolved",
            format!("could not resolve the running aikit executable: {error}"),
        )
    })?;
    // Prefer the managed stable `aikit` command only when it resolves to this
    // exact running executable. A worktree build must never install an older
    // managed binary by accident.
    let binary = crate::probe::which("aikit")
        .filter(|managed| {
            match (
                std::fs::canonicalize(managed),
                std::fs::canonicalize(&running),
            ) {
                (Ok(managed), Ok(running)) => managed == running,
                _ => false,
            }
        })
        .unwrap_or(running);
    let environment = ServiceEnvironment::discover(home_dir, home)?;
    install_with(
        platform,
        &SystemServiceControl,
        home_dir,
        home,
        &binary,
        &environment,
        options,
    )
}

/// Install: validate, write the service definition, then hand it to the
/// service manager. A gateway already answering at the home socket is
/// refused — the kept-alive service would fight it over the endpoint. A
/// manager that refuses leaves no definition behind.
#[allow(clippy::too_many_arguments)]
pub fn install_with(
    platform: ServicePlatform,
    control: &dyn ServiceControl,
    home_dir: &Path,
    home: &aikit_store::AikitHome,
    binary: &Path,
    environment: &ServiceEnvironment,
    options: &ServiceOptions,
) -> Result<Value> {
    options.validate()?;
    let socket = home.gateway_socket();
    #[cfg(unix)]
    let stale_socket_removed = clear_stale_gateway_socket(home)?;
    #[cfg(not(unix))]
    let stale_socket_removed = false;
    let unit = platform.unit_path(home_dir);
    let write_failed = |path: &Path, error: std::io::Error| {
        AikitError::new(
            "gateway.service_install_write_failed",
            format!("{}: {error}", path.display()),
        )
    };
    if let Some(parent) = unit.parent() {
        std::fs::create_dir_all(parent).map_err(|error| write_failed(parent, error))?;
    }
    let rendered = match platform {
        ServicePlatform::LaunchAgent => {
            render_plist_for(binary, &log_path(home_dir), environment, options)
        }
        ServicePlatform::SystemdUser => render_systemd_unit(binary, environment, options),
    };
    std::fs::write(&unit, rendered).map_err(|error| write_failed(&unit, error))?;
    let unit_text = unit.display().to_string();
    let started = match platform {
        ServicePlatform::LaunchAgent => {
            let domain = gui_domain(control);
            let bootstrapped = run(control, "launchctl", &["bootstrap", &domain, &unit_text])?;
            if bootstrapped.status.success() {
                Ok(())
            } else {
                // Older launchd builds only speak `load`.
                let loaded = run(control, "launchctl", &["load", &unit_text])?;
                if loaded.status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "launchctl bootstrap and load both refused the agent: {}",
                        stderr_of(&bootstrapped)
                    ))
                }
            }
        }
        ServicePlatform::SystemdUser => {
            let reloaded = run(control, "systemctl", &["--user", "daemon-reload"])?;
            if !reloaded.status.success() {
                Err(format!(
                    "systemctl --user daemon-reload refused: {}",
                    stderr_of(&reloaded)
                ))
            } else {
                let enabled = run(
                    control,
                    "systemctl",
                    &["--user", "enable", "--now", SYSTEMD_UNIT_NAME],
                )?;
                if enabled.status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "systemctl --user enable --now {SYSTEMD_UNIT_NAME} refused: {}",
                        stderr_of(&enabled)
                    ))
                }
            }
        }
    };
    if let Err(reason) = started {
        let _ = std::fs::remove_file(&unit);
        if platform == ServicePlatform::SystemdUser {
            let _ = run(control, "systemctl", &["--user", "daemon-reload"]);
        }
        return Err(three_part(
            "gateway.service_install_start_failed",
            reason,
            format!("The service definition {} was removed again; nothing is installed.", unit.display()),
            match platform {
                ServicePlatform::LaunchAgent => "Check `launchctl print gui/$(id -u)` for a stale ai.aikit.gateway, then install again.".to_owned(),
                ServicePlatform::SystemdUser => "Check that a systemd user manager runs (`systemctl --user status`), then install again.".to_owned(),
            },
        ));
    }
    let mut command = vec![binary.display().to_string()];
    command.extend(options.serve_arguments());
    Ok(json!({
        "schema": SERVICE_VERSION,
        "action": "installed",
        "platform": platform.as_str(),
        "label": match platform {
            ServicePlatform::LaunchAgent => LAUNCH_AGENT_LABEL,
            ServicePlatform::SystemdUser => SYSTEMD_UNIT_NAME,
        },
        "unit": unit_text,
        "log": platform.log_location(home_dir),
        "binary": binary.display().to_string(),
        "command": command,
        "serves": {
            "unix": socket.display().to_string(),
            "websocket": options.websocket_bind,
            "token_location": options.token_location,
        },
        "environment": options
            .environment()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value)))
            .collect::<serde_json::Map<_, _>>(),
        "native_owners": environment
            .values
            .iter()
            .filter(|(key, _)| matches!(key.as_str(), "CENTRAL_CTRL_BIN" | "FACTORY_BIN" | "ACTUATION_BIN"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
        "aikit_home": home.root().display().to_string(),
        "central_root": environment.values.get("AIKIT_CENTRAL_ROOT"),
        "stale_socket_removed": stale_socket_removed,
        "note": match platform {
            ServicePlatform::LaunchAgent => "launchd keeps `aikit gateway serve` alive; the Routine dispatcher and the relay pass tick every 30 seconds",
            ServicePlatform::SystemdUser => "systemd keeps `aikit gateway serve` alive while your user manager runs; the Routine dispatcher and the relay pass tick every 30 seconds. To run it before you log in, the machine owner enables lingering (`loginctl enable-linger`)",
        },
    }))
}

/// Uninstall on this platform with the real service manager.
pub fn uninstall(home_dir: &Path) -> Result<Value> {
    uninstall_with(ServicePlatform::current()?, &SystemServiceControl, home_dir)
}

/// Stop the service, remove its definition. Refuses when nothing is
/// installed.
pub fn uninstall_with(
    platform: ServicePlatform,
    control: &dyn ServiceControl,
    home_dir: &Path,
) -> Result<Value> {
    let unit = platform.unit_path(home_dir);
    if !unit.exists() {
        return Err(AikitError::new(
            "gateway.service_not_installed",
            format!(
                "no gateway service is installed at {}; nothing to uninstall",
                unit.display()
            ),
        ));
    }
    let unit_text = unit.display().to_string();
    match platform {
        ServicePlatform::LaunchAgent => {
            let domain = gui_domain(control);
            let booted_out = run(
                control,
                "launchctl",
                &["bootout", &format!("{domain}/{LAUNCH_AGENT_LABEL}")],
            )?;
            if !booted_out.status.success() {
                let _ = run(control, "launchctl", &["unload", &unit_text]);
            }
        }
        ServicePlatform::SystemdUser => {
            let _ = run(
                control,
                "systemctl",
                &["--user", "disable", "--now", SYSTEMD_UNIT_NAME],
            )?;
        }
    }
    std::fs::remove_file(&unit).map_err(|error| {
        AikitError::new(
            "gateway.service_uninstall_remove_failed",
            format!("{}: {error}", unit.display()),
        )
    })?;
    if platform == ServicePlatform::SystemdUser {
        let _ = run(control, "systemctl", &["--user", "daemon-reload"]);
    }
    Ok(json!({
        "schema": SERVICE_VERSION,
        "action": "uninstalled",
        "platform": platform.as_str(),
        "label": match platform {
            ServicePlatform::LaunchAgent => LAUNCH_AGENT_LABEL,
            ServicePlatform::SystemdUser => SYSTEMD_UNIT_NAME,
        },
        "note": "the gateway is no longer kept alive; scheduled automations and the relay pass will not run until it is started again",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records every service-manager call and answers from a script.
    struct Recorded {
        calls: RefCell<Vec<String>>,
        fail: Option<&'static str>,
    }

    impl Recorded {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail: None,
            }
        }
    }

    #[cfg(unix)]
    fn status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }

    impl ServiceControl for Recorded {
        fn run(&self, program: &str, arguments: &[String]) -> Result<std::process::Output> {
            let line = format!("{program} {}", arguments.join(" "));
            self.calls.borrow_mut().push(line.clone());
            let failed = self.fail.is_some_and(|needle| line.contains(needle));
            Ok(std::process::Output {
                status: status(if failed { 1 } else { 0 }),
                stdout: if program == "id" {
                    b"502\n".to_vec()
                } else {
                    Vec::new()
                },
                stderr: if failed {
                    b"refused by fixture".to_vec()
                } else {
                    Vec::new()
                },
            })
        }
    }

    fn remote_options(dir: &Path) -> ServiceOptions {
        let token = dir.join("gateway.token");
        std::fs::write(&token, "loopback-token-0123456789").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        ServiceOptions {
            websocket_bind: Some("100.92.62.101:7800".into()),
            token_location: Some(format!("file:{}", token.display())),
            gateway_ref: Some("agency-gateway/omarchy".into()),
            workcell_ref: Some("workcell:omarchy".into()),
        }
    }

    fn home(dir: &Path) -> aikit_store::AikitHome {
        aikit_store::AikitHome::at(dir.join("aikit-home"))
    }

    fn owners() -> ServiceEnvironment {
        ServiceEnvironment {
            values: BTreeMap::from([
                ("AIKIT_CENTRAL_ROOT".into(), "/central".into()),
                ("CENTRAL_CTRL_BIN".into(), "/opt/central/ctrl".into()),
                ("PATH".into(), "/opt/central:/usr/bin:/bin".into()),
            ]),
        }
    }

    #[test]
    fn the_plist_serves_the_unix_socket_and_the_named_websocket_with_identity_in_its_environment() {
        let dir = tempfile::tempdir().unwrap();
        let options = remote_options(dir.path());
        let plist = render_plist_for(
            Path::new("/usr/local/bin/aikit"),
            Path::new("/Users/me/Library/Logs/ai.aikit.gateway.log"),
            &owners(),
            &options,
        );
        let arguments: Vec<&str> = plist
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("<string>")?.strip_suffix("</string>"))
            .collect();
        assert_eq!(
            &arguments[..9],
            &[
                "ai.aikit.gateway",
                "/usr/local/bin/aikit",
                "gateway",
                "serve",
                "--unix",
                "--ws",
                "100.92.62.101:7800",
                "--ws-token-location",
                options.token_location.as_deref().unwrap(),
            ]
        );
        assert!(plist.contains("<key>EnvironmentVariables</key>"));
        assert!(plist
            .contains("<key>AIKIT_WORKCELL_REF</key>\n      <string>workcell:omarchy</string>"));
        assert!(plist.contains(
            "<key>AIKIT_GATEWAY_REF</key>\n      <string>agency-gateway/omarchy</string>"
        ));
        // The native-owner relation rides beside the identity.
        assert!(
            plist.contains("<key>CENTRAL_CTRL_BIN</key>\n      <string>/opt/central/ctrl</string>")
        );
        assert!(
            plist.contains("<key>PATH</key>\n      <string>/opt/central:/usr/bin:/bin</string>")
        );
        assert!(
            !plist.contains("loopback-token"),
            "the token is never written"
        );
        assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
        assert_eq!(
            plist.matches("<dict>").count(),
            plist.matches("</dict>").count()
        );

        // With no options: the Unix socket only, and only the owner relation.
        let bare = render_plist(
            Path::new("/usr/local/bin/aikit"),
            Path::new("/tmp/log"),
            &owners(),
        );
        assert!(bare.contains("<string>--unix</string>"));
        assert!(!bare.contains("--ws"));
        assert!(!bare.contains("AIKIT_WORKCELL_REF"));
        assert!(bare.contains("<key>AIKIT_CENTRAL_ROOT</key>"));
    }

    #[test]
    fn the_systemd_unit_serves_both_carriers_restarts_and_is_wanted_by_the_default_target() {
        let dir = tempfile::tempdir().unwrap();
        let options = remote_options(dir.path());
        let unit = render_systemd_unit(Path::new("/home/me/.cargo/bin/aikit"), &owners(), &options);
        let exec = unit
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .unwrap();
        assert_eq!(
            exec,
            format!(
                "/home/me/.cargo/bin/aikit gateway serve --unix --ws 100.92.62.101:7800 --ws-token-location {}",
                options.token_location.as_deref().unwrap()
            )
        );
        assert!(unit.contains("Environment=AIKIT_WORKCELL_REF=workcell:omarchy\n"));
        assert!(unit.contains("Environment=AIKIT_GATEWAY_REF=agency-gateway/omarchy\n"));
        assert!(unit.contains("Environment=CENTRAL_CTRL_BIN=/opt/central/ctrl\n"));
        assert!(unit.contains("Environment=PATH=/opt/central:/usr/bin:/bin\n"));
        assert!(unit.contains("Restart=always\n"));
        assert!(unit.contains("[Install]\nWantedBy=default.target\n"));
        assert!(!unit.contains("loopback-token"));
        // Paths with spaces and systemd specifiers survive as one word.
        let odd = render_systemd_unit(
            Path::new("/opt/My Tools/aikit%1$x"),
            &ServiceEnvironment {
                values: BTreeMap::new(),
            },
            &ServiceOptions::default(),
        );
        assert!(
            odd.contains("ExecStart=\"/opt/My Tools/aikit%%1$$x\" gateway serve --unix\n"),
            "{odd}"
        );
    }

    #[test]
    fn a_websocket_needs_an_owner_only_token_location_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let control = Recorded::new();
        let home = home(dir.path());
        let mut options = remote_options(dir.path());
        options.token_location = None;
        let refused = install_with(
            ServicePlatform::SystemdUser,
            &control,
            dir.path(),
            &home,
            Path::new("/bin/aikit"),
            &owners(),
            &options,
        )
        .unwrap_err();
        assert_eq!(refused.code(), "gateway.service_ws_token_required");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let options = remote_options(dir.path());
            let token = options
                .token_location
                .as_deref()
                .unwrap()
                .trim_start_matches("file:")
                .to_owned();
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o644)).unwrap();
            let refused = install_with(
                ServicePlatform::LaunchAgent,
                &control,
                dir.path(),
                &home,
                Path::new("/bin/aikit"),
                &owners(),
                &options,
            )
            .unwrap_err();
            assert_eq!(refused.code(), "gateway.service_token_unusable");
            assert!(refused.to_string().contains("owner only"), "{refused}");
        }
        assert!(
            control.calls.borrow().is_empty(),
            "no service manager was asked"
        );
        assert!(!systemd_unit_path(dir.path()).exists());
        assert!(!plist_path(dir.path()).exists());
    }

    #[test]
    fn systemd_install_and_uninstall_are_exact_inverses() {
        let dir = tempfile::tempdir().unwrap();
        let control = Recorded::new();
        let home = home(dir.path());
        let options = remote_options(dir.path());
        let receipt = install_with(
            ServicePlatform::SystemdUser,
            &control,
            dir.path(),
            &home,
            Path::new("/bin/aikit"),
            &owners(),
            &options,
        )
        .unwrap();
        let unit = systemd_unit_path(dir.path());
        assert!(unit.ends_with(".config/systemd/user/aikit-gateway.service"));
        assert_eq!(
            std::fs::read_to_string(&unit).unwrap(),
            render_systemd_unit(Path::new("/bin/aikit"), &owners(), &options)
        );
        assert_eq!(receipt["central_root"], "/central");
        assert_eq!(
            receipt["native_owners"]["CENTRAL_CTRL_BIN"],
            "/opt/central/ctrl"
        );
        assert_eq!(receipt["platform"], "systemd-user-unit");
        assert_eq!(receipt["serves"]["websocket"], "100.92.62.101:7800");
        assert_eq!(
            receipt["environment"]["AIKIT_WORKCELL_REF"],
            "workcell:omarchy"
        );
        assert_eq!(
            control.calls.borrow().as_slice(),
            &[
                "systemctl --user daemon-reload".to_owned(),
                "systemctl --user enable --now aikit-gateway.service".to_owned(),
            ]
        );
        control.calls.borrow_mut().clear();
        uninstall_with(ServicePlatform::SystemdUser, &control, dir.path()).unwrap();
        assert!(!unit.exists());
        assert_eq!(
            control.calls.borrow().as_slice(),
            &[
                "systemctl --user disable --now aikit-gateway.service".to_owned(),
                "systemctl --user daemon-reload".to_owned(),
            ]
        );
        assert_eq!(
            uninstall_with(ServicePlatform::SystemdUser, &control, dir.path())
                .unwrap_err()
                .code(),
            "gateway.service_not_installed"
        );
    }

    #[test]
    fn launch_agent_install_bootstraps_into_the_gui_domain_and_uninstall_boots_it_out() {
        let dir = tempfile::tempdir().unwrap();
        let control = Recorded::new();
        let home = home(dir.path());
        install_with(
            ServicePlatform::LaunchAgent,
            &control,
            dir.path(),
            &home,
            Path::new("/bin/aikit"),
            &owners(),
            &ServiceOptions::default(),
        )
        .unwrap();
        let plist = plist_path(dir.path());
        assert!(plist.exists());
        assert_eq!(
            control.calls.borrow().as_slice(),
            &[
                "id -u".to_owned(),
                format!("launchctl bootstrap gui/502 {}", plist.display()),
            ]
        );
        control.calls.borrow_mut().clear();
        uninstall_with(ServicePlatform::LaunchAgent, &control, dir.path()).unwrap();
        assert!(!plist.exists());
        assert_eq!(
            control.calls.borrow().as_slice(),
            &[
                "id -u".to_owned(),
                "launchctl bootout gui/502/ai.aikit.gateway".to_owned()
            ]
        );
    }

    #[test]
    fn a_manager_that_refuses_leaves_no_definition_behind() {
        let dir = tempfile::tempdir().unwrap();
        let control = Recorded {
            calls: RefCell::new(Vec::new()),
            fail: Some("enable --now"),
        };
        let home = home(dir.path());
        let refused = install_with(
            ServicePlatform::SystemdUser,
            &control,
            dir.path(),
            &home,
            Path::new("/bin/aikit"),
            &owners(),
            &ServiceOptions::default(),
        )
        .unwrap_err();
        assert_eq!(refused.code(), "gateway.service_install_start_failed");
        assert!(refused.to_string().contains("refused by fixture"));
        assert!(!systemd_unit_path(dir.path()).exists());
        assert_eq!(
            control.calls.borrow().last().map(String::as_str),
            Some("systemctl --user daemon-reload")
        );
    }

    #[cfg(unix)]
    #[test]
    fn installer_socket_boundary_preserves_live_and_other_paths_but_clears_real_stale_socket() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::{UnixListener, UnixStream};

        let dir = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(dir.path());
        std::fs::create_dir_all(home.state()).unwrap();
        let socket = home.gateway_socket();

        let listener = UnixListener::bind(&socket).unwrap();
        let conflict = clear_stale_gateway_socket(&home).unwrap_err();
        assert_eq!(conflict.code(), "gateway.service_install_conflict");
        assert!(
            UnixStream::connect(&socket).is_ok(),
            "live listener must remain reachable"
        );
        drop(listener);

        let owner_lock = aikit_adapters::acquire_gateway_state_lock(
            &home.gateway_state(),
            Duration::from_millis(100),
            "real lock regression test",
        )
        .unwrap();
        let conflict = clear_stale_gateway_socket(&home).unwrap_err();
        assert_eq!(conflict.code(), "gateway.service_install_conflict");
        assert!(
            socket.exists(),
            "a held native owner lock preserves the socket"
        );
        drop(owner_lock);

        assert!(
            clear_stale_gateway_socket(&home).unwrap(),
            "closed real Unix listener leaves a stale path"
        );
        assert!(!socket.exists());
        assert!(
            !clear_stale_gateway_socket(&home).unwrap(),
            "a missing socket needs no cleanup"
        );

        std::fs::write(&socket, b"owner material").unwrap();
        let conflict = clear_stale_gateway_socket(&home).unwrap_err();
        assert_eq!(conflict.code(), "gateway.service_install_conflict");
        assert_eq!(std::fs::read(&socket).unwrap(), b"owner material");

        std::fs::remove_file(&socket).unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"symlink target").unwrap();
        symlink(&target, &socket).unwrap();
        let conflict = clear_stale_gateway_socket(&home).unwrap_err();
        assert_eq!(conflict.code(), "gateway.service_install_conflict");
        assert!(std::fs::symlink_metadata(&socket)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"symlink target");
    }
}
