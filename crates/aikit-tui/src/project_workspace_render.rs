//! Read-only Project-world presentation for the V2 Workspace.
//!
//! This module formats [`ProjectWorldReadModel`] into human-facing Workspace
//! lines. It owns no resolver, selection, retrieval or mutation state: live
//! selection remains [`TuiState::selected`], ContextSource retrieval remains an
//! explicit provider operation, and durable composition remains the existing
//! staging -> preview -> confirm -> apply path.
//!
//! The public Workspace field is Search / Worlds / Compose / Work / Knowledge /
//! History / System — spec `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §3-§8
//! (carried unmerged on PR #212 as of this writing). Explain is deliberately
//! not a section here: spec §17 retires it as a top-level destination in
//! favour of an Inspector overlay reached through the `:` Explain contextual
//! Action (see `Overlay::Explain` in `crate::v2_render`, which now renders
//! `explain_lines`' content alongside the provider Explain evidence rather
//! than losing it).

use aikit_core::context_resolution::Availability;
use aikit_core::project::ProjectBindingLocator;
use aikit_core::resource::{Eligibility, SourceAuthority};
use aikit_core::{ContextSourceHit, ProjectWorldReadModel, ProjectWorldResource};

use crate::application::{TuiState, WorkspaceSection};
use crate::layout::Glyphs;

/// Canonical product label for each Workspace slot.
///
/// Search is the universal query field and therefore does not need its own
/// `WorkspaceSection`; the six section slots complete the canonical field as
/// Worlds / Compose / Work / Knowledge / History / System.
pub fn workspace_section_label(section: WorkspaceSection) -> &'static str {
    match section {
        WorkspaceSection::Worlds => "Worlds",
        WorkspaceSection::Compose => "Compose",
        WorkspaceSection::Work => "Work",
        WorkspaceSection::Knowledge => "Knowledge",
        WorkspaceSection::History => "History",
        WorkspaceSection::System => "System",
    }
}

/// Section-specific Project-world lines. Empty means another canonical read model
/// (currently Knowledge relations) owns the presentation for this section.
pub fn project_world_lines(
    state: &TuiState,
    world: &ProjectWorldReadModel,
    glyphs: Glyphs,
) -> Vec<String> {
    match state.workspace_section {
        WorkspaceSection::Worlds => context_lines(world, glyphs),
        WorkspaceSection::Compose => compose_lines(state, world, glyphs),
        WorkspaceSection::Work => work_lines(state, world, glyphs),
        WorkspaceSection::History => history_lines(world, glyphs),
        WorkspaceSection::System => system_lines(world, glyphs),
        WorkspaceSection::Knowledge => Vec::new(),
    }
}

fn context_lines(world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("Context {sep} resolved Project world"),
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
        lines.push("Scopes   not exposed by application boundary".into());
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
        "Revision catalog {} {sep} resolution {}{}",
        world.effective_revision.catalog_revision,
        world.effective_revision.resolution_hash,
        world
            .effective_revision
            .generation
            .as_ref()
            .map(|generation| format!(" {sep} generation {generation}"))
            .unwrap_or_default(),
    ));
    for warning in &world.warnings {
        lines.push(format!("Boundary {warning}"));
    }
    lines
}

