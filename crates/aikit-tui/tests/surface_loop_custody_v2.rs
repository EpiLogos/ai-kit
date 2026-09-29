//! The surface loop's custody contract.
//!
//! An orphaned palette — its shell or terminal emulator gone — used to keep
//! polling and redrawing into a dead terminal forever: one was observed on
//! 2026-09-25 burning ~74% CPU for 4.6 days with nobody watching. Two rules
//! hold that line, proven here against the real loop:
//!
//! 1. The loop draws only when an event that can change the frame arrived.
//!    Idle ticks and mouse motion are no-ops in the controller, so redrawing
//!    on them burns a full render for an identical frame.
//! 2. When the process is orphaned (its parent changed — the operating system
//!    reparents after a parent's death), the loop closes through the ordinary
//!    exit path rather than waiting for keys that will never come.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;

use aikit_tui::application_surface::{event_loop, ApplicationSurfaceRequest};
use aikit_tui::event::{PaletteEvent, ScriptedEvents};
use aikit_tui::host::UiHost;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::{Backend, ClearType, TestBackend};
use ratatui::prelude::{Position, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};

/// Counts full-frame draws so a test can tell "the loop drew once and then
/// waited" from "the loop kept redrawing an unchanged frame".
struct CountingBackend {
    inner: TestBackend,
    draws: Arc<AtomicUsize>,
}

impl Backend for CountingBackend {
    type Error = core::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.draws.fetch_add(1, Ordering::SeqCst);
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn counting_terminal() -> (Terminal<CountingBackend>, Arc<AtomicUsize>) {
    let draws = Arc::new(AtomicUsize::new(0));
    let terminal = Terminal::with_options(
        CountingBackend {
            inner: TestBackend::new(80, 24),
            draws: Arc::clone(&draws),
        },
        TerminalOptions {
            viewport: Viewport::Inline(10),
        },
    )
    .unwrap();
    (terminal, draws)
}

fn fixture() -> (tempfile::TempDir, common::Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(dir.path(), vec![skill("skill/custody/one")]);
    (dir, backend)
}

/// The orphan flag checked before anything else: even with input queued, a
/// surface whose session is gone closes without drawing or consuming it.
#[test]
fn an_orphaned_surface_closes_without_drawing() {
    let (_dir, mut backend) = fixture();
    let (mut terminal, draws) = counting_terminal();
    let orphaned = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mut events = ScriptedEvents::keys([KeyCode::Char('q'), KeyCode::Esc]);
    let outcome = event_loop(
        &mut terminal,
        &mut events,
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::Inline(10)),
        orphaned,
    )
    .unwrap();
    assert_eq!(outcome, aikit_tui::PaletteOutcome::Closed);
    assert_eq!(draws.load(Ordering::SeqCst), 0, "an orphan closes silently");
}

/// Idle ticks are no-ops in the controller: an idle surface waits at its poll
/// interval instead of redrawing an identical frame every tick.
#[test]
fn idle_ticks_never_redraw_the_surface() {
    let (_dir, mut backend) = fixture();
    let (mut terminal, draws) = counting_terminal();
    let orphaned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut events = ScriptedEvents::new([PaletteEvent::Idle, PaletteEvent::Idle]);
    let outcome = event_loop(
        &mut terminal,
        &mut events,
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::Inline(10)),
        orphaned,
    )
    .unwrap();
    assert_eq!(outcome, aikit_tui::PaletteOutcome::Closed);
    assert_eq!(
        draws.load(Ordering::SeqCst),
        1,
        "only the initial frame; idles add none"
    );
}

/// A batch of frame-changing events queues exactly one redraw: keys and a
/// resize each mark the frame dirty, and the loop drains the batch before
/// drawing once.
#[test]
fn frame_changing_events_queue_exactly_one_redraw() {
    let (_dir, mut backend) = fixture();
    let (mut terminal, draws) = counting_terminal();
    let orphaned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut events = ScriptedEvents::new([
        key(KeyCode::Char('a')),
        key(KeyCode::Char('b')),
        PaletteEvent::Resize(80, 24),
    ]);
    let outcome = event_loop(
        &mut terminal,
        &mut events,
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::Inline(10)),
        orphaned,
    )
    .unwrap();
    assert_eq!(outcome, aikit_tui::PaletteOutcome::Closed);
    assert_eq!(
        draws.load(Ordering::SeqCst),
        2,
        "initial frame plus one redraw for the whole batch"
    );
}
