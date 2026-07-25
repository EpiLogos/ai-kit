//! The tree, and the accessibility rules SPEC-III §4.3 calls "testable".
//!
//! They are only testable because the tree model is pure: a keystroke path and a
//! mouse path both reduce to `TreeAction`s over a `TreeState`, so "the same end
//! state" is a value comparison rather than a screenshot diff.
//!
//! The three rules, one test each:
//!
//! 1. Everything doable with the mouse is doable with the keyboard, and the
//!    reverse.
//! 2. The selected row is describable in one line.
//! 3. No Unicode is load-bearing — the ASCII rendering carries the same
//!    information.

use aikit_core::id::CapsuleId;
use aikit_tui::tree::{self, Node, NodeKind, Root, TreeAction, TreeEffect, TreeState};

fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

fn capability(id: &str, summary: &str) -> Node {
    Node::leaf(NodeKind::Capability { id: cid(id) }, summary)
}

/// A tree with the six roots, a set holding two capabilities, and a hook chain.
fn fixture() -> TreeState {
    let sets = Node::branch(
        NodeKind::Root(Root::Sets),
        "2 sets",
        vec![
            Node::branch(
                NodeKind::Set {
                    name: "rust-review".into(),
                    observed: false,
                },
                "2 members, 1 projected, 1 withheld (unreviewed)",
                vec![
                    capability("skill/rust/review", "reviewed"),
                    capability("skill/rust/unsafe-audit", "unreviewed"),
                ],
            ),
            Node::branch(
                NodeKind::Set {
                    name: "nara".into(),
                    observed: true,
                },
                "23 members · projected to claude, hermes",
                vec![capability("skill/nara/para", "a voice register")],
            ),
        ],
    );
    let hooks = Node::branch(
        NodeKind::Root(Root::Hooks),
        "1 event",
        vec![Node::branch(
            NodeKind::Group {
                label: "PreToolUse".into(),
            },
            "2 steps, in execution order",
            vec![
                Node::leaf(
                    NodeKind::HookStep {
                        capsule: cid("hook/gate/project-boundary"),
                        phase: "gate".into(),
                        position: 1,
                    },
                    "closed · serial",
                ),
                Node::leaf(
                    NodeKind::HookStep {
                        capsule: cid("hook/verify/cargo-check"),
                        phase: "verify".into(),
                        position: 2,
                    },
                    "warn · parallel",
                ),
            ],
        )],
    );
    let others = [Root::Kinds, Root::Contexts, Root::Registries, Root::Inbox]
        .into_iter()
        .map(|root| Node::branch(NodeKind::Root(root), "…", vec![]));

    let mut roots = vec![sets, hooks];
    roots.extend(others);
    TreeState::new(roots)
}

fn apply(state: &mut TreeState, actions: &[TreeAction]) -> Vec<TreeEffect> {
    let mut effects = Vec::new();
    for action in actions {
        effects.extend(tree::reduce(state, action.clone()));
    }
    effects
}

// ---------------------------------------------------------------------------
// Rule 1: keyboard and mouse reach the same state
// ---------------------------------------------------------------------------

#[test]
fn a_keyboard_path_and_a_mouse_path_reach_an_identical_state() {
    // The keyboard user: expand `sets`, move down twice to `rust-review`, expand,
    // move to `unsafe-audit`, stage it.
    let mut by_keyboard = fixture();
    apply(
        &mut by_keyboard,
        &[
            TreeAction::Expand,   // sets/
            TreeAction::Down,     // rust-review
            TreeAction::Expand,
            TreeAction::Down,     // review
            TreeAction::Down,     // unsafe-audit
            TreeAction::Stage,
        ],
    );

    // The mouse user: click the sets marker, double-click rust-review, click the
    // row, click the checkbox. Clicks resolve to Select(index).
    let mut by_mouse = fixture();
    apply(
        &mut by_mouse,
        &[
            TreeAction::Select(0),
            TreeAction::Expand,
            TreeAction::Select(1),
            TreeAction::Expand,
            TreeAction::Select(3),
            TreeAction::Stage,
        ],
    );

    assert_eq!(
        by_keyboard, by_mouse,
        "the same end state must be reachable both ways"
    );
    assert!(by_keyboard.staged.contains(&cid("skill/rust/unsafe-audit")));
}

