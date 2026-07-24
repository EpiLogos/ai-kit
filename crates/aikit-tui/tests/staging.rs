//! Staging: what a change would cost, computed before anything is written.
//!
//! Space stages. It does not apply. The distinction is the reason the palette can
//! be fast and safe at the same time: a user flips four things, sees that two
//! dependencies come along and that Codex will need a restart, and *then* decides.
//! Two properties are load-bearing and both are tested against a real overlay file
//! in a real temporary directory:
//!
//! 1. Staging writes nothing. The file's bytes are compared before and after.
//! 2. A staged set that would not resolve is reported *before* apply, with the
//!    resolver's own error code and details — not paraphrased, not deferred to a
//!    failure after the fact.

mod common;

use common::*;

use aikit_core::id::CapsuleId;
use aikit_core::platform::TargetId;
use aikit_core::projection::ActivationEffect;
use aikit_core::scope::ScopeKind;
use aikit_tui::backend::{ClientEffect, PaletteBackend, Toggle};
use aikit_tui::staging::{is_on, stage, ProblemKind, StagedSet};

fn ids(list: &[CapsuleId]) -> Vec<String> {
    list.iter().map(|id| id.to_string()).collect()
}

// ---------------------------------------------------------------------------
// The staged set itself
// ---------------------------------------------------------------------------

#[test]
fn a_row_reads_as_on_when_it_is_active_or_declared_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
            hook("hook/gate/boundary"),
            script("script/ops/idle"),
        ],
    )
    .enable(ScopeKind::Project, &["script/app/uses-lib", "hook/gate/boundary"])
    .set_trust("hook/gate/boundary", aikit_core::trust::TrustState::Unseen);

    // Active because something requires it.
    assert!(is_on(fixture.view(), &cid("script/lib/core")));
    // Declared on, held back by trust: still a switch that is up.
    assert!(is_on(fixture.view(), &cid("hook/gate/boundary")));
    assert!(!fixture.view().is_active(&cid("hook/gate/boundary")));
    // Neither.
    assert!(!is_on(fixture.view(), &cid("script/ops/idle")));
}

#[test]
fn space_stages_the_opposite_of_what_a_scope_currently_declares() {
    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/ops/deploy"), false);
    assert_eq!(staged.state_of(&cid("script/ops/deploy")), Some(true));

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/ops/deploy"), true);
    assert_eq!(staged.state_of(&cid("script/ops/deploy")), Some(false));
}

#[test]
fn pressing_space_twice_leaves_nothing_staged_rather_than_staging_a_no_op() {
    let mut staged = StagedSet::default();
    let id = cid("script/ops/deploy");
    staged.toggle(&id, false);
    staged.toggle(&id, false);
    assert!(staged.is_empty(), "the second press must undo the first");
    assert_eq!(staged.state_of(&id), None);
}

#[test]
fn staged_toggles_are_handed_over_in_a_deterministic_order() {
    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/z/last"), false);
    staged.toggle(&cid("script/a/first"), false);
    assert_eq!(
        staged
            .toggles()
            .iter()
            .map(|t| t.capsule.to_string())
            .collect::<Vec<_>>(),
        vec!["script/a/first", "script/z/last"]
    );
}

// ---------------------------------------------------------------------------
// Staging writes nothing
// ---------------------------------------------------------------------------

#[test]
fn staging_a_change_does_not_touch_the_profile_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
        ],
    );

    let before = fixture.overlay_bytes();
    let hash_before = fixture.view().hash.clone();

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/app/uses-lib"), false);
    let outcome = stage(&fixture, ScopeKind::Session, &staged);
    assert!(outcome.is_ok(), "the fixture set must resolve");

    assert_eq!(
        fixture.overlay_bytes(),
        before,
        "staging rewrote {}",
        fixture.overlay_path().display()
    );
    assert_eq!(
        fixture.view().hash,
        hash_before,
        "staging changed the live effective view"
    );
    assert!(fixture.applied.is_empty(), "staging applied something");
}

