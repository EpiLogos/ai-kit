mod common;

use std::path::PathBuf;

use aikit_core::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
use aikit_core::resource::{
    Eligibility, FactoryInteropView, MemoryResourceIndex, PreferenceIntent, ProviderOffer,
    ProviderRef, ProviderState, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSource, SourceRef, SourceRevision, SourceState,
};
use aikit_core::scope::ScopeKind;
use aikit_core::{
    compose_context_resolution, resource_availability, Availability, ReferenceResolution,
    RequestedActors, CONTEXT_RESOLUTION_VERSION,
};

use common::{layer_using, profile, script, Fixture};

fn record(
    id: &str,
    kind: ResourceKind,
    source_state: SourceState,
    provider_state: Option<ProviderState>,
) -> ResourceRecord {
    let mut descriptor = ResourceDescriptor::new(
        ResourceRef::parse(id).unwrap(),
        kind,
        id,
        format!("V2 test resource {id}"),
    );
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse(&format!("source:{id}")).unwrap(),
        authority: None,
        revision: Some(SourceRevision::parse("rev:1").unwrap()),
        locator: None,
        state: source_state,
    });
    let mut record = ResourceRecord::new(descriptor);
    if let Some(state) = provider_state {
        record.providers.push(ProviderOffer {
            provider: ProviderRef::parse(&format!("provider:{id}")).unwrap(),
            locator: None,
            state,
        });
    }
    record
}

fn deterministic_fixture() -> (aikit_core::ResolvedView, Vec<aikit_core::ScopeLayer>) {
    let layers = vec![layer_using(ScopeKind::Project, &["profile/code/base"])];
    let fixture = Fixture::new(vec![script("script/test/check")])
        .with_profiles(vec![profile(
            "profile/code/base",
            &["script/test/check"],
            &[],
        )])
        .with_layers(layers.clone());
    (fixture.resolve().expect("deterministic resolution"), layers)
}

fn factory_view() -> FactoryInteropView {
    FactoryInteropView::from_fixture_json(include_str!("fixtures/factory-interop-v1.json"))
        .expect("Factory CR-001 fixture")
}

fn project_binding(view: &FactoryInteropView) -> ProjectBinding {
    let mut binding = ProjectBinding::new(
        view.project_ref().expect("Factory ProjectRef"),
        ProjectConstituentRef::parse("constituent:source").unwrap(),
        ProjectBindingLocator::LocalDirectory {
            path: PathBuf::from("/work/factory"),
        },
    );
    binding.source = Some(view.project_source().unwrap().source);
    binding
}

#[test]
fn extended_resolution_preserves_deterministic_resolver_and_composes_full_v2_field() {
    let (deterministic, layers) = deterministic_fixture();
    let factory = factory_view();
    let mut resources = MemoryResourceIndex::default();
    resources.insert(factory.action_resource().unwrap().record);
    resources.insert(factory.capability_resource().unwrap().record);
    resources.insert(record(
        "factory:agent:generic-builder",
        ResourceKind::Agent,
        SourceState::Available,
        None,
    ));
    resources.insert(record(
        "factory:agency:generic-build",
        ResourceKind::Agency,
        SourceState::Available,
        None,
    ));
    resources.insert(record(
        "host:test-host",
        ResourceKind::Host,
        SourceState::Available,
        None,
    ));
    resources.insert(record(
        "context-source:project-map",
        ResourceKind::ContextSource,
        SourceState::Available,
        None,
    ));
    resources.insert(record(
        "model:gpt-test",
        ResourceKind::Model,
        SourceState::Unresolved,
        Some(ProviderState::Available),
    ));
    resources.insert(record(
        "harness:pi",
        ResourceKind::Harness,
        SourceState::Unavailable {
            reason: "not installed on this host".into(),
        },
        None,
    ));
    resources.insert(record(
        "execution-offer:local-shell",
        ResourceKind::ExecutionOffer,
        SourceState::Unresolved,
        Some(ProviderState::Unresolved),
    ));

    let result = compose_context_resolution(
        &deterministic,
        project_binding(&factory),
        &layers,
        &resources,
        RequestedActors {
            agent: Some(ResourceRef::parse("factory:agent:generic-builder").unwrap()),
            agency: Some(ResourceRef::parse("factory:agency:generic-build").unwrap()),
            host: Some(ResourceRef::parse("host:test-host").unwrap()),
        },
    );

    assert_eq!(result.version, CONTEXT_RESOLUTION_VERSION);
    assert_eq!(result.project_binding.project.as_str(), "factory:project:factory");
    assert_eq!(result.deterministic, deterministic);
    assert_eq!(result.profiles.len(), 1);
    assert_eq!(result.profiles[0].to_string(), "profile/code/base");
    assert_eq!(result.scopes.len(), 1);
    assert_eq!(result.scopes[0].kind, ScopeKind::Project);
    assert!(matches!(result.agent, Some(ReferenceResolution::Resolved { .. })));
    assert!(matches!(result.agency, Some(ReferenceResolution::Resolved { .. })));
    assert!(matches!(result.host, Some(ReferenceResolution::Resolved { .. })));

    assert_eq!(result.actions.len(), 1);
    assert_eq!(
        result.actions[0].resource.descriptor.id.as_str(),
        "factory:action:update"
    );
    assert_eq!(result.capabilities.len(), 1);
    assert_eq!(
        result.capabilities[0].resource.descriptor.id.as_str(),
        "factory:capability:browser"
    );
    assert_eq!(result.context_sources.len(), 1);
    assert_eq!(result.model_candidates.len(), 1);
    assert_eq!(result.harness_candidates.len(), 1);
    assert_eq!(result.execution_offers.len(), 1);
    assert_eq!(
        result.retrieval.context_sources[0].as_str(),
        "context-source:project-map"
    );
    let mut expected_targets = deterministic.context.targets.clone();
    expected_targets.sort();
    expected_targets.dedup();
    assert_eq!(result.projection.targets, expected_targets);
    assert_eq!(result.projection.active_capabilities, ["script/test/check"]);
    assert_eq!(result.warnings, deterministic.warnings);

    let encoded = serde_json::to_string(&result).expect("serialize ContextResolution");
    let round_trip: aikit_core::ContextResolution =
        serde_json::from_str(&encoded).expect("deserialize ContextResolution");
    assert_eq!(round_trip, result);
}

