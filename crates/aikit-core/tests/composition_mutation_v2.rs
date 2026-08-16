use std::collections::BTreeSet;

use aikit_core::{
    apply_confirmed_harness_composition, preview_harness_composition_change,
    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentContribution,
    ComponentDescriptor, ComponentSelection, CompositionActivationMode, CompositionCatalog,
    CompositionState, ContributionKind, HarnessCompositionRequest, LifetimeOwner,
    LifetimeOwnerKind, ResolutionScope, ResourceKind, ResourceRef, RetractionMode, ScopeKind,
    StagedHarnessComposition, SurfaceDescriptor, SurfaceKind,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn selection(component: &ResourceRef) -> ComponentSelection {
    ComponentSelection {
        component: component.clone(),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, ".aikit/project.toml"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session:test"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation)
            .with_reference("generation:test-next"),
        activation_mode: CompositionActivationMode::Generated,
    }
}

fn catalog() -> (CompositionCatalog, ResourceRef, ResourceRef, ResourceRef, ResourceRef) {
    let component = r("component/review-runtime");
    let action = r("action/review");
    let tui = r("surface/aikit/tui");
    let agent = r("surface/aikit/agent-tool");

    let mut catalog = CompositionCatalog::default();
    catalog.insert_surface(SurfaceDescriptor {
        resource: tui.clone(),
        kind: SurfaceKind::Tui,
        target_native_id: Some("workspace.review".into()),
        owner_component: Some(component.clone()),
    });
    catalog.insert_surface(SurfaceDescriptor {
        resource: agent.clone(),
        kind: SurfaceKind::AgentTool,
        target_native_id: Some("review".into()),
        owner_component: Some(component.clone()),
    });

    let contribution = |id: &str, surface: ResourceRef| ComponentContribution {
        id: r(id),
        component: component.clone(),
        kind: ContributionKind::ActionProjection,
        target_contract: None,
        exposed_ref: Some(action.clone()),
        exposed_kind: Some(ResourceKind::Action),
        surface: Some(surface),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session:test"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation)
            .with_reference("generation:test-next"),
        activation_mode: CompositionActivationMode::Generated,
        retraction_mode: RetractionMode::NextSession,
        provenance: vec!["fixture:composition-mutation-v2".into()],
    };

    let mut descriptor = ComponentDescriptor::new(component.clone());
    descriptor.supported_surfaces = vec![tui.clone(), agent.clone()];
    descriptor.contributions = vec![
        contribution("contribution/review/tui", tui.clone()),
        contribution("contribution/review/agent", agent.clone()),
    ];
    descriptor.activation_modes = BTreeSet::from([CompositionActivationMode::Generated]);
    catalog.insert_component(descriptor);

    (catalog, component, action, tui, agent)
}

fn request(selections: Vec<ComponentSelection>) -> HarnessCompositionRequest {
    HarnessCompositionRequest {
        harness: r("harness/deepseek/test"),
        project: Some(r("project/payments")),
        agent: Some(r("agent/reviewer")),
        agency: Some(r("agency/payments")),
        session: Some("session:test".into()),
        model: Some(r("model/deepseek/test")),
        selections,
        target_revision: Some("deepseek:test-r1".into()),
        generation: Some("generation:test-next".into()),
    }
}

#[test]
fn staged_component_change_previews_surfaces_and_human_agent_projections_before_confirm_apply() {
    let (catalog, component, action, tui, agent) = catalog();
    let current = resolve_harness_composition(&catalog, request(vec![])).unwrap();
    assert!(current.component_bindings.is_empty());

    let mut staged = StagedHarnessComposition::new();
    staged.select(selection(&component));
    let preview = preview_harness_composition_change(&catalog, &current, staged).unwrap();

    assert_eq!(preview.before_fingerprint, current.fingerprint);
    assert_ne!(preview.projected.fingerprint, current.fingerprint);
    assert_eq!(preview.diff.mounted_components, vec![component.clone()]);
    assert_eq!(
        preview.diff.added_surfaces.iter().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([tui.clone(), agent.clone()])
    );
    assert_eq!(preview.diff.added_contributions.len(), 2);

    // One canonical Action identity is projected to both the human and agent
    // surfaces; staging does not mint surface-native semantic identities.
    assert_eq!(preview.projected.projections.len(), 2);
    assert!(preview
        .projected
        .projections
        .iter()
        .all(|projection| projection.canonical_ref == action));
    assert!(preview
        .projected
        .projections
        .iter()
        .all(|projection| projection.canonical_kind == ResourceKind::Action));
    assert_eq!(
        preview
            .projected
            .projections
            .iter()
            .map(|projection| projection.surface.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([tui, agent])
    );

    // Applying here means accepting a desired resolved body. No target adapter
    // participated, so material/live state must remain unclaimed.
    let applied = apply_confirmed_harness_composition(preview.confirm());
    assert_eq!(applied.state, CompositionState::Resolved);
    assert_eq!(applied.project, current.project);
    assert_eq!(applied.agent, current.agent);
    assert_eq!(applied.agency, current.agency);
    assert_eq!(applied.harness, current.harness);
    assert_eq!(applied.generation.as_deref(), Some("generation:test-next"));
}

#[test]
fn retraction_uses_the_same_resolver_and_reports_surface_projection_removal() {
    let (catalog, component, _action, tui, agent) = catalog();
    let current =
        resolve_harness_composition(&catalog, request(vec![selection(&component)])).unwrap();
    assert_eq!(current.projections.len(), 2);

    let mut staged = StagedHarnessComposition::new();
    staged.retract(component.clone());
    let preview = preview_harness_composition_change(&catalog, &current, staged).unwrap();

    assert_eq!(preview.diff.retracted_components, vec![component]);
    assert_eq!(preview.diff.removed_contributions.len(), 2);
    assert_eq!(
        preview.diff.removed_surfaces.iter().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([tui, agent])
    );
    assert!(preview.projected.projections.is_empty());
    assert!(preview.projected.component_bindings.is_empty());

    let applied = apply_confirmed_harness_composition(preview.confirm());
    assert_eq!(applied.state, CompositionState::Resolved);
    assert!(applied.component_bindings.is_empty());
}

#[test]
fn repeated_staging_for_one_component_is_deterministic_and_last_intent_wins() {
    let (catalog, component, _action, _tui, _agent) = catalog();
    let current = resolve_harness_composition(&catalog, request(vec![])).unwrap();

    let mut first = StagedHarnessComposition::new();
    first.select(selection(&component));
    let first_preview = preview_harness_composition_change(&catalog, &current, first).unwrap();

    let mut second = StagedHarnessComposition::new();
    second.retract(component.clone());
    second.select(selection(&component));
    assert_eq!(second.len(), 1);
    let second_preview = preview_harness_composition_change(&catalog, &current, second).unwrap();

    assert_eq!(first_preview.projected.fingerprint, second_preview.projected.fingerprint);
    assert_eq!(first_preview.diff, second_preview.diff);
}

#[test]
fn unknown_component_fails_at_preview_without_mutating_current_body() {
    let (catalog, _component, _action, _tui, _agent) = catalog();
    let current = resolve_harness_composition(&catalog, request(vec![])).unwrap();
    let before = current.clone();
    let mut staged = StagedHarnessComposition::new();
    staged.select(selection(&r("component/unknown")));

    let error = preview_harness_composition_change(&catalog, &current, staged).unwrap_err();
    assert_eq!(error.code(), "composition.unknown_component");
    assert_eq!(current, before);
}
