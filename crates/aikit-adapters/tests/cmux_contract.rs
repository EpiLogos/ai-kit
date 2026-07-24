//! cmux, driven by recorded responses.
//!
//! cmux is a macOS-native terminal with a JSON control socket. It is **not**
//! tmux, and this adapter does not pretend it is: a session becomes a workspace
//! (or a window grouping one workspace per view), a pane becomes a split surface,
//! and status, progress, logs and notifications go to native surfaces rather than
//! to a status-line string.
//!
//! Because cmux may not be installed — and, when it is, may not be running — the
//! adapter is exercised against JSON fixtures in `tests/fixtures/cmux/`. Those
//! fixtures are the recorded shape of the control protocol this build
//! understands; the parser is deliberately tolerant, and anything it does not
//! recognise degrades to "assume the feature is absent" rather than to a guess.
//!
//! The single most important property here is **capability discovery**: nothing
//! is hardcoded as present. Every feature is probed, and every unavailable one
//! produces a stated reason.

mod common;

use common::*;

use std::path::PathBuf;

use aikit_adapters::mux::cmux::{Cmux, Grouping, SidebarSurface};
use aikit_adapters::mux::{
    MuxAdapter, MuxTarget, Notification, PaletteRequest, ReconcileMode, SessionIdentity,
    SpawnRequest, StatusUpdate, UiHost,
};
use aikit_adapters::runner::ScriptedRunner;
use aikit_core::context::Isolation;
use aikit_core::id::{ContextId, SessionId};
use aikit_core::platform::MuxKind;
use aikit_core::session::Placement;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cmux")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} should exist: {e}", path.display()))
}

fn identity() -> SessionIdentity {
    SessionIdentity {
        session_id: Some(SessionId::parse("ses_CMUXSESSION000000000000").unwrap()),
        context_id: Some(ContextId::parse("ctx_CMUXCONTEXT000000000000").unwrap()),
        project_root: Some("/work/payments".into()),
        view_root: None,
        profile: Some("rust-review".into()),
        isolation: Isolation::Shared,
    }
}

/// A cmux that answers every command AIKit issues while building a session.
fn full_runner() -> ScriptedRunner {
    ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-full.json"))
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .on("list-workspaces", &fixture("list-workspaces-empty.json"))
        .on("new-window", &fixture("new-window.json"))
        .sequence(
            "new-workspace",
            &[
                &fixture("new-workspace.json"),
                &fixture("new-workspace.json").replace("workspace:3", "workspace:4"),
            ],
        )
        .sequence(
            "new-split",
            &[
                &fixture("new-split.json"),
                &fixture("new-split.json").replace("surface:5", "surface:6"),
            ],
        )
        .on("move-workspace-to-window", &fixture("ok.json"))
        .on("respawn-pane", &fixture("ok.json"))
        .on("workspace-action", &fixture("ok.json"))
        .on("select-workspace", &fixture("ok.json"))
        .on("focus-pane", &fixture("ok.json"))
        .on("notify", &fixture("ok.json"))
        .on("identify", &fixture("identify.json"))
        .on("markdown", &fixture("ok.json"))
        .on("close-workspace", &fixture("ok.json"))
        .on("close-surface", &fixture("ok.json"))
        .on("rename-workspace", &fixture("ok.json"))
}

fn cmux(runner: ScriptedRunner) -> Cmux<ScriptedRunner> {
    Cmux::new(runner).with_identity(identity())
}

// ---------------------------------------------------------------------------
// Capability discovery
// ---------------------------------------------------------------------------

#[test]
fn capabilities_are_probed_from_the_running_cmux_rather_than_assumed() {
    let adapter = cmux(full_runner());
    let caps = adapter.capabilities();

    assert!(caps.workspaces);
    assert!(caps.workspace_groups);
    assert!(caps.windows);
    assert!(caps.panes);
    assert!(caps.browser_surface);
    assert!(caps.notifications);
    assert!(caps.status_metadata);
    assert!(caps.remote_control);

    assert!(
        adapter.runner().call_lines().iter().any(|l| l.contains("capabilities")),
        "the capabilities have to come from the binary, not from this source file"
    );
}

