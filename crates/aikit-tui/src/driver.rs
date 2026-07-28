//! The loop: effects out, actions back, frames drawn.
//!
//! This is the only place in the crate that performs I/O, and it is deliberately
//! thin. [`Runtime::execute`] turns one [`Effect`] into the [`Action`]s it
//! produced; [`Runtime::settle`] runs that to a fixed point so a caller — a test
//! or the event loop — always sees a state with no work outstanding.
//!
//! ## Why the loop is generic
//!
//! [`event_loop`] takes any ratatui backend and any [`EventSource`]. The real
//! palette hands it a `CrosstermBackend` and real keys; the end-to-end tests hand
//! it a `TestBackend` and a scripted sequence. There is no second implementation
//! of the loop for tests to drift away from.
//!
//! ## Where the terminal is restored
//!
//! [`run_on_terminal`] restores raw mode and the alternate screen **before** the
//! outcome is returned, including on the error path. That is what makes
//! `PaletteOutcome::Run` safe: the caller executes a foreground command into a
//! terminal the palette has already given back.

use std::collections::VecDeque;
use std::io;

use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::{Terminal, TerminalOptions, Viewport};

use aikit_core::error::AikitError;
use aikit_core::search::parse_query;
use aikit_core::Result;

use crate::app::{reduce, Action, AppState, Effect, Opened, OpenedForm, Status};
use crate::backend::PaletteBackend;
use crate::backend::Toggle;
use crate::event::{action_for, CrosstermEvents, EventSource};
use crate::form::ArgForm;
use crate::host::UiHost;
use crate::render;
use crate::search::Matcher;
use crate::staging;
use crate::{PaletteOutcome, PaletteRequest};

/// A cascade longer than this is a bug in the reducer, not a deep computation.
/// Bounded rather than trusted so a mistake shows up as a message instead of a
/// hung terminal.
const MAX_CASCADE: usize = 64;

/// Holds what the effect layer wants to keep between keystrokes.
pub struct Runtime {
    matcher: Matcher,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(),
        }
    }

    /// Perform one effect and report what it produced.
    pub fn execute(
        &mut self,
        backend: &mut dyn PaletteBackend,
        state: &AppState,
        effect: Effect,
    ) -> Vec<Action> {
        match effect {
            Effect::Search { query } => {
                let parsed = parse_query(&query);
                let docs = backend.documents();
                vec![Action::ResultsUpdated(self.matcher.rank(&parsed, &docs))]
            }
            Effect::Open(id) => vec![Action::Opened(Box::new(open(backend, state, &id)))],
            Effect::Stage => vec![Action::Staged(Box::new(staging::stage(
                backend,
                state.scope.current(),
                &state.staged,
            )))],
            Effect::Apply => {
                let toggles = state.staged.toggles();
                vec![Action::ApplyFinished(
                    backend.apply(state.scope.current(), &toggles),
                )]
            }
            Effect::Run(intent) => match backend.start(&intent) {
                Ok(job) => vec![Action::JobOutput(job)],
                Err(error) => vec![Action::Failed(error)],
            },
            Effect::OpenSource(id) => vec![Action::SourceOpened(backend.open_source(&id))],
            Effect::Promote(index) => match state.drafts.get(index) {
                Some(draft) => {
                    let draft = draft.clone();
                    vec![Action::Promoted(backend.promote(&draft))]
                }
                None => vec![Action::Failed(AikitError::new(
                    "inbox.no_such_candidate",
                    "the capture being promoted is no longer in the inbox",
                ))],
            },
        }
    }

    /// Run effects, and the effects they cause, until nothing is outstanding.
    pub fn settle(
        &mut self,
        backend: &mut dyn PaletteBackend,
        mut state: AppState,
        effects: Vec<Effect>,
    ) -> AppState {
        let mut queue: VecDeque<Effect> = effects.into();
        let mut performed = 0usize;
        while let Some(effect) = queue.pop_front() {
            if state.outcome.is_some() {
                break;
            }
            performed += 1;
            if performed > MAX_CASCADE {
                state.status = Some(Status::error(&AikitError::new(
                    "tui.effect_cascade",
                    "the palette asked itself the same question too many times; \
                     this is a bug — nothing was applied",
                )));
                break;
            }
            for action in self.execute(backend, &state, effect) {
                let reduction = reduce(state, action);
                state = reduction.state;
                queue.extend(reduction.effects);
            }
        }
        state
    }

    /// Reduce one action and settle everything it caused.
    pub fn step(
        &mut self,
        backend: &mut dyn PaletteBackend,
        state: AppState,
        action: Action,
    ) -> AppState {
        let reduction = reduce(state, action);
        self.settle(backend, reduction.state, reduction.effects)
    }
}

