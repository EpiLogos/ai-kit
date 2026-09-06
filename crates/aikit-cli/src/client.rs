//! `aikit client install|launch|status` over the real client adapters.
//!
//! Installing a client's dispatcher entries edits files AIKit does not own —
//! `~/.claude/settings.json`, a Codex hooks file — so it is a **Procedure**:
//! planned, diffed, reversible. The adapters decide *what* the edit is (they know
//! each client's config shape); this module turns that into world edits with
//! inverses and hands them to the one engine.

use std::path::PathBuf;

use aikit_core::capsule::Kind;
use aikit_core::procedure::{Inverse, Plan, Procedure, ProcedureKind, WorldEdit};
use aikit_core::projection::ProjectionItem;
use aikit_core::{AikitError, Result};

use aikit_adapters::actuation_harness_capability::{
    intake_actuation_capability, CapabilityOutcome, HarnessCapability,
};
use aikit_adapters::clients::{
    broker::BrokerAdapter, claude::ClaudeAdapter, codex::CodexAdapter, zcode::ZcodeAdapter,
    ClientAdapter,
};
use aikit_adapters::runner::SystemRunner;

use crate::app::Service;

/// Where each client's dispatch wiring lands, decided the same way every time:
/// Actuation's capability descriptor declares the seam when it is reachable;
/// otherwise the row carries the disclosure and the legacy default path is
/// used for read models only. The broker is AIKit's own config home.
fn client_home(seam_path: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let expanded = seam_path
        .strip_prefix("~/")
        .map(|rest| home.join(rest))
        .unwrap_or_else(|| PathBuf::from(seam_path));
    expanded
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| {
            AikitError::new(
                "client.seam_has_no_directory",
                format!("the capability seam `{seam_path}` has no parent directory"),
            )
        })
}

/// Intake of one harness's capability descriptor: a descriptor, or a
/// disclosed unavailability — never a hard-coded substitute.
fn capability_for(client: &str, slug: &str) -> Result<HarnessCapability> {
    match intake_actuation_capability(&SystemRunner::new(), "actuation", slug) {
        CapabilityOutcome::Descriptor(capability) => Ok(*capability),
        CapabilityOutcome::Unavailable { reason } => Err(AikitError::new(
            "client.capability_unavailable",
            format!(
                "no capability descriptor for {slug}: AIKit installs only what Actuation \
                 declares the harness to be ({reason})"
            ),
        )
        .with("client", client.to_string())
        .with("harness", slug.to_string())),
    }
}

/// The adapter plus its configuration home. `capability` is `None` when
/// Actuation's descriptor is unreachable — readable, but not installable.
fn adapter_for(
    service: &Service,
    client: &str,
) -> Result<(Box<dyn ClientAdapter>, Option<HarnessCapability>, PathBuf)> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let ctx_dir = service.context_projection_root();
    let tree = service
        .descriptor()
        .project_root
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));

    match client {
        "claude" | "claude-code" => match capability_for(client, "claude-code") {
            Ok(capability) => {
                let config_dir = client_home(&capability.install_seam.config_path)?;
                Ok((
                    Box::new(ClaudeAdapter::new(ctx_dir).with_capability(capability.clone())),
                    Some(capability),
                    config_dir,
                ))
            }
            Err(_) => Ok((
                Box::new(ClaudeAdapter::new(ctx_dir)) as Box<dyn ClientAdapter>,
                None,
                home.join(".claude"),
            )),
        },
        "codex" => match capability_for(client, "codex") {
            Ok(capability) => {
                let config_dir = client_home(&capability.install_seam.config_path)?;
                Ok((
                    Box::new(CodexAdapter::new(tree).with_capability(capability.clone())),
                    Some(capability),
                    config_dir,
                ))
            }
            Err(_) => Ok((
                Box::new(CodexAdapter::new(tree)) as Box<dyn ClientAdapter>,
                None,
                home.join(".codex"),
            )),
        },
        "zcode" => match capability_for(client, "zcode") {
            Ok(capability) => {
                let config_dir = client_home(&capability.install_seam.config_path)?;
                Ok((
                    Box::new(ZcodeAdapter::new().with_capability(capability.clone())),
                    Some(capability),
                    config_dir,
                ))
            }
            Err(_) => Ok((
                Box::new(ZcodeAdapter::new()) as Box<dyn ClientAdapter>,
                None,
                home.join(".zcode/cli"),
            )),
        },
        "broker" => Ok((Box::new(BrokerAdapter::new()), None, home.join(".aikit"))),
        other => Err(AikitError::new(
            "client.unknown",
            format!(
                "`{other}` is not a client AIKit knows; try claude, codex, zcode or broker"
            ),
        )
        .with("client", other.to_string())),
    }
}

