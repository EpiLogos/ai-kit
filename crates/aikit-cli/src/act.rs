//! The `act` doorway: contextual discovery, exact contracts, explicit invocation.
//!
//! One transport over the seams that already exist — no second registry:
//!
//! * discovery and description read the shared resource index
//!   ([`aikit_tui::project_world_service::resource_index`]) and the resolver's
//!   own catalogued view; every row's contract comes from the real descriptor,
//!   never from a hand-written catalogue;
//! * invocation goes through [`Service::run`], the same native runner
//!   `aikit run` and scoped invocation use, so policy, trust and the
//!   confirm gate apply identically.
//!
//! Search and discovery are inert: nothing here records familiarity,
//! invocation or trust events. Only [`invoke`] performs an effect, and it
//! resolves the ref first — an unknown or ambiguous ref fails before any
//! change is possible.

use aikit_core::id::CapsuleId;
use aikit_core::resource::{
    ResourceIndex, ResourceKind, ResourceRef, ResourceSource, SourceAuthority, SourceState,
};
use aikit_core::{AikitError, Result};
use aikit_tui::project_world_service;

use crate::app::{AikitApplication, RunRequest, Service};
use crate::cli::{ActDescribeArgs, ActDiscoverArgs, ActInvokeArgs};

/// The kebab-case standing of a source state. [`SourceState`] carries its
/// state in the serde tag, not a method, and the doorway never invents a
/// second vocabulary for it.
fn source_state_str(state: &SourceState) -> &'static str {
    match state {
        SourceState::Unresolved => "unresolved",
        SourceState::Available => "available",
        SourceState::Unavailable { .. } => "unavailable",
    }
}

/// The kebab-case standing of a source authority, matching the serialised form.
fn source_authority_str(authority: &SourceAuthority) -> &'static str {
    match authority {
        SourceAuthority::Authored => "authored",
        SourceAuthority::Observed => "observed",
        SourceAuthority::Derived => "derived",
        SourceAuthority::Learned => "learned",
        SourceAuthority::Generated => "generated",
    }
}

/// One source relation row: where a record comes from, with what authority and
/// revision, in what availability state — an unavailable reason is named, not
/// collapsed into the state word.
fn source_json(source: &ResourceSource) -> serde_json::Value {
    let mut row = serde_json::json!({
        "source": source.source.to_string(),
        "authority": source.authority.as_ref().map(source_authority_str),
        "state": source_state_str(&source.state),
    });
    if let Some(revision) = &source.revision {
        row["revision"] = serde_json::json!(revision);
    }
    if let SourceState::Unavailable { reason } = &source.state {
        row["reason"] = serde_json::Value::from(reason.clone());
    }
    row
}

/// `aikit act` / `aikit act discover` — what can be done here.
pub fn discover(service: &mut Service, args: ActDiscoverArgs) -> Result<serde_json::Value> {
    let index = project_world_service::resource_index(&*service)?;
    let mut rows: Vec<serde_json::Value> = Vec::new();

    if let Some(subject_text) = &args.subject {
        // Subject-scoped discovery: the canonical contextual Actions of one
        // selected Resource. An unknown subject is a named absence with the
        // route that would have found it — never an empty "no actions".
        let subject = ResourceRef::parse(subject_text)?;
        let record = ResourceIndex::resource(&index, &subject).ok_or_else(|| {
            AikitError::new(
                "act.subject_unknown",
                format!("{subject} is not present in the resolved resource field"),
            )
            .with("subject", subject.to_string())
            .with(
                "recovery",
                "find the exact ref with `aikit search <text>`, or widen the context with `aikit world status`",
            )
        })?;
        let mut actions = index.actions_for(&subject);
        actions.sort_by(|left, right| (&left.label, &left.action).cmp(&(&right.label, &right.action)));
        rows.reserve(actions.len());
        for action in actions {
            let mut row = action_row(&index, action);
            row["subject"] = serde_json::Value::from(record.descriptor.id.to_string());
            row["subject_name"] = serde_json::Value::from(record.descriptor.name.clone());
            rows.push(row);
        }
    } else {
        // Scope discovery: the canonical Action records themselves, ranked by
        // the shared search order (empty query keeps authored order), bounded.
        let query = args.query.as_deref().unwrap_or("");
        for hit in index.search(query, args.limit) {
            if hit.kind != ResourceKind::Action {
                continue;
            }
            let record = match ResourceIndex::resource(&index, &hit.resource) {
                Some(record) => record,
                None => continue,
            };
            let mut row = record_row(&record.descriptor, &index);
            row["availability"] = availability(service, &hit.resource);
            rows.push(row);
            if rows.len() >= args.limit {
                break;
            }
        }
        rows.truncate(args.limit);
    }

    Ok(serde_json::json!({
        "schema": "aikit.act-discovery/v1",
        "subject": args.subject,
        "query": args.query,
        "count": rows.len(),
        "inert": true,
        "rows": rows,
        "next": {
            "describe": "aikit act describe <ref>",
            "invoke": "aikit act invoke <ref> [--input <json>|@file]",
        },
    }))
}