/// Reduce one action and settle it, with a fresh runtime.
pub fn step(backend: &mut dyn PaletteBackend, state: AppState, action: Action) -> AppState {
    Runtime::new().step(backend, state, action)
}

/// Settle a list of effects with a fresh runtime.
pub fn settle(backend: &mut dyn PaletteBackend, state: AppState, effects: Vec<Effect>) -> AppState {
    Runtime::new().settle(backend, state, effects)
}

/// One event's result inside the reusable palette controller.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteStep {
    Continue,
    Tree,
    Outcome(PaletteOutcome),
}

/// The palette's complete reducer and effect state, reusable inside a surface.
pub struct PaletteController {
    state: AppState,
    runtime: Runtime,
}

impl PaletteController {
    pub fn new(backend: &mut dyn PaletteBackend, request: PaletteRequest) -> Result<Self> {
        let (state, effects) = AppState::open(backend, &request)?;
        let mut runtime = Runtime::new();
        let mut state = runtime.settle(backend, state, effects);
        if request.activate_initial {
            if let Some(target) = request.activation_target.as_ref() {
                if let Some(index) = state.rows.iter().position(|row| &row.doc.id == target) {
                    state.cursor = index;
                    state = runtime.step(backend, state, Action::Enter);
                }
            }
        }
        Ok(Self { state, runtime })
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        self.state.area = (area.width, area.height);
        render::draw(frame, &self.state);
    }

    pub fn handle(
        &mut self,
        backend: &mut dyn PaletteBackend,
        event: crate::event::PaletteEvent,
    ) -> Result<PaletteStep> {
        let Some(action) = action_for(&event, &self.state) else {
            return Ok(PaletteStep::Continue);
        };
        self.state = self.runtime.step(backend, self.state.clone(), action);
        match self.state.outcome.clone() {
            Some(PaletteOutcome::Tree) => {
                self.state.outcome = None;
                Ok(PaletteStep::Tree)
            }
            Some(outcome) => Ok(PaletteStep::Outcome(outcome)),
            None => Ok(PaletteStep::Continue),
        }
    }

    /// Import staging from another view and resolve its real consequences.
    pub fn replace_staged(&mut self, backend: &mut dyn PaletteBackend, toggles: Vec<Toggle>) {
        self.state.staged.replace(toggles);
        self.state.staged_outcome = None;
        self.state = self
            .runtime
            .settle(backend, self.state.clone(), vec![Effect::Stage]);
    }
}

/// What opening a row turns out to mean.
fn open(backend: &dyn PaletteBackend, state: &AppState, id: &aikit_core::CapsuleId) -> Opened {
    let Some(capsule) = backend.capsule(id) else {
        return Opened::Failed(
            AikitError::new(
                "capsule.not_in_catalog",
                format!("{id} is no longer in the catalog"),
            )
            .with("capability", id.to_string()),
        );
    };
    let capsule = capsule.clone();

    // "Runnable" is the resolved view's answer, not a kind lookup: a blocked or
    // quarantined script is not something you can run.
    if !state.view.can_run(id) {
        return Opened::Details;
    }

    // An unreviewed executable may be exposed, but running it is a decision.
    let requires_confirmation = state
        .view
        .catalog_index
        .get(id)
        .map(|entry| entry.kind.is_executable() && !entry.trust.may_run_unattended())
        .unwrap_or(true);

    let form = ArgForm::new(&capsule, &state.form_context());
    if !form.is_empty() {
        return Opened::Form(Box::new(OpenedForm {
            capsule,
            form,
            requires_confirmation,
            mode_override: state.form_mode_override,
        }));
    }

    match form.intent_with_confirmation(&capsule, &state.descriptor, requires_confirmation) {
        Ok(mut intent) => {
            if let Some(mode) = state.form_mode_override {
                intent.mode = mode;
            }
            Opened::Ready(Box::new(intent))
        }
        Err(error) => Opened::Failed(error),
    }
}

