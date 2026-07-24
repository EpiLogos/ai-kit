//! The reducer.
//!
//! `event → Action → reduce → AppState → render`, with effects returning actions.
//! [`aikit_tui::reduce`] is a pure function, which is why almost everything the
//! palette does is testable here rather than through a terminal. Every keybinding
//! in the specification has a test in this file, and so does every rule that
//! would otherwise only be visible as a drawn frame: that `Space` stages without
//! applying, that a project-scope apply cannot skip its confirmation, that a
//! failed apply leaves the staged set intact so the user can recover.

mod common;

use common::*;

use aikit_core::capsule::ExecMode;
use aikit_core::scope::ScopeKind;
use aikit_core::trust::TrustState;
use aikit_tui::app::{Action, AppState, ConfirmKind, Effect, Level, Mode};
use aikit_tui::backend::JobOutput;
use aikit_tui::driver::step;
use aikit_tui::host::UiHost;
use aikit_tui::{reduce, PaletteOutcome, PaletteRequest};

fn request() -> PaletteRequest {
    PaletteRequest::new(UiHost::Inline(16))
}

/// Open a palette over a fixture and settle every start-up effect.
fn open(backend: &mut Fixture) -> AppState {
    let (state, effects) = AppState::open(&*backend, &request()).expect("the palette must open");
    aikit_tui::driver::settle(backend, state, effects)
}

fn type_query(backend: &mut Fixture, mut state: AppState, text: &str) -> AppState {
    for c in text.chars() {
        state = step(backend, state, Action::Input(c));
    }
    state
}

fn selected(state: &AppState) -> String {
    state
        .selected_row()
        .map(|row| row.doc.id.to_string())
        .unwrap_or_default()
}

fn catalog() -> Vec<aikit_core::capsule::Capsule> {
    vec![
        script_exporting("script/test/cargo-nextest", &["nt"]),
        script("script/ops/deploy"),
        requiring("script", "script/app/uses-lib", &["script/lib/core"]),
        script("script/lib/core"),
        skill("skill/rust/review"),
    ]
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

#[test]
fn a_freshly_opened_palette_is_searching_with_the_contexts_own_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    assert_eq!(state.mode, Mode::Search);
    assert_eq!(state.scope.current(), ScopeKind::Session);
    assert!(!state.rows.is_empty(), "the list is populated before the first keystroke");
    assert_eq!(state.cursor, 0);
    assert!(state.outcome.is_none());
}

#[test]
fn an_initial_query_is_applied_before_the_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let (state, effects) = AppState::open(&fixture, &request().with_query("deploy")).unwrap();
    let state = aikit_tui::driver::settle(&mut fixture, state, effects);

    assert_eq!(state.query, "deploy");
    assert_eq!(selected(&state), "script/ops/deploy");
}

#[test]
fn a_requested_scope_the_context_cannot_offer_refuses_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path(), catalog())
        .with_descriptor(aikit_core::context::ContextDescriptor {
            project_root: None,
            session_id: None,
            task: None,
            host: String::new(),
            ..descriptor()
        });
    let error = AppState::open(&fixture, &request().with_scope(ScopeKind::Project)).unwrap_err();
    assert_eq!(error.code(), "scope.unavailable_in_context");
}

// ---------------------------------------------------------------------------
// Typing and moving
// ---------------------------------------------------------------------------

#[test]
fn input_extends_the_query_and_asks_for_a_new_ranking() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    let reduction = reduce(state, Action::Input('d'));
    assert_eq!(reduction.state.query, "d");
    assert!(
        reduction.effects.iter().any(|e| matches!(e, Effect::Search { .. })),
        "matching must be requested as an effect, not done in the reducer"
    );
}

#[test]
fn backspace_shortens_the_query_and_is_harmless_when_it_is_already_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "dep");
    state = step(&mut fixture, state, Action::Backspace);
    assert_eq!(state.query, "de");

    for _ in 0..5 {
        state = step(&mut fixture, state, Action::Backspace);
    }
    assert_eq!(state.query, "");
    assert!(!state.rows.is_empty());
}

#[test]
fn narrowing_the_query_puts_the_cursor_back_on_a_row_that_exists() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::MoveDown);
    state = step(&mut fixture, state, Action::MoveDown);
    assert_eq!(state.cursor, 2);

    state = type_query(&mut fixture, state, "cargo-nextest");
    assert!(state.cursor < state.rows.len(), "the cursor is off the end of the list");
    assert_eq!(selected(&state), "script/test/cargo-nextest");
}

