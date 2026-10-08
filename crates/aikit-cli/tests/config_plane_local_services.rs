//! The decision-provider election and the Redis NOW election through the
//! configuration plane (`aikit config-contribution`, `config validate|plan|
//! apply|reset`, the `system` disclosure), and the commands that default to them.
//!
//! Real: the `aikit` binary, the contribution document, owner-native
//! validation of the elected documents, the receipt store, the disclosure
//! with a live read-only probe, and `aikit decide status` / `aikit now-context
//! status` resolving the election. The decision endpoint is `fake_kev.py` (a
//! protocol-compatible loopback process, not Kev's weights); Redis is a real
//! `redis-server` when one is installed.
#![cfg(unix)]

use assert_cmd::cargo::cargo_bin;
use serde_json::{json, Value};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

/// A port that was free a moment ago, drawn from a pid- and time-seeded range instead of the OS's ephemeral allocator. Binding :0 and
/// dropping hands the SAME port to the next bind(0) of a concurrently running test binary (the allocator is sequential on macOS), which
/// raced two service tests on a CI runner ("Address already in use"). A random pick over 25,000 ports makes that collision negligible.
fn free_port() -> u16 {
    let mut seed = u64::from(std::process::id())
        ^ std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
    loop {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let port = 20_000 + ((seed >> 33) % 20_000) as u16;
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

fn wait_for(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && TcpStream::connect(("127.0.0.1", port)).is_err() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct Children(Vec<Child>);
impl Drop for Children {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct World {
    home: TempDir,
    project: TempDir,
}

impl World {
    fn new() -> Self {
        let world = Self {
            home: TempDir::new().unwrap(),
            project: TempDir::new().unwrap(),
        };
        fs::create_dir_all(world.project.path().join(".aikit")).unwrap();
        fs::write(
            world.project.path().join(".aikit/profile.toml"),
            "schema = 1\n",
        )
        .unwrap();
        world
    }
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(cargo_bin("aikit"))
            .args(args)
            .env("AIKIT_HOME", self.home.path())
            .env("HOME", self.home.path())
            .current_dir(self.project.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("not JSON ({e}): {stdout:?}"));
        (output.status.code().unwrap_or(-1), value)
    }
    /// plan then apply one election value, returning the receipt.
    fn elect(&self, setting: &str, path: &str, changeset: &str) -> Value {
        let value = serde_json::to_string(path).unwrap();
        let (code, plan) = self.run(&[
            "config",
            "plan",
            "--json",
            "--setting",
            setting,
            "--scope",
            "machine",
            "--value",
            &value,
        ]);
        assert_eq!(code, 0, "{plan}");
        let plan_path = self.project.path().join(format!("{changeset}.json"));
        fs::write(&plan_path, serde_json::to_string(&plan).unwrap()).unwrap();
        let (code, receipt) = self.run(&[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            changeset,
        ]);
        assert_eq!(code, 0, "{receipt}");
        receipt
    }
    fn disclosed(&self, key: &str) -> Value {
        let (_, system) = self.run(&["system", "--json"]);
        system["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "local-services")
            .expect("local-services section is disclosed")["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap()
            .clone()
    }
}

fn decision_document(dir: &Path, port: u16, timeout_ms: u64) -> String {
    let path = dir.join("decision-provider.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schema": "aikit.decision-provider/v1", "mode": "endpoint",
            "address": format!("127.0.0.1:{port}"),
            "limits": {"timeout_ms": timeout_ms, "max_attempts": 1,
                       "max_input_tokens_per_attempt": 16384,
                       "max_output_tokens_per_attempt": 4000, "model": "kev-latest"}
        }))
        .unwrap(),
    )
    .unwrap();
    path.display().to_string()
}

#[test]
fn the_decision_election_is_contributed_validated_applied_read_back_and_used_by_default() {
    let world = World::new();
    let setting = "ai-kit:local-services:decision.provider";
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    // Contributed, writable, a `path` of the owner-native document type.
    let (_, contribution) = world.run(&["config-contribution", "--json"]);
    let spec = contribution["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["settings"].as_array().unwrap().iter().cloned())
        .find(|s| s["setting_ref"] == setting)
        .expect("the decision election is contributed");
    assert_eq!(spec["writable"], true);
    assert_eq!(spec["value_schema"]["type"], "path");
    assert_eq!(spec["value_schema"]["format"], "aikit.decision-provider/v1");
    assert_eq!(spec["effect"]["kind"], "value-change");
    assert_eq!(
        spec["allowed_scopes"],
        json!([{"scope_kind": "machine", "scope_ref": null}])
    );

    // Before any election a command with no file says so and names the setting.
    let (code, none) = world.run(&["--json", "decide", "status"]);
    assert_ne!(code, 0);
    assert_eq!(none["error"]["code"], "decision.no_election");
    assert!(none["error"]["message"].as_str().unwrap().contains(setting));

    // Owner-native validation refuses what is not an election document.
    for (bad, why) in [
        (json!("relative.json"), "absolute"),
        (json!("/nonexistent/provider.json"), "cannot read"),
        (json!(5), "absolute path"),
    ] {
        let (code, verdict) = world.run(&[
            "config",
            "validate",
            "--json",
            "--setting",
            setting,
            "--scope",
            "machine",
            "--value",
            &bad.to_string(),
        ]);
        assert_eq!(code, 0);
        assert_eq!(verdict["valid"], false, "{bad}");
        assert_eq!(verdict["violations"][0]["code"], "invalid_election");
        assert!(
            verdict["violations"][0]["message"]
                .as_str()
                .unwrap()
                .contains(why),
            "{verdict}"
        );
    }

    // A real endpoint (protocol-compatible stand-in) and a document pointing at it.
    let port = free_port();
    let scratch = TempDir::new().unwrap();
    let kev_cwd = scratch.path().join("kev-run/kev");
    fs::create_dir_all(&kev_cwd).unwrap();
    let _children = Children(vec![Command::new("python3")
        .arg(fixtures.join("fake_kev.py"))
        .args([
            "-m",
            "kev.serve",
            "--run",
            "jaredpalmer/kev-0.8b",
            "--host",
            "127.0.0.1",
            "--port",
        ])
        .arg(port.to_string())
        .current_dir(&kev_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()]);
    wait_for(port);
    let document = decision_document(scratch.path(), port, 20_000);

    let receipt = world.elect(setting, &document, "cs-decision-1");
    assert_eq!(receipt["outcome"], "applied");

    // Disclosure: declared (reference + digest), effective (document current),
    // active (the live endpoint answering its model card, read-only).
    let reading = world.disclosed("decision.provider");
    assert_eq!(
        reading["axes"]["declared"]["value"]["provider_file"],
        document
    );
    assert_eq!(reading["axes"]["effective"]["value"]["document"], "current");
    assert_eq!(
        reading["axes"]["effective"]["value"]["reading"]["mode"],
        "endpoint"
    );
    assert_eq!(reading["axes"]["active"]["value"]["reachable"], true);
    assert_eq!(
        reading["axes"]["active"]["value"]["models"],
        json!(["kev-latest"])
    );

    // A command with no file now uses the election.
    let (code, status) = world.run(&["--json", "decide", "status", "--probe"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["data"]["mode"], "endpoint");
    assert_eq!(status["data"]["install"]["state"], "loaded");
    assert_eq!(status["data"]["diagnostic"]["outcome"], "completed");

    // The elected document changes (as after a service upgrade): it is no longer
    // the elected one, and every surface says so.
    decision_document(scratch.path(), port, 21_000);
    let (code, drifted) = world.run(&["--json", "decide", "status"]);
    assert_ne!(code, 0);
    assert_eq!(drifted["error"]["code"], "config.election_drifted");
    assert_eq!(
        world.disclosed("decision.provider")["axes"]["effective"]["value"]["document"],
        "changed-since-elected"
    );
    // An explicit file still works and never consults the election.
    let (code, explicit) = world.run(&["--json", "decide", "status", "--provider-file", &document]);
    assert_eq!(code, 0, "{explicit}");

    // Re-applying (a new plan over the new digest) re-elects it.
    assert_eq!(
        world.elect(setting, &document, "cs-decision-2")["outcome"],
        "applied"
    );
    assert_eq!(world.run(&["--json", "decide", "status"]).0, 0);

    // Reset removes the election and nothing else: the document stays.
    let (code, reset) = world.run(&[
        "config",
        "reset",
        "--json",
        "--setting",
        setting,
        "--scope",
        "machine",
    ]);
    assert_eq!(code, 0, "{reset}");
    assert!(Path::new(&document).exists());
    assert_eq!(
        world.disclosed("decision.provider")["axes"]["declared"]["value"],
        Value::Null
    );
    assert_eq!(
        world.run(&["--json", "decide", "status"]).1["error"]["code"],
        "decision.no_election"
    );
}

fn real_redis_server() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("redis-server"))
        .find(|p| p.is_file())
}

#[test]
fn the_redis_election_round_trips_and_the_unreachable_service_is_reported_not_started() {
    let world = World::new();
    let setting = "ai-kit:local-services:now.redis";
    let scratch = TempDir::new().unwrap();
    let port = free_port();
    let document = scratch.path().join("redis-now.json");
    fs::write(
        &document,
        serde_json::to_vec_pretty(&json!({
            "schema": "aikit.redis-now-config/v1", "address": format!("127.0.0.1:{port}"),
            "database": 0, "key_prefix": "aikit-now", "username": null, "credential_ref": null,
            "allow_remote": false, "connect_timeout_ms": 300, "io_timeout_ms": 1000,
            "prepared_ttl_seconds": 21600, "coordination_retention_seconds": 1209600
        }))
        .unwrap(),
    )
    .unwrap();
    let document = document.display().to_string();

    assert_eq!(
        world.run(&["--json", "now-context", "status"]).1["error"]["code"],
        "now_context.no_election"
    );
    let (_, wrong) = world.run(&[
        "config",
        "validate",
        "--json",
        "--setting",
        setting,
        "--scope",
        "machine",
        "--value",
        &json!(scratch.path().join("absent.json").display().to_string()).to_string(),
    ]);
    assert_eq!(wrong["valid"], false);

    assert_eq!(
        world.elect(setting, &document, "cs-redis-1")["outcome"],
        "applied"
    );
    let reading = world.disclosed("now.redis");
    assert_eq!(
        reading["axes"]["declared"]["value"]["provider_file"],
        document
    );
    // Nothing listens: the disclosure says unreachable, and does not start Redis.
    assert_eq!(reading["axes"]["active"]["value"]["reachable"], false);
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "electing must not start a service"
    );
    let (code, down) = world.run(&["--json", "now-context", "status"]);
    assert_ne!(code, 0);
    assert!(
        down["error"]["code"]
            .as_str()
            .unwrap()
            .starts_with("now_context."),
        "{down}"
    );

    // With a real Redis behind the election, the default now answers.
    if let Some(redis) = real_redis_server() {
        let dir = scratch.path().join("data");
        fs::create_dir_all(&dir).unwrap();
        let _children = Children(vec![Command::new(redis)
            .args([
                "--port",
                &port.to_string(),
                "--bind",
                "127.0.0.1",
                "--save",
                "",
                "--appendonly",
                "no",
                "--dir",
            ])
            .arg(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()]);
        wait_for(port);
        let (code, up) = world.run(&["--json", "now-context", "status"]);
        assert_eq!(code, 0, "{up}");
        assert_eq!(up["data"]["available"], true);
        let reading = world.disclosed("now.redis");
        assert_eq!(reading["axes"]["active"]["value"]["reachable"], true);
    }

    let (code, _) = world.run(&[
        "config",
        "reset",
        "--json",
        "--setting",
        setting,
        "--scope",
        "machine",
    ]);
    assert_eq!(code, 0);
    assert_eq!(
        world.run(&["--json", "now-context", "status"]).1["error"]["code"],
        "now_context.no_election"
    );
}