// ---------------------------------------------------------------------------
// The event loop
// ---------------------------------------------------------------------------

/// Drive a palette to completion on any ratatui backend and any event source.
pub fn event_loop<B, E>(
    terminal: &mut Terminal<B>,
    events: &mut E,
    backend: &mut dyn PaletteBackend,
    request: PaletteRequest,
) -> Result<PaletteOutcome>
where
    B: Backend,
    B::Error: std::fmt::Display,
    E: EventSource + ?Sized,
{
    let mut controller = PaletteController::new(backend, request)?;
    loop {
        if let Some(outcome) = controller.state().outcome.clone() {
            return Ok(outcome);
        }
        terminal
            .draw(|frame| controller.draw(frame))
            .map_err(|e| draw_error("could not draw the palette", e))?;

        let Some(event) = events.next()? else {
            // The source is exhausted. Closing is the honest outcome: pretending
            // the user cancelled is at least not pretending they confirmed.
            return Ok(PaletteOutcome::Closed);
        };
        match controller.handle(backend, event)? {
            PaletteStep::Continue => {}
            PaletteStep::Tree => return Ok(PaletteOutcome::Tree),
            PaletteStep::Outcome(outcome) => return Ok(outcome),
        }
    }
}

fn draw_error(what: &str, e: impl std::fmt::Display) -> AikitError {
    AikitError::new("tui.render_failed", format!("{what}: {e}"))
}

/// Set up a real terminal, run the palette, and restore it before returning.
pub(crate) fn run_on_terminal(
    app: &mut dyn PaletteBackend,
    request: PaletteRequest,
) -> Result<PaletteOutcome> {
    let host = request.host;
    let io_error = |what: &str, e: io::Error| {
        AikitError::new("tui.terminal_setup_failed", format!("{what}: {e}"))
    };

    crossterm::terminal::enable_raw_mode().map_err(|e| io_error("could not enter raw mode", e))?;
    let fullscreen = host == UiHost::Fullscreen;
    if fullscreen {
        crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)
            .map_err(|e| io_error("could not enter the alternate screen", e))?;
    }

    let outcome = run_inner(app, request, host);

    // Restored before the outcome is returned, and on the error path too: a
    // caller about to hand the terminal to a foreground command must not receive
    // it in raw mode.
    if fullscreen {
        let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    }
    let _ = crossterm::terminal::disable_raw_mode();
    outcome
}

fn run_inner(
    app: &mut dyn PaletteBackend,
    request: PaletteRequest,
    host: UiHost,
) -> Result<PaletteOutcome> {
    let terminal_backend = CrosstermBackend::new(io::stdout());
    let options = TerminalOptions {
        viewport: match host {
            // A tmux popup is already its own surface; the palette fills it.
            UiHost::Inline(rows) => Viewport::Inline(rows),
            UiHost::TmuxPopup | UiHost::Fullscreen => Viewport::Fullscreen,
        },
    };
    let mut terminal = Terminal::with_options(terminal_backend, options)
        .map_err(|e| AikitError::new("tui.terminal_setup_failed", format!("{e}")))?;
    let mut events = CrosstermEvents::default();
    let outcome = event_loop(&mut terminal, &mut events, app, request);

    // An inline palette leaves the strip behind unless it is cleared, which would
    // put a dead widget above the user's next prompt.
    let _ = terminal.clear();
    let _ = terminal.show_cursor();
    outcome
}