#[test]
fn the_cursor_stops_at_both_ends_rather_than_wrapping_under_the_users_hand() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);

    state = step(&mut fixture, state, Action::MoveUp);
    assert_eq!(state.cursor, 0, "up at the top must not jump to the bottom");

    for _ in 0..50 {
        state = step(&mut fixture, state, Action::MoveDown);
    }
    assert_eq!(state.cursor, state.rows.len() - 1);
}

// ---------------------------------------------------------------------------
// Tab: the mutation scope
// ---------------------------------------------------------------------------

#[test]
fn tab_moves_the_mutation_scope_through_the_permitted_ones_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    let permitted = state.scope.permitted().to_vec();

    for _ in 0..permitted.len() * 2 {
        state = step(&mut fixture, state, Action::Tab);
        assert!(permitted.contains(&state.scope.current()));
    }
}

#[test]
fn changing_scope_with_something_staged_recomputes_the_consequences() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "uses-lib");
    state = step(&mut fixture, state, Action::Space);
    assert!(state.staged_diff().is_some());

    let reduction = reduce(state, Action::Tab);
    assert!(
        reduction.effects.iter().any(|e| matches!(e, Effect::Stage)),
        "the same toggle at a different scope can resolve differently"
    );
}

// ---------------------------------------------------------------------------
// Space: staging
// ---------------------------------------------------------------------------

#[test]
fn space_stages_a_change_and_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let before = fixture.overlay_bytes();
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);

    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.mode, Mode::Search, "staging does not leave the list");
    assert!(fixture.applied.is_empty());
    assert_eq!(fixture.overlay_bytes(), before);
}

#[test]
fn the_footer_reports_the_consequences_the_backend_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "uses-lib");
    state = step(&mut fixture, state, Action::Space);

    let diff = state.staged_diff().expect("the set resolves");
    assert_eq!(diff.footer(), "1 staged change · +1 dependency");
}

#[test]
fn a_second_space_on_the_same_row_removes_the_staging() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);
    state = step(&mut fixture, state, Action::Space);
    assert!(state.staged.is_empty());
    assert!(state.staged_outcome.is_none(), "nothing staged means nothing to report");
}

#[test]
fn a_staged_set_that_cannot_resolve_is_reported_without_being_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .enable(ScopeKind::Project, &["script/app/uses-lib"]);
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "lib/core");
    state = step(&mut fixture, state, Action::Space);

    let problem = state.staged_problem().expect("this cannot resolve");
    assert_eq!(problem.code(), "resolution.required_capability_disabled");
    assert_eq!(state.mode, Mode::StagedDiff, "a refusal must be shown, not buried");
    assert!(fixture.applied.is_empty());
}

// ---------------------------------------------------------------------------
// Ctrl+Enter: apply
// ---------------------------------------------------------------------------

#[test]
fn ctrl_enter_with_nothing_staged_says_so_instead_of_applying_an_empty_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);
    let state = step(&mut fixture, state, Action::CtrlEnter);

    assert!(fixture.applied.is_empty());
    assert_eq!(state.status.as_ref().map(|s| s.level), Some(Level::Info));
}

#[test]
fn ctrl_enter_at_a_cheap_scope_applies_the_whole_staged_graph_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "uses-lib");
    state = step(&mut fixture, state, Action::Space);
    state = type_query(&mut fixture, state, "");
    state = step(&mut fixture, state, Action::CtrlEnter);

    assert_eq!(fixture.applied.len(), 1);
    assert!(matches!(state.outcome, Some(PaletteOutcome::Applied(_))));
}

#[test]
fn ctrl_enter_at_the_project_scope_stops_at_a_confirmation_that_cannot_be_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);
    while state.scope.current() != ScopeKind::Project {
        state = step(&mut fixture, state, Action::Tab);
    }
    state = step(&mut fixture, state, Action::CtrlEnter);

    assert_eq!(state.mode, Mode::Confirm);
    assert!(fixture.applied.is_empty(), "the confirmation was skipped");
    assert!(matches!(
        state.confirm.as_ref().map(|c| c.kind.clone()),
        Some(ConfirmKind::WriteScope(ScopeKind::Project))
    ));

    // Nothing except a deliberate Enter gets past it.
    for action in [
        Action::Space,
        Action::Input('y'),
        Action::CtrlEnter,
        Action::Tab,
        Action::MoveDown,
    ] {
        state = step(&mut fixture, state, action);
        assert_eq!(state.mode, Mode::Confirm, "a key got past the confirmation");
        assert!(fixture.applied.is_empty());
    }

    state = step(&mut fixture, state, Action::Enter);
    assert_eq!(fixture.applied.len(), 1);
    assert_eq!(fixture.applied[0].0, ScopeKind::Project);
    assert!(matches!(state.outcome, Some(PaletteOutcome::Applied(_))));
}

