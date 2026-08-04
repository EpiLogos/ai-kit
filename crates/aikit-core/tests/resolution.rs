//! The resolver is the product. These tests encode the seven override rules from
//! the specification, plus the availability semantics that let the palette show
//! *declared* and *effective* state as different things.

mod common;

use common::*;

use aikit_core::capsule::Kind;
use aikit_core::context::Isolation;
use aikit_core::effects::EffectClass;
use aikit_core::id::Revision;
use aikit_core::platform::{Platform, TargetId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::SkillUsageOverlayPatch;
use aikit_core::resolve::{SelectionOrigin, UnavailableReason};
use aikit_core::scope::ScopeKind;
use aikit_core::trust::TrustState;

// ---------------------------------------------------------------------------
// Skill Usage Overlays — additive orientation with scoped provenance
// ---------------------------------------------------------------------------

#[test]
fn skill_usage_overlays_accumulate_in_scope_order_and_change_the_view_hash() {
    let id = cid("skill/test/wayfinder");
    let mut global = layer(ScopeKind::Global, &["skill/test/wayfinder"], &[]);
    global.patch.skill_overlays.insert(
        id.clone(),
        SkillUsageOverlayPatch {
            inherit: true,
            description: Some("Prefer for work crossing agent sessions.".into()),
            guidance: Some("Use the user's issue tracker as the shared map.".into()),
            reviewed_against: None,
        },
    );
    let mut project = layer(ScopeKind::Project, &[], &[]);
    project.patch.skill_overlays.insert(
        id.clone(),
        SkillUsageOverlayPatch {
            inherit: true,
            description: None,
            guidance: Some("This project's maps may carry execution when Notes say so.".into()),
            reviewed_against: None,
        },
    );

    let base = Fixture::new(vec![skill("skill/test/wayfinder")])
        .with_layers(vec![layer(
            ScopeKind::Global,
            &["skill/test/wayfinder"],
            &[],
        )])
        .resolve()
        .unwrap();
    let augmented = Fixture::new(vec![skill("skill/test/wayfinder")])
        .with_layers(vec![project, global])
        .resolve()
        .unwrap();

    let overlays = augmented.skill_usage_overlays.get(&id).unwrap();
    assert_eq!(overlays.len(), 2);
    assert_eq!(overlays[0].scope, ScopeKind::Global);
    assert_eq!(overlays[1].scope, ScopeKind::Project);
    assert_eq!(overlays[0].origin.to_string(), "test:global");
    assert_ne!(augmented.hash, base.hash);
    assert_eq!(augmented.active[&id].revision, base.active[&id].revision);
    assert_eq!(augmented.active[&id].trust, base.active[&id].trust);
}

#[test]
fn a_more_specific_overlay_can_reset_inherited_orientation_without_forking_the_skill() {
    let id = cid("skill/test/wayfinder");
    let mut global = layer(ScopeKind::Global, &["skill/test/wayfinder"], &[]);
    global.patch.skill_overlays.insert(
        id.clone(),
        SkillUsageOverlayPatch {
            inherit: true,
            description: None,
            guidance: Some("Personal orientation.".into()),
            reviewed_against: None,
        },
    );
    let mut project = layer(ScopeKind::Project, &[], &[]);
    project.patch.skill_overlays.insert(
        id.clone(),
        SkillUsageOverlayPatch {
            inherit: false,
            description: Some("Use only for this project's long-running maps.".into()),
            guidance: Some("Shared project orientation.".into()),
            reviewed_against: None,
        },
    );

    let view = Fixture::new(vec![skill("skill/test/wayfinder")])
        .with_layers(vec![global, project])
        .resolve()
        .unwrap();
    let overlays = &view.skill_usage_overlays[&id];
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0].scope, ScopeKind::Project);
    assert_eq!(overlays[0].guidance.as_deref(), Some("Shared project orientation."));
}

#[test]
fn an_overlay_reviewed_against_an_older_revision_is_retained_with_a_warning() {
    let id = cid("skill/test/wayfinder");
    let reviewed = Revision::from_hash(blake3::hash(b"an older upstream skill"));
    let mut global = layer(ScopeKind::Global, &["skill/test/wayfinder"], &[]);
    global.patch.skill_overlays.insert(
        id.clone(),
        SkillUsageOverlayPatch {
            inherit: true,
            description: None,
            guidance: Some("User orientation that now needs re-review.".into()),
            reviewed_against: Some(reviewed.clone()),
        },
    );

    let view = Fixture::new(vec![skill("skill/test/wayfinder")])
        .with_layers(vec![global])
        .resolve()
        .unwrap();

    assert_eq!(view.skill_usage_overlays[&id][0].reviewed_against, Some(reviewed));
    assert!(view.warnings.iter().any(|warning| {
        warning.contains("review the augmentation against the updated source")
    }));
}

