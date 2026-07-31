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
        .on("new-window", "OK window:2")
        .sequence("new-workspace", &["OK workspace:3", "OK workspace:4"])
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
        .on("rename-tab", &fixture("ok.json"))
        .on("list-panes", &fixture("list-panes-one.json"))
        .on(
            "list-pane-surfaces",
            &fixture("list-pane-surfaces-root.json"),
        )
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
        adapter
            .runner()
            .call_lines()
            .iter()
            .any(|l| l.contains("capabilities")),
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
        presence
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("Socket not found"),
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
fn an_access_control_rejection_is_not_misreported_as_cmux_being_stopped() {
    let runner = ScriptedRunner::new()
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .failing(
            "capabilities",
            1,
            "Error: connect failed: Operation not permitted, errno 1",
        );
    let presence = cmux(runner).detect().unwrap();

    assert!(presence.installed);
    assert!(
        presence.server_running,
        "the CLI connected before the server rejected the write"
    );
    assert!(presence
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("allowed automation scope")));
}

#[test]
fn session_up_reports_cmux_access_control_instead_of_claiming_missing_features() {
    let runner = ScriptedRunner::new()
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .failing("capabilities", 1, "Error: Failed to write to socket");
    let error = cmux(runner)
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap_err();
    assert_eq!(error.code(), "mux.cmux_access_denied");
    assert!(error.message().contains("inside cmux"));
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
    assert_eq!(
        probes, 1,
        "a palette that reprobes on every keystroke is slow"
    );
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
    assert_eq!(
        binding.surface_of("main", "shell"),
        Some("surface:1"),
        "the binding must use the surface cmux reported, not a synthetic handle"
    );
    assert!(
        adapter
            .runner()
            .call_lines()
            .iter()
            .all(|line| !line.contains("workspace:3:root")),
        "cmux has no documented workspace:root handle"
    );
}

#[test]
fn every_machine_read_that_is_parsed_as_json_requests_json_from_cmux() {
    let adapter = cmux(full_runner()).with_env_var("CMUX_WORKSPACE_ID", "workspace:2");
    adapter.workspaces().unwrap();
    adapter.current_location().unwrap();
    adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    let calls = adapter.runner().calls();
    for command in [
        "list-workspaces",
        "identify",
        "list-panes",
        "list-pane-surfaces",
    ] {
        let argv = calls
            .iter()
            .find(|argv| argv.iter().any(|arg| arg == command))
            .unwrap_or_else(|| panic!("expected {command} call in {calls:?}"));
        assert_eq!(
            argv.get(1).map(String::as_str),
            Some("--json"),
            "{command} output is parsed as JSON, so the global --json flag is mandatory: {argv:?}"
        );
    }
}

#[test]
fn current_cmux_ref_shaped_responses_and_plain_creation_handles_are_supported() {
    let runner = ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-full.json"))
        .on("version", "cmux 0.63.1 (78) [45090d23d]")
        .on(
            "list-workspaces",
            r#"{"workspaces":[{"ref":"workspace:9","title":"payments · main · ses_CMUXSESSION000000000000","window_ref":"window:4"}]}"#,
        )
        .on(
            "identify",
            r#"{"focused":{"workspace_ref":"workspace:9","window_ref":"window:4","surface_ref":"surface:7"},"caller":null}"#,
        );
    let adapter = cmux(runner).with_env_var("CMUX_WORKSPACE_ID", "workspace:9");

    let workspaces = adapter.workspaces().unwrap();
    assert_eq!(workspaces[0].id, "workspace:9");
    assert_eq!(workspaces[0].window.as_deref(), Some("window:4"));
    let location = adapter.current_location().unwrap();
    assert_eq!(location.session.as_deref(), Some("workspace:9"));
    assert_eq!(location.view.as_deref(), Some("window:4"));
    assert_eq!(location.surface.as_deref(), Some("surface:7"));
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
    assert!(
        lines.iter().any(|l| l.contains("new-window")),
        "got: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains(
        "new-workspace --name payments · code · ses_CMUXSESSION000000000000 --cwd /work/payments"
    )));
    assert!(lines.iter().any(|l| l.contains(
        "new-workspace --name payments · ops · ses_CMUXSESSION000000000000 --cwd /work/payments"
    )));
    assert!(
        lines
            .iter()
            .filter(|l| l.contains("move-workspace-to-window"))
            .count()
            == 2,
        "each view's workspace joins the group: {lines:?}"
    );

    assert_eq!(
        binding.session, "window:2",
        "the group is the session handle"
    );
    assert_eq!(binding.views.len(), 2);
}