#[test]
fn an_older_cmux_that_lacks_a_feature_is_reported_as_lacking_it() {
    let runner = ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-minimal.json"))
        .on("list-workspaces", &fixture("list-workspaces-empty.json"));
    let caps = cmux(runner).capabilities();

    assert!(caps.workspaces, "it does have workspaces");
    assert!(
        !caps.workspace_groups,
        "no `new-window`, so a grouped session is not possible here"
    );
    assert!(!caps.windows);
    assert!(!caps.notifications, "no `notify` command in this build");
    assert!(!caps.browser_surface);
    assert!(!caps.status_metadata, "no `workspace-action` command");
}

#[test]
fn a_popup_command_in_the_list_is_not_enough_to_claim_a_true_popup() {
    // cmux ships a tmux-compatibility `popup` command whose semantics are not
    // documented as an arbitrary-command overlay. Inferring `true_popup` from its
    // presence would make the palette try to open something that may not exist,
    // so the flag requires an explicit feature declaration.
    let runner = ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-minimal.json"))
        .on("list-workspaces", &fixture("list-workspaces-empty.json"));
    let adapter = cmux(runner);

    assert!(
        fixture("capabilities-minimal.json").contains("\"popup\""),
        "this test is only meaningful if the fixture really lists a popup command"
    );
    assert!(!adapter.capabilities().true_popup);
}

#[test]
fn a_cmux_that_is_not_running_reports_absence_with_the_socket_error_as_the_reason() {
    let runner = ScriptedRunner::new()
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .failing("capabilities", 1, &fixture("socket-missing.txt"));
    let adapter = cmux(runner);

    let presence = adapter.detect().unwrap();
    assert!(presence.installed, "the binary is on PATH");
    assert!(!presence.server_running);
    assert!(
        presence.detail.as_deref().unwrap_or("").contains("Socket not found"),
        "the user is owed the actual reason, got {:?}",
        presence.detail
    );

    // With no socket there is nothing to probe, so nothing is claimed.
    assert_eq!(
        adapter.capabilities(),
        aikit_adapters::mux::MuxCapabilities::default()
    );
}

#[test]
fn an_unparseable_capabilities_response_degrades_instead_of_guessing() {
    let runner = ScriptedRunner::new()
        .on("version", "cmux 99.0.0")
        .on("capabilities", "this is not json, and never was");
    let adapter = cmux(runner);

    assert_eq!(
        adapter.capabilities(),
        aikit_adapters::mux::MuxCapabilities::default(),
        "a protocol this build does not understand must not be read optimistically"
    );
    assert!(adapter
        .probe()
        .unwrap()
        .note
        .unwrap_or_default()
        .contains("could not be parsed"));
}

#[test]
fn the_probe_is_made_once_and_reused() {
    let adapter = cmux(full_runner());
    adapter.capabilities();
    adapter.capabilities();
    adapter.capabilities();

    let probes = adapter
        .runner()
        .call_lines()
        .iter()
        .filter(|l| l.contains("capabilities"))
        .count();
    assert_eq!(probes, 1, "a palette that reprobes on every keystroke is slow");
}

// ---------------------------------------------------------------------------
// Workspaces and groups
// ---------------------------------------------------------------------------

#[test]
fn a_single_view_session_becomes_one_workspace_with_no_group() {
    let adapter = cmux(full_runner());
    let binding = adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    assert!(binding.created);
    assert_eq!(binding.kind, Some(MuxKind::Cmux));

    let lines = adapter.runner().call_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("new-workspace --name payments")),
        "got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("new-window")),
        "grouping = auto must not wrap a single view in a group: {lines:?}"
    );
    assert_eq!(binding.surfaces.len(), 1);
}

