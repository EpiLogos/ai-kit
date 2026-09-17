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
use aikit_core::RelationDirection;
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

    assert!(
        panes.list.width >= 50,
        "the list must stay readable: {panes:?}"
    );
    assert!(
        preview.width >= 18,
        "a preview narrower than this explains nothing"
    );
    assert_eq!(
        panes.list.x + panes.list.width,
        preview.x,
        "list and preview must abut without overlapping"
    );
}

#[test]
fn a_wide_terminal_also_reserves_a_persistent_inspector_column() {
    // Spec §2.1: the Inspector is a persistent column in a wide shell. It is
    // carved out of the preview pane's own share of a wide terminal — the
    // list pane keeps exactly the width it always had (`the_documented_
    // breakpoints_are_exactly_where_they_say_they_are` and `a_wide_terminal_
    // shows_the_list_and_the_preview_at_once` above are unaffected by
    // Inspector's arrival for that reason) — so `preview` ends earlier than
    // it used to and `inspector` fills the gap up to the frame's edge.
    let layout = Layout::for_width(120);
    let panes = layout.split(Rect::new(0, 0, 120, 20));
    let preview = panes.preview.expect("a wide layout has a preview pane");
    let inspector = panes
        .inspector
        .expect("a wide layout has an Inspector column");

    assert!(
        inspector.width >= 18,
        "an Inspector narrower than this explains nothing"
    );
    assert_eq!(
        preview.x + preview.width,
        inspector.x,
        "preview and Inspector must abut without overlapping"
    );
    assert_eq!(
        inspector.x + inspector.width,
        120,
        "Inspector reaches the frame's edge, where preview alone used to"
    );
}

