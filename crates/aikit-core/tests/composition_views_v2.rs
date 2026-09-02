use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::{
    diff_harness_compositions, explain_composed_component, resolve_harness_composition,
    ActivationScope, ActivationScopeKind, ComponentContribution, ComponentDescriptor,
    ComponentRequirement, ComponentSelection, CompositionActivationMode, CompositionCatalog,
    ContractProvider, ContributionKind, HarnessCompositionRequest, LifetimeOwner,
    LifetimeOwnerKind, RequirementResolution, ResolutionScope, RetractionMode, ScopeKind,
    SurfaceDescriptor, SurfaceKind,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn selection(component: &str) -> ComponentSelection {
    ComponentSelection {
        component: r(component),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "project profile"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session/alpha"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::ComponentContext)
            .with_reference(format!("context/{component}")),
        activation_mode: CompositionActivationMode::LiveMounted,
    }
}

fn request(selections: Vec<ComponentSelection>) -> HarnessCompositionRequest {
    HarnessCompositionRequest {
        harness: r("harness/deepseek"),
        project: Some(r("project/epilogos")),
        agent: Some(r("agent/parashakti")),
        agency: Some(r("agency/parashakti/design")),
        session: Some("session/alpha".into()),
        model: Some(r("model/deepseek-v3")),
        selections,
        target_revision: Some("runtime-r1".into()),
        generation: None,
    }
}

fn component(id: &str) -> ComponentDescriptor {
    let mut component = ComponentDescriptor::new(r(id));
    component
        .activation_modes
        .insert(CompositionActivationMode::LiveMounted);
    component
}

#[test]
fn explain_uses_resolver_owned_provider_scope_lifetime_and_surface_evidence() {
    let mut catalog = CompositionCatalog::default();
    let component_ref = r("component/agent-loop");
    let contract_ref = r("contract/session-store");
    let surface_ref = r("surface/trajectory");

    catalog.insert_surface(SurfaceDescriptor {
        resource: surface_ref.clone(),
        kind: SurfaceKind::Trajectory,
        target_native_id: Some("trajectory-pane".into()),
        owner_component: Some(component_ref.clone()),
    });

    let mut descriptor = component("component/agent-loop");
    descriptor
        .requirements
        .push(ComponentRequirement::required(contract_ref.clone()).reactive());
    descriptor.supported_surfaces.push(surface_ref.clone());
    descriptor.contributions.push(ComponentContribution {
        id: r("contribution/trajectory"),
        component: component_ref.clone(),
        kind: ContributionKind::Trajectory,
        target_contract: None,
        exposed_ref: Some(r("knowledge-node/session-trajectory")),
        exposed_kind: Some(ResourceKind::KnowledgeNode),
        surface: Some(surface_ref.clone()),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session/alpha"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::ComponentContext)
            .with_reference("cordis-context/agent-loop"),
        activation_mode: CompositionActivationMode::LiveMounted,
        retraction_mode: RetractionMode::Live,
        provenance: vec!["target session events".into()],
    });
    catalog.insert_component(descriptor);
    catalog.add_provider(
        ContractProvider::available(contract_ref.clone(), r("provider/session-log"))
            .with_priority(10),
    );

    let composition =
        resolve_harness_composition(&catalog, request(vec![selection("component/agent-loop")]))
            .unwrap();
    let explanation = explain_composed_component(&catalog, &composition, &component_ref).unwrap();

    assert_eq!(explanation.component, component_ref);
    assert_eq!(explanation.resolution_scope.scope, ScopeKind::Project);
    assert_eq!(
        explanation.activation_scope.kind,
        ActivationScopeKind::AgentSession
    );
    assert_eq!(
        explanation.lifetime_owner.kind,
        LifetimeOwnerKind::ComponentContext
    );
    assert_eq!(explanation.requirements.len(), 1);
    assert!(explanation.requirements[0].required);
    assert!(explanation.requirements[0].reactive);
    assert!(matches!(
        &explanation.requirements[0].resolution,
        RequirementResolution::Provider { provider, .. }
            if provider == &r("provider/session-log")
    ));
    assert_eq!(explanation.contributions.len(), 1);
    assert_eq!(explanation.surfaces.len(), 1);
    assert_eq!(explanation.surfaces[0].resource, surface_ref);
}

#[test]
fn history_diff_reports_mount_retract_rebind_and_surface_changes() {
    let contract = r("contract/session-store");

    let mut before_catalog = CompositionCatalog::default();
    let mut base = component("component/base");
    base.requirements
        .push(ComponentRequirement::required(contract.clone()));
    before_catalog.insert_component(base.clone());
    before_catalog.add_provider(
        ContractProvider::available(contract.clone(), r("provider/session-v1")).with_priority(10),
    );
    let before =
        resolve_harness_composition(&before_catalog, request(vec![selection("component/base")]))
            .unwrap();

    let mut after_catalog = CompositionCatalog::default();
    after_catalog.insert_surface(SurfaceDescriptor {
        resource: r("surface/inspector"),
        kind: SurfaceKind::Tui,
        target_native_id: Some("inspector".into()),
        owner_component: Some(r("component/inspector")),
    });
    after_catalog.insert_component(base);
    let mut inspector = component("component/inspector");
    inspector.contributions.push(ComponentContribution {
        id: r("contribution/inspector"),
        component: r("component/inspector"),
        kind: ContributionKind::UiNode,
        target_contract: None,
        exposed_ref: None,
        exposed_kind: None,
        surface: Some(r("surface/inspector")),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
        activation_mode: CompositionActivationMode::LiveMounted,
        retraction_mode: RetractionMode::Live,
        provenance: vec!["runtime inspector".into()],
    });
    after_catalog.insert_component(inspector);
    after_catalog.add_provider(
        ContractProvider::available(contract.clone(), r("provider/session-v2")).with_priority(20),
    );
    let after = resolve_harness_composition(
        &after_catalog,
        request(vec![
            selection("component/base"),
            selection("component/inspector"),
        ]),
    )
    .unwrap();

    let diff = diff_harness_compositions(&before, &after).unwrap();
    assert_eq!(diff.mounted_components, vec![r("component/inspector")]);
    assert!(diff.retracted_components.is_empty());
    assert_eq!(diff.rebound_contracts.len(), 1);
    assert_eq!(
        diff.rebound_contracts[0].before_provider,
        r("provider/session-v1")
    );
    assert_eq!(
        diff.rebound_contracts[0].after_provider,
        r("provider/session-v2")
    );
    assert_eq!(diff.added_contributions, vec![r("contribution/inspector")]);
    assert_eq!(diff.added_surfaces, vec![r("surface/inspector")]);
    assert_ne!(diff.before_fingerprint, diff.after_fingerprint);
}

#[test]
fn body_history_refuses_to_compare_different_actor_anchors() {
    let mut catalog = CompositionCatalog::default();
    catalog.insert_component(component("component/base"));
    let before =
        resolve_harness_composition(&catalog, request(vec![selection("component/base")])).unwrap();

    let mut changed_request = request(vec![selection("component/base")]);
    changed_request.agent = Some(r("agent/other"));
    let after = resolve_harness_composition(&catalog, changed_request).unwrap();

    let error = diff_harness_compositions(&before, &after).unwrap_err();
    assert_eq!(error.code(), "composition.diff_identity_mismatch");
}