#[test]
fn skill_usage_overlays_reject_non_skill_capabilities() {
    let id = cid("script/test/wayfinder");
    let mut global = layer(ScopeKind::Global, &["script/test/wayfinder"], &[]);
    global.patch.skill_overlays.insert(
        id,
        SkillUsageOverlayPatch {
            inherit: true,
            description: None,
            guidance: Some("This must never be applied to a script.".into()),
            reviewed_against: None,
        },
    );

    let diagnosis = Fixture::new(vec![script("script/test/wayfinder")])
        .with_layers(vec![global])
        .diagnose();
    assert!(diagnosis
        .problems
        .iter()
        .any(|problem| problem.code() == "skill_overlay.not_a_skill"));
    assert!(diagnosis.view.is_none());
}

// ---------------------------------------------------------------------------
// Rule 1 — later layers may undo earlier ordinary enable/disable operations
// ---------------------------------------------------------------------------

#[test]
fn a_session_overlay_can_disable_what_the_project_enabled() {
    let f = Fixture::new(vec![script("script/test/cargo-nextest")]).with_layers(vec![
        layer(ScopeKind::Project, &["script/test/cargo-nextest"], &[]),
        layer(ScopeKind::Session, &[], &["script/test/cargo-nextest"]),
    ]);

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    assert!(view.is_declared_disabled(&cid("script/test/cargo-nextest")));
}

#[test]
fn a_session_overlay_can_re_enable_what_the_project_disabled() {
    let f = Fixture::new(vec![hook("hook/verify/full-regression")]).with_layers(vec![
        layer(ScopeKind::Project, &[], &["hook/verify/full-regression"]),
        layer(ScopeKind::Session, &["hook/verify/full-regression"], &[]),
    ]);

    let view = f.resolve().unwrap();
    assert_eq!(active_ids(&view), vec!["hook/verify/full-regression"]);
}

#[test]
fn layers_are_applied_in_precedence_order_regardless_of_the_order_they_are_supplied() {
    let ordered = Fixture::new(vec![script("script/test/a")]).with_layers(vec![
        layer(ScopeKind::Global, &["script/test/a"], &[]),
        layer(ScopeKind::Session, &[], &["script/test/a"]),
    ]);
    let shuffled = Fixture::new(vec![script("script/test/a")]).with_layers(vec![
        layer(ScopeKind::Session, &[], &["script/test/a"]),
        layer(ScopeKind::Global, &["script/test/a"], &[]),
    ]);

    assert!(ordered.resolve().unwrap().active.is_empty());
    assert!(shuffled.resolve().unwrap().active.is_empty());
    assert_eq!(
        ordered.resolve().unwrap().hash,
        shuffled.resolve().unwrap().hash
    );
}

#[test]
fn nested_project_layers_apply_from_the_repository_root_towards_the_working_directory() {
    let mut root = layer(ScopeKind::Project, &["script/test/a"], &[]);
    root.depth = 0;
    let mut nested = layer(ScopeKind::Project, &[], &["script/test/a"]);
    nested.depth = 2;

    // Supplied deepest-first on purpose.
    let f = Fixture::new(vec![script("script/test/a")]).with_layers(vec![nested, root]);
    assert!(
        f.resolve().unwrap().active.is_empty(),
        "the deeper package profile must win"
    );
}

// ---------------------------------------------------------------------------
// Rule 2 — managed denials cannot be overridden
// ---------------------------------------------------------------------------

#[test]
fn a_managed_denial_survives_an_explicit_session_enable() {
    let f = Fixture::new(vec![script("script/danger/rm-rf")])
        .with_layers(vec![layer(ScopeKind::Session, &["script/danger/rm-rf"], &[])])
        .with_policy(ManagedPolicy {
            deny: vec![cid("script/danger/rm-rf")],
            source: "test-policy".into(),
            ..Default::default()
        });

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    let reason = view.unavailable_reason(&cid("script/danger/rm-rf")).unwrap();
    assert_eq!(reason, &UnavailableReason::DeniedByPolicy);
    // Declared state and effective state must differ visibly, not silently.
    assert!(view.is_declared_enabled(&cid("script/danger/rm-rf")));
}

