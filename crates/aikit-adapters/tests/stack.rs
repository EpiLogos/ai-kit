//! Hybrid stacks: a tmux running inside a cmux, possibly on another machine.
//!
//! This is the configuration that quietly breaks session tools. Someone opens a
//! cmux workspace, ssh's into a build box, attaches to a tmux there, and asks for
//! "a new pane". There are two multiplexers in play and only one of them owns the
//! topology.
//!
//! The rules this file pins down:
//!
//! * topology changes go to the **innermost** mux — the one whose panes the user
//!   is actually looking at;
//! * the palette uses tmux's real popup whenever a tmux is in the stack;
//! * status may **fan out** to the outer presentation host, because that is where
//!   a workspace pill lives;
//! * `--mux` overrides detection entirely;
//! * across a remote boundary the effective registry is the remote one, and local
//!   and remote registries are never mixed.

mod common;

use common::*;

use std::sync::Arc;

use aikit_adapters::mux::cmux::Cmux;
use aikit_adapters::mux::plain::Plain;
use aikit_adapters::mux::stack::{combine_registries, EffectiveRegistry, MuxStack, RemoteBoundary};
use aikit_adapters::mux::tmux::Tmux;
use aikit_adapters::mux::{MuxTarget, SpawnRequest, StatusUpdate, UiHost};
use aikit_adapters::runner::ScriptedRunner;
use aikit_core::platform::MuxKind;
use aikit_core::session::{Direction, Placement};

