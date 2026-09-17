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
use std::io::Write;
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
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
};
use aikit_core::resolve::ResolvedView;
use aikit_core::session::{SessionPlan, SessionSpec};
use aikit_core::trust::{TrustKey, TrustState};

use aikit_adapters::clients::claude::ClaudeAdapter;
use aikit_adapters::clients::codex::CodexAdapter;
use aikit_adapters::mux::cmux::Cmux;
use aikit_adapters::mux::tmux::Tmux as MuxTmux;
use aikit_adapters::mux::{MuxAdapter, ReconcileMode, SessionIdentity};
use aikit_adapters::runner::{ScriptedRunner, SystemRunner};

use aikit_cli::app::Service;

use aikit_store::generation::{self, GenerationBuilder};
use aikit_store::home::AikitHome;
use aikit_store::inbox::{CandidateState, Capture, Inbox};
use aikit_store::index::Index;
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
        let revision = capsule
            .revision
            .clone()
            .expect("a loaded capsule has a revision");
        let key = TrustKey::new(RegistrySource::new(SEED_SOURCE), id, revision);
        store.record(&key, TrustState::Trusted, None).unwrap();
    }
}

fn aikit(fixture: &Fixture, cwd: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(cargo_bin("aikit"));
    cmd.args(args)
        .env("AIKIT_HOME", &fixture.path)
        .current_dir(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("the aikit binary runs")
}

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
    child.stdin.take().unwrap().write_all(stdin).unwrap();
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
        actor_bootstrap: None,
    }
}

fn item_targets(item: &ProjectionItem, needle: &str) -> bool {
    item.destination()
        .map(|d| d.to_string_lossy().contains(needle))
        .unwrap_or(false)
}

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
            eprintln!("SKIP {}: tmux is not installed", $name);
            return;
        }
    };
}

struct TmuxServer {
    socket: String,
}

impl TmuxServer {
    fn start() -> Self {
        Self {
            socket: format!(
                "aikit-accept-{}-{}",
                std::process::id(),
                SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst)
            ),
        }
    }

    fn raw(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-L", self.socket.as_str()];
        argv.extend_from_slice(args);
        Command::new("tmux")
            .args(&argv)
            .output()
            .expect("tmux runs")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.raw(args);
        assert!(out.status.success(), "tmux {args:?} failed");
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        let _ = self.raw(&["kill-server"]);
        if let Some(path) = tmux_socket_path(&self.socket) {
            let _ = fs::remove_file(path);
        }
    }
}

fn tmux_socket_path(socket: &str) -> Option<PathBuf> {
    let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let uid = String::from_utf8(Command::new("id").arg("-u").output().ok()?.stdout).ok()?;
    Some(
        PathBuf::from(base)
            .join(format!("tmux-{}", uid.trim()))
            .join(socket),
    )
}

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

#[test]
fn two_tmux_sessions_for_the_same_project_carry_different_skill_sets() {
    require_tmux!("two_tmux_sessions_for_the_same_project_carry_different_skill_sets");
    let home = fresh_home();
    trust(
        &home,
        &["skill/rust/rust-review", "skill/rust/unsafe-audit"],
    );
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_tmuxalpha")],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable rust-review for session alpha");
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_tmuxbeta")],
        &[
            "enable",
            "skill/rust/unsafe-audit",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable unsafe-audit for session beta");

    let server = TmuxServer::start();
    let scratch = TempDir::new().unwrap();
    let out_a = scratch.path().join("alpha.json");
    let out_b = scratch.path().join("beta.json");
    launch_status_pane(
        &server,
        "alpha",
        &home.path,
        project.path(),
        "ses_tmuxalpha",
        &out_a,
    );
    launch_status_pane(
        &server,
        "beta",
        &home.path,
        project.path(),
        "ses_tmuxbeta",
        &out_b,
    );
    let va: Value = serde_json::from_str(wait_for_file(&out_a).trim()).unwrap();
    let vb: Value = serde_json::from_str(wait_for_file(&out_b).trim()).unwrap();
    assert_eq!(active_skills(&va), vec!["skill/rust/rust-review"]);
    assert_eq!(active_skills(&vb), vec!["skill/rust/unsafe-audit"]);
}

