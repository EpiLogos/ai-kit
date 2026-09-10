//! Entity-aware disclosure (W10 V6): the composed continuity capability
//! that names the pasu participants present in the event's context.
//!
//! Descope law: this runs only when the active composition selected
//! `hook/continuity/entity-disclosure` — never ambient. The content is
//! bounded and factual, compiled from the authored identity/profile/agent-set
//! carriers (W10 V3 materialisation) as partitioned by the world binding
//! (W10 V5): durable entity facts only. Volatile state — who is the current
//! actor, who is available right now — stays in NOW/Flow and Frames, never
//! here and never in persistent nodes (W10 rev 4 §3).
//!
//! Fail-open: every failure downgrades to a warning on the decision; the
//! turn proceeds.

use std::path::{Path, PathBuf};

use aikit_adapters::central_world_sources::{bind_project_context, read_project_binding};
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_adapters::{central_entities, central_world_sources::WorldBinding};
use aikit_core::hooks::HookEvent;
use aikit_core::WikiObject;

use crate::temporal::process_central_root;

/// The disclosure block for this event's context, or `None` when there is
/// nothing to disclose (not a Central world, no entities).
pub fn entity_disclosure(event: &HookEvent) -> Result<Option<String>, String> {
    let central_root = process_central_root(event.cwd.as_deref());
    entity_disclosure_in(
        &SystemRunner::new(),
        central_root.as_deref(),
        event.cwd.as_deref(),
    )
}

/// The env-resolved form (test seam over [`process_central_root`]).
pub fn entity_disclosure_with<R: CommandRunner>(
    runner: &R,
    cwd: Option<&Path>,
) -> Result<Option<String>, String> {
    let central_root = process_central_root(cwd);
    entity_disclosure_in(runner, central_root.as_deref(), cwd)
}

/// The injectable core: render the participant disclosure for `cwd` as seen
/// from the Central world `central_root`.
pub fn entity_disclosure_in<R: CommandRunner>(
    runner: &R,
    central_root: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<Option<String>, String> {
    let (Some(central_root), Some(cwd)) = (central_root, cwd) else {
        return Ok(None);
    };
    if !central_root.join("Control/user/identity/manifest.json").is_file() {
        // No identity manifest: not an inhabited Central world.
        return Ok(None);
    }

    let mut objects = central_entities::materialise_central_entities(central_root).objects;
    let binding = project_binding(runner, central_root, cwd);
    if let Some(binding) = &binding {
        // Exclusions withhold; annotations ride along (same refs).
        let mut sink = Vec::new();
        bind_project_context(&mut objects, binding, &mut sink);
    }

    let mut lines = Vec::new();
    for object in &objects {
        let WikiObject::Node(node) = object else {
            continue;
        };
        let Some(pasu) = node.extensions.get(central_entities::PASU_EXTENSION) else {
            continue;
        };
        let form = pasu["form"].as_str().unwrap_or("unknown");
        let subject = pasu["subject_ref"].as_str().unwrap_or("unknown");
        let extra = &pasu["extra"];
        match form {
            "nara" => {
                let revision = extra["manifest_revision"].as_str().unwrap_or("unversioned");
                let sourced = extra["sourced"].as_array().map(Vec::len).unwrap_or(0);
                lines.push(format!(
                    "- nara: {subject} — identity manifest revision {revision}, {sourced} sourced files"
                ));
            }
            "agent" => {
                let profiles = extra["profiles"].as_array().map(Vec::len).unwrap_or(0);
                lines.push(format!("- agent: {subject} — {profiles} profile(s)"));
            }
            "agent-set" => {
                let members = extra["members"].as_array().map(Vec::len).unwrap_or(0);
                lines.push(format!("- agent-set: {subject} — {members} authored member(s)"));
            }
            other => lines.push(format!("- {other}: {subject}")),
        }
        if lines.len() >= 12 {
            lines.push("- … further participants withheld by the disclosure budget".into());
            break;
        }
    }
    if lines.is_empty() {
        return Ok(None);
    }
    if let Some(binding) = binding {
        lines.push(format!(
            "- context binding: {} ({} source(s) effective{})",
            binding.world_ref,
            binding.sources.iter().filter(|s| s.state == "available").count(),
            if binding.inherited_root_lineage {
                ", root lineage by convention"
            } else {
                ""
            }
        ));
    }
    Ok(Some(format!(
        "[continuity/entity-disclosure] participants present in this context (composed):\n{}",
        lines.join("\n")
    )))
}

/// The world binding for the event's context, or None (fail-open: a failed
/// ctrl call degrades to uncontextualised disclosure, never to an error).
fn project_binding<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    cwd: &Path,
) -> Option<WorldBinding> {
    let project = cwd.strip_prefix(central_root).ok().and_then(|relative| {
        let mut parts = relative.components();
        if parts.next()?.as_os_str() != "Work" {
            return None;
        }
        parts.next()?.as_os_str().to_str().map(str::to_owned)
    })?;
    let executable = std::env::var_os("CENTRAL_CTRL_BIN")
        .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ctrl"));
    let mut absences = Vec::new();
    read_project_binding(runner, &executable, central_root, &project, &mut absences)
}
