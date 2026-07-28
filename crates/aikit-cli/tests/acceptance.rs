//! The release-blocking acceptance suite (`ARCHITECTURE.md` §15), driven against
//! the **real built `aikit` binary** with a **real `AIKIT_HOME`**, a **real git
//! repository**, **real SQLite**, and **real tmux** where a multiplexer is the
//! thing under test.
//!
//! Every capability these tests exercise comes from `examples/registry/`, copied
//! into the temp home under the source name `seed`. That registry doubles as the
//! documentation of what a real capsule looks like; using it here is what keeps
//! that documentation honest.
//!
//! ## Where a case is not driven purely by the binary, and why
//!
//! Three seams are exercised through the real library adapters (`aikit-adapters`,
//! `aikit-store`) rather than the binary, because the binary does not yet wire
//! them and pretending it did would be a worse test than saying so:
//!
//! * **cmux topology** — cmux is a macOS GUI terminal with a JSON control socket;
//!   it cannot be driven headlessly, so its behaviour is exercised at the contract
//!   level with recorded responses (a `ScriptedRunner`), exactly as the adapter's
//!   own contract tests and the brief prescribe. The *effect* of what cmux carries
//!   is then verified against the real binary.
//! * **client projections (Claude, Codex)** — `aikit apply` currently materialises
//!   only the shell shim projection into a generation. The Claude/Codex projection
//!   contracts are therefore exercised through the real adapters fed the real
//!   resolved state produced by the shared `Service` (the same engine the binary
//!   runs), with real files on disk.
//! * **`session up`** — the capsule-spec → mux-adapter wiring is stubbed in the
//!   CLI, so the "same portable capsule in two multiplexers" case compiles the
//!   seed session capsule's spec and hands the identical `SessionPlan` to the real
//!   tmux and cmux adapters.
//!
//! These gaps are reported in the integrator's notes.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

use aikit_core::catalog::Catalog;
use aikit_core::context::Isolation;
use aikit_core::id::{CapsuleId, ContextId, RegistrySource, SessionId};
use aikit_core::platform::{MuxKind, TargetId};
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
};
use aikit_core::resolve::ResolvedView;
use aikit_core::session::{SessionPlan, SessionSpec};
use aikit_core::trust::{TrustKey, TrustState};
use aikit_core::Capsule;

use aikit_adapters::clients::claude::ClaudeAdapter;
use aikit_adapters::clients::codex::CodexAdapter;
use aikit_adapters::mux::cmux::Cmux;
use aikit_adapters::mux::tmux::Tmux as MuxTmux;
use aikit_adapters::mux::{MuxAdapter, ReconcileMode, SessionIdentity};
use aikit_adapters::runner::{ScriptedRunner, SystemRunner};

use aikit_cli::app::Service;

use aikit_store::generation::{self, GenerationBuilder};
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::inbox::{CandidateState, Capture, Inbox};
use aikit_store::registry::load_registry;
use aikit_store::trust::TrustStore;

/// The registry source name the seed registry is copied in under.
const SEED_SOURCE: &str = "seed";

// ===========================================================================
// Harness
// ===========================================================================

/// The committed seed registry, resolved relative to this crate.
fn seed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/aikit-cli has a workspace root two levels up")
        .join("examples/registry")
}

/// A fresh, isolated AIKit home with the seed registry loaded.
struct Fixture {
    _tmp: TempDir,
    home: AikitHome,
    path: PathBuf,
}

fn fresh_home() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().to_path_buf();
    let home = AikitHome::at(&path);
    home.ensure_layout().unwrap();

    let seed_dest = home.registry(SEED_SOURCE);
    copy_tree(&seed_dir(), &seed_dest);
    // A git checkout does not always preserve the execute bit, so restore it on the
    // two payloads that are run as real subprocesses.
    make_executable(&seed_dest.join("capsules/script/rust/cargo-nextest/payload/run.sh"));
    make_executable(&seed_dest.join("capsules/hook/guard/project-boundary/payload/check"));

    Fixture {
        _tmp: tmp,
        home,
        path,
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

fn make_executable(path: &Path) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(target_os = "macos")]
fn wait_for_pty_ui(
    child: &mut std::process::Child,
) -> std::thread::JoinHandle<Vec<u8>> {
    let mut stdout = child.stdout.take().expect("PTY stdout is captured");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut announced = false;
        while let Ok(read) = stdout.read(&mut buffer) {
            if read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..read]);
            if !announced
                && output
                    .windows(b"\x1b[?1000h".len())
                    .any(|window| window == b"\x1b[?1000h")
            {
                announced = true;
                let _ = ready_tx.send(());
            }
        }
        output
    });
    ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the PTY UI enables mouse capture after entering raw mode");
    reader
}

#[cfg(target_os = "macos")]
fn finish_pty(
    mut child: std::process::Child,
    stdout_reader: std::thread::JoinHandle<Vec<u8>>,
) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>) {
    let status = child.wait().expect("the PTY child exits");
    let stdout = stdout_reader.join().expect("the PTY output reader exits");
    let mut stderr = Vec::new();
    if let Some(mut stream) = child.stderr.take() {
        stream.read_to_end(&mut stderr).unwrap();
    }
    (status, stdout, stderr)
}

/// Record a deliberate human trust review for the given capsules, keyed on the
/// exact `(source, capsule, revision)` — the same revision the binary computes,
/// because both load the same files under the same source name. Skills, hooks and
/// guidance stay inert until this happens.
fn trust(fixture: &Fixture, ids: &[&str]) {
    let index = Index::open(&fixture.home.database()).unwrap();
    let load = load_registry(
        &fixture.home.registry(SEED_SOURCE),
        RegistrySource::new(SEED_SOURCE),
    )
    .unwrap();
    assert!(
        load.problems.is_empty(),
        "the seed registry must load cleanly: {:?}",
        load.problems
    );
    let store = TrustStore::new(&index);
    for id_str in ids {
        let id = CapsuleId::parse(id_str).unwrap();
        let capsule = load
            .catalog
            .get(&id)
            .unwrap_or_else(|| panic!("seed registry is missing {id_str}"));
        let revision = capsule.revision.clone().expect("a loaded capsule has a revision");
        let key = TrustKey::new(RegistrySource::new(SEED_SOURCE), id, revision);
        store.record(&key, TrustState::Trusted, None).unwrap();
    }
    // `index` is dropped here, closing the database before any binary opens it.
}

/// Run the real binary against this home, returning its raw output.
fn aikit(fixture: &Fixture, cwd: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(cargo_bin("aikit"));
    cmd.args(args).env("AIKIT_HOME", &fixture.path).current_dir(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("the aikit binary runs")
}

/// Run the real binary with a JSON payload fed to its stdin (the hook dispatcher).
fn aikit_stdin(
    fixture: &Fixture,
    cwd: &Path,
    env: &[(&str, &str)],
    args: &[&str],
    stdin: &[u8],
) -> Output {
    let mut cmd = Command::new(cargo_bin("aikit"));
    cmd.args(args)
        .env("AIKIT_HOME", &fixture.path)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("the aikit binary spawns");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin)
        .expect("wrote the event to stdin");
    child.wait_with_output().expect("the aikit binary finishes")
}