fn compose_lines(state: &TuiState, world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let actor_runtime_count = usize::from(world.actor_runtime.agent.effective.is_some())
        + usize::from(world.actor_runtime.agency.effective.is_some())
        + usize::from(world.actor_runtime.host.effective.is_some())
        + world.actor_runtime.models.len()
        + world.actor_runtime.harnesses.len()
        + world.actor_runtime.execution_offers.len();
    let mut lines = vec![
        format!("Compose {sep} resolved Project world"),
        String::new(),
        format!(
            "Capabilities  {} capabilities {sep} {} actions",
            world.capability_horizon.capabilities.len(),
            world.capability_horizon.actions.len(),
        ),
        format!(
            "Information   {} visible sources {sep} {} planned retrievals",
            world.information_horizon.sources.len(),
            world.information_horizon.planned_retrieval.len(),
        ),
        format!("Actor/Runtime {actor_runtime_count} effective or candidate resources"),
        format!(
            "Projection    {} targets {sep} {} effective capabilities",
            world.projection.targets.len(),
            world.projection.active_capabilities.len(),
        ),
    ];

    if let Some(agent) = world.actor_runtime.agent.effective.as_ref() {
        lines.push(format!("Agent         {}", agent.resource));
    }
    if let Some(agency) = world.actor_runtime.agency.effective.as_ref() {
        lines.push(format!("Agency        {}", agency.resource));
    }
    for harness in &world.actor_runtime.harnesses {
        lines.push(format!("Harness       {}", harness.resource));
    }
    for model in &world.actor_runtime.models {
        lines.push(format!("Model         {}", model.resource));
    }
    for offer in &world.actor_runtime.execution_offers {
        lines.push(format!("Execution     {}", offer.resource));
    }

    if let Some(selected) = state.selected.as_ref() {
        if let Some(resource) = selected_world_resource(world, selected) {
            lines.push(String::new());
            lines.extend(resource_lines(resource, glyphs));
        } else if let Some(source) = world
            .information_horizon
            .sources
            .iter()
            .find(|source| &source.resource == selected)
        {
            lines.push(String::new());
            lines.extend(context_source_lines(source, glyphs));
        }
    }

    if !state.staged.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} staged change{} {sep} preview -> explain -> confirm -> apply",
            state.staged.len(),
            if state.staged.len() == 1 { "" } else { "s" },
        ));
    }
    lines
}

/// Work's human question (spec §6) is "what is actually running", distinct
/// from Compose's "what could I build". Grounded in real, already-resolved
/// `actor_runtime` facts (the same facts `compose_lines` already folds in) —
/// this section is honest-minimal rather than fabricated: the fuller §6.3
/// Active-Work (DIRECT/FACTORY/ATTENTION) dashboard needs Factory Journey/Run
/// read-model plumbing this application boundary does not expose yet, so this
/// says so plainly instead of inventing rows, reusing this codebase's own
/// established "not exposed by application boundary" disclosure idiom
/// (`context_lines`'s `Scopes` row).
fn work_lines(state: &TuiState, world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![format!("Work {sep} what is actually running"), String::new()];
    let mut any_runtime = false;
    if let Some(agent) = world.actor_runtime.agent.effective.as_ref() {
        lines.push(format!("Agent         {}", agent.resource));
        any_runtime = true;
    }
    if let Some(agency) = world.actor_runtime.agency.effective.as_ref() {
        lines.push(format!("Agency        {}", agency.resource));
        any_runtime = true;
    }
    if let Some(host) = world.actor_runtime.host.effective.as_ref() {
        lines.push(format!("Host          {}", host.resource));
        any_runtime = true;
    }
    for harness in &world.actor_runtime.harnesses {
        lines.push(format!("Harness       {}", harness.resource));
        any_runtime = true;
    }
    for model in &world.actor_runtime.models {
        lines.push(format!("Model         {}", model.resource));
        any_runtime = true;
    }
    for offer in &world.actor_runtime.execution_offers {
        lines.push(format!("Execution     {}", offer.resource));
        any_runtime = true;
    }
    if !any_runtime {
        lines.push("Runtime       no effective Actor/Runtime resource in this world".into());
    }

    if let Some(selected) = state.selected.as_ref() {
        if let Some(resource) = selected_world_resource(world, selected) {
            lines.push(String::new());
            lines.extend(resource_lines(resource, glyphs));
        }
    }

    lines.push(String::new());
    lines.push(
        "Direct Session  reachable through Search (kind session-space)".into(),
    );
    lines.push("Factory work    not exposed by application boundary".into());
    lines
}

