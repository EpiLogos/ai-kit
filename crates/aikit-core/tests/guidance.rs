//! Guidance composition exists to stop AIKit doing the thing every "context
//! manager" eventually does: concatenating an unbounded wall of Markdown into
//! somebody's prompt. The budget is therefore a hard property, not advice, and
//! these tests assert it on the composed text rather than on an internal counter.

use aikit_core::guidance::{
    compose, estimate_tokens, Composition, CompositionRequest, FragmentStatus, GuidanceFragment,
};
use aikit_core::hooks::HookEventKind;
use aikit_core::id::CapsuleId;
use aikit_core::platform::TargetId;

fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

/// A body whose whitespace-normalized length is exactly `tokens * 4` characters,
/// so the estimate is exact and the arithmetic in these tests is checkable.
fn body(tokens: u32) -> String {
    "x".repeat(tokens as usize * 4)
}

fn fragment(id: &str, order: i32, tokens: u32) -> GuidanceFragment {
    GuidanceFragment::new(cid(id), body(tokens)).with_order(order)
}

fn request(budget: u32) -> CompositionRequest {
    CompositionRequest::new(
        HookEventKind::UserPromptSubmit,
        TargetId::claude_code(),
        budget,
    )
}

fn included(composition: &Composition) -> Vec<String> {
    composition
        .entries
        .iter()
        .filter(|e| e.status.is_included())
        .map(|e| e.capsule.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Ordering
// ---------------------------------------------------------------------------

#[test]
fn fragments_are_ordered_by_declared_order_then_capsule_id() {
    let composition = compose(
        vec![
            fragment("guidance/z/last", 10, 1),
            fragment("guidance/a/also-ten", 10, 1),
            fragment("guidance/m/first", 1, 1),
        ],
        &request(100),
    );

    assert_eq!(
        included(&composition),
        vec![
            "guidance/m/first",
            "guidance/a/also-ten",
            "guidance/z/last"
        ]
    );
}

#[test]
fn the_composed_text_follows_the_same_order_as_the_record() {
    let composition = compose(
        vec![
            GuidanceFragment::new(cid("guidance/b/second"), "second").with_order(20),
            GuidanceFragment::new(cid("guidance/a/first"), "first").with_order(10),
        ],
        &request(100),
    );
    assert_eq!(composition.text, "first\n\nsecond");
}

// ---------------------------------------------------------------------------
// Deduplication
// ---------------------------------------------------------------------------

#[test]
fn fragments_sharing_a_dedup_key_are_included_once_and_the_highest_precedence_wins() {
    // The session-scoped fragment is declared later and is longer, but precedence
    // — not position — is what decides.
    let composition = compose(
        vec![
            GuidanceFragment::new(cid("guidance/global/research"), body(4))
                .with_order(10)
                .with_dedup_key("research-mode")
                .with_precedence(0),
            GuidanceFragment::new(cid("guidance/session/research"), body(9))
                .with_order(20)
                .with_dedup_key("research-mode")
                .with_precedence(4),
        ],
        &request(100),
    );

    assert_eq!(included(&composition), vec!["guidance/session/research"]);
    assert_eq!(composition.text, body(9));

    let loser = composition.entry("guidance/global/research").unwrap();
    assert_eq!(
        loser.status,
        FragmentStatus::SkippedDuplicate {
            winner: cid("guidance/session/research")
        }
    );
}

#[test]
fn a_dedup_tie_is_broken_by_composition_order_so_the_result_is_deterministic() {
    let build = || {
        compose(
            vec![
                GuidanceFragment::new(cid("guidance/b/two"), "two")
                    .with_order(20)
                    .with_dedup_key("same"),
                GuidanceFragment::new(cid("guidance/a/one"), "one")
                    .with_order(10)
                    .with_dedup_key("same"),
            ],
            &request(100),
        )
    };
    assert_eq!(included(&build()), vec!["guidance/a/one"]);
    assert_eq!(build().text, build().text);
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

#[test]
fn a_fragment_that_would_blow_the_total_budget_is_skipped_whole_rather_than_truncated() {
    let composition = compose(
        vec![
            fragment("guidance/a/small", 10, 4),
            fragment("guidance/b/huge", 20, 500),
        ],
        &request(20),
    );

    assert_eq!(included(&composition), vec!["guidance/a/small"]);
    assert_eq!(
        composition.text,
        body(4),
        "the oversized fragment must not appear at all, not even partially"
    );
    assert!(matches!(
        composition.entry("guidance/b/huge").unwrap().status,
        FragmentStatus::SkippedOverTotalBudget { .. }
    ));
}

#[test]
fn a_smaller_later_fragment_still_fits_after_a_bigger_one_was_skipped() {
    // Skipping is per fragment, not a stop condition: losing a short, high-order
    // instruction because a long one happened to precede it would be arbitrary.
    let composition = compose(
        vec![
            fragment("guidance/a/small", 10, 4),
            fragment("guidance/b/huge", 20, 500),
            fragment("guidance/c/also-small", 30, 4),
        ],
        &request(20),
    );

    assert_eq!(
        included(&composition),
        vec!["guidance/a/small", "guidance/c/also-small"]
    );
}

#[test]
fn the_composed_text_is_always_within_the_total_budget() {
    let fragments: Vec<GuidanceFragment> = (0..30)
        .map(|i| fragment(&format!("guidance/bulk/f{i:02}"), i, 17))
        .collect();

    for budget in [0, 1, 5, 17, 18, 40, 100, 999] {
        let composition = compose(fragments.clone(), &request(budget));
        assert!(
            estimate_tokens(&composition.text) <= budget,
            "budget {budget} was exceeded: {} tokens of text",
            estimate_tokens(&composition.text)
        );
        assert!(composition.used_tokens <= budget);
    }
}

#[test]
fn a_zero_budget_composes_nothing_but_still_explains_itself() {
    let composition = compose(vec![fragment("guidance/a/one", 10, 1)], &request(0));
    assert!(composition.text.is_empty());
    assert_eq!(composition.entries.len(), 1);
    assert!(matches!(
        composition.entries[0].status,
        FragmentStatus::SkippedOverTotalBudget { remaining: 0 }
    ));
}

#[test]
fn a_fragment_over_its_own_budget_is_included_but_recorded_as_over_budget() {
    // The per-fragment budget is the capsule author's own claim about size. It is
    // worth reporting when it is wrong, but it is not the operator's budget and
    // must not silently drop the author's guidance.
    let composition = compose(
        vec![GuidanceFragment::new(cid("guidance/a/verbose"), body(30))
            .with_order(10)
            .with_per_fragment_budget(10)],
        &request(100),
    );

    assert_eq!(included(&composition), vec!["guidance/a/verbose"]);
    assert_eq!(
        composition.entries[0].status,
        FragmentStatus::IncludedOverFragmentBudget { budget: 10 }
    );
    assert_eq!(composition.over_budget_fragments().len(), 1);
}

#[test]
fn an_empty_fragment_contributes_nothing_and_leaves_no_stray_separator() {
    let composition = compose(
        vec![
            GuidanceFragment::new(cid("guidance/a/one"), "one").with_order(10),
            GuidanceFragment::new(cid("guidance/b/blank"), "   \n  ").with_order(20),
            GuidanceFragment::new(cid("guidance/c/two"), "two").with_order(30),
        ],
        &request(100),
    );

    assert_eq!(composition.text, "one\n\ntwo");
    assert_eq!(
        composition.entry("guidance/b/blank").unwrap().status,
        FragmentStatus::SkippedEmpty
    );
}

// ---------------------------------------------------------------------------
// Estimation
// ---------------------------------------------------------------------------

#[test]
fn estimation_normalizes_whitespace_so_formatting_does_not_change_the_budget() {
    let tight = "the quick brown fox";
    let loose = "the   quick\n\n\tbrown     fox\n";
    assert_eq!(estimate_tokens(tight), estimate_tokens(loose));
}

#[test]
fn estimation_is_roughly_four_characters_per_token_and_never_rounds_to_zero() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("    "), 0);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abcde"), 2, "a partial token still costs one");
    assert_eq!(estimate_tokens(&"x".repeat(400)), 100);
}

#[test]
fn estimation_is_deterministic_across_calls() {
    let text = "Prefer the project's own test runner over a global one.";
    assert_eq!(estimate_tokens(text), estimate_tokens(text));
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

#[test]
fn the_composition_record_renders_the_documented_table() {
    let composition = compose(
        vec![
            GuidanceFragment::new(cid("guidance/mode/research"), body(12))
                .with_order(10)
                .with_dedup_key("research-mode")
                .with_precedence(4),
            GuidanceFragment::new(cid("guidance/rust/style"), body(5))
                .with_order(20)
                .with_per_fragment_budget(3),
            GuidanceFragment::new(cid("guidance/team/research"), body(2))
                .with_order(30)
                .with_dedup_key("research-mode")
                .with_precedence(0),
            GuidanceFragment::new(cid("guidance/big/handbook"), body(100)).with_order(40),
        ],
        &request(30),
    );

    assert_eq!(
        composition.render_record(),
        "guidance composition — UserPromptSubmit → claude-code\n\
         \x20 guidance/mode/research   12 tokens  included\n\
         \x20 guidance/rust/style       5 tokens  included, over its own 3-token budget\n\
         \x20 guidance/team/research    2 tokens  skipped: duplicate of guidance/mode/research\n\
         \x20 guidance/big/handbook   100 tokens  skipped: only 12 tokens of budget remained\n\
         \x20 total                    18 / 30 tokens\n"
    );
}

#[test]
fn an_empty_composition_still_renders_a_record() {
    let composition = compose(vec![], &request(2000));
    assert_eq!(
        composition.render_record(),
        "guidance composition — UserPromptSubmit → claude-code\n\
         \x20 nothing to compose\n\
         \x20 total  0 / 2000 tokens\n"
    );
}

#[test]
fn composition_is_a_pure_function_of_its_inputs() {
    let fragments = vec![
        fragment("guidance/a/one", 10, 3),
        fragment("guidance/b/two", 20, 3),
    ];
    let a = compose(fragments.clone(), &request(50));
    let b = compose(fragments, &request(50));
    assert_eq!(a.text, b.text);
    assert_eq!(a.render_record(), b.render_record());
    assert_eq!(a.used_tokens, b.used_tokens);
}
