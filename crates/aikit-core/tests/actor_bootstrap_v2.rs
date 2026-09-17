mod common;

use std::path::PathBuf;

use aikit_core::actor_bootstrap::{
    project_actor_bootstrap, ActorBootstrapRequest, BootstrapReference, MissingCause,
};
use aikit_core::composition::{
    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentDescriptor,
    ComponentSelection, CompositionActivationMode, CompositionCatalog, HarnessComposition,
    HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
};
use aikit_core::context_resolution::{
    compose_context_resolution, ContextResolution, HarnessDetectionGround, RequestedActors,
};
use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::resource::{
    MemoryResourceIndex, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
};
use aikit_core::scope::ScopeKind;
use aikit_core::session_space::SessionSpaceRef;

use common::{script, Fixture};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn resource(raw: &str, kind: ResourceKind) -> ResourceRecord {
    ResourceRecord::new(ResourceDescriptor::new(r(raw), kind, raw, raw))
}

fn resolution() -> ContextResolution {
    let deterministic = Fixture::new(vec![script("script/test/check")])
        .resolve()
        .expect("deterministic resolution");
    let binding = ProjectBinding::new(
        ProjectRef::parse("project/test").unwrap(),
        ProjectConstituentRef::parse("constituent:working-tree").unwrap(),
        ProjectBindingLocator::LocalDirectory {
            path: PathBuf::from("/work/test"),
        },
    );
    let mut resources = MemoryResourceIndex::default();
    for (raw, kind) in [
        ("agent:test", ResourceKind::Agent),
        ("agency:test", ResourceKind::Agency),
        ("harness:test", ResourceKind::Harness),
        ("model:test", ResourceKind::Model),
        ("host:test-host", ResourceKind::Host),
    ] {
        resources.insert(resource(raw, kind));
    }
    compose_context_resolution(
        &deterministic,
        binding,
        &[],
        &resources,
        RequestedActors {
            agent: Some(r("agent:test")),
            agency: Some(r("agency:test")),
            host: Some(r("host:test-host")),
        },
    )
}

fn selection(component: &str, session: &str) -> ComponentSelection {
    ComponentSelection {
        component: r(component),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "project profile"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference(session),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession).with_reference(session),
        activation_mode: CompositionActivationMode::LiveMounted,
    }
}

fn body(components: &[&str], session: &str) -> HarnessComposition {
    let mut catalog = CompositionCatalog::default();
    for component in components {
        let mut descriptor = ComponentDescriptor::new(r(component));
        descriptor
            .activation_modes
            .insert(CompositionActivationMode::LiveMounted);
        catalog.insert_component(descriptor);
    }
    resolve_harness_composition(
        &catalog,
        HarnessCompositionRequest {
            harness: r("harness:test"),
            project: Some(r("project/test")),
            agent: Some(r("agent:test")),
            agency: Some(r("agency:test")),
            session: Some(session.into()),
            model: Some(r("model:test")),
            selections: components
                .iter()
                .map(|component| selection(component, session))
                .collect(),
            target_revision: Some("target-r1".into()),
            generation: Some("generation/1".into()),
        },
    )
    .unwrap()
}

fn bootstrap(
    resolution: &ContextResolution,
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
            session_space: Some(SessionSpaceRef::parse("session-space/test").unwrap()),
            runtime_body: body,
        },
    )
    .unwrap()
}

#[test]
fn bootstrap_is_thin_but_preserves_actor_provenance_and_runtime_body_pointer() {
    let resolution = resolution();
    let body = body(&["component:a"], "session/a");
    let bootstrap = bootstrap(&resolution, Some(&body), "session/a");

    assert_eq!(bootstrap.project.project.as_str(), "project/test");
    assert_eq!(
        bootstrap.agent.as_ref().unwrap().resource(),
        &r("agent:test")
    );
    assert_eq!(
        bootstrap.agency.as_ref().unwrap().resource(),
        &r("agency:test")
    );
    assert_eq!(
        bootstrap.harness.as_ref().unwrap().resource(),
        &r("harness:test")
    );
    assert_eq!(
        bootstrap.model.as_ref().unwrap().resource(),
        &r("model:test")
    );
    assert_eq!(
        bootstrap.host.as_ref().unwrap().resource(),
        &r("host:test-host")
    );
    assert_eq!(bootstrap.agent_session.as_deref(), Some("session/a"));
    assert_eq!(
        bootstrap.session_space.as_ref().unwrap().to_string(),
        "session-space/test"
    );
    assert_eq!(
        bootstrap.runtime_body.as_ref().unwrap().fingerprint,
        body.fingerprint
    );
    assert_eq!(
        bootstrap
            .runtime_body
            .as_ref()
            .unwrap()
            .generation
            .as_deref(),
        Some("generation/1")
    );
    assert_eq!(
        bootstrap
            .runtime_body
            .as_ref()
            .unwrap()
            .target_revision
            .as_deref(),
        Some("target-r1")
    );
    assert_eq!(bootstrap.run, Some(r("run/client-supplied")));
}

