//! The real `mux install` command owns the binding all the way to live tmux.

use std::fs;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

static NEXT_SOCKET: AtomicU32 = AtomicU32::new(0);

fn run(home: &std::path::Path, socket: &str, args: &[&str]) -> Output {
    Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .env("AIKIT_TMUX_SOCKET", socket)
        .current_dir(home)
        .output()
        .expect("the real aikit binary runs")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not JSON ({error}): {:?}; stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn socket() -> String {
    format!(
        "aikit-install-test-{}-{}",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    )
}

struct Server {
    socket: String,
}

impl Server {
    fn start() -> Self {
        let socket = socket();
        let output = Command::new("tmux")
            .args([
                "-L",
                &socket,
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "install-test",
            ])
            .output()
            .expect("tmux is installed for the real integration test");
        assert!(
            output.status.success(),
            "private tmux server failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self { socket }
    }

    fn list_key(&self, key: &str) -> Output {
        Command::new("tmux")
            .args(["-L", &self.socket, "list-keys", "-T", "root", key])
            .output()
            .expect("list the private server's binding")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
    }
}

#[test]
fn custom_key_is_written_and_a_missing_server_is_reported_honestly() {
    let home = tempfile::tempdir().unwrap();
    let output = run(
        home.path(),
        &socket(),
        &["--json", "mux", "install", "tmux", "--key", "M-k"],
    );
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply = json(&output);
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["key"], "M-k");
    assert_eq!(reply["data"]["live"], false);
    assert!(
        reply["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty()),
        "no running server must be an explicit degraded verification: {reply}"
    );
    let config = fs::read_to_string(home.path().join(".tmux.conf")).unwrap();
    assert!(config.contains("bind-key -n M-k display-popup -E -w 82% -h 70% -T AIKit 'aikit ui'"));
}

#[test]
fn installing_twice_is_a_successful_byte_for_byte_no_op() {
    let home = tempfile::tempdir().unwrap();
    let socket = socket();
    let first = run(home.path(), &socket, &["--json", "mux", "install", "tmux"]);
    assert!(first.status.success());
    let config = home.path().join(".tmux.conf");
    let before = fs::read(&config).unwrap();

    let second = run(home.path(), &socket, &["--json", "mux", "install", "tmux"]);
    assert!(
        second.status.success(),
        "idempotent reinstall failed: stdout={} stderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(fs::read(config).unwrap(), before);
    assert_eq!(json(&second)["data"]["edits"], 0);
}

#[test]
fn a_key_that_could_inject_tmux_configuration_is_rejected_before_writing() {
    let home = tempfile::tempdir().unwrap();
    let output = run(
        home.path(),
        &socket(),
        &["--json", "mux", "install", "tmux", "--key", "M-a;run-shell"],
    );
    assert!(!output.status.success());
    assert_eq!(json(&output)["error"]["code"], "mux.invalid_key");
    assert!(!home.path().join(".tmux.conf").exists());
}

#[test]
fn an_existing_binding_is_refused_unless_replacement_is_explicit() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".tmux.conf");
    fs::write(&config, "bind-key -n M-a split-window\n").unwrap();

    let refused = run(
        home.path(),
        &socket(),
        &["--json", "mux", "install", "tmux"],
    );
    assert!(!refused.status.success());
    let error = json(&refused);
    assert_eq!(error["error"]["code"], "mux.key_conflict");
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "bind-key -n M-a split-window\n",
        "a refused install writes nothing"
    );

    let replaced = run(
        home.path(),
        &socket(),
        &["--json", "mux", "install", "tmux", "--replace-key"],
    );
    assert!(
        replaced.status.success(),
        "explicit replacement failed: {}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    let contents = fs::read_to_string(config).unwrap();
    assert!(contents.starts_with("bind-key -n M-a split-window\n"));
    assert!(contents.contains("bind-key -n M-a display-popup"));
}

#[test]
fn an_effective_live_binding_is_a_conflict_even_when_the_config_is_empty() {
    let server = Server::start();
    let bound = Command::new("tmux")
        .args([
            "-L",
            &server.socket,
            "bind-key",
            "-n",
            "M-a",
            "split-window",
        ])
        .output()
        .unwrap();
    assert!(bound.status.success());
    let home = tempfile::tempdir().unwrap();

    let output = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux"],
    );
    assert!(!output.status.success());
    assert_eq!(json(&output)["error"]["code"], "mux.key_conflict");
    assert!(!home.path().join(".tmux.conf").exists());
}

#[test]
fn a_running_private_server_is_reloaded_and_the_live_binding_is_verified() {
    let server = Server::start();
    let home = tempfile::tempdir().unwrap();

    let output = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux"],
    );
    assert!(
        output.status.success(),
        "live install failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reply = json(&output);
    assert_eq!(reply["data"]["live"], true);
    assert_eq!(reply["data"]["verified"], true);

    let binding = server.list_key("M-a");
    assert!(
        binding.status.success(),
        "binding was not live: {}",
        String::from_utf8_lossy(&binding.stderr)
    );
    let line = String::from_utf8_lossy(&binding.stdout);
    assert!(line.contains("display-popup"));
    assert!(line.contains("82%"));
    assert!(line.contains("70%"));
    assert!(line.contains("aikit ui"));
}
