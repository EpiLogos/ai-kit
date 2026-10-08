//! Prepared NOW context selected by the elected local decision provider, with
//! AIKit's own Redis, and no Workcell or Factory anywhere.
//!
//! Real: the `aikit` binary, `aikit now-context service` (a real
//! `redis-server`), the provider-selection path (`selection.mode = provider`)
//! over the SystemOne wire to a loopback endpoint, Redis publication and the
//! readback. Stood in: the endpoint is `fake_kev.py` (a protocol-compatible
//! loopback process, not Kev's weights) and `ctrl` is a script replaying three
//! recorded Central answers (`central.now.read`, `central.file-map.inspect`,
//! `central.file-map.resolve`).
//!
//! What it pins: the prepared view records the decision invocation and the
//! decision provider identity behind its selection. Before this the provider
//! path left `jev_invocation_ref` empty, so a delivery receipt could not say
//! which determination its context was selected by.
#![cfg(unix)]

use assert_cmd::cargo::cargo_bin;
use serde_json::{json, Value};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
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

fn real_redis_server() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("redis-server"))
        .find(|p| p.is_file())
        .map(|p| fs::canonicalize(p).unwrap())
}

struct Guard {
    kev: Option<Child>,
    redis_dir: Option<PathBuf>,
    path: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(kev) = &mut self.kev {
            let _ = kev.kill();
            let _ = kev.wait();
        }
        if let Some(dir) = &self.redis_dir {
            let _ = Command::new(cargo_bin("aikit"))
                .args(["--json", "now-context", "service", "stop", "--service-dir"])
                .arg(dir)
                .env_clear()
                .env("PATH", &self.path)
                .env("HOME", dir)
                .output();
        }
    }
}

