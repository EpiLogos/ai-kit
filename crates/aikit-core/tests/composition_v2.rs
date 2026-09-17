use std::collections::BTreeSet;

use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::{
    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentContribution,
    ComponentDescriptor, ComponentRequirement, ComponentSelection, CompositionActivationMode,
    CompositionCatalog, ContractProvider, ContributionKind, HarnessCompositionRequest,
    LifetimeOwner, LifetimeOwnerKind, RequirementStrength, ResolutionScope, RetractionMode,
    ScopeKind, SurfaceDescriptor, SurfaceKind,
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
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession)
            .with_reference("session/alpha"),
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
        target_revision: Some("dsh-2026-08-15".into()),
        generation: None,
    }
}

fn live_component(id: &str) -> ComponentDescriptor {
    let mut component = ComponentDescriptor::new(r(id));
    component
        .activation_modes
        .insert(CompositionActivationMode::LiveMounted);
    component
}

#[test]
fn one_action_can_project_through_multiple_surfaces_without_identity_multiplication() {
    let mut catalog = CompositionCatalog::default();
    let component_ref = r("component/actions");
    let action_ref = r("action/project/open");
    let tui_surface = r("surface/tui/actions");
    let tool_surface = r("surface/tool/actions");

    catalog.insert_surface(SurfaceDescriptor {
        resource: tui_surface.clone(),
        kind: SurfaceKind::Tui,
        target_native_id: Some("ui.actions".into()),
        owner_component: Some(component_ref.clone()),
    });
    catalog.insert_surface(SurfaceDescriptor {
        resource: tool_surface.clone(),
        kind: SurfaceKind::AgentTool,
        target_native_id: Some("tools.actions".into()),
        owner_component: Some(component_ref.clone()),
    });

    let mut component = live_component("component/actions");
    component.supported_surfaces = vec![tui_surface.clone(), tool_surface.clone()];
    for (id, surface) in [
        ("contribution/action-tui", tui_surface.clone()),
        ("contribution/action-tool", tool_surface.clone()),
    ] {
        component.contributions.push(ComponentContribution {
            id: r(id),
            component: component_ref.clone(),
            kind: ContributionKind::ActionProjection,
            target_contract: None,
            exposed_ref: Some(action_ref.clone()),
            exposed_kind: Some(ResourceKind::Action),
            surface: Some(surface),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
                .with_reference("session/alpha"),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::ComponentContext),
            activation_mode: CompositionActivationMode::LiveMounted,
            retraction_mode: RetractionMode::Live,
            provenance: vec!["project Action catalog".into()],
        });
    }
    catalog.insert_component(component);

    let composition =
        resolve_harness_composition(&catalog, request(vec![selection("component/actions")]))
            .unwrap();

    assert_eq!(composition.projections.len(), 2);
    assert!(composition
        .projections
        .iter()
        .all(|projection| projection.canonical_ref == action_ref));
    assert!(composition
        .projections
        .iter()
        .all(|projection| projection.canonical_kind == ResourceKind::Action));
    assert_eq!(
        composition
            .projections
            .iter()
            .map(|projection| projection.surface.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([tui_surface, tool_surface])
    );
}

#[test]
fn non_action_reading_projects_to_a_surface_without_becoming_an_action() {
    let mut catalog = CompositionCatalog::default();
    let component_ref = r("component/trajectory");
    let surface_ref = r("surface/trajectory");
    let reading_ref = r("knowledge-node/session-trajectory");

    catalog.insert_surface(SurfaceDescriptor {
        resource: surface_ref.clone(),
        kind: SurfaceKind::Trajectory,
        target_native_id: Some("trajectory".into()),
        owner_component: Some(component_ref.clone()),
    });
    let mut component = live_component("component/trajectory");
    component.contributions.push(ComponentContribution {
        id: r("contribution/trajectory-reading"),
        component: component_ref,
        kind: ContributionKind::Trajectory,
        target_contract: None,
        exposed_ref: Some(reading_ref.clone()),
        exposed_kind: Some(ResourceKind::KnowledgeNode),
        surface: Some(surface_ref),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
        activation_mode: CompositionActivationMode::LiveMounted,
        retraction_mode: RetractionMode::Live,
        provenance: vec!["target session events".into()],
    });
    catalog.insert_component(component);

    let composition =
        resolve_harness_composition(&catalog, request(vec![selection("component/trajectory")]))
            .unwrap();

    assert_eq!(composition.projections.len(), 1);
    assert_eq!(composition.projections[0].canonical_ref, reading_ref);
    assert_eq!(
        composition.projections[0].canonical_kind,
        ResourceKind::KnowledgeNode
    );
}