fn json_of(out: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {stdout:?}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn run_json(fixture: &Fixture, cwd: &Path, env: &[(&str, &str)], args: &[&str]) -> (Output, Value) {
    let out = aikit(fixture, cwd, env, args);
    let value = json_of(&out);
    (out, value)
}

fn expect_ok(out: &Output, v: &Value, what: &str) {
    assert!(
        out.status.success() && v["ok"] == true,
        "{what} should succeed but did not: status={:?}\nbody={v}\nstderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The ids of the active skills in a `status --json` envelope, sorted.
fn active_skills(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = v["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["kind"] == "skill")
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

/// A real, minimal git repository that is also an AIKit project.
fn project_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path();
    git(p, &["init", "-q", "-b", "main"]);
    fs::create_dir_all(p.join(".aikit")).unwrap();
    fs::write(p.join("README.md"), "# project\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "initial"]);
    tmp
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// An environment lookup for the in-process `Service`, so a test drives the same
/// engine the binary does without touching the real process environment.
fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k| map.get(k).cloned()
}

fn no_env(_k: &str) -> Option<String> {
    None
}

fn resolved_context(view: &ResolvedView, roots: &BTreeMap<CapsuleId, PathBuf>) -> ResolvedContext {
    ResolvedContext {
        view: view.clone(),
        capsule_roots: roots.clone(),
    }
}

fn item_targets(item: &ProjectionItem, needle: &str) -> bool {
    item.destination()
        .map(|d| d.to_string_lossy().contains(needle))
        .unwrap_or(false)
}

// ===========================================================================
// A private tmux server, torn down even on panic
// ===========================================================================

static SOCKET_COUNTER: AtomicU32 = AtomicU32::new(0);

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

macro_rules! require_tmux {
    ($name:literal) => {
        if !tmux_available() {
            eprintln!(
                "SKIP {}: tmux is not installed, so the real-server behaviour was not exercised",
                $name
            );
            return;
        }
    };
}

/// A tmux server on a private socket. Its `Drop` kills the server during unwind
/// too, so a panicking test cannot leave a server — or its `sleep`ing panes —
/// behind, and nothing here can reach the user's own session.
struct TmuxServer {
    socket: String,
}

impl TmuxServer {
    fn start() -> Self {
        let socket = format!(
            "aikit-accept-{}-{}",
            std::process::id(),
            SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        Self { socket }
    }

    fn raw(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-L", self.socket.as_str()];
        argv.extend_from_slice(args);
        Command::new("tmux").args(&argv).output().expect("tmux runs")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.raw(args);
        assert!(
            out.status.success(),
            "tmux {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        let _ = self.raw(&["kill-server"]);
        // tmux leaves the socket inode behind after the server exits; the names are
        // unique per run, so a stale one is harmless — but a pile of them per run is
        // litter in somebody's /tmp.
        if let Some(path) = tmux_socket_path(&self.socket) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Where tmux places a named socket: `$TMUX_TMPDIR` (or `/tmp`) plus `tmux-<uid>`.
fn tmux_socket_path(socket: &str) -> Option<PathBuf> {
    let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let uid = String::from_utf8(Command::new("id").arg("-u").output().ok()?.stdout).ok()?;
    Some(PathBuf::from(base).join(format!("tmux-{}", uid.trim())).join(socket))
}

/// Wait for a pane's redirected output file to have content: a pane's command runs
/// asynchronously once tmux has forked it.
fn wait_for_file(path: &Path) -> String {
    for _ in 0..400 {
        if let Ok(contents) = fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                return contents;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("{} never got any content", path.display());
}

// ===========================================================================
// Case 1
// ===========================================================================

#[test]
fn two_tmux_sessions_for_the_same_project_carry_different_skill_sets() {
    require_tmux!("two_tmux_sessions_for_the_same_project_carry_different_skill_sets");
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review", "skill/rust/unsafe-audit"]);
    let project = project_repo();

    // Two AIKit sessions over the *same* project, each session overlay enabling a
    // different skill.
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_tmuxalpha")],
        &["enable", "skill/rust/rust-review", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable rust-review for session alpha");
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_tmuxbeta")],
        &["enable", "skill/rust/unsafe-audit", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable unsafe-audit for session beta");

    // Two REAL tmux sessions, each carrying its AIKit identity in the session
    // environment, and the real binary resolving inside each pane.
    let server = TmuxServer::start();
    let scratch = TempDir::new().unwrap();
    let out_a = scratch.path().join("alpha.json");
    let out_b = scratch.path().join("beta.json");
    launch_status_pane(&server, "alpha", &home.path, project.path(), "ses_tmuxalpha", &out_a);
    launch_status_pane(&server, "beta", &home.path, project.path(), "ses_tmuxbeta", &out_b);

    let va: Value = serde_json::from_str(wait_for_file(&out_a).trim()).expect("alpha wrote JSON");
    let vb: Value = serde_json::from_str(wait_for_file(&out_b).trim()).expect("beta wrote JSON");

    // Each tmux session really carries its own AIKit session identity...
    assert_eq!(va["context"]["session_id"], "ses_tmuxalpha");
    assert_eq!(vb["context"]["session_id"], "ses_tmuxbeta");
    // ...and therefore a different skill set over the one project.
    assert_eq!(active_skills(&va), vec!["skill/rust/rust-review"]);
    assert_eq!(active_skills(&vb), vec!["skill/rust/unsafe-audit"]);
    assert_ne!(active_skills(&va), active_skills(&vb));
}

/// Create a real tmux session that carries `session_id` (and the temp home) in its
/// environment, then runs the real binary's `status --json` into `out`.
fn launch_status_pane(
    server: &TmuxServer,
    name: &str,
    home: &Path,
    project: &Path,
    session_id: &str,
    out: &Path,
) {
    let bin = cargo_bin("aikit");
    // Create the session with no command yet, so the environment can be set before
    // any process starts in the pane (tmux copies the session environment into a
    // pane as it is created).
    let pane = server.ok(&[
        "new-session",
        "-d",
        "-s",
        name,
        "-c",
        project.to_str().unwrap(),
        "-P",
        "-F",
        "#{pane_id}",
    ]);
    server.ok(&["set-environment", "-t", name, "AIKIT_HOME", home.to_str().unwrap()]);
    server.ok(&["set-environment", "-t", name, "AIKIT_SESSION_ID", session_id]);
    let err = out.with_extension("err");
    let command = format!(
        "'{}' --cwd '{}' status --json >'{}' 2>'{}'",
        bin.display(),
        project.display(),
        out.display(),
        err.display()
    );
    server.ok(&["respawn-pane", "-k", "-t", &pane, &command]);
}

// ===========================================================================
// Case 2
// ===========================================================================

#[test]
fn two_cmux_workspaces_for_the_same_project_carry_different_session_overlays() {
    // Contract-level for the cmux topology (cmux is a GUI app with a JSON control
    // socket and cannot be driven headlessly), plus the real binary for the effect.
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review", "skill/rust/unsafe-audit"]);
    let project = project_repo();

    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_cmuxone")],
        &["enable", "skill/rust/rust-review", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable rust-review for cmux workspace one");
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_cmuxtwo")],
        &["enable", "skill/rust/unsafe-audit", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable unsafe-audit for cmux workspace two");

    // cmux has no session environment, so each workspace carries its AIKit identity
    // in the pane *command*. Two workspaces, two identities.
    let plan = plan_from_spec(
        r#"
schema = 1
id = "payments"
name = "payments"
[[views]]
id = "work"
[[views.panes]]
id = "agent"
command = ["claude"]
"#,
    );
    let one = Cmux::new(cmux_full_runner()).with_identity(session_identity("ses_cmuxone"));
    one.ensure_session(&plan, ReconcileMode::default()).unwrap();
    let cmd_one = command_line(&one);
    assert!(cmd_one.contains("AIKIT_SESSION_ID=ses_cmuxone"), "got: {cmd_one}");

    let two = Cmux::new(cmux_full_runner()).with_identity(session_identity("ses_cmuxtwo"));
    two.ensure_session(&plan, ReconcileMode::default()).unwrap();
    let cmd_two = command_line(&two);
    assert!(cmd_two.contains("AIKIT_SESSION_ID=ses_cmuxtwo"), "got: {cmd_two}");
    assert_ne!(cmd_one, cmd_two, "the two workspaces carry different overlays");

    // And those identities really resolve to different overlays in the binary.
    let s1 = active_skills(
        &run_json(&home, project.path(), &[("AIKIT_SESSION_ID", "ses_cmuxone")], &["status", "--json"]).1,
    );
    let s2 = active_skills(
        &run_json(&home, project.path(), &[("AIKIT_SESSION_ID", "ses_cmuxtwo")], &["status", "--json"]).1,
    );
    assert_eq!(s1, vec!["skill/rust/rust-review"]);
    assert_eq!(s2, vec!["skill/rust/unsafe-audit"]);
    assert_ne!(s1, s2);
}

// ===========================================================================
// Case 3
// ===========================================================================

#[test]
fn a_project_profile_change_does_not_mutate_another_projects_context() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let p1 = project_repo();
    let p2 = project_repo();

    // Enable the skill at PROJECT scope in p1 only.
    let (o, v) = run_json(
        &home,
        p1.path(),
        &[],
        &["enable", "skill/rust/rust-review", "--scope", "project", "--json"],
    );
    expect_ok(&o, &v, "enable rust-review in p1");

    // p1 has it; p2 is untouched, both its context and its files.
    let s1 = active_skills(&run_json(&home, p1.path(), &[], &["status", "--json"]).1);
    let s2 = active_skills(&run_json(&home, p2.path(), &[], &["status", "--json"]).1);
    assert_eq!(s1, vec!["skill/rust/rust-review"]);
    assert!(s2.is_empty(), "another project must not see p1's profile change: {s2:?}");
    assert!(
        !p2.path().join(".aikit/profile.toml").exists(),
        "p2's profile file was never written"
    );
}

// ===========================================================================
// Case 4
// ===========================================================================

#[test]
fn a_session_toggle_cannot_affect_a_non_child_context() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();

    // Toggle the skill on in session `child` only.
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_child")],
        &["enable", "skill/rust/rust-review", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable rust-review in the child session");

    let child = active_skills(
        &run_json(&home, project.path(), &[("AIKIT_SESSION_ID", "ses_child")], &["status", "--json"]).1,
    );
    assert_eq!(child, vec!["skill/rust/rust-review"], "the child session sees the toggle");

    // A sibling session is not a child of the toggle.
    let sibling = active_skills(
        &run_json(&home, project.path(), &[("AIKIT_SESSION_ID", "ses_sibling")], &["status", "--json"]).1,
    );
    assert!(sibling.is_empty(), "a sibling session is not a child: {sibling:?}");

    // Nor is the sessionless context in the same project.
    let sessionless = active_skills(&run_json(&home, project.path(), &[], &["status", "--json"]).1);
    assert!(
        sessionless.is_empty(),
        "the sessionless context is not a child of the session overlay: {sessionless:?}"
    );
}

// ===========================================================================
// Case 5
// ===========================================================================

#[test]
fn the_same_portable_session_capsule_launches_in_tmux_and_cmux() {
    require_tmux!("the_same_portable_session_capsule_launches_in_tmux_and_cmux");
    let home = fresh_home();
    let plan = compiled_rust_dev_plan(&home);

    // --- tmux: for real ----------------------------------------------------
    let server = TmuxServer::start();
    let tmux = MuxTmux::new(SystemRunner::new()).with_socket(&server.socket);
    let binding = tmux.ensure_session(&plan, ReconcileMode::default()).unwrap();

    assert!(binding.created);
    assert_eq!(binding.kind, Some(MuxKind::Tmux));
    assert!(tmux.has_session("rust-dev").unwrap());
    let windows = server.ok(&["list-windows", "-t", "rust-dev", "-F", "#{window_name}"]);
    assert!(windows.lines().any(|w| w == "code"), "views: {windows}");
    assert!(windows.lines().any(|w| w == "agent"), "views: {windows}");
    assert_eq!(
        server
            .ok(&["list-panes", "-t", "rust-dev:code", "-F", "#{pane_id}"])
            .lines()
            .count(),
        2,
        "the code view is an editor split with a test pane"
    );
    assert!(binding.surface_of("code", "editor").is_some());
    assert!(binding.surface_of("code", "tests").is_some());
    assert!(binding.surface_of("agent", "assistant").is_some());

    // --- cmux: the identical plan, at the contract level -------------------
    let cmux = Cmux::new(cmux_full_runner()).with_identity(session_identity("ses_rustdev"));
    let cmux_binding = cmux.ensure_session(&plan, ReconcileMode::default()).unwrap();

    assert!(cmux_binding.created);
    assert_eq!(cmux_binding.kind, Some(MuxKind::Cmux));
    let lines = cmux.runner().call_lines();
    assert!(
        lines.iter().any(|l| l.contains("new-window")),
        "a two-view session becomes a cmux group: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("new-workspace --name rust-dev · code")),
        "got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("new-workspace --name rust-dev · agent")),
        "got: {lines:?}"
    );
    assert!(cmux_binding.views.contains_key("code"));
    assert!(cmux_binding.views.contains_key("agent"));
    // The same capsule produced a session in each multiplexer with its own geometry.
    assert_ne!(binding.kind, cmux_binding.kind);
}

/// Compile the seed `session/dev/rust-dev` capsule's portable spec into a plan.
fn compiled_rust_dev_plan(fixture: &Fixture) -> SessionPlan {
    let load = load_registry(
        &fixture.home.registry(SEED_SOURCE),
        RegistrySource::new(SEED_SOURCE),
    )
    .unwrap();
    let id = CapsuleId::parse("session/dev/rust-dev").unwrap();
    let capsule = load.catalog.get(&id).expect("seed has the session capsule");
    let section = capsule.session().expect("it is a session capsule");
    let spec_path = capsule.root.as_ref().unwrap().join(&section.spec);
    let text = fs::read_to_string(&spec_path).expect("the spec payload exists");
    SessionSpec::from_toml_str(&text).unwrap().compile().unwrap()
}

// ===========================================================================
// Case 6
// ===========================================================================

#[test]
fn a_failed_projection_leaves_the_previous_generation_active() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[],
        &["enable", "skill/rust/rust-review", "--scope", "project", "--json"],
    );
    expect_ok(&o, &v, "enable rust-review at project scope");

    // Resolve through the shared engine and build a first, good generation whose
    // Claude projection really lands.
    let service = Service::open(home.home.clone(), project.path(), no_env).unwrap();
    let view = service.resolved().clone();
    let roots = service.snapshot().capsule_roots();
    let ctx_dir = home.home.ensure_context_dir(&view.context.context_id).unwrap();

    let good = ClaudeAdapter::new(ctx_dir.join("claude"))
        .plan(&resolved_context(&view, &roots))
        .unwrap();
    let g1 = GenerationBuilder::new()
        .build(&ctx_dir, &view, &[good])
        .unwrap()
        .commit(None)
        .unwrap();
    assert_eq!(
        generation::current(&ctx_dir).unwrap().as_ref(),
        Some(&g1.id),
        "the first generation is current"
    );

    // Now a build whose projection cannot land: a link whose source payload is gone
    // (the "capsule payload removed since indexing" case the store documents).
    let doomed = ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live()).with_item(
        ProjectionItem::link("/no/such/payload/rust-review", ".claude/skills/rust-review").unwrap(),
    );
    let err = GenerationBuilder::new()
        .build(&ctx_dir, &view, &[doomed])
        .expect_err("a projection with a missing source must fail to build");
    assert_eq!(err.code(), "generation.source_missing");

    // The previous generation is still current, byte-for-byte untouched.
    assert_eq!(
        generation::current(&ctx_dir).unwrap().as_ref(),
        Some(&g1.id),
        "a failed build must never replace the live generation"
    );
}

// ===========================================================================
// Case 7
// ===========================================================================

#[test]
fn a_claude_session_receives_a_live_session_specific_skill_projection() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_claudehas")],
        &["enable", "skill/rust/rust-review", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable the skill in the Claude session");

    // The binary confirms the skill is active in that session's context.
    let active = active_skills(
        &run_json(&home, project.path(), &[("AIKIT_SESSION_ID", "ses_claudehas")], &["status", "--json"]).1,
    );
    assert_eq!(active, vec!["skill/rust/rust-review"]);

    // The Claude projection for the session: live, and it contains the skill.
    let svc = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[("AIKIT_SESSION_ID", "ses_claudehas")]),
    )
    .unwrap();
    let view = svc.resolved().clone();
    let roots = svc.snapshot().capsule_roots();
    let ctx_dir = home.home.ensure_context_dir(&view.context.context_id).unwrap();

    let claude = ClaudeAdapter::new(ctx_dir.join("claude"));
    let plan = claude.plan(&resolved_context(&view, &roots)).unwrap();
    assert_eq!(
        claude.activation_effect(None, &plan),
        ActivationEffect::LiveReloadExpected,
        "Claude picks up a session projection live"
    );
    assert!(
        plan.items.iter().any(|i| item_targets(i, "rust-review")),
        "the projection contains the session's skill"
    );

    // Materialise it and prove the skill really lands, session-specific, on disk.
    let committed = GenerationBuilder::new()
        .build(&ctx_dir, &view, &[plan])
        .unwrap()
        .commit(None)
        .unwrap();
    // Locate the projection via the store's own layout function rather than a
    // hardcoded path, so the test is coupled to where a generation actually places
    // a target's projection (see the integrator note on the `projections/claude`
    // vs `projections/claude-code` inconsistency).
    let skill_md = ctx_dir
        .join("generations")
        .join(committed.id.as_str())
        .join(generation::plan_root(&TargetId::claude_code()))
        .join(".claude/skills/rust-review/SKILL.md");
    let body = fs::read_to_string(&skill_md).expect("the projected SKILL.md resolves");
    assert!(body.contains("name: rust-review"), "it is really the seed skill");

    // A sibling Claude session that did not enable the skill gets a projection
    // without it: the projection is session-specific, not global.
    let sibling = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[("AIKIT_SESSION_ID", "ses_claudenone")]),
    )
    .unwrap();
    let sibling_plan = ClaudeAdapter::new(ctx_dir.join("claude"))
        .plan(&resolved_context(
            &sibling.resolved().clone(),
            &sibling.snapshot().capsule_roots(),
        ))
        .unwrap();
    assert!(
        sibling_plan.items.is_empty(),
        "a session without the skill projects nothing for Claude"
    );
}

// ===========================================================================
// Case 8
// ===========================================================================

#[test]
fn an_isolated_codex_task_gets_an_isolated_projection_and_a_shared_task_falls_back_with_a_reason() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    // A session-scoped skill: a per-session delta, exactly the kind a shared tree
    // must not silently leak to sibling tasks.
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_codextask")],
        &["enable", "skill/rust/rust-review", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable the session-only skill");

    // The binary reports the two isolations honestly.
    let iso_w = run_json(
        &home,
        project.path(),
        &[("AIKIT_ISOLATION", "worktree"), ("AIKIT_TASK", "review")],
        &["context", "current", "--json"],
    )
    .1;
    assert_eq!(iso_w["data"]["isolation"], "worktree");
    let iso_s = run_json(
        &home,
        project.path(),
        &[("AIKIT_ISOLATION", "shared"), ("AIKIT_TASK", "review")],
        &["context", "current", "--json"],
    )
    .1;
    assert_eq!(iso_s["data"]["isolation"], "shared");

    // Isolated branch: the task has its own tree, so Codex writes the skill natively
    // and the effect is live.
    let svc_iso = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[
            ("AIKIT_SESSION_ID", "ses_codextask"),
            ("AIKIT_ISOLATION", "worktree"),
            ("AIKIT_TASK", "review"),
        ]),
    )
    .unwrap();
    let iso_tree = TempDir::new().unwrap();
    let plan_iso = CodexAdapter::new(iso_tree.path())
        .plan(&resolved_context(
            &svc_iso.resolved().clone(),
            &svc_iso.snapshot().capsule_roots(),
        ))
        .unwrap();
    assert_eq!(plan_iso.effect, ActivationEffect::LiveReloadExpected);
    assert!(
        plan_iso.items.iter().any(|i| item_targets(i, "rust-review")),
        "an isolated task writes its skill natively"
    );

    // Shared branch: the task uses the session's tree, so Codex falls back and
    // states why, and does not write the session-only skill into the shared tree.
    let svc_sh = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[
            ("AIKIT_SESSION_ID", "ses_codextask"),
            ("AIKIT_ISOLATION", "shared"),
            ("AIKIT_TASK", "review"),
        ]),
    )
    .unwrap();
    let shared_tree = TempDir::new().unwrap();
    let plan_sh = CodexAdapter::new(shared_tree.path())
        .plan(&resolved_context(
            &svc_sh.resolved().clone(),
            &svc_sh.snapshot().capsule_roots(),
        ))
        .unwrap();
    match &plan_sh.effect {
        ActivationEffect::Brokered { reason } => assert!(
            reason.contains("shared working tree"),
            "the fallback must state the reason: {reason}"
        ),
        other => panic!("a shared tree with a session-only skill must broker, got {other:?}"),
    }
    assert!(
        plan_sh.items.iter().all(|i| !item_targets(i, "rust-review")),
        "the session-only skill must not be written into the shared tree"
    );
    assert!(
        plan_sh.notes.iter().any(|n| n.contains("broker")),
        "the note names the broker fallback: {:?}",
        plan_sh.notes
    );
}

// ===========================================================================
// Case 9
// ===========================================================================

#[test]
fn a_hook_bypass_is_visible_and_recorded() {
    let home = fresh_home();
    trust(&home, &["hook/guard/project-boundary"]);
    let project = project_repo();
    // A fixed context id, so the bypass ledger is the same ledger across the
    // separate short-lived processes below.
    let ctx: &[(&str, &str)] = &[("AIKIT_CONTEXT_ID", "ctx_bypasscase")];

    let (o, v) = run_json(
        &home,
        project.path(),
        ctx,
        &["enable", "hook/guard/project-boundary", "--scope", "project", "--json"],
    );
    expect_ok(&o, &v, "enable the boundary gate");

    let deny = br#"{"tool":"Bash","tool_input":{"command":"cat /etc/passwd"}}"#;

    // 1. The gate denies a boundary-crossing tool call.
    let v = json_of(&aikit_stdin(
        &home,
        project.path(),
        ctx,
        &["hook", "dispatch", "claude", "PreToolUse", "--json"],
        deny,
    ));
    assert_eq!(v["data"]["allowed"], false, "the gate must deny: {v}");
    assert!(v["data"]["denial"].is_string(), "the denial is reported");

    // 2. Issue a scoped, reasoned, single-use bypass.
    let (o, iv) = run_json(
        &home,
        project.path(),
        ctx,
        &["bypass", "issue", "--scope", "next-event", "--reason", "debugging a flake", "--json"],
    );
    expect_ok(&o, &iv, "issue a bypass");
    assert!(iv["data"]["bypass_id"].is_string());

    // 3. The bypass is VISIBLE in status, with its reason.
    let st = run_json(&home, project.path(), ctx, &["status", "--json"]).1;
    let open = st["data"]["bypasses"].as_array().unwrap();
    assert_eq!(open.len(), 1, "the open bypass is shown in status: {st}");
    assert_eq!(open[0]["reason"], "debugging a flake");

    // 4. The very next event is let through and RECORDED as bypassed.
    let v = json_of(&aikit_stdin(
        &home,
        project.path(),
        ctx,
        &["hook", "dispatch", "claude", "PreToolUse", "--json"],
        deny,
    ));
    assert_eq!(v["data"]["allowed"], true, "the bypass lets exactly one event through");
    assert_eq!(v["data"]["bypassed"], true, "and it is recorded as bypassed");

    // 5. The token is spent: status shows none, and the gate denies again — a
    //    bypass was a scoped token, never a global switch.
    let st = run_json(&home, project.path(), ctx, &["status", "--json"]).1;
    assert!(
        st["data"]["bypasses"].as_array().unwrap().is_empty(),
        "the token is spent after one event"
    );
    let v = json_of(&aikit_stdin(
        &home,
        project.path(),
        ctx,
        &["hook", "dispatch", "claude", "PreToolUse", "--json"],
        deny,
    ));
    assert_eq!(v["data"]["allowed"], false, "with the token spent, the gate denies once more");
}

// ===========================================================================
// Case 10
// ===========================================================================

#[test]
fn a_captured_secret_never_enters_the_ordinary_registry() {
    let home = fresh_home();
    let leaky = "#!/bin/sh\n# deploy helper\nexport AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\naws s3 sync ./dist s3://bucket\n";

    // Capture runs the real pipeline (it is not wired into the CLI). The secret is
    // scanned, redacted and quarantined before anything is written.
    let candidate_id = {
        let index = Index::open(&home.home.database()).unwrap();
        let inbox = Inbox::new(&home.home, &index);
        let outcome = inbox.capture(Capture::new("leaky deploy", leaky)).unwrap();
        assert_eq!(outcome.candidate.state, CandidateState::Quarantined);
        assert!(!outcome.candidate.findings.is_empty(), "the scanner found the key");

        let stored = fs::read_to_string(outcome.candidate.body_path()).unwrap();
        assert!(
            !stored.contains("AKIAIOSFODNN7EXAMPLE"),
            "the raw secret must never reach a file on disk"
        );
        assert!(
            outcome.candidate.path.starts_with(home.home.inbox_quarantine()),
            "a quarantined capture lives in the quarantine queue"
        );
        outcome.candidate.id.clone()
        // `index` is dropped here, before the binary opens the database.
    };

    // Promotion of a quarantined candidate is refused by the real binary, so the
    // secret never becomes a registry capsule.
    let cwd = TempDir::new().unwrap();
    let (out, v) = run_json(
        &home,
        cwd.path(),
        &[],
        &["promote", &candidate_id, "--id", "script/captured/leaky", "--json"],
    );
    assert!(!out.status.success(), "promotion must fail");
    assert_eq!(v["error"]["code"], "inbox.quarantined");
    assert!(
        !home
            .home
            .registry("personal")
            .join("capsules/script/captured/leaky")
            .exists(),
        "nothing was written into the registry"
    );
}

// ===========================================================================
// Case 11
// ===========================================================================

#[test]
fn promotion_completes_without_hand_writing_a_manifest() {
    let home = fresh_home();
    let clean = "#!/bin/sh\n# format the whole workspace\nexec cargo fmt --all\n";

    let candidate_id = {
        let index = Index::open(&home.home.database()).unwrap();
        let inbox = Inbox::new(&home.home, &index);
        let outcome = inbox.capture(Capture::new("format all", clean)).unwrap();
        assert_eq!(outcome.candidate.state, CandidateState::Ready);
        assert!(outcome.candidate.findings.is_empty(), "no secret, so it is ready");
        outcome.candidate.id.clone()
    };

    // The user supplies only an id (and, implicitly, a description). No manifest is
    // authored by hand.
    let cwd = TempDir::new().unwrap();
    let (out, v) = run_json(
        &home,
        cwd.path(),
        &[],
        &["promote", &candidate_id, "--id", "script/captured/fmt-all", "--json"],
    );
    expect_ok(&out, &v, "promote the clean candidate");

    let manifest_path = v["data"]["manifest"].as_str().unwrap();
    let manifest = fs::read_to_string(manifest_path).unwrap();
    assert!(
        Capsule::from_toml_str(&manifest).is_ok(),
        "the generated manifest must load as a real capsule:\n{manifest}"
    );
    assert!(
        manifest.contains("Generated by `aikit promote`"),
        "it is generated, not hand-written"
    );

    // And it is now a real, discoverable capsule in the registry.
    let read = run_json(
        &home,
        cwd.path(),
        &[],
        &["capabilities", "read", "script/captured/fmt-all", "--json"],
    )
    .1;
    assert_eq!(read["data"]["id"], "script/captured/fmt-all");
    assert_eq!(read["data"]["kind"], "script");
}

// ===========================================================================
// Case 12
// ===========================================================================

#[test]
fn the_entire_cli_works_without_a_running_daemon() {
    let home = fresh_home();
    let project = project_repo();

    // There is no daemon to start in the first place: no such subcommand exists.
    let no_daemon = aikit(&home, project.path(), &[], &["daemon"]);
    assert!(
        !no_daemon.status.success(),
        "there is no daemon subcommand to run"
    );

    let battery: &[&[&str]] = &[
        &["status", "--json"],
        &["status", "--all", "--json"],
        &["search", "rust", "--json"],
        &["explain", "skill/rust/rust-review", "--json"],
        &["capabilities", "list", "--json"],
        &["capabilities", "read", "guidance/research/deep-research", "--json"],
        &["context", "current", "--json"],
        &["bypasses", "--json"],
    ];
    for args in battery {
        let (out, v) = run_json(&home, project.path(), &[], args);
        assert!(
            out.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(v["ok"], true, "{args:?}: {v}");
    }

    // A non-JSON command works from cold too.
    let shell = aikit(&home, project.path(), &[], &["shell", "init", "bash"]);
    assert!(shell.status.success());
    assert!(!shell.stdout.is_empty(), "the shell snippet is printed");

    // No process is left holding this home's database open after a command returns:
    // every invocation was a short-lived process, and coordination is SQLite plus
    // per-context file locks, not a long-lived server. Scoped to this home's own
    // database, so it is immune to sibling tests that run the same binary against
    // their own homes.
    assert_eq!(
        processes_holding(&home.home.database()),
        0,
        "no aikit process outlived its command holding the home open; there is no daemon"
    );

    // And no daemon artefact — a control socket or a pidfile — was ever created.
    assert!(
        !daemon_artifact_exists(&home.path),
        "no socket or pidfile under the home: there is nothing for a daemon to bind"
    );
}

/// How many live processes hold `path` open. Best-effort: if `lsof` is unavailable
/// the check degrades to zero rather than failing a build for the wrong reason.
fn processes_holding(path: &Path) -> usize {
    match Command::new("lsof").arg("-t").arg(path).output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        Err(_) => 0,
    }
}

/// Would a daemon's socket or pidfile be found anywhere under the home?
fn daemon_artifact_exists(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if daemon_artifact_exists(&path) {
                return true;
            }
        } else {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".sock") || name.ends_with(".socket") || name.ends_with("daemon.pid") {
                return true;
            }
        }
    }
    false
}

