//! The `act` doorway: contextual discovery, exact contracts, explicit invocation.
//!
//! One transport over the seams that already exist — no second registry:
//!
//! * discovery and description read the shared resource field
//!   ([`shared_index`]: the same index the terminal surface renders) and the
//!   resolver's own catalogued view; every row's contract comes from the real
//!   descriptor, never from a hand-written catalogue;
//! * a contextual Action dispatches through its owner's own operation (the
//!   same seam the surface itself uses), and exported capabilities keep going
//!   through [`Service::run`], the native runner `aikit run` and scoped
//!   invocation use, so policy, trust and the confirm gate apply identically.
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
use crate::app::{AikitApplication, RunRequest, Service};
use crate::cli::{ActDescribeArgs, ActDiscoverArgs, ActInvokeArgs};

/// The one shared resource field: discovery, description and invocation read
/// the same index the terminal surface renders — destination navigation, the
/// native Factory Action, Explain/History and every contextual relation — so
/// an Action the surface shows is exactly an Action this doorway can describe
/// and dispatch. Building a second, narrower field here would make the
/// doorway's Action field drift from the surface's.
fn shared_index(service: &Service) -> Result<aikit_core::resource::ResourceSearchIndex> {
    aikit_tui::application_service::ApplicationService::navigation_index_from(service)
}

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
    let index = shared_index(service)?;
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

/// The contextual Action rows a task-language search should surface: the
/// Action records the shared index holds in this scope, ranked by the same
/// search order discovery uses, each carrying its subjects and next routes.
/// Inert like every search reading: it records no observation event.
pub fn search_rows(
    service: &Service,
    query: &str,
    limit: usize,
) -> Result<Vec<serde_json::Value>> {
    let index = shared_index(service)?;
    let mut rows = Vec::new();
    for hit in index.search(query, limit) {
        if hit.kind != ResourceKind::Action {
            continue;
        }
        let Some(record) = ResourceIndex::resource(&index, &hit.resource) else {
            continue;
        };
        let mut row = record_row(&record.descriptor, &index);
        row["availability"] = availability(service, &hit.resource);
        rows.push(row);
        if rows.len() >= limit {
            break;
        }
    }
    Ok(rows)
}