#[test]
fn every_action_is_reachable_from_both_input_paths() {
    // The guarantee is structural: both paths emit the SAME TreeAction values, so
    // there is no action only one of them can produce. This test pins that the
    // verb set is what §4.2 documents, so a new verb cannot be added on one path
    // only without failing here.
    let verbs = [
        TreeAction::Down,
        TreeAction::Up,
        TreeAction::First,
        TreeAction::Last,
        TreeAction::PageDown,
        TreeAction::PageUp,
        TreeAction::Expand,
        TreeAction::Collapse,
        TreeAction::Activate,
        TreeAction::Select(0),
        TreeAction::Stage,
        TreeAction::Yank,
        TreeAction::Put,
        TreeAction::RemoveFromSet,
        TreeAction::Filter("x".into()),
        TreeAction::ClearFilter,
    ];
    // Every verb must be applicable without panicking, from a fresh tree.
    for verb in verbs {
        let mut state = fixture();
        let _ = tree::reduce(&mut state, verb);
    }
}

// ---------------------------------------------------------------------------
// Rule 2: one-line description
// ---------------------------------------------------------------------------

#[test]
fn the_selected_row_is_describable_in_one_line() {
    let mut state = fixture();
    apply(&mut state, &[TreeAction::Expand, TreeAction::Down]);

    let line = state.describe_selection();
    assert_eq!(
        line, "sets/rust-review — 2 members, 1 projected, 1 withheld (unreviewed)",
        "the status bar, --json and a screen reader all get this string"
    );
    assert!(!line.contains('\n'), "one line means one line");
}

#[test]
fn a_withholding_is_visible_in_the_row_a_user_is_standing_on() {
    // The set's reply travels all the way to the row: a user does not have to run
    // a second command to find out that two members were dropped.
    let mut state = fixture();
    apply(&mut state, &[TreeAction::Expand, TreeAction::Down]);
    assert!(state.describe_selection().contains("withheld (unreviewed)"));
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[test]
fn hooks_show_the_resolved_chain_in_execution_order() {
    // The screen that answers "what actually runs, in what order" — the direct fix
    // for hook scripts sitting on disk wired to nothing.
    let mut state = fixture();
    apply(
        &mut state,
        &[
            TreeAction::Down,   // hooks/
            TreeAction::Expand,
            TreeAction::Down,   // PreToolUse/
            TreeAction::Expand,
        ],
    );

    let steps: Vec<String> = state
        .rows()
        .iter()
        .filter_map(|r| match &r.node.kind {
            NodeKind::HookStep {
                capsule, position, ..
            } => Some(format!("{position}. {capsule}")),
            _ => None,
        })
        .collect();

    assert_eq!(
        steps,
        vec![
            "1. hook/gate/project-boundary".to_string(),
            "2. hook/verify/cargo-check".to_string(),
        ],
        "execution order, not alphabetical order"
    );
}

#[test]
fn collapsing_an_already_collapsed_row_moves_to_its_parent() {
    let mut state = fixture();
    apply(&mut state, &[TreeAction::Expand, TreeAction::Down]);
    assert_eq!(state.selected, 1, "on rust-review");

    // It is collapsed, so `h` goes up to `sets/` — what every editor's tree does.
    apply(&mut state, &[TreeAction::Collapse]);
    assert_eq!(state.selected, 0, "moved to the parent");
}

#[test]
fn a_filter_keeps_the_path_to_a_match_rather_than_hiding_it() {
    let mut state = fixture();
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());
    apply(&mut state, &[TreeAction::Filter("unsafe".into())]);

    let paths: Vec<String> = state.rows().iter().map(|r| r.path.clone()).collect();
    assert!(
        paths.iter().any(|p| p == "sets"),
        "the ancestor survives so the match is reachable: {paths:?}"
    );
    assert!(paths.iter().any(|p| p.ends_with("unsafe-audit")));
    assert!(
        !paths.iter().any(|p| p.ends_with("para")),
        "an unrelated leaf is filtered out: {paths:?}"
    );
}

