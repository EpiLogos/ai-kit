//! Session commands must drive the installed multiplexer, not return a plausible
//! no-op. These tests use a private real tmux server and the real AIKit binary.

use std::fs;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use aikit_core::{MuxKind, SessionId};
use aikit_store::events::Timestamp;
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::state::{SessionRecord, SessionState, StateStore};
use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

static NEXT_SOCKET: AtomicU32 = AtomicU32::new(0);

fn socket() -> String {
    format!(
        "aikit-session-test-{}-{}",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    )
}

fn run(home: &std::path::Path, socket: &str, args: &[&str]) -> Output {
    Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home.join(".aikit-state"))
        .env("HOME", home)
        .env("AIKIT_TMUX_SOCKET", socket)
        .env("AIKIT_MUX", "tmux")
        .env_remove("AIKIT_PROJECT_ID")
        .env_remove("AIKIT_SESSION_ID")
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

#[test]
fn attach_reads_only_the_current_projects_session_record_from_real_state() {
    let root = tempfile::tempdir().unwrap();
    let project_a = root.path().join("project-a");
    let project_b = root.path().join("project-b");
    fs::create_dir_all(project_a.join(".aikit")).unwrap();
    fs::create_dir_all(project_b.join(".aikit")).unwrap();

    let home = AikitHome::at(root.path().join("state"));
    home.ensure_layout().unwrap();
    let index = Index::open(&home.database()).unwrap();
    let state = StateStore::new(&index);
    let now = Timestamp::now();
    for (project_root, binding) in [
        (project_a.clone(), "a-private-session"),
        (project_b.clone(), "b-private-session"),
    ] {
        state
            .put_session(&SessionRecord {
                session_id: SessionId::generate(),
                name: "dev".into(),
                project_root: Some(project_root),
                project_marker: None,
                mux: MuxKind::Tmux,
                mux_session: Some(binding.into()),
                state: SessionState::Live,
                created_at: now,
                last_seen: now,
            })
            .unwrap();
    }

    let output = Command::new(cargo_bin("aikit"))
        .args(["--json", "session", "attach", "dev"])
        .env("AIKIT_HOME", home.root())
        .env("HOME", root.path())
        .env("AIKIT_MUX", "tmux")
        .env_remove("AIKIT_PROJECT_ID")
        .env_remove("AIKIT_SESSION_ID")
        .current_dir(&project_a)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reply = json(&output);
    assert_eq!(
        reply["data"]["command"],
        serde_json::json!(["tmux", "attach-session", "-t", "a-private-session"])
    );
    assert_eq!(reply["data"]["commands"].as_array().unwrap().len(), 1);
}

#[test]
fn cmux_attach_without_durable_state_refuses_to_invent_a_workspace_ref() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".aikit")).unwrap();
    let output = Command::new(cargo_bin("aikit"))
        .args(["--json", "session", "attach", "dev"])
        .env("AIKIT_HOME", root.path().join("state"))
        .env("HOME", root.path())
        .env("AIKIT_MUX", "cmux")
        .env_remove("AIKIT_PROJECT_ID")
        .env_remove("AIKIT_SESSION_ID")
        .current_dir(root.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let reply = json(&output);
    assert_eq!(reply["error"]["code"], "session.cmux_binding_missing");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("select-workspace"),
        "a human session name is not a valid cmux workspace ref"
    );
}

struct PrivateTmux(String);

impl PrivateTmux {
    fn command(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-L", self.0.as_str()];
        argv.extend_from_slice(args);
        Command::new("tmux").args(argv).output().unwrap()
    }
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        let _ = self.command(&["kill-server"]);
    }
}

#[test]
fn session_diff_is_read_only_when_the_named_session_does_not_exist() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".aikit")).unwrap();
    let server = PrivateTmux(socket());

    let output = run(
        home.path(),
        &server.0,
        &["--json", "session", "diff", "not-running"],
    );
    assert!(
        output.status.success(),
        "a missing session is a diff, not a command failure: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reply = json(&output);
    assert_eq!(reply["data"]["mux"], "tmux");
    assert_eq!(reply["data"]["matches_spec"], false);
    assert!(
        reply["data"]["differences"]
            .as_array()
            .is_some_and(|differences| differences.iter().any(|difference| {
                difference
                    .as_str()
                    .unwrap_or_default()
                    .contains("not running")
            })),
        "missing state must be reported explicitly: {reply}"
    );
    let exists = server.command(&["has-session", "-t", "not-running"]);
    assert!(
        !exists.status.success(),
        "a read-only diff created the session it was supposed to inspect"
    );
}