#[test]
fn availability_is_independent_from_eligibility_and_preference() {
    let mut resource = record(
        "model:preferred-but-offline",
        ResourceKind::Model,
        SourceState::Unavailable {
            reason: "registry offline".into(),
        },
        Some(ProviderState::Unavailable {
            reason: "runtime offline".into(),
        }),
    );
    resource.eligibility = Eligibility::Eligible;
    resource.preference = Some(PreferenceIntent {
        source: SourceRef::parse("profile:preference/default").unwrap(),
        rank: 100,
        rationale: Some("preferred model".into()),
    });

    let availability = resource_availability(&resource);
    assert!(matches!(availability, Availability::Unavailable { .. }));
    assert!(resource.eligibility.is_eligible());
    assert_eq!(resource.preference.as_ref().unwrap().rank, 100);
}

#[test]
fn unavailable_resources_remain_visible_and_external_identity_survives_reresolution() {
    let (deterministic, layers) = deterministic_fixture();
    let factory = factory_view();
    let capability = factory.capability_resource().unwrap().record;
    let canonical = capability.descriptor.id.clone();

    let mut unresolved_index = MemoryResourceIndex::default();
    unresolved_index.insert(capability.clone());
    let first = compose_context_resolution(
        &deterministic,
        project_binding(&factory),
        &layers,
        &unresolved_index,
        RequestedActors::default(),
    );
    assert_eq!(first.capabilities[0].resource.descriptor.id, canonical);
    assert!(matches!(
        first.capabilities[0].availability,
        Availability::Unresolved { .. }
    ));

    let mut available = capability;
    available.providers[0].state = ProviderState::Available;
    let mut available_index = MemoryResourceIndex::default();
    available_index.insert(available);
    let second = compose_context_resolution(
        &deterministic,
        project_binding(&factory),
        &layers,
        &available_index,
        RequestedActors::default(),
    );

    assert_eq!(second.capabilities[0].resource.descriptor.id, canonical);
    assert_eq!(first.project_binding.project, second.project_binding.project);
    assert_eq!(first.deterministic, second.deterministic);
    assert_eq!(second.capabilities[0].availability, Availability::Available);
}

#[test]
fn missing_and_wrong_kind_actor_relations_remain_explicit_and_explainable() {
    let (deterministic, layers) = deterministic_fixture();
    let factory = factory_view();
    let mut resources = MemoryResourceIndex::default();
    resources.insert(record(
        "actor:actually-a-model",
        ResourceKind::Model,
        SourceState::Available,
        None,
    ));

    let result = compose_context_resolution(
        &deterministic,
        project_binding(&factory),
        &layers,
        &resources,
        RequestedActors {
            agent: Some(ResourceRef::parse("agent:missing").unwrap()),
            agency: Some(ResourceRef::parse("actor:actually-a-model").unwrap()),
            host: Some(ResourceRef::parse("host:missing").unwrap()),
        },
    );

    assert!(matches!(
        result.agent,
        Some(ReferenceResolution::Missing { .. })
    ));
    assert!(matches!(
        result.agency,
        Some(ReferenceResolution::WrongKind { .. })
    ));
    assert!(matches!(
        result.host,
        Some(ReferenceResolution::Missing { .. })
    ));
    assert!(result.warnings.iter().any(|warning| warning.contains("requested agent")));
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("requested agency")));
    assert!(result.warnings.iter().any(|warning| warning.contains("requested host")));
}
