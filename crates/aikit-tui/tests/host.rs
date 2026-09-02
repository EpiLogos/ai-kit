//! Where the palette appears.
//!
//! The default is deliberately the least disruptive thing that still fits: an
//! inline strip that leaves the terminal's scrollback alone. Fullscreen is an
//! escalation with a named reason, never a default, because taking over someone's
//! screen to show them three rows is exactly the "control centre" this is not.

use aikit_core::platform::MuxKind;
use aikit_tui::host::{Escalation, TerminalProfile, UiHost, INLINE_MAX_ROWS, INLINE_MIN_ROWS};

#[test]
fn a_plain_terminal_gets_an_inline_palette_that_preserves_scrollback() {
    let host = UiHost::choose(&TerminalProfile::new(120, 40));
    assert!(matches!(host, UiHost::Inline(_)), "got {host:?}");
    assert!(host.preserves_scrollback());
}

#[test]
fn cmux_gets_an_inline_palette_because_it_has_no_documented_popup_primitive() {
    let profile = TerminalProfile::new(120, 40).in_mux(MuxKind::Cmux);
    assert!(matches!(UiHost::choose(&profile), UiHost::Inline(_)));
}

#[test]
fn tmux_gets_a_real_popup_because_it_has_one() {
    let profile = TerminalProfile::new(120, 40).in_mux(MuxKind::Tmux);
    assert_eq!(UiHost::choose(&profile), UiHost::TmuxPopup);
}

#[test]
fn an_inline_palette_stays_within_the_documented_row_band() {
    for rows in 16..=200u16 {
        let host = UiHost::choose(&TerminalProfile::new(100, rows));
        let UiHost::Inline(inline_rows) = host else {
            panic!("a {rows}-row terminal should host inline, got {host:?}");
        };
        assert!(
            (INLINE_MIN_ROWS..=INLINE_MAX_ROWS).contains(&inline_rows),
            "{inline_rows} rows is outside the 14–20 band for a {rows}-row terminal"
        );
        assert!(
            inline_rows < rows,
            "an inline palette must leave the terminal something to scroll back to"
        );
    }
}

#[test]
fn a_terminal_too_short_for_an_inline_strip_goes_fullscreen_rather_than_cramped() {
    // 15 rows cannot hold a 14-row strip plus any surrounding context, so there
    // is nothing left to preserve and the honest answer is the whole screen.
    assert_eq!(
        UiHost::choose(&TerminalProfile::new(100, 15)),
        UiHost::Fullscreen
    );
    assert_eq!(
        UiHost::choose(&TerminalProfile::new(30, 40)),
        UiHost::Fullscreen
    );
}

#[test]
fn an_explicit_request_wins_over_every_inference() {
    let profile = TerminalProfile::new(120, 40)
        .in_mux(MuxKind::Tmux)
        .requested(UiHost::Fullscreen);
    assert_eq!(UiHost::choose(&profile), UiHost::Fullscreen);

    let profile = TerminalProfile::new(100, 15).requested(UiHost::Inline(14));
    assert_eq!(UiHost::choose(&profile), UiHost::Inline(14));
}

#[test]
fn a_large_promotion_diff_escalates_an_inline_palette_to_fullscreen() {
    let inline = UiHost::Inline(16);
    assert_eq!(
        inline.escalated_for(Escalation::LargePromotionDiff { lines: 400 }),
        UiHost::Fullscreen
    );
    // A popup is no better a place for 400 lines than a 16-row strip.
    assert_eq!(
        UiHost::TmuxPopup.escalated_for(Escalation::LargePromotionDiff { lines: 400 }),
        UiHost::Fullscreen
    );
}

#[test]
fn a_small_result_does_not_take_over_the_screen() {
    let inline = UiHost::Inline(16);
    assert_eq!(
        inline.escalated_for(Escalation::LargeResult { lines: 3 }),
        UiHost::Inline(16)
    );
    assert_eq!(
        UiHost::TmuxPopup.escalated_for(Escalation::LargePromotionDiff { lines: 2 }),
        UiHost::TmuxPopup
    );
}

#[test]
fn a_long_captured_result_escalates_but_a_merely_tall_one_does_not() {
    let inline = UiHost::Inline(20);
    assert_eq!(
        inline.escalated_for(Escalation::LargeResult { lines: 21 }),
        UiHost::Inline(20),
        "a result a little taller than the strip scrolls; it does not warrant the screen"
    );
    assert_eq!(
        inline.escalated_for(Escalation::LargeResult { lines: 500 }),
        UiHost::Fullscreen
    );
}

#[test]
fn escalation_is_one_way_and_fullscreen_stays_fullscreen() {
    assert_eq!(
        UiHost::Fullscreen.escalated_for(Escalation::LargeResult { lines: 1 }),
        UiHost::Fullscreen
    );
}

#[test]
fn an_inline_host_is_clamped_at_construction_so_no_caller_can_ask_for_a_takeover() {
    assert_eq!(UiHost::inline(3), UiHost::Inline(INLINE_MIN_ROWS));
    assert_eq!(UiHost::inline(999), UiHost::Inline(INLINE_MAX_ROWS));
}

#[test]
fn only_the_inline_host_promises_to_preserve_scrollback() {
    assert!(UiHost::Inline(16).preserves_scrollback());
    assert!(
        UiHost::TmuxPopup.preserves_scrollback(),
        "a popup is drawn over the pane and leaves its buffer alone"
    );
    assert!(!UiHost::Fullscreen.preserves_scrollback());
}

#[test]
fn the_viewport_height_never_exceeds_the_terminal() {
    assert_eq!(UiHost::Inline(20).viewport_rows(40), 20);
    assert_eq!(UiHost::Inline(20).viewport_rows(12), 12);
    assert_eq!(UiHost::Fullscreen.viewport_rows(40), 40);
    assert_eq!(UiHost::TmuxPopup.viewport_rows(40), 40);
}
