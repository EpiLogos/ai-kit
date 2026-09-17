//! DeepSeek Harness/Cordis composition adapter.
//!
//! This adapter is pinned to the current public DeepSeek Harness revision AIKit
//! has conformed against. It deliberately models only facts the upstream
//! architecture publishes: Cordis plugin rows as Components, service seams as
//! Contracts, injected dependencies as Requirements, registrations as
//! lifecycle-owned Contributions, and UI/tool/log encounter points as Surfaces.
//!
//! The semantic adapter is read-oriented. DeepSeek Harness itself has reversible
//! Cordis effects; the separate live activation driver proves the target control
//! path when the pinned source is physically present in CI.

use std::collections::BTreeSet;

use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::{
    ActivationScope, ActivationScopeKind, ComponentContribution, ComponentDescriptor,
    ComponentRequirement, ComponentSelection, CompositionActivationMode, CompositionCatalog,
    ContractProvider, ContributionKind, HarnessCompositionRequest, LifetimeOwner,
    LifetimeOwnerKind, ResolutionScope, RetractionMode, ScopeKind, SurfaceDescriptor, SurfaceKind,
    TargetNativeComponentBinding,
};

/// `deepseek-ai/deepseek-harness` revision used for current conformance.
pub const DEEPSEEK_HARNESS_UPSTREAM_REVISION: &str = "99f6f02fecdb7dff40c3fbc9470f5907c29f74ca";
pub const DEEPSEEK_HARNESS_RELEASE: &str = "0.1.0-rc.7";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepSeekShellProvider {
    Local,
    Sandbox,
}

/// A deterministic representative body from the current public DeepSeek Harness
/// architecture. It is not a dump of every shipped plugin: it deliberately spans
/// the component/service/provider/consumer/effect/surface relations AIKit must be
/// able to preserve before a broader inventory importer is useful.
pub struct DeepSeekHarnessConformance {
    pub catalog: CompositionCatalog,
    pub request: HarnessCompositionRequest,
}