#[test]
fn actor_identity_survives_session_and_body_replacement() {
    let resolution = resolution();
    let first_body = body(&["component:a"], "session/a");
    let second_body = body(&["component:a", "component:b"], "session/b");
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
    assert_eq!(
        bootstrap.harness.as_ref().unwrap().resource(),
        &r("harness:test")
    );
    assert_eq!(bootstrap.agent_session.as_deref(), Some("session/thin"));
}

fn missing_bootstrap(
    resolution: &ContextResolution,
    harness_ref: &str,
) -> aikit_core::ActorBootstrap {
    project_actor_bootstrap(
        resolution,
        ActorBootstrapRequest {
            selected_harness: Some(r(harness_ref)),
            ..ActorBootstrapRequest::default()
        },
    )
    .unwrap()
}

fn observed_ground() -> HarnessDetectionGround {
    let mut states = std::collections::BTreeMap::new();
    states.insert("ghost".to_string(), "not-installed".to_string());
    states.insert("shrouded".to_string(), "unavailable".to_string());
    states.insert("present".to_string(), "detected".to_string());
    let mut reasons = std::collections::BTreeMap::new();
    reasons.insert("shrouded".to_string(), "all probes failed".to_string());
    HarnessDetectionGround::Observed {
        detection_ref: "actuation.detection/2026-09-05T00:00:00Z".to_string(),
        catalog_revision: 1,
        states,
        reasons,
    }
}

fn missing_cause_of(bootstrap: &aikit_core::ActorBootstrap) -> MissingCause {
    match bootstrap.harness.as_ref().unwrap() {
        BootstrapReference::Missing { cause, .. } => cause.clone(),
        other => panic!("expected Missing, got {other:?}"),
    }
}

#[test]
fn missing_harness_reasons_from_detection_ground() {
    let mut resolution = resolution();
    resolution.harness_detection = Some(observed_ground());

    let not_installed = missing_bootstrap(&resolution, "harness/ghost");
    assert_eq!(
        missing_cause_of(&not_installed),
        MissingCause::NotInstalled {
            detection_ref: "actuation.detection/2026-09-05T00:00:00Z".to_string()
        }
    );

    let unprovable = missing_bootstrap(&resolution, "harness/shrouded");
    assert_eq!(
        missing_cause_of(&unprovable),
        MissingCause::DetectionUnavailable {
            reason: "all probes failed".to_string()
        }
    );

    let intake_gap = missing_bootstrap(&resolution, "harness/present");
    assert!(matches!(
        missing_cause_of(&intake_gap),
        MissingCause::DetectedButUnresolved { .. }
    ));

    let unknown = missing_bootstrap(&resolution, "harness/nobody");
    assert!(matches!(
        missing_cause_of(&unknown),
        MissingCause::UnknownToDetection { .. }
    ));
}

#[test]
fn failed_detection_run_is_disclosed_not_absence() {
    let mut resolution = resolution();
    resolution.harness_detection = Some(HarnessDetectionGround::Unavailable {
        reason: "could not run actuation: no such file".to_string(),
    });
    assert_eq!(
        missing_cause_of(&missing_bootstrap(&resolution, "harness/ghost")),
        MissingCause::DetectionUnavailable {
            reason: "could not run actuation: no such file".to_string()
        }
    );
}

#[test]
fn no_detection_ground_stays_unproven_never_absence() {
    let resolution = resolution();
    assert_eq!(resolution.harness_detection, None);
    assert!(matches!(
        missing_cause_of(&missing_bootstrap(&resolution, "harness/ghost")),
        MissingCause::Unproven
    ));

    // A model reference never borrows harness detection ground.
    let mut with_ground = resolution.clone();
    with_ground.harness_detection = Some(observed_ground());
    let bootstrap = project_actor_bootstrap(
        &with_ground,
        ActorBootstrapRequest {
            selected_model: Some(r("model/ghost")),
            ..ActorBootstrapRequest::default()
        },
    )
    .unwrap();
    let BootstrapReference::Missing { cause, .. } = bootstrap.model.as_ref().unwrap() else {
        panic!("expected Missing");
    };
    assert!(matches!(cause, MissingCause::Unproven));
}
