//! `aikit gateway install-service | uninstall-service` — the macOS user
//! LaunchAgent that keeps the gateway (and with it the Routine dispatcher's
//! 30-second tick) alive across restart, sleep and reboot.
//!
//! The agent runs `aikit gateway serve` with no flags: the well-known default
//! endpoint (`~/.aikit/state/gateway.sock`) is the whole posture. KeepAlive
//! means launchd restarts the process if it exits, which is exactly the
//! persistent-lifetime proof the CAW native-delivery join asks for. Uninstall
//! is the exact inverse and refuses when nothing is installed.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

pub const LAUNCH_AGENT_LABEL: &str = "ai.aikit.gateway";
pub const SERVICE_VERSION: &str = "aikit.gateway-service-install/v1";

/// The LaunchAgent plist path under the given home directory.
pub fn plist_path(home_dir: &std::path::Path) -> PathBuf {
    home_dir
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// The gateway's log file under the given home directory.
pub fn log_path(home_dir: &std::path::Path) -> PathBuf {
    home_dir
        .join("Library/Logs")
        .join(format!("{LAUNCH_AGENT_LABEL}.log"))
}

/// The only process material carried into launchd. Credentials are resolved
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
    /// launchd receives only discovered owner directories and system paths.
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
                format!("native owner search path cannot be carried to launchd: {error}"),
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

/// Render the LaunchAgent plist. `binary` is the exact executable launchd
/// should run; args mirror a bare `aikit gateway serve`.
pub fn render_plist(binary: &Path, log: &Path, environment: &ServiceEnvironment) -> String {
    let program_arguments = [
        binary.display().to_string(),
        "gateway".into(),
        "serve".into(),
    ];
    let esc = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let arguments = program_arguments
        .iter()
        .map(|argument| format!("        <string>{}</string>", esc(argument)))
        .collect::<Vec<_>>()
        .join("\n");
    let variables = environment
        .values
        .iter()
        .map(|(key, value)| {
            format!(
                "        <key>{}</key>\n        <string>{}</string>",
                esc(key),
                esc(value)
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
{variables}
    </dict>
    <key>RunAtLoad</key>
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
        variables = variables,
        log = esc(&log.display().to_string()),
    )
}

/// The launchd GUI domain for this user (`gui/<uid>`).
fn gui_domain() -> String {
    format!("gui/{}", nix_uid())
}

fn nix_uid() -> u32 {
    // std has no uid(); read it through the one-call contract every macOS
    // machine answers.
    let output = std::process::Command::new("id").arg("-u").output();
    output
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(501)
}

fn launchctl(arguments: &[&str]) -> Result<std::process::Output> {
    std::process::Command::new("launchctl")
        .args(arguments)
        .output()
        .map_err(|error| {
            AikitError::new(
                "gateway.launchctl_unavailable",
                format!("could not run launchctl: {error}"),
            )
        })
}

fn assert_macos() -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(AikitError::new(
            "gateway.service_install_unsupported",
            "the LaunchAgent installer is the macOS path; manage the gateway as a Workcell \
             service on other platforms",
        ));
    }
    Ok(())
}

/// Is the LaunchAgent installed at its plist path?
pub fn is_installed(home_dir: &std::path::Path) -> bool {
    plist_path(home_dir).exists()
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

/// Install the LaunchAgent: write the plist, then bootstrap it into the user's
/// GUI domain. A gateway already answering at the default socket is refused —
/// KeepAlive would fight it over the endpoint. A proven stale socket from an
/// exited owner is cleared under the native Gateway state lock.
pub fn install(home_dir: &std::path::Path, home: &aikit_store::AikitHome) -> Result<Value> {
    assert_macos()?;
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
    #[cfg(unix)]
    let stale_socket_removed = clear_stale_gateway_socket(home)?;
    #[cfg(not(unix))]
    let stale_socket_removed = false;
    let plist = plist_path(home_dir);
    let log = log_path(home_dir);
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "gateway.service_install_write_failed",
                format!("{}: {error}", parent.display()),
            )
        })?;
    }
    std::fs::write(&plist, render_plist(&binary, &log, &environment)).map_err(|error| {
        AikitError::new(
            "gateway.service_install_write_failed",
            format!("{}: {error}", plist.display()),
        )
    })?;
    let domain = gui_domain();
    let bootstrapped = launchctl(&["bootstrap", &domain, &plist.display().to_string()])?;
    if !bootstrapped.status.success() {
        // Older launchd builds only speak `load`.
        let loaded = launchctl(&["load", &plist.display().to_string()])?;
        if !loaded.status.success() {
            let _ = std::fs::remove_file(&plist);
            return Err(AikitError::new(
                "gateway.service_install_bootstrap_failed",
                format!(
                    "launchctl bootstrap and load both refused the agent: {}",
                    String::from_utf8_lossy(&bootstrapped.stderr).trim()
                ),
            ));
        }
    }
    Ok(json!({
        "schema": SERVICE_VERSION,
        "action": "installed",
        "label": LAUNCH_AGENT_LABEL,
        "plist": plist.display().to_string(),
        "log": log.display().to_string(),
        "binary": binary.display().to_string(),
        "native_owners": environment.values.iter().filter(|(key, _)| matches!(key.as_str(), "CENTRAL_CTRL_BIN" | "FACTORY_BIN" | "ACTUATION_BIN")).map(|(key, value)| (key.clone(), value.clone())).collect::<BTreeMap<_, _>>(),
        "aikit_home": home.root().display().to_string(),
        "stale_socket_removed": stale_socket_removed,
        "central_root": environment.values["AIKIT_CENTRAL_ROOT"],
        "note": "launchd keeps `aikit gateway serve` alive; the Routine dispatcher ticks every 30 seconds",
    }))
}