fn cmux_runner() -> Arc<ScriptedRunner> {
    Arc::new(
        ScriptedRunner::new()
            .on("version", "cmux 0.63.1 (78)")
            .on(
                "capabilities",
                r#"{"version":"0.63.1","commands":["new-workspace","list-workspaces","new-window",
                    "move-workspace-to-window","new-split","workspace-action","notify",
                    "identify","select-workspace"],
                    "features":{"workspaces":true,"workspace_groups":true,"status_pill":true}}"#,
            )
            .on("identify", r#"{"workspace":{"id":"workspace:2"},"surface":{"id":"surface:7"},"host":"laptop"}"#)
            .on("new-split", r#"{"surface":{"id":"surface:99"}}"#)
            .on("new-workspace", r#"{"workspace":{"id":"workspace:42"}}"#)
            .on("workspace-action", r#"{"ok":true}"#)
            .on("notify", r#"{"ok":true}"#),
    )
}

fn tmux_runner() -> Arc<ScriptedRunner> {
    Arc::new(
        ScriptedRunner::new()
            .on("-V", "tmux 3.6a")
            .on("list-sessions", "payments: 2 windows")
            .on(
                "display-message -p #{session_name}",
                "payments\t@1\t%4\tstaging-box",
            )
            .on("split-window", "%12")
            .on("display-popup", "")
            .on("set-option", ""),
    )
}

/// A cmux presenting a tmux: both report that we are inside them.
fn hybrid() -> (MuxStack, Arc<ScriptedRunner>, Arc<ScriptedRunner>) {
    let cmux_calls = cmux_runner();
    let tmux_calls = tmux_runner();

    let cmux = Cmux::new(Arc::clone(&cmux_calls)).with_env_var("CMUX_WORKSPACE_ID", "workspace:2");
    let tmux = Tmux::new(Arc::clone(&tmux_calls)).with_env_var("TMUX", "/tmp/tmux-501/default,1,0");

    let stack = MuxStack::detect(vec![Box::new(cmux), Box::new(tmux)], None)
        .unwrap()
        .with_project("payments");
    (stack, cmux_calls, tmux_calls)
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

#[test]
fn the_innermost_multiplexer_owns_the_topology_and_the_outermost_presents_it() {
    let (stack, _, _) = hybrid();

    assert_eq!(stack.topology_kind(), MuxKind::Tmux);
    assert_eq!(stack.presentation_kind(), Some(MuxKind::Cmux));
    assert_eq!(stack.kinds(), vec![MuxKind::Tmux, MuxKind::Cmux]);
    assert!(stack.is_hybrid());
}

#[test]
fn a_plain_tmux_session_is_a_stack_of_one() {
    let tmux = Tmux::new(Arc::clone(&tmux_runner())).with_env_var("TMUX", "/tmp/tmux-501/default");
    let stack = MuxStack::detect(vec![Box::new(tmux)], None).unwrap();

    assert_eq!(stack.topology_kind(), MuxKind::Tmux);
    assert_eq!(stack.presentation_kind(), None);
    assert!(!stack.is_hybrid());
}

#[test]
fn nothing_detected_falls_back_to_a_plain_terminal_rather_than_to_nothing() {
    let cmux = Cmux::new(Arc::clone(&cmux_runner()));
    let tmux = Tmux::new(Arc::clone(&tmux_runner()));
    let stack = MuxStack::detect(vec![Box::new(cmux), Box::new(tmux), Box::new(Plain::new())], None)
        .unwrap();

    assert_eq!(
        stack.topology_kind(),
        MuxKind::Plain,
        "neither multiplexer said we were inside it"
    );
}

#[test]
fn the_mux_override_wins_over_detection() {
    // `--mux cmux` from inside a tmux is a deliberate instruction: the user is
    // telling AIKit which layer they mean.
    let cmux_calls = cmux_runner();
    let cmux = Cmux::new(Arc::clone(&cmux_calls)).with_env_var("CMUX_WORKSPACE_ID", "workspace:2");
    let tmux = Tmux::new(Arc::clone(&tmux_runner())).with_env_var("TMUX", "/tmp/t");

    let stack = MuxStack::detect(vec![Box::new(cmux), Box::new(tmux)], Some(MuxKind::Cmux)).unwrap();

    assert_eq!(stack.topology_kind(), MuxKind::Cmux);
    assert!(
        !stack.is_hybrid(),
        "an explicit choice collapses the stack to the layer that was chosen"
    );
}

#[test]
fn an_override_naming_a_multiplexer_that_is_not_here_is_refused() {
    let tmux = Tmux::new(Arc::clone(&tmux_runner())).with_env_var("TMUX", "/tmp/t");
    let error = MuxStack::detect(vec![Box::new(tmux)], Some(MuxKind::Cmux)).unwrap_err();

    assert_eq!(error.code(), "mux.not_available");
    assert!(error.message().contains("cmux"));
}

// ---------------------------------------------------------------------------
// Topology goes inwards
// ---------------------------------------------------------------------------

#[test]
fn open_in_new_pane_inside_a_tmux_inside_cmux_splits_the_tmux_pane() {
    // The point of the whole module. A cmux workspace split here would put the
    // new pane in a different place from the one the user is looking at.
    let (stack, cmux_calls, tmux_calls) = hybrid();

    let spawned = stack
        .spawn(
            SpawnRequest::new(Placement::NewPane, vec!["claude".into()])
                .splitting(Direction::Right, Some(0.4))
                .from_target(MuxTarget::surface(MuxKind::Tmux, "%4")),
        )
        .unwrap();

    assert_eq!(spawned.target.kind, MuxKind::Tmux);
    assert_eq!(spawned.target.surface.as_deref(), Some("%12"));

    assert!(
        tmux_calls
            .call_lines()
            .iter()
            .any(|l| l.contains("split-window -t %4 -h -l 40%")),
        "the tmux pane was not split: {:?}",
        tmux_calls.call_lines()
    );
    assert!(
        !cmux_calls.call_lines().iter().any(|l| l.contains("new-split")),
        "cmux must not be asked to split anything: {:?}",
        cmux_calls.call_lines()
    );
}

#[test]
fn the_topology_adapter_is_the_one_reached_for_focus_and_close_too() {
    let (stack, cmux_calls, tmux_calls) = hybrid();
    let _ = stack.close(&MuxTarget::surface(MuxKind::Tmux, "%9"));

    assert!(
        !cmux_calls.call_lines().iter().any(|l| l.contains("close-surface")),
        "closing a tmux pane is not a cmux operation"
    );
    // The tmux runner has no recorded `kill-pane`, which is itself the proof that
    // the call went there: an unscripted command is an error, not a silent pass.
    assert!(tmux_calls
        .call_lines()
        .iter()
        .any(|l| l.contains("kill-pane")));
}

// ---------------------------------------------------------------------------
// The palette goes to the real popup
// ---------------------------------------------------------------------------

#[test]
fn the_palette_uses_the_tmux_popup_when_a_tmux_is_anywhere_in_the_stack() {
    let (stack, cmux_calls, tmux_calls) = hybrid();
    let host = stack
        .open_palette(
            aikit_adapters::mux::PaletteRequest::default()
                .running(vec!["aikit".into(), "palette".into()]),
        )
        .unwrap();

    assert!(matches!(host, UiHost::TruePopup { .. }));
    assert!(tmux_calls
        .call_lines()
        .iter()
        .any(|l| l.contains("display-popup -E -w 82% -h 70% -T AIKit")));
    assert!(
        !cmux_calls.call_lines().iter().any(|l| l.contains("new-split")),
        "a cmux surface must not be created when a real popup is available"
    );
}

#[test]
fn a_cmux_only_stack_hosts_the_palette_inline() {
    let cmux = Cmux::new(Arc::clone(&cmux_runner())).with_env_var("CMUX_WORKSPACE_ID", "workspace:2");
    let stack = MuxStack::detect(vec![Box::new(cmux)], None).unwrap();

    assert_eq!(
        stack
            .open_palette(aikit_adapters::mux::PaletteRequest::default())
            .unwrap(),
        UiHost::InlineCurrentTerminal
    );
}

// ---------------------------------------------------------------------------
// Status fans outwards
// ---------------------------------------------------------------------------

#[test]
fn status_reaches_both_the_inner_and_the_outer_host() {
    let (stack, cmux_calls, tmux_calls) = hybrid();
    let deliveries = stack
        .set_status(
            StatusUpdate::for_session("payments")
                .with("profile", "rust-review"),
        )
        .unwrap();

    assert_eq!(deliveries.len(), 2);
    assert!(deliveries.iter().all(|d| d.delivered), "{deliveries:?}");
    assert!(tmux_calls
        .call_lines()
        .iter()
        .any(|l| l.contains("set-option -t payments @aikit_profile rust-review")));
    assert!(
        cmux_calls
            .call_lines()
            .iter()
            .any(|l| l.contains("workspace-action")),
        "the workspace pill is the outer host's job and it should get the news too"
    );
}

#[test]
fn a_layer_with_no_status_surface_is_reported_as_undelivered_rather_than_skipped() {
    let tmux = Tmux::new(Arc::clone(&tmux_runner())).with_env_var("TMUX", "/tmp/t");
    let plain = Plain::new();
    let stack = MuxStack::detect(vec![Box::new(plain), Box::new(tmux)], None).unwrap();

    let deliveries = stack
        .set_status(StatusUpdate::for_session("payments").with("profile", "rust-review"))
        .unwrap();

    let plain_delivery = deliveries
        .iter()
        .find(|d| d.kind == MuxKind::Plain)
        .expect("every layer is accounted for");
    assert!(!plain_delivery.delivered);
    assert!(plain_delivery.note.is_some());
}

// ---------------------------------------------------------------------------
// The remote boundary
// ---------------------------------------------------------------------------

#[test]
fn a_remote_inner_mux_renders_the_host_in_the_location_line() {
    let (stack, _, _) = hybrid();
    let remote = stack.across(RemoteBoundary::new("staging-box"));

    assert_eq!(
        remote.describe_location(),
        "payments · staging-box · tmux · presented by cmux"
    );
}

#[test]
fn a_local_hybrid_stack_still_names_the_presentation_host() {
    let (stack, _, _) = hybrid();
    assert_eq!(stack.describe_location(), "payments · tmux · presented by cmux");
}

#[test]
fn a_single_local_mux_says_the_least_it_can_get_away_with() {
    let tmux = Tmux::new(Arc::clone(&tmux_runner())).with_env_var("TMUX", "/tmp/t");
    let stack = MuxStack::detect(vec![Box::new(tmux)], None)
        .unwrap()
        .with_project("payments");

    assert_eq!(stack.describe_location(), "payments · tmux");
}

#[test]
fn the_effective_registry_across_a_remote_boundary_is_the_remote_one() {
    let (stack, _, _) = hybrid();
    assert_eq!(stack.effective_registry(), EffectiveRegistry::Local);

    let remote = stack.across(RemoteBoundary::new("staging-box"));
    assert_eq!(
        remote.effective_registry(),
        EffectiveRegistry::Remote {
            host: "staging-box".into()
        }
    );
    assert!(remote.effective_registry().is_remote());
    assert!(remote.effective_registry().describe().contains("staging-box"));
}

#[test]
fn a_local_and_a_remote_registry_are_never_combined() {
    let local = EffectiveRegistry::Local;
    let remote = EffectiveRegistry::Remote {
        host: "staging-box".into(),
    };

    let error = combine_registries(&local, &remote).unwrap_err();
    assert_eq!(error.code(), "registry.cross_host_mix");
    assert!(
        error.message().contains("staging-box"),
        "the message must name the host that would have been mixed in: {}",
        error.message()
    );

    // Two views of the same place combine perfectly happily.
    assert_eq!(combine_registries(&local, &local).unwrap(), local);
    assert_eq!(combine_registries(&remote, &remote).unwrap(), remote);
    assert_eq!(
        combine_registries(
            &remote,
            &EffectiveRegistry::Remote {
                host: "other-box".into()
            }
        )
        .unwrap_err()
        .code(),
        "registry.cross_host_mix"
    );
}

#[test]
fn a_remote_stack_says_plainly_that_the_local_registry_is_not_in_play() {
    let (stack, _, _) = hybrid();
    assert!(
        stack.warnings().is_empty(),
        "a local stack has nothing to warn about"
    );

    let remote = stack.across(RemoteBoundary::new("staging-box"));
    assert!(
        remote
            .warnings()
            .iter()
            .any(|w| w.contains("staging-box") && w.contains("registr")),
        "got: {:?}",
        remote.warnings()
    );
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

#[test]
fn ensuring_a_session_targets_the_innermost_mux() {
    let tmux_calls = Arc::new(
        ScriptedRunner::new()
            .on("-V", "tmux 3.6a")
            .on("list-sessions", "payments")
            .failing("has-session", 1, "no session")
            .on("new-session", "%0")
            .on("set-environment", "")
            .on("set-option", "")
            .on("select-window", ""),
    );
    let cmux_calls = cmux_runner();

    let cmux = Cmux::new(Arc::clone(&cmux_calls)).with_env_var("CMUX_WORKSPACE_ID", "workspace:2");
    let tmux = Tmux::new(Arc::clone(&tmux_calls)).with_env_var("TMUX", "/tmp/t");
    let stack = MuxStack::detect(vec![Box::new(cmux), Box::new(tmux)], None).unwrap();

    let binding = stack
        .ensure_session(
            &single_pane_plan("payments"),
            aikit_adapters::mux::ReconcileMode::default(),
        )
        .unwrap();

    assert_eq!(binding.kind, Some(MuxKind::Tmux));
    assert!(
        !cmux_calls
            .call_lines()
            .iter()
            .any(|l| l.contains("new-workspace")),
        "the outer host must not build a second copy of the session"
    );
}