/// One contextual Action relation row (a canonical Action on a subject).
fn action_row(
    index: &aikit_core::resource::ResourceSearchIndex,
    action: &aikit_core::resource::ContextualActionDescriptor,
) -> serde_json::Value {
    let record = ResourceIndex::resource(index, &action.action);
    let descriptor = record.map(|record| &record.descriptor);
    serde_json::json!({
        "ref": action.action.to_string(),
        "kind": "action",
        "label": action.label,
        "description": action.description,
        "stageability": action.stageability,
        "subjects": index.subjects_for_action(&action.action)
            .iter()
            .map(|contextual| contextual.subject.to_string())
            .collect::<Vec<_>>(),
        "owner": descriptor.and_then(|d| d.owner.as_ref()).map(|owner| owner.to_string()),
        "sources": descriptor.map(|d| d.sources.iter().map(source_json).collect::<Vec<_>>())
            .unwrap_or_default(),
        "routes": {
            "describe": format!("aikit act describe {}", action.action),
            "invoke": format!("aikit act invoke {}", action.action),
        },
    })
}

/// One canonical Action record row, with its subjects attached.
fn record_row(
    descriptor: &aikit_core::resource::ResourceDescriptor,
    index: &aikit_core::resource::ResourceSearchIndex,
) -> serde_json::Value {
    serde_json::json!({
        "ref": descriptor.id.to_string(),
        "kind": "action",
        "label": descriptor.name,
        "description": descriptor.description,
        "subjects": index.subjects_for_action(&descriptor.id)
            .iter()
            .map(|contextual| serde_json::json!({
                "subject": contextual.subject.to_string(),
                "label": contextual.label,
            }))
            .collect::<Vec<_>>(),
        "owner": descriptor.owner.as_ref().map(|owner| owner.to_string()),
        "sources": descriptor.sources.iter().map(source_json).collect::<Vec<_>>(),
        "routes": {
            "describe": format!("aikit act describe {}", descriptor.id),
            "invoke": format!("aikit act invoke {}", descriptor.id),
        },
    })
}

/// The resolver's own availability opinion for a ref: runnable now, or the
/// named reason it is not. Static catalogue presence is never confused with
/// installed eligibility.
fn availability(service: &Service, reference: &ResourceRef) -> serde_json::Value {
    let view = service.resolved();
    let Ok(id) = CapsuleId::parse(reference.as_str()) else {
        // Not a capability id: an Action relation is available where its
        // subjects are; the row carries them.
        return serde_json::json!({ "state": "contextual" });
    };
    let Some(entry) = view.catalog_index.get(&id) else {
        return serde_json::json!({
            "state": "unavailable",
            "reason": "not present in any registry",
            "recovery": "find the exact ref with `aikit search <text>`",
        });
    };
    if view.is_active(&id) {
        return serde_json::json!({ "state": "runnable", "kind": entry.kind.as_str() });
    }
    let reason = view
        .unavailable_reason(&id)
        .map(|reason| reason.describe())
        .unwrap_or_else(|| "catalogued, but no scope enables it in this context".into());
    serde_json::json!({
        "state": "unavailable",
        "reason": reason,
        "recovery": "inspect the resolution with `aikit explain <ref>`",
    })
}

