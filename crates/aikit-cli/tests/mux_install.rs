//! The real `mux install` command owns the binding all the way to live tmux.

use std::fs;
#[cfg(target_os = "macos")]
use std::io::{Read, Write};
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

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

    fn command(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-L", self.socket.as_str()];
        argv.extend_from_slice(args);
        Command::new("tmux")
            .args(argv)
            .output()
            .expect("run a command on the private tmux server")
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
    assert!(config.contains(
        "bind-key -n M-k display-popup -E -w 82% -h 70% \
         -d '#{pane_current_path}' -T AIKit 'aikit ui'"
    ));
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
fn a_live_binding_that_only_mentions_aikit_words_is_not_mistaken_for_ownership() {
    let server = Server::start();
    let bound = Command::new("tmux")
        .args([
            "-L",
            &server.socket,
            "bind-key",
            "-n",
            "M-a",
            "run-shell",
            "printf 'display-popup aikit ui'",
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
fn prefix_and_named_table_bindings_do_not_collide_with_the_global_hotkey() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".tmux.conf");
    fs::write(
        &config,
        "bind-key M-a last-window\n\
         bind-key -T copy-mode M-a send-keys -X begin-selection\n",
    )
    .unwrap();

    let output = run(
        home.path(),
        &socket(),
        &["--json", "mux", "install", "tmux"],
    );
    assert!(
        output.status.success(),
        "non-root bindings were falsely rejected: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn changing_the_managed_key_removes_the_old_live_binding() {
    let server = Server::start();
    let home = tempfile::tempdir().unwrap();
    let first = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux"],
    );
    assert!(first.status.success());
    assert!(server.list_key("M-a").status.success());

    let changed = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux", "--key", "M-k"],
    );
    assert!(
        changed.status.success(),
        "key change failed: stdout={} stderr={}",
        String::from_utf8_lossy(&changed.stdout),
        String::from_utf8_lossy(&changed.stderr)
    );
    assert!(server.list_key("M-k").status.success());
    assert!(
        !server.list_key("M-a").status.success(),
        "changing the managed key left the previous AIKit key live"
    );
}

#[test]
fn procedure_undo_restores_both_the_config_and_the_live_key_table() {
    let server = Server::start();
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".tmux.conf");
    let original = "set -g status off\n";
    fs::write(&config, original).unwrap();

    let installed = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux"],
    );
    assert!(installed.status.success());
    let installed_reply = json(&installed);
    let procedure = installed_reply["data"]["procedure"].as_str().unwrap();
    assert!(server.list_key("M-a").status.success());

    let undone = run(
        home.path(),
        &server.socket,
        &["--json", "procedure", "undo", procedure],
    );
    assert!(
        undone.status.success(),
        "undo failed: stdout={} stderr={}",
        String::from_utf8_lossy(&undone.stdout),
        String::from_utf8_lossy(&undone.stderr)
    );
    assert_eq!(fs::read_to_string(config).unwrap(), original);
    assert!(
        !server.list_key("M-a").status.success(),
        "undo removed the block on disk but left Alt-A live in tmux"
    );
}

#[test]
fn undo_restores_a_user_binding_that_was_explicitly_replaced() {
    let server = Server::start();
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".tmux.conf");
    fs::write(&config, "bind-key -n M-a split-window\n").unwrap();
    let seeded = server.command(&["source-file", config.to_str().unwrap()]);
    assert!(seeded.status.success());

    let installed = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux", "--replace-key"],
    );
    assert!(installed.status.success());
    let procedure = json(&installed)["data"]["procedure"]
        .as_str()
        .unwrap()
        .to_string();

    let undone = run(
        home.path(),
        &server.socket,
        &["--json", "procedure", "undo", &procedure],
    );
    assert!(undone.status.success());
    let binding = server.list_key("M-a");
    assert!(binding.status.success());
    let rendered = String::from_utf8_lossy(&binding.stdout);
    assert!(rendered.contains("split-window"));
    assert!(!rendered.contains("aikit ui"));
}

