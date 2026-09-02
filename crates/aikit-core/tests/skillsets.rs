//! Skill-sets (SPEC-III §1): folders you point a harness at.
//!
//! The load-bearing test here is `a_set_cannot_launder_trust_onto_an_unreviewed_member`.
//! Everything else is ergonomics; that one is the reason a set is allowed to be as
//! light as it is.

mod common;
use common::*;

use aikit_core::scope::ScopeKind;
use aikit_core::skillset::{self, SetMembership, SetProvenance, SkillSet};
use aikit_core::trust::TrustState;

fn set_of(name: &str, members: &[&str]) -> SkillSet {
    let mut set = SkillSet::new(name, SetProvenance::Composed);
    for id in members {
        set = set.with_member(cid(id), SetMembership::Explicit);
    }
    set
}

#[test]
fn a_set_cannot_launder_trust_onto_an_unreviewed_member() {
    // The attack this refuses: bundle a reviewed skill with an unreviewed hook,
    // point a harness at the bundle, and let the bundle's reputation carry the
    // hook in. A set confers nothing — every member passes its own gate or is
    // withheld, and the set says so.
    let reviewed = skill("skill/rust/review");
    let sneaky = hook("hook/gate/sneaky");

    let fixture = Fixture::new(vec![reviewed, sneaky])
        .with_layers(vec![layer(
            ScopeKind::Project,
            &["skill/rust/review", "hook/gate/sneaky"],
            &[],
        )])
        .set_trust("hook/gate/sneaky", TrustState::Unseen);
    let view = fixture.resolve().expect("resolves");

    let bundle = set_of("bundle", &["skill/rust/review", "hook/gate/sneaky"]);
    let projection = skillset::project(&bundle, &view);

    assert_eq!(
        projection.projected,
        vec![cid("skill/rust/review")],
        "only the member that passes its own gate projects"
    );
    assert_eq!(projection.withheld.len(), 1);
    assert_eq!(projection.withheld[0].capsule, cid("hook/gate/sneaky"));
    assert!(
        !projection.is_complete(),
        "the set reports that it did not get everything it asked for"
    );
    assert!(
        projection.withheld[0].describe().contains("review"),
        "and says why: {}",
        projection.withheld[0].describe()
    );
}

#[test]
fn a_set_reports_what_it_withheld_in_one_describable_line() {
    // §4.3: the selected row must be describable in one line, for the status bar,
    // for `--json`, and for a screen reader — which are the same requirement.
    let a = skill("skill/rust/a");
    let b = skill("skill/rust/b");
    let c = skill("skill/rust/c");
    let view = Fixture::new(vec![a, b, c])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/a"], &[])])
        .set_trust("skill/rust/b", TrustState::Unseen)
        .resolve()
        .expect("resolves");

    let set = set_of(
        "rust-review",
        &["skill/rust/a", "skill/rust/b", "skill/rust/c"],
    );
    let projection = skillset::project(&set, &view);
    let line = projection.summarize("sets/rust-review");

    assert!(line.contains("3 members"), "{line}");
    assert!(line.contains("1 projected"), "{line}");
    assert!(line.contains("2 withheld"), "{line}");
}

#[test]
fn nesting_gives_sub_sets_and_the_parent_carries_the_whole_subtree() {
    // `nara/` is a set; `nara/paśyantī/` is a set. Point a harness at the first and
    // it gets everything; point it at the second and it gets the subtree.
    let child = set_of("pasyanti", &["skill/rust/a", "skill/rust/b"]);
    let parent = set_of("nara", &["skill/rust/c"]).with_child(child.clone());

    assert_eq!(
        parent.len(),
        3,
        "the parent carries its own plus the child's"
    );
    assert_eq!(child.len(), 2, "the child carries only its own");
    assert!(parent.all_members().contains(&cid("skill/rust/a")));
}

#[test]
fn sets_compose_by_union_and_a_shared_member_appears_once() {
    let left = set_of("left", &["skill/rust/a", "skill/rust/shared"]);
    let right = set_of("right", &["skill/rust/b", "skill/rust/shared"]);

    let union = skillset::union(&[&left, &right]);
    assert_eq!(
        union.len(),
        3,
        "union, with the shared member once: {union:?}"
    );
    assert!(union.contains(&cid("skill/rust/shared")));
}

#[test]
fn an_observed_set_is_read_only_and_wears_its_sigil() {
    // `@` marks an observed set so the origin of membership is visible at the point
    // of use rather than requiring a lookup.
    let observed = SkillSet::new(
        "nara",
        SetProvenance::Observed {
            path: "/home/me/.hermes/skills/nara".into(),
        },
    );
    assert_eq!(observed.label(), "@nara");
    assert!(
        !observed.provenance.is_writable(),
        "observed is read-only until adopted"
    );

    let composed = SkillSet::new("rust-review", SetProvenance::Composed);
    assert_eq!(composed.label(), "rust-review");
    assert!(composed.provenance.is_writable());
}

#[test]
fn a_new_capsule_matching_a_retained_pattern_is_proposed_never_joined() {
    // §1.5: globs expand at authoring time. A capsule catalogued later that matches
    // the retained pattern raises a candidate — it does not become a member,
    // because dynamic membership would mean syncing a registry silently changes
    // what a harness sees (rule 6).
    let mut set = set_of("rust-review", &["skill/rust/review"]);
    set.patterns = vec!["skill/rust/*".to_string()];

    let catalogued = vec![
        cid("skill/rust/review"),
        cid("skill/rust/unsafe-audit"),
        cid("skill/python/lint"),
    ];
    let candidates = skillset::candidates(&set, &catalogued);

    assert_eq!(
        candidates.len(),
        1,
        "only the new matching capsule: {candidates:?}"
    );
    assert_eq!(candidates[0].capsule, cid("skill/rust/unsafe-audit"));
    assert!(
        !set.all_members().contains(&cid("skill/rust/unsafe-audit")),
        "it is proposed, not joined"
    );
}

#[test]
fn a_single_star_stays_within_a_path_segment_and_a_double_star_crosses() {
    use skillset::glob_matches;
    assert!(glob_matches("skill/rust/*", "skill/rust/review"));
    assert!(
        !glob_matches("skill/rust/*", "skill/rust/deep/review"),
        "a single star must not cross a separator"
    );
    assert!(glob_matches("skill/**", "skill/rust/deep/review"));
    assert!(!glob_matches("skill/rust/*", "skill/python/lint"));
    assert!(glob_matches(
        "script/test/cargo-*",
        "script/test/cargo-nextest"
    ));
}

#[test]
fn a_set_asking_for_something_not_installed_says_so_rather_than_failing() {
    // A set is a request. Asking for something absent is answered, not fatal —
    // unlike a `requires` edge, which is a dependency and does fail.
    let present = skill("skill/rust/review");
    let view = Fixture::new(vec![present])
        .with_layers(vec![layer(ScopeKind::Project, &["skill/rust/review"], &[])])
        .resolve()
        .expect("resolves");

    let set = set_of(
        "wishful",
        &["skill/rust/review", "skill/rust/not-installed"],
    );
    let projection = skillset::project(&set, &view);

    assert_eq!(projection.projected, vec![cid("skill/rust/review")]);
    assert_eq!(projection.withheld.len(), 1);
    assert!(projection
        .summarize("sets/wishful")
        .contains("not installed"));
}