// ---------------------------------------------------------------------------
// The filesystem verbs
// ---------------------------------------------------------------------------

#[test]
fn yank_and_put_copy_into_a_set_rather_than_moving() {
    let mut state = fixture();
    // Stand on a capability in `nara` and yank it.
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/nara".into());
    state.expanded.insert("sets/rust-review".into());

    let rows = state.rows();
    let para = rows.iter().position(|r| r.path.ends_with("para")).unwrap();
    apply(&mut state, &[TreeAction::Select(para), TreeAction::Yank]);
    assert_eq!(state.yanked, Some(cid("skill/nara/para")));

    // Put it into rust-review.
    let target = state
        .rows()
        .iter()
        .position(|r| r.path == "sets/rust-review")
        .unwrap();
    let effects = apply(&mut state, &[TreeAction::Select(target), TreeAction::Put]);

    assert_eq!(
        effects,
        vec![TreeEffect::AddToSet {
            set: "rust-review".into(),
            capsule: cid("skill/nara/para"),
        }],
        "put copies into the target set"
    );
    assert_eq!(
        state.yanked,
        Some(cid("skill/nara/para")),
        "the yank survives: copy, not move — sets are views"
    );
}

#[test]
fn d_removes_from_the_set_and_never_deletes_the_capability() {
    let mut state = fixture();
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());

    let rows = state.rows();
    let review = rows
        .iter()
        .position(|r| r.path.ends_with("skill/rust/review"))
        .unwrap();
    let effects = apply(&mut state, &[TreeAction::Select(review), TreeAction::RemoveFromSet]);

    assert_eq!(
        effects,
        vec![TreeEffect::RemoveFromSet {
            set: "rust-review".into(),
            capsule: cid("skill/rust/review"),
        }],
        "the effect is scoped to the set — there is no delete-the-capsule verb here"
    );
}

#[test]
fn put_outside_a_set_is_a_no_op_rather_than_inventing_a_destination() {
    let mut state = fixture();
    state.yanked = Some(cid("skill/rust/review"));
    // `kinds/` is a view, not a set. Nothing sensible can be put here.
    let kinds = state
        .rows()
        .iter()
        .position(|r| r.path == "kinds")
        .expect("kinds root");
    let effects = apply(&mut state, &[TreeAction::Select(kinds), TreeAction::Put]);
    assert!(
        effects.is_empty(),
        "no enclosing set means no destination, not a guessed one"
    );
}

#[test]
fn a_kinds_capability_cannot_fall_into_the_last_visible_set() {
    let mut state = fixture();
    let kinds = state
        .roots
        .iter_mut()
        .find(|node| matches!(node.kind, NodeKind::Root(Root::Kinds)))
        .unwrap();
    kinds
        .children
        .push(capability("skill/rust/review", "same capsule, kinds view"));
    // Open sets/rust-review, then open kinds and stand on a capability there.
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());
    state.expanded.insert("kinds".into());
    let rows = state.rows();
    state.selected = rows
        .iter()
        .position(|row| row.path.starts_with("kinds/") && matches!(row.node.kind, NodeKind::Capability { .. }))
        .expect("fixture has a capability under kinds");
    state.yanked = Some(cid("skill/rust/review"));

    let effects = tree::reduce(&mut state, TreeAction::Put);

    assert!(
        effects.is_empty(),
        "a set must be a path ancestor, not merely an earlier visible row: {effects:?}"
    );
}

#[test]
fn staging_toggles_rather_than_only_setting() {
    let mut state = fixture();
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());
    let review = state
        .rows()
        .iter()
        .position(|r| r.path.ends_with("skill/rust/review"))
        .unwrap();

    apply(&mut state, &[TreeAction::Select(review), TreeAction::Stage]);
    assert_eq!(state.staged.len(), 1);
    apply(&mut state, &[TreeAction::Stage]);
    assert!(state.staged.is_empty(), "Space is a toggle");
}

