//! Exactly what the tmux adapter says to tmux.
//!
//! These are the assertions a live-server test cannot make: a real tmux will
//! happily accept `-l 30` where `-l 30%` was meant, and the pane will merely be
//! the wrong size. Pinning the argv is what stops a flag regression from becoming
//! "the layout looks a bit odd sometimes".
//!
//! Every command asserted here is also issued against a real tmux server in
//! `tmux_real.rs`, so these are not assertions about an imagined binary.

mod common;

use common::*;

use aikit_adapters::mux::tmux::{SessionIdentity, Tmux};
use aikit_adapters::mux::{
    MuxAdapter, MuxTarget, Notification, PaletteRequest, ReconcileMode, SpawnRequest, StatusUpdate,
    UiHost,
};
use aikit_adapters::runner::ScriptedRunner;
use aikit_core::context::Isolation;
use aikit_core::id::{ContextId, SessionId};
use aikit_core::platform::MuxKind;
use aikit_core::session::{Direction, Placement};

fn identity() -> SessionIdentity {
    SessionIdentity {
        session_id: Some(SessionId::parse("ses_TESTSESSION000000000000").unwrap()),
        context_id: Some(ContextId::parse("ctx_TESTCONTEXT000000000000").unwrap()),
        project_root: Some("/work/payments".into()),
        view_root: Some("/home/u/.aikit/state/contexts/ctx_x/current".into()),
        profile: Some("rust-review".into()),
        isolation: Isolation::Shared,
    }
}

/// Enough recorded tmux responses to build a fresh session.
fn creating_runner() -> ScriptedRunner {
    ScriptedRunner::new()
        .failing("has-session", 1, "can't find session")
        .on("new-session", "%0")
        .sequence("split-window", &["%1", "%2", "%3"])
        .sequence("new-window", &["%10", "%11"])
        .on("set-environment", "")
        .on("set-option", "")
        .on("respawn-pane", "")
        .on("select-pane", "")
        .on("select-window", "")
}

fn tmux(runner: ScriptedRunner) -> Tmux<ScriptedRunner> {
    Tmux::new(runner)
        .with_socket("aikit-argv")
        .with_identity(identity())
}

// ---------------------------------------------------------------------------
// The private socket is on every single command
// ---------------------------------------------------------------------------

#[test]
fn every_command_carries_the_private_socket_when_one_is_configured() {
    let adapter = tmux(creating_runner());
    adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    let calls = adapter.runner().calls();
    assert!(!calls.is_empty());
    for call in &calls {
        assert_eq!(
            &call[..3],
            &[
                "tmux".to_string(),
                "-L".to_string(),
                "aikit-argv".to_string()
            ],
            "a command that forgets the socket would touch the user's real server: {call:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Creating a session
// ---------------------------------------------------------------------------

#[test]
fn a_missing_session_is_created_detached_with_the_view_as_the_window_name() {
    let adapter = tmux(creating_runner());
    let binding = adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    assert!(binding.created);
    assert_eq!(binding.session, "payments");
    assert_eq!(binding.kind, Some(MuxKind::Tmux));

    let lines = adapter.runner().call_lines();
    assert!(
        lines.iter().any(|l| l.contains("has-session -t payments")),
        "existence has to be asked about before anything is created: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("new-session -d -s payments -n main -P -F #{pane_id}")),
        "got: {lines:?}"
    );
}

#[test]
fn the_context_environment_is_set_before_any_pane_runs_a_command() {
    // tmux copies the session environment into every pane it creates, so the
    // order is not cosmetic: variables set after a pane exists never reach it.
    let adapter = tmux(creating_runner());
    let root = std::path::PathBuf::from("/work/payments");
    adapter
        .ensure_session(
            &three_pane_plan("payments", &root),
            ReconcileMode::default(),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    let first_env = lines
        .iter()
        .position(|l| l.contains("set-environment"))
        .expect("the environment must be set");
    let first_split = lines
        .iter()
        .position(|l| l.contains("split-window"))
        .expect("the plan splits panes");
    assert!(
        first_env < first_split,
        "panes created before the environment is set would not inherit it: {lines:?}"
    );

    for expected in [
        "set-environment -t payments AIKIT_SESSION_ID ses_TESTSESSION000000000000",
        "set-environment -t payments AIKIT_CONTEXT_ID ctx_TESTCONTEXT000000000000",
        "set-environment -t payments AIKIT_PROJECT_ROOT /work/payments",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(expected)),
            "missing `{expected}` in {lines:?}"
        );
    }
}

#[test]
fn the_session_carries_aikit_user_options_for_status_and_recovery() {
    let adapter = tmux(creating_runner());
    adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines
        .iter()
        .any(|l| l.contains("set-option -t payments @aikit_session ses_TESTSESSION000000000000")));
    assert!(lines
        .iter()
        .any(|l| l.contains("set-option -t payments @aikit_profile rust-review")));
}

#[test]
fn a_split_maps_a_compass_direction_and_a_ratio_onto_tmux_flags() {
    let adapter = tmux(creating_runner());
    let root = std::path::PathBuf::from("/work/payments");
    adapter
        .ensure_session(
            &three_pane_plan("payments", &root),
            ReconcileMode::default(),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    // `right` is a horizontal split placed after the origin pane; 0.3 of the
    // parent is `-l 30%`, which tmux only understands with the percent sign.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("split-window -t %0 -h -l 30% -c /work/payments -P -F #{pane_id}")),
        "got: {lines:?}"
    );
    // `down` is a vertical split, also after the origin pane.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("split-window -t %0 -v -l 40% -c /work/payments -P -F #{pane_id}")),
        "got: {lines:?}"
    );
}