#[test]
fn common_plan_names_in_different_sessions_cannot_claim_the_same_workspace() {
    let plan = single_pane_plan("dev");
    let first = cmux(full_runner());
    let mut other_identity = identity();
    other_identity.session_id = Some(SessionId::generate());
    let second = Cmux::new(full_runner()).with_identity(other_identity);

    let first_title = first.workspace_title(&plan, &plan.views[0]).unwrap();
    let second_title = second.workspace_title(&plan, &plan.views[0]).unwrap();
    assert_ne!(first_title, second_title);
    assert!(first_title.contains("ses_CMUXSESSION000000000000"));
}

#[test]
fn duplicate_human_view_names_still_have_distinct_workspace_identities() {
    let plan = plan_from(
        r#"
schema = 1
id = "dev"
name = "dev"

[[views]]
id = "frontend"
name = "work"
[[views.panes]]
id = "shell"

[[views]]
id = "backend"
name = "work"
[[views.panes]]
id = "shell"
"#,
    );
    let adapter = cmux(full_runner());

    let first = adapter.workspace_title(&plan, &plan.views[0]).unwrap();
    let second = adapter.workspace_title(&plan, &plan.views[1]).unwrap();
    assert_ne!(first, second);
    assert!(first.contains("frontend"));
    assert!(second.contains("backend"));
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
fn grouping_never_rebinds_each_view_to_its_own_identity_scoped_workspace() {
    let listing = r#"{"workspaces":[
      {"ref":"workspace:9","title":"payments · code · ses_CMUXSESSION000000000000"},
      {"ref":"workspace:11","title":"payments · ops · ses_CMUXSESSION000000000000"}
    ]}"#;
    let runner = full_runner_with_workspaces(listing).sequence(
        "list-pane-surfaces",
        &[
            r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"},{"ref":"surface:21","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/tests"},{"ref":"surface:22","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/logs"}]}"#,
            r#"{"surfaces":[{"ref":"surface:30","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
        ],
    );
    let adapter = cmux(runner).with_grouping(Grouping::Never);

    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    assert_eq!(
        binding.views.get("code").map(String::as_str),
        Some("workspace:9")
    );
    assert_eq!(
        binding.views.get("ops").map(String::as_str),
        Some("workspace:11")
    );
    let lines = adapter.runner().call_lines();
    assert!(
        !lines.iter().any(|line| line.contains("new-workspace")),
        "a second up must rebind, not duplicate: {lines:?}"
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
    adapter
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

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
fn a_cmux_without_surface_introspection_refuses_unsafe_topology_changes() {
    let runner = ScriptedRunner::new()
        .on("capabilities", &fixture("capabilities-minimal.json"))
        .on("list-workspaces", &fixture("list-workspaces-empty.json"))
        .sequence(
            "new-workspace",
            &[
                &fixture("new-workspace.json"),
                &fixture("new-workspace.json"),
            ],
        )
        .on("new-split", &fixture("new-split.json"))
        .on("select-workspace", &fixture("ok.json"));
    let adapter = cmux(runner).with_grouping(Grouping::Always);

    let error = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap_err();

    assert_eq!(error.code(), "mux.cmux_topology_unsupported");
    assert!(
        !adapter
            .runner()
            .call_lines()
            .iter()
            .any(|line| line.contains("new-workspace")),
        "AIKit must fail before creating topology it cannot safely read back"
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
    adapter
        .ensure_session(&plan, ReconcileMode::default())
        .unwrap();

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
    let runner = full_runner_with_workspaces(&fixture("list-workspaces-rebound.json")).sequence(
        "list-pane-surfaces",
        &[
            r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"}]}"#,
            r#"{"surfaces":[{"ref":"surface:21","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
        ],
    );
    let adapter = cmux(runner);

    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    assert!(!binding.created, "the workspaces were already there");
    assert_eq!(
        binding.views.get("code").map(String::as_str),
        Some("workspace:9")
    );
    assert_eq!(
        binding.views.get("ops").map(String::as_str),
        Some("workspace:11")
    );
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
fn durable_session_markers_resolve_the_current_workspace_and_window_handles() {
    let runner = full_runner_with_workspaces(&fixture("list-workspaces-rebound.json")).sequence(
        "list-pane-surfaces",
        &[
            r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"}]}"#,
            r#"{"surfaces":[{"ref":"surface:21","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
            r#"{"surfaces":[{"ref":"surface:22","title":"manual scratch"}]}"#,
        ],
    );
    let adapter = cmux(runner);
    let targets = adapter
        .session_targets(&SessionId::parse("ses_CMUXSESSION000000000000").unwrap())
        .unwrap();

    assert_eq!(targets.workspaces, ["workspace:11", "workspace:9"]);
    assert_eq!(targets.common_window.as_deref(), Some("window:4"));
    assert_eq!(targets.exclusive_window.as_deref(), Some("window:4"));
    let lines = adapter.runner().call_lines();
    assert!(
        !lines.iter().any(|line| {
            line.contains("new-")
                || line.contains("close-")
                || line.contains("rename-")
                || line.contains("move-")
        }),
        "live rebinding must only inspect cmux: {lines:?}"
    );
}

#[test]
fn a_group_window_with_a_foreign_workspace_is_not_exclusive_to_aikit() {
    let listing = fixture("list-workspaces-rebound.json")
        .replace(r#""window": "window:1""#, r#""window": "window:4""#);
    let runner = full_runner_with_workspaces(&listing).sequence(
        "list-pane-surfaces",
        &[
            r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"}]}"#,
            r#"{"surfaces":[{"ref":"surface:21","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
            r#"{"surfaces":[{"ref":"surface:22","title":"manual scratch"}]}"#,
        ],
    );
    let adapter = cmux(runner);
    let targets = adapter
        .session_targets(&SessionId::parse("ses_CMUXSESSION000000000000").unwrap())
        .unwrap();

    assert_eq!(targets.common_window.as_deref(), Some("window:4"));
    assert_eq!(
        targets.exclusive_window, None,
        "closing window:4 would discard the manual scratch workspace"
    );
}

#[test]
fn a_same_named_untagged_workspace_is_not_adopted_by_title_alone() {
    let runner = full_runner_with_workspaces(
        r#"{"workspaces":[{"ref":"workspace:9","title":"payments · main · ses_CMUXSESSION000000000000","window_ref":"window:4"}]}"#,
    );
    let adapter = cmux(runner);

    let error = adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap_err();
    assert_eq!(error.code(), "mux.cmux_ownership_ambiguous");
    let lines = adapter.runner().call_lines();
    assert!(
        !lines.iter().any(|line| {
            line.contains("rename-tab")
                || line.contains("new-split")
                || line.contains("close-surface")
        }),
        "the foreign workspace must remain untouched: {lines:?}"
    );
}

#[test]
fn every_existing_workspace_is_preflighted_before_any_missing_view_is_created() {
    // `code` is missing, but the later `ops` title belongs to the user. A
    // one-pass reconciler would create `code` and only then refuse `ops`, leaving
    // a partial session behind.
    let runner = full_runner_with_workspaces(
        r#"{"workspaces":[{"ref":"workspace:11","title":"payments · ops · ses_CMUXSESSION000000000000","window_ref":"window:4"}]}"#,
    );
    let adapter = cmux(runner);

    let error = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "mux.cmux_ownership_ambiguous");

    let lines = adapter.runner().call_lines();
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("new-window") || line.contains("new-workspace")),
        "ownership ambiguity must be discovered before the first mutation: {lines:?}"
    );
}

#[test]
fn an_existing_owned_workspace_gets_missing_panes_without_touching_manual_surfaces() {
    let listing = r#"{"workspaces":[{"ref":"workspace:9","title":"payments · code · ses_CMUXSESSION000000000000","window_ref":"window:4"},{"ref":"workspace:11","title":"payments · ops · ses_CMUXSESSION000000000000","window_ref":"window:4"}]}"#;
    let tagged = r#"{"surfaces":[{"ref":"surface:20","pane_ref":"pane:1","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"},{"ref":"surface:21","pane_ref":"pane:2","title":"my manual shell"}]}"#;
    let runner = full_runner_with_workspaces(listing)
        .sequence(
            "list-pane-surfaces",
            &[
                tagged,
                r#"{"surfaces":[{"ref":"surface:40","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
            ],
        )
        .sequence(
            "new-split",
            &[
                r#"{"surface_ref":"surface:30","pane_ref":"pane:3"}"#,
                r#"{"surface_ref":"surface:31","pane_ref":"pane:4"}"#,
            ],
        );
    let adapter = cmux(runner);

    let binding = adapter
        .ensure_session(
            &three_pane_plan("payments", &PathBuf::from("/work/payments")),
            ReconcileMode::default(),
        )
        .unwrap();

    assert_eq!(binding.surface_of("code", "editor"), Some("surface:20"));
    assert_eq!(binding.surface_of("code", "tests"), Some("surface:30"));
    assert_eq!(binding.surface_of("code", "logs"), Some("surface:31"));
    let lines = adapter.runner().call_lines();
    assert!(
        lines.iter().any(|line| {
            line.contains("--json new-split right") && line.contains("--surface surface:20")
        }),
        "missing declared panes must be added from their declared origin: {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("close-surface") && line.contains("surface:21")),
        "an untagged user surface is never AIKit's to close: {lines:?}"
    );
}

#[test]
fn exact_reconcile_closes_only_surplus_surfaces_with_aikit_ownership_tags() {
    let listing = r#"{"workspaces":[{"ref":"workspace:9","title":"payments · main · ses_CMUXSESSION000000000000","window_ref":"window:4"}]}"#;
    let surfaces = r#"{"surfaces":[
        {"ref":"surface:20","pane_ref":"pane:1","title":"AIKit · ses_CMUXSESSION000000000000/payments/main/shell"},
        {"ref":"surface:22","pane_ref":"pane:2","title":"AIKit · ses_CMUXSESSION000000000000/payments/main/removed"},
        {"ref":"surface:23","pane_ref":"pane:3","title":"user notes"}
    ]}"#;
    let runner = full_runner_with_workspaces(listing).on("list-pane-surfaces", surfaces);
    let adapter = cmux(runner);

    adapter
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::Exact)
        .unwrap();

    let lines = adapter.runner().call_lines();
    assert!(lines.iter().any(|line| {
        line.contains("close-surface")
            && line.contains("surface:22")
            && line.contains("--workspace workspace:9")
    }));
    assert!(!lines
        .iter()
        .any(|line| { line.contains("close-surface") && line.contains("surface:23") }));
}

