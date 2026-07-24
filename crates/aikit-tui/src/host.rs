//! Where the palette is drawn.
//!
//! The seam exists because "open a small overlay" is not one primitive. tmux has
//! a real `display-popup`; cmux does not document an arbitrary-popup primitive
//! and a plain terminal has none at all. Rather than pretend they are the same —
//! which would mean either a fake popup drawn over cmux's own surfaces or a
//! full-screen takeover everywhere — the host is chosen per environment and named
//! honestly.
//!
//! ## Inline is the default, and it is a promise
//!
//! [`UiHost::Inline`] draws a short strip at the bottom of the terminal and leaves
//! everything above it, including the scrollback, untouched. That is the whole
//! point of a palette: the user was in the middle of something, and closing the
//! palette should return them to it with the screen as they left it.
//!
//! ## Fullscreen is an escalation with a reason
//!
//! Nothing chooses fullscreen because it is convenient to render. It is chosen
//! when the user asked for it, when the terminal is too small for a strip to be
//! anything but cramped, or when the content genuinely cannot be read in twenty
//! rows — a promotion diff or a large structured result. [`Escalation`] names
//! which, so the reason can be shown rather than inferred.

use aikit_core::platform::MuxKind;

/// The shortest inline strip that can carry a query line, a border, four result
/// rows and a footer without any of them being the only thing visible.
pub const INLINE_MIN_ROWS: u16 = 14;

/// The tallest inline strip. Beyond this the palette stops feeling like an
/// overlay and starts feeling like a takeover that forgot to say so.
pub const INLINE_MAX_ROWS: u16 = 20;

/// Rows of surrounding work an inline palette must leave visible. Without this a
/// palette on a 15-row terminal would technically fit and preserve nothing.
const INLINE_HEADROOM: u16 = 2;

/// Below this width the two-column layout is impossible and the single-line rows
/// have nowhere to put a badge; the whole screen is the honest answer.
const MIN_INLINE_COLUMNS: u16 = 40;

/// A promotion diff longer than this cannot be reviewed in a strip, and a
/// promotion the user cannot read is a promotion they will approve blind.
const FULLSCREEN_DIFF_LINES: usize = 24;

/// A captured result longer than this is a document, not a status line. Below it
/// the result panel scrolls, which is cheaper than taking the screen.
const FULLSCREEN_RESULT_LINES: usize = 60;

/// Where the palette draws itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiHost {
    /// A real tmux `display-popup` overlay.
    TmuxPopup,
    /// A strip at the bottom of the current terminal, full width, this many rows.
    Inline(u16),
    /// The alternate screen.
    Fullscreen,
}

impl UiHost {
    /// An inline host clamped into the documented band.
    ///
    /// Clamping at construction rather than at render time means no caller — not
    /// a config file, not a flag, not a future adapter — can produce a
    /// three-row palette or a forty-row "overlay".
    pub fn inline(rows: u16) -> Self {
        Self::Inline(rows.clamp(INLINE_MIN_ROWS, INLINE_MAX_ROWS))
    }

    /// Pick a host for an environment.
    pub fn choose(profile: &TerminalProfile) -> Self {
        if let Some(explicit) = profile.explicit {
            return explicit;
        }
        if profile.cols < MIN_INLINE_COLUMNS || profile.rows < INLINE_MIN_ROWS + INLINE_HEADROOM {
            return Self::Fullscreen;
        }
        // tmux is the only environment with a documented overlay primitive.
        // cmux gets an inline modal, which its own design expects.
        if profile.mux == Some(MuxKind::Tmux) {
            return Self::TmuxPopup;
        }
        Self::inline(profile.rows.saturating_sub(INLINE_HEADROOM))
    }

    /// Does the user's screen survive the palette?
    pub fn preserves_scrollback(self) -> bool {
        match self {
            // A popup is drawn over the pane and removed again; the pane's own
            // buffer is never written to.
            UiHost::TmuxPopup | UiHost::Inline(_) => true,
            UiHost::Fullscreen => false,
        }
    }

    /// How many rows the palette actually gets on a terminal of this height.
    pub fn viewport_rows(self, terminal_rows: u16) -> u16 {
        match self {
            UiHost::Inline(rows) => rows.min(terminal_rows),
            UiHost::TmuxPopup | UiHost::Fullscreen => terminal_rows,
        }
    }

    /// The host to use once this content is known.
    ///
    /// One-way: a host that is already fullscreen never de-escalates mid-session,
    /// because a palette that resized itself underneath the user would be worse
    /// than one that is simply too big.
    pub fn escalated_for(self, reason: Escalation) -> Self {
        if reason.warrants_fullscreen() {
            Self::Fullscreen
        } else {
            self
        }
    }
}

/// Why the palette might need the whole screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escalation {
    /// The user asked for it.
    Requested,
    /// There is not enough terminal for anything smaller.
    TerminalTooSmall { cols: u16, rows: u16 },
    /// A promotion is about to be approved and its diff must be readable.
    LargePromotionDiff { lines: usize },
    /// A captured result that is a document rather than a status line.
    LargeResult { lines: usize },
}

impl Escalation {
    fn warrants_fullscreen(self) -> bool {
        match self {
            Escalation::Requested => true,
            Escalation::TerminalTooSmall { cols, rows } => {
                cols < MIN_INLINE_COLUMNS || rows < INLINE_MIN_ROWS + INLINE_HEADROOM
            }
            Escalation::LargePromotionDiff { lines } => lines > FULLSCREEN_DIFF_LINES,
            Escalation::LargeResult { lines } => lines > FULLSCREEN_RESULT_LINES,
        }
    }

    /// The sentence shown when the palette takes the screen, so the change is
    /// never unexplained.
    pub fn describe(self) -> String {
        match self {
            Escalation::Requested => "fullscreen requested".to_string(),
            Escalation::TerminalTooSmall { cols, rows } => {
                format!("terminal is {cols}×{rows}, too small for an inline palette")
            }
            Escalation::LargePromotionDiff { lines } => {
                format!("promotion diff is {lines} lines")
            }
            Escalation::LargeResult { lines } => format!("result is {lines} lines"),
        }
    }
}

/// What the palette knows about the terminal it was invoked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalProfile {
    pub cols: u16,
    pub rows: u16,
    pub mux: Option<MuxKind>,
    /// A host named on the command line or in configuration.
    pub explicit: Option<UiHost>,
}

impl TerminalProfile {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            mux: None,
            explicit: None,
        }
    }

    #[must_use]
    pub fn in_mux(mut self, mux: MuxKind) -> Self {
        self.mux = Some(mux);
        self
    }

    #[must_use]
    pub fn requested(mut self, host: UiHost) -> Self {
        self.explicit = Some(host);
        self
    }
}