#[test]
fn a_left_or_up_split_asks_tmux_to_place_the_new_pane_before_the_origin() {
    let adapter = tmux(creating_runner());
    let plan = plan_from(
        r#"
schema = 1
id = "s"
name = "s"

[[views]]
id = "v"
[[views.panes]]
id = "a"
[[views.panes]]
id = "b"
split_from = "a"
direction = "left"
[[views.panes]]
id = "c"
split_from = "a"
direction = "up"
"#,
    );
    adapter
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines.iter().any(|l| l.contains("split-window -t %0 -h -b")));
    assert!(lines.iter().any(|l| l.contains("split-window -t %0 -v -b")));
}

#[test]
fn every_created_pane_is_tagged_so_a_later_run_can_recognise_it() {
    // The tag is what separates "a pane AIKit made from this plan" from "a pane
    // the user split off by hand". Without it, idempotency is guesswork.
    let adapter = tmux(creating_runner());
    adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("set-option -p -t %0 @aikit_pane main/shell")),
        "got: {lines:?}"
    );
}

#[test]
fn a_root_panes_command_is_respawned_after_the_environment_is_in_place() {
    let adapter = tmux(creating_runner());
    let plan = plan_from(
        r#"
schema = 1
id = "s"
name = "s"

[[views]]
id = "v"
[[views.panes]]
id = "a"
command = ["htop", "-d", "10"]
"#,
    );
    adapter
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("respawn-pane -k -t %0 htop -d 10")),
        "got: {lines:?}"
    );
}

#[test]
fn a_second_view_becomes_a_window_rather_than_a_second_session() {
    let adapter = tmux(creating_runner());
    let root = std::path::PathBuf::from("/work/payments");
    adapter
        .ensure_session(
            &three_pane_plan("payments", &root),
            ReconcileMode::default(),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(
        lines.iter().any(
            |l| l.contains("new-window -t payments: -n ops -c /work/payments -P -F #{pane_id}")
        ),
        "got: {lines:?}"
    );
}

// ---------------------------------------------------------------------------
// The popup
// ---------------------------------------------------------------------------

#[test]
fn the_palette_popup_argv_is_exactly_the_documented_geometry() {
    let runner = ScriptedRunner::new().on("display-popup", "");
    let adapter = tmux(runner);

    let host = adapter
        .open_palette(PaletteRequest::default().running(vec!["aikit".into(), "palette".into()]))
        .unwrap();
    assert!(matches!(host, UiHost::TruePopup { .. }));

    assert_eq!(
        adapter.runner().call_lines(),
        vec![
            "tmux -L aikit-argv display-popup -E -w 82% -h 70% -T AIKit aikit palette".to_string()
        ]
    );
}

#[test]
fn a_palette_with_no_command_still_reports_the_popup_host_without_opening_one() {
    // The caller only wanted to know where the palette would go.
    let adapter = tmux(ScriptedRunner::new());
    let host = adapter.open_palette(PaletteRequest::default()).unwrap();

    assert!(matches!(host, UiHost::TruePopup { .. }));
    assert!(
        adapter.runner().calls().is_empty(),
        "asking where the palette goes must not open one"
    );
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

#[test]
fn spawning_a_new_pane_splits_the_target_pane() {
    let runner = ScriptedRunner::new().on("split-window", "%7");
    let adapter = tmux(runner);
    let mut request = SpawnRequest::new(Placement::NewPane, vec!["claude".into()])
        .in_dir("/work/payments")
        .splitting(Direction::Right, Some(0.45))
        .from_target(MuxTarget::surface(MuxKind::Tmux, "%3"));
    request
        .env
        .insert("AIKIT_PROMPT".to_string(), "review carefully".to_string());

    let spawned = adapter.spawn(request).unwrap();

    assert_eq!(spawned.target.surface.as_deref(), Some("%7"));
    assert!(spawned.created);
    assert_eq!(
        adapter.runner().call_lines(),
        vec!["tmux -L aikit-argv split-window -t %3 -h -l 45% -c /work/payments -P -F #{pane_id} env 'AIKIT_PROMPT=review carefully' claude"
            .to_string()]
    );
}

#[test]
fn spawning_a_new_view_creates_a_window_and_spawning_in_the_background_detaches_it() {
    let runner = ScriptedRunner::new().on("new-window", "%8");
    let adapter = tmux(runner);

    adapter
        .spawn(
            SpawnRequest::new(Placement::NewView, vec!["claude".into()])
                .named("review")
                .from_target(MuxTarget::session(MuxKind::Tmux, "payments")),
        )
        .unwrap();
    assert!(adapter
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("new-window -t payments: -n review -P -F #{pane_id} claude")));

    let background = tmux(ScriptedRunner::new().on("new-window", "%9"));
    background
        .spawn(
            SpawnRequest::new(Placement::Background, vec!["cargo".into(), "test".into()])
                .from_target(MuxTarget::session(MuxKind::Tmux, "payments")),
        )
        .unwrap();
    assert!(
        background
            .runner()
            .call_lines()
            .iter()
            .any(|l| l.contains("new-window -d -t payments:")),
        "a background job must not steal focus: {:?}",
        background.runner().call_lines()
    );
}

#[test]
fn spawning_into_the_current_pane_runs_the_command_there_rather_than_splitting() {
    let adapter = tmux(ScriptedRunner::new().on("respawn-pane", ""));
    let spawned = adapter
        .spawn(
            SpawnRequest::new(Placement::Current, vec!["claude".into()])
                .from_target(MuxTarget::surface(MuxKind::Tmux, "%2")),
        )
        .unwrap();

    assert!(!spawned.created);
    assert_eq!(spawned.target.surface.as_deref(), Some("%2"));
    assert!(adapter
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("respawn-pane -k -t %2 claude")));
}

