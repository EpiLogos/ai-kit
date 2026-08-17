//! Responsive geometry, and the glyphs that carry state.
//!
//! ## Degrade by dropping, never by truncating
//!
//! Narrowing the terminal removes whole columns rather than shortening every
//! cell. A description clipped to eight characters is worse than no description:
//! it looks like information and is not. So the description goes first, then the
//! kind and trust columns, and what remains at forty columns is a state pair, a
//! scope badge and a name — each of which is still complete.
//!
//! ## Declared and effective are two marks, never one checkbox
//!
//! `ARCHITECTURE.md` §4 is explicit that a layer may declare a capability enabled
//! while it is nevertheless unavailable, and that this is a different rendering
//! rather than an error. A single checkbox cannot say that. Every row therefore
//! carries a [`Declared`] mark (what a scope said) next to a
//! [`DocStatus`] mark (what the resolver did), and they are drawn with disjoint
//! glyph sets so no state can be mistaken for another.
//!
//! ## No colour, Unicode or Nerd Font is load-bearing
//!
//! [`Glyphs::ascii`] carries exactly the same distinctions as
//! [`Glyphs::unicode`]. Nerd Font glyphs appear nowhere at all: a private-use
//! codepoint that renders as a box on a stock terminal is not a fallback story,
//! it is a bug waiting for someone else's machine.

use aikit_core::resolve::UnavailableReason;
use aikit_core::scope::ScopeKind;
use aikit_core::search::DocStatus;
use ratatui::layout::Rect;

/// Below this the list and a preview cannot both be useful.
const WIDE_COLUMNS: u16 = 100;
/// Below this a row cannot hold more than a name.
const MEDIUM_COLUMNS: u16 = 60;
/// The preview's share of a wide terminal.
const PREVIEW_NUMERATOR: u16 = 2;
const PREVIEW_DENOMINATOR: u16 = 5;

/// Which of the three renderings applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// ≥ 100 columns: search and list on the left, preview on the right.
    Wide,
    /// 60–99 columns: list only; the preview replaces it on demand.
    Medium,
    /// < 60 columns: single-line rows, minimal badges, details behind Enter.
    Narrow,
}

/// The panes one frame is divided into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub query: Rect,
    pub list: Rect,
    /// `None` at every width below [`Width::Wide`].
    pub preview: Option<Rect>,
    pub footer: Rect,
}

impl Panes {
    pub fn all(&self) -> Vec<Rect> {
        let mut out = vec![self.query, self.list, self.footer];
        out.extend(self.preview);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub width: Width,
}

impl Layout {
    pub fn for_width(cols: u16) -> Self {
        let width = if cols >= WIDE_COLUMNS {
            Width::Wide
        } else if cols >= MEDIUM_COLUMNS {
            Width::Medium
        } else {
            Width::Narrow
        };
        Self { width }
    }

    /// Divide a frame. One query line at the top, one footer line at the bottom,
    /// the rest to the list and — when there is room — a preview beside it.
    pub fn split(&self, area: Rect) -> Panes {
        let query = Rect {
            height: 1.min(area.height),
            ..area
        };
        let footer_height = if area.height > 2 { 1 } else { 0 };
        let footer = Rect {
            y: area.y + area.height.saturating_sub(footer_height),
            height: footer_height,
            ..area
        };
        let body = Rect {
            y: area.y + query.height,
            height: area
                .height
                .saturating_sub(query.height)
                .saturating_sub(footer_height),
            ..area
        };

        if self.width != Width::Wide {
            return Panes {
                query,
                list: body,
                preview: None,
                footer: Rect {
                    height: footer_height.max(1),
                    ..footer
                },
            };
        }

        let preview_width = (body.width * PREVIEW_NUMERATOR / PREVIEW_DENOMINATOR).max(30);
        let list_width = body.width.saturating_sub(preview_width);
        Panes {
            query,
            list: Rect {
                width: list_width,
                ..body
            },
            preview: Some(Rect {
                x: body.x + list_width,
                width: preview_width,
                ..body
            }),
            footer: Rect {
                height: footer_height.max(1),
                ..footer
            },
        }
    }

    /// Does opening the preview cost the user the list?
    pub fn preview_replaces_list(&self) -> bool {
        self.width != Width::Wide
    }