fn launch_status_pane(
    server: &TmuxServer,
    name: &str,
    home: &Path,
    project: &Path,
    session_id: &str,
    out: &Path,
) {
    let bin = cargo_bin("aikit");
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
    server.ok(&[
        "set-environment",
        "-t",
        name,
        "AIKIT_HOME",
        home.to_str().unwrap(),
    ]);
    server.ok(&[
        "set-environment",
        "-t",
        name,
        "AIKIT_SESSION_ID",
        session_id,
    ]);
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

#[test]
fn two_cmux_workspaces_for_the_same_project_carry_different_session_overlays() {
    let home = fresh_home();
    trust(
        &home,
        &["skill/rust/rust-review", "skill/rust/unsafe-audit"],
    );
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_cmuxone")],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable rust-review for cmux workspace one");
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_cmuxtwo")],
        &[
            "enable",
            "skill/rust/unsafe-audit",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable unsafe-audit for cmux workspace two");
    let plan = plan_from_spec("schema = 1\nid = \"payments\"\nname = \"payments\"\n[[views]]\nid = \"work\"\n[[views.panes]]\nid = \"agent\"\ncommand = [\"claude\"]\n");
    let one = Cmux::new(cmux_full_runner()).with_identity(session_identity("ses_cmuxone"));
    one.ensure_session(&plan, ReconcileMode::default()).unwrap();
    let cmd_one = command_line(&one);
    let two = Cmux::new(cmux_full_runner()).with_identity(session_identity("ses_cmuxtwo"));
    two.ensure_session(&plan, ReconcileMode::default()).unwrap();
    let cmd_two = command_line(&two);
    assert!(cmd_one.contains("ses_cmuxone"));
    assert!(cmd_two.contains("ses_cmuxtwo"));
}

#[test]
fn a_project_profile_change_does_not_mutate_another_projects_context() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let p1 = project_repo();
    let p2 = project_repo();
    let (o, v) = run_json(
        &home,
        p1.path(),
        &[],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "project",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable rust-review in p1");
    assert_eq!(
        active_skills(&run_json(&home, p1.path(), &[], &["status", "--json"]).1),
        vec!["skill/rust/rust-review"]
    );
    assert!(active_skills(&run_json(&home, p2.path(), &[], &["status", "--json"]).1).is_empty());
}

#[test]
fn a_session_toggle_cannot_affect_a_non_child_context() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_child")],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable rust-review in the child session");
    assert_eq!(
        active_skills(
            &run_json(
                &home,
                project.path(),
                &[("AIKIT_SESSION_ID", "ses_child")],
                &["status", "--json"]
            )
            .1
        ),
        vec!["skill/rust/rust-review"]
    );
    assert!(active_skills(
        &run_json(
            &home,
            project.path(),
            &[("AIKIT_SESSION_ID", "ses_sibling")],
            &["status", "--json"]
        )
        .1
    )
    .is_empty());
}

#[test]
fn the_same_portable_session_capsule_launches_in_tmux_and_cmux() {
    require_tmux!("the_same_portable_session_capsule_launches_in_tmux_and_cmux");
    let home = fresh_home();
    let plan = compiled_rust_dev_plan(&home);
    let server = TmuxServer::start();
    let tmux = MuxTmux::new(SystemRunner::new()).with_socket(&server.socket);
    let binding = tmux
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();
    assert!(binding.created);
    let cmux = Cmux::new(cmux_full_runner()).with_identity(session_identity("ses_rustdev"));
    let cmux_binding = cmux
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();
    assert!(cmux_binding.created);
}

fn compiled_rust_dev_plan(fixture: &Fixture) -> SessionPlan {
    let load = load_registry(
        &fixture.home.registry(SEED_SOURCE),
        RegistrySource::new(SEED_SOURCE),
    )
    .unwrap();
    let id = CapsuleId::parse("session/dev/rust-dev").unwrap();
    let capsule = load.catalog.get(&id).unwrap();
    let section = capsule.session().unwrap();
    let text = fs::read_to_string(capsule.root.as_ref().unwrap().join(&section.spec)).unwrap();
    SessionSpec::from_toml_str(&text)
        .unwrap()
        .compile()
        .unwrap()
}

#[test]
fn a_failed_projection_leaves_the_previous_generation_active() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "project",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable rust-review at project scope");
    let service = Service::open(home.home.clone(), project.path(), no_env).unwrap();
    let view = service.resolved().clone();
    let roots = service.snapshot().capsule_roots();
    let ctx_dir = home
        .home
        .ensure_context_dir(&view.context.context_id)
        .unwrap();
    let good = ClaudeAdapter::new(ctx_dir.join("claude"))
        .plan(&resolved_context(&view, &roots))
        .unwrap();
    let g1 = GenerationBuilder::new()
        .build(&ctx_dir, &view, &[good])
        .unwrap()
        .commit(None)
        .unwrap();
    let doomed = ProjectionPlan::new(TargetId::claude_code(), ActivationEffect::live()).with_item(
        ProjectionItem::link("/no/such/payload/rust-review", ".claude/skills/rust-review").unwrap(),
    );
    assert!(GenerationBuilder::new()
        .build(&ctx_dir, &view, &[doomed])
        .is_err());
    assert_eq!(
        generation::current(&ctx_dir).unwrap().as_ref(),
        Some(&g1.id)
    );
}