/// Uninstall the LaunchAgent: boot it out of the GUI domain and remove the
/// plist. Refuses when nothing is installed.
pub fn uninstall(home_dir: &std::path::Path) -> Result<Value> {
    assert_macos()?;
    let plist = plist_path(home_dir);
    if !plist.exists() {
        return Err(AikitError::new(
            "gateway.service_not_installed",
            format!(
                "no LaunchAgent is installed at {}; nothing to uninstall",
                plist.display()
            ),
        ));
    }
    let domain = gui_domain();
    let booted_out = launchctl(&["bootout", &format!("{domain}/{LAUNCH_AGENT_LABEL}")])?;
    if !booted_out.status.success() {
        let _ = launchctl(&["unload", &plist.display().to_string()]);
    }
    std::fs::remove_file(&plist).map_err(|error| {
        AikitError::new(
            "gateway.service_uninstall_remove_failed",
            format!("{}: {error}", plist.display()),
        )
    })?;
    Ok(json!({
        "schema": SERVICE_VERSION,
        "action": "uninstalled",
        "label": LAUNCH_AGENT_LABEL,
        "note": "the gateway is no longer kept alive; scheduled automations will not fire until it is started again",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_render_is_valid_xml_with_keepalive_and_serve_arguments() {
        let plist = render_plist(
            std::path::Path::new("/usr/local/bin/aikit"),
            std::path::Path::new("/Users/me/Library/Logs/ai.aikit.gateway.log"),
            &ServiceEnvironment {
                values: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            },
        );
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains(LAUNCH_AGENT_LABEL));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<true/>"));
        assert!(plist.contains("<string>/usr/local/bin/aikit</string>"));
        assert!(plist.contains("<string>gateway</string>"));
        assert!(plist.contains("<string>serve</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        // The document parses as XML.
        let reader = quick_xml_check(&plist);
        assert!(reader.is_ok(), "rendered plist must be well-formed XML");
    }

    // Minimal well-formedness probe without adding a dependency: pair up the
    // tags the renderer emits.
    fn quick_xml_check(document: &str) -> std::result::Result<(), String> {
        let opens = document.matches("<dict>").count();
        let closes = document.matches("</dict>").count();
        if opens != closes || opens == 0 {
            return Err(format!("dict tags unbalanced: {opens}/{closes}"));
        }
        let plist_opens = document.matches("<plist").count();
        let plist_closes = document.matches("</plist>").count();
        if plist_opens != plist_closes {
            return Err("plist tags unbalanced".into());
        }
        Ok(())
    }

    #[test]
    fn uninstall_refuses_when_nothing_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let error = uninstall(dir.path()).unwrap_err();
        #[cfg(target_os = "macos")]
        assert_eq!(error.code(), "gateway.service_not_installed");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(error.code(), "gateway.service_install_unsupported");
        assert!(!is_installed(dir.path()));
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