#[test]
fn a_multi_view_session_becomes_a_group_with_one_workspace_per_view() {
    let adapter = cmux(full_runner());
    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines.iter().any(|l| l.contains("new-window")), "got: {lines:?}");
    assert!(lines
        .iter()
        .any(|l| l.contains("new-workspace --name payments · code --cwd /work/payments")));
    assert!(lines
        .iter()
        .any(|l| l.contains("new-workspace --name payments · ops --cwd /work/payments")));
    assert!(
        lines
            .iter()
            .filter(|l| l.contains("move-workspace-to-window"))
            .count()
            == 2,
        "each view's workspace joins the group: {lines:?}"
    );

    assert_eq!(binding.session, "window:2", "the group is the session handle");
    assert_eq!(binding.views.len(), 2);
}

#[test]
fn grouping_never_keeps_the_views_as_loose_workspaces_and_says_so() {
    let adapter = cmux(full_runner()).with_grouping(Grouping::Never);
    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(!lines.iter().any(|l| l.contains("new-window")));
    assert!(!lines.iter().any(|l| l.contains("move-workspace-to-window")));
    assert!(
        binding.warnings.iter().any(|w| w.contains("grouping")),
        "a session split across ungrouped workspaces is worth mentioning: {:?}",
        binding.warnings
    );
}

#[test]
fn grouping_always_wraps_even_a_single_view() {
    let adapter = cmux(full_runner()).with_grouping(Grouping::Always);
    adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    assert!(adapter
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("new-window")));
}

#[test]
fn grouping_comes_from_the_session_specs_own_backend_table() {
    // `[backend.cmux] grouping = "never"` is a per-session decision, so it has to
    // beat the adapter default rather than the other way round.
    let plan = plan_from(
        r#"
schema = 1
id = "payments"
name = "payments"

[backend]
kind = "cmux"

[backend.cmux]
grouping = "never"

[[views]]
id = "code"
[[views.panes]]
id = "editor"

[[views]]
id = "ops"
[[views.panes]]
id = "watch"
"#,
    );
    let adapter = cmux(full_runner()).with_grouping(Grouping::Always);
    adapter.ensure_session(&plan, ReconcileMode::default()).unwrap();

    assert!(
        !adapter
            .runner()
            .call_lines()
            .iter()
            .any(|l| l.contains("new-window")),
        "the spec said never"
    );
}

#[test]
fn a_cmux_without_grouping_support_degrades_to_loose_workspaces_with_a_reason() {
    let runner = ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-minimal.json"))
        .on("list-workspaces", &fixture("list-workspaces-empty.json"))
        .sequence(
            "new-workspace",
            &[&fixture("new-workspace.json"), &fixture("new-workspace.json")],
        )
        .on("new-split", &fixture("new-split.json"))
        .on("select-workspace", &fixture("ok.json"));
    let adapter = cmux(runner).with_grouping(Grouping::Always);

    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    assert!(
        binding
            .warnings
            .iter()
            .any(|w| w.contains("grouping") && w.contains("this cmux")),
        "asking for a group from a cmux that has none must say so: {:?}",
        binding.warnings
    );
}

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

#[test]
fn a_pane_becomes_a_split_surface_in_the_views_workspace() {
    let adapter = cmux(full_runner());
    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    // `right` and `down` are cmux's own direction words, so no translation table
    // is needed — but the workspace and origin surface still have to be named.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("new-split right --workspace workspace:3")),
        "got: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("new-split down --workspace workspace:3")),
        "got: {lines:?}"
    );
    assert!(binding.surface_of("code", "tests").is_some());
    assert!(binding.surface_of("code", "logs").is_some());
}

