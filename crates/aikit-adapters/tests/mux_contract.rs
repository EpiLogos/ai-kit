//! The multiplexer contract, independent of any multiplexer.
//!
//! Two of these assertions are load-bearing policy rather than plumbing:
//!
//! * [`MuxCapabilities`] defaults to all-false, so an adapter that forgets to
//!   describe itself is treated as the least capable thing rather than the most.
//! * [`ReconcileMode`] defaults to `CreateOrAttach`, which never destroys
//!   anything. `session up` run twice in a session someone has been working in
//!   must not be able to close their panes.

use aikit_adapters::mux::{
    MuxCapabilities, MuxLocation, MuxPresence, MuxTarget, Notification, NotificationLevel,
    PaletteRequest, ReconcileMode, StatusUpdate, UiHost,
};
use aikit_core::platform::MuxKind;

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

#[test]
fn a_multiplexer_that_forgets_to_describe_itself_is_assumed_to_do_nothing() {
    let caps = MuxCapabilities::default();
    assert!(!caps.true_popup);
    assert!(!caps.workspaces);
    assert!(!caps.workspace_groups);
    assert!(!caps.windows);
    assert!(!caps.panes);
    assert!(!caps.browser_surface);
    assert!(!caps.status_metadata);
    assert!(!caps.notifications);
    assert!(!caps.remote_control);
}

#[test]
fn a_multiplexer_without_a_popup_hosts_the_palette_inline() {
    let caps = MuxCapabilities {
        panes: true,
        ..MuxCapabilities::default()
    };
    assert_eq!(caps.default_palette_host(), UiHost::InlineCurrentTerminal);

    let with_popup = MuxCapabilities {
        true_popup: true,
        ..caps
    };
    assert!(matches!(
        with_popup.default_palette_host(),
        UiHost::TruePopup { .. }
    ));
}

// ---------------------------------------------------------------------------
// Reconciliation
// ---------------------------------------------------------------------------

#[test]
fn the_default_reconcile_mode_is_the_non_destructive_one() {
    assert_eq!(ReconcileMode::default(), ReconcileMode::CreateOrAttach);
    assert!(!ReconcileMode::CreateOrAttach.may_close_panes());
    assert!(ReconcileMode::Exact.may_close_panes());
}

// ---------------------------------------------------------------------------
// Targets and locations
// ---------------------------------------------------------------------------

#[test]
fn a_target_addresses_the_most_specific_thing_it_knows() {
    let pane = MuxTarget::surface(MuxKind::Tmux, "%7");
    assert_eq!(pane.selector(), "%7");

    let view = MuxTarget {
        kind: MuxKind::Tmux,
        session: Some("payments".into()),
        view: Some("@3".into()),
        surface: None,
    };
    assert_eq!(view.selector(), "@3");

    let session = MuxTarget::session(MuxKind::Tmux, "payments");
    assert_eq!(session.selector(), "payments");
}

#[test]
fn a_target_with_nothing_in_it_has_no_selector_rather_than_an_empty_one() {
    let nothing = MuxTarget {
        kind: MuxKind::Plain,
        session: None,
        view: None,
        surface: None,
    };
    assert_eq!(nothing.selector(), "");
    assert!(!nothing.is_addressable());
    assert!(MuxTarget::surface(MuxKind::Tmux, "%1").is_addressable());
}

#[test]
fn a_location_renders_the_line_the_palette_title_bar_shows() {
    let local = MuxLocation {
        kind: MuxKind::Tmux,
        host: "localhost".into(),
        remote: false,
        project: Some("payments".into()),
        session: Some("payments".into()),
        view: Some("@0".into()),
        surface: Some("%0".into()),
    };
    assert_eq!(local.describe(), "payments · tmux");

    let remote = MuxLocation {
        host: "staging-box".into(),
        remote: true,
        ..local
    };
    assert_eq!(
        remote.describe(),
        "payments · staging-box · tmux",
        "a remote host has to be visible without being asked for"
    );
}

// ---------------------------------------------------------------------------
// Presence
// ---------------------------------------------------------------------------

#[test]
fn an_absent_multiplexer_carries_the_reason_it_is_absent() {
    let absent = MuxPresence::absent(MuxKind::Cmux, "`cmux` is not on PATH");
    assert!(!absent.installed);
    assert!(!absent.is_usable());
    assert_eq!(absent.detail.as_deref(), Some("`cmux` is not on PATH"));
    assert!(absent.describe().contains("not on PATH"));
}

#[test]
fn a_present_multiplexer_reports_its_version_and_whether_we_are_inside_it() {
    let inside = MuxPresence {
        kind: MuxKind::Tmux,
        installed: true,
        version: Some("3.6a".into()),
        server_running: true,
        inside: true,
        detail: None,
    };
    assert!(inside.is_usable());
    assert!(inside.describe().contains("3.6a"));
}

// ---------------------------------------------------------------------------
// Palette hosts
// ---------------------------------------------------------------------------

#[test]
fn a_palette_request_carries_the_geometry_the_popup_is_opened_with() {
    let request = PaletteRequest::default();
    assert_eq!(request.title, "AIKit");
    assert_eq!(request.width_percent, 82);
    assert_eq!(request.height_percent, 70);
    assert!(request.command.is_empty());
}

#[test]
fn every_ui_host_says_what_it_is_in_words_the_palette_can_print() {
    assert_eq!(UiHost::InlineCurrentTerminal.describe(), "inline");
    assert_eq!(
        UiHost::TruePopup {
            target: "%0".into()
        }
        .describe(),
        "popup"
    );
    assert_eq!(
        UiHost::TemporarySurface {
            id: "surface:9".into()
        }
        .describe(),
        "temporary surface surface:9"
    );
    assert_eq!(
        UiHost::Unsupported {
            reason: "no terminal".into()
        }
        .describe(),
        "unsupported — no terminal"
    );
}

// ---------------------------------------------------------------------------
// Status and notifications
// ---------------------------------------------------------------------------

#[test]
fn a_status_update_is_a_set_of_named_values_not_a_rendered_string() {
    // The multiplexer decides how to render; AIKit only supplies the facts.
    let update = StatusUpdate::for_session("payments")
        .with("profile", "rust-review")
        .with("generation", "gen_ab12");
    assert_eq!(update.session.as_deref(), Some("payments"));
    assert_eq!(update.values.get("profile").map(String::as_str), Some("rust-review"));
    assert_eq!(update.values.len(), 2);
}

#[test]
fn a_notification_defaults_to_the_least_alarming_level() {
    let n = Notification::new("Applied", "generation gen_ab12 is current");
    assert_eq!(n.level, NotificationLevel::Info);
    assert_eq!(n.title, "Applied");
}
