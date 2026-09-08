mod common;

use aikit_core::composition_mutation::{
    ensure_profile_composition_preview_current, inspect_profile_composition,
    preview_profile_composition_change, ProfileActivationIntent, SkillSetMemberRelationState,
    StagedProfileComposition, StagedSkillSetRelations,
};
use aikit_core::profile::PoolPatch;
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::skillset::{SetMembership, SetProvenance, SkillSet};

use common::{cid, skill, Fixture};

fn project_layer(patch: PoolPatch) -> ScopeLayer {
    ScopeLayer {
        kind: ScopeKind::Project,
        depth: 0,
        origin: LayerOrigin::new("test:project-composition"),
        patch,
    }
}

fn authored(enabled: &[&str]) -> PoolPatch {
    PoolPatch {
        enable: enabled.iter().map(|id| cid(id)).collect(),
        ..PoolPatch::default()
    }
}

fn set() -> SkillSet {
    SkillSet::new("operator", SetProvenance::Project)
        .with_member(cid("skill/a"), SetMembership::Explicit)
        .with_member(cid("skill/b"), SetMembership::Explicit)
}

#[test]
fn authored_profile_and_effective_profile_remain_distinct_and_skillset_projection_uses_effective_truth(
) {
    let patch = authored(&["skill/a"]);
    let fixture = Fixture::new(vec![skill("skill/a"), skill("skill/b")])
        .with_layers(vec![project_layer(patch.clone())]);
    let view = fixture.resolve().unwrap();

    let read = inspect_profile_composition(ScopeKind::Project, &patch, &view, &[set()]);

    assert_eq!(read.authored.enabled, vec![cid("skill/a")]);
    assert_eq!(read.effective.active, vec![cid("skill/a")]);
    assert_eq!(read.skill_sets.len(), 1);
    assert!(matches!(
        read.skill_sets[0].members[0].state,
        SkillSetMemberRelationState::Effective
    ));
    assert!(matches!(
        read.skill_sets[0].members[1].state,
        SkillSetMemberRelationState::Withheld { .. }
    ));
}

#[test]
fn staging_profile_activation_is_typed_and_does_not_write_authored_state() {
    let before = authored(&["skill/a"]);
    let snapshot = before.clone();
    let mut staged = StagedProfileComposition::default();
    staged.stage(cid("skill/b"), ProfileActivationIntent::Enable);

    let after = staged.authored_after(&before);

    assert_eq!(before, snapshot);
    assert_eq!(after.enable, vec![cid("skill/a"), cid("skill/b")]);
    assert!(after.disable.is_empty());
}

#[test]
fn preview_is_resolver_backed_reports_changed_ground_and_rejects_a_stale_basis() {
    let before_patch = authored(&["skill/a"]);
    let before_fixture = Fixture::new(vec![skill("skill/a"), skill("skill/b")])
        .with_layers(vec![project_layer(before_patch.clone())]);
    let before = before_fixture.resolve().unwrap();

    let mut staged_profile = StagedProfileComposition::default();
    staged_profile.stage(cid("skill/b"), ProfileActivationIntent::Enable);
    let after_patch = staged_profile.authored_after(&before_patch);
    let after_fixture = Fixture::new(vec![skill("skill/a"), skill("skill/b")])
        .with_layers(vec![project_layer(after_patch)]);
    let after = after_fixture.resolve().unwrap();

    let preview = preview_profile_composition_change(
        ScopeKind::Project,
        &before_patch,
        &before,
        &[set()],
        staged_profile,
        StagedSkillSetRelations::default(),
        &after,
    )
    .unwrap();

    assert_eq!(preview.before.authored.enabled, vec![cid("skill/a")]);
    assert_eq!(
        preview.after.authored.enabled,
        vec![cid("skill/a"), cid("skill/b")]
    );
    assert_eq!(
        preview.changed_ground.capabilities_added,
        vec![cid("skill/b")]
    );
    assert!(preview.changed_ground.capabilities_removed.is_empty());
    assert!(ensure_profile_composition_preview_current(&preview, &before).is_ok());

    let error = ensure_profile_composition_preview_current(&preview, &after).unwrap_err();
    assert_eq!(error.code(), "composition.preview_stale");
}