#[test]
fn a_managed_requirement_survives_an_explicit_session_disable() {
    let f = Fixture::new(vec![hook("hook/gate/project-boundary")])
        .with_layers(vec![layer(
            ScopeKind::Session,
            &[],
            &["hook/gate/project-boundary"],
        )])
        .with_policy(ManagedPolicy {
            require: vec![cid("hook/gate/project-boundary")],
            source: "test-policy".into(),
            ..Default::default()
        });

    let view = f.resolve().unwrap();
    assert_eq!(active_ids(&view), vec!["hook/gate/project-boundary"]);
    assert!(
        view.warnings.iter().any(|w| w.contains("managed policy")),
        "the user must be told their disable was overridden: {:?}",
        view.warnings
    );
}

#[test]
fn managed_policy_can_deny_by_declared_effect_class() {
    let f = Fixture::new(vec![script_with(
        "script/deploy/push",
        r#"
[effects]
network = true
"#,
    )])
    .with_layers(vec![layer(ScopeKind::Session, &["script/deploy/push"], &[])])
    .with_policy(ManagedPolicy {
        deny_effects: vec![EffectClass::Network],
        source: "test-policy".into(),
        ..Default::default()
    });

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    assert_eq!(
        view.unavailable_reason(&cid("script/deploy/push")).unwrap(),
        &UnavailableReason::DeniedByPolicy
    );
}

// ---------------------------------------------------------------------------
// Rule 3 — dependencies expand after explicit selection
// ---------------------------------------------------------------------------

#[test]
fn dependencies_are_expanded_transitively_and_marked_as_such() {
    let f = Fixture::new(vec![
        requiring("skill", 
            "skill/rust/release-review",
            &["guidance/security/release-policy"],
        ),
        requiring("guidance", 
            "guidance/security/release-policy",
            &["guidance/code/review-standard"],
        ),
        guidance("guidance/code/review-standard"),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["skill/rust/release-review"],
        &[],
    )]);

    let view = f.resolve().unwrap();
    assert_eq!(
        active_ids(&view),
        vec![
            "guidance/code/review-standard",
            "guidance/security/release-policy",
            "skill/rust/release-review",
        ]
    );

    let transitive = &view.active[&cid("guidance/code/review-standard")];
    assert!(matches!(
        transitive.origin,
        SelectionOrigin::Dependency { .. }
    ));
    let explicit = &view.active[&cid("skill/rust/release-review")];
    assert!(matches!(explicit.origin, SelectionOrigin::Layer { .. }));
}

#[test]
fn an_optional_dependency_that_is_absent_does_not_break_resolution() {
    let f = Fixture::new(vec![script_with(
        "script/test/gate",
        r#"
[[requires]]
id = "script/env/venv-detect"
optional = true
"#,
    )])
    .with_layers(vec![layer(ScopeKind::Project, &["script/test/gate"], &[])]);

    let view = f.resolve().unwrap();
    assert_eq!(active_ids(&view), vec!["script/test/gate"]);
    assert!(view.warnings.iter().any(|w| w.contains("venv-detect")));
}

#[test]
fn a_missing_required_dependency_fails_visibly() {
    let f = Fixture::new(vec![script_with(
        "script/test/gate",
        r#"
[[requires]]
id = "script/env/venv-detect"
"#,
    )])
    .with_layers(vec![layer(ScopeKind::Project, &["script/test/gate"], &[])]);

    let err = f.resolve().unwrap_err();
    assert_eq!(err.code(), "resolution.missing_dependency");
}

// ---------------------------------------------------------------------------
// Rule 4 — an explicitly disabled dependency fails; it is never silently re-enabled
// ---------------------------------------------------------------------------

#[test]
fn an_explicitly_disabled_requirement_fails_rather_than_being_silently_re_enabled() {
    let f = Fixture::new(vec![
        requiring("skill", 
            "skill/rust/release-review",
            &["guidance/security/release-policy"],
        ),
        guidance("guidance/security/release-policy"),
    ])
    .with_layers(vec![
        layer(ScopeKind::Project, &["skill/rust/release-review"], &[]),
        layer(ScopeKind::Session, &[], &["guidance/security/release-policy"]),
    ]);

    let err = f.resolve().unwrap_err();
    assert_eq!(err.code(), "resolution.required_capability_disabled");
    assert_eq!(
        err.details().get("capability").map(String::as_str),
        Some("guidance/security/release-policy")
    );
    assert_eq!(
        err.details().get("required_by").map(String::as_str),
        Some("skill/rust/release-review")
    );
    assert_eq!(err.details().get("scope").map(String::as_str), Some("session"));
    assert!(
        err.details().contains_key("origin"),
        "the message must be able to name the file and line that disabled it"
    );
}