#[test]
fn staging_the_same_set_repeatedly_still_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path(), vec![script("script/ops/deploy")]);
    let before = fixture.overlay_bytes();

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/ops/deploy"), false);
    for _ in 0..5 {
        assert!(stage(&fixture, ScopeKind::Session, &staged).is_ok());
    }
    assert_eq!(fixture.overlay_bytes(), before);
}

// ---------------------------------------------------------------------------
// Consequences
// ---------------------------------------------------------------------------

#[test]
fn staging_names_the_dependencies_a_toggle_would_pull_in() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
        ],
    );

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/app/uses-lib"), false);
    let diff = stage(&fixture, ScopeKind::Session, &staged).expect("this resolves");

    assert_eq!(ids(&diff.added_dependencies), vec!["script/lib/core"]);
    assert_eq!(
        diff.requested
            .iter()
            .map(|t| t.capsule.to_string())
            .collect::<Vec<_>>(),
        vec!["script/app/uses-lib"],
        "a dependency is not a request"
    );
}

#[test]
fn switching_something_off_names_the_dependencies_that_go_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
        ],
    )
    .enable(ScopeKind::Project, &["script/app/uses-lib"]);

    assert!(fixture.view().is_active(&cid("script/lib/core")));

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/app/uses-lib"), true);
    let diff = stage(&fixture, ScopeKind::Session, &staged).expect("this resolves");

    assert_eq!(ids(&diff.dropped_dependencies), vec!["script/lib/core"]);
    assert!(diff.added_dependencies.is_empty());
}

#[test]
fn client_effects_are_the_adapters_answer_and_not_the_palettes_guess() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path(), vec![script("script/ops/deploy")]).with_effects(vec![
        ClientEffect::new(TargetId::claude_code(), ActivationEffect::live()),
        ClientEffect::new(
            TargetId::codex(),
            ActivationEffect::brokered("this task uses the session's shared working tree"),
        ),
    ]);

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/ops/deploy"), false);
    let diff = stage(&fixture, ScopeKind::Session, &staged).unwrap();

    let described: Vec<String> = diff.client_effects.iter().map(|e| e.describe()).collect();
    assert_eq!(described[0], "Claude: live");
    assert!(
        described[1].starts_with("Codex: brokered — "),
        "a fallback must carry its reason: {}",
        described[1]
    );
}

#[test]
fn the_footer_reads_exactly_as_the_specification_says() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/one", &["script/lib/alpha"]),
            requiring("script", "script/app/two", &["script/lib/beta"]),
            script("script/app/three"),
            script("script/lib/alpha"),
            script("script/lib/beta"),
        ],
    )
    .with_effects(vec![
        ClientEffect::new(TargetId::claude_code(), ActivationEffect::live()),
        ClientEffect::new(TargetId::codex(), ActivationEffect::restart_client("codex")),
    ]);

    let mut staged = StagedSet::default();
    for id in ["script/app/one", "script/app/two", "script/app/three"] {
        staged.toggle(&cid(id), false);
    }
    let diff = stage(&fixture, ScopeKind::Session, &staged).unwrap();

    assert_eq!(
        diff.footer(),
        "3 staged changes · +2 dependencies · 1 client restart"
    );
}

#[test]
fn a_single_change_with_no_consequences_says_so_in_the_singular() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path(), vec![script("script/ops/deploy")]).with_effects(vec![
        ClientEffect::new(TargetId::claude_code(), ActivationEffect::live()),
    ]);

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/ops/deploy"), false);
    let diff = stage(&fixture, ScopeKind::Session, &staged).unwrap();
    assert_eq!(diff.footer(), "1 staged change");
}

// ---------------------------------------------------------------------------
// A set that would not resolve is refused before apply
// ---------------------------------------------------------------------------

