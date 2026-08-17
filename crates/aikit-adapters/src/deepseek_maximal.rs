//! Maximal DeepSeek Harness/Cordis composition conformance specimen.
//!
//! `deepseek_harness_conformance()` remains the small executable provider-swap
//! fixture. This module layers the additional current target pressure required by
//! #65: nested plugin/UI composition, reactive injected services, scoped effects,
//! replaceable loop runtime, human commands/policy and rich web UI. DeepSeek
//! Harness is evidence for the language, never an AIKit dependency or ontology.

use std::collections::BTreeSet;

use aikit_core::resource::ResourceRef;
use aikit_core::{
    ActivationScope, ActivationScopeKind, ComponentContribution, ComponentDescriptor,
    ComponentRequirement, ComponentSelection, CompositionActivationMode, ContractProvider,
    ContributionKind, LifetimeOwner, LifetimeOwnerKind, ResolutionScope, RetractionMode, ScopeKind,
    SurfaceDescriptor, SurfaceKind, TargetNativeComponentBinding,
};

use crate::composition_topology::ComponentContainment;
use crate::deepseek_harness::{
    deepseek_harness_conformance, DeepSeekHarnessConformance, DeepSeekShellProvider,
    DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};

pub const DEEPSEEK_CORDIS_REVISION: &str = DEEPSEEK_HARNESS_UPSTREAM_REVISION;

pub struct DeepSeekMaximalConformance {
    pub specimen: DeepSeekHarnessConformance,
    pub containments: Vec<ComponentContainment>,
}

pub fn deepseek_maximal_conformance(shell: DeepSeekShellProvider) -> DeepSeekMaximalConformance {
    let mut specimen = deepseek_harness_conformance(shell);
    let root = r("component/deepseek/profile-root");
    let ui_slots = r("component/deepseek/client-ui-slots");
    let ui_conversation = r("component/deepseek/client-ui-conversation");
    let ui_commands = r("component/deepseek/client-ui-commands");
    let ui_permission = r("component/deepseek/client-ui-permission");
    let agent_loop = r("component/deepseek/agent-loop");
    let web_surface = r("surface/deepseek/web-conversation");

    specimen.catalog.insert_surface(SurfaceDescriptor {
        resource: web_surface.clone(),
        kind: SurfaceKind::Web,
        target_native_id: Some("packages/client/ui-conversation + ui-slots".into()),
        owner_component: Some(ui_conversation.clone()),
    });

    specimen.catalog.insert_component(native_component(
        root.clone(),
        "profile-root",
        "examples/agent-spine-demo + configured Cordis loader tree",
    ));
    specimen.request.selections.push(selection(
        root.clone(),
        ActivationScopeKind::Host,
        LifetimeOwnerKind::Generation,
        "deepseek-harness/profile",
    ));

    let mut slots = native_component(
        ui_slots.clone(),
        "client-ui-slots",
        "@deepseek-ai/dsh-client-ui-slots",
    );
    slots
        .provisions
        .push(r("contract/deepseek/client-ui-slots"));
    specimen.catalog.add_provider(
        ContractProvider::available(r("contract/deepseek/client-ui-slots"), ui_slots.clone())
            .supplied_by(ui_slots.clone()),
    );
    specimen.catalog.insert_component(slots);
    specimen.request.selections.push(selection(
        ui_slots.clone(),
        ActivationScopeKind::Host,
        LifetimeOwnerKind::ComponentContext,
        "deepseek-web-client",
    ));

    let mut conversation = native_component(
        ui_conversation.clone(),
        "client-ui-conversation",
        "@deepseek-ai/dsh-client-ui-conversation",
    );
    conversation
        .requirements
        .push(ComponentRequirement::required(r("contract/deepseek/client-ui-slots")).reactive());
    conversation.supported_surfaces.push(web_surface.clone());
    conversation.contributions.push(contribution(
        "contribution/deepseek/ui-conversation/main",
        &ui_conversation,
        ContributionKind::UiNode,
        Some(web_surface.clone()),
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::ComponentContext,
        "rich React conversation/input contribution registered through client UI slots",
    ));
    specimen.catalog.insert_component(conversation);
    specimen.request.selections.push(selection(
        ui_conversation.clone(),
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::ComponentContext,
        "selected-session",
    ));

    let mut commands = native_component(
        ui_commands.clone(),
        "client-ui-commands",
        "@deepseek-ai/dsh-client-ui-commands",
    );
    commands
        .requirements
        .push(ComponentRequirement::required(r("contract/deepseek/client-ui-slots")).reactive());
    commands.supported_surfaces.push(web_surface.clone());
    commands.contributions.push(contribution(
        "contribution/deepseek/ui-commands/discovery",
        &ui_commands,
        ContributionKind::HumanCommand,
        Some(web_surface.clone()),
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::ComponentContext,
        "session-aware command discovery/dispatch remains a native command contribution, not an AIKit Action",
    ));
    specimen.catalog.insert_component(commands);
    specimen.request.selections.push(selection(
        ui_commands.clone(),
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::ComponentContext,
        "selected-session",
    ));

    let mut permission = native_component(
        ui_permission.clone(),
        "client-ui-permission",
        "@deepseek-ai/dsh-client-ui-permission",
    );
    permission
        .requirements
        .push(ComponentRequirement::required(r("contract/deepseek/client-ui-slots")).reactive());
    permission.supported_surfaces.push(web_surface.clone());
    permission.contributions.extend([
        contribution(
            "contribution/deepseek/ui-permission/policy",
            &ui_permission,
            ContributionKind::Policy,
            None,
            ActivationScopeKind::AgentSession,
            LifetimeOwnerKind::ComponentContext,
            "current-session permission preset/configuration contribution",
        ),
        contribution(
            "contribution/deepseek/ui-permission/node",
            &ui_permission,
            ContributionKind::UiNode,
            Some(web_surface.clone()),
            ActivationScopeKind::AgentSession,
            LifetimeOwnerKind::ComponentContext,
            "permission UI remains presentation over native interaction/policy state",
        ),
    ]);
    specimen.catalog.insert_component(permission);
    specimen.request.selections.push(selection(
        ui_permission.clone(),
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::ComponentContext,
        "selected-session",
    ));

    let mut loop_component = native_component(
        agent_loop.clone(),
        "agent-loop",
        "@deepseek-ai/dsh-agent-loop",
    );
    loop_component
        .provisions
        .push(r("contract/deepseek/agent-loop"));
    loop_component.contributions.push(contribution(
        "contribution/deepseek/agent-loop/runtime",
        &agent_loop,
        ContributionKind::LoopRuntime,
        None,
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::AgentSession,
        "default concrete agent driver behind the stable agent seam; target documentation states it is swappable",
    ));
    specimen.catalog.add_provider(
        ContractProvider::available(r("contract/deepseek/agent-loop"), agent_loop.clone())
            .supplied_by(agent_loop.clone()),
    );
    specimen.catalog.insert_component(loop_component);
    specimen.request.selections.push(selection(
        agent_loop.clone(),
        ActivationScopeKind::AgentSession,
        LifetimeOwnerKind::AgentSession,
        "agent-session",
    ));

    let mut containments = vec![
        contains(
            &root,
            "component/deepseek/tools",
            "Cordis profile plugin tree",
        ),
        contains(
            &root,
            "component/deepseek/system-prompt",
            "Cordis profile plugin tree",
        ),
        contains(
            &root,
            "component/deepseek/shell-env",
            "Cordis profile plugin tree",
        ),
        contains(
            &root,
            "component/deepseek/session",
            "Cordis profile plugin tree",
        ),
        contains(
            &root,
            "component/deepseek/tool-bash",
            "Cordis profile plugin tree",
        ),
        ComponentContainment::new(root.clone(), ui_slots.clone())
            .with_provenance("Cordis loader/profile contains client UI composition root"),
        ComponentContainment::new(ui_slots.clone(), ui_conversation)
            .with_provenance("ui-slots composes the conversation feature"),
        ComponentContainment::new(ui_slots.clone(), ui_commands)
            .with_provenance("ui-slots composes command discovery/dispatch"),
        ComponentContainment::new(ui_slots, ui_permission)
            .with_provenance("ui-slots composes permission presentation"),
        ComponentContainment::new(root.clone(), agent_loop)
            .with_provenance("Cordis profile supplies the replaceable agent-loop implementation"),
    ];

    match shell {
        DeepSeekShellProvider::Local => containments.push(contains(
            &root,
            "component/deepseek/bash-local",
            "Cordis profile plugin tree",
        )),
        DeepSeekShellProvider::Sandbox => {
            for child in [
                "component/deepseek/sandbox-local",
                "component/deepseek/sandbox-policy",
                "component/deepseek/bash-sandbox",
            ] {
                containments.push(contains(&root, child, "Cordis nested sandbox composition"));
            }
        }
    }

    DeepSeekMaximalConformance {
        specimen,
        containments,
    }
}

