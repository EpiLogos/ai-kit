use aikit_core::actor_bootstrap::{project_actor_bootstrap, ActorBootstrapRequest};
use aikit_core::composition::{
    resolve_harness_composition, ComponentCatalog, ComponentContribution, ComponentDescriptor,
    ComponentKind, ComponentSelection, CompositionActivationMode, ContractBinding,
    HarnessComposition, HarnessCompositionRequest,
};
use aikit_core::context_resolution::{
    compose_context_resolution, ContextResolution, RequestedActors,
};
use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
use aikit_core::resource::{
    ResourceDescriptor, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef,
};
use aikit_core::resolve::ResolvedView;
use aikit_core::scope::ScopeLayer;
use aikit_core::ContextDescriptor;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn resource(raw: &str, kind: ResourceKind) -> ResourceRecord {
    ResourceRecord::new(ResourceDescriptor::new(r(raw), kind, raw, raw))
}

fn resolution() -> ContextResolution {
    let descriptor = ContextDescriptor::for_project("/work/test");
    let project = ProjectRef::parse("project/test").unwrap();
    let binding = ProjectBinding::from_legacy_context(
        project,
        ProjectConstituentRef::parse("source:working-tree").unwrap(),
        &descriptor,
    )
    .unwrap();
    let mut resources = ResourceIndex::default();
    for (raw, kind) in [
        ("agent:test", ResourceKind::Agent),
        ("agency:test", ResourceKind::Agency),
        ("harness:test", ResourceKind::Harness),
        ("model:test", ResourceKind::Model),
        ("host:test-host", ResourceKind::Host),
    ] {
        resources.insert(resource(raw, kind));
    }
    let mut view = ResolvedView::empty(descriptor);
    view.hash = "resolution-test".into();
    compose_context_resolution(
        &view,
        binding,
        &[] as &[ScopeLayer],
        &resources,
        RequestedActors {
            agent: Some(r("agent:test")),
            agency: Some(r("agency:test")),
            harness: Some(r("harness:test")),
            model: Some(r("model:test")),
            host: Some(r("host:test-host")),
        },
    )
}

fn selection(resource: &str) -> ComponentSelection {
    ComponentSelection {
        resource: r(resource),
        provider: None,
        activation: CompositionActivationMode::NextSession,
        binding: None,
    }
}

fn body(components: &[&str]) -> HarnessComposition {
    let mut catalog = ComponentCatalog::default();
    catalog.insert_contract(aikit_core::composition::ContractDescriptor {
        contract: r("contract:prompt-context"),
        label: "prompt-context".into(),
        description: "test contract".into(),
        provider_kinds: Default::default(),
    });
    for component in components {
        let mut descriptor = ComponentDescriptor::new(
            r(component),
            ComponentKind::Other("test".into()),
            *component,
        );
        descriptor.contributions.push(ComponentContribution {
            contract: r("contract:prompt-context"),
            value: serde_json::json!({"component": component}),
            surfaces: Vec::new(),
        });
        descriptor.bindings.push(ContractBinding {
            contract: r("contract:prompt-context"),
            provider: r(component),
            surface: None,
            metadata: Default::default(),
        });
        descriptor
            .activation_modes
            .insert(CompositionActivationMode::NextSession);
        catalog.insert_component(descriptor);
    }
    resolve_harness_composition(
        &catalog,
        HarnessCompositionRequest {
            harness: r("harness:test"),
            project: Some(r("project/test")),
            agent: Some(r("agent:test")),
            agency: Some(r("agency:test")),
            session: Some("session/a".into()),
            model: Some(r("model:test")),
            selections: components.iter().map(|component| selection(component)).collect(),
            target_revision: Some("target-r1".into()),
            generation: Some("generation/1".into()),
        },
    )
    .unwrap()
}

fn bootstrap(
    resolution: &aikit_core::ContextResolution,
    body: Option<&HarnessComposition>,
    session: &str,
) -> aikit_core::ActorBootstrap {
    project_actor_bootstrap(
        resolution,
        ActorBootstrapRequest {
            run: Some(r("run/client-supplied")),
            selected_harness: Some(r("harness:test")),
            selected_model: Some(r("model:test")),
            agent_session: Some(session.into()),
            runtime_body: body,
        },
    )
    .unwrap()
}

#[test]
fn bootstrap_is_thin_but_preserves_actor_provenance_and_runtime_body_pointer() {
    let resolution = resolution();
    let body = body(&["component:a"]);
    let bootstrap = bootstrap(&resolution, Some(&body), "session/a");

    assert_eq!(bootstrap.project, r("project/test"));
    assert_eq!(bootstrap.agent.as_ref().unwrap().resource(), &r("agent:test"));
    assert_eq!(bootstrap.agency.as_ref().unwrap().resource(), &r("agency:test"));
    assert_eq!(bootstrap.harness.as_ref().unwrap().resource(), &r("harness:test"));
    assert_eq!(bootstrap.model.as_ref().unwrap().resource(), &r("model:test"));
    assert_eq!(bootstrap.host.as_ref().unwrap().resource(), &r("host:test-host"));
    assert_eq!(bootstrap.agent_session.as_deref(), Some("session/a"));
    assert_eq!(bootstrap.runtime_body.as_ref().unwrap().fingerprint, body.fingerprint);
    assert_eq!(bootstrap.runtime_body.as_ref().unwrap().generation.as_deref(), Some("generation/1"));
    assert_eq!(bootstrap.runtime_body.as_ref().unwrap().target_revision.as_deref(), Some("target-r1"));
    assert!(bootstrap.context.resolution_hash.contains("resolution-test"));
    assert_eq!(bootstrap.run, Some(r("run/client-supplied")));
}

#[test]
fn actor_identity_survives_session_and_body_replacement() {
    let resolution = resolution();
    let first_body = body(&["component:a"]);
    let second_body = body(&["component:a", "component:b"]);
    let first = bootstrap(&resolution, Some(&first_body), "session/a");
    let second = bootstrap(&resolution, Some(&second_body), "session/b");

    assert_eq!(first.project, second.project);
    assert_eq!(first.agent, second.agent);
    assert_eq!(first.agency, second.agency);
    assert_ne!(first.agent_session, second.agent_session);
    assert_ne!(
        first.runtime_body.as_ref().unwrap().fingerprint,
        second.runtime_body.as_ref().unwrap().fingerprint
    );
}

#[test]
fn bootstrap_remains_valid_without_a_composition_capable_body() {
    let resolution = resolution();
    let bootstrap = bootstrap(&resolution, None, "session/thin");

    assert!(bootstrap.runtime_body.is_none());
    assert_eq!(bootstrap.harness.as_ref().unwrap().resource(), &r("harness:test"));
    assert_eq!(bootstrap.agent_session.as_deref(), Some("session/thin"));
}