#[test]
fn skillset_relation_stage_add_remove_is_write_free_and_observed_sets_remain_read_only() {
    let project_set = SkillSet::new("operator", SetProvenance::Project)
        .with_member(cid("skill/a"), SetMembership::Explicit);
    let snapshot = project_set.clone();
    let mut staged = StagedSkillSetRelations::default();
    staged.add("operator", cid("skill/b"));

    let after = staged
        .authored_after(std::slice::from_ref(&project_set))
        .unwrap();
    assert_eq!(project_set, snapshot);
    assert!(after[0].members.contains_key(&cid("skill/a")));
    assert!(after[0].members.contains_key(&cid("skill/b")));

    staged.remove("operator", cid("skill/a"));
    let after = staged
        .authored_after(std::slice::from_ref(&project_set))
        .unwrap();
    assert!(!after[0].members.contains_key(&cid("skill/a")));
    assert!(after[0].members.contains_key(&cid("skill/b")));

    let observed = SkillSet::new(
        "observed",
        SetProvenance::Observed {
            path: "/tmp/observed".into(),
        },
    );
    let mut staged_observed = StagedSkillSetRelations::default();
    staged_observed.add("observed", cid("skill/a"));
    let error = staged_observed.authored_after(&[observed]).unwrap_err();
    assert_eq!(error.code(), "composition.skillset_read_only");
}

#[test]
fn nested_skillsets_expose_direct_containment_and_transitive_projection() {
    let verification = SkillSet::new("verification", SetProvenance::Project)
        .with_member(cid("skill/verify"), SetMembership::Explicit);
    let application = SkillSet::new("application", SetProvenance::Project)
        .with_member(cid("skill/apply"), SetMembership::Explicit)
        .with_child(verification);
    let methodology = SkillSet::new("methodology", SetProvenance::Project)
        .with_member(cid("skill/orient"), SetMembership::Explicit)
        .with_child(application);
    let patch = authored(&["skill/orient", "skill/apply"]);
    let fixture = Fixture::new(vec![
        skill("skill/orient"),
        skill("skill/apply"),
        skill("skill/verify"),
    ])
    .with_layers(vec![project_layer(patch.clone())]);
    let view = fixture.resolve().unwrap();

    let read = inspect_profile_composition(
        ScopeKind::Project,
        &patch,
        &view,
        std::slice::from_ref(&methodology),
    );

    assert_eq!(read.skill_sets.len(), 3);
    let root = &read.skill_sets[0];
    assert_eq!(root.identity, "methodology");
    assert_eq!(root.parent, None);
    assert_eq!(root.children, vec!["methodology/application"]);
    assert_eq!(root.members.len(), 3);
    assert!(root.members.iter().any(|member| {
        member.capability == cid("skill/verify")
            && !member.direct
            && member.declared_by == "methodology/application/verification"
            && matches!(member.state, SkillSetMemberRelationState::Withheld { .. })
    }));

    let application = &read.skill_sets[1];
    assert_eq!(application.identity, "methodology/application");
    assert_eq!(application.parent.as_deref(), Some("methodology"));
    assert_eq!(
        application.children,
        vec!["methodology/application/verification"]
    );
    assert!(application
        .members
        .iter()
        .any(|member| { member.capability == cid("skill/apply") && member.direct }));

    let verification = &read.skill_sets[2];
    assert_eq!(
        verification.parent.as_deref(),
        Some("methodology/application")
    );
    assert!(verification.members[0].direct);
}

#[test]
fn nested_skillsets_preserve_every_declaring_set_for_a_shared_skill() {
    let research = SkillSet::new("research", SetProvenance::Project)
        .with_member(cid("skill/verify"), SetMembership::Explicit);
    let release = SkillSet::new("release", SetProvenance::Project)
        .with_member(cid("skill/verify"), SetMembership::Explicit);
    let methodology = SkillSet::new("methodology", SetProvenance::Project)
        .with_child(research)
        .with_child(release);
    let patch = authored(&["skill/verify"]);
    let fixture =
        Fixture::new(vec![skill("skill/verify")]).with_layers(vec![project_layer(patch.clone())]);
    let view = fixture.resolve().unwrap();

    let read = inspect_profile_composition(
        ScopeKind::Project,
        &patch,
        &view,
        std::slice::from_ref(&methodology),
    );

    let root = &read.skill_sets[0];
    let shared_relations: Vec<_> = root
        .members
        .iter()
        .filter(|member| member.capability == cid("skill/verify"))
        .collect();
    assert_eq!(shared_relations.len(), 2);
    assert_eq!(
        shared_relations
            .iter()
            .map(|member| member.declared_by.as_str())
            .collect::<Vec<_>>(),
        vec!["methodology/research", "methodology/release"]
    );
    assert!(shared_relations.iter().all(|member| !member.direct));
    assert!(shared_relations
        .iter()
        .all(|member| matches!(member.state, SkillSetMemberRelationState::Effective)));

    for child in &read.skill_sets[1..] {
        assert_eq!(child.members.len(), 1);
        assert!(child.members[0].direct);
        assert_eq!(child.members[0].declared_by, child.identity);
    }
}