// ---------------------------------------------------------------------------
// Rule 5 — conflicts fail visibly by default
// ---------------------------------------------------------------------------

#[test]
fn two_conflicting_active_capsules_fail_resolution() {
    let f = Fixture::new(vec![
        script_with(
            "script/test/pytest-gate",
            r#"
[[conflicts]]
id = "script/test/pytest-direct"
reason = "Both export the pytest-gate command."
"#,
        ),
        script("script/test/pytest-direct"),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["script/test/pytest-gate", "script/test/pytest-direct"],
        &[],
    )]);

    let err = f.resolve().unwrap_err();
    assert_eq!(err.code(), "resolution.conflict");
    assert!(err.to_string().contains("pytest-gate"));
}

#[test]
fn a_conflict_declared_by_only_one_side_is_still_detected() {
    let f = Fixture::new(vec![
        script("script/test/pytest-gate"),
        script_with(
            "script/test/pytest-direct",
            r#"
[[conflicts]]
id = "script/test/pytest-gate"
"#,
        ),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["script/test/pytest-gate", "script/test/pytest-direct"],
        &[],
    )]);

    assert_eq!(f.resolve().unwrap_err().code(), "resolution.conflict");
}

#[test]
fn two_capsules_exporting_the_same_command_collide() {
    let f = Fixture::new(vec![
        script_exporting("script/a/run", &["deploy"]),
        script_exporting("script/b/run", &["deploy"]),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["script/a/run", "script/b/run"],
        &[],
    )]);

    let err = f.resolve().unwrap_err();
    assert_eq!(err.code(), "resolution.export_collision");
    assert!(err.to_string().contains("deploy"));
}

#[test]
fn a_dependency_cycle_is_reported_rather_than_looping_forever() {
    let f = Fixture::new(vec![
        requiring("script", "script/a/one", &["script/b/two"]),
        requiring("script", "script/b/two", &["script/a/one"]),
    ])
    .with_layers(vec![layer(ScopeKind::Project, &["script/a/one"], &[])]);

    assert_eq!(f.resolve().unwrap_err().code(), "resolution.dependency_cycle");
}

// ---------------------------------------------------------------------------
// Rule 6 — nothing becomes active merely because it matches a tag
// ---------------------------------------------------------------------------

#[test]
fn a_capsule_in_the_catalog_is_not_active_merely_by_being_catalogued() {
    let f = Fixture::new(vec![
        script("script/test/a"),
        script("script/test/b"),
        skill("skill/rust/review"),
    ]);
    assert!(f.resolve().unwrap().active.is_empty());
}

#[test]
fn sharing_a_tag_with_an_active_capsule_does_not_activate_anything() {
    let f = Fixture::new(vec![
        script_with("script/test/a", "tags = [\"rust\"]"),
        script_with("script/test/b", "tags = [\"rust\"]"),
    ])
    .with_layers(vec![layer(ScopeKind::Project, &["script/test/a"], &[])]);

    assert_eq!(active_ids(&f.resolve().unwrap()), vec!["script/test/a"]);
}

// ---------------------------------------------------------------------------
// Rule 7 — every decision is explainable
// ---------------------------------------------------------------------------

#[test]
fn every_active_capability_can_explain_why_it_is_active() {
    let f = Fixture::new(vec![
        requiring("skill", "skill/rust/review", &["guidance/code/review-standard"]),
        guidance("guidance/code/review-standard"),
    ])
    .with_profiles(vec![profile(
        "profile/code/rust",
        &["skill/rust/review"],
        &[],
    )])
    .with_layers(vec![layer_using(ScopeKind::Project, &["profile/code/rust"])]);

    let view = f.resolve().unwrap();
    let explanation = view.explain(&cid("skill/rust/review")).unwrap();

    assert!(explanation.selected_by.iter().any(|s| s.contains("profile/code/rust")));
    assert!(explanation
        .selected_by
        .iter()
        .any(|s| s.contains("test:project")));
    assert_eq!(
        explanation.dependencies,
        vec!["guidance/code/review-standard".to_string()]
    );
    assert!(explanation.required_by.is_empty());

    let dep = view.explain(&cid("guidance/code/review-standard")).unwrap();
    assert_eq!(dep.required_by, vec!["skill/rust/review".to_string()]);
}