#[test]
fn escaping_a_scope_confirmation_keeps_the_staged_set_for_a_second_try() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);
    while state.scope.current() != ScopeKind::Project {
        state = step(&mut fixture, state, Action::Tab);
    }
    state = step(&mut fixture, state, Action::CtrlEnter);
    state = step(&mut fixture, state, Action::Esc);

    assert_ne!(state.mode, Mode::Confirm);
    assert_eq!(state.staged.len(), 1, "escaping a confirmation must not discard work");
    assert!(state.outcome.is_none(), "Esc out of a dialog does not close the palette");
}

#[test]
fn an_apply_that_fails_leaves_the_staged_set_and_names_the_error_code() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);

    // Arrive at the failure through the reducer's own action, which is how the
    // effect layer reports it.
    let reduction = reduce(
        state,
        Action::ApplyFinished(Err(aikit_core::AikitError::new(
            "generation.stale_base",
            "the session overlay moved under this apply",
        ))),
    );
    let status = reduction.state.status.expect("a failure must be shown");
    assert_eq!(status.level, Level::Error);
    assert_eq!(status.code, Some("generation.stale_base"));
    assert!(reduction.state.outcome.is_none(), "a failed apply does not close the palette");
}

// ---------------------------------------------------------------------------
// Enter: opening and running
// ---------------------------------------------------------------------------

#[test]
fn enter_on_a_script_with_no_arguments_hands_a_foreground_run_back_to_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Enter);

    let Some(PaletteOutcome::Run(intent)) = state.outcome.clone() else {
        panic!("expected a run intent, got {:?}", state.outcome);
    };
    assert_eq!(intent.capsule.to_string(), "script/ops/deploy");
    assert_eq!(intent.mode, ExecMode::Foreground);
    assert!(
        intent.mode.releases_terminal(),
        "a foreground run needs the terminal the palette is holding"
    );
}

#[test]
fn enter_on_a_script_with_arguments_opens_the_form_rather_than_running_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            "\n[[args]]\nname = \"target\"\ntype = \"string\"\nposition = 1\n",
            "entry = \"payload/run.sh\"",
        )],
    );
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);

    assert_eq!(state.mode, Mode::ArgForm);
    assert_eq!(state.form.as_ref().map(|f| f.fields().len()), Some(1));
    assert!(state.outcome.is_none(), "opening a form does not run anything");
}

#[test]
fn a_captured_run_stays_in_the_palette_and_shows_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/status",
            "",
            "entry = \"payload/run.sh\"\nmode = \"capture\"",
        )],
    )
    .with_job(JobOutput {
        capsule: Some(cid("script/ops/status")),
        status: Some(0),
        lines: vec!["all good".into()],
        truncated: false,
    });

    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);

    assert_eq!(state.mode, Mode::JobOutput);
    assert!(state.outcome.is_none(), "a captured run does not close the palette");
    assert_eq!(
        state.job.as_ref().map(|j| j.lines.clone()),
        Some(vec!["all good".to_string()])
    );
}

#[test]
fn enter_on_a_capability_that_cannot_be_run_explains_it_instead() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "review");
    state = step(&mut fixture, state, Action::Enter);

    assert_eq!(state.mode, Mode::Preview);
    assert!(state.outcome.is_none());
    let explanation = state.explanation().expect("a preview explains the row");
    assert_eq!(explanation.id.to_string(), "skill/rust/review");
    assert!(!explanation.activation_meaning.is_empty());
}

#[test]
fn running_an_unreviewed_script_stops_at_a_trust_confirmation_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog())
        .set_trust("script/ops/deploy", TrustState::Unseen);
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Enter);

    assert_eq!(state.mode, Mode::Confirm);
    assert!(matches!(
        state.confirm.as_ref().map(|c| c.kind.clone()),
        Some(ConfirmKind::RunUnreviewed(_))
    ));
    assert!(state.outcome.is_none(), "an unreviewed script does not just run");

    state = step(&mut fixture, state, Action::Enter);
    assert!(matches!(state.outcome, Some(PaletteOutcome::Run(_))));
}

