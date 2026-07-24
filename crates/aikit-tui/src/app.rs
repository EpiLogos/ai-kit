//! State, actions and the reducer.
//!
//! ```text
//! event → Action → reduce → AppState → render
//!            ▲                  │
//!            └──── Effect ◄─────┘
//! ```
//!
//! [`reduce`] is pure: it takes the state by value, returns a new one and a list
//! of effects, and touches nothing else. Everything that talks to the application
//! — ranking a query, resolving a staged set, applying, running, promoting — is an
//! [`Effect`], executed by [`crate::driver`], and everything it learns comes back
//! as another [`Action`]. Nothing in this file performs I/O, so the palette's
//! entire behaviour is reachable from a test without a terminal.
//!
//! ## The rules that live here, and only here
//!
//! * `Space` stages. It never applies. The only path to a write is
//!   [`Effect::Apply`], and the only actions that emit it are `CtrlEnter` at a
//!   cheap scope and `Enter` inside a [`Mode::Confirm`] raised for that purpose.
//! * A confirmation is unskippable. In [`Mode::Confirm`] every action except
//!   `Enter` and `Esc` is inert, which is why the test that hammers it with
//!   `Space`, `y`, `Tab` and a second `Ctrl+Enter` still finds nothing applied.
//! * A failure never closes the palette and never discards the staged set. A
//!   stale-base apply, a refused resolution and a missing capsule all land in
//!   [`AppState::status`] with the error's stable code, and the user can adjust
//!   and try again.
//!
//! ## What is deliberately absent
//!
//! There is no ranking, no dependency expansion, no scope precedence and no trust
//! evaluation in this file. The reducer moves a cursor, holds a set, and decides
//! which question to ask next.

use aikit_core::capsule::{Capsule, ExecMode};
use aikit_core::context::ContextDescriptor;
use aikit_core::error::AikitError;
use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::resolve::{Explanation, ResolvedView};
use aikit_core::scope::ScopeKind;
use aikit_core::search::{parse_query, FastPrefix};
use aikit_core::Result;

use crate::backend::{JobOutput, PaletteBackend, PromotionDraft, RunIntent};
use crate::form::{ArgForm, FormContext};
use crate::host::UiHost;
use crate::layout::Glyphs;
use crate::scope::ScopeSelector;
use crate::search::Row;
use crate::staging::{is_on, StagedDiff, StagedOutcome, StagedProblem, StagedSet};
use crate::{PaletteOutcome, PaletteRequest};

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

/// What the palette is asking the user right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The list. The palette's resting state and the only one it opens in.
    Search,
    /// Why this row is in the state it is in.
    Preview,
    /// Filling in `[[args]]` before a run.
    ArgForm,
    /// The full consequences of the staged set.
    StagedDiff,
    /// A question that must be answered before something irreversible or shared.
    Confirm,
    /// A capture from the inbox, and what promoting it would produce.
    Promotion,
    /// The output of a captured run.
    JobOutput,
    Help,
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Everything that can happen to the palette.
///
/// The first block is keys; the second is what effects report back. Keeping both
/// in one enum is what makes the reducer the single place where the palette's
/// behaviour is decided — an effect result cannot take a shortcut around it.
#[derive(Debug, Clone)]
pub enum Action {
    Input(char),
    Backspace,
    MoveUp,
    MoveDown,
    Enter,
    /// The fuller story: the staged set when there is one, otherwise this row.
    ShiftEnter,
    /// Stage a toggle.
    Space,
    /// Cycle the mutation scope.
    Tab,
    /// Apply the staged set.
    CtrlEnter,
    /// Run in a new multiplexer pane.
    AltEnter,
    /// Reveal where this capability's source lives.
    CtrlO,
    /// Repeat the most recent run.
    CtrlR,
    Esc,
    Help,

