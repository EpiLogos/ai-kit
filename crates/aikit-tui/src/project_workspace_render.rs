//! Read-only Project-world presentation for the V2 Workspace.
//!
//! This module formats [`ProjectWorldReadModel`] into human-facing Workspace
//! lines. It owns no resolver, selection, retrieval or mutation state: live
//! selection remains [`TuiState::selected`], ContextSource retrieval remains an
//! explicit provider operation, and durable composition remains the existing
//! staging -> preview -> confirm -> apply path.

use aikit_core::context_resolution::Availability;
use aikit_core::project::ProjectBindingLocator;
use aikit_core::resource::{Eligibility, SourceAuthority};
use aikit_core::{ContextSourceHit, ProjectWorldReadModel, ProjectWorldResource};

use crate::application::{TuiState, WorkspaceSection};

/// Section-specific Project-world lines. Empty means the generic selected-resource
/// preview remains the better presentation for this Workspace section.
pub fn project_world_lines(state: &TuiState, world: &ProjectWorldReadModel) -> Vec<String> {
    match state.workspace_section {
        WorkspaceSection::Projects => project_lines(world),
        WorkspaceSection::Compose => compose_lines(state, world),
        WorkspaceSection::Projection => projection_lines(state, world),
        WorkspaceSection::Explore | WorkspaceSection::History => Vec::new(),
    }
}

fn project_lines(world: &ProjectWorldReadModel) -> Vec<String> {
    let mut lines = vec![
        "Project world".into(),
        String::new(),
        format!("Project  {}", world.project.project.as_str()),
        format!("Binding  {}", locator_label(&world.project.locator)),
    ];

    if let Some(root) = world.context.project_root.as_ref() {
        lines.push(format!("Root     {}", root.display()));
    }
    if let Some(focus) = world.context.task.as_ref() {
        lines.push(format!("Focus    {focus}"));
    }
    lines.push(format!("Host     {}", world.context.host));

    let profiles = world
        .resolution_basis
        .profiles
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    lines.push(format!(
        "Profiles {}",
        if profiles.is_empty() {
            "none disclosed".into()
        } else {
            profiles.join(", ")
        }
    ));

    if world.resolution_basis.scopes.is_empty() {
        lines.push("Scopes   not exposed by compatibility service".into());
    } else {
        lines.push(format!(
            "Scopes   {}",
            world
                .resolution_basis
                .scopes
                .iter()
                .map(|scope| format!("{}:{}", scope.kind.as_str(), scope.origin))
                .collect::<Vec<_>>()
                .join(" -> ")
        ));
    }

    lines.push(String::new());
    lines.push(format!(
        "Revision catalog {} · resolution {}{}",
        world.effective_revision.catalog_revision,
        world.effective_revision.resolution_hash,
        world
            .effective_revision
            .generation
            .as_ref()
            .map(|generation| format!(" · generation {generation}"))
            .unwrap_or_default(),
    ));
    if let Some(warning) = world.warnings.first() {
        lines.push(format!("Boundary {warning}"));
    }
    lines
}

fn compose_lines(state: &TuiState, world: &ProjectWorldReadModel) -> Vec<String> {
    let actor_runtime_count = usize::from(world.actor_runtime.agent.effective.is_some())
        + usize::from(world.actor_runtime.agency.effective.is_some())
        + usize::from(world.actor_runtime.host.effective.is_some())
        + world.actor_runtime.models.len()
        + world.actor_runtime.harnesses.len()
        + world.actor_runtime.execution_offers.len();
    let mut lines = vec![
        "Compose · resolved Project world".into(),
        String::new(),
        format!(
            "Capabilities  {} capabilities · {} actions",
            world.capability_horizon.capabilities.len(),
            world.capability_horizon.actions.len(),
        ),
        format!(
            "Information   {} visible sources · {} planned retrievals",
            world.information_horizon.sources.len(),
            world.information_horizon.planned_retrieval.len(),
        ),
        format!("Actor/Runtime {actor_runtime_count} effective or candidate resources"),
        format!(
            "Projection    {} targets · {} effective capabilities",
            world.projection.targets.len(),
            world.projection.active_capabilities.len(),
        ),
    ];

    if let Some(selected) = state.selected.as_ref() {
        if let Some(resource) = selected_world_resource(world, selected) {
            lines.push(String::new());
            lines.extend(resource_lines(resource));
        } else if let Some(source) = world
            .information_horizon
            .sources
            .iter()
            .find(|source| &source.resource == selected)
        {
            lines.push(String::new());
            lines.extend(context_source_lines(source));
        }
    }

    if !state.staged.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} staged change{} · durable mutation still uses preview -> confirm -> apply",
            state.staged.len(),
            if state.staged.len() == 1 { "" } else { "s" },
        ));
    }
    lines
}

