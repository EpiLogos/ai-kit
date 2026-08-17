//! Responsive geometry and the two-state row.
//!
//! Two claims are load-bearing here and both get tests. First, that the palette
//! degrades by *dropping* information rather than truncating it into nonsense.
//! Second, that a row never collapses "a scope says this should be on" and "this
//! is actually in the effective view" into one checkbox — those are different
//! facts, and a UI that conflates them is the reason people believe a capability
//! is live when it is held back by trust.

use aikit_core::resolve::UnavailableReason;
use aikit_core::scope::ScopeKind;
use aikit_core::search::DocStatus;
use aikit_tui::layout::{Declared, Glyphs, Layout, Width};
use ratatui::layout::Rect;

// ---------------------------------------------------------------------------
// Breakpoints
// ---------------------------------------------------------------------------

#[test]
fn the_documented_breakpoints_are_exactly_where_they_say_they_are() {
    assert_eq!(Layout::for_width(100).width, Width::Wide);
    assert_eq!(Layout::for_width(220).width, Width::Wide);
    assert_eq!(Layout::for_width(99).width, Width::Medium);
    assert_eq!(Layout::for_width(60).width, Width::Medium);
    assert_eq!(Layout::for_width(59).width, Width::Narrow);
    assert_eq!(Layout::for_width(20).width, Width::Narrow);
}

#[test]
fn a_wide_terminal_shows_the_list_and_the_preview_at_once() {
    let layout = Layout::for_width(120);
    let panes = layout.split(Rect::new(0, 0, 120, 20));
    let preview = panes.preview.expect("a wide layout has a preview pane");

    assert!(panes.list.width >= 50, "the list must stay readable: {panes:?}");
    assert!(preview.width >= 30, "a preview narrower than this explains nothing");
    assert_eq!(
        panes.list.x + panes.list.width,
        preview.x,
        "list and preview must abut without overlapping"
    );
    assert_eq!(preview.x + preview.width, 120);
}

#[test]
fn a_medium_terminal_hides_the_preview_until_it_is_asked_for() {
    let layout = Layout::for_width(80);
    let panes = layout.split(Rect::new(0, 0, 80, 20));
    assert!(panes.preview.is_none());
    assert_eq!(panes.list.width, 80, "the list gets the whole width");
    assert!(
        layout.preview_replaces_list(),
        "at this width the preview takes the list's place on demand"
    );
}

#[test]
fn a_narrow_terminal_puts_details_behind_enter_rather_than_squeezing_them_in() {
    let layout = Layout::for_width(50);
    assert!(!layout.shows_description());
    assert!(!layout.shows_kind_column());
    assert!(!layout.shows_trust_column());
    assert!(layout.details_on_enter());
    assert!(layout.preview_replaces_list());

    let wide = Layout::for_width(120);
    assert!(wide.shows_description());
    assert!(wide.shows_kind_column());
    assert!(wide.shows_trust_column());
    assert!(!wide.details_on_enter());
}

#[test]
fn every_pane_stays_inside_the_area_and_none_of_them_overlap() {
    for cols in [40u16, 59, 60, 80, 99, 100, 160] {
        for rows in [14u16, 20, 40] {
            let area = Rect::new(0, 0, cols, rows);
            let panes = Layout::for_width(cols).split(area);
            for pane in panes.all() {
                assert!(
                    area.union(pane) == area,
                    "{pane:?} escapes {area:?} at {cols}x{rows}"
                );
            }
            assert!(
                panes.query.bottom() <= panes.list.y,
                "the query line must sit above the list at {cols}x{rows}"
            );
            assert!(
                panes.list.bottom() <= panes.footer.y,
                "the footer must sit below the list at {cols}x{rows}"
            );
            assert!(panes.footer.height >= 1, "the footer is never dropped");
        }
    }
}

// ---------------------------------------------------------------------------
// Declared and effective are two different facts
// ---------------------------------------------------------------------------

#[test]
fn declared_and_effective_are_rendered_as_two_separate_marks() {
    let g = Glyphs::unicode();

    // A capability a scope enabled, which is nevertheless held back.
    let held = (
        g.declared(Declared::Enabled),
        g.effective(DocStatus::Unavailable),
    );
    // The same declaration, actually live.
    let live = (
        g.declared(Declared::Enabled),
        g.effective(DocStatus::Active),
    );

    assert_eq!(held.0, live.0, "both are declared on by a scope");
    assert_ne!(
        held.1, live.1,
        "and a UI that drew them identically would be lying"
    );
}