pub fn deepseek_harness_conformance(shell: DeepSeekShellProvider) -> DeepSeekHarnessConformance {
    let mut catalog = CompositionCatalog::default();

    let tool_surface = r("surface/deepseek/model-tools");
    let prompt_surface = r("surface/deepseek/system-prompt");
    let web_surface = r("surface/deepseek/web-tool-card");
    let trajectory_surface = r("surface/deepseek/session-trajectory");

    catalog.insert_surface(surface(
        &tool_surface,
        SurfaceKind::AgentTool,
        "ctx.tools",
        Some("component/deepseek/tools"),
    ));
    catalog.insert_surface(surface(
        &prompt_surface,
        SurfaceKind::Conversation,
        "ctx.systemPrompt",
        Some("component/deepseek/system-prompt"),
    ));
    catalog.insert_surface(surface(
        &web_surface,
        SurfaceKind::Web,
        "tool.presentCall/presentResult",
        Some("component/deepseek/tool-bash"),
    ));
    catalog.insert_surface(surface(
        &trajectory_surface,
        SurfaceKind::Trajectory,
        "session/event",
        Some("component/deepseek/session"),
    ));

    let tools = provider_component(
        "component/deepseek/tools",
        "tools",
        "@deepseek-ai/dsh-tools",
        "contract/deepseek/tools",
        "ctx.tools",
    );
    let system_prompt = provider_component(
        "component/deepseek/system-prompt",
        "system-prompt",
        "@deepseek-ai/dsh-system-prompt",
        "contract/deepseek/system-prompt",
        "ctx.systemPrompt",
    );
    let shell_env = provider_component(
        "component/deepseek/shell-env",
        "shell-env",
        "@deepseek-ai/dsh-shell-env",
        "contract/deepseek/bash-env",
        "ctx.bashEnv",
    );
    let session = session_component(&trajectory_surface);

    insert_provider_component(&mut catalog, tools, "contract/deepseek/tools", "ctx.tools");
    insert_provider_component(
        &mut catalog,
        system_prompt,
        "contract/deepseek/system-prompt",
        "ctx.systemPrompt",
    );
    insert_provider_component(
        &mut catalog,
        shell_env,
        "contract/deepseek/bash-env",
        "ctx.bashEnv",
    );
    insert_provider_component(
        &mut catalog,
        session,
        "contract/deepseek/session",
        "ctx.sessions",
    );

    let mut selections = vec![
        selection("component/deepseek/tools"),
        selection("component/deepseek/system-prompt"),
        selection("component/deepseek/shell-env"),
        selection("component/deepseek/session"),
    ];

    match shell {
        DeepSeekShellProvider::Local => {
            let shell = provider_component(
                "component/deepseek/bash-local",
                "bash",
                "@deepseek-ai/dsh-bash-local",
                "contract/deepseek/bash",
                "ctx.shell",
            );
            insert_provider_component(&mut catalog, shell, "contract/deepseek/bash", "ctx.shell");
            selections.push(selection("component/deepseek/bash-local"));
        }
        DeepSeekShellProvider::Sandbox => {
            let sandbox = provider_component(
                "component/deepseek/sandbox-local",
                "sandbox",
                "@deepseek-ai/dsh-sandbox-local",
                "contract/deepseek/sandbox",
                "ctx.sandbox",
            );
            let policy = provider_component(
                "component/deepseek/sandbox-policy",
                "sandbox-policy",
                "@deepseek-ai/dsh-sandbox-policy",
                "contract/deepseek/sandbox-policy",
                "ctx.sandboxPolicy",
            );
            insert_provider_component(
                &mut catalog,
                sandbox,
                "contract/deepseek/sandbox",
                "ctx.sandbox",
            );
            insert_provider_component(
                &mut catalog,
                policy,
                "contract/deepseek/sandbox-policy",
                "ctx.sandboxPolicy",
            );

            let mut bash = provider_component(
                "component/deepseek/bash-sandbox",
                "bash",
                "@deepseek-ai/dsh-bash-sandbox",
                "contract/deepseek/bash",
                "ctx.shell",
            );
            bash.requirements = vec![
                ComponentRequirement::required(r("contract/deepseek/sandbox")),
                ComponentRequirement::required(r("contract/deepseek/sandbox-policy")),
            ];
            insert_provider_component(&mut catalog, bash, "contract/deepseek/bash", "ctx.shell");
            selections.extend([
                selection("component/deepseek/sandbox-local"),
                selection("component/deepseek/sandbox-policy"),
                selection("component/deepseek/bash-sandbox"),
            ]);
        }
    }

    let tool_bash = tool_bash_component(&tool_surface, &prompt_surface, &web_surface);
    catalog.insert_component(tool_bash);
    selections.push(selection("component/deepseek/tool-bash"));

    DeepSeekHarnessConformance {
        catalog,
        request: HarnessCompositionRequest {
            harness: r("harness/deepseek"),
            project: None,
            agent: None,
            agency: None,
            session: None,
            model: None,
            selections,
            target_revision: Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION.to_string()),
            generation: None,
        },
    }
}

fn tool_bash_component(
    tool_surface: &ResourceRef,
    prompt_surface: &ResourceRef,
    web_surface: &ResourceRef,
) -> ComponentDescriptor {
    let component = r("component/deepseek/tool-bash");
    let mut descriptor =
        component_descriptor(component.clone(), "tool-bash", "@deepseek-ai/dsh-tool-bash");
    // Current upstream package contract: inject
    // ['tools', 'bash', 'systemPrompt', 'bashEnv'].
    descriptor.requirements = vec![
        ComponentRequirement::required(r("contract/deepseek/tools")),
        ComponentRequirement::required(r("contract/deepseek/bash"))
            .with_compatibility("bash-dialect"),
        ComponentRequirement::required(r("contract/deepseek/system-prompt")),
        ComponentRequirement::required(r("contract/deepseek/bash-env")),
    ];
    descriptor.supported_surfaces = vec![
        tool_surface.clone(),
        prompt_surface.clone(),
        web_surface.clone(),
    ];
    descriptor.contributions = vec![
        contribution(
            "contribution/deepseek/tool-bash/schema",
            &component,
            ContributionKind::Tool,
            Some(r("capability/deepseek/bash")),
            Some(ResourceKind::Capability),
            Some(tool_surface.clone()),
            "dsh-tool-bash registers the model-facing bash schema on ctx.tools",
        ),
        contribution(
            "contribution/deepseek/tool-bash/prompt",
            &component,
            ContributionKind::ContextSection,
            None,
            None,
            Some(prompt_surface.clone()),
            "dsh-tool-bash contributes the tool:bash prompt section (order 105)",
        ),
        contribution(
            "contribution/deepseek/tool-bash/web-card",
            &component,
            ContributionKind::UiNode,
            None,
            None,
            Some(web_surface.clone()),
            "dsh-tool-bash owns replay-safe presentCall/presentResult rendering intent",
        ),
    ];
    descriptor
}

