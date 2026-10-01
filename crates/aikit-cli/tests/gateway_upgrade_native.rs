//! The managed upgrade through the real `aikit` binary, real gateway
//! processes and a real detached worker — with a scripted supervisor standing
//! in for launchd/systemd and a scripted `oi` standing in for the managed
//! installer. The platform service managers themselves are rehearsed against a
//! controlled instance (`AIKIT_GATEWAY_SERVICE_INSTANCE`) in
//! `docs/GATEWAY-UPGRADE.md`; what this file proves is the machine around
//! them: that an upgrade ends with a *different process* running the *expected
//! image*, that nothing is replayed, that every failure leaves a named state,
//! and that the receipt is durable.
//!
//! Two builds of the gateway are real: the binary under test, and a copy
//! whose embedded source revision was rewritten in place (and, on macOS,
//! re-signed ad hoc). They differ in bytes, in digest and in what they report
//! as their revision, so "the new build is running" is a fact the running
//! process states, not something the test assumes.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;

fn aikit_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("aikit")
}

/// The source revision this binary was built from. A build with no stamp
/// cannot prove which image is running, so the test refuses to pretend.
fn stamped_revision() -> &'static str {
    match option_env!("AIKIT_BUILD_SOURCE_REVISION") {
        Some(revision) => revision,
        None => panic!(
            "this test needs a stamped build: build inside a git checkout, or set \
             AIKIT_BUILD_SOURCE_REVISION (the managed updater does)"
        ),
    }
}

fn replace_all(haystack: &mut [u8], from: &[u8], to: &[u8]) -> usize {
    assert_eq!(from.len(), to.len(), "an in-place rewrite keeps the length");
    let mut replaced = 0;
    let mut index = 0;
    // `position` on a byte iterator is what keeps a scan of a few hundred MB
    // quick in an unoptimised test binary.
    while let Some(offset) = haystack[index..].iter().position(|byte| *byte == from[0]) {
        index += offset;
        if index + from.len() > haystack.len() {
            break;
        }
        if &haystack[index..index + from.len()] == from {
            haystack[index..index + from.len()].copy_from_slice(to);
            replaced += 1;
            index += from.len();
        } else {
            index += 1;
        }
    }
    replaced
}

/// `aikit`, byte-for-byte, with its embedded revision rewritten: a different
/// image that reports a different build.
fn patched_build(dst: &Path, revision: &str, replacement: &str) {
    let mut bytes = std::fs::read(aikit_bin()).unwrap();
    let full = replace_all(&mut bytes, revision.as_bytes(), replacement.as_bytes());
    assert!(full > 0, "the stamped revision is embedded in the binary");
    let short = |r: &str| r.chars().take(12).collect::<String>();
    replace_all(
        &mut bytes,
        short(revision).as_bytes(),
        short(replacement).as_bytes(),
    );
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::write(dst, &bytes).unwrap();
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A rewritten Mach-O has an invalid signature and is killed on exec:
    // sign it ad hoc, as a locally built binary is.
    if cfg!(target_os = "macos") {
        let signed = Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(dst)
            .output()
            .expect("codesign");
        assert!(
            signed.status.success(),
            "{}",
            String::from_utf8_lossy(&signed.stderr)
        );
    }
}

fn copy_build(dst: &Path) {
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::copy(aikit_bin(), dst).unwrap();
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn write_script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn flip(link: &Path, target: &Path) {
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// The two builds, made once per test binary: a copy of `aikit` and a copy
/// whose embedded revision was rewritten. Each machine hard-links them into its
/// own managed layout, so a test costs no copy of a few hundred MB.
struct Builds {
    a: PathBuf,
    b: PathBuf,
    revision_a: String,
    revision_b: String,
}

fn shared_builds() -> &'static Builds {
    static BUILDS: std::sync::OnceLock<Builds> = std::sync::OnceLock::new();
    BUILDS.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gateway-upgrade-builds");
        let _ = std::fs::remove_dir_all(&dir);
        let revision_a = stamped_revision().to_owned();
        // Same length, different content, still hexadecimal.
        let revision_b: String = revision_a
            .chars()
            .map(|c| if c == 'f' { '0' } else { 'f' })
            .collect();
        assert_ne!(revision_a, revision_b);
        copy_build(&dir.join("a/aikit"));
        patched_build(&dir.join("b/aikit"), &revision_a, &revision_b);
        Builds {
            a: dir.join("a/aikit"),
            b: dir.join("b/aikit"),
            revision_a,
            revision_b,
        }
    })
}