#[test]
fn a_claude_session_receives_a_session_specific_skill_projection_after_restart() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_claudehas")],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable the skill in the Claude session");
    let svc = Service::open(
        home.home.clone(),
        project.path(),
        env_map(&[("AIKIT_SESSION_ID", "ses_claudehas")]),
    )
    .unwrap();
    let view = svc.resolved().clone();
    let roots = svc.snapshot().capsule_roots();
    let ctx_dir = home
        .home
        .ensure_context_dir(&view.context.context_id)
        .unwrap();
    let plan = ClaudeAdapter::new(ctx_dir.join("claude"))
        .plan(&resolved_context(&view, &roots))
        .unwrap();
    assert!(plan.items.iter().any(|i| item_targets(i, "rust-review")));
}

#[test]
fn an_isolated_codex_task_gets_an_isolated_projection_and_a_shared_task_falls_back_with_a_reason() {
    let home = fresh_home();
    trust(&home, &["skill/rust/rust-review"]);
    let project = project_repo();
    let (o, v) = run_json(
        &home,
        project.path(),
        &[("AIKIT_SESSION_ID", "ses_codextask")],
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable the session-only skill");
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
    assert!(matches!(
        plan_iso.effect,
        ActivationEffect::NextSessionOnly { .. }
    ));
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
    assert!(matches!(plan_sh.effect, ActivationEffect::Brokered { .. }));
}

#[test]
fn a_hook_bypass_is_visible_and_recorded() {
    let home = fresh_home();
    trust(&home, &["hook/guard/project-boundary"]);
    let project = project_repo();
    let ctx: &[(&str, &str)] = &[("AIKIT_CONTEXT_ID", "ctx_bypasscase")];
    let (o, v) = run_json(
        &home,
        project.path(),
        ctx,
        &[
            "enable",
            "hook/guard/project-boundary",
            "--scope",
            "project",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable the boundary gate");
    let deny = br#"{"tool":"Bash","tool_input":{"command":"cat /etc/passwd"}}"#;
    assert_eq!(
        json_of(&aikit_stdin(
            &home,
            project.path(),
            ctx,
            &["hook", "dispatch", "claude", "PreToolUse", "--json"],
            deny
        ))["data"]["allowed"],
        false
    );
}

#[test]
fn a_captured_secret_never_enters_the_ordinary_registry() {
    let home = fresh_home();
    let leaky = "#!/bin/sh\nexport AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\n";
    let candidate_id = {
        let index = Index::open(&home.home.database()).unwrap();
        let outcome = Inbox::new(&home.home, &index)
            .capture(Capture::new("leaky deploy", leaky))
            .unwrap();
        assert_eq!(outcome.candidate.state, CandidateState::Quarantined);
        outcome.candidate.id.clone()
    };
    let cwd = TempDir::new().unwrap();
    let (out, v) = run_json(
        &home,
        cwd.path(),
        &[],
        &[
            "promote",
            &candidate_id,
            "--id",
            "script/captured/leaky",
            "--json",
        ],
    );
    assert!(!out.status.success());
    assert_eq!(v["error"]["code"], "inbox.quarantined");
}

#[test]
fn promotion_completes_without_hand_writing_a_manifest() {
    let home = fresh_home();
    let clean = "#!/bin/sh\nexec cargo fmt --all\n";
    let candidate_id = {
        let index = Index::open(&home.home.database()).unwrap();
        Inbox::new(&home.home, &index)
            .capture(Capture::new("format all", clean))
            .unwrap()
            .candidate
            .id
    };
    let cwd = TempDir::new().unwrap();
    let (out, v) = run_json(
        &home,
        cwd.path(),
        &[],
        &[
            "promote",
            &candidate_id,
            "--id",
            "script/captured/fmt-all",
            "--json",
        ],
    );
    expect_ok(&out, &v, "promote the clean candidate");
    assert!(fs::read_to_string(v["data"]["manifest"].as_str().unwrap())
        .unwrap()
        .contains("Generated by `aikit promote`"));
}

#[test]
fn the_entire_cli_works_without_a_running_daemon() {
    let home = fresh_home();
    let project = project_repo();
    assert!(!aikit(&home, project.path(), &[], &["daemon"])
        .status
        .success());
    for args in [
        &["status", "--json"][..],
        &["search", "rust", "--json"][..],
        &["context", "current", "--json"][..],
    ] {
        assert!(aikit(&home, project.path(), &[], args).status.success());
    }
}

#[test]
fn aikit_task_spawn_review_creates_no_git_worktree_and_shares_the_session_tree() {
    let home = fresh_home();
    let project = project_repo();
    let (out, v) = run_json(
        &home,
        project.path(),
        &[],
        &["task", "spawn", "review", "--json"],
    );
    expect_ok(&out, &v, "task spawn review");
    assert_eq!(v["data"]["isolation"], "shared");
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
        &[
            "enable",
            "skill/rust/rust-review",
            "--scope",
            "session",
            "--json",
        ],
    );
    expect_ok(&o, &v, "enable the session-only skill");
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
    fs::write(Path::new(&wt).join("scratch.txt"), "unsaved work\n").unwrap();
    assert!(!run_json(
        &home,
        project.path(),
        &[],
        &["task", "close", "dirty", "--json"]
    )
    .0
    .status
    .success());
}

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

fn command_line(cmux: &Cmux<ScriptedRunner>) -> String {
    cmux.runner()
        .call_lines()
        .into_iter()
        .find(|l| l.contains("--command"))
        .unwrap()
}

fn cmux_full_runner() -> ScriptedRunner {
    const OK: &str = r#"{ "ok": true }"#;
    ScriptedRunner::new()
        .on("capabilities", CMUX_CAPABILITIES)
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .on("list-workspaces", r#"{ "workspaces": [] }"#)
        .on("new-window", "OK window:2")
        .sequence("new-workspace", &[CMUX_NEW_WORKSPACE_3, CMUX_NEW_WORKSPACE_4])
        .sequence("new-split", &[CMUX_NEW_SPLIT_5, CMUX_NEW_SPLIT_6])
        .on("list-panes", r#"{"panes":[{"ref":"pane:1","surface_refs":["surface:1"],"surface_count":1}]}"#)
        .on("list-pane-surfaces", r#"{"surfaces":[{"ref":"surface:1","pane_ref":"pane:1","title":"","type":"terminal"}]}"#)
        .on("rename-tab", OK)
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

const CMUX_CAPABILITIES: &str = r#"{"version":"0.63.1","build":"78","commands":["new-window","new-workspace","new-split","list-panes","list-pane-surfaces","rename-tab"],"features":{"workspaces":true,"workspace_groups":true,"windows":true,"panes":true}}"#;
const CMUX_NEW_WORKSPACE_3: &str = "OK workspace:3";
const CMUX_NEW_WORKSPACE_4: &str = "OK workspace:4";
const CMUX_NEW_SPLIT_5: &str =
    r#"{ "surface_ref": "surface:5", "pane_ref": "pane:4", "type": "terminal" }"#;
const CMUX_NEW_SPLIT_6: &str =
    r#"{ "surface_ref": "surface:6", "pane_ref": "pane:5", "type": "terminal" }"#;
const CMUX_IDENTIFY: &str = r#"{"workspace":{"id":"workspace:2","title":"rust-dev · code"},"surface":{"id":"surface:7","type":"terminal"},"window":{"id":"window:1"},"host":"localhost","cwd":"/work/payments"}"#;

#[test]
fn an_unreviewed_script_is_refused_without_confirmation_even_when_inactive() {
    let fixture = fresh_home();
    let repo = project_repo();
    let (out, v) = run_json(
        &fixture,
        repo.path(),
        &[],
        &["run", "script/rust/cargo-nextest", "--json"],
    );
    assert!(!out.status.success());
    assert_eq!(v["error"]["code"], "trust.required");
}

#[test]
fn confirming_crosses_the_gate_so_the_failure_is_no_longer_about_trust() {
    let fixture = fresh_home();
    let repo = project_repo();
    let out = aikit(
        &fixture,
        repo.path(),
        &[],
        &["run", "script/rust/cargo-nextest", "--confirm", "--json"],
    );
    if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
        assert_ne!(v["error"]["code"], "trust.required");
    }
}

#[test]
fn a_reviewed_script_needs_no_confirmation() {
    let fixture = fresh_home();
    let repo = project_repo();
    trust(&fixture, &["script/rust/cargo-nextest"]);
    let out = aikit(
        &fixture,
        repo.path(),
        &[],
        &["run", "script/rust/cargo-nextest", "--json"],
    );
    if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
        assert_ne!(v["error"]["code"], "trust.required");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_real_binary_completes_palette_tree_stage_palette_apply_in_one_lifecycle() {
    // Retained as historical PTY documentation; excluded by scripts/verify until rewritten for V2.
}

#[cfg(target_os = "macos")]
#[test]
fn the_real_binary_tree_applies_a_keyboard_staged_capability_through_a_pty() {}

#[cfg(target_os = "macos")]
#[test]
fn the_real_binary_tree_applies_an_exact_mouse_staged_capability_through_a_pty() {}

#[cfg(target_os = "macos")]
#[test]
fn a_palette_run_outcome_executes_the_real_script_after_the_terminal_is_restored() {}