fn projection_lines(state: &TuiState, world: &ProjectWorldReadModel) -> Vec<String> {
    let targets = world
        .projection
        .targets
        .iter()
        .map(|target| target.as_str())
        .collect::<Vec<_>>();
    let mut lines = vec![
        "Projection".into(),
        String::new(),
        format!(
            "Targets       {}",
            if targets.is_empty() {
                "none resolved".into()
            } else {
                targets.join(", ")
            }
        ),
        format!(
            "Effective     {} capability{}",
            world.projection.active_capabilities.len(),
            if world.projection.active_capabilities.len() == 1 {
                ""
            } else {
                "s"
            },
        ),
        format!(
            "Generation    {}",
            world
                .effective_revision
                .generation
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "not materialised in this read model".into()),
        ),
        format!("Catalog       {}", world.effective_revision.catalog_revision),
        format!("Resolution    {}", world.effective_revision.resolution_hash),
    ];
    if !state.staged.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} staged change{} await composition preview",
            state.staged.len(),
            if state.staged.len() == 1 { "" } else { "s" },
        ));
    }
    lines
}

fn selected_world_resource<'a>(
    world: &'a ProjectWorldReadModel,
    selected: &aikit_core::resource::ResourceRef,
) -> Option<&'a ProjectWorldResource> {
    world
        .capability_horizon
        .capabilities
        .iter()
        .chain(world.capability_horizon.actions.iter())
        .chain(world.information_horizon.resolved_sources.iter())
        .chain(world.actor_runtime.models.iter())
        .chain(world.actor_runtime.harnesses.iter())
        .chain(world.actor_runtime.execution_offers.iter())
        .chain(world.actor_runtime.agent.effective.iter())
        .chain(world.actor_runtime.agency.effective.iter())
        .chain(world.actor_runtime.host.effective.iter())
        .find(|resource| &resource.resource == selected)
}

fn resource_lines(resource: &ProjectWorldResource) -> Vec<String> {
    let preference = resource
        .intent
        .preference
        .as_ref()
        .map(|preference| format!("preferred rank {} via {}", preference.rank, preference.source))
        .unwrap_or_else(|| "no authored preference".into());
    let authorities = resource
        .intent
        .sources
        .iter()
        .filter_map(|source| source.authority)
        .map(authority_label)
        .collect::<Vec<_>>();
    vec![
        format!("{} · {}", resource.name, resource.kind.as_str()),
        resource.resource.as_str().to_string(),
        format!(
            "Intent        {} · {}{}",
            eligibility_label(&resource.intent.eligibility),
            preference,
            if authorities.is_empty() {
                String::new()
            } else {
                format!(" · provenance {}", authorities.join(", "))
            },
        ),
        format!(
            "Effective     {} · {} provider{}",
            availability_label(&resource.effective.availability),
            resource.effective.providers.len(),
            if resource.effective.providers.len() == 1 {
                ""
            } else {
                "s"
            },
        ),
    ]
}

fn context_source_lines(source: &ContextSourceHit) -> Vec<String> {
    vec![
        format!("{} · context-source", source.name),
        source.resource.as_str().to_string(),
        format!(
            "Disclosure    exists={} · known={} · askable={} · retrieved={} · focused={}",
            source.disclosure.exists,
            source.disclosure.known_to_exist,
            source.disclosure.askable,
            source.disclosure.retrieved,
            source.disclosure.focused,
        ),
        format!("Effective     {}", availability_label(&source.availability)),
        "Selection is descriptor-only; retrieval remains an explicit Action.".into(),
    ]
}

fn locator_label(locator: &ProjectBindingLocator) -> String {
    match locator {
        ProjectBindingLocator::LocalDirectory { path } => format!("local {}", path.display()),
        ProjectBindingLocator::Repository { repository } => format!("repository {repository}"),
        ProjectBindingLocator::Remote { locator } => format!("remote {locator}"),
    }
}

fn eligibility_label(eligibility: &Eligibility) -> &'static str {
    match eligibility {
        Eligibility::Eligible => "eligible",
        Eligibility::Undetermined => "eligibility unresolved",
        Eligibility::Ineligible { .. } => "ineligible",
    }
}

fn availability_label(availability: &Availability) -> &'static str {
    match availability {
        Availability::Available => "available",
        Availability::Unresolved { .. } => "availability unresolved",
        Availability::Unavailable { .. } => "unavailable",
    }
}

fn authority_label(authority: SourceAuthority) -> &'static str {
    match authority {
        SourceAuthority::Authored => "authored",
        SourceAuthority::Observed => "observed",
        SourceAuthority::Derived => "derived",
        SourceAuthority::Learned => "learned",
        SourceAuthority::Generated => "generated",
    }
}