#[test]
fn disabling_something_another_capability_requires_is_reported_before_apply() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
        ],
    )
    .enable(ScopeKind::Project, &["script/app/uses-lib"]);

    let before = fixture.overlay_bytes();

    let mut staged = StagedSet::default();
    // The row reads as on — it is live as somebody's dependency — so Space means
    // "switch this off".
    assert!(is_on(fixture.view(), &cid("script/lib/core")));
    staged.toggle(&cid("script/lib/core"), is_on(fixture.view(), &cid("script/lib/core")));
    let problem = stage(&fixture, ScopeKind::Session, &staged)
        .expect_err("the resolver refuses to silently re-enable a disabled requirement");

    assert_eq!(problem.code(), "resolution.required_capability_disabled");
    assert_eq!(
        problem.kind,
        ProblemKind::BreaksDependent {
            capability: cid("script/lib/core"),
            dependent: cid("script/app/uses-lib"),
        }
    );
    assert!(
        problem.headline().contains("script/app/uses-lib"),
        "the message must name what breaks: {}",
        problem.headline()
    );
    assert_eq!(
        fixture.overlay_bytes(),
        before,
        "a refused stage must still not have written anything"
    );
}

#[test]
fn two_capabilities_that_conflict_are_offered_as_a_choice() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(
        dir.path(),
        vec![
            conflicting("script/fmt/alpha", "script/fmt/beta"),
            script("script/fmt/beta"),
        ],
    );

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/fmt/alpha"), false);
    staged.toggle(&cid("script/fmt/beta"), false);
    let problem = stage(&fixture, ScopeKind::Session, &staged).expect_err("these conflict");

    assert_eq!(problem.code(), "resolution.conflict");
    assert_eq!(
        problem.kind,
        ProblemKind::Conflict {
            left: cid("script/fmt/alpha"),
            right: cid("script/fmt/beta"),
        }
    );
    assert_eq!(
        problem.choices(),
        vec![
            "keep script/fmt/alpha".to_string(),
            "keep script/fmt/beta".to_string(),
        ],
        "a conflict is a choice, not a dead end"
    );
}

#[test]
fn an_unclassified_resolver_refusal_still_reaches_the_user_with_its_code() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path(), vec![script("script/ops/deploy")]);

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/ghost/missing"), false);
    let problem = stage(&fixture, ScopeKind::Session, &staged).expect_err("there is no such capsule");

    assert_eq!(problem.code(), "resolution.unknown_capability");
    assert!(!problem.headline().is_empty());
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

#[test]
fn applying_a_staged_set_commits_the_whole_graph_in_one_act() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
            script("script/ops/deploy"),
        ],
    );
    let before = fixture.overlay_bytes();

    let mut staged = StagedSet::default();
    staged.toggle(&cid("script/app/uses-lib"), false);
    staged.toggle(&cid("script/ops/deploy"), false);

    let generation = fixture
        .apply(ScopeKind::Session, &staged.toggles())
        .expect("the set resolves, so it applies");

    assert!(generation.as_str().starts_with("gen_"));
    assert_ne!(fixture.overlay_bytes(), before, "apply must write");
    assert_eq!(fixture.applied.len(), 1, "one apply, not one per toggle");
    assert_eq!(
        fixture.applied[0].1,
        vec![
            Toggle::new(cid("script/app/uses-lib"), true),
            Toggle::new(cid("script/ops/deploy"), true),
        ]
    );
    assert!(fixture.view().is_active(&cid("script/lib/core")));
}

#[test]
fn an_apply_that_would_not_resolve_leaves_the_previous_view_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(
        dir.path(),
        vec![
            requiring("script", "script/app/uses-lib", &["script/lib/core"]),
            script("script/lib/core"),
        ],
    )
    .enable(ScopeKind::Project, &["script/app/uses-lib"]);
    let before = fixture.overlay_bytes();
    let hash_before = fixture.view().hash.clone();

    let error = fixture
        .apply(
            ScopeKind::Session,
            &[Toggle::new(cid("script/lib/core"), false)],
        )
        .expect_err("this cannot resolve");

    assert_eq!(error.code(), "resolution.required_capability_disabled");
    assert_eq!(fixture.overlay_bytes(), before);
    assert_eq!(fixture.view().hash, hash_before);
}
