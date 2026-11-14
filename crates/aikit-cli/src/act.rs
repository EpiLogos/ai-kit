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

use crate::app::{AikitApplication, RunRequest, Service};
use crate::cli::{ActDescribeArgs, ActDiscoverArgs, ActInvokeArgs};
use aikit_core::id::CapsuleId;
use aikit_core::resource::{
    ResourceIndex, ResourceKind, ResourceRef, ResourceSource, SourceAuthority, SourceState,
};
use aikit_core::{AikitError, Result};

/// The one shared resource field: discovery, description and invocation read
/// the same index the terminal surface renders — destination navigation, the
/// native Factory Action, Explain/History and every contextual relation — so
/// an Action the surface shows is exactly an Action this doorway can describe
/// and dispatch. Building a second, narrower field here would make the
/// doorway's Action field drift from the surface's.
fn shared_index(service: &Service) -> Result<aikit_core::resource::ResourceSearchIndex> {
    aikit_tui::application_service::ApplicationService::navigation_index_from(service)
}

/// One of AIKit's own operations as the O:I Action registry carries it (`oi act`, O-I #1002):
/// an id under `aikit.`, the route of this binary's parser it is, and the mutation class it has.
/// Nothing else is written here. Its description and every input are read from the parser
/// itself — the route's own arguments and help — so the descriptor cannot drift from what
/// parses, and invoking it runs exactly that route of this binary.
struct RouteAction {
    id: &'static str,
    title: &'static str,
    route: &'static [&'static str],
    mutation_class: &'static str,
}

/// The operations the modes' cores write through (docs/architecture/world-shell.md §9.3 in O-I):
/// send a Communique, control a binding's conversation (`new` starts one, `stop` ends it), cross a
/// Communique into work, the population those act on, and whether a sent Communique was heard —
/// the read a send is checked by — and end a session AIKit opened.
const ROUTE_ACTIONS: &[RouteAction] = &[
    RouteAction {
        id: "aikit.gateway.send",
        title: "Send a Communique to a Position",
        route: &["gateway", "send"],
        mutation_class: "externally-mutating",
    },
    RouteAction {
        id: "aikit.gateway.agent",
        title: "Control a binding's conversation",
        route: &["gateway", "agent"],
        mutation_class: "externally-mutating",
    },
    // Scope's Calls to other agents (O-I #1006; manual §19.12): the rule for one sender kind.
    RouteAction {
        id: "aikit.gateway.ingress",
        title: "Rule a sender kind's ingress on a binding",
        route: &["gateway", "ingress"],
        mutation_class: "externally-mutating",
    },
    // A session's own settings and its bound task's (O-I #1006): Scope's Wait and On no answer, the
    // task's Effort and Tool calls envelopes, and the rest the verb names. Without `request_json`
    // it reads.
    RouteAction {
        id: "aikit.session.settings",
        title: "Read or change a session's settings and its task's",
        route: &[FOLDED, "encounter-session-settings"],
        mutation_class: "locally-mutating",
    },
    RouteAction {
        id: "aikit.gateway.delegate",
        title: "Cross a Communique into work",
        route: &["gateway", "delegate"],
        mutation_class: "externally-mutating",
    },
    RouteAction {
        id: "aikit.gateway.who",
        title: "Who is here",
        route: &["gateway", "who"],
        mutation_class: "read-only",
    },
    RouteAction {
        id: "aikit.gateway.heard",
        title: "Whether a Communique was heard",
        route: &["gateway", "heard"],
        mutation_class: "read-only",
    },
    // A session's End, the one press that ends it (O-I manual §19.1): its body released, its end
    // recorded in the lifecycle its start opened (O-I #1007, #1016).
    RouteAction {
        id: "aikit.session.end",
        title: "End a session AIKit opened",
        route: &["session", "end"],
        mutation_class: "externally-mutating",
    },
    // The receipt a launch binds a session's work to (Factory's task run on a seated session, O-I #1006).
    RouteAction {
        id: "aikit.model.selection",
        title: "Read the selection a launch binds for a session AIKit opened",
        route: &["compose", "model", "selection"],
        mutation_class: "read-only",
    },
];

/// The arguments a route takes that are its input. `--json` is not one: the doorway always asks
/// for the machine-read answer.
const ROUTE_PLUMBING: [&str; 2] = ["help", "json"];

