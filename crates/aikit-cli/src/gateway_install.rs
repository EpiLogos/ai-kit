//! `aikit gateway install-service | uninstall-service` — the macOS user
//! LaunchAgent that keeps the gateway (and with it the Routine dispatcher's
//! 30-second tick) alive across restart, sleep and reboot.
//!
//! The agent runs `aikit gateway serve` with no flags: the well-known default
//! endpoint (`~/.aikit/state/gateway.sock`) is the whole posture. KeepAlive
//! means launchd restarts the process if it exits, which is exactly the
//! persistent-lifetime proof the CAW native-delivery join asks for. Uninstall
//! is the exact inverse and refuses when nothing is installed.

use std::path::PathBuf;

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

/// Render the LaunchAgent plist. `binary` is the exact executable launchd
/// should run; args mirror a bare `aikit gateway serve`.
pub fn render_plist(binary: &std::path::Path, log: &std::path::Path) -> String {
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

/// Install the LaunchAgent: write the plist, then bootstrap it into the user's
/// GUI domain. A gateway already answering at the default socket is refused —
/// KeepAlive would fight it over the endpoint.
pub fn install(home_dir: &std::path::Path, home: &aikit_store::AikitHome) -> Result<Value> {
    assert_macos()?;
    let binary = std::env::current_exe().map_err(|error| {
        AikitError::new(
            "gateway.service_install_binary_unresolved",
            format!("could not resolve the running aikit executable: {error}"),
        )
    })?;
    let socket = home.gateway_socket();
    if socket.exists() {
        return Err(AikitError::new(
            "gateway.service_install_conflict",
            format!(
                "a gateway is already answering at {}; stop it (`aikit gateway serve` owns that \
                 socket) before installing the persistent service",
                socket.display()
            ),
        ));
    }
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
    std::fs::write(&plist, render_plist(&binary, &log)).map_err(|error| {
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
}