// ---------------------------------------------------------------------------
// Alt+Enter, Ctrl+O, Ctrl+R, Shift+Enter
// ---------------------------------------------------------------------------

#[test]
fn alt_enter_asks_the_multiplexer_for_a_new_pane_instead_of_taking_the_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::AltEnter);

    let Some(PaletteOutcome::Run(intent)) = state.outcome.clone() else {
        panic!("expected a run intent, got {:?}", state.outcome);
    };
    assert_eq!(intent.mode, ExecMode::NewPane);
    assert!(intent.mode.needs_mux());
}

#[test]
fn ctrl_o_reveals_where_the_selected_capability_actually_lives() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::CtrlO);

    let status = state.status.expect("Ctrl+O reports what it did");
    assert_eq!(status.level, Level::Info);
    assert!(
        status.message.contains("script-ops-deploy"),
        "the path must be named: {}",
        status.message
    );
}

#[test]
fn ctrl_r_repeats_the_most_recent_run_without_replaying_a_secret() {
    let dir = tempfile::tempdir().unwrap();
    let capsule = manifest(
        "script",
        "script/ops/release",
        "\n[[args]]\nname = \"target\"\ntype = \"string\"\nposition = 1\n",
        "entry = \"payload/run.sh\"",
    );
    let form = aikit_tui::form::ArgForm::new(&capsule, &aikit_tui::form::FormContext::from_descriptor(&descriptor()));
    let mut form = form;
    form.set_input(0, "production");
    let intent = form.intent(&capsule, &descriptor()).unwrap();

    let mut fixture = Fixture::new(dir.path(), vec![capsule]).with_recent(vec![intent]);
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::CtrlR);

    let Some(PaletteOutcome::Run(intent)) = state.outcome.clone() else {
        panic!("expected the recent run, got {:?}", state.outcome);
    };
    assert_eq!(intent.argv().unwrap(), vec!["production"]);
    assert!(!intent.has_secrets());
}

#[test]
fn ctrl_r_with_no_history_says_so_rather_than_doing_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let state = open(&mut fixture);
    let state = step(&mut fixture, state, Action::CtrlR);
    assert!(state.outcome.is_none());
    assert_eq!(state.status.as_ref().map(|s| s.level), Some(Level::Info));
}

#[test]
fn shift_enter_shows_the_staged_set_when_there_is_one_and_the_row_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);

    state = step(&mut fixture, state, Action::ShiftEnter);
    assert_eq!(state.mode, Mode::Preview, "with nothing staged, explain the row");

    state = step(&mut fixture, state, Action::Esc);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);
    state = step(&mut fixture, state, Action::ShiftEnter);
    assert_eq!(state.mode, Mode::StagedDiff);
}

// ---------------------------------------------------------------------------
// Esc and Help
// ---------------------------------------------------------------------------

#[test]
fn esc_from_the_list_closes_the_palette_and_esc_from_a_mode_only_leaves_the_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);

    state = step(&mut fixture, state, Action::ShiftEnter);
    assert_eq!(state.mode, Mode::Preview);
    state = step(&mut fixture, state, Action::Esc);
    assert_eq!(state.mode, Mode::Search);
    assert!(state.outcome.is_none());

    state = step(&mut fixture, state, Action::Esc);
    assert_eq!(state.outcome, Some(PaletteOutcome::Closed));
}

#[test]
fn esc_clears_a_query_before_it_closes_the_palette() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");

    state = step(&mut fixture, state, Action::Esc);
    assert_eq!(state.query, "");
    assert!(state.outcome.is_none(), "the first Esc undoes the query");

    state = step(&mut fixture, state, Action::Esc);
    assert_eq!(state.outcome, Some(PaletteOutcome::Closed));
}

#[test]
fn help_opens_over_whatever_was_there_and_returns_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog());
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, "deploy");
    state = step(&mut fixture, state, Action::Space);
    state = step(&mut fixture, state, Action::ShiftEnter);
    assert_eq!(state.mode, Mode::StagedDiff);

    state = step(&mut fixture, state, Action::Help);
    assert_eq!(state.mode, Mode::Help);
    state = step(&mut fixture, state, Action::Esc);
    assert_eq!(state.mode, Mode::StagedDiff, "help must not lose the user's place");
    assert_eq!(state.staged.len(), 1);
}

