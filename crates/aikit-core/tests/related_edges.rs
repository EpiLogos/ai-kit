//! `related` as a real walkable edge (composables spec §6, PRIOR-ART-ACTIONS L5).
//!
//! Three decisions the spec left open (§9.3), settled here and pinned by tests:
//!
//! * **Hand-declared, not usage-weighted.** Co-usage would make the graph drift
//!   under you and would quietly re-import the "usage promotes" failure the whole
//!   design refuses. A relationship is an authored claim.
//! * **Directed in the manifest, surfaced symmetrically.** An author should not
//!   have to edit two capsules to state one relationship, and a user asking "what
//!   goes with this" wants the answer whichever end they are standing at. So the
//!   view carries a reverse index.
//! * **Never a dependency.** A related edge to something absent, unreviewed or
//!   unavailable is inert: it changes nothing about resolution.

mod common;
use common::*;

use aikit_core::scope::ScopeKind;

#[test]
fn a_related_edge_is_walkable_from_both_ends() {
    // `review` declares the relationship; `unsafe-audit` says nothing. Both ends
    // can still answer "what is often used with this?".
    let review = skill_with(
        "skill/rust/review",
        "related_skills = [\"skill/rust/unsafe-audit\"]",
    );
    let audit = skill("skill/rust/unsafe-audit");

    let view = Fixture::new(vec![review, audit])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/review"], &[])])
        .resolve()
        .expect("resolves");

    assert_eq!(
        view.related_to(&cid("skill/rust/review")),
        vec![cid("skill/rust/unsafe-audit")],
        "the declaring end walks forward"
    );
    assert_eq!(
        view.related_to(&cid("skill/rust/unsafe-audit")),
        vec![cid("skill/rust/review")],
        "the other end walks back without having declared anything"
    );
}

#[test]
fn a_related_edge_never_selects_or_activates_anything() {
    // Rule 6: a relationship is a suggestion for a human or an agent to consider,
    // never a reason for something to become active.
    let review = skill_with(
        "skill/rust/review",
        "related_skills = [\"skill/rust/unsafe-audit\"]",
    );
    let audit = skill("skill/rust/unsafe-audit");

    let view = Fixture::new(vec![review, audit])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/review"], &[])])
        .resolve()
        .expect("resolves");

    assert!(view.is_active(&cid("skill/rust/review")));
    assert!(
        !view.is_active(&cid("skill/rust/unsafe-audit")),
        "a related capability is not pulled in the way a dependency would be"
    );
}

#[test]
fn a_related_edge_to_something_absent_is_inert_rather_than_fatal() {
    // A dangling `requires` fails resolution. A dangling `related` must not: it is
    // advisory, and a registry that has not been synced yet is not an error.
    let review = skill_with(
        "skill/rust/review",
        "related_skills = [\"skill/rust/not-installed\"]",
    );

    let view = Fixture::new(vec![review])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/review"], &[])])
        .resolve()
        .expect("a dangling related edge must not fail resolution");

    assert!(view.is_active(&cid("skill/rust/review")));
    assert!(
        view.related_to(&cid("skill/rust/review")).is_empty(),
        "an edge to something not in the catalogue is dropped from the walk"
    );
}

#[test]
fn related_edges_are_deduplicated_and_ordered_for_a_stable_ui() {
    // Two capsules pointing at the same third, plus a mutual declaration, must not
    // produce duplicates or a jittering order.
    let a = skill_with(
        "skill/rust/a",
        "related_skills = [\"skill/rust/c\", \"skill/rust/b\"]",
    );
    let b = skill_with("skill/rust/b", "related_skills = [\"skill/rust/a\"]");
    let c = skill("skill/rust/c");

    let view = Fixture::new(vec![a, b, c])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/a"], &[])])
        .resolve()
        .expect("resolves");

    let related = view.related_to(&cid("skill/rust/a"));
    assert_eq!(
        related,
        vec![cid("skill/rust/b"), cid("skill/rust/c")],
        "sorted, and `b` appears once despite being declared from both ends"
    );
}
