use aikit_adapters::{
    deepseek_maximal_conformance, resolve_component_topology, DeepSeekShellProvider,
    DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{
    apply_confirmed_harness_composition, preview_harness_composition_change,
    resolve_harness_composition, ActivationScopeKind, CompositionActivationMode,
    ContributionKind, LifetimeOwnerKind, RetractionMode, StagedHarnessComposition,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn maximal_dsh_specimen_expresses_nested_cordis_and_client_ui_composition() {
    let maximal = deepseek_maximal_conformance(DeepSeekShellProvider::Sandbox);
    let body =
        resolve_harness_composition(&maximal.specimen.catalog, maximal.specimen.request).unwrap();
    let topology = resolve_component_topology(&body, maximal.containments).unwrap();

    assert_eq!(
        body.target_revision.as_deref(),
        Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION)
    );
    assert_eq!(topology.composition_fingerprint, body.fingerprint);
    assert_eq!(topology.roots, vec![r("component/deepseek/profile-root")]);
    assert_eq!(
        topology.parent_of(&r("component/deepseek/client-ui-conversation")),
        Some(&r("component/deepseek/client-ui-slots"))
    );
    assert_eq!(
        topology.parent_of(&r("component/deepseek/bash-sandbox")),
        Some(&r("component/deepseek/profile-root"))
    );
}

#[test]
fn current_dsh_pressure_covers_reactive_scoped_lifecycle_loop_command_policy_and_rich_web_ui() {
    let maximal = deepseek_maximal_conformance(DeepSeekShellProvider::Local);
    let body =
        resolve_harness_composition(&maximal.specimen.catalog, maximal.specimen.request).unwrap();

    let ui_reactive = body.contract_bindings.iter().filter(|binding| {
        binding.contract == r("contract/deepseek/client-ui-slots") && binding.reactive
    });
    assert_eq!(ui_reactive.count(), 3);

    let conversation = body
        .component_bindings
        .iter()
        .find(|binding| binding.component == r("component/deepseek/client-ui-conversation"))
        .unwrap();
    assert_eq!(
        conversation.activation_scope.kind,
        ActivationScopeKind::AgentSession
    );
    assert_eq!(
        conversation.lifetime_owner.kind,
        LifetimeOwnerKind::ComponentContext
    );
    assert_eq!(
        conversation.activation_mode,
        CompositionActivationMode::NextSession
    );

    let kinds = body
        .contributions
        .iter()
        .map(|contribution| contribution.kind)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(kinds.contains(&ContributionKind::HumanCommand));
    assert!(kinds.contains(&ContributionKind::Policy));
    assert!(kinds.contains(&ContributionKind::UiNode));
    assert!(kinds.contains(&ContributionKind::LoopRuntime));
    assert!(kinds.contains(&ContributionKind::Trajectory));
    assert!(kinds.contains(&ContributionKind::Tool));
    assert!(kinds.contains(&ContributionKind::ContextSection));

    let web = body
        .surfaces
        .iter()
        .find(|surface| surface.resource == r("surface/deepseek/web-conversation"))
        .unwrap();
    assert_eq!(
        web.owner_component.as_ref(),
        Some(&r("component/deepseek/client-ui-conversation"))
    );

    assert!(body.contributions.iter().all(|contribution| {
        contribution.activation_mode == CompositionActivationMode::NextSession
    }));
    assert!(body
        .contributions
        .iter()
        .filter(|contribution| {
            matches!(
                contribution.kind,
                ContributionKind::UiNode
                    | ContributionKind::HumanCommand
                    | ContributionKind::Policy
                    | ContributionKind::LoopRuntime
            )
        })
        .all(|contribution| contribution.retraction_mode == RetractionMode::Live));
}

#[test]
fn staged_dsh_mutation_changes_desired_body_without_claiming_live_cordis_activation() {
    let maximal = deepseek_maximal_conformance(DeepSeekShellProvider::Local);
    let catalog = maximal.specimen.catalog;
    let current = resolve_harness_composition(&catalog, maximal.specimen.request).unwrap();

    let mut staged = StagedHarnessComposition::new();
    staged.retract(r("component/deepseek/client-ui-commands"));
    let preview = preview_harness_composition_change(&catalog, &current, staged).unwrap();

    assert_ne!(preview.before_fingerprint, preview.projected.fingerprint);
    assert!(preview
        .projected
        .component_bindings
        .iter()
        .all(|binding| binding.component != r("component/deepseek/client-ui-commands")));
    assert!(preview
        .projected
        .contributions
        .iter()
        .all(|contribution| contribution.activation_mode == CompositionActivationMode::NextSession));

    let desired = apply_confirmed_harness_composition(preview.confirm());
    assert!(desired
        .component_bindings
        .iter()
        .all(|binding| binding.component != r("component/deepseek/client-ui-commands")));
    assert!(desired.component_bindings.iter().all(|binding| {
        binding.activation_mode == CompositionActivationMode::NextSession
    }));
}

#[test]
fn containment_is_semantic_and_rejects_unmounted_or_cyclic_target_trees() {
    use aikit_adapters::ComponentContainment;

    let maximal = deepseek_maximal_conformance(DeepSeekShellProvider::Local);
    let body =
        resolve_harness_composition(&maximal.specimen.catalog, maximal.specimen.request).unwrap();

    let error = resolve_component_topology(
        &body,
        vec![ComponentContainment::new(
            r("component/deepseek/profile-root"),
            r("component/deepseek/not-mounted"),
        )],
    )
    .unwrap_err();
    assert_eq!(
        error.code(),
        "composition.containment_unmounted_component"
    );

    let error = resolve_component_topology(
        &body,
        vec![
            ComponentContainment::new(
                r("component/deepseek/profile-root"),
                r("component/deepseek/tools"),
            ),
            ComponentContainment::new(
                r("component/deepseek/tools"),
                r("component/deepseek/profile-root"),
            ),
        ],
    )
    .unwrap_err();
    assert_eq!(error.code(), "composition.containment_cycle");
}