// ---------------------------------------------------------------------------
// The argument form, through the reducer
// ---------------------------------------------------------------------------

#[test]
fn typing_in_a_form_edits_the_field_and_not_the_query() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            "\n[[args]]\nname = \"target\"\ntype = \"string\"\nposition = 1\n",
            "entry = \"payload/run.sh\"",
        )],
    );
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);
    state = type_query(&mut fixture, state, "prod");

    assert_eq!(state.query, "", "the query box is not the focused field");
    assert_eq!(state.form.as_ref().unwrap().fields()[0].input(), "prod");
}

#[test]
fn enter_on_an_incomplete_form_reports_the_field_rather_than_running() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            "\n[[args]]\nname = \"target\"\ntype = \"string\"\nposition = 1\n",
            "entry = \"payload/run.sh\"",
        )],
    );
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);
    state = step(&mut fixture, state, Action::Enter);

    assert_eq!(state.mode, Mode::ArgForm);
    assert!(state.outcome.is_none());
    assert!(state.form.as_ref().unwrap().fields()[0].error().is_some());
}

#[test]
fn a_completed_form_produces_the_run_the_manifest_describes() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            "\n[[args]]\nname = \"target\"\ntype = \"string\"\nposition = 1\n",
            "entry = \"payload/run.sh\"",
        )],
    );
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);
    state = type_query(&mut fixture, state, "production");
    state = step(&mut fixture, state, Action::Enter);

    let Some(PaletteOutcome::Run(intent)) = state.outcome.clone() else {
        panic!("expected a run intent, got {:?}", state.outcome);
    };
    assert_eq!(intent.argv().unwrap(), vec!["production"]);
}

#[test]
fn space_in_a_form_activates_the_field_instead_of_staging_a_capability() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![manifest(
            "script",
            "script/ops/release",
            "\n[[args]]\nname = \"changed\"\ntype = \"bool\"\nflag = \"--changed\"\n",
            "entry = \"payload/run.sh\"",
        )],
    );
    let mut state = open(&mut fixture);
    state = step(&mut fixture, state, Action::Enter);
    state = step(&mut fixture, state, Action::Space);

    assert_eq!(state.form.as_ref().unwrap().fields()[0].input(), "true");
    assert!(state.staged.is_empty(), "Space in a form must not stage a capability");
}

// ---------------------------------------------------------------------------
// Promotion
// ---------------------------------------------------------------------------

#[test]
fn the_manage_lane_offers_the_captures_waiting_for_review() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog()).with_drafts(vec![ready_draft()]);
    let mut state = open(&mut fixture);
    state = type_query(&mut fixture, state, ":");

    assert!(
        state.manage_rows().iter().any(|a| a.label().contains("capture")),
        "the manage lane must offer the inbox"
    );
}

#[test]
fn a_ready_capture_can_be_promoted_and_the_palette_says_what_it_produced() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog()).with_drafts(vec![ready_draft()]);
    let mut state = open(&mut fixture);
    state = enter_promotion(&mut fixture, state);
    assert_eq!(state.mode, Mode::Promotion);

    state = step(&mut fixture, state, Action::Enter);
    assert_eq!(state.mode, Mode::Confirm);
    state = step(&mut fixture, state, Action::Enter);

    assert_eq!(fixture.promoted.len(), 1);
    assert!(matches!(state.outcome, Some(PaletteOutcome::Promoted(_))));
}

#[test]
fn a_quarantined_capture_is_refused_and_its_body_never_reaches_the_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path(), catalog()).with_drafts(vec![quarantined_draft()]);
    let mut state = open(&mut fixture);
    state = enter_promotion(&mut fixture, state);

    let draft = state.promotion_draft().expect("a draft is selected");
    assert!(draft.withheld_reason().is_some());
    assert!(
        draft.body().is_empty(),
        "a quarantined body must never be held for display"
    );

    state = step(&mut fixture, state, Action::Enter);
    assert_ne!(state.mode, Mode::Confirm, "a quarantined capture is not promotable");
    assert_eq!(state.status.as_ref().map(|s| s.level), Some(Level::Error));
    assert!(fixture.promoted.is_empty());
}