// ===========================================================================
// Worktrees are OPT-IN (the deviation from the source specification)
// ===========================================================================

#[test]
fn aikit_task_spawn_review_creates_no_git_worktree_and_shares_the_session_tree() {
    let home = fresh_home();
    let project = project_repo();

    let (out, v) = run_json(&home, project.path(), &[], &["task", "spawn", "review", "--json"]);
    expect_ok(&out, &v, "task spawn review");

    assert_eq!(v["data"]["isolation"], "shared", "shared is the default");
    assert!(v["data"]["worktree"].is_null(), "a shared task cuts no worktree");
    assert!(
        v["data"]["note"].as_str().unwrap_or("").contains("shared tree"),
        "the shared fallback is stated, not hidden: {v}"
    );
    assert_eq!(
        fs::canonicalize(v["data"]["directory"].as_str().unwrap()).unwrap(),
        fs::canonicalize(project.path()).unwrap(),
        "a shared task runs in the session's own tree"
    );

    // git knows of exactly one worktree — the main one.
    let list = git_out(project.path(), &["worktree", "list"]);
    assert_eq!(list.lines().count(), 1, "no second checkout was created: {list}");
}

#[test]
fn aikit_task_spawn_review_worktree_creates_one() {
    let home = fresh_home();
    let project = project_repo();

    let (out, v) = run_json(
        &home,
        project.path(),
        &[],
        &["task", "spawn", "review", "--worktree", "--json"],
    );
    expect_ok(&out, &v, "task spawn review --worktree");

    assert_eq!(v["data"]["isolation"], "worktree");
    let wt = v["data"]["worktree"]["path"].as_str().unwrap();
    assert!(
        Path::new(wt).join(".git").exists(),
        "a real git worktree has a .git pointer"
    );
    let list = git_out(project.path(), &["worktree", "list"]);
    assert_eq!(list.lines().count(), 2, "the worktree is registered with git: {list}");
    assert!(list.contains("aikit/review"), "on its own branch: {list}");
}

