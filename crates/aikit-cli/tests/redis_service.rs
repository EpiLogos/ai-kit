//! The native Redis NOW lifecycle, end to end through the real `aikit` binary,
//! with a real `redis-server` and no Workcell or Factory anywhere.
//!
//! Tripwire: `workcell`, `workcell-write-boundary` and `factory` are on PATH and
//! every one records the call and fails; the marker file must stay empty. The
//! Redis here is a real `redis-server` (this machine's, ≥ 8.10); the test says
//! so and skips only if none is installed, so a green run is never a fake.
#![cfg(unix)]

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn alive(pid: u64) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .map(|o| {
            o.status.success()
                && !String::from_utf8_lossy(&o.stdout)
                    .trim_start()
                    .starts_with('Z')
        })
        .unwrap_or(false)
}

fn real_redis_server() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("redis-server"))
        .find(|p| p.is_file())
        .map(|p| fs::canonicalize(p).unwrap())
}

/// One RESP request over a fresh connection, returning the raw reply text.
fn resp(port: u16, parts: &[&str]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut request = format!("*{}\r\n", parts.len());
    for part in parts {
        request.push_str(&format!("${}\r\n{}\r\n", part.len(), part));
    }
    stream.write_all(request.as_bytes()).unwrap();
    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).unwrap();
    String::from_utf8_lossy(&buffer[..n]).into_owned()
}

struct Scene {
    root: TempDir,
    path: String,
    mark: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let tripwire = root.path().join("tripwire");
        let mark = root.path().join("TRIPWIRE-HIT");
        for name in ["workcell", "workcell-write-boundary", "factory"] {
            executable(
                &tripwire.join(name),
                &format!(
                    "#!/bin/sh\necho \"{name} $*\" >> \"{}\"\nexit 99\n",
                    mark.display()
                ),
            );
        }
        // Deliberately no directory holding redis-server on PATH: the binary is
        // always named with --redis-server, so discovery cannot be what works.
        let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", tripwire.display());
        Self { root, path, mark }
    }

    fn run(&self, args: &[&str]) -> (bool, Value) {
        let output = Command::new(cargo_bin("aikit"))
            .args(["--json", "now-context", "service"])
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.root.path().join("home"))
            .env("AIKIT_HOME", self.root.path().join("home/.aikit"))
            .env("WORKCELL_CONTROL_TOKEN", "must-not-leak")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "no JSON envelope ({e}); stdout={stdout:?} stderr={:?}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), envelope)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (success, envelope) = self.run(args);
        assert!(success, "{args:?} failed: {envelope}");
        envelope["data"].clone()
    }

    fn hits(&self) -> String {
        fs::read_to_string(&self.mark).unwrap_or_default()
    }
}

struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for dir in &self.0 {
            if let Ok(state) = fs::read_to_string(dir.join("redis-service.json")) {
                if let Ok(state) = serde_json::from_str::<Value>(&state) {
                    if let Some(pid) = state["process"]["pid"].as_u64() {
                        let _ = Command::new("kill").arg(pid.to_string()).status();
                    }
                }
            }
        }
    }
}