fn session_component(trajectory_surface: &ResourceRef) -> ComponentDescriptor {
    let component = r("component/deepseek/session");
    let mut descriptor =
        component_descriptor(component.clone(), "session", "@deepseek-ai/dsh-session");
    descriptor.provisions.push(r("contract/deepseek/session"));
    descriptor
        .supported_surfaces
        .push(trajectory_surface.clone());
    descriptor.contributions.push(contribution(
        "contribution/deepseek/session/trajectory",
        &component,
        ContributionKind::Trajectory,
        Some(r("knowledge-node/deepseek/session-event-log")),
        Some(ResourceKind::KnowledgeNode),
        Some(trajectory_surface.clone()),
        "session/event is the durable append-only stream from which replay/UI/history derive",
    ));
    descriptor
}

fn provider_component(
    component: &str,
    row_id: &str,
    package: &str,
    contract: &str,
    _target_service: &str,
) -> ComponentDescriptor {
    let mut descriptor = component_descriptor(r(component), row_id, package);
    descriptor.provisions.push(r(contract));
    if contract == "contract/deepseek/bash" {
        descriptor
            .activation_modes
            .insert(CompositionActivationMode::NextSession);
    }
    descriptor
}

fn insert_provider_component(
    catalog: &mut CompositionCatalog,
    component: ComponentDescriptor,
    contract: &str,
    target_service: &str,
) {
    let component_ref = component.resource.clone();
    let mut provider = ContractProvider::available(r(contract), component_ref.clone())
        .supplied_by(component_ref.clone());
    provider.target_native_id = Some(target_service.to_string());
    if contract == "contract/deepseek/bash" {
        provider.compatibility.insert("bash-dialect".into());
    }
    catalog.add_provider(provider);
    catalog.insert_component(component);
}

fn component_descriptor(resource: ResourceRef, row_id: &str, package: &str) -> ComponentDescriptor {
    let mut descriptor = ComponentDescriptor::new(resource);
    descriptor.implementation = Some(TargetNativeComponentBinding {
        implementation_target: "deepseek-ai/deepseek-harness".into(),
        native_id: format!("{row_id}:{package}"),
        revision: Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION.into()),
    });
    descriptor.activation_modes = BTreeSet::from([CompositionActivationMode::NextSession]);
    descriptor
}

fn selection(component: &str) -> ComponentSelection {
    ComponentSelection {
        component: r(component),
        resolution_scope: ResolutionScope::new(
            ScopeKind::Global,
            "DeepSeek Harness profile/bundle composition",
        ),
        activation_scope: ActivationScope::new(ActivationScopeKind::Host)
            .with_reference("deepseek-harness/profile"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation)
            .with_reference("deepseek-harness/profile"),
        activation_mode: CompositionActivationMode::NextSession,
    }
}

fn contribution(
    id: &str,
    component: &ResourceRef,
    kind: ContributionKind,
    exposed_ref: Option<ResourceRef>,
    exposed_kind: Option<ResourceKind>,
    surface: Option<ResourceRef>,
    provenance: &str,
) -> ComponentContribution {
    ComponentContribution {
        id: r(id),
        component: component.clone(),
        kind,
        target_contract: None,
        exposed_ref,
        exposed_kind,
        surface,
        activation_scope: ActivationScope::new(ActivationScopeKind::Host)
            .with_reference("deepseek-harness/profile"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::ComponentContext),
        activation_mode: CompositionActivationMode::NextSession,
        retraction_mode: RetractionMode::Live,
        provenance: vec![
            format!(
                "deepseek-ai/deepseek-harness@{}",
                DEEPSEEK_HARNESS_UPSTREAM_REVISION
            ),
            provenance.to_string(),
        ],
    }
}

fn surface(
    resource: &ResourceRef,
    kind: SurfaceKind,
    native_id: &str,
    owner_component: Option<&str>,
) -> SurfaceDescriptor {
    SurfaceDescriptor {
        resource: resource.clone(),
        kind,
        target_native_id: Some(native_id.into()),
        owner_component: owner_component.map(r),
    }
}

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).expect("DeepSeek Harness adapter uses static valid ResourceRefs")
}
