//! W7: the Model roster surface, previously reachable only from a CLl command,
//! is opened as an on-demand overlay from inside the running palette.

use aikit_tui::application::{reduce_tui, Overlay, TuiState, UiAction, UiEffect};

/// Opening the roster asks the backend to compose it — it is fetched on demand,
/// not carried on every world read — and does not open the overlay until the
/// roster is actually loaded.
#[test]
fn requesting_the_roster_asks_the_backend_to_load_it() {
    let reduction = reduce_tui(TuiState::default(), UiAction::RequestModelRoster);
    assert_eq!(reduction.effects, vec![UiEffect::LoadModelRoster]);
    assert_ne!(reduction.state.overlay, Some(Overlay::ModelRoster));
}

/// A loaded roster opens the overlay and carries the reading onto state.
#[test]
fn a_loaded_roster_opens_the_overlay() {
    let reduction = reduce_tui(TuiState::default(), UiAction::ModelRosterLoaded(None));
    assert_eq!(reduction.state.overlay, Some(Overlay::ModelRoster));
    // `None` (the backend could compose none — e.g. no Project here) is carried
    // through, so the overlay says so rather than showing an empty roster as if
    // no Models existed.
    assert!(reduction.state.model_roster.is_none());
}

/// Esc dismisses the overlay like any other, leaving navigation intact.
#[test]
fn esc_dismisses_the_roster_overlay() {
    let opened = reduce_tui(TuiState::default(), UiAction::ModelRosterLoaded(None)).state;
    assert_eq!(opened.overlay, Some(Overlay::ModelRoster));
    let dismissed = reduce_tui(opened, UiAction::Back);
    assert_eq!(dismissed.state.overlay, None);
}