#[test]
fn a_pane_command_carries_the_aikit_context_because_cmux_has_no_session_environment() {
    let adapter = cmux(full_runner());
    let plan = plan_from(
        r#"
schema = 1
id = "payments"
name = "payments"

[[views]]
id = "code"
[[views.panes]]
id = "editor"
command = ["claude"]
"#,
    );
    adapter.ensure_session(&plan, ReconcileMode::default()).unwrap();

    let lines = adapter.runner().call_lines();
    let command_line = lines
        .iter()
        .find(|l| l.contains("--command"))
        .unwrap_or_else(|| panic!("the pane command must be sent: {lines:?}"));
    assert!(command_line.contains("AIKIT_SESSION_ID=ses_CMUXSESSION000000000000"));
    assert!(command_line.contains("AIKIT_CONTEXT_ID=ctx_CMUXCONTEXT000000000000"));
    assert!(command_line.contains("AIKIT_PROJECT_ROOT=/work/payments"));
    assert!(command_line.trim_end().ends_with("claude"));
}

#[test]
fn a_pane_with_no_command_is_flagged_because_it_cannot_inherit_the_context() {
    let adapter = cmux(full_runner());
    let binding = adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    assert!(
        binding.warnings.iter().any(|w| w.contains("environment")),
        "pretending a default shell got AIKIT_SESSION_ID would be a lie: {:?}",
        binding.warnings
    );
}

// ---------------------------------------------------------------------------
// Restore and rebind
// ---------------------------------------------------------------------------

#[test]
fn an_existing_session_is_rebound_by_title_when_cmux_hands_back_new_ids() {
    // cmux ids are bindings, not identity: a restored app gives the same
    // human-meaningful workspace a different id. Rebinding by title is what stops
    // `session up` from building a duplicate session after a restart.
    let runner = full_runner_with_workspaces(&fixture("list-workspaces-rebound.json"));
    let adapter = cmux(runner);

    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    assert!(!binding.created, "the workspaces were already there");
    assert_eq!(binding.views.get("code").map(String::as_str), Some("workspace:9"));
    assert_eq!(binding.views.get("ops").map(String::as_str), Some("workspace:11"));
    assert_eq!(
        binding.session, "window:4",
        "the group is whichever window those workspaces are in now"
    );

    let lines = adapter.runner().call_lines();
    assert!(
        !lines.iter().any(|l| l.contains("new-workspace")),
        "rebinding must not create a second copy of the session: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("close-workspace")),
        "and it must not close the unrelated `scratch` workspace either: {lines:?}"
    );
}

#[test]
fn an_exact_reconcile_closes_workspaces_the_plan_no_longer_declares() {
    let runner = full_runner_with_workspaces(&fixture("list-workspaces-rebound.json"));
    let adapter = cmux(runner);

    // The plan lost its `ops` view.
    let plan = plan_from(
        r#"
schema = 1
id = "payments"
name = "payments"

[[views]]
id = "code"
[[views.panes]]
id = "editor"
"#,
    );
    let binding = adapter.ensure_session(&plan, ReconcileMode::Exact).unwrap();

    let lines = adapter.runner().call_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("close-workspace --workspace workspace:11")),
        "got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("workspace:12")),
        "a workspace that was never AIKit's is not the plan's business: {lines:?}"
    );
    assert!(binding.actions.iter().any(|a| a.contains("ops")));
}

// ---------------------------------------------------------------------------
// The palette
// ---------------------------------------------------------------------------

#[test]
fn the_palette_is_inline_by_default_because_cmux_has_no_arbitrary_popup() {
    let adapter = cmux(full_runner());
    assert_eq!(
        adapter.open_palette(PaletteRequest::default()).unwrap(),
        UiHost::InlineCurrentTerminal
    );
}

#[test]
fn a_temporary_surface_palette_is_available_when_it_is_asked_for() {
    let adapter = cmux(full_runner());
    let host = adapter
        .open_palette(PaletteRequest {
            prefer_temporary_surface: true,
            ..PaletteRequest::default()
        })
        .unwrap();

    match host {
        UiHost::TemporarySurface { id } => assert_eq!(id, "surface:5"),
        other => panic!("expected a temporary surface, got {other:?}"),
    }
}

