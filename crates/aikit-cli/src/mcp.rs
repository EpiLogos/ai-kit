//! Native MCP projections owned by AIKit.
//!
//! This module intentionally starts with one small, fully retractable
//! projection: the Bimba map. It uses each installed harness's own MCP CLI
//! rather than guessing its private configuration grammar, records exactly
//! which harnesses were changed, and gives Bimba a fail-closed selection record
//! for every request after startup.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const STATE_SCHEMA: &str = "aikit.bimba-map-mcp/v1";
const MCP_NAME: &str = "bimba-map";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BimbaMapState {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    selected: bool,
    #[serde(default)]
    installed_clients: Vec<String>,
}

fn state_path(home: &AikitHome) -> PathBuf {
    home.state().join("bimba-map-mcp.json")
}

fn bimba_root() -> Result<PathBuf> {
    let user_home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AikitError::new("mcp.home_unavailable", "HOME is not set"))?;
    Ok(user_home.join(".local/share/bimba-mcp/current"))
}

fn health_script() -> Result<PathBuf> {
    Ok(bimba_root()?.join("dist/health.js"))
}

fn harness_script() -> Result<PathBuf> {
    // Bimba 2.0's explicit legacy adapter retains the current Claude/Codex
    // protocol handshake while the Bimba application and selection gate remain
    // version 0.2.0. The modern entrypoint stays available to 2026-07-28 clients.
    Ok(bimba_root()?.join("dist/legacy.js"))
}

fn read_state(home: &AikitHome) -> Result<BimbaMapState> {
    let path = state_path(home);
    match fs::read(&path) {
        Ok(bytes) => {
            let state: BimbaMapState = serde_json::from_slice(&bytes).map_err(|error| {
                AikitError::new(
                    "mcp.bimba_state_invalid",
                    format!(
                        "{} is not a valid Bimba MCP selection record: {error}",
                        path.display()
                    ),
                )
            })?;
            if state.schema != STATE_SCHEMA {
                return Err(AikitError::new(
                    "mcp.bimba_state_invalid",
                    format!("{} has an unsupported selection schema", path.display()),
                ));
            }
            Ok(state)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BimbaMapState {
            schema: STATE_SCHEMA.to_owned(),
            ..BimbaMapState::default()
        }),
        Err(error) => Err(io_error(&path, error)),
    }
}

fn write_state(home: &AikitHome, state: &BimbaMapState) -> Result<()> {
    let path = state_path(home);
    let parent = path.parent().expect("state path has a parent");
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| AikitError::new("mcp.bimba_state_encode", error.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|error| io_error(&temporary, error))?;
    fs::rename(&temporary, &path).map_err(|error| io_error(&path, error))
}

fn map_health() -> Value {
    let Ok(script) = health_script() else {
        return json!({ "healthy": false, "detail": "Bimba deployment path is unavailable" });
    };
    if !script.is_file() {
        return json!({ "healthy": false, "detail": format!("missing {}", script.display()) });
    }
    let output = Command::new("node")
        .arg(script)
        .env("NEO4J_URI", "bolt://127.0.0.1:7687")
        .env("NEO4J_AUTH_MODE", "none")
        .output();
    let Ok(output) = output else {
        return json!({ "healthy": false, "detail": "could not run node for Bimba health" });
    };
    let mut health: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| json!({ "healthy": false }));
    let healthy = output.status.success() && health["healthy"] == json!(true);
    health["healthy"] = json!(healthy);
    if !healthy && health.get("detail").is_none() {
        health["detail"] = json!("Neo4j map did not pass Bimba health check");
    }
    health
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("mcp")
        .arg("--help")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn projection_environment(state: &Path) -> Vec<(String, String)> {
    vec![
        ("NEO4J_URI".to_owned(), "bolt://127.0.0.1:7687".to_owned()),
        ("NEO4J_AUTH_MODE".to_owned(), "none".to_owned()),
        ("BIMBA_MCP_PRINCIPAL".to_owned(), "omarchy-map".to_owned()),
        ("BIMBA_MCP_PERMISSIONS".to_owned(), "bimba:read".to_owned()),
        (
            "BIMBA_MCP_SELECTION_STATE".to_owned(),
            state.display().to_string(),
        ),
    ]
}

fn run_native(program: &str, args: &[String]) -> Result<()> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        AikitError::new(
            "mcp.native_command_failed",
            format!("could not run {program}: {error}"),
        )
    })?;
    if output.status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(AikitError::new(
        "mcp.native_command_failed",
        format!(
            "{program} rejected Bimba MCP projection{}",
            if diagnostic.is_empty() {
                String::new()
            } else {
                format!(": {diagnostic}")
            }
        ),
    ))
}