/// `aikit act describe <ref>` — the exact input/output/effect contract, from
/// the real descriptor. Read-only.
pub fn describe(service: &Service, args: ActDescribeArgs) -> Result<serde_json::Value> {
    let reference = ResourceRef::parse(&args.reference)?;
    let index = project_world_service::resource_index(service)?;

    let mut document = serde_json::json!({
        "schema": "aikit.act-description/v1",
        "ref": reference.to_string(),
    });

    // The resource-field contract when the ref is present there.
    if let Some(record) = ResourceIndex::resource(&index, &reference) {
        let descriptor = &record.descriptor;
        document["kind"] = serde_json::Value::from(descriptor.kind.as_str());
        document["label"] = serde_json::Value::from(descriptor.name.clone());
        document["description"] = serde_json::Value::from(descriptor.description.clone());
        document["owner"] = descriptor
            .owner
            .as_ref()
            .map(|owner| serde_json::Value::from(owner.to_string()))
            .unwrap_or(serde_json::Value::Null);
        document["sources"] = serde_json::Value::from(
            descriptor.sources.iter().map(source_json).collect::<Vec<_>>(),
        );
        let return_forms = descriptor
            .annotations
            .get("action.expected-return-forms")
            .cloned();
        if let Some(forms) = return_forms {
            document["expected_return_forms"] = serde_json::Value::from(forms);
        }
        document["subjects"] = serde_json::Value::from(
            index
                .subjects_for_action(&reference)
                .iter()
                .map(|contextual| contextual.subject.to_string())
                .collect::<Vec<_>>(),
        );
    }

    // The capability contract when the ref resolves to a catalogued capsule:
    // the resolver's own standing and the exact invocation envelope.
    if let Ok(id) = CapsuleId::parse(reference.as_str()) {
        let view = service.resolved();
        if let Some(entry) = view.catalog_index.get(&id) {
            document["capability"] = serde_json::json!({
                "id": id.to_string(),
                "kind": entry.kind.as_str(),
                "name": entry.name,
                "description": entry.description,
                "revision": entry.revision.as_ref().map(|revision| revision.to_string()),
                "trust": entry.trust.as_str(),
                "active": view.is_active(&id),
                "declared_enabled": view.is_declared_enabled(&id),
                "unavailable_reason": view
                    .unavailable_reason(&id)
                    .map(|reason| reason.describe()),
                "runnable": view.can_run(&id),
            });
            document["input"] = serde_json::json!({
                "form": "argv",
                "contract": "positional arguments are passed through verbatim to the capability; `--input <json>|@file` contributes the JSON document as one argument",
                "confirm_required": entry.trust != aikit_core::TrustState::Trusted,
            });
            document["routes"] = serde_json::json!({
                "invoke": format!("aikit act invoke {reference} [--input <json>|@file]"),
                "explain": format!("aikit explain {reference}"),
            });
        }
    }

    if document.get("kind").is_none() && document.get("capability").is_none() {
        return Err(AikitError::new(
            "act.ref_unknown",
            format!("{reference} is not present in the resolved resource field or the capability catalogue"),
        )
        .with("ref", reference.to_string())
        .with(
            "recovery",
            "find the exact ref with `aikit search <text>` or `aikit act`; describe never guesses a contract",
        ));
    }
    Ok(document)
}

/// `aikit act invoke <ref>` — resolve, then perform once through the existing
/// native runner. The returned handle carries the real status and output;
/// unknown or ambiguous refs are refused before any effect.
pub fn invoke(
    service: &mut Service,
    args: ActInvokeArgs,
) -> Result<(crate::app::RunHandle, String)> {
    let input = match &args.input {
        Some(raw) => match raw.strip_prefix('@') {
            Some(path) => Some(
                std::fs::read_to_string(path).map_err(|error| {
                    AikitError::new(
                        "act.input_unreadable",
                        format!("could not read the action input from {path}: {error}"),
                    )
                })?,
            ),
            None => Some(raw.clone()),
        },
        None => None,
    };
    let mut argv = Vec::with_capacity(input.is_some() as usize + args.args.len());
    if let Some(input) = input {
        argv.push(input);
    }
    argv.extend(args.args.iter().cloned());

    // The same seam `aikit run` uses: resolution first (an unknown ref fails
    // here, before anything runs), then the trust gate, then one execution.
    let run = service.run(RunRequest {
        name: args.reference.clone(),
        args: argv,
        export: None,
        confirmed: args.confirm,
    });
    match run {
        Ok(run) => {
            let digest = crate::scoped_invocation::run_result_digest(&run);
            Ok((run, digest))
        }
        Err(error) => {
            let code = error.code().to_owned();
            let recovery = match code.as_str() {
                "run.unknown_command" => Some(format!(
                    "find the exact ref with `aikit search <text>` or `aikit act`; `{}` was not resolved and nothing ran",
                    args.reference
                )),
                "trust.required" => Some(format!(
                    "re-run with --confirm once you accept the risk: `aikit act invoke {} --confirm`",
                    args.reference
                )),
                _ => None,
            };
            match recovery {
                Some(recovery) => Err(error.with("recovery", recovery)),
                None => Err(error),
            }
        }
    }
}
