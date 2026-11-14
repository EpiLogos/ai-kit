//! The resident encounter owner must boot through the invocation shape the
//! running binary actually accepts. Since the O-I #376 fold the main `aikit`
//! binary only accepts `encounter-serve` nested under `session-space`; the
//! companion-era top-level spawn made every `encounter-start` die with clap's
//! "unrecognized subcommand 'encounter-serve'" and the owner exiting 2.
#![cfg(unix)]

use aikit_cli::encounter_service::{request, socket_path, EncounterRequest};
use aikit_store::AikitHome;
use std::path::Path;
use std::process::{Command, Stdio};
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

/// A World removed while something still asks for its owner (O-I #1001): the kernel, or any door,
/// runs `encounter-start`, or the owner itself `encounter-serve`, with the World's home after the
/// World's folder is gone. Nothing may bring the World back: no folder is created where it stood
/// and no owner is started to serve it, and the refusal names the folder that is gone.
#[test]
fn an_owner_is_never_started_in_a_world_that_is_gone() {
    let temp = tempfile::tempdir().unwrap();
    let world = temp.path().join("world");
    let home = world.join("home").join(".aikit");
    let cwd = world.join("home").join("Central");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::remove_dir_all(&world).unwrap();
    for verb in ["encounter-start", "encounter-serve"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .args(["session-space", verb, "-C"])
            .arg(&cwd)
            .env("AIKIT_HOME", &home)
            // An owner started here anyway closes itself a second after its last request.
            .env("AIKIT_ENCOUNTER_IDLE_CLOSE_SECONDS", "1")
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("AIKIT_ISOLATION")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{verb} in a removed World did not return: an owner is serving it");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            // An owner was started in the removed World: stop it before failing.
            let pid = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                .ok()
                .and_then(|started| started["data"]["pid"].as_u64());
            if let Some(pid) = pid {
                let _ = request(
                    &socket_path(&AikitHome::at(&home)),
                    &EncounterRequest::Shutdown {
                        expected_pid: pid as u32,
                    },
                );
            }
        }
        assert!(
            !output.status.success() && stderr.contains("home.ground_gone"),
            "{verb} in a removed World must be refused, naming why: {} {stdout} {stderr}",
            output.status
        );
        let named = format!("({})", world.join("home").display());
        assert!(
            stderr.contains(&named),
            "the refusal names the folder that is gone, {named}: {stderr}"
        );
        assert!(!world.exists(), "{verb} created the removed World again");
    }
}

/// Every command opens AIKit through the home it finds, so in a World that has been removed every
/// command is refused before anything is made (O-I #1001): the read a window makes when its change
/// watch announces a move (the World Model's Models slice runs `aikit model-catalogue show`), which
/// lays out the home, and a command that never lays it out but makes its own folder in it
/// (`observe status` opens the index under `state/`). Either would make the removed World again,
/// after which the owner's own guard would pass.
#[test]
fn no_command_brings_back_a_world_that_is_gone() {
    let temp = tempfile::tempdir().unwrap();
    let world = temp.path().join("world");
    let home = world.join("home").join(".aikit");
    let cwd = world.join("home").join("Central");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::remove_dir_all(&world).unwrap();
    for command in [
        ["model-catalogue", "show", "--json"],
        ["observe", "status", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .args(command)
            .current_dir(temp.path())
            .env("AIKIT_HOME", &home)
            .env("HOME", world.join("home"))
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("AIKIT_ISOLATION")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && format!("{stdout}{stderr}").contains("home.ground_gone"),
            "{command:?} in a removed World must be refused, naming why: {} {stdout} {stderr}",
            output.status
        );
        assert!(
            !world.exists(),
            "{command:?} created the removed World again"
        );
    }
}

/// A read of the code lens that names its GitNexus binary (`aikit knowledge code …
/// --gitnexus-binary`) remembers that binary's surface in the AIKit home's cache. The read runs
/// before any home is found, so the cache is kept only in a home that stands: in a World that has
/// been removed nothing is made for it and the World does not come back (O-I #1001). In a home that
/// stands, the surface is still remembered.
#[test]
fn a_code_lens_read_never_makes_a_removed_world_s_home_for_its_cache() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let read = |home: &Path, user_home: &Path| {
        Command::new(env!("CARGO_BIN_EXE_aikit"))
            .args(["knowledge", "code", "status", "--repo"])
            .arg(&repo)
            .args(["--gitnexus-binary", "/usr/bin/true", "--json"])
            .current_dir(temp.path())
            .env("AIKIT_HOME", home)
            .env("HOME", user_home)
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("AIKIT_ISOLATION")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };

    let world = temp.path().join("world");
    std::fs::create_dir_all(world.join("home")).unwrap();
    std::fs::remove_dir_all(&world).unwrap();
    let output = read(&world.join("home").join(".aikit"), &world.join("home"));
    assert!(
        !world.exists(),
        "the code lens read made the removed World again: {} {} {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let standing = temp.path().join("standing").join("home");
    std::fs::create_dir_all(standing.join(".aikit")).unwrap();
    let output = read(&standing.join(".aikit"), &standing);
    assert!(
        standing
            .join(".aikit/cache/gitnexus-cli-surface.json")
            .is_file(),
        "a home that stands keeps the surface: {} {} {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