#[test]
fn the_codex_projection_differs_between_a_shared_and_a_worktree_task_and_says_why() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_wtcodex")],
        &["enable", "skill/rust/rust-review", "--scope", "session", "--json"],
    );
    expect_ok(&o, &v, "enable the session-only skill");

    // Spawn both kinds of task through the real binary.
    let shared = run_json(&home, project.path(), &[], &["task", "spawn", "shared-review", "--json"]).1;
    let worked = run_json(
        &home,
        project.path(),
        &[],
        &["task", "spawn", "wt-review", "--worktree", "--json"],
    )
    .1;
    let shared_dir = shared["data"]["directory"].as_str().unwrap().to_string();
    let wt_dir = worked["data"]["worktree"]["path"].as_str().unwrap().to_string();

    // The worktree task owns its tree: Codex writes the skill natively and live.
    let svc_wt = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[
            ("AIKIT_SESSION_ID", "ses_wtcodex"),
            ("AIKIT_ISOLATION", "worktree"),
            ("AIKIT_TASK", "wt-review"),
        ]),
    )
    .unwrap();
    let plan_wt = CodexAdapter::new(&wt_dir)
        .plan(&resolved_context(
            &svc_wt.resolved().clone(),
            &svc_wt.snapshot().capsule_roots(),
        ))
        .unwrap();
    assert_eq!(plan_wt.effect, ActivationEffect::LiveReloadExpected);
    assert!(plan_wt.items.iter().any(|i| item_targets(i, "rust-review")));

    // The shared task uses the session's tree: Codex falls back and says why,
    // pointing at the `--worktree` that would change the answer.
    let svc_sh = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[
            ("AIKIT_SESSION_ID", "ses_wtcodex"),
            ("AIKIT_ISOLATION", "shared"),
            ("AIKIT_TASK", "shared-review"),
        ]),
    )
    .unwrap();
    let plan_sh = CodexAdapter::new(&shared_dir)
        .plan(&resolved_context(
            &svc_sh.resolved().clone(),
            &svc_sh.snapshot().capsule_roots(),
        ))
        .unwrap();
    assert!(
        matches!(plan_sh.effect, ActivationEffect::Brokered { .. }),
        "the shared task brokers rather than leaking into the shared tree"
    );
    assert!(plan_sh.items.iter().all(|i| !item_targets(i, "rust-review")));

    let explanation = format!("{} {:?}", plan_sh.effect.describe(), plan_sh.notes);
    assert!(
        explanation.contains("shared working tree") && explanation.contains("worktree"),
        "the difference is explained and points at --worktree: {explanation}"
    );
}

