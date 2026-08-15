mod common;

use common::*;

use aikit_core::projection::ActivationEffect;
use aikit_core::scope::ScopeKind;
use aikit_core::search::UsageStats;
use aikit_core::TargetId;
use aikit_tui::{
    ActivationIntent, ClientEffect, PaletteApplicationService, StagedChanges,
    TuiApplicationService,
};

#[test]
fn production_adapter_searches_the_same_resolved_catalog_as_the_palette() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );

    let service = PaletteApplicationService::new(&mut backend);
    let model = service.search("deploy").unwrap();

    assert_eq!(model.resources.len(), 1);
    assert_eq!(model.resources[0].resource.as_str(), "script/ops/deploy");
    assert!(model.revision.contains("deploy"));
}

#[test]
fn zero_query_learned_usage_stays_labelled_as_evidence_not_preference() {
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

    let service = PaletteApplicationService::new(&mut backend);
    let model = service.search("").unwrap();
    let deploy = model
        .resources
        .iter()
        .find(|item| item.resource.as_str() == "script/ops/deploy")
        .expect("learned usage should make the destination visible at zero query");

    assert!(deploy.summary.contains("evidence: learned usage"));
    assert!(deploy.summary.contains("7 successful run(s)"));
    assert!(!deploy.summary.contains("preferred"));
    assert!(!deploy.summary.contains("trusted"));
}

#[test]
fn production_adapter_previews_target_activation_semantics_before_apply() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );
    let mut staged = StagedChanges::default();
    let resource = aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap();
    staged.stage(resource, ActivationIntent::Enable);

    let preview = {
        let service = PaletteApplicationService::new(&mut backend);
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
        let mut service = PaletteApplicationService::new(&mut backend);
        service.apply_composition(&preview).unwrap()
    };
    assert!(receipt.summary.contains("applied generation"));
    assert_eq!(backend.applied.len(), 1);
    assert_eq!(backend.applied[0].0, ScopeKind::Project);
    assert_eq!(backend.applied[0].1.len(), 1);
    assert!(backend.applied[0].1[0].enable);
}

#[test]
fn production_adapter_never_calls_restart_only_effect_live() {
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
        let service = PaletteApplicationService::new(&mut backend);
        service
            .preview_composition(ScopeKind::Project, &staged)
            .unwrap()
    };

    assert!(preview.summary.contains("restart Claude Code"));
    assert!(!preview.summary.contains(": live"));
    assert!(backend.applied.is_empty());
}

#[test]
fn production_adapter_explain_context_and_relations_are_resolved_read_models() {
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
    let service = PaletteApplicationService::new(&mut backend);

    let disclosure = service.context_disclosure(&subject).unwrap();
    assert_eq!(disclosure["resource"], subject.as_str());
    assert!(disclosure.get("resolutionHash").is_some());

    let explanation = service.explain(&subject).unwrap();
    assert_eq!(explanation["resource"], subject.as_str());
    assert!(explanation.get("active").is_some());

    let relations = service.relations(&subject).unwrap();
    assert_eq!(relations.subject, subject);
    assert!(relations.value.get("related").is_some());
}