/// `aikit act describe <ref>` — the exact input/output/effect contract, from
/// the real descriptor. Read-only.
pub fn describe(service: &Service, args: ActDescribeArgs) -> Result<serde_json::Value> {
    let reference = ResourceRef::parse(&args.reference)?;
    let index = shared_index(service)?;

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
        // The exact invocation contract of a contextual Action comes from its
        // owner binding — never the capability argv contract, which does not
        // apply to an Action that has no capability behind it.
        if descriptor.kind == ResourceKind::Action {
            match reference.as_str() {
                aikit_tui::workspace_navigation::START_FACTORY_WORK_ACTION_REF => {
                    document["input"] = serde_json::json!({
                        "form": "owner-native",
                        "contract": "the reviewed Commission request file is the exact owner input; `--input` is not accepted",
                        "effect": "submits the configured developmental Commission through Factory's owner operation; the reply is the native receipt",
                        "confirm_required": false,
                    });
                }
                aikit_core::explain_history_actions::EXPLAIN_ACTION_REF
                | aikit_core::explain_history_actions::HISTORY_ACTION_REF => {
                    let verb = reference
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap_or("explain")
                        .to_owned();
                    document["input"] = serde_json::json!({
                        "form": "subject-scoped",
                        "contract": "the owner operation is the existing read verb applied to one subject",
                        "effect": "read-only",
                        "routes": {
                            "invoke": format!("aikit {verb} <subject>"),
                        },
                    });
                }
                _ => {
                    document["input"] = serde_json::json!({
                        "form": "surface-internal",
                        "contract": "this Action is the application surface's own navigation or staging act; it has no CLI input contract",
                        "routes": {
                            "surface": "aikit ui",
                        },
                    });
                }
            }
        }
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

/// What one explicit `act invoke` did. Exported capabilities keep the native
/// runner handle; a contextual Action dispatches through its owner's own
/// operation and carries the owner's native result — a capability run is never
/// substituted for it.
#[derive(Debug)]
pub enum ActOutcome {
    Capability {
        run: crate::app::RunHandle,
        digest: String,
    },
    Owner {
        action: ResourceRef,
        owner: String,
        subject: Option<String>,
        output: String,
        digest: String,
    },
}

/// The owner-operation binding for contextual Actions: each entry routes one
/// Action record to the owner operation that already exists — the same seam
/// the application surface itself uses. An Action with no bound owner
/// operation refuses with its real route instead of being substituted.
fn contextual_dispatch(
    service: &mut Service,
    index: &aikit_core::resource::ResourceSearchIndex,
    action: &ResourceRef,
) -> Result<(String, Option<String>, String)> {
    match action.as_str() {
        aikit_tui::workspace_navigation::START_FACTORY_WORK_ACTION_REF => {
            // The exact owner adapter the Work surface uses: the reviewed
            // request file is the input, Factory's native receipt the result.
            let receipt = aikit_tui::PaletteBackend::start_factory_work(service)?;
            let output = format!("{}\n{}", receipt.summary, receipt.receipt);
            Ok(("factory".to_owned(), None, output))
        }
        aikit_core::explain_history_actions::EXPLAIN_ACTION_REF
        | aikit_core::explain_history_actions::HISTORY_ACTION_REF => {
            // The owner operation of these Actions IS the existing read verb
            // on the subject; the doorway never re-transports it. The refusal
            // names the exact equivalent route per subject relation.
            let subjects = index
                .subjects_for_action(action)
                .iter()
                .map(|contextual| contextual.subject.to_string())
                .collect::<Vec<_>>();
            let verb = action.as_str().rsplit('/').next().unwrap_or("explain");
            Err(AikitError::new(
                "act.owner_is_read_verb",
                format!("`{action}` names an existing owner read route, not a separate doorway operation"),
            )
            .with(
                "recovery",
                format!(
                    "invoke the owner verb directly on a subject: `aikit {verb} <subject>` — e.g. {}",
                    subjects
                        .first()
                        .map(|subject| format!("`aikit {verb} {subject}`"))
                        .unwrap_or_else(|| "`aikit search <text>` to find the subject".into())
                ),
            ))
        }
        other => Err(AikitError::new(
            "act.surface_internal",
            format!(
                "`{other}` is the application surface's own act (navigation or staging); it has no CLI owner operation"
            ),
        )
        .with(
            "recovery",
            "open the surface with `aikit ui`, where this Action is one of the World's numbered next steps",
        ))
    }
}

/// `aikit act invoke <ref>` — resolve, then perform once through the existing
/// native runner. The returned handle carries the real status and output;
/// unknown or ambiguous refs are refused before any effect.
pub fn invoke(service: &mut Service, args: ActInvokeArgs) -> Result<ActOutcome> {
    let reference = ResourceRef::parse(&args.reference)?;
    let index = shared_index(service)?;

    // A contextual Action dispatches through its owner's own operation, with
    // the Action's subject relation and native result preserved together.
    if let Some(record) = ResourceIndex::resource(&index, &reference) {
        if record.descriptor.kind == ResourceKind::Action {
            let (owner, subject, output) = contextual_dispatch(service, &index, &reference)?;
            let digest = blake3::hash(
                format!("aikit.act-owner-result/v1\0{reference}\0{output}").as_bytes(),
            )
            .to_hex()
            .to_string();
            return Ok(ActOutcome::Owner {
                action: reference,
                owner,
                subject,
                output,
                digest,
            });
        }
    }

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
            Ok(ActOutcome::Capability { run, digest })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Service;
    use aikit_store::AikitHome;
    use std::collections::BTreeMap;

    fn service() -> Service {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        // A complete native Factory start-work binding: with it the shared
        // field installs the Factory Action, exactly as on a configured machine.
        let factory_state = tmp.path().join("factory-state.json");
        let factory_request = tmp.path().join("factory-request.json");
        std::fs::write(&factory_state, "{}").unwrap();
        std::fs::write(&factory_request, "{}").unwrap();
        let mut env = BTreeMap::new();
        env.insert(
            "AIKIT_CONTEXT_ID".to_owned(),
            aikit_core::ContextId::generate().to_string(),
        );
        env.insert(
            "AIKIT_FACTORY_STATE".to_owned(),
            factory_state.display().to_string(),
        );
        env.insert(
            "AIKIT_FACTORY_REQUEST_FILE".to_owned(),
            factory_request.display().to_string(),
        );
        // The temporary lives for the process lifetime of the test; the
        // service holds no handle to the directory after reads.
        Service::open(AikitHome::at(tmp.path()), &cwd, |key| env.get(key).cloned())
            .unwrap()
    }

    fn invoke_ref(service: &mut Service, reference: &str) -> Result<ActOutcome> {
        invoke(
            service,
            ActInvokeArgs {
                reference: reference.to_owned(),
                input: None,
                confirm: true,
                args: vec![],
            },
        )
    }

    /// The doorway holds together: a search hit for a task phrase surfaces the
    /// contextual Action with its owner and subjects, describe names the exact
    /// owner input contract, and invoke dispatches through the owner's own
    /// operation — the factory Action reaches the Factory start-work adapter
    /// (whose missing-binding refusal is the owner's own answer), never a
    /// substituted capability run and never `run.unknown_command`.
    #[test]
    fn search_describe_and_invoke_hold_together_across_contextual_actions() {
        let mut service = service();

        // Search: task language surfaces the contextual Action rows.
        let rows = search_rows(&service, "commission", 24).unwrap();
        let factory = rows
            .iter()
            .find(|row| row["ref"] == "action/factory/start-work")
            .expect("the Factory Action joins the search answer");
        assert_eq!(factory["owner"], "factory");
        assert!(
            factory["subjects"].as_array().is_some_and(|s| !s.is_empty()),
            "the Action's subject relation rides the row"
        );
        assert_eq!(
            factory["routes"]["invoke"], "aikit act invoke action/factory/start-work",
            "the doorway route is the next act"
        );

        // Describe: the exact owner input contract, not the capability argv.
        let described = describe(
            &service,
            ActDescribeArgs {
                reference: "action/factory/start-work".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(described["input"]["form"], "owner-native");
        assert_eq!(described["expected_return_forms"], "factory.commission-receipt/v1");

        // Invoke: dispatch reaches the owner operation. The bare test service
        // holds no Factory binding, so the owner's own entry refusal is the
        // honest outcome — the decisive point is it is NOT the capability
        // runner's `run.unknown_command`.
        let error = invoke_ref(&mut service, "action/factory/start-work")
            .expect_err("no Factory binding is configured here");
        assert_ne!(
            error.code(),
            "run.unknown_command",
            "the contextual Action reached its owner dispatch, not capability resolution"
        );

        // Read Actions name their owner read verb instead of re-transports.
        let error = invoke_ref(&mut service, "action/aikit/explain")
            .expect_err("read Actions route to the existing read verb");
        assert_eq!(error.code(), "act.owner_is_read_verb");

        // Surface-internal Actions name the surface, honestly.
        let error = invoke_ref(&mut service, "action/workspace/open-destination")
            .expect_err("navigation Actions are the surface's own acts");
        assert_eq!(error.code(), "act.surface_internal");
    }
}