#[test]
fn task_close_refuses_to_delete_a_dirty_worktree_without_force() {
    let home = fresh_home();
    let project = project_repo();

    let v = run_json(
        &home,
        project.path(),
        &[],
        &["task", "spawn", "dirty", "--worktree", "--json"],
    )
    .1;
    let wt = v["data"]["worktree"]["path"].as_str().unwrap().to_string();

    // Make the worktree unclean with an untracked file.
    fs::write(Path::new(&wt).join("scratch.txt"), "unsaved work\n").unwrap();

    // A close without --force is refused, and the worktree is left intact.
    let (out, v) = run_json(&home, project.path(), &[], &["task", "close", "dirty", "--json"]);
    assert!(!out.status.success(), "a dirty worktree close must be refused");
    assert_eq!(v["error"]["code"], "task.worktree_dirty");
    assert!(Path::new(&wt).exists(), "a refused close leaves the worktree intact");

    // With --force it is discarded.
    let (out, v) = run_json(
        &home,
        project.path(),
        &[],
        &["task", "close", "dirty", "--force", "--json"],
    );
    expect_ok(&out, &v, "task close --force");
    assert_eq!(v["data"]["forced"], true);
    assert!(!Path::new(&wt).exists(), "force discards the worktree");
}

// ===========================================================================
// cmux contract fixtures (recorded control-protocol responses)
// ===========================================================================