    // Effect results.
    ResultsUpdated(Vec<Row>),
    Staged(Box<StagedOutcome>),
    Opened(Box<Opened>),
    ApplyFinished(std::result::Result<GenerationId, AikitError>),
    JobOutput(JobOutput),
    SourceOpened(std::result::Result<std::path::PathBuf, AikitError>),
    Promoted(std::result::Result<CapsuleId, AikitError>),
    Failed(AikitError),
    Resized(u16, u16),
}

/// What opening a row turned out to be.
#[derive(Debug, Clone)]
pub enum Opened {
    /// It takes arguments.
    Form(Box<OpenedForm>),
    /// It does not, and this is the invocation.
    Ready(Box<RunIntent>),
    /// It is not something you run.
    Details,
    Failed(AikitError),
}

/// A form, plus the two facts the reducer will need when it is completed.
#[derive(Debug, Clone)]
pub struct OpenedForm {
    pub capsule: Capsule,
    pub form: ArgForm,
    /// The revision has not been reviewed, so running it needs a confirmation.
    pub requires_confirmation: bool,
    /// Set when the user asked for a new pane rather than the manifest's mode.
    pub mode_override: Option<ExecMode>,
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

/// The only things that touch the application.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Rank the catalog against this query. Runs off the render path.
    Search { query: String },
    /// Work out what opening this row means: a form, a ready run, or a preview.
    Open(CapsuleId),
    /// Ask the resolver what the staged set would do.
    Stage,
    /// Commit the staged set.
    Apply,
    /// Run something whose output the palette will show.
    Run(Box<RunIntent>),
    OpenSource(CapsuleId),
    Promote(usize),
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// One line of feedback. Carries the machine code when there is one, so the
/// footer can show what `--json` would have reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub level: Level,
    pub message: String,
    pub code: Option<&'static str>,
}

impl Status {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            level: Level::Info,
            message: message.into(),
            code: None,
        }
    }

    pub fn error(error: &AikitError) -> Self {
        Self {
            level: Level::Error,
            message: error.message().to_string(),
            code: Some(error.code()),
        }
    }
}

// ---------------------------------------------------------------------------
// Confirmations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Writing a durable, shared profile.
    WriteScope(ScopeKind),
    /// Running a revision nobody has reviewed.
    RunUnreviewed(CapsuleId),
    /// Turning a capture into a capsule.
    Promote(usize),
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub kind: ConfirmKind,
    pub prompt: String,
    pub detail: String,
    /// Held rather than re-derived, so answering cannot silently run something
    /// other than what was described.
    pub pending_run: Option<RunIntent>,
    /// Where `Esc` returns to.
    pub back: Mode,
}

// ---------------------------------------------------------------------------
// The management lane
// ---------------------------------------------------------------------------

/// The `:` lane. AIKit's own actions, which are not capsules and must not be
/// searched as if they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageAction {
    ReviewCaptures,
    ApplyStaged,
    DiscardStaged,
    ShowHelp,
}