#[test]
fn session_existence_is_read_only_and_uses_the_same_stable_titles_as_rebind() {
    let runner = full_runner_with_workspaces(
        r#"{"workspaces":[{"id":"workspace:88","title":"payments · main · ses_CMUXSESSION000000000000","window":"window:4"}]}"#,
    )
    .on(
        "list-pane-surfaces",
        r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/main/shell"}]}"#,
    );
    let adapter = cmux(runner);
    let plan = single_pane_plan("payments");

    assert!(adapter.session_exists(&plan).unwrap());
    assert!(
        !adapter
            .runner()
            .call_lines()
            .iter()
            .any(|line| line.contains("new-workspace") || line.contains("new-window")),
        "a diff/existence query must never create topology"
    );
}

#[test]
fn inspecting_cmux_drift_reports_missing_panes_without_reconciling_or_focusing() {
    let listing = r#"{"workspaces":[
      {"ref":"workspace:9","title":"payments · code · ses_CMUXSESSION000000000000","window_ref":"window:4"},
      {"ref":"workspace:11","title":"payments · ops · ses_CMUXSESSION000000000000","window_ref":"window:4"}
    ]}"#;
    let runner = full_runner_with_workspaces(listing).sequence(
        "list-pane-surfaces",
        &[
            r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"}]}"#,
            r#"{"surfaces":[{"ref":"surface:30","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
        ],
    );
    let adapter = cmux(runner);
    let binding = adapter
        .inspect_session(&three_pane_plan(
            "payments",
            &PathBuf::from("/work/payments"),
        ))
        .unwrap();

    assert!(binding
        .actions
        .iter()
        .any(|action| action.contains("code/tests")));
    let lines = adapter.runner().call_lines();
    assert!(
        !lines.iter().any(|line| {
            line.contains("new-")
                || line.contains("rename-")
                || line.contains("close-")
                || line.contains("select-workspace")
                || line.contains("focus-")
        }),
        "inspection must be physically read-only: {lines:?}"
    );
}