fn route_action(reference: &str) -> Option<&'static RouteAction> {
    ROUTE_ACTIONS.iter().find(|action| action.id == reference)
}

/// The folded SessionSpace surface (`aikit session-space <verb>`): the main binary forwards
/// everything after the word to that surface's own parser (session_space_cli.rs), so a route under
/// it is read from that parser, takes the main binary's `--cwd` before the word, and answers in
/// JSON with no `--json` of its own.
const FOLDED: &str = "session-space";

fn folded(route: &[&str]) -> bool {
    route.first() == Some(&FOLDED)
}

/// The parser's own command for a route, built (globals propagated), or `None` when the
/// route no longer parses — which the tests below refuse.
fn route_command(route: &[&str]) -> Option<clap::Command> {
    use clap::CommandFactory;
    let (mut command, words) = if folded(route) {
        (crate::session_space_cli::command(), &route[1..])
    } else {
        (crate::cli::Cli::command(), route)
    };
    command.build();
    let mut current = command;
    for word in words {
        current = current.find_subcommand(word)?.clone();
    }
    Some(current)
}

/// One input of a route, as the parser takes it.
fn route_inputs(command: &clap::Command) -> Vec<serde_json::Value> {
    let mut positional = 0usize;
    command
        .get_arguments()
        .filter(|argument| !ROUTE_PLUMBING.contains(&argument.get_id().as_str()))
        .map(|argument| {
            let action = argument.get_action();
            let kind = if !argument.get_action().takes_values() {
                "boolean"
            } else if matches!(action, clap::ArgAction::Append) {
                "string[]"
            } else {
                "string"
            };
            // A flag's input is named as the flag is spelt (`--reply-to` → `reply_to`), a
            // positional by the parser's own name for it.
            let name = match argument.get_long() {
                Some(long) if !argument.is_positional() => long.replace('-', "_"),
                _ => argument.get_id().as_str().to_owned(),
            };
            let mut input = serde_json::json!({
                "name": name,
                "type": kind,
                "required": argument.is_required_set(),
                "description": argument.get_help().map(|help| help.to_string()),
            });
            if argument.is_positional() {
                input["positional"] = serde_json::Value::from(positional);
                positional += 1;
            } else if let Some(long) = argument.get_long() {
                input["flag"] = serde_json::Value::from(format!("--{long}"));
            }
            input
        })
        .collect()
}

/// A route Action's descriptor, in the registry's shape (`oi.owner-action-field/v1`: Central's
/// Action descriptor) beside the field's own row keys.
fn route_row(action: &RouteAction) -> Option<serde_json::Value> {
    let command = route_command(action.route)?;
    // The folded surface answers with its document alone, never in the envelope.
    let output = if folded(action.route) {
        "aikit-session-space-json"
    } else {
        "aikit-json-envelope"
    };
    Some(serde_json::json!({
        "ref": action.id,
        "id": action.id,
        "kind": "action",
        "label": action.title,
        "title": action.title,
        "description": command.get_about().map(|about| about.to_string()),
        "mutation_class": action.mutation_class,
        "inputs": route_inputs(&command),
        "output": {"type": output},
        "route": action.route,
        "owner": "ai-kit",
        "availability": {"available": true, "reason": null, "state": "runnable"},
        "routes": {
            "describe": format!("aikit act describe {}", action.id),
            "invoke": format!("aikit act invoke {} --input '<json>'", action.id),
        },
    }))
}