fn link_or_copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).unwrap();
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// One machine: a managed layout (`cur/aikit` is the flip point, as
/// `<data-root>/bin/aikit` is), a tools directory on PATH holding `aikit` and a
/// scripted `oi`, and an AIKit home.
struct Machine {
    dir: TempDir,
    revision_a: String,
    revision_b: String,
}

impl Machine {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let builds = shared_builds();
        let (revision_a, revision_b) = (builds.revision_a.clone(), builds.revision_b.clone());
        let root = dir.path().join("managed");
        link_or_copy(&builds.a, &root.join("bin-a/aikit"));
        link_or_copy(&builds.b, &root.join("bin-b/aikit"));
        write_script(
            &root.join("bin-bad/aikit"),
            "echo 'this build is broken' >&2\nexit 1",
        );
        flip(&root.join("cur/aikit"), &root.join("bin-a/aikit"));
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        flip(&tools.join("aikit"), &root.join("cur/aikit"));
        // The scripted managed installer. `next` names what `--apply` flips
        // `current` to (or FAIL, or FLIP_THEN_FAIL:<path>); `--rollback` flips
        // it back to the build recorded in `previous`.
        write_script(
            &tools.join("oi"),
            &format!(
                r#"ROOT='{root}'
echo "$@" >> "$ROOT/oi.calls"
case "$*" in
  *--rollback*)
    ln -sfn "$(cat "$ROOT/previous")" "$ROOT/cur/aikit"
    exit 0 ;;
  *--apply*)
    next="$(cat "$ROOT/next")"
    case "$next" in
      FAIL) echo "cargo build failed" >&2; exit 1 ;;
      FLIP_THEN_FAIL:*) ln -sfn "${{next#FLIP_THEN_FAIL:}}" "$ROOT/cur/aikit"; echo "a later product failed" >&2; exit 1 ;;
      *) readlink "$ROOT/cur/aikit" > "$ROOT/previous"
         ln -sfn "$next" "$ROOT/cur/aikit"; exit 0 ;;
    esac ;;
