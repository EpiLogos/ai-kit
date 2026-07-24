//! The tmux adapter against a real tmux server.
//!
//! Every test here starts its own server on a **private socket** (`tmux -L
//! aikit-test-<unique>`) and kills it from a drop guard, so a panicking test
//! cannot leave a server — or a `sleep` it started — behind, and nothing here can
//! reach the user's own session.
//!
//! When tmux is not installed the tests print a skip line and return. They do not
//! fail: an adapter test that turns "no tmux on this machine" into a red build
//! teaches people to ignore red builds.

mod common;

use common::*;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use aikit_adapters::mux::tmux::{SessionIdentity, Tmux, PANE_TAG, PROFILE_OPTION, SESSION_OPTION};
use aikit_adapters::mux::{MuxAdapter, MuxTarget, ReconcileMode, SpawnRequest};
use aikit_adapters::runner::SystemRunner;
use aikit_core::context::Isolation;
use aikit_core::id::{ContextId, SessionId};
use aikit_core::platform::MuxKind;
use aikit_core::session::Placement;

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

fn tmux_installed() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Skip loudly rather than failing when tmux is absent.
macro_rules! require_tmux {
    ($name:literal) => {
        if !tmux_installed() {
            eprintln!(
                "SKIP {}: tmux is not installed on this machine, so the real-server \
                 behaviour was not exercised",
                $name
            );
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// A private server, torn down even on panic
// ---------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct PrivateServer {
    adapter: Tmux<SystemRunner>,
    socket: String,
}

impl PrivateServer {
    fn start(identity: SessionIdentity) -> Self {
        let socket = format!(
            "aikit-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let adapter = Tmux::new(SystemRunner::new())
            .with_socket(&socket)
            .with_identity(identity);
        Self { adapter, socket }
    }

    fn tmux(&self) -> &Tmux<SystemRunner> {
        &self.adapter
    }

    /// Run a raw tmux command against this private server.
    fn raw(&self, args: &[&str]) -> String {
        let mut argv = vec!["tmux".to_string(), "-L".to_string(), self.socket.clone()];
        argv.extend(args.iter().map(|a| a.to_string()));
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap_or_else(|e| panic!("could not run {argv:?}: {e}"));
        assert!(
            out.status.success(),
            "{argv:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for PrivateServer {
    fn drop(&mut self) {
        // Runs during unwind too, which is the point: a failing test must not
        // leave a tmux server and a pile of `sleep`s running.
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
        // tmux leaves the socket inode behind after the server exits. The names
        // are unique per run, so a stale one is harmless — but a few dozen of
        // them per test run is litter in somebody's /tmp.
        if let Some(path) = socket_path(&self.socket) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Where tmux puts a named socket, which is `$TMUX_TMPDIR` or `/tmp` plus
/// `tmux-<uid>`.
fn socket_path(socket: &str) -> Option<PathBuf> {
    let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let uid = String::from_utf8(
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()?
            .stdout,
    )
    .ok()?;
    Some(PathBuf::from(base).join(format!("tmux-{}", uid.trim())).join(socket))
}

fn identity() -> SessionIdentity {
    SessionIdentity {
        session_id: Some(SessionId::parse("ses_REALTMUX00000000000000").unwrap()),
        context_id: Some(ContextId::parse("ctx_REALTMUX00000000000000").unwrap()),
        project_root: Some(PathBuf::from("/work/payments")),
        view_root: None,
        profile: Some("rust-review".into()),
        isolation: Isolation::Shared,
    }
}

/// Wait for a file to contain something, because a pane's command runs
/// asynchronously once tmux has forked it.
fn wait_for_file(path: &Path) -> String {
    for _ in 0..200 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                return contents;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{} never got any content", path.display());
}

/// A plan whose two panes each append a line to a file and then stay alive, so a
/// second `ensure_session` re-running them would be visible as a second line.
fn ticking_plan(name: &str, dir: &Path) -> aikit_core::session::SessionPlan {
    plan_from(&format!(
        r#"
schema = 1
id = "{name}"
name = "{name}"
root = "{dir}"

[[views]]
id = "main"
[[views.panes]]
id = "top"
focus = true
command = ["sh", "-c", "echo tick >> {dir}/top.log; sleep 300"]
[[views.panes]]
id = "bottom"
split_from = "top"
direction = "down"
ratio = 0.3
command = ["sh", "-c", "echo tick >> {dir}/bottom.log; sleep 300"]
"#,
        dir = dir.display()
    ))
}

// ---------------------------------------------------------------------------
// Creating
// ---------------------------------------------------------------------------

#[test]
fn a_session_a_window_and_split_panes_are_really_created() {
    require_tmux!("a_session_a_window_and_split_panes_are_really_created");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    let plan = three_pane_plan("payments", dir.path());

    let binding = server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    assert!(binding.created);
    assert_eq!(binding.kind, Some(MuxKind::Tmux));
    assert!(server.tmux().has_session("payments").unwrap());

    assert_eq!(
        server.raw(&["list-windows", "-t", "payments", "-F", "#{window_name}"]),
        "code\nops"
    );
    assert_eq!(
        server
            .raw(&["list-panes", "-t", "payments:code", "-F", "#{pane_id}"])
            .lines()
            .count(),
        3,
        "the view declares a root plus two splits"
    );

    // The binding is the map the rest of AIKit navigates by; a wrong entry here
    // sends the next command to the wrong pane.
    let editor = binding.surface_of("code", "editor").unwrap();
    assert_eq!(
        server.raw(&["display-message", "-p", "-t", editor, "#{pane_id}"]),
        editor
    );
    assert!(binding.surface_of("ops", "watch").is_some());
}

#[test]
fn a_split_ratio_really_changes_the_pane_geometry() {
    require_tmux!("a_split_ratio_really_changes_the_pane_geometry");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();

    let plan = plan_from(&format!(
        r#"
schema = 1
id = "ratios"
name = "ratios"
root = "{}"

[[views]]
id = "v"
[[views.panes]]
id = "left"
[[views.panes]]
id = "right"
split_from = "left"
direction = "right"
ratio = 0.25
"#,
        dir.path().display()
    ));
    server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    // Measured against the window tmux actually gave us, rather than a size this
    // test assumes: a detached session's dimensions are a server default.
    let window: u32 = server
        .raw(&["display-message", "-p", "-t", "ratios:v", "#{window_width}"])
        .parse()
        .unwrap();
    let widths: Vec<u32> = server
        .raw(&["list-panes", "-t", "ratios:v", "-F", "#{pane_width}"])
        .lines()
        .map(|l| l.parse().unwrap())
        .collect();

    assert_eq!(widths.len(), 2);
    let expected = window / 4;
    let right = widths[1];
    assert!(
        right.abs_diff(expected) <= 2,
        "a ratio of 0.25 in a {window}-column window should give about {expected} columns, \
         got {right} (widths {widths:?}); a bare `-l 25` would give 25 columns instead"
    );
}

// ---------------------------------------------------------------------------
// Environment and user options
// ---------------------------------------------------------------------------

#[test]
fn a_pane_really_inherits_the_aikit_context_environment() {
    require_tmux!("a_pane_really_inherits_the_aikit_context_environment");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("env.txt");

    let plan = plan_from(&format!(
        r#"
schema = 1
id = "envtest"
name = "envtest"
root = "{dir}"

[[views]]
id = "main"
[[views.panes]]
id = "probe"
command = ["sh", "-c", "printf '%s|%s|%s' \"$AIKIT_SESSION_ID\" \"$AIKIT_CONTEXT_ID\" \"$AIKIT_PROJECT_ROOT\" > {marker}; sleep 300"]
"#,
        dir = dir.path().display(),
        marker = marker.display()
    ));
    server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    // The real proof: not "set-environment was issued", but "a process started
    // inside the pane could read the values".
    assert_eq!(
        wait_for_file(&marker),
        "ses_REALTMUX00000000000000|ctx_REALTMUX00000000000000|/work/payments"
    );
}

#[test]
fn the_aikit_user_options_are_written_and_can_be_read_back() {
    require_tmux!("the_aikit_user_options_are_written_and_can_be_read_back");
    let server = PrivateServer::start(identity());
    server
        .tmux()
        .ensure_session(&single_pane_plan("opts"), ReconcileMode::default())
        .unwrap();

    assert_eq!(
        server
            .tmux()
            .session_option("opts", SESSION_OPTION)
            .unwrap()
            .as_deref(),
        Some("ses_REALTMUX00000000000000")
    );
    assert_eq!(
        server
            .tmux()
            .session_option("opts", PROFILE_OPTION)
            .unwrap()
            .as_deref(),
        Some("rust-review")
    );
    assert_eq!(
        server.tmux().session_option("opts", "@not_set").unwrap(),
        None,
        "an unset option is absent, not an empty string"
    );

    let pane = server.raw(&["list-panes", "-t", "opts:main", "-F", "#{pane_id}"]);
    assert_eq!(
        server.tmux().pane_option(&pane, PANE_TAG).unwrap().as_deref(),
        Some("main/shell"),
        "the pane tag is what makes a later reconcile non-destructive"
    );
}

// ---------------------------------------------------------------------------
// Idempotency — the property the whole design exists for
// ---------------------------------------------------------------------------

#[test]
fn ensure_session_twice_does_not_rerun_startup_commands_in_healthy_panes() {
    require_tmux!("ensure_session_twice_does_not_rerun_startup_commands_in_healthy_panes");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    let plan = ticking_plan("idem", dir.path());

    let first = server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();
    assert!(first.created);
    wait_for_file(&dir.path().join("top.log"));
    wait_for_file(&dir.path().join("bottom.log"));

    let second = server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();
    assert!(!second.created, "the session already existed");
    assert!(
        second.actions.is_empty(),
        "a second run of an unchanged plan should change nothing, but did: {:?}",
        second.actions
    );
    assert!(
        second.preserved.iter().any(|p| p.contains("main/top")),
        "the binding must say what it left alone: {:?}",
        second.preserved
    );

    // Give a mistakenly re-run command time to append a second line.
    std::thread::sleep(std::time::Duration::from_millis(400));
    for log in ["top.log", "bottom.log"] {
        let contents = std::fs::read_to_string(dir.path().join(log)).unwrap();
        assert_eq!(
            contents.lines().count(),
            1,
            "{log} was written twice, so a live pane was respawned: {contents:?}"
        );
    }
    assert_eq!(
        server
            .raw(&["list-panes", "-t", "idem:main", "-F", "#{pane_id}"])
            .lines()
            .count(),
        2,
        "a second run must not duplicate panes either"
    );
}

#[test]
fn a_pane_the_user_split_off_by_hand_survives_create_or_attach() {
    require_tmux!("a_pane_the_user_split_off_by_hand_survives_create_or_attach");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    let plan = ticking_plan("handmade", dir.path());

    server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    // The user splits a pane themselves. It carries no AIKit tag.
    let hand = server.raw(&[
        "split-window",
        "-t",
        "handmade:main",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep 300",
    ]);
    assert_eq!(
        server
            .raw(&["list-panes", "-t", "handmade:main", "-F", "#{pane_id}"])
            .lines()
            .count(),
        3
    );

    let binding = server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    let panes = server.raw(&["list-panes", "-t", "handmade:main", "-F", "#{pane_id}"]);
    assert!(
        panes.lines().any(|p| p == hand),
        "the user's own pane was closed by a non-destructive reconcile: {panes:?}"
    );
    assert!(
        binding
            .preserved
            .iter()
            .any(|p| p.contains(&hand) && p.contains("by hand")),
        "the binding must say the pane was left alone and why: {:?}",
        binding.preserved
    );
}

#[test]
fn exact_reconcile_closes_what_the_plan_does_not_declare_and_keeps_what_it_does() {
    require_tmux!("exact_reconcile_closes_what_the_plan_does_not_declare_and_keeps_what_it_does");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    let plan = ticking_plan("exact", dir.path());

    server
        .tmux()
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();
    wait_for_file(&dir.path().join("top.log"));
    let hand = server.raw(&[
        "split-window",
        "-t",
        "exact:main",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep 300",
    ]);

    let binding = server
        .tmux()
        .ensure_session(&plan, ReconcileMode::Exact)
        .unwrap();

    let panes = server.raw(&["list-panes", "-t", "exact:main", "-F", "#{pane_id}"]);
    assert!(
        !panes.lines().any(|p| p == hand),
        "an exact reconcile was asked for and did not remove the extra pane: {panes:?}"
    );
    assert_eq!(panes.lines().count(), 2);
    assert!(
        binding.actions.iter().any(|a| a.contains(&hand)),
        "a destructive action has to be reported: {:?}",
        binding.actions
    );

    // The planned panes were kept, not rebuilt: their commands did not re-run.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let contents = std::fs::read_to_string(dir.path().join("top.log")).unwrap();
    assert_eq!(
        contents.lines().count(),
        1,
        "exact reconcile must still leave healthy planned panes alone: {contents:?}"
    );
}

#[test]
fn a_view_the_plan_gained_since_the_session_was_built_is_added_without_disturbing_the_rest() {
    require_tmux!("a_view_the_plan_gained_since_the_session_was_built_is_added");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();

    server
        .tmux()
        .ensure_session(&ticking_plan("growing", dir.path()), ReconcileMode::default())
        .unwrap();
    wait_for_file(&dir.path().join("top.log"));

    let grown = plan_from(&format!(
        r#"
schema = 1
id = "growing"
name = "growing"
root = "{dir}"

[[views]]
id = "main"
[[views.panes]]
id = "top"
focus = true
command = ["sh", "-c", "echo tick >> {dir}/top.log; sleep 300"]
[[views.panes]]
id = "bottom"
split_from = "top"
direction = "down"
ratio = 0.3
command = ["sh", "-c", "echo tick >> {dir}/bottom.log; sleep 300"]

[[views]]
id = "docs"
[[views.panes]]
id = "reader"
"#,
        dir = dir.path().display()
    ));

    let binding = server
        .tmux()
        .ensure_session(&grown, ReconcileMode::default())
        .unwrap();

    assert_eq!(
        server.raw(&["list-windows", "-t", "growing", "-F", "#{window_name}"]),
        "main\ndocs"
    );
    assert!(binding.surface_of("docs", "reader").is_some());
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("top.log"))
            .unwrap()
            .lines()
            .count(),
        1,
        "adding a view must not restart the existing one"
    );
}

// ---------------------------------------------------------------------------
// Drift detection
// ---------------------------------------------------------------------------

#[test]
fn the_layout_can_be_read_back_and_changes_when_the_user_moves_a_pane() {
    require_tmux!("the_layout_can_be_read_back_and_changes_when_the_user_moves_a_pane");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    server
        .tmux()
        .ensure_session(&three_pane_plan("drift", dir.path()), ReconcileMode::default())
        .unwrap();

    let before = server.tmux().layout_of("drift:code").unwrap();
    assert!(!before.is_empty());
    assert_eq!(
        server.tmux().layout_of("drift:code").unwrap(),
        before,
        "reading the layout twice with nothing in between must be stable"
    );

    server.raw(&["select-layout", "-t", "drift:code", "even-horizontal"]);
    assert_ne!(
        server.tmux().layout_of("drift:code").unwrap(),
        before,
        "a rearranged window has to be detectable as drift"
    );
}

// ---------------------------------------------------------------------------
// Detection, location, spawn, close
// ---------------------------------------------------------------------------

#[test]
fn detection_finds_the_real_tmux_and_notices_whether_a_server_is_up() {
    require_tmux!("detection_finds_the_real_tmux_and_notices_whether_a_server_is_up");
    let server = PrivateServer::start(identity());

    let before = server.tmux().detect().unwrap();
    assert!(before.installed);
    assert!(
        before.version.as_deref().unwrap_or("").starts_with('3'),
        "expected a 3.x version, got {:?}",
        before.version
    );
    assert!(!before.server_running, "nothing has started this server yet");
    assert!(!before.inside, "the test process is not inside this server");

    server
        .tmux()
        .ensure_session(&single_pane_plan("detect"), ReconcileMode::default())
        .unwrap();
    assert!(server.tmux().detect().unwrap().server_running);
}

#[test]
fn a_missing_tmux_binary_is_reported_as_absent_rather_than_as_an_error() {
    let adapter = Tmux::new(SystemRunner::new()).with_binary("/definitely/not/tmux");
    let presence = adapter.detect().unwrap();

    assert!(!presence.installed);
    assert!(!presence.is_usable());
    assert!(presence.describe().contains("not on PATH"));
}

#[test]
fn spawning_a_new_pane_and_closing_it_really_changes_the_session() {
    require_tmux!("spawning_a_new_pane_and_closing_it_really_changes_the_session");
    let server = PrivateServer::start(identity());
    let dir = tempfile::tempdir().unwrap();
    let binding = server
        .tmux()
        .ensure_session(&single_pane_plan("spawning"), ReconcileMode::default())
        .unwrap();
    let root = binding.surface_of("main", "shell").unwrap().to_string();

    let spawned = server
        .tmux()
        .spawn(
            SpawnRequest::new(
                Placement::NewPane,
                vec!["sh".into(), "-c".into(), "sleep 300".into()],
            )
            .in_dir(dir.path())
            .splitting(aikit_core::session::Direction::Right, Some(0.4))
            .from_target(MuxTarget::surface(MuxKind::Tmux, &root)),
        )
        .unwrap();

    assert!(spawned.created);
    let panes = server.raw(&["list-panes", "-t", "spawning:main", "-F", "#{pane_id}"]);
    assert_eq!(panes.lines().count(), 2);
    assert!(panes.lines().any(|p| Some(p) == spawned.target.surface.as_deref()));

    server.tmux().close(&spawned.target).unwrap();
    assert_eq!(
        server
            .raw(&["list-panes", "-t", "spawning:main", "-F", "#{pane_id}"])
            .lines()
            .count(),
        1
    );
}

#[test]
fn outside_tmux_the_current_location_is_empty_rather_than_invented() {
    let adapter = Tmux::new(SystemRunner::new()).with_socket("aikit-test-nowhere");
    let location = adapter.current_location().unwrap();

    assert_eq!(location.kind, MuxKind::Tmux);
    assert_eq!(location.session, None);
    assert_eq!(location.surface, None);
    assert!(!location.target().is_addressable());
}
