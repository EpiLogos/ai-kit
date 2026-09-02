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

use aikit_adapters::clients::{
    broker::BrokerAdapter, claude::ClaudeAdapter, codex::CodexAdapter, ClientAdapter,
};

use crate::app::Service;

/// The clients AIKit can install for, and where each keeps its configuration.
fn adapter_for(service: &Service, client: &str) -> Result<(Box<dyn ClientAdapter>, PathBuf)> {
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
        "claude" | "claude-code" => {
            Ok((Box::new(ClaudeAdapter::new(ctx_dir)), home.join(".claude")))
        }
        "codex" => Ok((Box::new(CodexAdapter::new(tree)), home.join(".codex"))),
        "broker" => Ok((Box::new(BrokerAdapter::new()), home.join(".aikit"))),
        other => Err(AikitError::new(
            "client.unknown",
            format!("`{other}` is not a client AIKit knows; try claude, codex or broker"),
        )
        .with("client", other.to_string())),
    }
}

/// Plan the install as a Procedure.
pub fn plan_install(service: &Service, client: &str) -> Result<Procedure> {
    let (adapter, config_dir) = adapter_for(service, client)?;
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
            other => return Err(AikitError::new(
                "client.unexpected_install_item",
                format!(
                    "the {client} adapter asked for an install item AIKit cannot stage: {other:?}"
                ),
            )),
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
    let (adapter, _) = adapter_for(service, client)?;
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
    for client in ["claude", "codex", "broker"] {
        if only.is_some_and(|o| o != client && !(o == "claude-code" && client == "claude")) {
            continue;
        }
        let (adapter, config_dir) = adapter_for(service, client)?;
        let planned = adapter.plan(&rc);
        let semantic_items = match client {
            "claude" | "codex" => rc.view.active_of_kind(Kind::Skill).len(),
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
            "notes": planned.as_ref().ok().map(|p| p.notes.clone()).unwrap_or_default(),
            "error": planned.as_ref().err().map(|e| e.message().to_string()),
        }));
    }
    Ok(rows)
}