fn session_identity(session_id: &str) -> SessionIdentity {
    SessionIdentity {
        session_id: Some(SessionId::parse(session_id).unwrap()),
        context_id: Some(ContextId::parse("ctx_cmuxcontext").unwrap()),
        project_root: Some(PathBuf::from("/work/payments")),
        view_root: None,
        profile: Some("rust-review".into()),
        isolation: Isolation::Shared,
    }
}

fn plan_from_spec(src: &str) -> SessionPlan {
    SessionSpec::from_toml_str(src).unwrap().compile().unwrap()
}

/// The recorded cmux call that delivered a pane command.
fn command_line(cmux: &Cmux<ScriptedRunner>) -> String {
    cmux.runner()
        .call_lines()
        .into_iter()
        .find(|l| l.contains("--command"))
        .expect("cmux delivered a pane command")
}

/// A cmux that answers every command AIKit issues while building a session, from
/// recorded responses shaped like the real control protocol.
fn cmux_full_runner() -> ScriptedRunner {
    const OK: &str = r#"{ "ok": true }"#;
    ScriptedRunner::new()
        .on("capabilities", CMUX_CAPABILITIES)
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .on("list-workspaces", r#"{ "workspaces": [] }"#)
        .on("new-window", CMUX_NEW_WINDOW)
        .sequence("new-workspace", &[CMUX_NEW_WORKSPACE_3, CMUX_NEW_WORKSPACE_4])
        .sequence("new-split", &[CMUX_NEW_SPLIT_5, CMUX_NEW_SPLIT_6])
        .on("move-workspace-to-window", OK)
        .on("respawn-pane", OK)
        .on("workspace-action", OK)
        .on("select-workspace", OK)
        .on("focus-pane", OK)
        .on("notify", OK)
        .on("identify", CMUX_IDENTIFY)
        .on("markdown", OK)
        .on("close-workspace", OK)
        .on("close-surface", OK)
        .on("rename-workspace", OK)
}

const CMUX_CAPABILITIES: &str = r#"{
  "version": "0.63.1",
  "build": "78",
  "commands": [
    "browser", "capabilities", "close-surface", "close-workspace", "current-workspace",
    "focus-pane", "focus-window", "identify", "list-pane-surfaces", "list-panes",
    "list-windows", "list-workspaces", "markdown", "move-workspace-to-window", "new-pane",
    "new-split", "new-surface", "new-window", "new-workspace", "notify", "ping",
    "rename-workspace", "respawn-pane", "select-workspace", "version", "workspace-action"
  ],
  "features": {
    "workspaces": true, "workspace_groups": true, "windows": true, "panes": true,
    "browser_surface": true, "status_pill": true, "progress": true, "log_panel": true
  }
}"#;

const CMUX_NEW_WINDOW: &str = r#"{ "window": { "id": "window:2", "title": "rust-dev" } }"#;
const CMUX_NEW_WORKSPACE_3: &str =
    r#"{ "workspace": { "id": "workspace:3", "title": "rust-dev · code" } }"#;
const CMUX_NEW_WORKSPACE_4: &str =
    r#"{ "workspace": { "id": "workspace:4", "title": "rust-dev · agent" } }"#;
const CMUX_NEW_SPLIT_5: &str =
    r#"{ "surface": { "id": "surface:5", "pane": "pane:4", "type": "terminal" } }"#;
const CMUX_NEW_SPLIT_6: &str =
    r#"{ "surface": { "id": "surface:6", "pane": "pane:5", "type": "terminal" } }"#;
const CMUX_IDENTIFY: &str = r#"{
  "workspace": { "id": "workspace:2", "title": "rust-dev · code" },
  "surface": { "id": "surface:7", "type": "terminal" },
  "window": { "id": "window:1" },
  "host": "localhost",
  "cwd": "/work/payments"
}"#;

// ---------------------------------------------------------------------------
// An unreviewed executable does not run unattended, even while inactive.
//
// The confirmation is what STANDARDS §6 requires of an unreviewed script. The
// dangerous case is the *inactive* one — a script run ad hoc, or reached through
// the broker by an agent, that no scope has reviewed — so the gate must key on
// the capsule's own trust, never on whether it happens to be active.
// ---------------------------------------------------------------------------

#[test]
fn an_unreviewed_script_is_refused_without_confirmation_even_when_inactive() {
    let fixture = fresh_home();
    let repo = project_repo();

    // `script/rust/cargo-nextest` is in the seed registry but reviewed by nobody,
    // and no profile here enables it, so it is inactive. Running it by id must be
    // refused before the payload is ever executed.
    let (out, v) = run_json(&fixture, repo.path(), &[], &["run", "script/rust/cargo-nextest", "--json"]);
    assert_eq!(v["ok"], false, "an unreviewed script must not run: {v}");
    assert_eq!(
        v["error"]["code"], "trust.required",
        "the refusal must be the trust gate, not an execution error: {v}"
    );
    // And it must have refused *before* running anything.
    assert!(
        !out.status.success(),
        "the process must exit non-zero when the gate refuses"
    );
}