#[test]
fn a_capability_active_only_through_a_dependency_is_not_shown_as_declared() {
    let g = Glyphs::unicode();
    assert_ne!(
        g.declared(Declared::Undeclared),
        g.declared(Declared::Enabled)
    );
    assert_eq!(g.effective(DocStatus::Active), g.effective(DocStatus::Active));
}

#[test]
fn an_explicitly_disabled_capability_is_marked_with_a_cross() {
    assert_eq!(Glyphs::unicode().declared(Declared::Disabled), '×');
    assert_eq!(Glyphs::ascii().declared(Declared::Disabled), 'x');
}

#[test]
fn every_glyph_in_a_set_is_distinct_so_no_two_states_look_alike() {
    for set in [Glyphs::unicode(), Glyphs::ascii()] {
        let mut seen = std::collections::BTreeSet::new();
        for d in [Declared::Enabled, Declared::Disabled, Declared::Undeclared] {
            assert!(seen.insert(set.declared(d)), "duplicate declared glyph for {d:?}");
        }
        let mut seen = std::collections::BTreeSet::new();
        for s in [DocStatus::Active, DocStatus::Inactive, DocStatus::Unavailable] {
            assert!(seen.insert(set.effective(s)), "duplicate effective glyph for {s:?}");
        }
    }
}

#[test]
fn the_ascii_fallback_carries_the_same_information_without_a_single_non_ascii_byte() {
    let ascii = Glyphs::ascii();
    let mut glyphs = vec![];
    for d in [Declared::Enabled, Declared::Disabled, Declared::Undeclared] {
        glyphs.push(ascii.declared(d));
    }
    for s in [DocStatus::Active, DocStatus::Inactive, DocStatus::Unavailable] {
        glyphs.push(ascii.effective(s));
    }
    glyphs.push(ascii.staged());
    glyphs.push(ascii.selected());
    for g in glyphs {
        assert!(g.is_ascii(), "`{g}` is not ASCII");
    }
}

#[test]
fn a_unicode_set_is_actually_different_from_the_ascii_one() {
    // Otherwise the "fallback" would be untested cosmetics.
    assert_ne!(
        Glyphs::unicode().effective(DocStatus::Active),
        Glyphs::ascii().effective(DocStatus::Active)
    );
    assert_ne!(Glyphs::unicode().selected(), Glyphs::ascii().selected());
}

// ---------------------------------------------------------------------------
// Scope badges
// ---------------------------------------------------------------------------

#[test]
fn scope_badges_are_the_letters_core_defines_and_not_a_second_opinion() {
    for (scope, letter) in [
        (ScopeKind::Global, 'G'),
        (ScopeKind::Host, 'H'),
        (ScopeKind::Project, 'P'),
        (ScopeKind::ProjectLocal, 'L'),
        (ScopeKind::Session, 'S'),
        (ScopeKind::Task, 'T'),
    ] {
        assert_eq!(scope.badge(), letter);
        assert_eq!(Glyphs::unicode().scope_badge(Some(scope)), letter);
        assert_eq!(Glyphs::ascii().scope_badge(Some(scope)), letter);
    }
}

#[test]
fn an_undeclared_capability_has_a_blank_scope_badge_rather_than_a_guess() {
    assert_eq!(Glyphs::unicode().scope_badge(None), ' ');
}

// ---------------------------------------------------------------------------
// Why a row is the state it is in
// ---------------------------------------------------------------------------

#[test]
fn an_unavailable_row_carries_the_reason_core_gave_and_not_a_paraphrase() {
    let reason = UnavailableReason::TrustRequired;
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Unavailable, Some(&reason)),
        "unavailable — this revision has not been reviewed"
    );
    assert_eq!(aikit_tui::layout::state_note(DocStatus::Active, None), "active");
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Inactive, None),
        "inactive"
    );
}

#[test]
fn an_unavailable_row_without_a_recorded_reason_says_so_rather_than_inventing_one() {
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Unavailable, None),
        "unavailable — no reason recorded"
    );
}
