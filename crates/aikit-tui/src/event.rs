//! Keys in, [`Action`]s out.
//!
//! The map is mode-aware, and it has to be, because one key genuinely means two
//! things. `Space` stages a change — but the query box also has to be able to
//! contain a space, since `kind:script cargo test` is a perfectly ordinary query.
//! The rule is stated rather than guessed at: **with an empty query you are
//! browsing and `Space` stages; once you are typing, a space is a space and
//! `Ctrl+Space` (or `Insert`) stages.**
//!
//! ## The Enter chords have fallbacks, and they are not decoration
//!
//! `Shift+Enter`, `Ctrl+Enter` and `Alt+Enter` only reach an application on
//! terminals that speak the kitty keyboard protocol. Everywhere else they arrive
//! as a bare `Enter`, which would silently apply a staged set when the user meant
//! to explain a row. So each has an unambiguous single-key alias — `Ctrl+E`,
//! `Ctrl+S`, `Ctrl+N` — and the help screen lists them.

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, poll, read,
};

use aikit_core::error::AikitError;
use aikit_core::Result;

use crate::app::{Action, AppState, Mode};

/// Something that happened to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    /// No input arrived within the poll interval. Lets a caller redraw a running
    /// job without blocking on a key.
    Idle,
}

/// Where events come from.
///
/// A trait rather than a direct `crossterm::event::read` so the end-to-end tests
/// can drive a real palette — real reducer, real effects, real ratatui frames —
/// from a scripted key sequence.
pub trait EventSource {
    /// The next event, or `None` when the source is exhausted.
    fn next(&mut self) -> Result<Option<PaletteEvent>>;
}

/// The real one.
pub struct CrosstermEvents {
    pub poll_interval: Duration,
}

impl Default for CrosstermEvents {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(100),
        }
    }
}

impl EventSource for CrosstermEvents {
    fn next(&mut self) -> Result<Option<PaletteEvent>> {
        let io = |e: std::io::Error| {
            AikitError::new("tui.terminal_read_failed", format!("could not read a key: {e}"))
        };
        if !poll(self.poll_interval).map_err(io)? {
            return Ok(Some(PaletteEvent::Idle));
        }
        Ok(match read().map_err(io)? {
            Event::Key(key) => Some(PaletteEvent::Key(key)),
            Event::Mouse(mouse) => Some(PaletteEvent::Mouse(mouse)),
            Event::Resize(cols, rows) => Some(PaletteEvent::Resize(cols, rows)),
            // Focus and paste events are not bound to anything.
            _ => Some(PaletteEvent::Idle),
        })
    }
}

/// A fixed sequence, for tests.
#[derive(Debug, Default)]
pub struct ScriptedEvents {
    queue: VecDeque<PaletteEvent>,
}

impl ScriptedEvents {
    pub fn new(events: impl IntoIterator<Item = PaletteEvent>) -> Self {
        Self {
            queue: events.into_iter().collect(),
        }
    }

    /// Convenience: a sequence of plain key presses.
    pub fn keys(codes: impl IntoIterator<Item = KeyCode>) -> Self {
        Self::new(codes.into_iter().map(|code| {
            PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
        }))
    }

    pub fn push(&mut self, event: PaletteEvent) {
        self.queue.push_back(event);
    }
}

impl EventSource for ScriptedEvents {
    fn next(&mut self) -> Result<Option<PaletteEvent>> {
        Ok(self.queue.pop_front())
    }
}

/// Translate one event into an action, given what the palette is doing.
///
/// `None` means the key is not bound here, which is different from bound-to-
/// nothing: an unbound key never reaches the reducer at all.
pub fn action_for(event: &PaletteEvent, state: &AppState) -> Option<Action> {
    match event {
        PaletteEvent::Resize(cols, rows) => Some(Action::Resized(*cols, *rows)),
        // The palette remains deliberately keyboard-first. Mouse input is kept
        // intact because the tree binds it to the same actions as its keys.
        PaletteEvent::Mouse(_) => None,
        PaletteEvent::Idle => None,
        PaletteEvent::Key(key) => key_action(*key, state),
    }
}

fn key_action(key: KeyEvent, state: &AppState) -> Option<Action> {
    // Key *releases* would otherwise double every keystroke on terminals that
    // report them.
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    Some(match key.code {
        KeyCode::Esc => Action::Esc,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Up => Action::MoveUp,
        KeyCode::Down => Action::MoveDown,
        KeyCode::Tab | KeyCode::BackTab => Action::Tab,
        KeyCode::F(1) => Action::Help,
        KeyCode::Insert => Action::Space,

        KeyCode::Enter if ctrl => Action::CtrlEnter,
        KeyCode::Enter if alt => Action::AltEnter,
        KeyCode::Enter if shift => Action::ShiftEnter,
        KeyCode::Enter => Action::Enter,

        KeyCode::Char(' ') if ctrl => Action::Space,
        // Browsing versus filtering: see the module header.
        KeyCode::Char(' ') if typing(state) => Action::Input(' '),
        KeyCode::Char(' ') => Action::Space,

        KeyCode::Char('c') if ctrl => Action::Esc,
        KeyCode::Char('e') if ctrl => Action::ShiftEnter,
        KeyCode::Char('s') if ctrl => Action::CtrlEnter,
        KeyCode::Char('n') if ctrl => Action::AltEnter,
        KeyCode::Char('o') if ctrl => Action::CtrlO,
        KeyCode::Char('r') if ctrl => Action::CtrlR,
        KeyCode::Char('t') if ctrl => Action::Tree,
        KeyCode::Char('k') if ctrl => Action::MoveUp,
        KeyCode::Char('j') if ctrl => Action::MoveDown,

        KeyCode::Char('?') if !typing(state) => Action::Help,
        KeyCode::Char(c) if !ctrl && !alt => Action::Input(c),
        _ => return None,
    })
}

/// Is the user composing text rather than browsing a list?
fn typing(state: &AppState) -> bool {
    match state.mode {
        // Every field is a text field until proven otherwise, so a space in a
        // form is always a space unless the field has discrete states.
        Mode::ArgForm => state
            .form
            .as_ref()
            .and_then(|f| f.fields().get(f.focused()))
            .map(|field| !has_discrete_states(field.spec.ty))
            .unwrap_or(true),
        Mode::Search | Mode::Preview => !state.query.is_empty(),
        _ => false,
    }
}

fn has_discrete_states(ty: aikit_core::arg::ArgType) -> bool {
    use aikit_core::arg::ArgType;
    matches!(ty, ArgType::Bool | ArgType::Enum | ArgType::Multiselect)
}