#[test]
fn confirming_crosses_the_gate_so_the_failure_is_no_longer_about_trust() {
    let fixture = fresh_home();
    let repo = project_repo();

    // With --confirm, the trust gate is satisfied. The payload may still fail
    // (cargo nextest has nothing to run here), but the point is that the reason
    // is no longer `trust.required`: the gate was crossed.
    let out = aikit(
        &fixture,
        repo.path(),
        &[],
        &["run", "script/rust/cargo-nextest", "--confirm", "--json"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
        if v["ok"] == false {
            assert_ne!(
                v["error"]["code"], "trust.required",
                "--confirm must cross the trust gate: {v}"
            );
        }
    }
}

#[test]
fn a_reviewed_script_needs_no_confirmation() {
    let fixture = fresh_home();
    let repo = project_repo();
    trust(&fixture, &["script/rust/cargo-nextest"]);

    // Reviewed: the gate does not fire, so any failure is about execution, never
    // about trust — a human running a reviewed script is never nagged.
    let out = aikit(
        &fixture,
        repo.path(),
        &[],
        &["run", "script/rust/cargo-nextest", "--json"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
        assert_ne!(
            v["error"]["code"], "trust.required",
            "a reviewed script must not trip the confirmation gate: {v}"
        );
    }
}

// ===========================================================================
// Phase E: authority, parameterised lenses, and the interactive tree
// ===========================================================================

#[cfg(unix)]
#[test]
fn adoption_moves_authority_and_its_recorded_procedure_restores_it() {
    let home = fresh_home();
    let project = project_repo();
    let foreign = TempDir::new().unwrap();
    let skill = foreign.path().join("incident-review");
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: incident-review\ndescription: Review an incident end to end.\n---\n\nRead the evidence.\n",
    )
    .unwrap();
    fs::write(skill.join("references/checklist.md"), "# Evidence\n").unwrap();
    let original = fs::read(skill.join("SKILL.md")).unwrap();

    let source = foreign.path().to_str().unwrap();
    let (preview_out, preview) = run_json(
        &home,
        project.path(),
        &[],
        &["adopt", source, "--namespace", "acceptance", "--json"],
    );
    expect_ok(&preview_out, &preview, "preview adoption");
    assert_eq!(preview["data"]["applied"], false);
    assert!(!skill.join("SKILL.md").is_symlink());
    let digest = preview["data"]["review_digest"].as_str().unwrap();

    let (apply_out, applied) = run_json(
        &home,
        project.path(),
        &[],
        &[
            "adopt",
            source,
            "--namespace",
            "acceptance",
            "--yes",
            "--expect-digest",
            digest,
            "--json",
        ],
    );
    expect_ok(&apply_out, &applied, "apply adoption");
    let procedure = applied["data"]["procedure"].as_str().unwrap();
    let owned = home
        .home
        .registry("personal")
        .join("capsules/skill/acceptance/incident-review");
    assert_eq!(fs::read(owned.join("payload/SKILL.md")).unwrap(), original);
    assert!(
        skill.join("SKILL.md").is_symlink(),
        "the former authority is now a projection"
    );

    let (undo_out, undone) = run_json(
        &home,
        project.path(),
        &[],
        &["procedure", "undo", procedure, "--json"],
    );
    expect_ok(&undo_out, &undone, "undo adoption");
    assert!(!skill.join("SKILL.md").is_symlink());
    assert_eq!(fs::read(skill.join("SKILL.md")).unwrap(), original);
    assert!(
        !owned.join("manifest.toml").exists(),
        "undo removes the owned copy created by that Procedure"
    );
}

#[test]
fn typed_profile_bindings_and_project_forks_remain_live_lenses() {
    let home = fresh_home();
    let personal = home.home.registry("personal");
    for name in ["cargo-test", "cargo-nextest", "project-extra"] {
        let capsule = personal.join(format!("capsules/script/acceptance/{name}"));
        fs::create_dir_all(capsule.join("payload")).unwrap();
        fs::write(
            capsule.join("manifest.toml"),
            format!(
                "schema = 1\nid = \"script/acceptance/{name}\"\nkind = \"script\"\n\
                 name = \"{name}\"\ndescription = \"Acceptance script.\"\n\n\
                 [script]\nentry = \"payload/run.sh\"\n"
            ),
        )
        .unwrap();
        fs::write(capsule.join("payload/run.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    }
    let base = personal.join("profiles/acceptance/rust.toml");
    fs::create_dir_all(base.parent().unwrap()).unwrap();
    fs::write(
        &base,
        r#"schema = 1
id = "profile/acceptance/rust"
enable = ["script/acceptance/{{runner}}"]

[params.runner]
type = "enum"
choices = ["cargo-test", "cargo-nextest"]
default = "cargo-nextest"
"#,
    )
    .unwrap();

    let bound = project_repo();
    fs::write(
        bound.path().join(".aikit/profile.toml"),
        r#"schema = 1

[[use]]
profile = "profile/acceptance/rust"
params = { runner = "cargo-test" }
"#,
    )
    .unwrap();
    let (bound_out, bound_status) =
        run_json(&home, bound.path(), &[], &["status", "--json"]);
    expect_ok(&bound_out, &bound_status, "resolve a typed profile binding");
    let bound_ids: Vec<&str> = bound_status["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(bound_ids, vec!["script/acceptance/cargo-test"]);

    let forked = project_repo();
    let (preview_out, preview) = run_json(
        &home,
        forked.path(),
        &[],
        &[
            "profile",
            "fork",
            "profile/acceptance/rust",
            "--name",
            "profile/project/acceptance-rust",
            "--scope",
            "project",
            "--json",
        ],
    );
    expect_ok(&preview_out, &preview, "preview a project fork");
    let digest = preview["data"]["review_digest"].as_str().unwrap();
    let (fork_out, fork) = run_json(
        &home,
        forked.path(),
        &[],
        &[
            "profile",
            "fork",
            "profile/acceptance/rust",
            "--name",
            "profile/project/acceptance-rust",
            "--scope",
            "project",
            "--yes",
            "--expect-digest",
            digest,
            "--json",
        ],
    );
    expect_ok(&fork_out, &fork, "create a project fork");
    let fork_path = forked
        .path()
        .join(".aikit/profiles/project/acceptance-rust.toml");
    let created = fs::read_to_string(&fork_path).unwrap();
    assert!(created.contains("extends = [\"profile/acceptance/rust\"]"));
    assert!(
        !created.contains("script/acceptance/cargo-nextest"),
        "the base is referenced, not copied"
    );

    fs::write(
        &fork_path,
        r#"schema = 1
id = "profile/project/acceptance-rust"
description = "Project-only check."
extends = ["profile/acceptance/rust"]
enable = ["script/acceptance/project-extra"]
"#,
    )
    .unwrap();
    // The fork is a live lens: changing the base after fork creation must flow
    // through without copying or regenerating the project delta.
    fs::write(
        &base,
        r#"schema = 1
id = "profile/acceptance/rust"
enable = ["script/acceptance/cargo-test"]
"#,
    )
    .unwrap();
    let (status_out, status) =
        run_json(&home, forked.path(), &[], &["status", "--json"]);
    expect_ok(&status_out, &status, "resolve an inherited project fork");
    let mut ids: Vec<&str> = status["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "script/acceptance/cargo-test",
            "script/acceptance/project-extra"
        ],
        "the live base default and the project delta both resolve"
    );
}

#[test]
fn the_interactive_tree_host_accepts_mouse_navigation_and_applies_staged_ids() {
    use aikit_tui::event::{PaletteEvent, ScriptedEvents};
    use aikit_tui::host::UiHost;
    use aikit_tui::tree::{Node, NodeKind, Root, TreeState};
    use aikit_tui::tree_driver::{event_loop, TreeOutcome, TreeRequest};
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let capability = CapsuleId::parse("skill/acceptance/tree").unwrap();
    let state = TreeState::new(vec![Node::branch(
        NodeKind::Root(Root::Kinds),
        "one capability",
        vec![Node::leaf(
            NodeKind::Capability {
                id: capability.clone(),
            },
            "interactive acceptance",
        )],
    )]);
    let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
    ]);

    let outcome = event_loop(
        &mut terminal,
        &mut events,
        state,
        TreeRequest::new(UiHost::Fullscreen),
    )
    .unwrap();

    assert_eq!(outcome, TreeOutcome::Apply(vec![capability]));
}

#[cfg(target_os = "macos")]
#[test]
fn the_real_binary_completes_palette_tree_stage_palette_apply_in_one_lifecycle() {
    let home = fresh_home();
    let project = project_repo();
    let mut child = Command::new("/usr/bin/script")
        .args([
            "-q",
            "/dev/null",
            "/bin/sh",
            "-c",
            "stty rows 24 cols 100; exec \"$@\"",
            "sh",
            "env",
        ])
        .arg(format!("AIKIT_HOME={}", home.path.display()))
        .arg(format!("HOME={}", home.path.display()))
        .arg("TERM=xterm-256color")
        .arg(cargo_bin("aikit"))
        .args(["ui", "--fullscreen"])
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the real unified surface starts under a pseudo-terminal");

    let stdout_reader = wait_for_pty_ui(&mut child);
    let mut input = child.stdin.take().unwrap();
    // Palette -> tree; kinds/ -> first kind -> first capability; stage; tree
    // -> palette; review the exact staged diff; apply it.
    for bytes in [
        b"\x14".as_slice(),
        b"j",
        b"l",
        b"j",
        b"l",
        b"j",
        b" ",
        b"\x14",
        b"\x05", // Ctrl-E opens the staged diff without applying.
        b"\r",   // Enter applies only from that review screen.
    ] {
        input.write_all(bytes).unwrap();
        input.flush().unwrap();
        std::thread::sleep(Duration::from_millis(75));
    }
    std::thread::sleep(Duration::from_millis(250));
    input.write_all(b"\x1b").unwrap(); // Successful apply returns to resting palette.
    drop(input);

    let (status, stdout, stderr) = finish_pty(child, stdout_reader);
    assert!(
        status.success(),
        "unified surface failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let frame_stream = String::from_utf8_lossy(&stdout);
    assert!(
        frame_stream.contains("AIKit palette") && frame_stream.contains("Ctrl-T palette"),
        "the initial palette and the tree's palette-switch control must both be rendered by the same invocation: {frame_stream:?}"
    );
    assert_eq!(
        frame_stream.matches("\u{1b}[?1049h").count(),
        1,
        "Ctrl-T must not tear down and recreate the alternate screen: {frame_stream:?}"
    );
    assert_eq!(
        frame_stream.matches("\u{1b}[?1049l").count(),
        1,
        "the one alternate screen must be restored exactly once: {frame_stream:?}"
    );

    let authored = fs::read_to_string(project.path().join(".aikit/profile.local.toml"))
        .unwrap_or_else(|error| {
            panic!(
                "the one real process did not apply its staged capability ({error}); terminal output={frame_stream:?}"
            )
        });
    let document: toml::Value = toml::from_str(&authored).unwrap();
    let enabled = document["enable"]
        .as_array()
        .and_then(|values| values.first())
        .and_then(toml::Value::as_str);
    assert_eq!(
        enabled,
        Some("skill/rust/rust-review"),
        "the complete cross-mode journey must apply its deterministic target: {authored}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_real_binary_tree_applies_a_keyboard_staged_capability_through_a_pty() {
    let home = fresh_home();
    let project = project_repo();
    let mut child = Command::new("/usr/bin/script")
        .args(["-q", "/dev/null"])
        .arg("env")
        .arg(format!("AIKIT_HOME={}", home.path.display()))
        .arg(format!("HOME={}", home.path.display()))
        .arg("TERM=xterm-256color")
        .arg(cargo_bin("aikit"))
        .args(["ui", "--tree", "--fullscreen"])
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the real binary starts under a pseudo-terminal");

    // Synchronise on mouse capture: it is emitted only after raw mode is active.
    // `S` remains the printable apply fallback for PTY bridges that reserve
    // Ctrl-S as terminal flow control.
    let stdout_reader = wait_for_pty_ui(&mut child);
    // kinds/ → first kind → first capability; stage, apply.
    let mut input = child.stdin.take().unwrap();
    for key in [b"j".as_slice(), b"l", b"j", b"l", b"j", b" ", b"S"] {
        input.write_all(key).unwrap();
        input.flush().unwrap();
        std::thread::sleep(Duration::from_millis(75));
    }
    // Apply now keeps the unified popup open on its palette result. Two Escapes
    // also close deterministically if navigation failed and left us in the tree.
    std::thread::sleep(Duration::from_millis(250));
    let _ = input.write_all(b"\x1b");
    std::thread::sleep(Duration::from_millis(75));
    let _ = input.write_all(b"\x1b");
    drop(input);
    let (pty_status, pty_stdout, pty_stderr) = finish_pty(child, stdout_reader);
    assert!(
        pty_status.success(),
        "PTY driver failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&pty_stdout),
        String::from_utf8_lossy(&pty_stderr)
    );

    let local = project.path().join(".aikit/profile.local.toml");
    let authored = fs::read_to_string(&local).unwrap_or_else(|error| {
        panic!(
            "the real event loop did not apply its staged capability ({error}); terminal output={:?}",
            String::from_utf8_lossy(&pty_stdout)
        )
    });
    assert!(
        authored.contains("enable"),
        "the PTY interaction must produce a real project-local declaration: {authored}"
    );
    let document: toml::Value = toml::from_str(&authored).unwrap();
    let enabled = document["enable"]
        .as_array()
        .and_then(|values| values.first())
        .and_then(toml::Value::as_str)
        .expect("the PTY-authored profile names the staged capability");
    assert_eq!(
        enabled, "skill/rust/rust-review",
        "the paced keys have one deterministic target; accepting whichever id appeared would not test navigation"
    );

    let (status_out, status) =
        run_json(&home, project.path(), &[], &["status", "--all", "--json"]);
    expect_ok(&status_out, &status, "resolve the PTY-authored declaration");
    assert!(
        status["data"]["active"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == enabled)
            || status["data"]["unavailable"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == enabled),
        "the resolver sees the exact PTY-authored capability even when its trust policy keeps it unavailable: {status}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_real_binary_tree_applies_an_exact_mouse_staged_capability_through_a_pty() {
    let home = fresh_home();
    let project = project_repo();
    let mut child = Command::new("/usr/bin/script")
        .args([
            "-q",
            "/dev/null",
            "/bin/sh",
            "-c",
            "stty rows 24 cols 80; exec \"$@\"",
            "sh",
            "env",
        ])
        .arg(format!("AIKIT_HOME={}", home.path.display()))
        .arg(format!("HOME={}", home.path.display()))
        .arg("TERM=xterm-256color")
        .arg(cargo_bin("aikit"))
        .args(["ui", "--tree", "--fullscreen"])
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the real binary starts under a sized pseudo-terminal");

    let stdout_reader = wait_for_pty_ui(&mut child);
    let mut input = child.stdin.take().unwrap();
    // SGR mouse coordinates are one-based. Expand kinds/, expand its first
    // (Skill) group, click rust-review's rendered checkbox, then click [apply].
    for event in [
        b"\x1b[<0;2;3M\x1b[<0;2;3m".as_slice(),
        b"\x1b[<0;4;4M\x1b[<0;4;4m",
        b"\x1b[<0;8;5M\x1b[<0;8;5m",
        b"\x1b[<0;5;22M\x1b[<0;5;22m",
    ] {
        input.write_all(event).unwrap();
        input.flush().unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(250));
    let _ = input.write_all(b"\x1b");
    std::thread::sleep(Duration::from_millis(75));
    let _ = input.write_all(b"\x1b");
    drop(input);
    let (status, stdout, stderr) = finish_pty(child, stdout_reader);
    assert!(
        status.success(),
        "mouse PTY driver failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    let authored = fs::read_to_string(project.path().join(".aikit/profile.local.toml"))
        .unwrap_or_else(|error| {
            panic!(
                "the real mouse event loop did not apply its staged capability ({error}); terminal output={:?}",
                String::from_utf8_lossy(&stdout)
            )
        });
    let document: toml::Value = toml::from_str(&authored).unwrap();
    let enabled = document["enable"]
        .as_array()
        .and_then(|values| values.first())
        .and_then(toml::Value::as_str);
    assert_eq!(
        enabled,
        Some("skill/rust/rust-review"),
        "the mouse coordinates must stage the exact rendered checkbox: {authored}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_palette_run_outcome_executes_the_real_script_after_the_terminal_is_restored() {
    let home = fresh_home();
    let project = project_repo();
    let capsule = home
        .home
        .registry("personal")
        .join("capsules/script/acceptance/palette-run");
    fs::create_dir_all(capsule.join("payload")).unwrap();
    fs::write(
        capsule.join("manifest.toml"),
        "schema = 1\nid = \"script/acceptance/palette-run\"\nkind = \"script\"\n\
         name = \"palette-run\"\ndescription = \"Prove the palette executes its returned run.\"\n\n\
         [script]\nentry = \"payload/run.sh\"\n",
    )
    .unwrap();
    fs::write(
        capsule.join("payload/run.sh"),
        "#!/bin/sh\nprintf 'ran\\n' > .palette-ran\n",
    )
    .unwrap();
    make_executable(&capsule.join("payload/run.sh"));
    let index = Index::open(&home.home.database()).unwrap();
    let source = RegistrySource::personal();
    let load = load_registry(&home.home.registry("personal"), source.clone()).unwrap();
    let id = CapsuleId::parse("script/acceptance/palette-run").unwrap();
    let revision = load
        .catalog
        .get(&id)
        .and_then(|capsule| capsule.revision.clone())
        .expect("the real registry loader computes the script revision");
    TrustStore::new(&index)
        .record(
            &TrustKey::new(source, id, revision),
            TrustState::Trusted,
            None,
        )
        .unwrap();
    drop(index);

    let mut child = Command::new("/usr/bin/script")
        .args([
            "-q",
            "/dev/null",
            "/bin/sh",
            "-c",
            "stty rows 24 cols 80; exec \"$@\"",
            "sh",
            "env",
        ])
        .arg(format!("AIKIT_HOME={}", home.path.display()))
        .arg(format!("HOME={}", home.path.display()))
        .arg("TERM=xterm-256color")
        .arg(cargo_bin("aikit"))
        .args(["ui", "--tree", "--fullscreen"])
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the palette starts under a real pseudo-terminal");

    let stdout_reader = wait_for_pty_ui(&mut child);
    let mut input = child.stdin.take().unwrap();
    // kinds/ -> Script -> the first (acceptance) script -> activate. The tree
    // hands the exact id to the palette, which must open its natural action and
    // then the CLI must execute the returned Run outcome.
    for key in [b"j".as_slice(), b"l", b"j", b"j", b"l", b"j", b"\r"] {
        input.write_all(key).unwrap();
        input.flush().unwrap();
        std::thread::sleep(Duration::from_millis(75));
    }
    std::thread::sleep(Duration::from_millis(250));
    let _ = input.write_all(b"q");
    drop(input);
    let (status, stdout, stderr) = finish_pty(child, stdout_reader);
    assert!(
        status.success(),
        "palette PTY driver failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let marker = fs::read_to_string(project.path().join(".palette-ran")).unwrap_or_else(|error| {
        panic!(
            "returning PaletteOutcome::Run is not enough; the CLI must execute it ({error}); terminal output={:?}",
            String::from_utf8_lossy(&stdout)
        )
    });
    assert_eq!(marker, "ran\n");
}