/// The argv a route Action's input makes: the route, its positionals in order, then each flag
/// the input names. An input the route does not take, a required input missing, or a value of
/// the wrong shape is refused by name before anything runs. Words the route can also read from
/// a file (`--body` beside `--body-file`) travel on the route's stdin as `--body-file -`, never
/// in argv: a process listing does not show them and no length meets the argv limit. One input
/// at most takes stdin; the second is the route's own refusal.
fn route_argv(
    action: &RouteAction,
    input: &serde_json::Value,
) -> Result<(Vec<String>, Option<String>)> {
    let command = route_command(action.route).ok_or_else(|| {
        AikitError::new(
            "act.route_unparsed",
            format!("`{}` names a route this binary no longer parses", action.id),
        )
    })?;
    let inputs = route_inputs(&command);
    let empty = serde_json::Map::new();
    let given = match input {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => &empty,
        _ => {
            return Err(AikitError::new(
                "act.input_not_object",
                format!("`{}` takes one JSON object of named inputs", action.id),
            ))
        }
    };
    for name in given.keys() {
        if !inputs.iter().any(|input| input["name"] == name.as_str()) {
            return Err(AikitError::new(
                "act.input_unknown",
                format!("`{}` takes no input `{name}`", action.id),
            )
            .with(
                "recovery",
                format!("`aikit act describe {}` names its inputs", action.id),
            ));
        }
    }
    let is_folded = folded(action.route);
    let mut positionals: Vec<(u64, String)> = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    // Before the route's words: a folded route's `--cwd`, the main binary's (see [`FOLDED`]).
    let mut first: Vec<String> = Vec::new();
    let mut stdin: Option<String> = None;
    for input in &inputs {
        let name = input["name"].as_str().unwrap_or_default();
        let Some(value) = given.get(name).filter(|value| !value.is_null()) else {
            if input["required"] == true {
                return Err(AikitError::new(
                    "act.input_missing",
                    format!("`{}` needs its input `{name}`", action.id),
                ));
            }
            continue;
        };
        let text = |value: &serde_json::Value| -> Result<String> {
            match value {
                serde_json::Value::String(text) => Ok(text.clone()),
                serde_json::Value::Number(number) => Ok(number.to_string()),
                _ => Err(AikitError::new(
                    "act.input_shape",
                    format!("`{}`'s input `{name}` is one value", action.id),
                )),
            }
        };
        match (input["type"].as_str(), input.get("positional")) {
            (_, Some(index)) => positionals.push((index.as_u64().unwrap_or(0), text(value)?)),
            (Some("boolean"), None) => match value {
                serde_json::Value::Bool(true) => {
                    flags.push(input["flag"].as_str().unwrap_or_default().to_owned())
                }
                serde_json::Value::Bool(false) => {}
                _ => {
                    return Err(AikitError::new(
                        "act.input_shape",
                        format!("`{}`'s input `{name}` is true or false", action.id),
                    ))
                }
            },
            (Some("string[]"), None) => {
                let items = value.as_array().ok_or_else(|| {
                    AikitError::new(
                        "act.input_shape",
                        format!("`{}`'s input `{name}` is a list", action.id),
                    )
                })?;
                for item in items {
                    flags.push(input["flag"].as_str().unwrap_or_default().to_owned());
                    flags.push(text(item)?);
                }
            }
            _ => {
                let flag = input["flag"].as_str().unwrap_or_default().to_owned();
                let words = text(value)?;
                let file_flag = format!("{flag}-file");
                if is_folded && flag == "--cwd" {
                    first.extend([flag, words]);
                } else if stdin.is_none()
                    && inputs
                        .iter()
                        .any(|other| other["flag"] == file_flag.as_str())
                {
                    flags.extend([file_flag, "-".to_owned()]);
                    stdin = Some(words);
                } else {
                    flags.extend([flag, words]);
                }
            }
        }
    }
    positionals.sort_by_key(|(index, _)| *index);
    let mut argv = first;
    argv.extend(action.route.iter().map(|word| (*word).to_owned()));
    argv.extend(positionals.into_iter().map(|(_, value)| value));
    argv.extend(flags);
    if !is_folded {
        argv.push("--json".to_owned());
    }
    Ok((argv, stdin))
}

