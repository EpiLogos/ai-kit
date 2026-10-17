//! `aikit saved` - what operations are saved, and what is waiting to be.
//!
//! A reading, never a store. Every field here is derived from something that
//! already owns it: the capsule catalogue (identity, revision, source, provenance),
//! the trust record (reviewed or not), the event log (use) and the capture inbox
//! (what is waiting). Nothing is written, so the reading cannot disagree with its
//! sources.
//!
//! `aikit search kind:script` says that a script exists, at which revision, and
//! whether it is reviewed - but not which commands it puts on the PATH, who saved
//! it, from what, when, in which registry, or how often it has been used (its
//! evidence line is prose, and its `source` is empty until the capsule is active).
//! `aikit praxis list` lists skills and methods only. `aikit capabilities list` is
//! the broker's tool list. This is the one place the question "what have I saved,
//! and does it get used?" has one answer.

use std::time::Duration;

use serde_json::Value;

use aikit_core::catalog::Catalog;
use aikit_core::{Capsule, Kind, Result};
use aikit_store::inbox::{CandidateState, Inbox};

use crate::app::Service;

/// Which saved operations to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    All,
    Script,
    Alias,
}

fn is_saved_operation(capsule: &Capsule) -> bool {
    matches!(capsule.kind, Kind::Script | Kind::Alias)
}

fn seconds(duration: Duration) -> u64 {
    duration.as_secs()
}

/// One saved operation, as a row.
fn row(service: &Service, capsule: &Capsule) -> Result<Value> {
    let usage = service.index().usage(&capsule.id)?;
    let entry = service.resolved().catalog_index.get(&capsule.id);
    let alias = capsule.alias();
    // What a person types: the command it puts on the PATH, or the `@name` it
    // answers to in search.
    let command = match alias {
        Some(alias) if capsule.is_operative_alias() => Some(format!("@{}", alias.name)),
        _ => capsule.exported_commands().into_iter().next(),
    };
    // Where it lives is the registry that carries it; "home" is the personal one,
    // "project" the repository's own.
    let registry = capsule.source.as_ref().map(|source| source.as_str());
    let scope = match registry {
        Some("project-local") => "project",
        _ => "home",
    };
    let provenance = &capsule.provenance;
    Ok(serde_json::json!({
        "id": capsule.id.to_string(),
        "kind": capsule.kind.as_str(),
        "syntax": alias.map(|alias| alias.syntax.as_str()),
        "name": capsule.name,
        "command": command,
        "exports": capsule.exported_commands(),
        "description": capsule.description,
        "registry": registry,
        "scope": scope,
        "manifest": capsule.root.as_ref().map(|root| root.join("manifest.toml").display().to_string()),
        "revision": capsule.revision.as_ref().map(ToString::to_string),
        "maturity": capsule.maturity.as_str(),
        "trust": entry.map(|entry| entry.trust.as_str()),
        "reviewed": entry.map(|entry| entry.trust.may_run_unattended()),
        "active": service.resolved().is_active(&capsule.id),
        "by": provenance.author,
        "saved_at": provenance.created_at,
        "origin": provenance.source.map(|source| match source {
            aikit_core::capsule::ProvenanceSource::Authored => "authored",
            aikit_core::capsule::ProvenanceSource::Harvested => "harvested",
            aikit_core::capsule::ProvenanceSource::Imported => "imported",
            aikit_core::capsule::ProvenanceSource::Generated => "generated",
        }),
        "source_ref": provenance.source_ref,
        "session": provenance.session_id,
        "candidate": provenance.source_event,
        "use": {
            "successful_runs": usage.successful_runs,
            "failed_runs": usage.failed_runs,
            "last_success_seconds_ago": usage.last_success_age.map(seconds),
        },
    }))
}

/// The reading: saved operations (most used first, then newest), and the
/// captures waiting to become some.
pub fn reading(service: &Service, which: Which, limit: usize) -> Result<Value> {
    let mut rows: Vec<(u32, String, Value)> = Vec::new();
    for capsule in service.snapshot().capsules() {
        if !is_saved_operation(capsule) {
            continue;
        }
        if (which == Which::Script && capsule.kind != Kind::Script)
            || (which == Which::Alias && capsule.kind != Kind::Alias)
        {
            continue;
        }
        let value = row(service, capsule)?;
        let runs = value["use"]["successful_runs"].as_u64().unwrap_or(0) as u32;
        let saved = capsule.provenance.created_at.clone().unwrap_or_default();
        rows.push((runs, saved, value));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

    let scripts = rows
        .iter()
        .filter(|(_, _, v)| v["kind"] == "script")
        .count();
    let shell = rows
        .iter()
        .filter(|(_, _, v)| v["syntax"] == "shell")
        .count();
    let operative = rows
        .iter()
        .filter(|(_, _, v)| v["syntax"] == "operative")
        .count();
    let ever_used = rows.iter().filter(|(runs, _, _)| *runs > 0).count();
    let total = rows.len();

    let inbox = Inbox::new(service.home(), service.index());
    let waiting: Vec<Value> = inbox
        .candidates()?
        .into_iter()
        .filter(|candidate| {
            matches!(
                candidate.state,
                CandidateState::Ready | CandidateState::Quarantined
            )
        })
        .filter(|candidate| matches!(candidate.kind, Kind::Script | Kind::Alias))
        .map(|candidate| {
            serde_json::json!({
                "candidate": candidate.id,
                "kind": candidate.kind.as_str(),
                "title": candidate.title,
                "state": candidate.state.as_str(),
                "confidence": candidate.confidence.as_str(),
                "exports": candidate.exports,
                "by": candidate.by,
                "source_ref": candidate.source_ref,
                "scope": candidate.scope.as_str(),
                "proposed_id": Inbox::proposed_id(&candidate).ok().map(|id| id.to_string()),
                "accept": format!("aikit inbox accept {}", candidate.id),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "saved": rows.into_iter().take(limit).map(|(_, _, value)| value).collect::<Vec<_>>(),
        "waiting": waiting,
        "summary": {
            "saved": total,
            "scripts": scripts,
            "aliases": { "shell": shell, "operative": operative },
            "ever_used": ever_used,
            "never_used": total - ever_used,
            "waiting": waiting.len(),
        },
        "standing": "a reading over the catalogue, the trust record, the event log and the capture inbox; it stores nothing",
    }))
}