#[test]
fn a_matching_workspace_title_without_this_sessions_marker_does_not_exist() {
    let runner = full_runner_with_workspaces(
        r#"{"workspaces":[{"id":"workspace:88","title":"payments · main · ses_CMUXSESSION000000000000","window":"window:4"}]}"#,
    )
    .on(
        "list-pane-surfaces",
        r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_SOMEONEELSE000000000000/payments/main/shell"}]}"#,
    );
    let adapter = cmux(runner);

    assert!(!adapter
        .session_exists(&single_pane_plan("payments"))
        .unwrap());
}

#[test]
fn an_exact_reconcile_closes_workspaces_the_plan_no_longer_declares() {
    let runner = full_runner_with_workspaces(&fixture("list-workspaces-rebound.json")).sequence(
        "list-pane-surfaces",
        &[
            r#"{"surfaces":[{"ref":"surface:20","title":"AIKit · ses_CMUXSESSION000000000000/payments/code/editor"}]}"#,
            r#"{"surfaces":[{"ref":"surface:21","title":"AIKit · ses_CMUXSESSION000000000000/payments/ops/watch"}]}"#,
        ],
    );
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
    let runner = ScriptedRunner::new().on(
        "capabilities",
        r#"{"version":"0.1.0","commands":["list-workspaces"]}"#,
    );
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
    let mut request = SpawnRequest::new(Placement::NewPane, vec!["claude".into()])
        .splitting(aikit_core::session::Direction::Right, None)
        .from_target(MuxTarget::surface(MuxKind::Cmux, "surface:2"));
    request
        .env
        .insert("AIKIT_PROMPT".to_string(), "review carefully".to_string());
    let pane = adapter.spawn(request).unwrap();
    assert_eq!(pane.target.surface.as_deref(), Some("surface:5"));
    assert!(
        adapter
            .runner()
            .call_lines()
            .iter()
            .any(|line| line.contains("env 'AIKIT_PROMPT=review carefully' claude")),
        "the per-run environment must reach the actual cmux command: {:?}",
        adapter.runner().call_lines()
    );

    let view = cmux(full_runner());
    view.spawn(SpawnRequest::new(Placement::NewView, vec!["claude".into()]).named("review"))
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
        .on("new-window", "OK window:2")
        .sequence("new-workspace", &["OK workspace:3", "OK workspace:4"])
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
        .on("rename-tab", &fixture("ok.json"))
        .on("list-panes", &fixture("list-panes-one.json"))
        .on(
            "list-pane-surfaces",
            &fixture("list-pane-surfaces-root.json"),
        )
}
