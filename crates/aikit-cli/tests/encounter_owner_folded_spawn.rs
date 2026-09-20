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

#[test]
fn surface_invocation_prefix_names_the_two_products() {
    // The standalone companion's top level is the session-space surface itself.
    assert!(
        aikit_cli::session_space_cli::surface_invocation_prefix(Path::new(
            "/installed/bin/aikit-session-space"
        ))
        .is_empty()
    );
    // The folded main binary reaches the surface only under `session-space`.
    assert_eq!(
        aikit_cli::session_space_cli::surface_invocation_prefix(Path::new("/installed/bin/aikit")),
        vec![std::ffi::OsString::from("session-space")]
    );
    // Unknown names take the folded shape, the surface's primary invocation.
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

    // The exact production path: the folded binary's encounter-start spawns
    // `aikit session-space -C <cwd> encounter-serve` as the resident owner.
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

    // The resident owns the canonical socket for this home and answers IPC.
    let socket = socket_path(&AikitHome::at(&home));
    let health = request(&socket, &EncounterRequest::Health).unwrap();
    assert_eq!(health["ok"], true, "owner not reachable: {health}");
    assert_eq!(health["data"]["pid"].as_u64(), Some(pid as u64));

    // Shut the resident down through its documented IPC path; no orphan stays.
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true, "owner shutdown failed: {ack}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while socket.exists() {
        assert!(Instant::now() < deadline, "owner socket was not released");
        std::thread::sleep(Duration::from_millis(20));
    }
    let reaped = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&reaped.stdout);
    assert!(
        state.trim().is_empty() || state.trim().starts_with('Z'),
        "owner survived shutdown: {pid} {state}"
    );
}

#[test]
fn companion_binary_encounter_start_still_boots_the_real_owner() {
    // The standalone companion keeps its companion-era top-level spawn shape.
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
    let ack = request(&socket, &EncounterRequest::Shutdown { expected_pid: pid }).unwrap();
    assert_eq!(ack["ok"], true, "owner shutdown failed: {ack}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while socket.exists() {
        assert!(Instant::now() < deadline, "owner socket was not released");
        std::thread::sleep(Duration::from_millis(20));
    }
}
