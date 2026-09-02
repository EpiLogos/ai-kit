mod common;

use common::*;

use aikit_core::projection::ActivationEffect;
use aikit_core::scope::ScopeKind;
use aikit_core::search::UsageStats;
use aikit_core::TargetId;
use aikit_tui::{
    ActivationIntent, ApplicationService, ClientEffect, StagedChanges, TuiApplicationService,
};

#[test]
fn production_application_searches_the_resolved_resource_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );

    let service = ApplicationService::new(&mut backend);
    let model = service.search("deploy").unwrap();

    assert!(model
        .resources
        .iter()
        .any(|item| item.resource.as_str() == "script/ops/deploy"));
    assert!(model.revision.contains("deploy"));
}

#[test]
fn legacy_package_usage_stats_do_not_become_v2_navigation_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );
    backend.usage.insert(
        cid("script/ops/deploy"),
        UsageStats {
            successful_runs: 7,
            failed_runs: 0,
            last_success_age: None,
        },
    );

    let service = ApplicationService::new(&mut backend);

    let zero_query = service.search("").unwrap();
    assert!(
        zero_query
            .resources
            .iter()
            .all(|item| item.resource.as_str() != "script/ops/deploy"),
        "legacy SearchDoc/UsageStats must not manufacture zero-query V2 navigation evidence"
    );

    let deploy = service
        .search("deploy")
        .unwrap()
        .resources
        .into_iter()
        .find(|item| item.resource.as_str() == "script/ops/deploy")
        .expect("resolved package-backed capability remains discoverable by canonical text search");
    assert!(
        !deploy.summary.contains("learned usage"),
        "legacy SearchDoc/UsageStats must not leak into the canonical Resource field"
    );
    assert!(!deploy.summary.contains("preferred"));
    assert!(!deploy.summary.contains("trusted"));
}

#[test]
fn production_application_previews_target_activation_semantics_before_apply() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );
    let mut staged = StagedChanges::default();
    let resource = aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap();
    staged.stage(resource, ActivationIntent::Enable);

    let preview = {
        let service = ApplicationService::new(&mut backend);
        service
            .preview_composition(ScopeKind::Project, &staged)
            .unwrap()
    };
    assert_eq!(preview.scope, ScopeKind::Project);
    assert_eq!(preview.staged, staged);
    assert!(preview.summary.contains("target effects:"));
    assert!(preview.summary.contains("live"));
    assert!(backend.applied.is_empty());

    let receipt = {
        let mut service = ApplicationService::new(&mut backend);
        service.apply_composition(&preview).unwrap()
    };
    assert!(receipt.summary.contains("applied generation"));
    assert_eq!(backend.applied.len(), 1);
    assert_eq!(backend.applied[0].0, ScopeKind::Project);
    assert_eq!(backend.applied[0].1.len(), 1);
    assert!(backend.applied[0].1[0].enable);
}

#[test]
fn production_application_never_calls_restart_only_effect_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    backend.effects = vec![ClientEffect::new(
        TargetId::claude_code(),
        ActivationEffect::restart_client("Claude Code"),
    )];
    let mut staged = StagedChanges::default();
    staged.stage(
        aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap(),
        ActivationIntent::Enable,
    );

    let preview = {
        let service = ApplicationService::new(&mut backend);
        service
            .preview_composition(ScopeKind::Project, &staged)
            .unwrap()
    };

    assert!(preview.summary.contains("restart Claude Code"));
    assert!(!preview.summary.contains(": live"));
    assert!(backend.applied.is_empty());
}

#[test]
fn package_backed_capability_state_is_explicit_compatibility_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![
            script_with(
                "script/ops/deploy",
                "\n[[requires]]\nid = \"skill/rust/review\"\n",
            ),
            skill("skill/rust/review"),
        ],
    );
    let subject = aikit_core::resource::ResourceRef::parse("script/ops/deploy").unwrap();
    let service = ApplicationService::new(&mut backend);

    let disclosure = service.context_disclosure(&subject).unwrap();
    assert_eq!(disclosure["resource"], subject.as_str());
    assert!(disclosure.get("resolutionHash").is_some());
    assert!(disclosure["packageCapabilityState"].is_object());

    let explanation = service.explain(&subject).unwrap();
    assert_eq!(explanation["resource"], subject.as_str());
    assert_eq!(explanation["kind"], "capability");
    assert!(explanation["packageCapabilityState"].is_object());

    let relations = service.relations(&subject).unwrap();
    assert_eq!(relations.subject, subject);
    assert!(relations.value.get("related").is_some());
}

#[test]
fn generic_host_never_falls_through_package_identity() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    let host = aikit_core::resource::ResourceRef::parse("host/test-host").unwrap();
    let service = ApplicationService::new(&mut backend);

    let disclosure = service.context_disclosure(&host).unwrap();
    assert_eq!(disclosure["resource"], host.as_str());
    assert_eq!(disclosure["kind"], "host");
    assert!(disclosure.get("ranking").is_some());
    assert!(disclosure["packageCapabilityState"].is_null());

    let explanation = service.explain(&host).unwrap();
    assert_eq!(explanation["resource"], host.as_str());
    assert_eq!(explanation["kind"], "host");
    assert!(explanation.get("eligibility").is_some());
    assert!(explanation.get("contextualActions").is_some());
    assert!(explanation.get("learnedAccessibility").is_some());
    assert!(explanation["packageCapabilityState"].is_null());

    let relations = service.relations(&host).unwrap();
    assert_eq!(relations.subject, host);
    assert!(relations.value.get("contextualActions").is_some());
    assert!(relations.value.get("resolverRelated").is_some());
    assert_eq!(
        relations.value["resolverRelated"].as_array().unwrap().len(),
        0
    );
}
