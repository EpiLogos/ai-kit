//! Terminal events for the final V2 application surface.
//!
//! Semantic key interpretation belongs to `ApplicationSurfaceController`, which
//! dispatches `UiAction` into the one `TuiState` reducer. This module only owns
//! terminal event transport and therefore has no dependency on the retired
//! Palette reducer or form modes.

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, poll, read};

use aikit_core::error::AikitError;
use aikit_core::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Idle,
}

pub trait EventSource {
    fn next(&mut self) -> Result<Option<PaletteEvent>>;
}

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
            _ => Some(PaletteEvent::Idle),
        })
    }
}

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
