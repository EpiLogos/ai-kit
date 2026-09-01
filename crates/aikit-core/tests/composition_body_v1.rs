use aikit_core::resource::ResourceRef;
use aikit_core::scope::ScopeKind;
use aikit_core::{
    resolve_composition_body, resolve_harness_composition, ActivationScope, ActivationScopeKind,
    ComponentDescriptor, ComponentSelection, CompositionActivationMode, CompositionBodyRequest,
    CompositionCatalog, HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind,
    ResolutionScope, SurfaceDescriptor, SurfaceKind, COMPOSITION_BODY_VERSION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn fixture() -> (CompositionCatalog, ComponentSelection) {
    let component = r("component/reference-world/shell");
    let surface = r("surface/reference-world/ambient");
    let mut catalog = CompositionCatalog::default();
    catalog.insert_surface(SurfaceDescriptor {
        resource: surface.clone(),
        kind: SurfaceKind::Tui,
        target_native_id: None,
        owner_component: Some(component.clone()),
    });
    let mut descriptor = ComponentDescriptor::new(component.clone());
    descriptor.supported_surfaces.push(surface);
    catalog.insert_component(descriptor);
    let selection = ComponentSelection {
        component,
        resolution_scope: ResolutionScope::new(ScopeKind::Session, "reference-world fixture"),
        activation_scope: ActivationScope::new(ActivationScopeKind::Host),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation),
        activation_mode: CompositionActivationMode::Generated,
    };
    (catalog, selection)
}

#[test]
fn scope_neutral_body_does_not_require_harness_or_actor_identity() {
    let (catalog, selection) = fixture();
    let body = resolve_composition_body(
        &catalog,
        CompositionBodyRequest {
            model: None,
            selections: vec![selection],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();

    assert_eq!(body.version, COMPOSITION_BODY_VERSION);
    assert_eq!(body.component_bindings.len(), 1);
    assert_eq!(body.surfaces.len(), 1);
    assert!(!body.fingerprint.is_empty());
}

#[test]
fn harness_composition_wraps_the_same_body_without_defining_body_identity() {
    let (catalog, selection) = fixture();
    let body = resolve_composition_body(
        &catalog,
        CompositionBodyRequest {
            model: None,
            selections: vec![selection.clone()],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();

    let harness_a = resolve_harness_composition(
        &catalog,
        HarnessCompositionRequest {
            harness: r("harness/a"),
            project: Some(r("project/reference")),
            agent: None,
            agency: None,
            session: Some("session-a".into()),
            model: None,
            selections: vec![selection.clone()],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();
    let harness_b = resolve_harness_composition(
        &catalog,
        HarnessCompositionRequest {
            harness: r("harness/b"),
            project: Some(r("project/reference")),
            agent: None,
            agency: None,
            session: Some("session-b".into()),
            model: None,
            selections: vec![selection],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();

    assert_eq!(harness_a.component_bindings, body.component_bindings);
    assert_eq!(harness_b.component_bindings, body.component_bindings);
    assert_eq!(harness_a.surfaces, body.surfaces);
    assert_eq!(harness_b.surfaces, body.surfaces);
    assert_ne!(harness_a.fingerprint, harness_b.fingerprint);

    let body_again = resolve_composition_body(
        &catalog,
        CompositionBodyRequest {
            model: None,
            selections: vec![ComponentSelection {
                component: harness_a.component_bindings[0].component.clone(),
                resolution_scope: harness_a.component_bindings[0].resolution_scope.clone(),
                activation_scope: harness_a.component_bindings[0].activation_scope.clone(),
                lifetime_owner: harness_a.component_bindings[0].lifetime_owner.clone(),
                activation_mode: harness_a.component_bindings[0].activation_mode,
            }],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();
    assert_eq!(body.fingerprint, body_again.fingerprint);
}
