//! Allow/deny rules: a family of capabilities named once, matched against the
//! catalogue every time resolution runs.
//!
//! The list this replaces was kept by hand: every skill authored after the list
//! was written reached no agent until someone appended its id.

mod common;

use aikit_core::profile::PoolPatch;
use aikit_core::resolve::UnavailableReason;
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::trust::TrustState;

use common::{active_ids, cid, layer, skill, Fixture};

fn rules(kind: ScopeKind, allow: &[&str], deny: &[&str]) -> ScopeLayer {
    ScopeLayer {
        kind,
        depth: 0,
        origin: LayerOrigin::new(format!("test:{}", kind.as_str())),
        patch: PoolPatch {
            allow: allow.iter().map(|s| s.to_string()).collect(),
            deny: deny.iter().map(|s| s.to_string()).collect(),
            ..PoolPatch::default()
        },
    }
}

fn spool() -> Vec<aikit_core::capsule::Capsule> {
    vec![
        skill("skill/personal/alpha"),
        skill("skill/personal/beta"),
        skill("skill/aikit/gamma"),
    ]
}

#[test]
fn an_allow_rule_activates_every_catalogued_match_and_nothing_else() {
    let fixture = Fixture::new(spool()).with_layers(vec![rules(
        ScopeKind::Global,
        &["skill/personal/*"],
        &[],
    )]);
    let view = fixture.resolve().unwrap();
    assert_eq!(
        active_ids(&view),
        vec!["skill/personal/alpha", "skill/personal/beta"]
    );
}

#[test]
fn a_skill_catalogued_after_the_rule_was_written_is_active_with_no_further_edit() {
    let layers = vec![rules(ScopeKind::Global, &["skill/personal/*"], &[])];
    let before = Fixture::new(spool()).with_layers(layers.clone());
    assert!(!active_ids(&before.resolve().unwrap()).contains(&"skill/personal/delta".to_string()));

    let mut grown = spool();
    grown.push(skill("skill/personal/delta"));
    let after = Fixture::new(grown).with_layers(layers);
    assert!(active_ids(&after.resolve().unwrap()).contains(&"skill/personal/delta".to_string()));
}

#[test]
fn deny_beats_allow_within_one_scope_and_names_the_rule_that_decided() {
    let fixture = Fixture::new(spool()).with_layers(vec![rules(
        ScopeKind::Global,
        &["skill/personal/*"],
        &["skill/personal/beta"],
    )]);
    let view = fixture.resolve().unwrap();
    assert_eq!(active_ids(&view), vec!["skill/personal/alpha"]);

    let declared = &view.declared[&cid("skill/personal/beta")];
    assert!(!declared.enabled);
    assert_eq!(
        declared.via_rule.as_deref(),
        Some("deny skill/personal/beta")
    );
    let allowed = &view.declared[&cid("skill/personal/alpha")];
    assert_eq!(allowed.via_rule.as_deref(), Some("allow skill/personal/*"));
}

#[test]
fn deny_beats_an_exact_enable_in_the_same_scope() {
    let mut scope = rules(ScopeKind::Global, &[], &["skill/personal/*"]);
    scope.patch.enable = vec![cid("skill/personal/alpha")];
    let view = Fixture::new(spool())
        .with_layers(vec![scope])
        .resolve()
        .unwrap();
    assert!(active_ids(&view).is_empty());
}

#[test]
fn a_nearer_scope_may_allow_what_a_farther_scope_denied() {
    let fixture = Fixture::new(spool()).with_layers(vec![
        rules(ScopeKind::Global, &["skill/**"], &["skill/aikit/*"]),
        rules(ScopeKind::Project, &["skill/aikit/gamma"], &[]),
    ]);
    let view = fixture.resolve().unwrap();
    assert!(active_ids(&view).contains(&"skill/aikit/gamma".to_string()));
}