#[test]
fn a_temporary_surface_palette_falls_back_to_inline_when_splitting_is_unavailable() {
    let runner = ScriptedRunner::new()
        .on("capabilities", r#"{"version":"0.1.0","commands":["list-workspaces"]}"#);
    let adapter = cmux(runner);

    assert_eq!(
        adapter
            .open_palette(PaletteRequest {
                prefer_temporary_surface: true,
                ..PaletteRequest::default()
            })
            .unwrap(),
        UiHost::InlineCurrentTerminal
    );
}

// ---------------------------------------------------------------------------
// Sidebar surfaces
// ---------------------------------------------------------------------------

#[test]
fn the_status_pill_is_a_native_workspace_action_not_a_status_line_string() {
    let adapter = cmux(full_runner());
    adapter
        .set_status(
            StatusUpdate::for_session("workspace:3")
                .with("profile", "rust-review")
                .with("color", "blue"),
        )
        .unwrap();

    let lines = adapter.runner().call_lines();
    let action = lines
        .iter()
        .find(|l| l.contains("workspace-action"))
        .unwrap_or_else(|| panic!("got: {lines:?}"));
    assert!(action.contains("--action set-title"));
    assert!(action.contains("--workspace workspace:3"));
    assert!(action.contains("rust-review"));
    assert!(
        action.contains("--color blue"),
        "a colour is a first-class pill property here: {action}"
    );
}

#[test]
fn progress_and_logs_go_to_their_own_surfaces() {
    let adapter = cmux(full_runner());

    let progress = adapter
        .post(
            SidebarSurface::Progress,
            "workspace:3",
            "applying gen_ab12 · 40%",
        )
        .unwrap();
    assert!(progress.delivered);

    let log = adapter
        .post(SidebarSurface::Log, "workspace:3", "/tmp/aikit/apply.md")
        .unwrap();
    assert!(log.delivered);
    assert!(
        adapter
            .runner()
            .call_lines()
            .iter()
            .any(|l| l.contains("markdown open /tmp/aikit/apply.md")),
        "the log panel is cmux's markdown viewer: {:?}",
        adapter.runner().call_lines()
    );
}

#[test]
fn a_surface_this_cmux_does_not_have_is_reported_undelivered_with_a_reason() {
    let runner = ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-minimal.json"))
        .on("list-workspaces", &fixture("list-workspaces-empty.json"));
    let adapter = cmux(runner);

    let posted = adapter
        .post(SidebarSurface::Log, "workspace:1", "/tmp/apply.md")
        .unwrap();
    assert!(!posted.delivered);
    assert!(
        posted.note.unwrap_or_default().contains("markdown"),
        "the reason has to name the missing capability"
    );
}

#[test]
fn a_notification_uses_the_native_one_when_there_is_one() {
    let adapter = cmux(full_runner());
    adapter
        .notify(Notification::new("Applied", "gen_ab12 is current"))
        .unwrap();

    assert!(adapter
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("notify --title Applied --body gen_ab12 is current")));
}