fn native_component(resource: ResourceRef, row_id: &str, native_id: &str) -> ComponentDescriptor {
    let mut descriptor = ComponentDescriptor::new(resource);
    descriptor.implementation = Some(TargetNativeComponentBinding {
        implementation_target: "deepseek-ai/deepseek-harness".into(),
        native_id: format!("{row_id}:{native_id}"),
        revision: Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION.into()),
    });
    descriptor.activation_modes = BTreeSet::from([CompositionActivationMode::NextSession]);
    descriptor
}

fn selection(
    component: ResourceRef,
    activation_kind: ActivationScopeKind,
    lifetime_kind: LifetimeOwnerKind,
    reference: &str,
) -> ComponentSelection {
    ComponentSelection {
        component,
        resolution_scope: ResolutionScope::new(
            ScopeKind::Global,
            "DeepSeek Harness profile/bundle composition",
        ),
        activation_scope: ActivationScope::new(activation_kind).with_reference(reference),
        lifetime_owner: LifetimeOwner::new(lifetime_kind).with_reference(reference),
        // AIKit can inspect this target now but cannot prove a live Cordis mount.
        activation_mode: CompositionActivationMode::NextSession,
    }
}

fn contribution(
    id: &str,
    component: &ResourceRef,
    kind: ContributionKind,
    surface: Option<ResourceRef>,
    activation_kind: ActivationScopeKind,
    lifetime_kind: LifetimeOwnerKind,
    provenance: &str,
) -> ComponentContribution {
    ComponentContribution {
        id: r(id),
        component: component.clone(),
        kind,
        target_contract: None,
        exposed_ref: None,
        exposed_kind: None,
        surface,
        activation_scope: ActivationScope::new(activation_kind),
        lifetime_owner: LifetimeOwner::new(lifetime_kind),
        activation_mode: CompositionActivationMode::NextSession,
        // Cordis owns/retracts registered effects with the owning Fiber/Context.
        retraction_mode: RetractionMode::Live,
        provenance: vec![
            format!("deepseek-ai/deepseek-harness@{DEEPSEEK_HARNESS_UPSTREAM_REVISION}"),
            provenance.into(),
        ],
    }
}

fn contains(parent: &ResourceRef, child: &str, provenance: &str) -> ComponentContainment {
    ComponentContainment::new(parent.clone(), r(child)).with_provenance(provenance)
}

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).expect("DeepSeek maximal conformance uses static valid ResourceRefs")
}