/// Plan the install as a Procedure.
pub fn plan_install(service: &Service, client: &str) -> Result<Procedure> {
    let (adapter, capability, config_dir) = adapter_for(service, client)?;
    if capability.is_none() && client != "broker" {
        return Err(AikitError::new(
            "client.capability_unavailable",
            format!(
                "cannot install for {client}: Actuation's capability descriptor is unreachable, \
                 and AIKit installs only what Actuation declares the harness to be"
            ),
        )
        .with("client", client.to_string()));
    }
    let items = adapter.install(&config_dir)?;

    let mut plan = Plan::new().with_note(format!(
        "install AIKit's {client} integration into {}",
        config_dir.display()
    ));
    for item in items {
        match item {
            ProjectionItem::Write { path, contents } => {
                let target = config_dir.join(&path);
                // The adapter has already merged with whatever was there, so the
                // inverse is restoring the previous bytes — or removing the file
                // when there were none.
                let inverse = if target.exists() {
                    Inverse::Restore {
                        blob: aikit_core::procedure::BlobId::deferred(),
                    }
                } else {
                    Inverse::Remove
                };
                plan = plan.with_edit(WorldEdit::WriteFile {
                    path: target,
                    contents: contents.into_bytes(),
                    inverse,
                });
            }
            // An install emits configuration, never payload links.
            other => {
                return Err(AikitError::new(
                    "client.unexpected_install_item",
                    format!("the {client} adapter asked for an install item AIKit cannot stage: {other:?}"),
                ))
            }
        }
    }

    if plan.is_empty() {
        return Err(AikitError::new(
            "client.nothing_to_install",
            format!("the {client} adapter needs no durable configuration"),
        )
        .with("client", client.to_string()));
    }
    aikit_store::procedure::plan_procedure(
        service.home(),
        ProcedureKind::ClientInstall {
            client: aikit_core::TargetId::new(client),
        },
        plan,
    )
}

/// The argv that starts a client against this context's projection.
pub fn launch_command(service: &Service, client: &str) -> Result<Vec<String>> {
    let (adapter, _, _) = adapter_for(service, client)?;
    let rc = service.projection_context()?;
    let argv = adapter.launch_command(&rc);
    if argv.is_empty() {
        return Err(AikitError::new(
            "client.not_launchable",
            format!("{client} is reached through another client, so there is no command to run"),
        )
        .with("client", client.to_string()));
    }
    Ok(argv)
}

/// What each client's semantic projection would contain, the lower-level
/// materialisation work required to realise it, and whether the client is
/// installed.
///
/// `items` deliberately counts selected semantic resources, not filesystem
/// operations. A managed actor bootstrap can add a second generated projection
/// item for one selected Skill; reporting that as "2 items" makes a correctly
/// Skill-Set-filtered projection look as though it leaked another capability.
/// `materialization_items` exposes the adapter plan count separately for callers
/// interested in the physical work.
pub fn status(service: &Service, only: Option<&str>) -> Result<Vec<serde_json::Value>> {
    let rc = service.projection_context()?;
    let mut rows = Vec::new();
    for client in ["claude", "codex", "zcode", "broker"] {
        if only.is_some_and(|o| o != client && !(o == "claude-code" && client == "claude")) {
            continue;
        }
        let (adapter, capability, config_dir) = adapter_for(service, client)?;
        let planned = adapter.plan(&rc);
        let semantic_items = match client {
            "claude" | "codex" => rc.view.active_of_kind(Kind::Skill).len(),
            // No native skill projection for zcode yet: the count is honestly
            // zero, not the broker's whole active set.
            "zcode" => 0,
            "broker" => rc.view.active.len(),
            _ => unreachable!("client list and semantic count must evolve together"),
        };
        rows.push(serde_json::json!({
            "client": client,
            "config_dir": config_dir.display().to_string(),
            "installed": config_dir.exists(),
            "effect": planned.as_ref().ok().map(|p| adapter.activation_effect(None, p).describe()),
            "items": planned.as_ref().ok().map(|_| semantic_items),
            "materialization_items": planned.as_ref().ok().map(|p| p.items.len()),
            "actor_bootstrap": rc.actor_bootstrap.is_some(),
            "capability": if capability.is_some() { "descriptor" } else { "unavailable" },
            "notes": planned.as_ref().ok().map(|p| p.notes.clone()).unwrap_or_default(),
            "error": planned.as_ref().err().map(|e| e.message().to_string()),
        }));
    }
    Ok(rows)
}