#[test]
fn a_notification_without_a_native_one_falls_back_rather_than_vanishing() {
    let runner = ScriptedRunner::new()
        .on(
            "capabilities",
            r#"{"version":"0.5.0","commands":["list-workspaces","workspace-action"]}"#,
        )
        .on("workspace-action", r#"{"ok":true}"#);
    let adapter = cmux(runner);

    adapter
        .notify(Notification::new("Applied", "gen_ab12 is current"))
        .unwrap();
    assert!(
        adapter
            .runner()
            .call_lines()
            .iter()
            .any(|l| l.contains("workspace-action")),
        "a message the user needs must land somewhere: {:?}",
        adapter.runner().call_lines()
    );
}

#[test]
fn a_cmux_with_nowhere_to_put_a_notification_says_so_instead_of_swallowing_it() {
    let runner =
        ScriptedRunner::new().on("capabilities", r#"{"version":"0.1.0","commands":["ping"]}"#);
    let error = cmux(runner)
        .notify(Notification::new("Applied", "gen_ab12"))
        .unwrap_err();

    assert_eq!(error.code(), "mux.unsupported");
}

// ---------------------------------------------------------------------------
// Location, spawn, focus, close
// ---------------------------------------------------------------------------

#[test]
fn the_current_location_comes_from_identify() {
    let adapter = cmux(full_runner()).with_env_var("CMUX_WORKSPACE_ID", "workspace:2");
    let location = adapter.current_location().unwrap();

    assert_eq!(location.kind, MuxKind::Cmux);
    assert_eq!(location.session.as_deref(), Some("workspace:2"));
    assert_eq!(location.surface.as_deref(), Some("surface:7"));
    assert_eq!(location.host, "localhost");
}

#[test]
fn outside_cmux_the_location_is_empty_rather_than_invented() {
    let adapter = cmux(full_runner());
    let location = adapter.current_location().unwrap();

    assert_eq!(location.session, None);
    assert!(!location.target().is_addressable());
}

#[test]
fn spawning_a_new_pane_splits_a_surface_and_a_new_view_creates_a_workspace() {
    let adapter = cmux(full_runner());
    let pane = adapter
        .spawn(
            SpawnRequest::new(Placement::NewPane, vec!["claude".into()])
                .splitting(aikit_core::session::Direction::Right, None)
                .from_target(MuxTarget::surface(MuxKind::Cmux, "surface:2")),
        )
        .unwrap();
    assert_eq!(pane.target.surface.as_deref(), Some("surface:5"));

    let view = cmux(full_runner());
    view.spawn(
        SpawnRequest::new(Placement::NewView, vec!["claude".into()]).named("review"),
    )
    .unwrap();
    assert!(view
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("new-workspace --name review")));
}

#[test]
fn focusing_addresses_a_surface_or_a_workspace_with_the_right_command() {
    let surfaces = cmux(full_runner());
    surfaces
        .focus(&MuxTarget::surface(MuxKind::Cmux, "surface:5"))
        .unwrap();
    assert!(surfaces
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("focus-pane --pane surface:5")));

    let workspaces = cmux(full_runner());
    workspaces
        .focus(&MuxTarget::session(MuxKind::Cmux, "workspace:3"))
        .unwrap();
    assert!(workspaces
        .runner()
        .call_lines()
        .iter()
        .any(|l| l.contains("select-workspace --workspace workspace:3")));
}

#[test]
fn closing_a_surface_and_closing_a_workspace_are_different_commands() {
    let adapter = cmux(full_runner());
    adapter
        .close(&MuxTarget::surface(MuxKind::Cmux, "surface:5"))
        .unwrap();
    adapter
        .close(&MuxTarget::session(MuxKind::Cmux, "workspace:3"))
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines
        .iter()
        .any(|l| l.contains("close-surface --surface surface:5")));
    assert!(lines
        .iter()
        .any(|l| l.contains("close-workspace --workspace workspace:3")));
}

#[test]
fn cmux_never_claims_to_be_tmux() {
    let adapter = cmux(full_runner());
    assert_eq!(adapter.kind(), MuxKind::Cmux);
    for line in adapter.runner().call_lines() {
        assert!(
            !line.starts_with("tmux "),
            "the cmux adapter issued a tmux command: {line}"
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn full_runner_with_workspaces(listing: &str) -> ScriptedRunner {
    ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-full.json"))
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .on("list-workspaces", listing)
        .on("new-window", &fixture("new-window.json"))
        .sequence(
            "new-workspace",
            &[&fixture("new-workspace.json"), &fixture("new-workspace.json")],
        )
        .sequence(
            "new-split",
            &[&fixture("new-split.json"), &fixture("new-split.json")],
        )
        .on("move-workspace-to-window", &fixture("ok.json"))
        .on("respawn-pane", &fixture("ok.json"))
        .on("workspace-action", &fixture("ok.json"))
        .on("select-workspace", &fixture("ok.json"))
        .on("close-workspace", &fixture("ok.json"))
        .on("close-surface", &fixture("ok.json"))
        .on("list-panes", r#"{"panes":[]}"#)
}
