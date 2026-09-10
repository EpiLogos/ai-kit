use aikit_adapters::{
    deepseek_harness_conformance, DeepSeekShellProvider, DEEPSEEK_HARNESS_RELEASE,
    DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};
use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::{
    diff_harness_compositions, resolve_harness_composition, CompositionActivationMode,
    RetractionMode,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn current_upstream_specimen_preserves_plugin_injection_effect_and_surface_relations() {
    assert_eq!(DEEPSEEK_HARNESS_RELEASE, "0.1.0-rc.7");
    assert_eq!(
        DEEPSEEK_HARNESS_UPSTREAM_REVISION,
        "99f6f02fecdb7dff40c3fbc9470f5907c29f74ca"
    );

    let specimen = deepseek_harness_conformance(DeepSeekShellProvider::Local);
    let composition =
        resolve_harness_composition(&specimen.catalog, specimen.request).unwrap();

    assert_eq!(composition.harness, r("harness/deepseek"));
    assert_eq!(
        composition.target_revision.as_deref(),
        Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION)
    );

    let tool = composition
        .component_bindings
        .iter()
        .find(|binding| binding.component == r("component/deepseek/tool-bash"))
        .unwrap();
    assert_eq!(tool.activation_mode, CompositionActivationMode::NextSession);

    let injected = composition
        .contract_bindings
        .iter()
        .filter(|binding| binding.consumer_component == r("component/deepseek/tool-bash"))
        .map(|binding| binding.contract.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        injected,
        std::collections::BTreeSet::from([
            r("contract/deepseek/tools"),
            r("contract/deepseek/bash"),
            r("contract/deepseek/system-prompt"),
            r("contract/deepseek/bash-env"),
        ])
    );

    let bash_projection = composition
        .projections
        .iter()
        .find(|projection| projection.canonical_ref == r("capability/deepseek/bash"))
        .unwrap();
    assert_eq!(bash_projection.canonical_kind, ResourceKind::Capability);
    assert_eq!(bash_projection.surface, r("surface/deepseek/model-tools"));

    let session_projection = composition
        .projections
        .iter()
        .find(|projection| {
            projection.canonical_ref == r("knowledge-node/deepseek/session-event-log")
        })
        .unwrap();
    assert_eq!(session_projection.canonical_kind, ResourceKind::KnowledgeNode);
    assert_eq!(
        session_projection.surface,
        r("surface/deepseek/session-trajectory")
    );

    let tool_schema = composition
        .contributions
        .iter()
        .find(|contribution| contribution.id == r("contribution/deepseek/tool-bash/schema"))
        .unwrap();
    assert_eq!(tool_schema.activation_mode, CompositionActivationMode::NextSession);
    assert_eq!(tool_schema.retraction_mode, RetractionMode::Live);
    assert!(tool_schema
        .provenance
        .iter()
        .any(|value| value.contains(DEEPSEEK_HARNESS_UPSTREAM_REVISION)));
}

#[test]
fn sandbox_is_a_provider_swap_not_a_new_bash_capability_or_tool_component() {
    let local = deepseek_harness_conformance(DeepSeekShellProvider::Local);
    let local_body = resolve_harness_composition(&local.catalog, local.request).unwrap();
    let sandbox = deepseek_harness_conformance(DeepSeekShellProvider::Sandbox);
    let sandbox_body = resolve_harness_composition(&sandbox.catalog, sandbox.request).unwrap();

    let diff = diff_harness_compositions(&local_body, &sandbox_body).unwrap();
    let shell_rebind = diff
        .rebound_contracts
        .iter()
        .find(|rebind| {
            rebind.consumer_component == r("component/deepseek/tool-bash")
                && rebind.contract == r("contract/deepseek/bash")
        })
        .expect("same tool consumer should bind to the sandbox executor provider");
    assert_eq!(
        shell_rebind.before_provider,
        r("component/deepseek/bash-local")
    );
    assert_eq!(
        shell_rebind.after_provider,
        r("component/deepseek/bash-sandbox")
    );

    assert!(diff
        .retracted_components
        .contains(&r("component/deepseek/bash-local")));
    assert!(diff
        .mounted_components
        .contains(&r("component/deepseek/bash-sandbox")));
    assert!(diff
        .mounted_components
        .contains(&r("component/deepseek/sandbox-local")));
    assert!(diff
        .mounted_components
        .contains(&r("component/deepseek/sandbox-policy")));

    for body in [&local_body, &sandbox_body] {
        assert_eq!(
            body.projections
                .iter()
                .filter(|projection| projection.canonical_ref == r("capability/deepseek/bash"))
                .count(),
            1
        );
        assert!(body
            .component_bindings
            .iter()
            .any(|binding| binding.component == r("component/deepseek/tool-bash")));
    }
}

#[test]
fn adapter_does_not_claim_a_live_control_path_it_cannot_prove() {
    let specimen = deepseek_harness_conformance(DeepSeekShellProvider::Sandbox);
    let body = resolve_harness_composition(&specimen.catalog, specimen.request).unwrap();

    assert!(body
        .component_bindings
        .iter()
        .all(|binding| binding.activation_mode == CompositionActivationMode::NextSession));
    assert!(body
        .contributions
        .iter()
        .all(|contribution| contribution.activation_mode == CompositionActivationMode::NextSession));
    assert!(body
        .contributions
        .iter()
        .all(|contribution| contribution.retraction_mode == RetractionMode::Live));
}
