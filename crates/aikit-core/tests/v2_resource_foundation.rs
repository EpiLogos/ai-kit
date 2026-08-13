use std::path::PathBuf;

use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::resource::{
    Eligibility, PreferenceIntent, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor,
    ResourceKind, ResourceLocator, ResourceRecord, ResourceRef, ResourceSource, SourceRef,
    SourceRevision, SourceState,
};
use aikit_core::{Capsule, ContextDescriptor, ProjectId, RegistrySource, Revision};

#[test]
fn action_and_capability_remain_distinct_resource_types() {
    let action = ResourceDescriptor::new(
        ResourceRef::parse("central:action/work.open").unwrap(),
        ResourceKind::Action,
        "Open work",
        "Externally owned Action view",
    );
    let capability = ResourceDescriptor::new(
        ResourceRef::parse("skill/code/review").unwrap(),
        ResourceKind::Capability,
        "Review",
        "AIKit capability view",
    );

    assert_eq!(action.kind, ResourceKind::Action);
    assert_eq!(capability.kind, ResourceKind::Capability);
    assert_ne!(action.id, capability.id);
}

#[test]
fn provider_and_source_loss_do_not_rewrite_resource_identity() {
    let identity = ResourceRef::parse("model:provider/model-x").unwrap();
    let mut descriptor = ResourceDescriptor::new(
        identity.clone(),
        ResourceKind::Model,
        "Model X",
        "Imported model identity",
    );
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse("source:model-registry").unwrap(),
        authority: None,
        revision: Some(SourceRevision::parse("rev-7").unwrap()),
        locator: None,
        state: SourceState::Available,
    });
    let mut record = ResourceRecord::new(descriptor);
    record.providers.push(ProviderOffer {
        provider: ProviderRef::parse("provider:model-runtime").unwrap(),
        locator: Some(ResourceLocator::Uri("https://provider.invalid".into())),
        state: ProviderState::Available,
    });

    record.descriptor.sources[0].state = SourceState::Unavailable {
        reason: "registry offline".into(),
    };
    record.providers[0].state = ProviderState::Unavailable {
        reason: "provider offline".into(),
    };

    let explanation = record.explanation();
    assert_eq!(explanation.id, identity);
    assert_eq!(explanation.sources.len(), 1);
    assert_eq!(explanation.providers.len(), 1);
    assert!(matches!(
        explanation.sources[0].state,
        SourceState::Unavailable { .. }
    ));
    assert!(matches!(
        explanation.providers[0].state,
        ProviderState::Unavailable { .. }
    ));
}

#[test]
fn eligibility_and_preference_are_independent_axes() {
    let descriptor = ResourceDescriptor::new(
        ResourceRef::parse("harness:pi").unwrap(),
        ResourceKind::Harness,
        "Pi",
        "Harness view",
    );
    let mut record = ResourceRecord::new(descriptor);
    record.eligibility = Eligibility::Ineligible {
        reasons: vec!["host unsupported".into()],
    };
    record.preference = Some(PreferenceIntent {
        source: SourceRef::parse("profile:default").unwrap(),
        rank: 100,
        rationale: Some("preferred harness".into()),
    });

    assert!(!record.eligibility.is_eligible());
    assert_eq!(record.preference.as_ref().unwrap().rank, 100);
}

#[test]
fn legacy_capsule_adapter_preserves_capsule_identity_and_provenance() {
    let mut capsule = Capsule::from_toml_str(
        r#"
schema = 1
id = "script/test/check"
kind = "script"
name = "Check"
description = "Legacy check capability."
[script]
entry = "payload/check.sh"
"#,
    )
    .unwrap();
    capsule.source = Some(RegistrySource::new("team"));
    capsule.revision = Some(Revision::from_raw("content-rev"));
    capsule.root = Some(PathBuf::from("/registry/check"));

    let record = ResourceRecord::try_from(&capsule).unwrap();
    assert_eq!(record.descriptor.id.as_str(), "script/test/check");
    assert_eq!(record.descriptor.kind, ResourceKind::Capability);
    assert_eq!(record.descriptor.sources[0].source.as_str(), "registry:team");
    assert_eq!(
        record.descriptor.sources[0].revision.as_ref().unwrap().as_str(),
        "content-rev"
    );
    assert!(record.providers.is_empty());
}

#[test]
fn legacy_context_becomes_a_binding_only_with_external_project_identity() {
    let mut context = ContextDescriptor::for_project("/work/ai-kit");
    let legacy = ProjectId::generate();
    context.project_id = Some(legacy.clone());

    let binding = ProjectBinding::from_legacy_context(
        ProjectRef::parse("project:epilogos/ai-kit").unwrap(),
        ProjectConstituentRef::parse("constituent:source").unwrap(),
        &context,
    )
    .unwrap();

    assert_eq!(binding.project.as_str(), "project:epilogos/ai-kit");
    assert_eq!(binding.legacy_aikit_project_id, Some(legacy));
    assert!(matches!(
        binding.locator,
        ProjectBindingLocator::LocalDirectory { .. }
    ));
}