/// Run a route Action: exactly that route of this binary, once, with the argv its input makes.
/// The route's answer is the result; its refusal is the owner's words, never a substituted one.
fn invoke_route(action: &RouteAction, input: &serde_json::Value) -> Result<(String, String)> {
    let (argv, words) = route_argv(action, input)?;
    let executable = std::env::current_exe().map_err(|error| {
        AikitError::new(
            "act.route_unavailable",
            format!(
                "this binary cannot find itself to run `{}`: {error}",
                action.id
            ),
        )
    })?;
    let unstarted = |error: std::io::Error| {
        AikitError::new(
            "act.route_unavailable",
            format!("`{}` could not start: {error}", action.id),
        )
    };
    let mut child = std::process::Command::new(executable)
        .args(&argv)
        .stdin(match words {
            Some(_) => std::process::Stdio::piped(),
            None => std::process::Stdio::null(),
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(unstarted)?;
    if let (Some(words), Some(mut pipe)) = (words, child.stdin.take()) {
        // A route that stops reading early says so itself; its exit is the outcome.
        let _ = std::io::Write::write_all(&mut pipe, words.as_bytes());
    }
    let output = child.wait_with_output().map_err(unstarted)?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let words = if stdout.is_empty() { stderr } else { stdout };
        let (words, code) = routed_refusal(words);
        let mut refused = AikitError::new(
            "act.route_refused",
            format!(
                "`aikit {}` refused (exit {}): {words}",
                action.route.join(" "),
                output.status.code().unwrap_or(1)
            ),
        )
        .with("action", action.id);
        if let Some(code) = code {
            refused = refused.with("route_code", code);
        }
        return Err(refused);
    }
    Ok((argv.join(" "), stdout))
}

/// A routed command's refusal in its own words: a route that refuses in AIKit's envelope (`{ok: false, error: {code, message}}`)
/// is carried by its message and its code, never as the envelope, so the person reads what the route said, and so is the folded
/// SessionSpace surface's `code: message` (its code dotted, as AIKit's codes are); any other words come as they are.
fn routed_refusal(words: String) -> (String, Option<String>) {
    let routed = serde_json::from_str::<serde_json::Value>(&words)
        .ok()
        .and_then(|envelope| {
            let message = envelope["error"]["message"].as_str()?.to_owned();
            Some((
                message,
                envelope["error"]["code"].as_str().map(str::to_owned),
            ))
        })
        .or_else(|| {
            let (code, message) = words.split_once(": ")?;
            let coded = code.contains('.')
                && code.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
                });
            coded.then(|| (message.to_owned(), Some(code.to_owned())))
        });
    match routed {
        Some((message, code)) => (message, code),
        None => (words, None),
    }
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
        actions
            .sort_by(|left, right| (&left.label, &left.action).cmp(&(&right.label, &right.action)));
        rows.reserve(actions.len());
        for action in actions {
            let mut row = action_row(&index, action);
            row["subject"] = serde_json::Value::from(record.descriptor.id.to_string());
            row["subject_name"] = serde_json::Value::from(record.descriptor.name.clone());
            rows.push(row);
        }
    } else {
        // Scope discovery: AIKit's own route Actions first (they are this
        // owner's operations in every scope), then the canonical Action
        // records themselves, ranked by the shared search order (empty query
        // keeps authored order), bounded.
        let query = args.query.as_deref().unwrap_or("");
        let words = query.to_lowercase();
        for action in ROUTE_ACTIONS {
            let named = words.is_empty()
                || words.split_whitespace().all(|word| {
                    action.id.contains(word) || action.title.to_lowercase().contains(word)
                });
            if let (true, Some(row)) = (named, route_row(action)) {
                rows.push(row);
            }
        }
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
pub fn search_rows(service: &Service, query: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
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

/// One nearby-suggestion row: a real descriptor record with its next routes.
fn suggestion_row(
    index: &aikit_core::resource::ResourceSearchIndex,
    reference: &ResourceRef,
) -> Option<serde_json::Value> {
    let record = ResourceIndex::resource(index, reference)?;
    Some(serde_json::json!({
        "ref": record.descriptor.id.to_string(),
        "kind": record.descriptor.kind.as_str(),
        "label": record.descriptor.name,
        "next": {
            "describe": format!("aikit act describe {}", record.descriptor.id),
            "open": format!("aikit knowledge open {}", record.descriptor.id),
        },
    }))
}

/// The bounded answer when a task-phrase query resolves to nothing at all:
/// which corpora were searched — named, with their record counts, an absent
/// corpus a zero rather than a collapsed silence — and up to `limit` nearby
/// records from the same field, found by word agreement on the query's
/// significant words, or the field's own head when nothing agrees. Never a
/// bare expression dump; inert like every search reading.
pub fn empty_query_disclosure(
    service: &Service,
    query: &str,
    limit: usize,
) -> Result<serde_json::Value> {
    let index = shared_index(service)?;
    let (readings, _) = crate::alias_family::read_all(service.home(), None);
    let terms = aikit_core::resource::significant_terms(query);

    let mut seen = std::collections::BTreeSet::new();
    let mut suggestions: Vec<serde_json::Value> = Vec::new();
    let consider = |hit: &aikit_core::resource::ResourceSearchHit,
                    seen: &mut std::collections::BTreeSet<String>,
                    suggestions: &mut Vec<serde_json::Value>| {
        if seen.insert(hit.resource.to_string()) {
            if let Some(row) = suggestion_row(&index, &hit.resource) {
                suggestions.push(row);
            }
        }
    };

    // Nearby first: field records whose words agree with the query's own.
    for term in terms.iter().take(4) {
        for hit in index.search(term, limit) {
            consider(&hit, &mut seen, &mut suggestions);
            if suggestions.len() >= limit {
                break;
            }
        }
        if suggestions.len() >= limit {
            break;
        }
    }
    // Then the field's own head: nothing in the query agrees with the field,
    // and the answer should still name where to look.
    if suggestions.is_empty() && !index.is_empty() {
        for hit in index.search("", limit) {
            consider(&hit, &mut seen, &mut suggestions);
            if suggestions.len() >= limit {
                break;
            }
        }
    }
    // A head with no navigation evidence still names its records, in ref order.
    if suggestions.is_empty() && !index.is_empty() {
        for record in ResourceIndex::resources(&index) {
            if seen.insert(record.descriptor.id.to_string()) {
                if let Some(row) = suggestion_row(&index, &record.descriptor.id) {
                    suggestions.push(row);
                }
            }
            if suggestions.len() >= limit {
                break;
            }
        }
    }

    Ok(serde_json::json!({
        "schema": "aikit.search-empty/v1",
        "query": query,
        "searched": [
            {
                "corpus": "resource-field",
                "what": "capability, skill, method, action and navigation descriptors: names, descriptions, ids, search tags",
                "records": index.len(),
            },
            {
                "corpus": "alias-families",
                "what": "authored command families and their entries",
                "records": readings.len(),
            },
        ],
        "suggestions": suggestions,
        "next": [
            "aikit act discover",
            "aikit search   (empty query shows the field's own head)",
        ],
    }))
}

/// `aikit act describe <ref>` — the exact input/output/effect contract, from
/// the real descriptor. Read-only.
pub fn describe(service: &Service, args: ActDescribeArgs) -> Result<serde_json::Value> {
    if let Some(action) = route_action(&args.reference) {
        let mut document = route_row(action).ok_or_else(|| {
            AikitError::new(
                "act.route_unparsed",
                format!("`{}` names a route this binary no longer parses", action.id),
            )
        })?;
        document["schema"] = serde_json::Value::from("aikit.act-description/v1");
        document["input"] = serde_json::json!({
            "form": "json",
            "contract": "one JSON object of the named inputs: positionals by name, each flag by its name; nothing else is accepted",
            "fields": document["inputs"].clone(),
        });
        return Ok(document);
    }
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
            descriptor
                .sources
                .iter()
                .map(source_json)
                .collect::<Vec<_>>(),
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
    /// One of AIKit's own route Actions ([`ROUTE_ACTIONS`]): the route ran once with the argv
    /// its input made, and its answer is carried as it came.
    Route {
        action: &'static str,
        argv: String,
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
/// An `--input` as given: inline JSON, `@FILE`, or `-` for the JSON on stdin (what the O:I
/// kernel's `act` hands every invocation, so no input meets the argv limit).
fn read_input(raw: &str) -> Result<String> {
    let unreadable = |from: &str, error: std::io::Error| {
        AikitError::new(
            "act.input_unreadable",
            format!("could not read the action input from {from}: {error}"),
        )
    };
    if raw == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .map_err(|error| unreadable("stdin", error))?;
        return Ok(text);
    }
    match raw.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path).map_err(|error| unreadable(path, error)),
        None => Ok(raw.to_owned()),
    }
}

pub fn invoke(service: &mut Service, args: ActInvokeArgs) -> Result<ActOutcome> {
    if let Some(action) = route_action(&args.reference) {
        let raw = match args.input.as_deref() {
            Some(raw) => read_input(raw)?,
            None => "{}".to_owned(),
        };
        let input: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|error| {
            AikitError::new(
                "act.input_not_json",
                format!("`{}` takes one JSON object: {error}", action.id),
            )
        })?;
        let (argv, output) = invoke_route(action, &input)?;
        let digest =
            blake3::hash(format!("aikit.act-owner-result/v1\0{}\0{output}", action.id).as_bytes())
                .to_hex()
                .to_string();
        return Ok(ActOutcome::Route {
            action: action.id,
            argv,
            output,
            digest,
        });
    }
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
        Some(raw) => Some(read_input(raw)?),
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
    /// A route's refusal reaches the person in the route's words, not AIKit's JSON envelope around them (O-I #1002: the kernel
    /// shows the owner's words).
    #[test]
    fn a_routed_refusal_is_the_route_s_words_not_its_envelope() {
        assert_eq!(
            super::routed_refusal(
                r#"{"ok":false,"error":{"code":"gateway.position_unknown","message":"No Position answers to @nobody"}}"#.to_owned()
            ),
            ("No Position answers to @nobody".to_owned(), Some("gateway.position_unknown".to_owned()))
        );
        assert_eq!(
            super::routed_refusal("error: unexpected argument '--nope'".to_owned()),
            ("error: unexpected argument '--nope'".to_owned(), None)
        );
        // The folded SessionSpace surface refuses as `code: message` on stderr (session_space_cli.rs
        // run_from_args): its message and its code, the same as an envelope's (O-I #1006).
        assert_eq!(
            super::routed_refusal(
                "encounter.runtime: Session settings revision conflict; read the current settings and write again".to_owned()
            ),
            (
                "Session settings revision conflict; read the current settings and write again".to_owned(),
                Some("encounter.runtime".to_owned())
            )
        );
    }

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
        Service::open(AikitHome::at(tmp.path()), &cwd, |key| env.get(key).cloned()).unwrap()
    }

    /// O-I #1002: AIKit's own route Actions stand in its field with a descriptor read from the
    /// parser — every route parses, its inputs are the route's own arguments — and an input
    /// makes exactly an argv this binary's parser accepts, while an input the route does not
    /// take, or a required one missing, is refused by name before anything runs.
    #[test]
    fn route_actions_are_the_parsers_own_routes_and_their_inputs_make_its_argv() {
        use clap::Parser;
        for action in ROUTE_ACTIONS {
            let row = route_row(action).unwrap_or_else(|| panic!("{} no longer parses", action.id));
            assert!(action.id.starts_with("aikit."), "{}", action.id);
            assert_eq!(row["id"], action.id);
            assert!(row["description"].is_string(), "{row}");
            let inputs = row["inputs"].as_array().unwrap();
            assert!(
                inputs
                    .iter()
                    .all(|input| input["name"] != "json" && input["name"] != "help"),
                "{row}"
            );
            assert!(
                inputs
                    .iter()
                    .any(|input| input["name"] == "cwd" && input["flag"] == "--cwd"),
                "the global --cwd is an input of every route: {row}"
            );
        }
        let send = route_action("aikit.gateway.send").unwrap();
        let row = route_row(send).unwrap();
        let body = row["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["name"] == "body")
            .cloned()
            .unwrap();
        assert_eq!(body["flag"], "--body");
        assert_eq!(body["type"], "string");

        let (argv, words) = route_argv(
            send,
            &serde_json::json!({"to": "central:position:w:desk", "body": "Hello", "handling": "steer"}),
        )
        .unwrap();
        assert_eq!(argv[..2], ["gateway", "send"]);
        assert_eq!(words.as_deref(), Some("Hello"), "the words travel on stdin");
        assert!(
            !argv.iter().any(|word| word == "Hello")
                && argv.windows(2).any(|pair| pair == ["--body-file", "-"]),
            "never in argv: {argv:?}"
        );
        assert_eq!(argv.last().map(String::as_str), Some("--json"));
        crate::cli::Cli::try_parse_from(std::iter::once("aikit".to_owned()).chain(argv.clone()))
            .unwrap_or_else(|error| panic!("{argv:?} does not parse: {error}"));

        // Scope's Calls to other agents (O-I #1006): one sender kind's rule on one binding.
        let ingress = route_action("aikit.gateway.ingress").unwrap();
        let (argv, words) = route_argv(
            ingress,
            &serde_json::json!({"binding": "gateway-binding/x", "kind": "agent", "decision": "pair"}),
        )
        .unwrap();
        assert_eq!(words, None);
        assert_eq!(argv[..2], ["gateway", "ingress"]);
        assert_eq!(argv.last().map(String::as_str), Some("--json"));
        crate::cli::Cli::try_parse_from(std::iter::once("aikit".to_owned()).chain(argv.clone()))
            .unwrap_or_else(|error| panic!("{argv:?} does not parse: {error}"));

        let agent = route_action("aikit.gateway.agent").unwrap();
        let (argv, words) = route_argv(
            agent,
            &serde_json::json!({"binding": "gateway-binding/x", "operation": "new"}),
        )
        .unwrap();
        assert_eq!(words, None);
        assert_eq!(
            argv[..3],
            ["gateway", "agent", "new"],
            "the positional comes first"
        );
        crate::cli::Cli::try_parse_from(std::iter::once("aikit".to_owned()).chain(argv.clone()))
            .unwrap_or_else(|error| panic!("{argv:?} does not parse: {error}"));

        let end = route_action("aikit.session.end").unwrap();
        let (argv, words) = route_argv(
            end,
            &serde_json::json!({"agent_session": "agent-session/conversation"}),
        )
        .unwrap();
        assert_eq!(words, None);
        assert_eq!(
            argv,
            ["session", "end", "agent-session/conversation", "--json"],
            "the session is the route's one positional"
        );
        crate::cli::Cli::try_parse_from(std::iter::once("aikit".to_owned()).chain(argv.clone()))
            .unwrap_or_else(|error| panic!("{argv:?} does not parse: {error}"));

        let unknown =
            route_argv(send, &serde_json::json!({"body": "x", "weather": "rain"})).unwrap_err();
        assert_eq!(unknown.code(), "act.input_unknown");
        let missing = route_argv(agent, &serde_json::json!({"operation": "stop"})).unwrap_err();
        assert_eq!(missing.code(), "act.input_missing");
        let shape = route_argv(send, &serde_json::json!({"body": ["a"]})).unwrap_err();
        assert_eq!(shape.code(), "act.input_shape");
    }

    /// O-I #1006: a route under the folded SessionSpace surface is read from that surface's own
    /// parser, and its argv is what the main binary forwards there: the main binary's `--cwd`
    /// before the word (after it, the surface would be given its `-C` twice), and no `--json`,
    /// which the surface has not (it answers in JSON).
    #[test]
    fn a_folded_route_takes_the_main_binary_s_cwd_and_no_json() {
        use clap::Parser;
        let settings = route_action("aikit.session.settings").unwrap();
        let row = route_row(settings).unwrap();
        let input = |name: &str| {
            row["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|input| input["name"] == name)
                .cloned()
                .unwrap_or_else(|| panic!("no input {name}: {row}"))
        };
        assert_eq!(input("agent_session")["type"], "string[]");
        assert_eq!(input("agent_session")["required"], true);
        assert_eq!(input("request_json")["flag"], "--request-json");
        assert_eq!(input("expected_revision")["required"], false);
        assert_eq!(
            row["route"],
            serde_json::json!(["session-space", "encounter-session-settings"])
        );

        let request = r#"{"track":{"wait":900,"no_answer":"deny"}}"#;
        let (argv, words) = route_argv(
            settings,
            &serde_json::json!({
                "cwd": "/w",
                "agent_session": ["agent-session/s"],
                "request_json": request,
                "expected_revision": "session-settings/r1",
            }),
        )
        .unwrap();
        assert_eq!(words, None);
        assert_eq!(
            argv,
            [
                "--cwd",
                "/w",
                "session-space",
                "encounter-session-settings",
                "--agent-session",
                "agent-session/s",
                "--request-json",
                request,
                "--expected-revision",
                "session-settings/r1",
            ]
        );
        // The main binary takes `--cwd` and forwards the rest to the surface after its own `-C`
        // (main.rs forward_session_space), where it parses.
        let parsed = crate::cli::Cli::try_parse_from(
            std::iter::once("aikit".to_owned()).chain(argv.clone()),
        )
        .unwrap_or_else(|error| panic!("{argv:?} does not parse: {error}"));
        assert_eq!(parsed.cwd.as_deref(), Some(std::path::Path::new("/w")));
        let Some(crate::cli::Command::SessionSpace { args }) = parsed.command else {
            panic!("{argv:?} is not the folded surface's");
        };
        crate::session_space_cli::command()
            .try_get_matches_from(
                ["aikit-session-space", "-C", "/w"]
                    .into_iter()
                    .map(std::ffi::OsString::from)
                    .chain(args),
            )
            .unwrap_or_else(|error| panic!("the surface refuses {argv:?}: {error}"));

        // Without a request it reads, several sessions at once.
        let (argv, _) = route_argv(
            settings,
            &serde_json::json!({"agent_session": ["agent-session/a", "agent-session/b"]}),
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "session-space",
                "encounter-session-settings",
                "--agent-session",
                "agent-session/a",
                "--agent-session",
                "agent-session/b",
            ]
        );
        let one = route_argv(
            settings,
            &serde_json::json!({"agent_session": "agent-session/a"}),
        )
        .unwrap_err();
        assert_eq!(
            one.code(),
            "act.input_shape",
            "a session is named in a list"
        );
    }

    #[test]
    fn the_field_lists_the_route_actions_and_describe_answers_them() {
        let mut service = service();
        let field = discover(
            &mut service,
            ActDiscoverArgs {
                subject: None,
                query: None,
                limit: 200,
            },
        )
        .unwrap();
        let ids: Vec<&str> = field["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect();
        for action in ROUTE_ACTIONS {
            assert!(
                ids.contains(&action.id),
                "{} is not in the field: {field}",
                action.id
            );
        }
        let narrowed = discover(
            &mut service,
            ActDiscoverArgs {
                subject: None,
                query: Some("send".to_owned()),
                limit: 200,
            },
        )
        .unwrap();
        assert!(narrowed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["id"] != "aikit.gateway.who"));
        let described = describe(
            &service,
            ActDescribeArgs {
                reference: "aikit.gateway.delegate".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(described["input"]["form"], "json");
        assert_eq!(described["mutation_class"], "externally-mutating");
        assert!(described["input"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "communique" && field["required"] == true));
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
            factory["subjects"]
                .as_array()
                .is_some_and(|s| !s.is_empty()),
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
        assert_eq!(
            described["expected_return_forms"],
            "factory.commission-receipt/v1"
        );

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

    /// X2: task language — not a handle — still surfaces the applicable
    /// Actions, each row carrying type, owner/source and the next route, and
    /// a query that resolves to nothing is answered with the bounded helpful
    /// form. Both readings are inert: no familiarity or observation event.
    #[test]
    fn task_phrase_surfaces_actions_and_an_empty_query_is_answered() {
        let service = service();

        // Task phrase through the Action lane: "this" breaks the strict
        // all-terms match, so the row surfaces through the significant-word
        // overlap fallback ("commission", "work").
        let rows = search_rows(&service, "commission this work", 24).unwrap();
        let factory = rows
            .iter()
            .find(|row| row["ref"] == "action/factory/start-work")
            .expect("task language surfaces the applicable Action");
        assert_eq!(factory["kind"], "action", "the row carries its type");
        assert_eq!(factory["owner"], "factory", "the row carries its owner");
        assert_eq!(
            factory["routes"]["describe"], "aikit act describe action/factory/start-work",
            "the row carries the next route"
        );

        // A query that resolves to nothing at all: corpora named, suggestions
        // bounded, routes attached — never a bare expression dump.
        let inert_before = aikit_tui::backend::PaletteBackend::familiarity(&service).unwrap();
        let disclosure = empty_query_disclosure(&service, "zzqx wobble flurb", 3).unwrap();
        assert_eq!(disclosure["schema"], "aikit.search-empty/v1");
        assert!(disclosure["searched"]
            .as_array()
            .is_some_and(|s| !s.is_empty()));
        let suggestions = disclosure["suggestions"].as_array().unwrap();
        assert!(!suggestions.is_empty() && suggestions.len() <= 3);
        assert!(suggestions
            .iter()
            .all(|s| s["next"]["describe"].is_string()));

        // Inert like every search reading (the standing familiarity
        // invariant): nothing above recorded an event.
        let inert_after = aikit_tui::backend::PaletteBackend::familiarity(&service).unwrap();
        assert_eq!(format!("{inert_before:?}"), format!("{inert_after:?}"));
    }
}