#[test]
fn the_redis_lifecycle_runs_without_workcell_keeps_its_data_and_never_adopts_a_foreign_redis() {
    let Some(real) = real_redis_server() else {
        eprintln!("skipping: no redis-server installed on PATH");
        return;
    };
    let scene = Scene::new();
    let service = scene.root.path().join("redis-service");
    let s = service.display().to_string();
    let _cleanup = Cleanup(vec![service.clone()]);
    let port = free_port();
    let port_s = port.to_string();
    let real_s = real.display().to_string();
    let call = |verb: &str, rest: &[&str]| -> Value {
        let mut argv = vec![verb, "--service-dir", s.as_str()];
        argv.extend_from_slice(rest);
        scene.ok(&argv)
    };

    assert_eq!(call("status", &[])["state"], "not-provisioned");

    // provision: the reference profile, generated; executable named and hashed.
    let provisioned = call(
        "provision",
        &[
            "--port",
            &port_s,
            "--redis-server",
            &real_s,
            "--maxmemory-mb",
            "64",
        ],
    );
    assert_eq!(provisioned["owner"], "aikit");
    assert_eq!(provisioned["profile"]["maxmemory_policy"], "noeviction");
    let version = provisioned["executable"]["version"].as_str().unwrap();
    assert!(
        version.starts_with("8.") || version.starts_with("9."),
        "{version}"
    );
    let conf = fs::read_to_string(service.join("redis.conf")).unwrap();
    assert!(conf.contains("appendonly yes") && conf.contains("bind 127.0.0.1"));
    let election: Value =
        serde_json::from_slice(&fs::read(service.join("redis-now.json")).unwrap()).unwrap();
    assert_eq!(election["schema"], "aikit.redis-now-config/v1");
    assert_eq!(election["address"], format!("127.0.0.1:{port}"));

    // start: ready means PING answers AND the live profile conforms.
    let started = call("start", &[]);
    assert_eq!(started["outcome"], "started");
    assert_eq!(started["health"]["profile_conforms"], true, "{started}");
    assert_eq!(started["health"]["profile"]["aof_enabled"], true);
    assert_eq!(
        started["health"]["profile"]["maxmemory"],
        64u64 * 1024 * 1024
    );
    assert_eq!(
        started["health"]["profile"]["maxmemory_policy"],
        "noeviction"
    );
    let pid = started["pid"].as_u64().unwrap();
    assert!(alive(pid), "the service outlives the start command");

    // The election it wrote is consumable by the ordinary `now-context status`.
    let status = Command::new(cargo_bin("aikit"))
        .args(["--json", "now-context", "status", "--config-file"])
        .arg(service.join("redis-now.json"))
        .env_clear()
        .env("PATH", &scene.path)
        .env("HOME", scene.root.path())
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["data"]["available"], true, "{status}");

    // adoption of our own process, not a second one.
    let again = call("start", &[]);
    assert_eq!(again["outcome"], "already-running");
    assert_eq!(again["pid"].as_u64().unwrap(), pid);

    // data written now must survive restart (AOF) and upgrade.
    assert!(resp(port, &["SET", "aikit-proof", "kept"]).starts_with("+OK"));
    let restarted = call("restart", &[]);
    assert_eq!(restarted["stop"]["outcome"], "stopped");
    let new_pid = restarted["start"]["pid"].as_u64().unwrap();
    assert_ne!(new_pid, pid);
    assert!(!alive(pid) && alive(new_pid));
    assert!(
        resp(port, &["GET", "aikit-proof"]).contains("kept"),
        "restart lost data"
    );

    // upgrade to another (shimmed) executable over the same data directory.
    let next = scene.root.path().join("bin/redis-next");
    executable(&next, &format!("#!/bin/sh\nexec \"{real_s}\" \"$@\"\n"));
    let next_s = next.display().to_string();
    let upgraded = call("upgrade", &["--redis-server", &next_s]);
    assert_eq!(upgraded["outcome"], "upgraded");
    assert_eq!(upgraded["to"]["path"], next_s);
    let upgraded_pid = upgraded["start"]["pid"].as_u64().unwrap();
    assert!(alive(upgraded_pid) && !alive(new_pid));
    assert!(
        resp(port, &["GET", "aikit-proof"]).contains("kept"),
        "upgrade lost data"
    );

    // an executable of an older series is refused before the service is touched.
    let old = scene.root.path().join("bin/redis-old");
    executable(
        &old,
        "#!/bin/sh\n[ \"$1\" = --version ] && echo 'Redis server v=7.4.1 sha=0 malloc=libc'\nexit 0\n",
    );
    let (ok, refused) = scene.run(&[
        "upgrade",
        "--service-dir",
        &s,
        "--redis-server",
        old.to_str().unwrap(),
    ]);
    assert!(!ok);
    assert_eq!(
        refused["error"]["code"],
        "redis_service.version_unsupported"
    );
    assert!(
        alive(upgraded_pid),
        "a refused upgrade leaves the service running"
    );

    // an executable that reports a good version but cannot serve: rolled back.
    let bad = scene.root.path().join("bin/redis-bad");
    executable(
        &bad,
        &format!(
            "#!/bin/sh\n[ \"$1\" = --version ] && exec \"{real_s}\" --version\necho 'induced serve failure' >&2\nexit 1\n"
        ),
    );
    let (ok, failed) = scene.run(&[
        "upgrade",
        "--service-dir",
        &s,
        "--redis-server",
        bad.to_str().unwrap(),
    ]);
    assert!(!ok);
    let message = failed["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("rolled back"), "{failed}");
    let after = call("status", &[]);
    assert_eq!(after["state"], "running", "service restored: {after}");
    assert_eq!(
        after["executable"]["path"], next_s,
        "previous executable restored"
    );
    assert!(
        resp(port, &["GET", "aikit-proof"]).contains("kept"),
        "rollback lost data"
    );

    // stop keeps the data; stopping again is idempotent; the port goes quiet.
    let stopped = call("stop", &[]);
    assert_eq!(stopped["outcome"], "stopped");
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    assert_eq!(call("stop", &[])["outcome"], "not-running");
    assert!(service.join("data").exists());

    // a Redis this service did not start is reported foreign and left alone.
    let foreign_port = free_port();
    let foreign_dir = scene.root.path().join("foreign");
    fs::create_dir_all(&foreign_dir).unwrap();
    let mut foreign = Command::new(&real)
        .args(["--port", &foreign_port.to_string(), "--bind", "127.0.0.1"])
        .args(["--save", "", "--appendonly", "no", "--dir"])
        .arg(&foreign_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && TcpStream::connect(("127.0.0.1", foreign_port)).is_err() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let b = scene.root.path().join("redis-b").display().to_string();
    scene.ok(&[
        "provision",
        "--service-dir",
        &b,
        "--port",
        &foreign_port.to_string(),
        "--redis-server",
        &real_s,
    ]);
    let (ok, occupied) = scene.run(&["start", "--service-dir", &b]);
    assert!(!ok);
    assert_eq!(
        occupied["error"]["code"], "redis_service.port_occupied",
        "{occupied}"
    );
    assert_eq!(
        scene.ok(&["status", "--service-dir", &b])["state"],
        "foreign-listener"
    );
    assert_eq!(
        scene.ok(&["stop", "--service-dir", &b])["outcome"],
        "not-running"
    );
    assert!(
        foreign.try_wait().unwrap().is_none(),
        "the foreign Redis was left running"
    );
    let _ = foreign.kill();
    let _ = foreign.wait();

    assert_eq!(scene.hits(), "", "an excluded owner was reached");
}

#[test]
fn a_missing_redis_server_is_a_named_prerequisite_not_a_workcell_search() {
    let scene = Scene::new();
    let service = scene.root.path().join("svc").display().to_string();
    let (ok, failed) = scene.run(&["provision", "--service-dir", &service]);
    assert!(!ok);
    assert_eq!(failed["error"]["code"], "redis_service.executable_missing");
    assert_eq!(scene.hits(), "");
}