fn aikit(path: &str, home: &Path, args: &[&str]) -> (bool, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(["--json"])
        .args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("AIKIT_HOME", home.join(".aikit"))
        .env("WORKCELL_CONTROL_TOKEN", "must-not-leak")
        .current_dir(home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!(
            "no envelope ({e}); {text:?} {:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

#[test]
fn provider_selection_records_the_decision_invocation_and_identity_in_the_prepared_view() {
    let Some(real_redis) = real_redis_server() else {
        eprintln!("skipping: no redis-server installed on PATH");
        return;
    };
    let root = TempDir::new().unwrap();
    let base = root.path();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mark = base.join("TRIPWIRE-HIT");
    for name in ["workcell", "workcell-write-boundary", "factory"] {
        executable(
            &base.join("tripwire").join(name),
            &format!(
                "#!/bin/sh\necho \"{name} $*\" >> \"{}\"\nexit 99\n",
                mark.display()
            ),
        );
    }
    executable(
        &base.join("tripwire/ctrl"),
        &format!(
            "#!/bin/sh\ncase \"$*\" in\n *central.now.read*) cat \"{}\";;\n *central.file-map.inspect*) cat \"{}\";;\n *central.file-map.resolve*) cat \"{}\";;\n *) echo \"unexpected ctrl call: $*\" >&2; exit 2;;\nesac\n",
            fixtures.join("central_now_read.json").display(),
            fixtures.join("central_file_map_inspect.json").display(),
            fixtures.join("central_file_map_resolve.json").display()
        ),
    );
    let path = format!(
        "{}:/usr/bin:/bin:/usr/sbin:/sbin",
        base.join("tripwire").display()
    );
    let home = base.join("home");
    fs::create_dir_all(&home).unwrap();
    let mut guard = Guard {
        kev: None,
        redis_dir: None,
        path: path.clone(),
    };

    // AIKit's own Redis, from the native lifecycle.
    let redis_dir = base.join("redis-now");
    let redis_port = free_port().to_string();
    let redis_s = redis_dir.display().to_string();
    let real_s = real_redis.display().to_string();
    let (ok, provisioned) = aikit(
        &path,
        &home,
        &[
            "now-context",
            "service",
            "provision",
            "--service-dir",
            &redis_s,
            "--port",
            &redis_port,
            "--redis-server",
            &real_s,
            "--maxmemory-mb",
            "64",
        ],
    );
    assert!(ok, "{provisioned}");
    guard.redis_dir = Some(redis_dir.clone());
    let (ok, started) = aikit(
        &path,
        &home,
        &["now-context", "service", "start", "--service-dir", &redis_s],
    );
    assert!(ok, "{started}");
    assert_eq!(started["data"]["health"]["profile_conforms"], true);

    // The elected decision endpoint (loopback, SystemOne-compatible).
    let kev_port = free_port();
    let kev_cwd = base.join("kev-run/kev");
    fs::create_dir_all(&kev_cwd).unwrap();
    guard.kev = Some(
        Command::new("python3")
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
            .arg(kev_port.to_string())
            .current_dir(&kev_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && TcpStream::connect(("127.0.0.1", kev_port)).is_err() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let provider_file = base.join("decision-provider.json");
    fs::write(
        &provider_file,
        serde_json::to_vec(&json!({
            "schema": "aikit.decision-provider/v1", "mode": "endpoint",
            "address": format!("127.0.0.1:{kev_port}"),
            "limits": {"timeout_ms": 20000, "max_attempts": 1,
                       "max_input_tokens_per_attempt": 16384,
                       "max_output_tokens_per_attempt": 4000, "model": "kev-latest"}
        }))
        .unwrap(),
    )
    .unwrap();

    // A preparation request that elects the provider.
    let redis_election: Value =
        serde_json::from_slice(&fs::read(redis_dir.join("redis-now.json")).unwrap()).unwrap();
    let candidate = |i: u32, title: &str, text: &str| {
        json!({"source_ref": format!("context-source/proof-{i}"), "source_revision": "r1",
               "title": title, "excerpt": text, "route": null,
               "agent_visibility": "payload", "external_egress": "allowed"})
    };
    let request_file = base.join("prepare.json");
    fs::write(
        &request_file,
        serde_json::to_vec(&json!({
            "schema": "aikit.now-preparation-request/v1", "redis": redis_election,
            "project_ref": "project/central-field-proof",
            "now_ref": "central:now:control:root:06cca2f04e1a408f451f36263de0d3c1dda2aa8547a27d3ed163c222bc711dea",
            "participant_ref": "agent/central-field-proof/a",
            "agent_session": "agent-session/central-field-proof",
            "concern": "Answer a reader's question about moving between graph, pages and Expressions",
            "disclosure_revision": "d1",
            "central": {"root": base, "ctrl_bin": base.join("tripwire/ctrl")},
            "candidate_items": [
                candidate(1, "Graph navigation", "Selecting a node does not navigate."),
                candidate(2, "Expression tabs", "An Expression opens as a tab and returns to its source."),
            ],
            "expected_version": 0, "external_provider": false,
            "selection": {"mode": "provider", "provider_file": provider_file,
                          "state": {"undertaking": "ground the reader's question"},
                          "relevance_threshold": 0.5}
        }))
        .unwrap(),
    )
    .unwrap();
    let (ok, prepared) = aikit(
        &path,
        &home,
        &[
            "now-context",
            "prepare",
            "--request-file",
            request_file.to_str().unwrap(),
        ],
    );
    assert!(ok, "{prepared}");
    let selection = &prepared["data"]["selection"];
    assert_eq!(selection["mode"], "provider");
    assert_eq!(selection["decisionInvocation"]["outcome"], "completed");
    assert_eq!(
        selection["decisionInvocation"]["standing"],
        "local-protocol"
    );
    assert_eq!(
        selection["selectedCandidateRefs"].as_array().unwrap().len(),
        2
    );
    let invocation = selection["decisionInvocation"]["invocation_ref"]
        .as_str()
        .unwrap();
    let provider_identity = selection["decisionProvider"].as_str().unwrap();

    // The published view carries both, so a delivery receipt can name them.
    let election_file = redis_dir.join("redis-now.json");
    let (ok, inspected) = aikit(
        &path,
        &home,
        &[
            "now-context",
            "inspect",
            "--config-file",
            election_file.to_str().unwrap(),
            "--participant-ref",
            "agent/central-field-proof/a",
        ],
    );
    assert!(ok, "{inspected}");
    let view = &inspected["data"]["prepared"];
    assert_eq!(view["version"], 1);
    assert_eq!(
        view["jev_invocation_ref"], invocation,
        "the decision invocation is recorded"
    );
    assert_eq!(view["basis"]["decision_provider"], provider_identity);

    assert_eq!(
        fs::read_to_string(&mark).unwrap_or_default(),
        "",
        "an excluded owner was reached"
    );
}
