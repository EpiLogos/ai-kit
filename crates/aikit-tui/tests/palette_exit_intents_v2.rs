//! W7: credential setup and `doctor` repair are reachable from the palette by
//! leaving it to run the interactive flow on the restored terminal — the
//! launcher idiom. The reducer records the intent; the surface maps it to a
//! `PaletteOutcome` the CLI runs.

use aikit_core::resource::ResourceRef;
use aikit_tui::application::{
    reduce_tui, ActivationIntent, ExitIntent, TuiState, UiAction,
};

#[test]
fn credential_setup_requests_a_clean_exit_carrying_its_intent() {
    let reduction = reduce_tui(TuiState::default(), UiAction::RequestCredentialSetup);
    assert!(reduction.state.exit_requested);
    assert_eq!(reduction.state.exit_intent, Some(ExitIntent::CredentialSetup));
}

#[test]
fn doctor_repair_requests_a_clean_exit_carrying_its_intent() {
    let reduction = reduce_tui(TuiState::default(), UiAction::RequestDoctorFix);
    assert!(reduction.state.exit_requested);
    assert_eq!(reduction.state.exit_intent, Some(ExitIntent::DoctorFix));
}

/// Staged composition changes are not silently lost: leaving for an interactive
/// flow is refused with the same guard `Exit` uses, and says why.
#[test]
fn staged_changes_block_leaving_for_an_interactive_flow() {
    let mut state = TuiState::default();
    state
        .staged
        .stage(ResourceRef::parse("capability:x").unwrap(), ActivationIntent::Enable);

    let reduction = reduce_tui(state, UiAction::RequestCredentialSetup);
    assert!(!reduction.state.exit_requested);
    assert!(reduction.state.exit_intent.is_none());
    assert!(reduction
        .state
        .status
        .expect("a refusal says why")
        .message
        .contains("staged change"));
}
