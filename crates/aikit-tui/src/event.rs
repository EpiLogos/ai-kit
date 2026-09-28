//! Terminal events for the final V2 application surface.
//!
//! Semantic key interpretation belongs to `ApplicationSurfaceController`, which
//! dispatches `UiAction` into the one `TuiState` reducer. This module only owns
//! terminal event transport and therefore has no dependency on the retired
//! Palette reducer or form modes.

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent};

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

    /// Is another event already available, with no wait at all?
    ///
    /// This answers a different question than `next()`'s own poll: `next()`
    /// is willing to wait up to a whole `poll_interval` to find out whether
    /// *anything* is coming; this is the zero-latency check the event loop
    /// uses, after handling one event, to decide whether the terminal has
    /// already queued more (fast typing outrunning the loop, a held key's
    /// autorepeat, a paste) before it draws. Answering `false` never costs
    /// the caller more than the wait it would have paid anyway on the next
    /// ordinary `next()` call.
    fn poll_ready(&mut self) -> Result<bool>;
}

pub struct CrosstermEvents {
    pub poll_interval: Duration,
}

/// The idle wait one `next()` call may spend before reporting `Idle`.
///
/// This constant is the idle surface's whole energy budget. The event loop
/// calls `next()` again the moment handling returns, so an idle surface
/// sleeps at exactly this interval and wakes `1s / poll_interval` times per
/// second — ten polls a second here — with each wake costing one bounded
/// `poll` and nothing else when the Conversation aperture is closed. The
/// bound has a floor as well as a ceiling: crossterm's `poll` returns the
/// instant an event arrives, so this interval is the *maximum* idle wait,
/// never added keystroke latency, but a near-zero tick would still spin the
/// loop at full speed, and an orphaned surface (the custody contract) must
/// not burn CPU merely because nobody is watching it.
pub const IDLE_POLL_TICK: Duration = Duration::from_millis(100);

impl Default for CrosstermEvents {
    fn default() -> Self {
        Self {
            poll_interval: IDLE_POLL_TICK,
        }
    }
}

impl EventSource for CrosstermEvents {
    fn next(&mut self) -> Result<Option<PaletteEvent>> {
        let io = |e: std::io::Error| {
            AikitError::new(
                "tui.terminal_read_failed",
                format!("could not read a key: {e}"),
            )
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

    fn poll_ready(&mut self) -> Result<bool> {
        poll(Duration::ZERO).map_err(|e| {
            AikitError::new(
                "tui.terminal_read_failed",
                format!("could not poll for a queued event: {e}"),
            )
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
        Self::new(
            codes
                .into_iter()
                .map(|code| PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))),
        )
    }

    pub fn push(&mut self, event: PaletteEvent) {
        self.queue.push_back(event);
    }
}

impl EventSource for ScriptedEvents {
    fn next(&mut self) -> Result<Option<PaletteEvent>> {
        Ok(self.queue.pop_front())
    }

    /// A script has no terminal to poll; "already available" is simply
    /// "queued". This is what lets a test script assert draining behaviour
    /// deterministically — no real clock, no real terminal, just a queue.
    fn poll_ready(&mut self) -> Result<bool> {
        Ok(!self.queue.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The idle energy contract, pinned at the constant: an idle surface
    /// sleeps at `IDLE_POLL_TICK` and therefore performs at most
    /// `1s / tick` polls per second. The band is the bounded tick the
    /// surface loop depends on — a zero or near-zero tick here would have
    /// `next()` return immediately and spin the loop at full speed (idle
    /// CPU burn with no drawing at all), while a tick above a quarter
    /// second starts to show in how long a surface takes to notice the
    /// orphan flag and repaint windows it may have missed.
    #[test]
    fn the_idle_poll_tick_is_bounded() {
        assert!(
            IDLE_POLL_TICK >= Duration::from_millis(100),
            "an idle tick below 100ms lets the event loop spin: {IDLE_POLL_TICK:?}"
        );
        assert!(
            IDLE_POLL_TICK <= Duration::from_millis(250),
            "an idle tick above 250ms makes the surface sluggish: {IDLE_POLL_TICK:?}"
        );

        // The real event source must derive its wait from the constant, not
        // from a private re-declared value that can drift.
        assert_eq!(
            CrosstermEvents::default().poll_interval,
            IDLE_POLL_TICK,
            "CrosstermEvents must sleep at the named idle tick"
        );

        // The cadence the loop actually gets: between four and ten idle
        // polls a second, each one a bounded sleep and nothing more.
        let polls_per_second = 1.0f64 / (IDLE_POLL_TICK.as_secs_f64());
        assert!(
            (4.0..=10.0).contains(&polls_per_second),
            "an idle surface must perform a bounded number of polls per second, got {polls_per_second}"
        );
    }
}
