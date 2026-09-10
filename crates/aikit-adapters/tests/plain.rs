//! No multiplexer at all.
//!
//! The point of this adapter is that AIKit works in a bare terminal, and says so
//! plainly instead of pretending. Everything it cannot do is an `Unsupported`
//! with a sentence a person can act on, not a silent no-op.

mod common;

use common::*;

use aikit_adapters::mux::plain::Plain;
use aikit_adapters::mux::{
    MuxAdapter, MuxTarget, Notification, PaletteRequest, ReconcileMode, SpawnRequest, StatusUpdate,
    UiHost,
};
use aikit_core::platform::MuxKind;
use aikit_core::session::Placement;

#[test]
fn a_bare_terminal_is_always_present_because_it_is_where_we_already_are() {
    let plain = Plain::new();
    let presence = plain.detect().unwrap();

    assert_eq!(presence.kind, MuxKind::Plain);
    assert!(presence.installed);
    assert!(presence.is_usable());
    assert!(presence.inside, "a plain terminal is always the one we are in");
}

#[test]
fn a_bare_terminal_claims_no_capability_at_all() {
    let caps = Plain::new().capabilities();
    assert_eq!(caps, aikit_adapters::mux::MuxCapabilities::default());
}

#[test]
fn the_single_pane_is_the_current_terminal() {
    let plain = Plain::new();
    let location = plain.current_location().unwrap();

    assert_eq!(location.kind, MuxKind::Plain);
    assert_eq!(location.surface.as_deref(), Some("current-terminal"));
    assert!(location.target().is_addressable());
}

#[test]
fn ensuring_a_single_pane_session_succeeds_without_creating_anything() {
    let plain = Plain::new();
    let binding = plain
        .ensure_session(&single_pane_plan("payments"), ReconcileMode::default())
        .unwrap();

    assert!(
        !binding.created,
        "a terminal that already exists was not created by us"
    );
    assert_eq!(binding.session, "payments");
    assert_eq!(
        binding.surface_of("main", "shell"),
        Some("current-terminal")
    );
}

#[test]
fn a_multi_pane_plan_warns_about_every_pane_that_cannot_exist_here() {
    let plain = Plain::new();
    let dir = tempfile::tempdir().unwrap();
    let binding = plain
        .ensure_session(&three_pane_plan("payments", dir.path()), ReconcileMode::default())
        .unwrap();

    // Three panes across two views were asked for; one terminal is available.
    assert_eq!(binding.warnings.len(), 3, "got: {:?}", binding.warnings);
    assert!(
        binding
            .warnings
            .iter()
            .all(|w| w.contains("no multiplexer")),
        "each warning must say why, got: {:?}",
        binding.warnings
    );
    assert_eq!(binding.surfaces.len(), 1);
}

#[test]
fn asking_for_a_new_pane_is_refused_with_a_message_that_names_the_fix() {
    let plain = Plain::new();
    let error = plain
        .spawn(SpawnRequest::new(Placement::NewPane, vec!["claude".into()]))
        .unwrap_err();

    assert_eq!(error.code(), "mux.unsupported");
    let message = error.message();
    assert!(
        message.contains("tmux") || message.contains("cmux"),
        "an unsupported message that does not say what would work is just a wall: {message}"
    );
}

#[test]
fn asking_for_a_new_view_or_a_background_job_is_refused_too() {
    let plain = Plain::new();
    for placement in [Placement::NewView, Placement::Background] {
        let error = plain
            .spawn(SpawnRequest::new(placement, vec!["claude".into()]))
            .unwrap_err();
        assert_eq!(error.code(), "mux.unsupported", "for {placement:?}");
    }
}

#[test]
fn running_in_the_current_terminal_is_the_one_placement_that_works() {
    let plain = Plain::new();
    let spawned = plain
        .spawn(SpawnRequest::new(Placement::Current, vec!["claude".into()]))
        .unwrap();

    assert!(!spawned.created);
    assert_eq!(spawned.target.surface.as_deref(), Some("current-terminal"));
    assert!(
        spawned.note.is_some(),
        "the caller has to be told it must run the command itself"
    );
}

#[test]
fn the_palette_is_hosted_inline_because_there_is_nowhere_else() {
    let plain = Plain::new();
    assert_eq!(
        plain.open_palette(PaletteRequest::default()).unwrap(),
        UiHost::InlineCurrentTerminal
    );
}

#[test]
fn focus_is_a_no_op_and_closing_the_only_terminal_is_refused() {
    let plain = Plain::new();
    let terminal = MuxTarget::surface(MuxKind::Plain, "current-terminal");

    assert!(
        plain.focus(&terminal).is_ok(),
        "the current terminal is already focused; asking is not an error"
    );
    assert_eq!(
        plain.close(&terminal).unwrap_err().code(),
        "mux.unsupported",
        "AIKit must not close the terminal it is running in"
    );
}

#[test]
fn status_and_notifications_are_accepted_and_recorded_rather_than_dropped_silently() {
    let plain = Plain::new();
    plain
        .set_status(StatusUpdate::for_session("payments").with("profile", "rust-review"))
        .unwrap();
    plain
        .notify(Notification::new("Applied", "gen_ab12 is current"))
        .unwrap();

    // A plain terminal has no status surface, so the adapter keeps what it was
    // told; the caller can print it if it wants to.
    assert_eq!(
        plain.pending_status(),
        vec![("profile".to_string(), "rust-review".to_string())]
    );
    assert_eq!(plain.pending_notifications().len(), 1);
}
