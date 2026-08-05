//! Colour and weight. Restraint is the aesthetic.
//!
//! Three rules, and they are the whole module.
//!
//! **Terminal-native colours only.** Every colour here is one of the sixteen ANSI
//! names, so it resolves through the user's own palette. A hard-coded RGB green
//! looks deliberate on the machine it was picked on and wrong on a light theme, a
//! high-contrast theme, and every carefully tuned colour scheme someone has spent
//! an evening on.
//!
//! **One border.** The palette draws a single frame and nothing else. Nested
//! boxes are how a small overlay turns into a dashboard: each one is defensible
//! and together they leave no room for content.
//!
//! **Colour is never the only signal.** Every state that has a colour also has a
//! glyph (see [`crate::layout::Glyphs`]) and a word. `STANDARDS.md` §5 requires
//! it, and the ASCII snapshot tests would catch it if it stopped being true.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

/// The palette's styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme;

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

impl Theme {
    pub fn new() -> Self {
        Self
    }

    /// Ordinary text. `Reset` rather than a named colour so the terminal's own
    /// foreground wins.
    pub fn base(self) -> Style {
        Style::default()
    }

    /// Supporting text: descriptions, hints, the parts of a row that are context
    /// rather than content.
    pub fn dim(self) -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }

    /// The one emphasis colour, used for the query and the scope badge.
    pub fn accent(self) -> Style {
        Style::default().fg(Color::Cyan)
    }

    pub fn active(self) -> Style {
        Style::default().fg(Color::Green)
    }

    pub fn unavailable(self) -> Style {
        Style::default().fg(Color::Yellow)
    }

    pub fn error(self) -> Style {
        Style::default().fg(Color::Red)
    }

    /// The staged mark. Magenta because it is neither a success nor a warning —
    /// it is a change that has not happened yet.
    pub fn staged(self) -> Style {
        Style::default().fg(Color::Magenta)
    }

    /// The cursor row. Reversed rather than coloured, so it is visible on every
    /// theme including monochrome.
    pub fn selected(self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    pub fn heading(self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    pub fn border(self) -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }

    /// Always plain. See the module header.
    pub fn border_type(self) -> BorderType {
        BorderType::Plain
    }

    /// The only animation in the palette: a running job's tick.
    ///
    /// Two frames, driven by the event loop's idle poll. A spinner is the one
    /// place motion carries information — that something is still happening —
    /// and everything else is static on purpose.
    pub fn tick(self, frame: u64) -> &'static str {
        if frame.is_multiple_of(2) {
            "·"
        } else {
            " "
        }
    }
}