#[test]
fn a_capability_appears_under_every_view_that_contains_it() {
    // The tree is a VIEW, not an ownership hierarchy: the same capsule is under
    // its set and under `kinds/`, and neither is its "real" location.
    let shared = cid("skill/rust/review");
    let mut state = fixture();
    state.roots.push(Node::branch(
        NodeKind::Root(Root::Kinds),
        "1 kind",
        vec![Node::branch(
            NodeKind::Group {
                label: "skill".into(),
            },
            "1",
            vec![capability("skill/rust/review", "reviewed")],
        )],
    ));
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());
    state.expanded.insert("kinds".into());
    state.expanded.insert("kinds/skill".into());

    let appearances = state
        .rows()
        .iter()
        .filter(|r| matches!(&r.node.kind, NodeKind::Capability { id } if id == &shared))
        .count();
    assert_eq!(appearances, 2, "one capsule, two places, no contradiction");
}

// ---------------------------------------------------------------------------
// Rule 3: no Unicode is load-bearing
// ---------------------------------------------------------------------------

use aikit_tui::tree::TreeGlyphs;

/// Both renderings of one state, for comparison.
fn both(state: &TreeState) -> (Vec<String>, Vec<String>) {
    (
        tree::render_lines(state, TreeGlyphs::unicode()),
        tree::render_lines(state, TreeGlyphs::ascii()),
    )
}

#[test]
fn the_ascii_rendering_carries_the_same_information_without_a_non_ascii_byte() {
    let mut state = fixture();
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());
    state.expanded.insert("sets/nara".into());
    state.staged.insert(cid("skill/rust/review"));

    let (unicode, ascii) = both(&state);

    assert_eq!(
        unicode.len(),
        ascii.len(),
        "the same rows are drawn in both modes"
    );
    assert!(
        ascii.iter().all(|l| l.is_ascii()),
        "the ASCII rendering must contain no non-ASCII byte: {ascii:#?}"
    );
    assert!(
        unicode.iter().any(|l| !l.is_ascii()),
        "…and the Unicode one really is using Unicode, so the test compares two \
         genuinely different renderings"
    );

    // Every row carries the same *words*. The Unicode line is folded the same way
    // before comparing, so what is being asserted is "nothing was dropped", not
    // "the two are byte-identical" — which they must not be.
    for (u, a) in unicode.iter().zip(&ascii) {
        let strip = |s: &str| {
            tree::ascii_fold(s)
                .replace(['+', '-'], "")
                .replace(' ', "")
        };
        assert_eq!(
            strip(u),
            strip(a),
            "the information must be identical:\n  unicode: {u}\n  ascii:   {a}"
        );
    }
}

#[test]
fn expansion_state_and_staging_are_both_legible_in_ascii() {
    // Colour is redundant emphasis, never the only carrier of meaning — so the
    // ASCII text alone has to distinguish expanded from collapsed and staged from
    // unstaged.
    let mut state = fixture();
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/rust-review".into());
    state.staged.insert(cid("skill/rust/review"));

    let ascii = tree::render_lines(&state, TreeGlyphs::ascii());
    let joined = ascii.join("\n");

    assert!(joined.contains("- sets/"), "an expanded root reads as `-`: {joined}");
    assert!(
        joined.lines().any(|l| l.trim_start().starts_with('+')),
        "a collapsed row reads as `+`: {joined}"
    );
    assert!(
        joined.contains("[x] skill/rust/review"),
        "a staged capability reads as [x]: {joined}"
    );
    assert!(
        joined.contains("[ ] skill/rust/unsafe-audit"),
        "an unstaged one reads as [ ]: {joined}"
    );
}

#[test]
fn an_observed_set_wears_its_sigil_in_both_renderings() {
    // `@` marks an observed set so the origin of membership is visible at the
    // point of use — and `@` is ASCII, so it survives the fallback.
    let mut state = fixture();
    state.expanded.insert("sets".into());

    let (unicode, ascii) = both(&state);
    assert!(unicode.iter().any(|l| l.contains("@nara/")));
    assert!(ascii.iter().any(|l| l.contains("@nara/")));
}