#[test]
fn an_exact_disable_in_a_nearer_scope_removes_one_member_of_an_allowed_family() {
    let fixture = Fixture::new(spool()).with_layers(vec![
        rules(ScopeKind::Global, &["skill/personal/*"], &[]),
        layer(ScopeKind::Project, &[], &["skill/personal/alpha"]),
    ]);
    assert_eq!(
        active_ids(&fixture.resolve().unwrap()),
        vec!["skill/personal/beta"]
    );
}

#[test]
fn a_rule_is_not_trust() {
    // Being named by a rule is a request, not a review: an unreviewed skill is
    // still withheld, and the view says so.
    let fixture = Fixture::new(spool())
        .set_trust("skill/personal/beta", TrustState::Unseen)
        .with_layers(vec![rules(ScopeKind::Global, &["skill/personal/*"], &[])]);
    let view = fixture.resolve().unwrap();
    assert_eq!(active_ids(&view), vec!["skill/personal/alpha"]);
    assert_eq!(
        view.unavailable[&cid("skill/personal/beta")],
        UnavailableReason::TrustRequired
    );
}

#[test]
fn a_rule_that_matches_nothing_is_said_aloud() {
    let fixture = Fixture::new(spool()).with_layers(vec![rules(
        ScopeKind::Global,
        &["skill/persnal/*"],
        &[],
    )]);
    let diagnosis = fixture.diagnose();
    let warnings = diagnosis.view.unwrap().warnings.join("\n");
    assert!(
        warnings.contains("allow `skill/persnal/*` matches nothing"),
        "{warnings}"
    );
}

#[test]
fn a_malformed_or_contradictory_rule_is_refused_not_ignored() {
    for (allow, deny, expect) in [
        (vec![""], vec![], "empty"),
        (vec!["  "], vec![], "empty"),
        (vec!["Skill/Personal/*"], vec![], "not a capability pattern"),
        (vec!["skill/a b"], vec![], "not a capability pattern"),
        (
            vec!["skill/x/*"],
            vec!["skill/x/*"],
            "both allowed and denied",
        ),
    ] {
        let fixture =
            Fixture::new(spool()).with_layers(vec![rules(ScopeKind::Global, &allow, &deny)]);
        let error = fixture.resolve().unwrap_err();
        assert!(
            error.message().contains(expect),
            "{allow:?}/{deny:?}: {}",
            error.message()
        );
    }
}

#[test]
fn the_app_stages_a_rule_previews_it_through_the_resolver_and_forgets_it() {
    use aikit_core::composition_mutation::{
        inspect_profile_composition, RuleIntent, StagedProfileComposition,
    };

    let before = PoolPatch::default();
    let mut staged = StagedProfileComposition::default();
    staged
        .stage_rule("skill/personal/*", RuleIntent::Allow)
        .unwrap();
    assert!(staged
        .stage_rule("Skill Personal", RuleIntent::Deny)
        .is_err());
    assert_eq!(staged.len(), 1);

    let after = staged.authored_after(&before);
    assert!(
        before.allow.is_empty(),
        "the caller's patch is never changed"
    );
    assert_eq!(after.allow, vec!["skill/personal/*".to_string()]);

    let fixture = Fixture::new(spool()).with_layers(vec![ScopeLayer {
        kind: ScopeKind::Global,
        depth: 0,
        origin: LayerOrigin::new("test:global"),
        patch: after.clone(),
    }]);
    let view = fixture.resolve().unwrap();
    let read = inspect_profile_composition(ScopeKind::Global, &after, &view, &[]);
    assert_eq!(read.authored.allow, vec!["skill/personal/*".to_string()]);
    assert_eq!(read.effective.active.len(), 2);

    let mut forget = StagedProfileComposition::default();
    forget
        .stage_rule("skill/personal/*", RuleIntent::Forget)
        .unwrap();
    assert!(forget.authored_after(&after).allow.is_empty());

    let mut flip = StagedProfileComposition::default();
    flip.stage_rule("skill/personal/*", RuleIntent::Deny)
        .unwrap();
    let flipped = flip.authored_after(&after);
    assert!(flipped.allow.is_empty());
    assert_eq!(flipped.deny, vec!["skill/personal/*".to_string()]);
}