#[test]
fn medium_and_narrow_terminals_never_carry_an_inspector_column() {
    // Requirement: narrow/medium shell behaviour is unchanged — the existing
    // modal `Overlay::Explain` remains the only way in, exactly as before
    // Inspector existed.
    for cols in [40u16, 59, 60, 80, 99] {
        let panes = Layout::for_width(cols).split(Rect::new(0, 0, cols, 20));
        assert!(
            panes.inspector.is_none(),
            "{cols} columns must not carry an Inspector column"
        );
    }
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
    assert_eq!(
        g.effective(DocStatus::Active),
        g.effective(DocStatus::Active)
    );
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
            assert!(
                seen.insert(set.declared(d)),
                "duplicate declared glyph for {d:?}"
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for s in [
            DocStatus::Active,
            DocStatus::Inactive,
            DocStatus::Unavailable,
        ] {
            assert!(
                seen.insert(set.effective(s)),
                "duplicate effective glyph for {s:?}"
            );
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
    for s in [
        DocStatus::Active,
        DocStatus::Inactive,
        DocStatus::Unavailable,
    ] {
        glyphs.push(ascii.effective(s));
    }
    glyphs.push(ascii.staged());
    glyphs.push(ascii.selected());
    for g in glyphs {
        assert!(g.is_ascii(), "`{g}` is not ASCII");
    }
}

/// The state marks above were never the whole set: the shell also draws
/// separators, cursors, elision marks, keycap hints and branch marks, and
/// for a long time it drew them as literals — so `Glyphs::ascii()` was
/// honoured for six characters and ignored for the footer, which emitted
/// `↑↓` and `←/→` on a terminal that could render neither.
///
/// This enumerates the chrome vocabulary rather than sampling it. A mark
/// added to `Glyphs` and forgotten here is a mark nobody proved renders on a
/// non-UTF-8 terminal, so the list is meant to be extended in the same
/// commit that extends the type.
#[test]
fn the_ascii_shell_chrome_is_ascii_too() {
    let ascii = Glyphs::ascii();
    let mut marks = vec![
        ascii.separator(),
        ascii.ellipsis(),
        ascii.list_cursor(),
        ascii.action_cursor(),
        ascii.vertical_keys(),
        ascii.horizontal_keys(),
        ascii.dash(),
        ascii.minus(),
        ascii.branch_tee(),
        ascii.branch_last(),
        ascii.branch_stem(),
    ];
    for direction in [
        RelationDirection::Outgoing,
        RelationDirection::Incoming,
        RelationDirection::Bidirectional,
    ] {
        marks.push(ascii.relation_arrow(direction));
    }
    for mark in marks {
        assert!(mark.is_ascii(), "`{mark}` is not ASCII");
    }
}

/// Every chrome mark differs between the two sets, so none of them is
/// cosmetics that would go untested if the fallback broke.
#[test]
fn every_chrome_mark_actually_changes_between_the_sets() {
    let (a, u) = (Glyphs::ascii(), Glyphs::unicode());
    for (name, ascii, unicode) in [
        ("separator", a.separator(), u.separator()),
        ("ellipsis", a.ellipsis(), u.ellipsis()),
        ("list_cursor", a.list_cursor(), u.list_cursor()),
        ("action_cursor", a.action_cursor(), u.action_cursor()),
        ("vertical_keys", a.vertical_keys(), u.vertical_keys()),
        ("horizontal_keys", a.horizontal_keys(), u.horizontal_keys()),
        ("dash", a.dash(), u.dash()),
        ("minus", a.minus(), u.minus()),
        ("branch_tee", a.branch_tee(), u.branch_tee()),
        ("branch_last", a.branch_last(), u.branch_last()),
        ("branch_stem", a.branch_stem(), u.branch_stem()),
    ] {
        assert_ne!(ascii, unicode, "{name} is the same in both sets");
    }
}

/// The marks a row's column arithmetic depends on are the same width in
/// both sets, so swapping the set cannot shift a column sideways. The two
/// deliberate exceptions — the elision mark and the keycap hints — are
/// excluded here and documented on `Glyphs` itself: neither is ever drawn
/// inside a column whose width was computed from the Unicode form.
#[test]
fn the_column_bearing_marks_are_the_same_width_in_both_sets() {
    let (a, u) = (Glyphs::ascii(), Glyphs::unicode());
    for (name, ascii, unicode) in [
        ("separator", a.separator(), u.separator()),
        ("list_cursor", a.list_cursor(), u.list_cursor()),
        ("action_cursor", a.action_cursor(), u.action_cursor()),
        ("branch_tee", a.branch_tee(), u.branch_tee()),
        ("branch_last", a.branch_last(), u.branch_last()),
        ("branch_stem", a.branch_stem(), u.branch_stem()),
        (
            "relation_arrow/out",
            a.relation_arrow(RelationDirection::Outgoing),
            u.relation_arrow(RelationDirection::Outgoing),
        ),
        (
            "relation_arrow/in",
            a.relation_arrow(RelationDirection::Incoming),
            u.relation_arrow(RelationDirection::Incoming),
        ),
        (
            "relation_arrow/both",
            a.relation_arrow(RelationDirection::Bidirectional),
            u.relation_arrow(RelationDirection::Bidirectional),
        ),
    ] {
        assert_eq!(
            ascii.chars().count(),
            unicode.chars().count(),
            "{name} changes width between the sets"
        );
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
        aikit_tui::layout::state_note(DocStatus::Unavailable, Some(&reason), Glyphs::unicode()),
        "unavailable — this revision has not been reviewed"
    );
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Unavailable, Some(&reason), Glyphs::ascii()),
        "unavailable -- this revision has not been reviewed"
    );
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Active, None, Glyphs::unicode()),
        "active"
    );
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Inactive, None, Glyphs::unicode()),
        "inactive"
    );
}

#[test]
fn an_unavailable_row_without_a_recorded_reason_says_so_rather_than_inventing_one() {
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Unavailable, None, Glyphs::unicode()),
        "unavailable — no reason recorded"
    );
    assert_eq!(
        aikit_tui::layout::state_note(DocStatus::Unavailable, None, Glyphs::ascii()),
        "unavailable -- no reason recorded"
    );
}