impl ManageAction {
    pub const ALL: [ManageAction; 4] = [
        ManageAction::ReviewCaptures,
        ManageAction::ApplyStaged,
        ManageAction::DiscardStaged,
        ManageAction::ShowHelp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ManageAction::ReviewCaptures => "review captures waiting in the inbox",
            ManageAction::ApplyStaged => "apply the staged changes",
            ManageAction::DiscardStaged => "discard the staged changes",
            ManageAction::ShowHelp => "show the key map",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ManageAction::ReviewCaptures => {
                "promote something an agent worked out into a capability"
            }
            ManageAction::ApplyStaged => "materialize a new generation for this context",
            ManageAction::DiscardStaged => "leave the effective view exactly as it is",
            ManageAction::ShowHelp => "every key, and what it does here",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Everything the palette knows. Cheap to move, never shared.
#[derive(Debug, Clone)]
pub struct AppState {
    pub mode: Mode,
    pub query: String,
    pub rows: Vec<Row>,
    /// Indexes [`Self::rows`], or the manage lane's actions when that lane is on.
    pub cursor: usize,
    pub scope: ScopeSelector,
    pub staged: StagedSet,
    pub staged_outcome: Option<StagedOutcome>,
    pub form: Option<ArgForm>,
    pub confirm: Option<Confirm>,
    pub job: Option<JobOutput>,
    pub promotion_cursor: usize,
    pub status: Option<Status>,
    /// Set exactly once, when the palette is finished.
    pub outcome: Option<PaletteOutcome>,

    pub host: UiHost,
    pub glyphs: Glyphs,
    pub area: (u16, u16),

    pub descriptor: ContextDescriptor,
    pub view: ResolvedView,
    pub recent: Vec<RunIntent>,
    pub drafts: Vec<PromotionDraft>,

    manage: Vec<ManageAction>,
    form_capsule: Option<Capsule>,
    form_requires_confirmation: bool,
    /// Set by `Alt+Enter` before the open effect runs, so a form completed later
    /// still lands in the pane the user asked for.
    pub(crate) form_mode_override: Option<ExecMode>,
    help_from: Mode,
}

impl AppState {
    /// Open a palette over a backend. Returns the start-up effects too, because
    /// the first ranking is an effect like any other.
    pub fn open(
        backend: &dyn PaletteBackend,
        request: &PaletteRequest,
    ) -> Result<(Self, Vec<Effect>)> {
        let descriptor = backend.context().clone();
        let scope = match request.scope {
            Some(scope) => ScopeSelector::with_scope(&descriptor, scope)?,
            None => ScopeSelector::for_context(&descriptor),
        };
        let query = request.initial_query.clone().unwrap_or_default();
        let state = Self {
            mode: Mode::Search,
            manage: manage_rows_for(&query),
            query: query.clone(),
            rows: Vec::new(),
            cursor: 0,
            scope,
            staged: StagedSet::default(),
            staged_outcome: None,
            form: None,
            confirm: None,
            job: None,
            promotion_cursor: 0,
            status: None,
            outcome: None,
            host: request.host,
            glyphs: Glyphs::from_env(),
            area: (80, 24),
            descriptor,
            view: backend.view().clone(),
            recent: backend.recent(),
            drafts: backend.promotion_drafts(),
            form_capsule: None,
            form_requires_confirmation: false,
            form_mode_override: None,
            help_from: Mode::Search,
        };
        let effects = vec![Effect::Search { query }];
        Ok((state, effects))
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    /// The `:` lane's rows, when that lane is on.
    pub fn manage_rows(&self) -> &[ManageAction] {
        &self.manage
    }

    pub fn selected_manage_row(&self) -> Option<ManageAction> {
        if !self.in_manage_lane() {
            return None;
        }
        self.manage.get(self.cursor).copied()
    }

    pub fn in_manage_lane(&self) -> bool {
        parse_query(&self.query).prefix == Some(FastPrefix::Manage)
    }

    pub fn staged_diff(&self) -> Option<&StagedDiff> {
        self.staged_outcome.as_ref().and_then(|o| o.as_ref().ok())
    }

    pub fn staged_problem(&self) -> Option<&StagedProblem> {
        self.staged_outcome.as_ref().and_then(|o| o.as_ref().err())
    }

    /// Why the selected row is in the state it is in, from the resolved view.
    pub fn explanation(&self) -> Option<Explanation> {
        self.view.explain(&self.selected_row()?.doc.id)
    }

    pub fn promotion_draft(&self) -> Option<&PromotionDraft> {
        self.drafts.get(self.promotion_cursor)
    }

    /// The form context, including the defaults derived from this context.
    pub fn form_context(&self) -> FormContext {
        FormContext::from_descriptor(&self.descriptor)
    }

    pub fn form_capsule(&self) -> Option<&Capsule> {
        self.form_capsule.as_ref()
    }

    /// How many rows the live list has, whichever lane is on.
    fn list_len(&self) -> usize {
        if self.in_manage_lane() {
            self.manage.len()
        } else {
            self.rows.len()
        }
    }
}

fn manage_rows_for(query: &str) -> Vec<ManageAction> {
    let parsed = parse_query(query);
    if parsed.prefix != Some(FastPrefix::Manage) {
        return Vec::new();
    }
    let needle = parsed.text.to_lowercase();
    ManageAction::ALL
        .into_iter()
        .filter(|action| needle.is_empty() || action.label().to_lowercase().contains(&needle))
        .collect()
}

// ---------------------------------------------------------------------------
// The reducer
// ---------------------------------------------------------------------------

/// A new state and the effects it wants run.
#[derive(Debug, Clone)]
pub struct Reduction {
    pub state: AppState,
    pub effects: Vec<Effect>,
}

impl Reduction {
    fn plain(state: AppState) -> Self {
        Self {
            state,
            effects: Vec::new(),
        }
    }

    fn with(state: AppState, effect: Effect) -> Self {
        Self {
            state,
            effects: vec![effect],
        }
    }
}

/// The reducer. Pure.
pub fn reduce(mut state: AppState, action: Action) -> Reduction {
    // Effect results are handled before the mode split: they are answers to
    // questions the palette already asked, and dropping one because the user
    // moved on would strand the state that asked for it.
    match action {
        Action::ResultsUpdated(rows) => {
            state.rows = rows;
            state.cursor = state.cursor.min(state.rows.len().saturating_sub(1));
            return Reduction::plain(state);
        }
        Action::Staged(outcome) => {
            let outcome = *outcome;
            if outcome.is_err() {
                // A refusal is the most useful thing the palette can show, and it
                // must be shown while the user is still choosing.
                state.mode = Mode::StagedDiff;
            }
            state.staged_outcome = Some(outcome);
            return Reduction::plain(state);
        }
        Action::Opened(opened) => return opened_reduction(state, *opened),
        Action::ApplyFinished(result) => {
            return match result {
                Ok(generation) => {
                    state.outcome = Some(PaletteOutcome::Applied(generation));
                    Reduction::plain(state)
                }
                Err(error) => {
                    // The staged set survives on purpose: a stale base or a lock
                    // contention is something to retry, not something to retype.
                    state.status = Some(Status::error(&error));
                    state.mode = Mode::StagedDiff;
                    Reduction::plain(state)
                }
            };
        }
        Action::JobOutput(job) => {
            state.job = Some(job);
            state.mode = Mode::JobOutput;
            return Reduction::plain(state);
        }
        Action::SourceOpened(result) => {
            state.status = Some(match result {
                Ok(path) => Status::info(format!("source: {}", path.display())),
                Err(error) => Status::error(&error),
            });
            return Reduction::plain(state);
        }
        Action::Promoted(result) => {
            return match result {
                Ok(id) => {
                    state.outcome = Some(PaletteOutcome::Promoted(id));
                    Reduction::plain(state)
                }
                Err(error) => {
                    state.status = Some(Status::error(&error));
                    state.mode = Mode::Promotion;
                    Reduction::plain(state)
                }
            };
        }
        Action::Failed(error) => {
            state.status = Some(Status::error(&error));
            return Reduction::plain(state);
        }
        Action::Resized(cols, rows) => {
            state.area = (cols, rows);
            return Reduction::plain(state);
        }
        _ => {}
    }

    match state.mode {
        Mode::Confirm => confirm(state, action),
        Mode::ArgForm => arg_form(state, action),
        Mode::Promotion => promotion(state, action),
        Mode::JobOutput => job_output(state, action),
        Mode::Help => help(state, action),
        Mode::StagedDiff => staged_diff(state, action),
        Mode::Search | Mode::Preview => searching(state, action),
    }
}

// ---------------------------------------------------------------------------
// Search and preview
// ---------------------------------------------------------------------------

fn searching(mut state: AppState, action: Action) -> Reduction {
    match action {
        Action::Input(c) => {
            state.mode = Mode::Search;
            state.status = None;
            state.query.push(c);
            requery(state)
        }
        Action::Backspace => {
            state.mode = Mode::Search;
            state.status = None;
            state.query.pop();
            requery(state)
        }
        Action::MoveUp => {
            state.cursor = state.cursor.saturating_sub(1);
            Reduction::plain(state)
        }
        Action::MoveDown => {
            let last = state.list_len().saturating_sub(1);
            state.cursor = (state.cursor + 1).min(last);
            Reduction::plain(state)
        }
        Action::Space => stage_selected(state),
        Action::Tab => {
            state.scope.cycle();
            if state.staged.is_empty() {
                Reduction::plain(state)
            } else {
                // The same toggle at a different scope can resolve differently:
                // a session overlay may undo a project disable that a global
                // profile cannot.
                Reduction::with(state, Effect::Stage)
            }
        }
        Action::Enter => enter(state, None),
        Action::AltEnter => enter(state, Some(ExecMode::NewPane)),
        Action::ShiftEnter => {
            state.mode = if state.staged.is_empty() {
                Mode::Preview
            } else {
                Mode::StagedDiff
            };
            Reduction::plain(state)
        }
        Action::CtrlEnter => apply(state),
        Action::CtrlO => match state.selected_row() {
            Some(row) => {
                let id = row.doc.id.clone();
                Reduction::with(state, Effect::OpenSource(id))
            }
            None => {
                state.status = Some(Status::info("nothing selected"));
                Reduction::plain(state)
            }
        },
        Action::CtrlR => repeat_recent(state),
        Action::Help => {
            state.help_from = state.mode;
            state.mode = Mode::Help;
            Reduction::plain(state)
        }
        Action::Esc => {
            if state.mode == Mode::Preview {
                state.mode = Mode::Search;
                return Reduction::plain(state);
            }
            if !state.query.is_empty() {
                state.query.clear();
                return requery(state);
            }
            if !state.staged.is_empty() {
                let count = state.staged.len();
                state.staged.clear();
                state.staged_outcome = None;
                state.status = Some(Status::info(format!(
                    "{count} staged change{} discarded",
                    if count == 1 { "" } else { "s" }
                )));
                return Reduction::plain(state);
            }
            state.outcome = Some(PaletteOutcome::Closed);
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

fn requery(mut state: AppState) -> Reduction {
    state.manage = manage_rows_for(&state.query);
    state.cursor = state.cursor.min(state.list_len().saturating_sub(1));
    if state.in_manage_lane() {
        // The manage lane is not the catalog; ranking capsules against `:` would
        // silently mix AIKit's own actions into the capability list.
        return Reduction::plain(state);
    }
    let query = state.query.clone();
    Reduction::with(state, Effect::Search { query })
}

fn stage_selected(mut state: AppState) -> Reduction {
    let Some(row) = state.selected_row() else {
        state.status = Some(Status::info("nothing selected"));
        return Reduction::plain(state);
    };
    let id = row.doc.id.clone();
    let on = is_on(&state.view, &id);
    state.staged.toggle(&id, on);
    state.status = None;
    if state.staged.is_empty() {
        state.staged_outcome = None;
        return Reduction::plain(state);
    }
    Reduction::with(state, Effect::Stage)
}

fn enter(mut state: AppState, mode_override: Option<ExecMode>) -> Reduction {
    if let Some(action) = state.selected_manage_row() {
        return manage(state, action);
    }
    let Some(row) = state.selected_row() else {
        state.status = Some(Status::info("nothing selected"));
        return Reduction::plain(state);
    };
    let id = row.doc.id.clone();
    state.form_mode_override = mode_override;
    Reduction::with(state, Effect::Open(id))
}

fn manage(mut state: AppState, action: ManageAction) -> Reduction {
    match action {
        ManageAction::ReviewCaptures => {
            if state.drafts.is_empty() {
                state.status = Some(Status::info("the inbox is empty"));
                return Reduction::plain(state);
            }
            state.promotion_cursor = 0;
            state.mode = Mode::Promotion;
            Reduction::plain(state)
        }
        ManageAction::ApplyStaged => apply(state),
        ManageAction::DiscardStaged => {
            state.staged.clear();
            state.staged_outcome = None;
            state.status = Some(Status::info("staged changes discarded"));
            Reduction::plain(state)
        }
        ManageAction::ShowHelp => {
            state.help_from = Mode::Search;
            state.mode = Mode::Help;
            Reduction::plain(state)
        }
    }
}

fn apply(mut state: AppState) -> Reduction {
    if state.staged.is_empty() {
        state.status = Some(Status::info("nothing staged"));
        return Reduction::plain(state);
    }
    if let Some(problem) = state.staged_problem() {
        let status = Status::error(&problem.error);
        state.status = Some(status);
        state.mode = Mode::StagedDiff;
        return Reduction::plain(state);
    }
    if let Some(confirmation) = state.scope.confirmation(state.staged.len()) {
        state.confirm = Some(Confirm {
            kind: ConfirmKind::WriteScope(confirmation.scope),
            prompt: confirmation.prompt,
            detail: confirmation.detail,
            pending_run: None,
            back: state.mode,
        });
        state.mode = Mode::Confirm;
        return Reduction::plain(state);
    }
    Reduction::with(state, Effect::Apply)
}

fn repeat_recent(mut state: AppState) -> Reduction {
    let Some(intent) = state.recent.first().cloned() else {
        state.status = Some(Status::info("nothing has been run in this context yet"));
        return Reduction::plain(state);
    };
    launch(state, intent)
}

/// Decide what to do with a ready invocation.
///
/// A mode that needs the terminal, or a multiplexer the palette does not own, is
/// handed back to the caller. A captured or background run stays here so its
/// output can be shown.
fn launch(mut state: AppState, intent: RunIntent) -> Reduction {
    if intent.requires_confirmation {
        state.confirm = Some(Confirm {
            kind: ConfirmKind::RunUnreviewed(intent.capsule.clone()),
            prompt: format!("Run {} without a reviewed revision?", intent.capsule),
            detail: "Nobody has reviewed this revision. Running it is a one-off decision; it \
                     does not record trust."
                .to_string(),
            pending_run: Some(intent),
            back: state.mode,
        });
        state.mode = Mode::Confirm;
        return Reduction::plain(state);
    }
    if intent.mode.releases_terminal() || intent.mode.needs_mux() {
        state.outcome = Some(PaletteOutcome::Run(intent));
        return Reduction::plain(state);
    }
    Reduction::with(state, Effect::Run(Box::new(intent)))
}

fn opened_reduction(mut state: AppState, opened: Opened) -> Reduction {
    match opened {
        Opened::Form(form) => {
            let OpenedForm {
                capsule,
                form,
                requires_confirmation,
                mode_override,
            } = *form;
            state.form = Some(form);
            state.form_capsule = Some(capsule);
            state.form_requires_confirmation = requires_confirmation;
            state.form_mode_override = mode_override;
            state.mode = Mode::ArgForm;
            Reduction::plain(state)
        }
        Opened::Ready(intent) => launch(state, *intent),
        Opened::Details => {
            state.mode = Mode::Preview;
            Reduction::plain(state)
        }
        Opened::Failed(error) => {
            state.status = Some(Status::error(&error));
            Reduction::plain(state)
        }
    }
}

// ---------------------------------------------------------------------------
// Confirm
// ---------------------------------------------------------------------------

/// Every action except `Enter` and `Esc` is inert here. That is the whole
/// mechanism: there is no second key, no "yes" character and no repeat of the
/// original chord that gets past a confirmation.
fn confirm(mut state: AppState, action: Action) -> Reduction {
    match action {
        Action::Enter => {
            let Some(confirmation) = state.confirm.take() else {
                state.mode = Mode::Search;
                return Reduction::plain(state);
            };
            state.mode = confirmation.back;
            match confirmation.kind {
                ConfirmKind::WriteScope(_) => Reduction::with(state, Effect::Apply),
                ConfirmKind::RunUnreviewed(_) => match confirmation.pending_run {
                    Some(mut intent) => {
                        // Consumed: the user answered for this run and this run
                        // only. Trust is not recorded here.
                        intent.requires_confirmation = false;
                        launch(state, intent)
                    }
                    None => Reduction::plain(state),
                },
                ConfirmKind::Promote(index) => Reduction::with(state, Effect::Promote(index)),
            }
        }
        Action::Esc => {
            let back = state
                .confirm
                .take()
                .map(|c| c.back)
                .unwrap_or(Mode::Search);
            state.mode = back;
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

// ---------------------------------------------------------------------------
// The argument form
// ---------------------------------------------------------------------------

fn arg_form(mut state: AppState, action: Action) -> Reduction {
    let Some(form) = state.form.as_mut() else {
        state.mode = Mode::Search;
        return Reduction::plain(state);
    };
    match action {
        Action::Input(c) => {
            form.input_char(c);
            Reduction::plain(state)
        }
        Action::Backspace => {
            form.backspace();
            Reduction::plain(state)
        }
        // `Tab` moves between fields here rather than cycling the mutation scope.
        // A form is not a place where a scope means anything, and every other
        // text form in every terminal moves focus with Tab.
        Action::MoveDown | Action::Tab => {
            form.focus_next();
            Reduction::plain(state)
        }
        Action::MoveUp => {
            form.focus_previous();
            Reduction::plain(state)
        }
        Action::Space => {
            form.activate_focused();
            Reduction::plain(state)
        }
        Action::Enter | Action::CtrlEnter => complete_form(state, None),
        Action::AltEnter => complete_form(state, Some(ExecMode::NewPane)),
        Action::Help => {
            state.help_from = Mode::ArgForm;
            state.mode = Mode::Help;
            Reduction::plain(state)
        }
        Action::Esc => {
            state.form = None;
            state.form_capsule = None;
            state.form_mode_override = None;
            state.mode = Mode::Search;
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

fn complete_form(mut state: AppState, mode_override: Option<ExecMode>) -> Reduction {
    let Some(form) = state.form.as_mut() else {
        return Reduction::plain(state);
    };
    if !form.validate() {
        // The per-field errors are already recorded; the footer names how many.
        let failing = form.fields().iter().filter(|f| f.error().is_some()).count();
        state.status = Some(Status::info(format!(
            "{failing} field{} still need{} attention",
            if failing == 1 { "" } else { "s" },
            if failing == 1 { "s" } else { "" }
        )));
        return Reduction::plain(state);
    }
    let Some(capsule) = state.form_capsule.clone() else {
        return Reduction::plain(state);
    };
    let requires_confirmation = state.form_requires_confirmation;
    let descriptor = state.descriptor.clone();
    let built = state
        .form
        .as_ref()
        .expect("checked above")
        .intent_with_confirmation(&capsule, &descriptor, requires_confirmation);
    match built {
        Ok(mut intent) => {
            if let Some(mode) = mode_override.or(state.form_mode_override) {
                intent.mode = mode;
            }
            state.form = None;
            state.form_capsule = None;
            state.form_mode_override = None;
            launch(state, intent)
        }
        Err(error) => {
            state.status = Some(Status::error(&error));
            Reduction::plain(state)
        }
    }
}

// ---------------------------------------------------------------------------
// Staged diff, job output, promotion, help
// ---------------------------------------------------------------------------

fn staged_diff(mut state: AppState, action: Action) -> Reduction {
    match action {
        Action::CtrlEnter | Action::Enter => apply(state),
        Action::Tab => {
            state.scope.cycle();
            if state.staged.is_empty() {
                Reduction::plain(state)
            } else {
                Reduction::with(state, Effect::Stage)
            }
        }
        Action::Help => {
            state.help_from = Mode::StagedDiff;
            state.mode = Mode::Help;
            Reduction::plain(state)
        }
        Action::Esc => {
            state.mode = Mode::Search;
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

fn job_output(mut state: AppState, action: Action) -> Reduction {
    match action {
        Action::Esc | Action::Enter => {
            state.mode = Mode::Search;
            state.job = None;
            Reduction::plain(state)
        }
        Action::Help => {
            state.help_from = Mode::JobOutput;
            state.mode = Mode::Help;
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

fn promotion(mut state: AppState, action: Action) -> Reduction {
    match action {
        Action::MoveDown => {
            let last = state.drafts.len().saturating_sub(1);
            state.promotion_cursor = (state.promotion_cursor + 1).min(last);
            Reduction::plain(state)
        }
        Action::MoveUp => {
            state.promotion_cursor = state.promotion_cursor.saturating_sub(1);
            Reduction::plain(state)
        }
        Action::Enter => {
            let index = state.promotion_cursor;
            let Some(draft) = state.drafts.get(index) else {
                state.mode = Mode::Search;
                return Reduction::plain(state);
            };
            if let Some(reason) = draft.withheld_reason() {
                // Release-blocking case 10. The refusal happens here as well as
                // in the store, because a palette that offers the button and
                // lets the store say no has already shown the user the body.
                state.status = Some(Status {
                    level: Level::Error,
                    message: format!("this capture cannot be promoted: {reason}"),
                    code: Some("inbox.quarantined"),
                });
                return Reduction::plain(state);
            }
            state.confirm = Some(Confirm {
                kind: ConfirmKind::Promote(index),
                prompt: format!("Promote this capture as {}?", draft.edits.id),
                detail: "It becomes a draft capsule in the personal registry. Promotion does \
                         not review it, and it will not activate until you say so."
                    .to_string(),
                pending_run: None,
                back: Mode::Promotion,
            });
            state.mode = Mode::Confirm;
            Reduction::plain(state)
        }
        Action::Help => {
            state.help_from = Mode::Promotion;
            state.mode = Mode::Help;
            Reduction::plain(state)
        }
        Action::Esc => {
            state.mode = Mode::Search;
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

fn help(mut state: AppState, action: Action) -> Reduction {
    match action {
        Action::Esc | Action::Enter | Action::Help => {
            state.mode = state.help_from;
            Reduction::plain(state)
        }
        _ => Reduction::plain(state),
    }
}

// ---------------------------------------------------------------------------
// The key map, as data
// ---------------------------------------------------------------------------

/// The help screen's content, and the single source of truth for what the
/// palette claims its keys do.
pub fn key_map(mode: Mode) -> Vec<(&'static str, &'static str)> {
    let mut rows: Vec<(&'static str, &'static str)> = match mode {
        Mode::ArgForm => vec![
            ("Tab / ↑ ↓", "move between fields"),
            ("Space", "flip a boolean, cycle an enum, pick a choice"),
            ("Enter", "run with these arguments"),
            ("Esc", "back to the list"),
        ],
        Mode::Promotion => vec![
            ("↑ ↓", "move between captures"),
            ("Enter", "promote this capture"),
            ("Esc", "back to the list"),
        ],
        Mode::JobOutput => vec![("Enter / Esc", "back to the list")],
        Mode::Confirm => vec![
            ("Enter", "yes"),
            ("Esc", "no — nothing is written"),
        ],
        _ => vec![
            ("↑ ↓", "move"),
            ("Space", "stage a change (Ctrl+Space while typing)"),
            ("Tab", "change where the change is written"),
            ("Enter", "run or open"),
            ("Shift+Enter", "explain this row, or review the staged set"),
            ("Ctrl+Enter", "apply everything staged"),
            ("Alt+Enter", "run in a new pane"),
            ("Ctrl+O", "reveal the source"),
            ("Ctrl+R", "repeat the last run"),
            ("Esc", "clear, then discard, then close"),
        ],
    };
    rows.push(("?", "this screen"));
    rows
}

/// The lanes, for the empty-query hint, in core's declared order.
///
/// A `Vec` rather than a map: sorting them by codepoint would put `+` before `>`
/// for no reason a reader could recover, and the order core declares is the
/// order they are worth learning in.
pub fn lane_hints() -> Vec<(char, &'static str)> {
    FastPrefix::ALL
        .into_iter()
        .map(|prefix| (prefix.as_char(), prefix.describe()))
        .collect()
}
