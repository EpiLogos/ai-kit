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
use aikit_core::RelationDirection;
use ratatui::layout::Rect;
use ratatui::symbols::border;

/// Below this the list and a preview cannot both be useful.
const WIDE_COLUMNS: u16 = 100;
/// Below this a row cannot hold more than a name.
const MEDIUM_COLUMNS: u16 = 60;
/// The preview's share of a wide terminal.
const PREVIEW_NUMERATOR: u16 = 2;
const PREVIEW_DENOMINATOR: u16 = 5;
/// The Inspector's target width once a wide terminal has room for one (spec
/// `23-TUI-HUMAN-EXPERIENCE-SPEC.md` §2.1's "always visible" column). Fixed
/// rather than proportional, matching the preview pane's own `.max(30)`
/// floor: Inspector content is label/fact prose, not something that benefits
/// from stretching arbitrarily wide.
const INSPECTOR_COLUMNS: u16 = 28;
/// Preview keeps at least this many columns once Inspector is carved out of
/// its share. The list pane is never touched by Inspector's arrival — only
/// the preview pane, which was already sized from the same `body.width`,
/// gives up part of its own share.
const MIN_PREVIEW_WITH_INSPECTOR: u16 = 16;

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
    /// The persistent Inspector column (spec §2.1). `None` at every width
    /// below [`Width::Wide`], exactly like `preview` — narrow/medium keep
    /// their existing modal `Overlay::Explain` path unchanged. Carved from
    /// the preview pane's own share of a wide terminal, never from `list`:
    /// the resource list keeps exactly the width it had before Inspector
    /// existed.
    pub inspector: Option<Rect>,
    pub footer: Rect,
}