#[test]
fn session_diff_does_not_repair_an_existing_drifted_tmux_session() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".aikit")).unwrap();
    let spec = home.path().join("session.toml");
    fs::write(
        &spec,
        format!(
            "schema = 1\nid = \"production\"\nname = \"production\"\nroot = \"{}\"\n[backend]\nkind = \"tmux\"\n[[views]]\nid = \"main\"\n[[views.panes]]\nid = \"shell\"\ncommand = [\"sleep\", \"60\"]\n",
            home.path().display()
        ),
    )
    .unwrap();
    let server = PrivateTmux(socket());
    let up = run(
        home.path(),
        &server.0,
        &["--json", "session", "up", spec.to_str().unwrap()],
    );
    assert!(up.status.success());

    let untag = server.command(&[
        "set-option",
        "-p",
        "-u",
        "-t",
        "production:main",
        "@aikit_pane",
    ]);
    assert!(untag.status.success());

    let diff = run(
        home.path(),
        &server.0,
        &["--json", "session", "diff", "production"],
    );
    assert!(
        diff.status.success(),
        "diff failed: stdout={} stderr={}",
        String::from_utf8_lossy(&diff.stdout),
        String::from_utf8_lossy(&diff.stderr)
    );
    let reply = json(&diff);
    assert_eq!(reply["data"]["matches_spec"], false);
    assert!(reply["data"]["differences"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item.as_str().unwrap_or_default().contains("main/shell"))));

    let tag = server.command(&[
        "show-options",
        "-p",
        "-v",
        "-t",
        "production:main",
        "@aikit_pane",
    ]);
    assert!(
        !tag.status.success() || tag.stdout.is_empty(),
        "diff repaired the missing tag instead of observing it: {}",
        String::from_utf8_lossy(&tag.stdout)
    );
    let panes = server.command(&["list-panes", "-t", "production:main"]);
    assert_eq!(
        String::from_utf8_lossy(&panes.stdout).lines().count(),
        1,
        "diff must not add a replacement pane"
    );
}

#[test]
fn diff_and_reconcile_compile_the_explicit_portable_spec() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".aikit")).unwrap();
    let spec = home.path().join("portable.toml");
    fs::write(
        &spec,
        format!(
            "schema = 1\n\
             id = \"portable\"\n\
             name = \"portable\"\n\
             root = \"{}\"\n\
             [backend]\n\
             kind = \"tmux\"\n\
             [[views]]\n\
             id = \"main\"\n\
             [[views.panes]]\n\
             id = \"shell\"\n\
             command = [\"sleep\", \"60\"]\n\
             [[views.panes]]\n\
             id = \"logs\"\n\
             split_from = \"shell\"\n\
             direction = \"right\"\n\
             command = [\"sleep\", \"60\"]\n",
            home.path().display()
        ),
    )
    .unwrap();
    let server = PrivateTmux(socket());
    let up = run(
        home.path(),
        &server.0,
        &["--json", "session", "up", spec.to_str().unwrap()],
    );
    assert!(up.status.success());

    let panes = server.command(&[
        "list-panes",
        "-t",
        "portable:main",
        "-F",
        "#{@aikit_pane}\t#{pane_id}",
    ]);
    let logs = String::from_utf8_lossy(&panes.stdout)
        .lines()
        .find_map(|line| {
            let (tag, pane) = line.split_once('\t')?;
            (tag == "main/logs").then_some(pane.to_string())
        })
        .expect("the explicit spec created its logs pane");
    assert!(server.command(&["kill-pane", "-t", &logs]).status.success());

    let diff = run(
        home.path(),
        &server.0,
        &["--json", "session", "diff", spec.to_str().unwrap()],
    );
    assert!(
        diff.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&diff.stdout),
        String::from_utf8_lossy(&diff.stderr)
    );
    let reply = json(&diff);
    assert_eq!(reply["data"]["session"], "portable");
    assert!(reply["data"]["differences"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item.as_str().unwrap_or_default().contains("main/logs"))));

    let reconcile = run(
        home.path(),
        &server.0,
        &["--json", "session", "reconcile", spec.to_str().unwrap()],
    );
    assert!(
        reconcile.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&reconcile.stdout),
        String::from_utf8_lossy(&reconcile.stderr)
    );
    let reply = json(&reconcile);
    assert_eq!(reply["data"]["session"], "portable");
    let panes = server.command(&["list-panes", "-t", "portable:main"]);
    assert_eq!(
        String::from_utf8_lossy(&panes.stdout).lines().count(),
        2,
        "reconcile must restore the pane declared by the explicit spec"
    );
}