#[cfg(target_os = "macos")]
#[test]
fn the_installed_alt_a_opens_the_real_surface_and_ctrl_t_switches_modes() {
    let server = Server::start();
    let home = tempfile::tempdir().unwrap();
    let installed = run(
        home.path(),
        &server.socket,
        &["--json", "mux", "install", "tmux"],
    );
    assert!(
        installed.status.success(),
        "install failed: stdout={} stderr={}",
        String::from_utf8_lossy(&installed.stdout),
        String::from_utf8_lossy(&installed.stderr)
    );

    let binary = cargo_bin("aikit");
    let binary_directory = binary.parent().unwrap();
    let source_project = home.path().join("source-project");
    fs::create_dir_all(source_project.join(".aikit")).unwrap();
    let source = server.command(&[
        "respawn-pane",
        "-k",
        "-t",
        "install-test:0.0",
        "-c",
        source_project.to_str().unwrap(),
        "sleep 60",
    ]);
    assert!(
        source.status.success(),
        "could not prepare the popup's source pane: {}",
        String::from_utf8_lossy(&source.stderr)
    );
    let path = format!(
        "{}:{}",
        binary_directory.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    for (key, value) in [
        ("PATH", path.as_str()),
        ("AIKIT_HOME", home.path().to_str().unwrap()),
        ("HOME", home.path().to_str().unwrap()),
    ] {
        let output = server.command(&["set-environment", "-g", key, value]);
        assert!(
            output.status.success(),
            "could not set {key} on private tmux: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut client = Command::new("/usr/bin/script")
        .args([
            "-q",
            "/dev/null",
            "tmux",
            "-L",
            &server.socket,
            "attach-session",
            "-t",
            "install-test",
        ])
        .env("TERM", "xterm-256color")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("attach a real pseudo-terminal client");
    let mut input = client.stdin.take().unwrap();
    let mut output = client.stdout.take().unwrap();
    let (signal_tx, signal_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut palette_seen = false;
        let mut tree_seen = false;
        while let Ok(read) = output.read(&mut buffer) {
            if read == 0 {
                break;
            }
            all.extend_from_slice(&buffer[..read]);
            if !palette_seen
                && all
                    .windows(b"Ctrl-T tree".len())
                    .any(|window| window == b"Ctrl-T tree")
            {
                palette_seen = true;
                let _ = signal_tx.send("palette");
            }
            if !tree_seen
                && all
                    .windows(b"AIKit tree".len())
                    .any(|window| window == b"AIKit tree")
            {
                tree_seen = true;
                let _ = signal_tx.send("tree");
            }
        }
        all
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    let client_name = loop {
        let listed = server.command(&["list-clients", "-F", "#{client_name}"]);
        if listed.status.success() {
            let name = String::from_utf8_lossy(&listed.stdout).trim().to_string();
            if !name.is_empty() {
                break name;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the real tmux client did not attach in time"
        );
        std::thread::sleep(Duration::from_millis(25));
    };

    input.write_all(b"\x1ba").unwrap();
    input.flush().unwrap();
    assert_eq!(
        signal_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        "palette",
        "Alt-A must open the palette mode of the real AIKit binary"
    );

    input.write_all(b"\x14").unwrap();
    input.flush().unwrap();
    assert_eq!(
        signal_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        "tree",
        "Ctrl-T must switch the same popup into tree mode"
    );

    input.write_all(b"\x1b").unwrap();
    input.flush().unwrap();
    std::thread::sleep(Duration::from_millis(75));
    input.write_all(b"\x1b").unwrap();
    input.flush().unwrap();
    std::thread::sleep(Duration::from_millis(75));
    let detached = server.command(&["detach-client", "-t", &client_name]);
    assert!(detached.status.success());
    drop(input);

    let status = client.wait().expect("the attached client exits");
    let rendered = reader.join().expect("the output reader exits");
    assert!(
        status.success(),
        "the PTY client failed; output={}",
        String::from_utf8_lossy(&rendered)
    );
    assert!(
        String::from_utf8_lossy(&rendered).contains("AIKit tree"),
        "the real popup never rendered tree mode"
    );
    assert!(
        String::from_utf8_lossy(&rendered).contains("source-project"),
        "the popup did not inherit and resolve the source pane's working directory"
    );

    let panes = server.command(&["list-panes", "-t", "install-test", "-F", "#{pane_id}"]);
    assert!(
        panes.status.success(),
        "closing the popup must leave its source session alive: {}",
        String::from_utf8_lossy(&panes.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&panes.stdout).lines().count(),
        1,
        "a popup must not create or destroy source panes"
    );
}
