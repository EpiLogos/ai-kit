//! Managed actor-bootstrap projection shared by client adapters.
//!
//! The same resolved [`ActorBootstrap`] is rendered into the portable Agent Skill
//! form both Claude and Codex already support. Target adapters decide whether
//! their native skill surface is sufficiently isolated to receive it; this module
//! never makes that policy decision.

use std::path::Path;

use aikit_core::actor_bootstrap::{ActorBootstrap, BootstrapReference};
use aikit_core::projection::ProjectionItem;
use aikit_core::Result;

pub const MANAGED_BOOTSTRAP_SKILL: &str = "aikit-context";

pub fn managed_bootstrap_item(
    prefix: &Path,
    bootstrap: &ActorBootstrap,
) -> Result<ProjectionItem> {
    ProjectionItem::write(
        prefix.join(MANAGED_BOOTSTRAP_SKILL).join("SKILL.md"),
        render_managed_bootstrap(bootstrap),
    )
}

pub fn render_managed_bootstrap(bootstrap: &ActorBootstrap) -> String {
    let mut body = String::from(
        "---\n\
         name: aikit-context\n\
         description: \"Orient into the AIKit-resolved Project, actor, capability and information world for this session; inspect richer state only when needed.\"\n\
         ---\n\n\
         # AIKit resolved world\n\n\
         This is a **managed bootstrap**, not Project canon and not a dump of the runtime body. \
         AIKit has already resolved the identities and operating relations below. Preserve those \
         identities while using AIKit's search, Explain, History, capability and context faculties \
         to retrieve deeper detail according to present need.\n\n\
         Keep these distinctions explicit: what exists; what is known to exist; what is askable; \
         what has been retrieved; what is active in the present runtime body; and what is currently \
         in Focus. Do not infer one state from another.\n\n",
    );

    body.push_str(&format!("- Bootstrap: `{}`\n", bootstrap.version));
    body.push_str(&format!("- Project: `{}`\n", bootstrap.project.project));
    if let Some(run) = &bootstrap.run {
        body.push_str(&format!("- Run: `{run}`\n"));
    }
    if let Some(agent) = &bootstrap.agent {
        body.push_str(&format!("- Agent: {}\n", reference_label(agent)));
    }
    if let Some(agency) = &bootstrap.agency {
        body.push_str(&format!("- Agency: {}\n", reference_label(agency)));
    }
    if let Some(host) = &bootstrap.host {
        body.push_str(&format!("- Host: {}\n", reference_label(host)));
    }
    if let Some(harness) = &bootstrap.harness {
        body.push_str(&format!("- Harness: {}\n", reference_label(harness)));
    }
    if let Some(model) = &bootstrap.model {
        body.push_str(&format!("- Model: {}\n", reference_label(model)));
    }
    if let Some(session) = &bootstrap.agent_session {
        body.push_str(&format!("- AgentSession: `{session}`\n"));
    }

    body.push_str("\n## Horizons\n\n");
    body.push_str(&format!(
        "- Capabilities: {} total ({} available, {} unresolved, {} unavailable)\n",
        bootstrap.capabilities.total,
        bootstrap.capabilities.available,
        bootstrap.capabilities.unresolved,
        bootstrap.capabilities.unavailable,
    ));
    body.push_str(&format!(
        "- Actions: {} total ({} available, {} unresolved, {} unavailable)\n",
        bootstrap.actions.total,
        bootstrap.actions.available,
        bootstrap.actions.unresolved,
        bootstrap.actions.unavailable,
    ));
    body.push_str(&format!(
        "- ContextSources: {} total ({} available, {} unresolved, {} unavailable)\n",
        bootstrap.context_sources.total,
        bootstrap.context_sources.available,
        bootstrap.context_sources.unresolved,
        bootstrap.context_sources.unavailable,
    ));
    if !bootstrap.projection_targets.is_empty() {
        body.push_str(&format!(
            "- Projection targets: {}\n",
            bootstrap
                .projection_targets
                .iter()
                .map(|target| format!("`{}`", target.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if let Some(runtime) = &bootstrap.runtime_body {
        body.push_str("\n## Runtime body\n\n");
        body.push_str(&format!("- Harness: `{}`\n", runtime.harness));
        body.push_str(&format!("- Composition fingerprint: `{}`\n", runtime.fingerprint));
        body.push_str(&format!("- State: `{:?}`\n", runtime.state));
        if let Some(revision) = &runtime.target_revision {
            body.push_str(&format!("- Target revision: `{revision}`\n"));
        }
        if let Some(generation) = &runtime.generation {
            body.push_str(&format!("- Generation: `{generation}`\n"));
        }
        body.push_str(&format!(
            "- Body summary: {} Components · {} Contract bindings · {} Contributions · {} Surfaces · {} explicit absences\n",
            runtime.component_count,
            runtime.contract_binding_count,
            runtime.contribution_count,
            runtime.surface_count,
            runtime.absence_count,
        ));
        body.push_str(
            "- Inspect the body through AIKit Component Explain / composition History rather than loading the Component graph into standing context.\n",
        );
    }

    if !bootstrap.warnings.is_empty() {
        body.push_str("\n## Resolution warnings\n\n");
        for warning in &bootstrap.warnings {
            body.push_str(&format!("- {warning}\n"));
        }
    }

    body.push_str(
        "\n## Operating rule\n\n\
         Search broadly, disclose progressively, and retrieve payloads late. A Resource being \
         addressable does not mean it is loaded; a Component being available does not mean it is \
         mounted; a projected Surface does not change the canonical identity it represents. Re-orient \
         through AIKit when Project, session, model, harness, Generation, or runtime-body state changes.\n",
    );
    body
}

fn reference_label(reference: &BootstrapReference) -> String {
    match reference {
        BootstrapReference::Resolved {
            resource,
            availability,
            ..
        } => format!("`{resource}` ({availability:?})"),
        BootstrapReference::Missing {
            reference,
            expected,
        } => format!("`{reference}` (missing; expected {})", expected.as_str()),
        BootstrapReference::WrongKind {
            reference,
            expected,
            actual,
        } => format!(
            "`{reference}` (wrong kind: {}; expected {})",
            actual.as_str(),
            expected.as_str()
        ),
    }
}