#[test]
fn session_up_builds_the_portable_spec_in_a_real_tmux_server_idempotently() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".aikit")).unwrap();
    let spec = home.path().join("session.toml");
    fs::write(
        &spec,
        format!(
            "schema = 1\n\
             id = \"production\"\n\
             name = \"production\"\n\
             root = \"{}\"\n\
             [backend]\n\
             kind = \"tmux\"\n\
             [[views]]\n\
             id = \"main\"\n\
             [[views.panes]]\n\
             id = \"shell\"\n\
             command = [\"sleep\", \"60\"]\n\
             [[views.panes]]\n\
             id = \"logs\"\n\
             split_from = \"shell\"\n\
             direction = \"right\"\n\
             command = [\"sleep\", \"60\"]\n",
            home.path().display()
        ),
    )
    .unwrap();
    let server = PrivateTmux(socket());

    let first = run(
        home.path(),
        &server.0,
        &[
            "--json",
            "session",
            "up",
            spec.to_str().expect("UTF-8 temp path"),
        ],
    );
    assert!(
        first.status.success(),
        "session up failed: stdout={} stderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let reply = json(&first);
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["mux"], "tmux");
    assert_eq!(reply["data"]["session"], "production");
    assert!(
        reply["warnings"].as_array().is_some_and(|warnings| warnings
            .iter()
            .all(|warning| { !warning.as_str().unwrap_or_default().contains("not wired") })),
        "a real session must not carry the old no-op warning: {reply}"
    );

    let panes = server.command(&[
        "list-panes",
        "-t",
        "production:main",
        "-F",
        "#{@aikit_pane}:#{pane_dead}",
    ]);
    assert!(
        panes.status.success(),
        "the real session was not created: {}",
        String::from_utf8_lossy(&panes.stderr)
    );
    let first_panes = String::from_utf8_lossy(&panes.stdout)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(first_panes.len(), 2);
    assert!(first_panes.iter().all(|pane| pane.ends_with(":0")));

    let second = run(
        home.path(),
        &server.0,
        &["--json", "session", "up", spec.to_str().unwrap()],
    );
    assert!(second.status.success());
    let panes = server.command(&[
        "list-panes",
        "-t",
        "production:main",
        "-F",
        "#{@aikit_pane}",
    ]);
    assert_eq!(
        String::from_utf8_lossy(&panes.stdout).lines().count(),
        2,
        "re-running session up must attach/reconcile, not duplicate panes"
    );
}

#[test]
fn a_cmux_pinned_session_never_falls_through_to_tmux() {
    if Command::new("cmux").arg("--version").output().is_err() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".aikit")).unwrap();
    let spec = home.path().join("cmux-session.toml");
    fs::write(
        &spec,
        "schema = 1\n\
         id = \"cmux-production\"\n\
         name = \"cmux-production\"\n\
         [backend]\n\
         kind = \"cmux\"\n\
         [[views]]\n\
         id = \"main\"\n\
         [[views.panes]]\n\
         id = \"shell\"\n\
         command = [\"sleep\", \"60\"]\n",
    )
    .unwrap();
    let socket = socket();

    let output = run(
        home.path(),
        &socket,
        &["--json", "session", "up", spec.to_str().unwrap()],
    );
    assert!(
        !output.status.success(),
        "cmux is installed but not running on this test machine; AIKit must report that instead of returning a tmux/no-op success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let reply = json(&output);
    assert!(
        reply["error"]["code"]
            .as_str()
            .is_some_and(|code| code.starts_with("mux.cmux")),
        "the pinned adapter must own the failure: {reply}"
    );
    let tmux = Command::new("tmux")
        .args(["-L", &socket, "has-session", "-t", "cmux-production"])
        .output()
        .unwrap();
    assert!(!tmux.status.success(), "cmux plan leaked into tmux");
}