esac
exit 0"#,
                root = root.display()
            ),
        );
        std::fs::write(
            root.join("previous"),
            root.join("bin-a/aikit").display().to_string(),
        )
        .unwrap();
        Self {
            dir,
            revision_a,
            revision_b,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("managed")
    }

    /// A service definition and a scripted service manager, so "ask the manager
    /// to start the gateway" has something to ask. The manager is a script on
    /// PATH that records what it was asked and succeeds; the [`Supervisor`]
    /// thread is what actually restarts the process.
    fn with_service_manager(&self) {
        let exe = self.dir.path().join("tools/aikit").display().to_string();
        let (unit, body) = if cfg!(target_os = "macos") {
            (
                self.home()
                    .join("Library/LaunchAgents/ai.aikit.gateway.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
                     <key>Label</key><string>ai.aikit.gateway</string><key>ProgramArguments</key>\
                     <array><string>{exe}</string><string>gateway</string><string>serve</string>\
                     <string>--unix</string></array></dict></plist>\n"
                ),
            )
        } else {
            (
                self.home()
                    .join(".config/systemd/user/aikit-gateway.service"),
                format!("[Service]\nExecStart={exe} gateway serve --unix\n"),
            )
        };
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, body).unwrap();
        for manager in ["launchctl", "systemctl"] {
            write_script(
                &self.dir.path().join("tools").join(manager),
                &format!(
                    "echo \"{manager} $*\" >> '{}/manager.calls'\nexit 0",
                    self.root().display()
                ),
            );
        }
    }

    fn manager_calls(&self) -> String {
        std::fs::read_to_string(self.root().join("manager.calls")).unwrap_or_default()
    }

    fn home(&self) -> PathBuf {
        let home = self.dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    fn set_next(&self, what: &str) {
        std::fs::write(self.root().join("next"), what).unwrap();
    }

    fn set_previous(&self, build: &str) {
        std::fs::write(
            self.root().join("previous"),
            self.root().join(build).join("aikit").display().to_string(),
        )
        .unwrap();
    }

    fn install(&self, build: &str) {
        flip(
            &self.root().join("cur/aikit"),
            &self.root().join(build).join("aikit"),
        );
    }

    fn path(&self) -> String {
        format!(
            "{}:/usr/bin:/bin:/usr/sbin:/sbin",
            self.dir.path().join("tools").display()
        )
    }

    /// `aikit …` as an operator on this machine would run it.
    fn run(&self, args: &[&str]) -> (bool, Value) {
        let mut all = args.to_vec();
        all.push("--json");
        let output = Command::new(self.dir.path().join("tools/aikit"))
            .args(&all)
            .env_clear()
            .env("PATH", self.path())
            .env("HOME", self.home())
            .env("AIKIT_HOME", self.home())
            .env("AIKIT_UPGRADE_WORKER_MODE", "process")
            .current_dir(self.home())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
            panic!(
                "aikit {args:?} must answer a JSON envelope; stdout={stdout} stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), envelope)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (ok, envelope) = self.run(args);
        assert!(ok, "aikit {args:?} failed: {envelope}");
        envelope["data"].clone()
    }

    fn socket(&self) -> PathBuf {
        self.home().join("state/gateway.sock")
    }

    /// One raw command to this machine's gateway over its Unix socket.
    fn raw(&self, command: Value) -> Value {
        let mut stream = UnixStream::connect(self.socket()).expect("the gateway answers");
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let request = json!({"request_id": null, "command": command}).to_string();
        stream.write_all(request.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    fn running(&self) -> Option<Value> {
        let mut stream = UnixStream::connect(self.socket()).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        stream
            .write_all(b"{\"request_id\":null,\"command\":{\"type\":\"protocol\"}}\n")
            .ok()?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).ok()?;
        let envelope: Value = serde_json::from_str(line.trim()).ok()?;
        envelope["response"]["build"]
            .as_object()
            .map(|_| envelope["response"].clone())
    }

    fn wait_running(&self) -> Value {
        // Generous: a debug binary of a few hundred MB starting beside other
        // builds on a loaded machine has taken over a minute to answer.
        let deadline = Instant::now() + Duration::from_secs(240);
        loop {
            if let Some(reading) = self.running() {
                return reading;
            }
            assert!(Instant::now() < deadline, "the gateway never answered");
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Stands in for launchd `KeepAlive` / systemd `Restart=always`: starts the
/// gateway from the managed flip point and starts it again whenever it exits.
struct Supervisor {
    stop: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    exits: Arc<Mutex<Vec<Option<i32>>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Supervisor {
    fn start(machine: &Machine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let starts = Arc::new(AtomicUsize::new(0));
        let exits = Arc::new(Mutex::new(Vec::new()));
        let current = machine.root().join("cur/aikit");
        let home = machine.home();
        let path = machine.path();
        let (s, n, e) = (Arc::clone(&stop), Arc::clone(&starts), Arc::clone(&exits));
        let handle = thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                let spawned = Command::new(&current)
                    .args(["gateway", "serve", "--unix"])
                    .env_clear()
                    .env("PATH", &path)
                    .env("HOME", &home)
                    .env("AIKIT_HOME", &home)
                    .env("AIKIT_GATEWAY_REF", "agency-gateway/upgrade-test")
                    .env("AIKIT_WORKCELL_REF", "workcell:upgrade-test")
                    // What a service definition declares: a supervisor stands
                    // behind this process and will start the next one.
                    .env("AIKIT_GATEWAY_LIFECYCLE", "supervised-launchd")
                    .current_dir(&home)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                let mut child: Child = match spawned {
                    Ok(child) => child,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                };
                n.fetch_add(1, Ordering::SeqCst);
                let status = loop {
                    if let Ok(Some(status)) = child.try_wait() {
                        break Some(status.code());
                    }
                    if s.load(Ordering::SeqCst) {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    thread::sleep(Duration::from_millis(50));
                };
                if let Some(code) = status {
                    e.lock().unwrap().push(code);
                }
                // launchd/systemd throttle a process that exits at once.
                thread::sleep(Duration::from_millis(400));
            }
        });
        Self {
            stop,
            starts,
            exits,
            handle: Some(handle),
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn communique_draft(reference: &str) -> Value {
    json!({
        "communique_ref": format!("aikit:communique:{reference}"),
        "attribution": "unknown",
        "attribution_basis": "test",
        "to_position_ref": "central:position:control:root:keeper",
        "body": "sent before the upgrade",
        "sent_at_unix_ms": 1,
        "state": "held",
        "state_basis": "test",
    })
}

fn outcome(data: &Value) -> &str {
    data["outcome"]["status"].as_str().unwrap_or("?")
}

#[test]
fn an_upgrade_replaces_the_running_process_with_the_installed_build_and_proves_it_without_losing_a_communique(
) {
    let machine = Machine::new();
    let supervisor = Supervisor::start(&machine);
    let before = machine.wait_running();
    assert_eq!(before["build"]["revision"], machine.revision_a);
    assert_eq!(before["build"]["lifecycle"], "supervised-launchd");
    let old_pid = before["build"]["pid"].as_u64().unwrap();

    // Journal state that must survive the restart.
    let sent = machine
        .raw(json!({"type": "send-communique", "draft": communique_draft("before-upgrade")}));
    assert_eq!(sent["ok"], true, "{sent}");

    // `oi update` flips the managed symlink. The process keeps running its old image.
    machine.install("bin-b");
    assert_eq!(
        machine.running().unwrap()["build"]["revision"],
        machine.revision_a,
        "an installed build is not a running build"
    );

    // The doctor says so, and names the fix.
    let doctor = machine.ok(&["gateway", "doctor"]);
    let stale = doctor["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "gateway.stale")
        .unwrap_or_else(|| panic!("the doctor must find the stale resident: {doctor}"));
    assert!(stale["remedy"].as_str().unwrap().contains("upgrade apply"));
    assert_eq!(doctor["verdict"], "fail");

    // The plan reads the same two facts.
    let plan = machine.ok(&["gateway", "upgrade", "plan"]);
    assert_eq!(plan["stale"], true);
    assert_eq!(plan["action"], "restart");
    assert_eq!(plan["running"]["identity"]["revision"], machine.revision_a);
    assert_eq!(
        plan["installed"]["revision"]
            .as_str()
            .unwrap()
            .chars()
            .take(12)
            .collect::<String>(),
        machine.revision_b.chars().take(12).collect::<String>()
    );

    // Apply: a detached worker drains, the supervisor restarts, the worker verifies.
    let applied = machine.ok(&[
        "gateway",
        "upgrade",
        "apply",
        "--wait",
        "--verify-timeout-secs",
        "60",
    ]);
    assert_eq!(outcome(&applied), "completed", "{applied}");
    let after = &applied["outcome"]["running"];
    assert_ne!(
        after["pid"].as_u64().unwrap(),
        old_pid,
        "a different process"
    );
    assert_eq!(
        after["identity"]["revision"], machine.revision_b,
        "the process now running states the new build"
    );
    let now = machine.wait_running();
    assert_eq!(now["build"]["revision"], machine.revision_b);
    assert_eq!(
        now["build"]["pid"].as_u64().unwrap(),
        after["pid"].as_u64().unwrap()
    );

    // The old process was drained, not killed: it exited cleanly, once, and
    // the supervisor started exactly one successor.
    assert_eq!(supervisor.starts.load(Ordering::SeqCst), 2);
    assert_eq!(supervisor.exits.lock().unwrap().as_slice(), &[Some(0)]);

    // The journal survived: the Communique is there, and nothing was replayed.
    let read = machine.raw(
        json!({"type": "read-communique", "communique_ref": "aikit:communique:before-upgrade"}),
    );
    assert_eq!(read["ok"], true, "{read}");
    assert_eq!(
        read["response"]["communique"]["body"],
        "sent before the upgrade"
    );

    // The receipt is durable and says what was kept.
    let status = machine.ok(&["gateway", "upgrade", "status"]);
    assert_eq!(status["phase"], "completed");
    let receipt = std::fs::read_to_string(status["receipt"].as_str().unwrap()).unwrap();
    assert!(receipt.contains("completed"));
    assert!(receipt.contains("nothing was replayed"), "{receipt}");

    // The doctor agrees, and a second apply changes nothing. A build with
    // local edits (this one) is told from the installed file by its digest, which
    // the process reads in the background after it starts: until then the
    // doctor says so instead of guessing either way.
    let deadline = Instant::now() + Duration::from_secs(120);
    let doctor = loop {
        let doctor = machine.ok(&["gateway", "doctor"]);
        let ids: Vec<String> = doctor["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap_or("?").to_owned())
            .collect();
        assert!(
            !ids.contains(&"gateway.stale".to_owned()),
            "the new build is not stale: {doctor}"
        );
        if ids.contains(&"gateway.current".to_owned()) {
            break doctor;
        }
        assert!(
            ids.contains(&"gateway.identity_pending".to_owned()),
            "neither current nor pending: {doctor}"
        );
        assert!(
            Instant::now() < deadline,
            "the digest never arrived: {doctor}"
        );
        thread::sleep(Duration::from_secs(1));
    };
    assert!(doctor["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == "gateway.current"));
    let again = machine.ok(&["gateway", "upgrade", "apply", "--wait"]);
    assert_eq!(outcome(&again), "no-change");
    assert_eq!(
        supervisor.starts.load(Ordering::SeqCst),
        2,
        "a current gateway is not restarted"
    );
}

#[test]
fn an_install_that_fails_leaves_the_running_gateway_and_the_installed_build_untouched() {
    let machine = Machine::new();
    let _supervisor = Supervisor::start(&machine);
    let before = machine.wait_running();
    machine.set_next("FAIL");
    let applied = machine.ok(&["gateway", "upgrade", "apply", "--install", "--wait"]);
    assert_eq!(outcome(&applied), "failed-before-change", "{applied}");
    assert!(applied["outcome"]["summary"]
        .as_str()
        .unwrap()
        .contains("unchanged"));
    let still = machine.wait_running();
    assert_eq!(
        still["build"]["pid"], before["build"]["pid"],
        "the gateway was never touched"
    );
    assert_eq!(
        std::fs::read_link(machine.root().join("cur/aikit")).unwrap(),
        machine.root().join("bin-a/aikit")
    );
}

#[test]
fn an_installer_that_flips_the_build_and_then_fails_is_rolled_back_and_not_reported_unchanged() {
    let machine = Machine::new();
    let _supervisor = Supervisor::start(&machine);
    let before = machine.wait_running();
    machine.set_previous("bin-a");
    machine.set_next(&format!(
        "FLIP_THEN_FAIL:{}",
        machine.root().join("bin-b/aikit").display()
    ));
    let applied = machine.ok(&["gateway", "upgrade", "apply", "--install", "--wait"]);
    assert_eq!(outcome(&applied), "rolled-back", "{applied}");
    assert_eq!(
        std::fs::read_link(machine.root().join("cur/aikit")).unwrap(),
        machine.root().join("bin-a/aikit"),
        "the previous build is installed again"
    );
    let still = machine.wait_running();
    assert_eq!(
        still["build"]["pid"], before["build"]["pid"],
        "the running gateway was never touched"
    );
    let calls = std::fs::read_to_string(machine.root().join("oi.calls")).unwrap();
    assert!(calls.contains("--rollback"), "{calls}");
}

#[test]
fn a_new_build_that_does_not_come_up_is_rolled_back_and_the_old_build_is_verified_running() {
    let machine = Machine::new();
    let supervisor = Supervisor::start(&machine);
    machine.with_service_manager();
    let before = machine.wait_running();
    machine.set_previous("bin-a");
    // The installer "succeeds" and leaves a build that exits at once. The
    // verify bound is the wait for the *restored* build to answer too, and a
    // debug binary of a few hundred MB on a loaded machine is slow to start.
    machine.set_next(&machine.root().join("bin-bad/aikit").display().to_string());
    let applied = machine.ok(&[
        "gateway",
        "upgrade",
        "apply",
        "--install",
        "--wait",
        "--exit-wait-secs",
        "5",
        "--verify-timeout-secs",
        "45",
    ]);
    assert_eq!(outcome(&applied), "rolled-back", "{applied}");
    assert!(
        machine.manager_calls().contains("kickstart") || machine.manager_calls().contains("start"),
        "the service manager was asked to start the gateway: {}",
        machine.manager_calls()
    );
    let now = machine.wait_running();
    assert_eq!(
        now["build"]["revision"], machine.revision_a,
        "the previous build runs again"
    );
    assert_ne!(
        now["build"]["pid"], before["build"]["pid"],
        "a new process, on the old build"
    );
    // The supervisor saw the drained exit, the broken builds, and the restore.
    assert!(supervisor.starts.load(Ordering::SeqCst) >= 3);
    let receipt = std::fs::read_to_string(
        machine.ok(&["gateway", "upgrade", "status"])["receipt"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(receipt.contains("rolled-back"), "{receipt}");
}

#[test]
fn a_foreground_gateway_is_installed_for_but_never_stopped() {
    let machine = Machine::new();
    // Run the gateway directly: nothing supervises it.
    let mut child = Command::new(machine.root().join("cur/aikit"))
        .args(["gateway", "serve", "--unix"])
        .env_clear()
        .env("PATH", machine.path())
        .env("HOME", machine.home())
        .env("AIKIT_HOME", machine.home())
        .current_dir(machine.home())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let before = machine.wait_running();
    assert_eq!(before["build"]["lifecycle"], "foreground");
    machine.install("bin-b");
    let applied = machine.ok(&["gateway", "upgrade", "apply", "--wait"]);
    assert_eq!(outcome(&applied), "needs-operator", "{applied}");
    assert!(applied["outcome"]["summary"]
        .as_str()
        .unwrap()
        .contains("nothing would start the new build"));
    assert!(
        child.try_wait().unwrap().is_none(),
        "a foreground gateway was left running"
    );
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn a_stop_signal_drains_and_exits_cleanly_instead_of_killing_the_gateway_mid_turn() {
    let machine = Machine::new();
    let child = Command::new(machine.root().join("cur/aikit"))
        .args(["gateway", "serve", "--unix"])
        .env_clear()
        .env("PATH", machine.path())
        .env("HOME", machine.home())
        .env("AIKIT_HOME", machine.home())
        .current_dir(machine.home())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    machine.wait_running();
    let sent =
        machine.raw(json!({"type": "send-communique", "draft": communique_draft("before-stop")}));
    assert_eq!(sent["ok"], true);
    // What `launchctl bootout` / `systemctl stop` do.
    assert!(Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .unwrap()
        .success());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "a stop is a clean exit: {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("gateway drained for stop"), "{stderr}");
    // The state it left is complete: a fresh gateway restores the journal.
    let state = std::fs::read_to_string(machine.home().join("state/gateway.json")).unwrap();
    assert!(state.contains("before-stop"));
}
