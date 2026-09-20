//! The resident encounter owner must boot through the invocation shape the
//! running binary actually accepts. Since the O-I #376 fold the main `aikit`
//! binary only accepts `encounter-serve` nested under `session-space`; the
//! companion-era top-level spawn made every `encounter-start` die with clap's
//! "unrecognized subcommand 'encounter-serve'" and the owner exiting 2.
#![cfg(unix)]

use aikit_cli::encounter_service::{request, socket_path, EncounterRequest};
use aikit_store::AikitHome;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// Releasing the socket and exiting the process are distinct observations.
/// Keep both requirements and the original five-second shutdown deadline.
fn await_owner_exit(socket: &Path, pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let reaped = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        // ps returns 1 for an absent PID; other failures cannot prove exit.
        assert!(
            matches!(reaped.status.code(), Some(0 | 1)),
            "cannot observe owner exit: {}",
            String::from_utf8_lossy(&reaped.stderr)
        );
        let state = String::from_utf8_lossy(&reaped.stdout);
        let exited = state.trim().is_empty() || state.trim().starts_with('Z');
        if !socket.exists() && exited {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owner shutdown incomplete: pid={pid}, socket_exists={}, state={state}",
            socket.exists()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn surface_invocation_prefix_names_the_two_products() {
    assert!(
        aikit_cli::session_space_cli::surface_invocation_prefix(Path::new(
            "/installed/bin/aikit-session-space"
        ))
        .is_empty()
    );
    assert_eq!(
        aikit_cli::session_space_cli::surface_invocation_prefix(Path::new("/installed/bin/aikit")),
        vec![std::ffi::OsString::from("session-space")]
    );
    assert_eq!(
        aikit_cli::session_space_cli::surface_invocation_prefix(Path::new("/tmp/renamed-aikit")),
        vec![std::ffi::OsString::from("session-space")]
    );
}

#[test]
fn folded_binary_encounter_start_boots_the_real_owner_and_shuts_it_down() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    std::fs::create_dir(&cwd).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .args(["session-space", "encounter-start", "-C"])
        .arg(&cwd)
        .env("AIKIT_HOME", &home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "folded encounter-start failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let started: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(started["ok"], true, "encounter-start not ok: {started}");
    let pid = started["data"]["pid"].as_u64().expect("owner pid") as u32;
    let socket = socket_path(&AikitHome::at(&home));
    let health = request(&socket, &EncounterRequest::Health).unwrap();
    assert_eq!(health["ok"], true, "owner not reachable: {health}");
    assert_eq!(health["data"]["pid"].as_u64(), Some(pid as u64));
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true, "owner shutdown failed: {ack}");
    await_owner_exit(&socket, pid);
}

#[test]
fn companion_binary_encounter_start_still_boots_the_real_owner() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    std::fs::create_dir(&cwd).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
        .arg("-C")
        .arg(&cwd)
        .arg("encounter-start")
        .env("AIKIT_HOME", &home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "companion encounter-start failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let started: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(started["ok"], true, "encounter-start not ok: {started}");
    let pid = started["data"]["pid"].as_u64().expect("owner pid") as u32;
    let socket = socket_path(&AikitHome::at(&home));
    let health = request(&socket, &EncounterRequest::Health).unwrap();
    assert_eq!(health["ok"], true, "owner not reachable: {health}");
    assert_eq!(health["data"]["pid"].as_u64(), Some(pid as u64));
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true, "owner shutdown failed: {ack}");
    await_owner_exit(&socket, pid);
}