/// System's human question (spec §11) is "what does this installation depend
/// on". `crate::credential_surface::CredentialSetupView` is real, tested
/// product code, but `ProjectWorldReadModel`/`ContextResolution` carry no
/// credential field today, so a live System tab cannot honestly show real
/// credential rows without new provider plumbing (see the PR body's owner-gap
/// note). This is therefore honest-minimal, matching `work_lines`' idiom
/// exactly rather than fabricating a dashboard: it names System as a real,
/// Ctrl+K-navigable destination and says plainly what is not yet disclosed.
fn system_lines(world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    vec![
        format!("System {sep} installation and provider disclosure"),
        String::new(),
        "Credentials   not exposed by application boundary".into(),
        "Providers     not exposed by application boundary".into(),
        "Adapters      not exposed by application boundary".into(),
        "Workcell      not exposed by application boundary".into(),
        String::new(),
        format!(
            "Revision      catalog {} {sep} resolution {}",
            world.effective_revision.catalog_revision, world.effective_revision.resolution_hash,
        ),
    ]
}

/// Selected-resource resolved intent/effective-state lines. No longer reached
/// as a Workspace tab (`WorkspaceSection::Projection` is retired, see this
/// module's own doc comment) — `crate::v2_render`'s `Overlay::Explain` branch
/// calls this directly, alongside the provider Explain evidence, when a
/// Project world is available.
pub fn explain_lines(state: &TuiState, world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("Explain {sep} authored intent and effective state"),
        String::new(),
    ];
    let Some(selected) = state.selected.as_ref() else {
        lines.push("Select a Resource to inspect its resolved intent/effective state.".into());
        lines.push(format!("Resolution {}", world.effective_revision.resolution_hash));
        return lines;
    };

    lines.push(format!("Resource       {selected}"));
    if let Some(resource) = selected_world_resource(world, selected) {
        lines.extend(resource_lines(resource, glyphs));
    } else if let Some(source) = world
        .information_horizon
        .sources
        .iter()
        .find(|source| &source.resource == selected)
    {
        lines.extend(context_source_lines(source, glyphs));
    } else {
        lines.push("No Project-world resolution record for this shallow navigation Resource.".into());
        lines.push("Use the contextual Explain Action for provider-specific detail.".into());
    }

    lines.push(String::new());
    lines.push(format!("Catalog        {}", world.effective_revision.catalog_revision));
    lines.push(format!("Resolution     {}", world.effective_revision.resolution_hash));
    lines.push(format!(
        "Generation     {}",
        world
            .effective_revision
            .generation
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "not materialised".into()),
    ));
    for warning in &world.warnings {
        lines.push(format!("Boundary       {warning}"));
    }
    lines
}

fn history_lines(world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let mut lines = vec![
        format!("History {} effective world lineage", glyphs.separator()),
        String::new(),
        format!("Catalog revision  {}", world.effective_revision.catalog_revision),
        format!("Resolution hash   {}", world.effective_revision.resolution_hash),
        format!(
            "Generation        {}",
            world
                .effective_revision
                .generation
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "none in this read model".into()),
        ),
        format!("Active projection {} capabilities", world.projection.active_capabilities.len()),
    ];
    if world.warnings.is_empty() {
        lines.push("Boundary          no degraded context disclosures".into());
    } else {
        for warning in &world.warnings {
            lines.push(format!("Boundary          {warning}"));
        }
    }
    lines.push(String::new());
    lines.push("Recent/familiar/route history remains application evidence, not a second resolver.".into());
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

fn resource_lines(resource: &ProjectWorldResource, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
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
        format!("{} {sep} {}", resource.name, resource.kind.as_str()),
        resource.resource.as_str().to_string(),
        format!(
            "Intent        {} {sep} {}{}",
            eligibility_label(&resource.intent.eligibility),
            preference,
            if authorities.is_empty() {
                String::new()
            } else {
                format!(" {sep} provenance {}", authorities.join(", "))
            },
        ),
        format!(
            "Effective     {} {sep} {} provider{}",
            availability_label(&resource.effective.availability),
            resource.effective.providers.len(),
            if resource.effective.providers.len() == 1 { "" } else { "s" },
        ),
    ]
}

fn context_source_lines(source: &ContextSourceHit, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    vec![
        format!("{} {sep} context-source", source.name),
        source.resource.as_str().to_string(),
        format!(
            "Disclosure    exists={} {sep} known={} {sep} askable={} {sep} retrieved={} {sep} focused={}",
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