    /// At the narrowest width a row is a single line and details wait for Enter.
    pub fn details_on_enter(&self) -> bool {
        self.width == Width::Narrow
    }

    pub fn shows_description(&self) -> bool {
        self.width != Width::Narrow
    }

    /// Is there room for prose rather than a compact hint?
    ///
    /// A sentence clipped mid-word is worse than four characters: the characters
    /// are complete, and the lane hint's job is to say the lanes exist.
    pub fn has_room_for_prose(&self) -> bool {
        self.width == Width::Wide
    }

    pub fn shows_kind_column(&self) -> bool {
        self.width == Width::Wide
    }

    pub fn shows_trust_column(&self) -> bool {
        self.width == Width::Wide
    }
}

// ---------------------------------------------------------------------------
// Glyphs
// ---------------------------------------------------------------------------

/// What a scope said about a capability, independent of what the resolver did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared {
    Enabled,
    Disabled,
    /// No layer mentions it. It may still be active — as somebody's dependency.
    Undeclared,
}

/// The character set a rendering uses.
///
/// Two complete sets rather than a per-glyph fallback: a mixed rendering, where
/// three marks are Unicode and one is ASCII because someone forgot, is how the
/// fallback silently rots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    ascii: bool,
}

impl Glyphs {
    pub fn unicode() -> Self {
        Self { ascii: false }
    }

    pub fn ascii() -> Self {
        Self { ascii: true }
    }

    /// The set this terminal can actually render.
    ///
    /// `AIKIT_ASCII` is checked first because a user who has been bitten by a
    /// terminal that claims UTF-8 and draws boxes needs a way to say so.
    pub fn from_env() -> Self {
        if std::env::var_os("AIKIT_ASCII").is_some() {
            return Self::ascii();
        }
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|key| {
            std::env::var(key)
                .map(|v| v.to_ascii_lowercase().contains("utf-8") || v.to_ascii_lowercase().contains("utf8"))
                .unwrap_or(false)
        });
        if utf8 {
            Self::unicode()
        } else {
            Self::ascii()
        }
    }

    pub fn is_ascii(&self) -> bool {
        self.ascii
    }

    /// What a scope declared.
    pub fn declared(&self, declared: Declared) -> char {
        match (declared, self.ascii) {
            (Declared::Enabled, _) => '+',
            (Declared::Disabled, false) => '×',
            (Declared::Disabled, true) => 'x',
            (Declared::Undeclared, false) => '·',
            (Declared::Undeclared, true) => '.',
        }
    }

    /// What the resolver actually decided.
    pub fn effective(&self, status: DocStatus) -> char {
        match (status, self.ascii) {
            (DocStatus::Active, false) => '●',
            (DocStatus::Active, true) => '*',
            (DocStatus::Inactive, false) => '○',
            (DocStatus::Inactive, true) => '-',
            (DocStatus::Unavailable, _) => '!',
        }
    }

    /// The mark on a row the user has staged but not applied.
    pub fn staged(&self) -> char {
        if self.ascii {
            '~'
        } else {
            '◆'
        }
    }

    /// The cursor.
    pub fn selected(&self) -> char {
        if self.ascii {
            '>'
        } else {
            '❯'
        }
    }

    /// The scope that declared a capability, or a blank when none did.
    ///
    /// Core owns the letters; repeating them here would mean two places to change
    /// and one of them would be missed.
    pub fn scope_badge(&self, scope: Option<ScopeKind>) -> char {
        match scope {
            Some(scope) => scope.badge(),
            None => ' ',
        }
    }
}

/// The one-line state sentence for a row or a preview header.
///
/// The unavailable case borrows core's own wording rather than paraphrasing it,
/// so the palette and `aikit explain` cannot drift into describing the same
/// refusal two different ways.
pub fn state_note(status: DocStatus, reason: Option<&UnavailableReason>) -> String {
    match (status, reason) {
        (DocStatus::Active, _) => "active".to_string(),
        (DocStatus::Inactive, _) => "inactive".to_string(),
        (DocStatus::Unavailable, Some(reason)) => format!("unavailable — {}", reason.describe()),
        // The resolver records a reason for everything it withholds; a row that
        // reaches here is a bug, and saying so beats inventing a cause.
        (DocStatus::Unavailable, None) => "unavailable — no reason recorded".to_string(),
    }
}