#[test]
fn an_inactive_capability_explains_why_it_is_not_active() {
    let f = Fixture::new(vec![script("script/test/a")]);
    let view = f.resolve().unwrap();
    let explanation = view.explain(&cid("script/test/a")).unwrap();
    assert!(explanation.selected_by.is_empty());
    assert!(!explanation.active);
}

// ---------------------------------------------------------------------------
// Availability: declared vs effective
// ---------------------------------------------------------------------------

#[test]
fn a_platform_incompatible_capsule_is_unavailable_rather_than_a_hard_error() {
    let mut d = descriptor();
    d.platform = Platform::Macos;
    let f = Fixture::new(vec![script_with(
        "script/linux/perf",
        "platforms = [\"linux\"]",
    )])
    .with_descriptor(d)
    .with_layers(vec![layer(ScopeKind::Session, &["script/linux/perf"], &[])]);

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    assert_eq!(
        view.unavailable_reason(&cid("script/linux/perf")).unwrap(),
        &UnavailableReason::PlatformUnsupported
    );
    assert!(view.is_declared_enabled(&cid("script/linux/perf")));
}

#[test]
fn an_unreviewed_hook_cannot_activate() {
    let f = Fixture::new(vec![hook("hook/gate/unknown")])
        .with_layers(vec![layer(ScopeKind::Session, &["hook/gate/unknown"], &[])])
        .untrust("hook/gate/unknown");

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    assert_eq!(
        view.unavailable_reason(&cid("hook/gate/unknown")).unwrap(),
        &UnavailableReason::TrustRequired
    );
}

#[test]
fn an_unreviewed_guidance_capsule_is_inspectable_but_not_injected() {
    let f = Fixture::new(vec![guidance("guidance/mode/research")])
        .with_layers(vec![layer(ScopeKind::Session, &["guidance/mode/research"], &[])])
        .untrust("guidance/mode/research");

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    assert_eq!(
        view.unavailable_reason(&cid("guidance/mode/research")).unwrap(),
        &UnavailableReason::TrustRequired
    );
    // It is still catalogued and previewable — the palette must be able to show it.
    assert!(f.catalog_contains("guidance/mode/research"));
}

#[test]
fn an_unreviewed_script_may_activate_but_is_flagged_for_run_confirmation() {
    let f = Fixture::new(vec![script("script/test/fresh")])
        .with_layers(vec![layer(ScopeKind::Session, &["script/test/fresh"], &[])])
        .untrust("script/test/fresh");

    let view = f.resolve().unwrap();
    let active = &view.active[&cid("script/test/fresh")];
    assert!(
        active.requires_run_confirmation,
        "an unreviewed script is exposed but must not run unattended"
    );
}

#[test]
fn a_quarantined_capsule_never_projects_whatever_its_kind() {
    for (id, make) in [
        ("script/test/x", script as fn(&str) -> _),
        ("skill/test/x", skill as fn(&str) -> _),
    ] {
        let f = Fixture::new(vec![make(id)])
            .with_layers(vec![layer(ScopeKind::Session, &[id], &[])])
            .set_trust(id, TrustState::Quarantined);
        let view = f.resolve().unwrap();
        assert!(view.active.is_empty(), "{id} must not activate");
        assert_eq!(
            view.unavailable_reason(&cid(id)).unwrap(),
            &UnavailableReason::Quarantined
        );
    }
}

#[test]
fn a_blocked_capsule_is_unavailable_even_when_trusted() {
    let f = Fixture::new(vec![script_with(
        "script/old/thing",
        "maturity = \"blocked\"",
    )])
    .with_layers(vec![layer(ScopeKind::Session, &["script/old/thing"], &[])]);

    let view = f.resolve().unwrap();
    assert_eq!(
        view.unavailable_reason(&cid("script/old/thing")).unwrap(),
        &UnavailableReason::Blocked
    );
}

#[test]
fn a_capability_whose_dependency_is_unavailable_becomes_unavailable_too() {
    let f = Fixture::new(vec![
        requiring("skill", "skill/rust/review", &["hook/gate/needed"]),
        hook("hook/gate/needed"),
    ])
    .with_layers(vec![layer(ScopeKind::Session, &["skill/rust/review"], &[])])
    .untrust("hook/gate/needed");

    let view = f.resolve().unwrap();
    assert!(view.active.is_empty());
    assert!(matches!(
        view.unavailable_reason(&cid("skill/rust/review")).unwrap(),
        UnavailableReason::DependencyUnavailable { .. }
    ));
}

