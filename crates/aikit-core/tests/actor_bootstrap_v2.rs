mod common;

use std::path::PathBuf;

use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::resource::{
    MemoryResourceIndex, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor,
    ResourceKind, ResourceRecord, ResourceRef, ResourceSource, SourceRef, SourceRevision,
    SourceState,
};
use aikit_core::{
    compose_context_resolution, project_actor_bootstrap, resolve_harness_composition,
    ActivationScope, ActivationScopeKind, ActorBootstrapRequest, BootstrapReference,
    ComponentDescriptor, ComponentSelection, CompositionActivationMode, CompositionCatalog,
    HarnessComposition, HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind,
    RequestedActors, ResolutionScope, ScopeKind, ACTOR_BOOTSTRAP_VERSION,
};

use common::{layer_using, profile, script, Fixture};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn record(id: &str, kind: ResourceKind, available: bool) -> ResourceRecord {
    let mut descriptor = ResourceDescriptor::new(r(id), kind, id, format!("test {id}"));
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse(&format!("source:{id}")).unwrap(),
        authority: None,
        revision: Some(SourceRevision::parse("rev:1").unwrap()),
        locator: None,
        state: if available {
            SourceState::Available
        } else {
            SourceState::Unresolved
        },
    });
    let mut record = ResourceRecord::new(descriptor);
    record.providers.push(ProviderOffer {
        provider: ProviderRef::parse(&format!("provider:{id}")).unwrap(),
        locator: None,
        state: if available {
            ProviderState::Available
        } else {
            ProviderState::Unresolved
        },
    });
    record
}

fn context_resolution() -> aikit_core::ContextResolution {
    let layers = vec![layer_using(ScopeKind::Project, &["profile/code/base"])];
    let fixture = Fixture::new(vec![script("script/test/check")])
        .with_profiles(vec![profile(
            "profile/code/base",
            &["script/test/check"],
            &[],
        )])
        .with_layers(layers.clone());
    let deterministic = fixture.resolve().unwrap();

    let mut resources = MemoryResourceIndex::default();
    resources.insert(record("agent:test", ResourceKind::Agent, true));
    resources.insert(record("agency:test", ResourceKind::Agency, true));
    resources.insert(record("host:test", ResourceKind::Host, true));
    resources.insert(record("model:test", ResourceKind::Model, true));
    resources.insert(record("harness:test", ResourceKind::Harness, true));
    resources.insert(record(
        "capability:test/browser",
        ResourceKind::Capability,
        true,
    ));
    resources.insert(record("action:test/open", ResourceKind::Action, true));
    resources.insert(record(
        "context-source:test/project-map",
        ResourceKind::ContextSource,
        false,
    ));

    let project = ProjectBinding::new(
        ProjectRef::parse("project/test").unwrap(),
        ProjectConstituentRef::parse("constituent/source").unwrap(),
        ProjectBindingLocator::LocalDirectory {
            path: PathBuf::from("/work/test"),
        },
    );

    compose_context_resolution(
        &deterministic,
        project,
        &layers,
        &resources,
        RequestedActors {
            agent: Some(r("agent:test")),
            agency: Some(r("agency:test")),
            host: Some(r("host:test")),
        },
    )
}

fn selection(component: &str) -> ComponentSelection {
    ComponentSelection {
        component: r(component),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "project profile"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session/a"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession)
            .with_reference("session/a"),
        activation_mode: CompositionActivationMode::NextSession,
    }
}

fn body(components: &[&str]) -> HarnessComposition {
    let mut catalog = CompositionCatalog::default();
    for component in components {
        let mut descriptor = ComponentDescriptor::new(r(component));
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

fn bootstrap<'a>(
    resolution: &aikit_core::ContextResolution,
    body: Option<&'a HarnessComposition>,
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
    let resolution = context_resolution();
    let body = body(&["component/runtime"]);
    let bootstrap = bootstrap(&resolution, Some(&body), "session/a");

    assert_eq!(bootstrap.version, ACTOR_BOOTSTRAP_VERSION);
    assert_eq!(bootstrap.project.project.as_str(), "project/test");
    assert_eq!(bootstrap.run, Some(r("run/client-supplied")));
    assert_eq!(bootstrap.agent_session.as_deref(), Some("session/a"));

    match bootstrap.agent.as_ref().unwrap() {
        BootstrapReference::Resolved {
            resource,
            sources,
            providers,
            ..
        } => {
            assert_eq!(resource, &r("agent:test"));
            assert_eq!(sources.len(), 1);
            assert_eq!(providers.len(), 1);
        }
        other => panic!("expected resolved agent, got {other:?}"),
    }
    assert_eq!(bootstrap.capabilities.total, 1);
    assert_eq!(bootstrap.actions.total, 1);
    assert_eq!(bootstrap.context_sources.total, 1);
    assert_eq!(bootstrap.context_sources.unresolved, 1);

    let pointer = bootstrap.runtime_body.as_ref().unwrap();
    assert_eq!(pointer.harness, r("harness:test"));
    assert_eq!(pointer.component_count, 1);
    assert_eq!(pointer.fingerprint, body.fingerprint);

    let encoded = serde_json::to_string(&bootstrap).unwrap();
    assert!(!encoded.contains("component/runtime"));
    assert!(!encoded.contains("component_bindings"));
}

#[test]
fn replacing_session_does_not_rewrite_project_run_or_actor_identity() {
    let resolution = context_resolution();
    let first = bootstrap(&resolution, None, "session/a");
    let second = bootstrap(&resolution, None, "session/b");

    assert_eq!(first.project, second.project);
    assert_eq!(first.run, second.run);
    assert_eq!(first.agent, second.agent);
    assert_eq!(first.agency, second.agency);
    assert_eq!(first.harness, second.harness);
    assert_eq!(first.model, second.model);
    assert_ne!(first.agent_session, second.agent_session);
}

#[test]
fn changing_runtime_body_changes_only_the_body_pointer_not_actor_identity() {
    let resolution = context_resolution();
    let body_a = body(&["component/runtime"]);
    let body_b = body(&["component/runtime", "component/inspector"]);
    let first = bootstrap(&resolution, Some(&body_a), "session/a");
    let second = bootstrap(&resolution, Some(&body_b), "session/a");

    assert_eq!(first.project, second.project);
    assert_eq!(first.run, second.run);
    assert_eq!(first.agent, second.agent);
    assert_eq!(first.agency, second.agency);
    assert_eq!(first.harness, second.harness);
    assert_eq!(first.model, second.model);
    assert_ne!(
        first.runtime_body.as_ref().unwrap().fingerprint,
        second.runtime_body.as_ref().unwrap().fingerprint
    );
}

#[test]
fn bootstrap_rejects_a_runtime_body_bound_to_a_different_harness() {
    let resolution = context_resolution();
    let mut body = body(&["component/runtime"]);
    body.harness = r("harness:other");

    let error = project_actor_bootstrap(
        &resolution,
        ActorBootstrapRequest {
            run: Some(r("run/client-supplied")),
            selected_harness: Some(r("harness:test")),
            selected_model: Some(r("model:test")),
            agent_session: Some("session/a".into()),
            runtime_body: Some(&body),
        },
    )
    .unwrap_err();

    assert_eq!(error.code(), "bootstrap.runtime_body_identity_mismatch");
    assert_eq!(error.details().get("role").map(String::as_str), Some("harness"));
}