impl Panes {
    pub fn all(&self) -> Vec<Rect> {
        let mut out = vec![self.query, self.list, self.footer];
        out.extend(self.preview);
        out.extend(self.inspector);
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
                inspector: None,
                footer: Rect {
                    height: footer_height.max(1),
                    ..footer
                },
            };
        }

        // `list_width` is derived from `preview_width_total` exactly as it
        // always was — Inspector never takes from the list. It is carved out
        // of the preview share only, after that share is already fixed, so a
        // narrower preview is the one and only geometry cost of Inspector
        // existing.
        let preview_width_total = (body.width * PREVIEW_NUMERATOR / PREVIEW_DENOMINATOR).max(30);
        let list_width = body.width.saturating_sub(preview_width_total);
        let inspector_width =
            INSPECTOR_COLUMNS.min(preview_width_total.saturating_sub(MIN_PREVIEW_WITH_INSPECTOR));
        let preview_width = preview_width_total - inspector_width;
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
            inspector: Some(Rect {
                x: body.x + list_width + preview_width,
                width: inspector_width,
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
                .map(|v| {
                    v.to_ascii_lowercase().contains("utf-8")
                        || v.to_ascii_lowercase().contains("utf8")
                })
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

    /// Where a §5.1 composition step stands.
    ///
    /// Three marks, not two, because `open` and `not exposed` are different
    /// facts: one is a choice waiting for the person, the other is a contract
    /// this application boundary does not publish. Drawing them alike would
    /// send someone to make a choice that cannot be made. Each is one cell in
    /// both sets, so the spine's columns line up either way.
    pub fn step_determined(&self) -> char {
        if self.ascii {
            '*'
        } else {
            '\u{25cf}'
        }
    }

    /// See [`Self::step_determined`].
    pub fn step_open(&self) -> char {
        if self.ascii {
            'o'
        } else {
            '\u{25cb}'
        }
    }

    /// See [`Self::step_determined`]. Shares the `undeclared` dot deliberately:
    /// nothing has been declared here and nothing can be.
    pub fn step_not_exposed(&self) -> char {
        if self.ascii {
            '.'
        } else {
            '\u{b7}'
        }
    }

    /// The arrows named in a keycap hint for vertical movement.
    ///
    /// Written through `Glyphs` rather than as literals for the reason this
    /// module exists: one such literal is all it takes for an ASCII rendering
    /// to come out three-quarters Unicode.
    pub fn step_up(&self) -> &'static str {
        if self.ascii {
            "Up"
        } else {
            "\u{2191}"
        }
    }

    /// See [`Self::step_up`].
    pub fn step_down(&self) -> &'static str {
        if self.ascii {
            "Down"
        } else {
            "\u{2193}"
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

    // -- shell chrome -------------------------------------------------------
    //
    // The marks below are the ones the resting shell draws around its own
    // content: separators, cursors, elision, the keycap hints in a footer,
    // the branch marks of a tree row. They live here, beside the state marks,
    // for the reason the module header gives: one set, chosen once, so a
    // rendering cannot come out three-quarters Unicode because one call site
    // was written with a literal. Each ASCII form below is either the same
    // width as its Unicode form or wider by a documented amount, and the
    // wider ones (`ellipsis`, the keycap hints) are never drawn inside a
    // column whose width was computed from the Unicode form.

    /// The separator between chrome fields on one line.
    pub fn separator(&self) -> &'static str {
        if self.ascii {
            "-"
        } else {
            "\u{b7}"
        }
    }

    /// The mark that stands for text elided to fit.
    ///
    /// Three cells in ASCII against one in Unicode, matching
    /// `graph_layout::GraphGlyphs::ellipsis`. A caller eliding to a fixed
    /// width must therefore reserve this mark's own width rather than assume
    /// a single cell — see `v2_render::truncate`.
    pub fn ellipsis(&self) -> &'static str {
        if self.ascii {
            "..."
        } else {
            "\u{2026}"
        }
    }

    /// The cursor on the selected row of a list.
    ///
    /// Distinct from [`Self::selected`], which marks the palette's own
    /// selection: this one sits in a one-cell gutter drawn on every row,
    /// blank where there is no cursor, so it is exactly one cell in both
    /// sets and a row cannot shift sideways when the cursor arrives.
    pub fn list_cursor(&self) -> &'static str {
        if self.ascii {
            ">"
        } else {
            "\u{203a}"
        }
    }

    /// The cursor in the contextual-Action lane. One cell, for the same
    /// reason [`Self::list_cursor`] is.
    pub fn action_cursor(&self) -> &'static str {
        if self.ascii {
            ">"
        } else {
            "\u{2192}"
        }
    }

    /// The keycap hint for the vertical navigation keys.
    pub fn vertical_keys(&self) -> &'static str {
        if self.ascii {
            "^v"
        } else {
            "\u{2191}\u{2193}"
        }
    }

    /// The keycap hint for the horizontal navigation keys.
    pub fn horizontal_keys(&self) -> &'static str {
        if self.ascii {
            "<-/->"
        } else {
            "\u{2190}/\u{2192}"
        }
    }

    /// The dash that separates a statement from the reason for it.
    pub fn dash(&self) -> &'static str {
        if self.ascii {
            "--"
        } else {
            "\u{2014}"
        }
    }

    /// The sign that prefixes a removed count.
    pub fn minus(&self) -> &'static str {
        if self.ascii {
            "-"
        } else {
            "\u{2212}"
        }
    }

    /// A typed relation's direction, drawn between the two ends of a list
    /// row. Three cells in both sets, so a column of rows stays aligned
    /// either way.
    ///
    /// `graph_layout::GraphGlyphs::direction_glyph` answers the same question
    /// for the spatial Graph, where a connector shares a cell budget with
    /// lane geometry and is drawn as a bare arrowhead. A list row has the
    /// room for the shaft and reads better with it, so the two stay separate
    /// marks — behind, as of `ApplicationSurfaceController`, a single
    /// resolved capability.
    pub fn relation_arrow(&self, direction: RelationDirection) -> &'static str {
        match (direction, self.ascii) {
            (RelationDirection::Outgoing, false) => "\u{2500}\u{2500}\u{25b6}",
            (RelationDirection::Outgoing, true) => "-->",
            (RelationDirection::Incoming, false) => "\u{25c0}\u{2500}\u{2500}",
            (RelationDirection::Incoming, true) => "<--",
            (RelationDirection::Bidirectional, false) => "\u{25c0}\u{2500}\u{25b6}",
            (RelationDirection::Bidirectional, true) => "<->",
        }
    }

    /// The branch mark for a tree row with siblings after it.
    pub fn branch_tee(&self) -> &'static str {
        if self.ascii {
            "|"
        } else {
            "\u{251c}"
        }
    }

    /// The branch mark for the last row of a group.
    pub fn branch_last(&self) -> &'static str {
        if self.ascii {
            "`"
        } else {
            "\u{2514}"
        }
    }

    /// The horizontal run a branch mark hangs its label from.
    pub fn branch_stem(&self) -> &'static str {
        if self.ascii {
            "-"
        } else {
            "\u{2500}"
        }
    }

    /// The frame itself.
    ///
    /// `ratatui`'s own sets are all box-drawing, so a terminal that cannot
    /// render `\u{250c}` draws the palette's single border as a rectangle of
    /// replacement boxes — the loudest possible version of the defect this
    /// type exists to prevent, since the border is on screen at every width
    /// and in every state. The ASCII set is the conventional `+`/`-`/`|`
    /// frame, one cell per side exactly like `PLAIN`, so the geometry
    /// `Layout::split` computed is unaffected.
    ///
    /// `Theme` still owns *which* border (one, plain, never nested); this
    /// owns what it is drawn with.
    pub fn border_set(&self) -> border::Set<'static> {
        if self.ascii {
            border::Set {
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                vertical_left: "|",
                vertical_right: "|",
                horizontal_top: "-",
                horizontal_bottom: "-",
            }
        } else {
            border::PLAIN
        }
    }
}

/// The one-line state sentence for a row or a preview header.
///
/// The unavailable case borrows core's own wording rather than paraphrasing it,
/// so the palette and `aikit explain` cannot drift into describing the same
/// refusal two different ways.
pub fn state_note(status: DocStatus, reason: Option<&UnavailableReason>, glyphs: Glyphs) -> String {
    let dash = glyphs.dash();
    match (status, reason) {
        (DocStatus::Active, _) => "active".to_string(),
        (DocStatus::Inactive, _) => "inactive".to_string(),
        (DocStatus::Unavailable, Some(reason)) => {
            format!("unavailable {dash} {}", reason.describe())
        }
        // The resolver records a reason for everything it withholds; a row that
        // reaches here is a bug, and saying so beats inventing a cause.
        (DocStatus::Unavailable, None) => format!("unavailable {dash} no reason recorded"),
    }
}