#[test]
fn required_and_optional_requirements_bind_or_degrade_explicitly() {
    let mut catalog = CompositionCatalog::default();
    let filesystem = r("contract/filesystem");
    let telemetry = r("contract/telemetry");
    let mut consumer = live_component("component/agent-loop");
    consumer
        .requirements
        .push(ComponentRequirement::required(filesystem.clone()).reactive());
    consumer
        .requirements
        .push(ComponentRequirement::optional(telemetry.clone()));
    catalog.insert_component(consumer);
    catalog.add_provider(
        ContractProvider::available(filesystem.clone(), r("provider/fs/native")).with_priority(10),
    );

    let composition =
        resolve_harness_composition(&catalog, request(vec![selection("component/agent-loop")]))
            .unwrap();

    assert_eq!(composition.contract_bindings.len(), 1);
    assert_eq!(composition.contract_bindings[0].contract, filesystem);
    assert!(composition.contract_bindings[0].required);
    assert!(composition.contract_bindings[0].reactive);
    assert_eq!(composition.absences.len(), 1);
    assert_eq!(composition.absences[0].requirement, telemetry);
    assert!(!composition.absences[0].required);
    assert!(composition.absences[0].reason.contains("no provider"));
}

#[test]
fn missing_required_requirement_is_a_structured_composition_error() {
    let mut catalog = CompositionCatalog::default();
    let mut component = live_component("component/needs-sandbox");
    component
        .requirements
        .push(ComponentRequirement::required(r("contract/sandbox")));
    catalog.insert_component(component);

    let error = resolve_harness_composition(
        &catalog,
        request(vec![selection("component/needs-sandbox")]),
    )
    .unwrap_err();

    assert_eq!(error.code(), "composition.required_requirement_unsatisfied");
    assert_eq!(
        error.details().get("component").map(String::as_str),
        Some("component/needs-sandbox")
    );
    assert_eq!(
        error.details().get("requirement").map(String::as_str),
        Some("contract/sandbox")
    );
}

#[test]
fn provider_substitution_is_deterministic_and_preserves_contract_identity() {
    let mut catalog = CompositionCatalog::default();
    let contract = r("contract/session-store");
    let mut component = live_component("component/session");
    component
        .requirements
        .push(ComponentRequirement::required(contract.clone()).with_compatibility("append-log"));
    catalog.insert_component(component);
    catalog.add_provider(
        ContractProvider::available(contract.clone(), r("provider/z-secondary"))
            .with_priority(5)
            .with_compatibility("append-log"),
    );
    catalog.add_provider(
        ContractProvider::available(contract.clone(), r("provider/a-primary"))
            .with_priority(20)
            .with_compatibility("append-log"),
    );

    let composition =
        resolve_harness_composition(&catalog, request(vec![selection("component/session")]))
            .unwrap();

    assert_eq!(composition.contract_bindings[0].contract, contract);
    assert_eq!(
        composition.contract_bindings[0].provider,
        r("provider/a-primary")
    );
}

#[test]
fn resolution_activation_and_lifetime_scopes_remain_independent() {
    let mut catalog = CompositionCatalog::default();
    catalog.insert_component(live_component("component/ui"));
    let selection = ComponentSelection {
        component: r("component/ui"),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "project profile"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session/alpha"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::ComponentContext)
            .with_reference("cordis-context/42"),
        activation_mode: CompositionActivationMode::LiveMounted,
    };

    let composition = resolve_harness_composition(&catalog, request(vec![selection])).unwrap();
    let binding = &composition.component_bindings[0];

    assert_eq!(binding.resolution_scope.scope, ScopeKind::Project);
    assert_eq!(
        binding.activation_scope.kind,
        ActivationScopeKind::AgentSession
    );
    assert_eq!(
        binding.lifetime_owner.kind,
        LifetimeOwnerKind::ComponentContext
    );
    assert_eq!(
        binding.activation_mode,
        CompositionActivationMode::LiveMounted
    );
}

#[test]
fn body_can_change_without_project_agent_or_harness_identity_drift() {
    let mut catalog = CompositionCatalog::default();
    catalog.insert_component(live_component("component/tools"));
    catalog.insert_component(live_component("component/ui"));

    let body_zero =
        resolve_harness_composition(&catalog, request(vec![selection("component/tools")])).unwrap();
    let body_one = resolve_harness_composition(
        &catalog,
        request(vec![
            selection("component/tools"),
            selection("component/ui"),
        ]),
    )
    .unwrap();

    assert_eq!(body_zero.project, body_one.project);
    assert_eq!(body_zero.agent, body_one.agent);
    assert_eq!(body_zero.agency, body_one.agency);
    assert_eq!(body_zero.harness, body_one.harness);
    assert_ne!(body_zero.fingerprint, body_one.fingerprint);
}

#[test]
fn thin_static_harness_is_a_valid_empty_composition() {
    let composition =
        resolve_harness_composition(&CompositionCatalog::default(), request(vec![])).unwrap();

    assert!(composition.component_bindings.is_empty());
    assert!(composition.contract_bindings.is_empty());
    assert!(composition.contributions.is_empty());
    assert!(composition.surfaces.is_empty());
    assert_eq!(composition.harness, r("harness/deepseek"));
}

#[test]
fn requirement_strength_is_not_activation_mode() {
    assert!(RequirementStrength::Required.is_required());
    assert!(!RequirementStrength::Optional.is_required());
    assert_ne!(
        CompositionActivationMode::NextSession,
        CompositionActivationMode::LiveMounted
    );
}