fn add_client(client: &str, state: &Path, modern: &Path) -> Result<()> {
    let environment = projection_environment(state);
    let mut args = match client {
        "claude" => vec![
            "mcp".to_owned(),
            "add".to_owned(),
            MCP_NAME.to_owned(),
            "--scope".to_owned(),
            "user".to_owned(),
        ],
        "codex" => vec!["mcp".to_owned(), "add".to_owned()],
        _ => return Err(AikitError::new("mcp.unsupported_client", client.to_owned())),
    };
    for (key, value) in &environment {
        if client == "claude" {
            args.extend(["--env".to_owned(), format!("{key}={value}")]);
        } else {
            args.extend(["--env".to_owned(), format!("{key}={value}")]);
        }
    }
    if client == "codex" {
        args.push(MCP_NAME.to_owned());
    }
    args.push("--".to_owned());
    args.push("node".to_owned());
    args.push(modern.display().to_string());
    run_native(client, &args)
}

fn remove_client(client: &str) -> Result<()> {
    let args = match client {
        "claude" => vec![
            "mcp".to_owned(),
            "remove".to_owned(),
            "--scope".to_owned(),
            "user".to_owned(),
            MCP_NAME.to_owned(),
        ],
        "codex" => vec!["mcp".to_owned(), "remove".to_owned(), MCP_NAME.to_owned()],
        _ => return Err(AikitError::new("mcp.unsupported_client", client.to_owned())),
    };
    run_native(client, &args)
}

fn io_error(path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new("mcp.filesystem", format!("{}: {error}", path.display()))
}

pub fn bimba_map_status(home: &AikitHome) -> Result<Value> {
    let state = read_state(home)?;
    let health = map_health();
    let healthy = health["healthy"] == json!(true);
    let available_clients = ["claude", "codex", "zcode"]
        .into_iter()
        .filter(|client| command_available(client))
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": STATE_SCHEMA,
        "selected": state.selected,
        "map": health,
        "active": state.selected && healthy,
        "selection_record": state_path(home).display().to_string(),
        "native_clients": {
            "projected": state.installed_clients,
            "available": available_clients,
        },
        "reload": "new Claude and Codex sessions read the native MCP configuration; already-running Bimba requests are selection-gated",
    }))
}

pub fn select_bimba_map(home: &AikitHome) -> Result<Value> {
    let health = map_health();
    if health["healthy"] != json!(true) {
        return Err(AikitError::new(
            "mcp.bimba_map_unhealthy",
            "Bimba map is not healthy; selection was not changed",
        ));
    }
    let mut state = read_state(home)?;
    // Establish fail-closed state before any client can launch the server.
    state.selected = false;
    write_state(home, &state)?;
    let harness = harness_script()?;
    if !harness.is_file() {
        return Err(AikitError::new(
            "mcp.bimba_deployment_missing",
            format!("missing {}", harness.display()),
        ));
    }

    let mut added: Vec<String> = Vec::new();
    for client in ["claude", "codex"] {
        if state
            .installed_clients
            .iter()
            .any(|installed| installed == client)
        {
            continue;
        }
        if !command_available(client) {
            continue;
        }
        if let Err(error) = add_client(client, &state_path(home), &harness) {
            for previous in added.iter().rev() {
                let _ = remove_client(previous);
            }
            return Err(error);
        }
        added.push(client.to_owned());
    }
    if state.installed_clients.is_empty() && added.is_empty() {
        return Err(AikitError::new(
            "mcp.no_native_client",
            "Claude Code or Codex CLI with native MCP support is required",
        ));
    }
    state.installed_clients.extend(added);
    state.installed_clients.sort();
    state.installed_clients.dedup();
    state.selected = true;
    write_state(home, &state)?;
    bimba_map_status(home)
}

pub fn deselect_bimba_map(home: &AikitHome) -> Result<Value> {
    let mut state = read_state(home)?;
    // Write the gate first. A stale native configuration can no longer reach Bimba.
    state.selected = false;
    write_state(home, &state)?;
    let clients = state.installed_clients.clone();
    let mut failures = Vec::new();
    for client in clients {
        match remove_client(&client) {
            Ok(()) => state
                .installed_clients
                .retain(|installed| installed != &client),
            Err(error) => failures.push(error.message().to_owned()),
        }
        write_state(home, &state)?;
    }
    if !failures.is_empty() {
        return Err(AikitError::new(
            "mcp.native_retraction_incomplete",
            format!(
                "Bimba is deselected and request-gated, but native retraction needs retry: {}",
                failures.join("; ")
            ),
        ));
    }
    bimba_map_status(home)
}