#[test]
fn a_capsule_that_declares_no_support_for_a_context_target_is_not_projected_there() {
    let f = Fixture::new(vec![script_with(
        "script/shell/only",
        "targets = [\"shell\"]",
    )])
    .with_layers(vec![layer(ScopeKind::Session, &["script/shell/only"], &[])]);

    let view = f.resolve().unwrap();
    let active = &view.active[&cid("script/shell/only")];
    assert!(active.targets.contains(&TargetId::shell()));
    assert!(!active.targets.contains(&TargetId::claude_code()));
}

#[test]
fn a_capsule_supporting_no_context_target_at_all_is_unavailable() {
    let f = Fixture::new(vec![script_with(
        "script/other/only",
        "targets = [\"some-other-client\"]",
    )])
    .with_layers(vec![layer(ScopeKind::Session, &["script/other/only"], &[])]);

    let view = f.resolve().unwrap();
    assert_eq!(
        view.unavailable_reason(&cid("script/other/only")).unwrap(),
        &UnavailableReason::NoSupportedTarget
    );
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[test]
fn profiles_compose_through_extends_depth_first() {
    let f = Fixture::new(vec![
        script("script/base/safe-thing"),
        script("script/general/general-thing"),
        script("script/rust/rust-thing"),
    ])
    .with_profiles(vec![
        profile("profile/base/safe", &["script/base/safe-thing"], &[]),
        profile("profile/code/general", &["script/general/general-thing"], &[]),
        {
            let mut p = profile("profile/code/rust", &["script/rust/rust-thing"], &[]);
            p.extends = vec![pid("profile/base/safe"), pid("profile/code/general")];
            p
        },
    ])
    .with_layers(vec![layer_using(ScopeKind::Project, &["profile/code/rust"])]);

    let view = f.resolve().unwrap();
    assert_eq!(
        active_ids(&view),
        vec![
            "script/base/safe-thing",
            "script/general/general-thing",
            "script/rust/rust-thing"
        ]
    );
}

#[test]
fn a_profile_referenced_twice_is_expanded_once() {
    let f = Fixture::new(vec![script("script/base/thing")])
        .with_profiles(vec![
            profile("profile/base/safe", &["script/base/thing"], &[]),
            {
                let mut p = profile("profile/code/rust", &[], &[]);
                p.extends = vec![pid("profile/base/safe")];
                p
            },
        ])
        .with_layers(vec![layer_using(
            ScopeKind::Project,
            &["profile/base/safe", "profile/code/rust"],
        )]);

    assert_eq!(active_ids(&f.resolve().unwrap()), vec!["script/base/thing"]);
}

#[test]
fn a_profile_cycle_is_reported() {
    let f = Fixture::new(vec![])
        .with_profiles(vec![
            {
                let mut p = profile("profile/a/one", &[], &[]);
                p.extends = vec![pid("profile/b/two")];
                p
            },
            {
                let mut p = profile("profile/b/two", &[], &[]);
                p.extends = vec![pid("profile/a/one")];
                p
            },
        ])
        .with_layers(vec![layer_using(ScopeKind::Project, &["profile/a/one"])]);

    assert_eq!(f.resolve().unwrap_err().code(), "resolution.profile_cycle");
}

#[test]
fn an_unknown_profile_is_an_error_not_a_silent_no_op() {
    let f = Fixture::new(vec![]).with_layers(vec![layer_using(
        ScopeKind::Project,
        &["profile/does/not-exist"],
    )]);
    assert_eq!(f.resolve().unwrap_err().code(), "resolution.unknown_profile");
}

#[test]
fn enabling_an_unknown_capability_is_an_error_so_typos_surface_immediately() {
    let f = Fixture::new(vec![]).with_layers(vec![layer(
        ScopeKind::Session,
        &["script/typo/thnig"],
        &[],
    )]);
    let err = f.resolve().unwrap_err();
    assert_eq!(err.code(), "resolution.unknown_capability");
}

#[test]
fn disabling_an_unknown_capability_is_tolerated_because_registries_shrink() {
    let f = Fixture::new(vec![]).with_layers(vec![layer(
        ScopeKind::Session,
        &[],
        &["script/gone/away"],
    )]);
    let view = f.resolve().unwrap();
    assert!(view.warnings.iter().any(|w| w.contains("script/gone/away")));
}

// ---------------------------------------------------------------------------
// Configuration layering
// ---------------------------------------------------------------------------

#[test]
fn configuration_from_higher_layers_overrides_lower_layers_key_by_key() {
    let mut project = layer(ScopeKind::Project, &["hook/verify/cargo-check"], &[]);
    project.patch.config.insert(
        cid("hook/verify/cargo-check"),
        toml_table(&[("mode", "changed-crates"), ("timeout", "90s")]),
    );
    let mut session = layer(ScopeKind::Session, &[], &[]);
    session
        .patch
        .config
        .insert(cid("hook/verify/cargo-check"), toml_table(&[("mode", "full")]));

    let f = Fixture::new(vec![hook("hook/verify/cargo-check")])
        .with_layers(vec![project, session]);

    let view = f.resolve().unwrap();
    let config = &view.active[&cid("hook/verify/cargo-check")].config;
    assert_eq!(config.get("mode").unwrap().as_str(), Some("full"));
    assert_eq!(config.get("timeout").unwrap().as_str(), Some("90s"));
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn the_resolution_hash_is_stable_across_semantically_equivalent_orderings() {
    let build = |order: [&str; 3]| {
        Fixture::new(vec![
            script("script/test/a"),
            script("script/test/b"),
            script("script/test/c"),
        ])
        .with_layers(vec![layer(ScopeKind::Project, &order, &[])])
        .resolve()
        .unwrap()
        .hash
    };

    assert_eq!(
        build(["script/test/a", "script/test/b", "script/test/c"]),
        build(["script/test/c", "script/test/a", "script/test/b"])
    );
}

#[test]
fn the_resolution_hash_changes_when_the_active_set_changes() {
    let one = Fixture::new(vec![script("script/test/a"), script("script/test/b")])
        .with_layers(vec![layer(ScopeKind::Project, &["script/test/a"], &[])])
        .resolve()
        .unwrap()
        .hash;
    let two = Fixture::new(vec![script("script/test/a"), script("script/test/b")])
        .with_layers(vec![layer(
            ScopeKind::Project,
            &["script/test/a", "script/test/b"],
            &[],
        )])
        .resolve()
        .unwrap()
        .hash;
    assert_ne!(one, two);
}

#[test]
fn the_resolution_hash_changes_when_a_capsule_revision_changes() {
    let base = Fixture::new(vec![script("script/test/a")])
        .with_layers(vec![layer(ScopeKind::Project, &["script/test/a"], &[])]);
    let first = base.resolve().unwrap().hash;

    let mut bumped = base;
    bumped.bump_revision("script/test/a");
    assert_ne!(first, bumped.resolve().unwrap().hash);
}

#[test]
fn the_resolution_hash_ignores_the_file_a_layer_came_from() {
    let mut a = layer(ScopeKind::Project, &["script/test/a"], &[]);
    a.origin = aikit_core::scope::LayerOrigin::new("/one/.aikit/profile.toml");
    let mut b = layer(ScopeKind::Project, &["script/test/a"], &[]);
    b.origin = aikit_core::scope::LayerOrigin::new("/two/.aikit/profile.toml");

    let ha = Fixture::new(vec![script("script/test/a")])
        .with_layers(vec![a])
        .resolve()
        .unwrap()
        .hash;
    let hb = Fixture::new(vec![script("script/test/a")])
        .with_layers(vec![b])
        .resolve()
        .unwrap()
        .hash;
    assert_eq!(ha, hb);
}

#[test]
fn the_resolution_hash_distinguishes_contexts_with_different_targets() {
    let mut d = descriptor();
    d.targets = vec![TargetId::shell()];
    let narrow = Fixture::new(vec![script("script/test/a")])
        .with_descriptor(d)
        .with_layers(vec![layer(ScopeKind::Project, &["script/test/a"], &[])])
        .resolve()
        .unwrap()
        .hash;
    let wide = Fixture::new(vec![script("script/test/a")])
        .with_layers(vec![layer(ScopeKind::Project, &["script/test/a"], &[])])
        .resolve()
        .unwrap()
        .hash;
    assert_ne!(narrow, wide);
}

// ---------------------------------------------------------------------------
// Isolation is a choice, not a default
// ---------------------------------------------------------------------------

#[test]
fn a_context_defaults_to_sharing_the_session_working_tree() {
    assert_eq!(descriptor().isolation, Isolation::Shared);
    assert!(!descriptor().isolation.is_isolated());
}

#[test]
fn worktree_isolation_is_available_but_must_be_asked_for() {
    let mut d = descriptor();
    d.isolation = Isolation::Worktree;
    assert!(d.isolation.is_isolated());
    assert!(d.isolation.owns_a_git_worktree());

    let mut plain_dir = descriptor();
    plain_dir.isolation = Isolation::Directory;
    assert!(plain_dir.isolation.is_isolated());
    assert!(!plain_dir.isolation.owns_a_git_worktree());
}

#[test]
fn isolation_is_part_of_the_resolution_hash_because_it_changes_projection() {
    let shared = Fixture::new(vec![skill("skill/rust/review")])
        .with_layers(vec![layer(ScopeKind::Session, &["skill/rust/review"], &[])])
        .resolve()
        .unwrap()
        .hash;

    let mut d = descriptor();
    d.isolation = Isolation::Worktree;
    let isolated = Fixture::new(vec![skill("skill/rust/review")])
        .with_descriptor(d)
        .with_layers(vec![layer(ScopeKind::Session, &["skill/rust/review"], &[])])
        .resolve()
        .unwrap()
        .hash;

    assert_ne!(shared, isolated);
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_resolution_reports_every_problem_instead_of_stopping_at_the_first() {
    let f = Fixture::new(vec![
        requiring("script", "script/a/one", &["script/missing/dep"]),
        script("script/b/two"),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["script/a/one", "script/b/two", "script/also/missing"],
        &[],
    )]);

    let diagnosis = f.diagnose();
    assert!(diagnosis.view.is_none() || diagnosis.problems.len() >= 2);
    let codes: Vec<&str> = diagnosis.problems.iter().map(|p| p.code()).collect();
    assert!(codes.contains(&"resolution.unknown_capability"));
    assert!(codes.contains(&"resolution.missing_dependency"));
}

#[test]
fn a_healthy_context_diagnoses_cleanly() {
    let f = Fixture::new(vec![script("script/test/a")])
        .with_layers(vec![layer(ScopeKind::Project, &["script/test/a"], &[])]);
    let diagnosis = f.diagnose();
    assert!(diagnosis.problems.is_empty());
    assert!(diagnosis.view.is_some());
}

// ---------------------------------------------------------------------------
// Kind-aware active semantics
// ---------------------------------------------------------------------------

#[test]
fn an_inactive_script_is_still_runnable_but_an_inactive_hook_is_not() {
    let f = Fixture::new(vec![script("script/test/a"), hook("hook/gate/b")]);
    let view = f.resolve().unwrap();
    assert!(view.can_run(&cid("script/test/a")));
    assert!(!view.can_run(&cid("hook/gate/b")));
    assert!(Kind::Script.runnable_while_inactive());
}

#[test]
fn activating_a_hook_or_guidance_capsule_does_not_make_it_runnable() {
    // "Run" is not a meaningful separate act for a hook, a skill or a guidance
    // capsule: a hook runs when its event fires and guidance is composed into a
    // prompt. Activation controls *ambient exposure*, so letting it also confer
    // runnability would put every active hook into the palette's `>` lane, where
    // selecting one could do nothing useful.
    let f = Fixture::new(vec![
        script("script/test/a"),
        hook("hook/gate/b"),
        guidance("guidance/mode/c"),
        skill("skill/rust/d"),
    ])
    .with_layers(vec![layer(
        ScopeKind::Session,
        &[
            "script/test/a",
            "hook/gate/b",
            "guidance/mode/c",
            "skill/rust/d",
        ],
        &[],
    )]);
    let view = f.resolve().unwrap();

    for id in [
        "script/test/a",
        "hook/gate/b",
        "guidance/mode/c",
        "skill/rust/d",
    ] {
        assert!(view.is_active(&cid(id)), "{id} should be active");
    }

    assert!(view.can_run(&cid("script/test/a")));
    assert!(!view.can_run(&cid("hook/gate/b")));
    assert!(!view.can_run(&cid("guidance/mode/c")));
    assert!(!view.can_run(&cid("skill/rust/d")));
}

// ---------------------------------------------------------------------------
// local helpers
// ---------------------------------------------------------------------------

fn toml_table(pairs: &[(&str, &str)]) -> toml::value::Table {
    let mut t = toml::value::Table::new();
    for (k, v) in pairs {
        t.insert(k.to_string(), toml::Value::String(v.to_string()));
    }
    t
}