// ---------------------------------------------------------------------------
// Focus, close, status, notify
// ---------------------------------------------------------------------------

#[test]
fn focusing_a_pane_selects_the_pane_and_its_window() {
    let adapter = tmux(
        ScriptedRunner::new()
            .on("select-pane", "")
            .on("select-window", ""),
    );
    adapter
        .focus(&MuxTarget::surface(MuxKind::Tmux, "%4"))
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines.iter().any(|l| l.contains("select-pane -t %4")));
    assert!(
        lines.iter().any(|l| l.contains("select-window -t %4")),
        "selecting a pane in a background window has to bring the window forward too: {lines:?}"
    );
}

#[test]
fn closing_a_target_kills_the_smallest_thing_it_names() {
    let panes = tmux(ScriptedRunner::new().on("kill-pane", ""));
    panes
        .close(&MuxTarget::surface(MuxKind::Tmux, "%4"))
        .unwrap();
    assert!(panes
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("kill-pane -t %4")));

    let sessions = tmux(ScriptedRunner::new().on("kill-session", ""));
    sessions
        .close(&MuxTarget::session(MuxKind::Tmux, "payments"))
        .unwrap();
    assert!(sessions
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("kill-session -t payments")));
}

#[test]
fn status_values_become_user_options_which_a_status_line_can_interpolate() {
    let adapter = tmux(ScriptedRunner::new().on("set-option", ""));
    adapter
        .set_status(
            StatusUpdate::for_session("payments")
                .with("profile", "rust-review")
                .with("generation", "gen_ab12"),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines
        .iter()
        .any(|l| l.contains("set-option -t payments @aikit_profile rust-review")));
    assert!(lines
        .iter()
        .any(|l| l.contains("set-option -t payments @aikit_generation gen_ab12")));
}

#[test]
fn a_notification_becomes_a_display_message_because_tmux_has_no_native_one() {
    let adapter = tmux(ScriptedRunner::new().on("display-message", ""));
    adapter
        .notify(Notification::new("Applied", "gen_ab12 is current"))
        .unwrap();

    assert!(adapter
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("display-message Applied: gen_ab12 is current")));
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

#[test]
fn tmux_claims_a_true_popup_and_panes_but_not_a_browser_surface() {
    let caps = tmux(ScriptedRunner::new()).capabilities();
    assert!(caps.true_popup);
    assert!(caps.panes);
    assert!(caps.workspaces, "tmux windows are the workspace analogue");
    assert!(caps.status_metadata);
    assert!(caps.remote_control);
    assert!(
        !caps.browser_surface,
        "tmux has no browser surface and must not claim one"
    );
    assert!(
        !caps.workspace_groups,
        "a tmux session is not a switchable group of workspaces in cmux's sense"
    );
}
