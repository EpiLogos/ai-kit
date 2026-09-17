use std::path::PathBuf;

use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::resource::{
    Eligibility, FactoryInteropView, PreferenceIntent, ProviderOffer, ProviderRef, ProviderState,
    ResourceDescriptor, ResourceKind, ResourceLocator, ResourceRecord, ResourceRef, ResourceSource,
    SourceRef, SourceRevision, SourceState,
};
use aikit_core::{ContextDescriptor, ProjectId};

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

#[test]
fn factory_cr001_fixture_is_consumed_as_external_resource_views_without_identity_translation() {
    let view =
        FactoryInteropView::from_fixture_json(include_str!("fixtures/factory-interop-v1.json"))
            .expect("Factory CR-001 fixture");

    let action = view.action_resource().expect("Factory Action view");
    assert_eq!(
        action.record.descriptor.id.as_str(),
        "factory:action:update"
    );
    assert_eq!(action.record.descriptor.kind, ResourceKind::Action);
    assert_eq!(
        action.record.descriptor.owner.as_ref().unwrap().as_str(),
        "factory:project:factory"
    );
    assert_eq!(
        action.record.descriptor.sources[0].source.as_str(),
        "source:project:factory"
    );
    assert_eq!(
        action.record.descriptor.sources[0]
            .revision
            .as_ref()
            .unwrap()
            .as_str(),
        "rev:3"
    );
    assert_eq!(
        action.record.descriptor.sources[0].state,
        SourceState::Unresolved
    );
    assert!(action.declared_provider.is_none());

    let capability = view.capability_resource().expect("Factory Capability view");
    assert_eq!(
        capability.record.descriptor.id.as_str(),
        "factory:capability:browser"
    );
    assert_eq!(capability.record.descriptor.kind, ResourceKind::Capability);
    assert_ne!(action.record.descriptor.id, capability.record.descriptor.id);
    assert_eq!(
        capability.declared_provider.as_ref().unwrap().as_str(),
        "provider:browser"
    );
    assert_eq!(
        capability.record.providers[0].state,
        ProviderState::Unresolved
    );
    assert_eq!(
        capability.record.providers[0].provider.as_str(),
        "provider:browser"
    );

    let factory_project_ref = view.project_ref().expect("Factory ProjectRef");
    assert_eq!(factory_project_ref.as_str(), "factory:project:factory");
    let project_source = view.project_source().expect("Factory project source");
    assert_eq!(
        project_source.source.as_str(),
        "source:git:EpiLogos/agent-system-design"
    );
    assert_eq!(project_source.state, SourceState::Unresolved);

    let operational_binding = ProjectBinding::new(
        factory_project_ref,
        ProjectConstituentRef::parse("constituent:source").unwrap(),
        ProjectBindingLocator::LocalDirectory {
            path: PathBuf::from("/work/factory"),
        },
    );
    assert_eq!(
        operational_binding.project.as_str(),
        "factory:project:factory"
    );
    assert!(operational_binding.legacy_aikit_project_id.is_none());
}

#[test]
fn imported_factory_provider_and_source_state_can_degrade_without_rewriting_external_identity() {
    let view =
        FactoryInteropView::from_fixture_json(include_str!("fixtures/factory-interop-v1.json"))
            .unwrap();
    let mut capability = view.capability_resource().unwrap().record;
    let original = capability.descriptor.id.clone();

    capability.descriptor.sources[0].state = SourceState::Unavailable {
        reason: "Factory source not reachable".into(),
    };
    capability.providers[0].state = ProviderState::Unavailable {
        reason: "declared provider not installed".into(),
    };

    let explanation = capability.explanation();
    assert_eq!(explanation.id, original);
    assert_eq!(explanation.id.as_str(), "factory:capability:browser");
    assert!(matches!(
        explanation.sources[0].state,
        SourceState::Unavailable { .. }
    ));
    assert!(matches!(
        explanation.providers[0].state,
        ProviderState::Unavailable { .. }
    ));
}
