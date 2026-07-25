//! The `aikit` binary.
//!
//! Deliberately thin: it decides whether it was invoked as `aikit` or under an
//! exported command name (the multicall shim, [`aikit_cli::multicall`]), parses
//! the [`aikit_cli::cli`] tree, and dispatches to the one shared
//! [`aikit_cli::app::Service`]. Every substantive command speaks the stable JSON
//! envelope under `--json` and maps its error to the published exit-code table.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use serde_json::{json as jval, Value};

use aikit_cli::app::{
    AikitApplication, ApplyRequest, PromoteRequest, RunRequest, Service, SessionRequest,
};
use aikit_cli::cli::*;
use aikit_cli::json::{self, EnvelopeContext};
use aikit_cli::{hook, multicall, ui};

use aikit_core::hooks::HookEvent;
use aikit_core::id::CapsuleId;
use aikit_core::scope::ScopeKind;
use aikit_core::{AikitError, Result};

use aikit_store::home::AikitHome;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let argv0 = argv.first().cloned().unwrap_or_default();

    let code = if multicall::is_aikit(&argv0) {
        run_cli()
    } else {
        run_multicall(&multicall::program_name(&argv0), &argv[1..])
    };
    ExitCode::from(code as u8)
}

/// The busybox path: the binary was invoked under an export name.
fn run_multicall(export: &str, args: &[String]) -> i32 {
    let result = (|| -> Result<i32> {
        let home = AikitHome::discover()?;
        let cwd = std::env::current_dir()
            .map_err(|e| AikitError::new("cli.cwd_unavailable", format!("no cwd: {e}")))?;
        multicall::dispatch(export, args, &home, &cwd, |k| std::env::var(k).ok())
    })();
    match result {
        Ok(status) => status,
        Err(e) => {
            eprintln!("aikit: {e}");
            json::exit_code(&e)
        }
    }
}

/// The normal path: parse and run a subcommand.
fn run_cli() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // clap renders help and usage errors itself; honour its intended
            // stream and status but keep our usage exit code.
            let _ = e.print();
            return if e.use_stderr() { json::EXIT_USAGE } else { json::EXIT_OK };
        }
    };

    let json_mode = cli.json;
    let cwd = cli
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    match dispatch(cli, &cwd) {
        Ok(reply) => emit(reply, json_mode),
        Err(e) => {
            if json_mode {
                println!("{}", json::line(&json::failure(&e)));
            } else {
                eprintln!("aikit: {e}");
            }
            json::exit_code(&e)
        }
    }
}

/// A command's result, before rendering.
enum Reply {
    /// Data to wrap in a success envelope (or pretty-print in human mode).
    Data {
        context: EnvelopeContext,
        data: Value,
        warnings: Vec<String>,
    },
    /// Raw text to print verbatim, envelope or not (`shell init`, an explanation).
    Text(String),
    /// A child process ran; its exit status is ours.
    Status(i32),
    /// The palette ran and restored the terminal; nothing to print.
    Silent,
}

fn reply(service: &Service, data: Value, warnings: Vec<String>) -> Reply {
    Reply::Data {
        context: EnvelopeContext::from_descriptor(service.descriptor()),
        data,
        warnings,
    }
}

fn emit(reply: Reply, json_mode: bool) -> i32 {
    match reply {
        Reply::Data {
            context,
            data,
            warnings,
        } => {
            if json_mode {
                println!("{}", json::line(&json::success(&context, data, warnings)));
            } else {
                println!("{}", json::pretty(&data));
                for warning in warnings {
                    eprintln!("warning: {warning}");
                }
            }
            json::EXIT_OK
        }
        Reply::Text(text) => {
            println!("{text}");
            json::EXIT_OK
        }
        Reply::Status(code) => code,
        Reply::Silent => json::EXIT_OK,
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn dispatch(cli: Cli, cwd: &std::path::Path) -> Result<Reply> {
    match cli.command {
        None => open_palette(cwd, None, false),
        Some(Command::Init(a)) => cmd_init(cwd, a),
        Some(Command::Collate(a)) => cmd_collate(cwd, a),
        Some(Command::Ui(a)) => open_palette(cwd, a.query, a.fullscreen),

        Some(Command::Search(a)) => cmd_search(cwd, a),
        Some(Command::Status(a)) => cmd_status(cwd, a),
        Some(Command::Explain(a)) => cmd_explain(cwd, a),
        Some(Command::Run(a)) => cmd_run(cwd, a),
        Some(Command::Enable(a)) => cmd_toggle(cwd, a, true),
        Some(Command::Disable(a)) => cmd_toggle(cwd, a, false),
        Some(Command::Apply(a)) => cmd_apply(cwd, a),
        Some(Command::Rollback(_)) => cmd_rollback(cwd),
        Some(Command::Prune(a)) => cmd_prune(cwd, a),
        Some(Command::Context(c)) => cmd_context(cwd, c),
        Some(Command::Task(c)) => cmd_task(cwd, c),
        Some(Command::Bypass(c)) => cmd_bypass(cwd, c),
        Some(Command::Bypasses(_)) => cmd_bypasses(cwd),
        Some(Command::Hook(c)) => cmd_hook(cwd, c),
        Some(Command::Capabilities(c)) => cmd_capabilities(cwd, c),
        Some(Command::Session(c)) => cmd_session(cwd, c),
        Some(Command::Promote(a)) => cmd_promote(cwd, a),
        Some(Command::Inbox(a)) => cmd_inbox(cwd, a),
        Some(Command::Shell(c)) => cmd_shell(c),

        // Real, but delegated to modules whose full wiring is the integration
        // phase's job. Returning a clear, stable code is more honest than a
        // half-working stub that pretends to have done the work.
        Some(other) => Err(not_implemented(command_name(&other))),
    }
}

/// `aikit init` — discover the foreign skill roots already on the machine and
/// show them, read-only. It indexes what is there and reports counts (including
/// the dead symlinks and unusable frontmatter a user cannot otherwise see); it
/// asks nothing and writes nothing. Adoption — turning a foreign root into
/// something AIKit owns — is a separate, explicitly confirmed Procedure.
fn cmd_init(cwd: &std::path::Path, a: InitArgs) -> Result<Reply> {
    use aikit_cli::foreign;
    let service = Service::discover(cwd)?;

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut roots = foreign::default_roots(&home);
    for extra in &a.roots {
        let label = extra
            .file_name()
            .map(|n| format!("@{}", n.to_string_lossy()))
            .unwrap_or_else(|| "@root".to_string());
        roots.push((label, extra.clone()));
    }

    let found = foreign::discover(&roots);
    let rows: Vec<Value> = found
        .iter()
        .map(|r| {
            jval!({
                "label": r.label,
                "path": r.path.display().to_string(),
                "skills": r.skills,
                "dead_symlinks": r.dead_symlinks,
                "missing_frontmatter": r.missing_frontmatter,
                "problems": r.problems(),
            })
        })
        .collect();
    let total_skills: usize = found.iter().map(|r| r.skills).sum();
    let total_problems: usize = found.iter().map(foreign::ForeignRoot::problems).sum();

    let data = jval!({
        "roots": rows,
        "root_count": found.len(),
        "total_skills": total_skills,
        "total_problems": total_problems,
    });
    Ok(reply(&service, data, service.load_warnings()))
}

/// `aikit collate` — survey every skill root on this machine and report which
/// version of each skill is actually running and where. Read-only: foreign roots
/// are indexed, never edited. Genuine ambiguities are filed to the inbox as
/// `VersionConflict` items; byte-identical duplicates are counted, not filed,
/// because a dedup is not a decision.
fn cmd_collate(cwd: &std::path::Path, a: CollateArgs) -> Result<Reply> {
    use aikit_cli::{collate, foreign};
    let service = Service::discover(cwd)?;

    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let mut roots: Vec<collate::ForeignRootRef> = foreign::default_roots(&home)
        .into_iter()
        .filter(|(_, path)| path.exists())
        .map(|(label, path)| collate::ForeignRootRef { label, path })
        .collect();
    for extra in &a.roots {
        roots.push(collate::ForeignRootRef {
            label: extra.file_name().map(|n| format!("@{}", n.to_string_lossy())).unwrap_or_else(|| "@root".into()),
            path: extra.clone(),
        });
    }

    let (report, clusters) = collate::collate(service.index(), &roots)?;

    // Plugin caches keep `<plugin>/<version>/` side by side, which is where the
    // version conflicts that matter actually live. Surveyed separately because a
    // plugin declares its own name and version in a manifest (PRIOR-ART #33),
    // rather than being inferred from a skill's frontmatter.
    let plugin_roots: Vec<PathBuf> = [".claude/plugins", ".codex/plugins", ".codex", ".agents", ".hermes"]
        .iter()
        .map(|r| home.join(r))
        .filter(|p| p.exists())
        .collect();
    let plugins = collate::survey_plugins(&plugin_roots, 6);
    let plugin_conflicts = collate::plugin_conflicts(plugins.clone());
    collate::report_plugin_conflicts(service.index(), &plugin_conflicts)?;
    let interesting: Vec<Value> = clusters
        .iter()
        .filter(|c| a.all || c.is_conflict() || c.is_duplicate())
        .map(|c| {
            jval!({
                "name": c.name,
                "copies": c.observations.len(),
                "distinct_contents": c.distinct_contents(),
                "versions": c.versions(),
                "conflict": c.is_conflict(),
                "duplicate": c.is_duplicate(),
                "where": c.observations.iter().map(|o| jval!({
                    "root": o.root_label,
                    "path": o.path.display().to_string(),
                    "version": o.version,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    let data = jval!({
        "roots": report.roots,
        "skills": report.skills,
        "names": report.names,
        "conflicts": report.conflicts,
        "duplicates": report.duplicates,
        "clusters": interesting,
        "plugins": plugins.len(),
        "plugin_conflicts": plugin_conflicts.iter().map(|c| jval!({
            "name": c.name,
            "versions": c.versions(),
            "installations": c.installations.iter().map(|i| jval!({
                "version": i.version,
                "path": i.path.display().to_string(),
                "skills": i.skills,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    Ok(reply(&service, data, service.load_warnings()))
}

fn open_palette(cwd: &std::path::Path, query: Option<String>, fullscreen: bool) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    ui::run(&mut service, query, fullscreen)?;
    Ok(Reply::Silent)
}

fn cmd_search(cwd: &std::path::Path, a: SearchArgs) -> Result<Reply> {
    use aikit_cli::app::SearchRequest;
    let service = Service::discover(cwd)?;
    let results = service.search(SearchRequest {
        query: a.query,
        limit: a.limit,
    })?;
    let rows: Vec<Value> = results
        .rows
        .iter()
        .map(|r| {
            jval!({
                "id": r.id.to_string(),
                "name": r.name,
                "kind": r.kind.as_str(),
                "active": r.active,
                "runnable": r.runnable,
            })
        })
        .collect();
    Ok(reply(&service, jval!({ "rows": rows }), results.warnings))
}

fn cmd_status(cwd: &std::path::Path, a: StatusArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let view = service.resolved();
    let active: Vec<Value> = view
        .active
        .values()
        .map(|c| {
            jval!({
                "id": c.id.to_string(),
                "kind": c.kind.as_str(),
                "name": c.name,
                "exports": c.exports,
            })
        })
        .collect();
    let properties = service.current_generation_properties();
    let mut data = jval!({
        "active": active,
        "active_count": view.active.len(),
        "hash": view.hash.to_string(),
        "isolation": service.descriptor().isolation.as_str(),
        "bypasses": bypass_summaries(&service)?,
        "generation_label": properties.get("label"),
        "generation_properties": properties,
    });
    if a.all {
        let unavailable: Vec<Value> = view
            .unavailable
            .iter()
            .map(|(id, reason)| jval!({ "id": id.to_string(), "reason": reason.describe() }))
            .collect();
        data["unavailable"] = jval!(unavailable);
    }
    Ok(reply(&service, data, service.load_warnings()))
}

fn cmd_explain(cwd: &std::path::Path, a: ExplainArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let id = CapsuleId::parse(&a.capability)?;
    let explanation = service.resolved().explain(&id).ok_or_else(|| {
        AikitError::new(
            "resolution.unknown_capability",
            format!("{id} is not in the catalogue for this context"),
        )
        .with("capability", id.to_string())
    })?;
    let data = jval!({
        "id": explanation.id.to_string(),
        "active": explanation.active,
        "declared_enabled": explanation.declared_enabled,
        "selected_by": explanation.selected_by,
        "required_by": explanation.required_by,
        "dependencies": explanation.dependencies,
        "exports": explanation.exports,
        "unavailable": explanation.unavailable.as_ref().map(|r| r.describe()),
        "render": explanation.render(),
    });
    Ok(reply(&service, data, vec![]))
}

fn cmd_run(cwd: &std::path::Path, a: RunArgs) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    let handle = service.run(RunRequest {
        name: a.name,
        args: a.args,
        export: None,
        // The confirmation must be explicit (`--confirm`), not implied by the
        // invocation: `aikit run` is also how the broker lets an *agent* run a
        // capability, and how a multicall shim on the PATH dispatches. A reviewed
        // or trusted script never trips the gate, so this only asks when the
        // executable is genuinely unreviewed.
        confirmed: a.confirm,
    })?;
    for line in &handle.report.output {
        println!("{line}");
    }
    Ok(Reply::Status(handle.report.status))
}

fn cmd_toggle(cwd: &std::path::Path, a: ToggleArgs, enable: bool) -> Result<Reply> {
    use aikit_tui::backend::Toggle;
    let mut service = Service::discover(cwd)?;
    let id = CapsuleId::parse(&a.capability)?;
    let scope = resolve_scope(&service, a.scope.as_deref())?;
    let applied = service.apply(ApplyRequest {
        scope,
        toggles: vec![Toggle::new(id.clone(), enable)],
        label: None,
    })?;
    let data = jval!({
        "capability": id.to_string(),
        "enabled": enable,
        "scope": scope.as_str(),
        "generation": applied.id.to_string(),
        "replaced": applied.replaced.as_ref().map(|g| g.to_string()),
    });
    Ok(reply(&service, data, applied.warnings))
}

fn cmd_apply(cwd: &std::path::Path, a: ApplyArgs) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    let scope = service.descriptor().default_mutation_scope();
    let applied = service.apply(ApplyRequest {
        scope,
        toggles: vec![],
        label: a.label.clone(),
    })?;
    let data = jval!({
        "generation": applied.id.to_string(),
        "replaced": applied.replaced.as_ref().map(|g| g.to_string()),
        "label": a.label,
    });
    Ok(reply(&service, data, applied.warnings))
}

fn cmd_rollback(cwd: &std::path::Path) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let outcome = service.rollback()?;
    let data = jval!({
        "was_current": outcome.was_current.to_string(),
        "now_current": outcome.now_current.to_string(),
    });
    Ok(reply(&service, data, vec![]))
}

fn cmd_prune(cwd: &std::path::Path, a: PruneArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let removed = service.prune(a.keep)?;
    let data = jval!({
        "kept": a.keep,
        "removed": removed.iter().map(|g| g.to_string()).collect::<Vec<_>>(),
    });
    Ok(reply(&service, data, vec![]))
}

fn cmd_context(cwd: &std::path::Path, c: ContextCmd) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    match c.command {
        ContextSub::Current(_) => {
            let d = service.descriptor();
            let data = jval!({
                "context_id": d.context_id.to_string(),
                "session_id": d.session_id.as_ref().map(|s| s.to_string()),
                "project_root": d.project_root.as_ref().map(|p| p.display().to_string()),
                "task": d.task,
                "isolation": d.isolation.as_str(),
                "host": d.host,
                "mux": d.mux.map(|m| m.as_str()),
                "targets": d.targets.iter().map(|t| t.as_str().to_string()).collect::<Vec<_>>(),
            });
            Ok(reply(&service, data, vec![]))
        }
        _ => Err(not_implemented("context")),
    }
}

fn cmd_task(cwd: &std::path::Path, c: TaskCmd) -> Result<Reply> {
    use aikit_cli::task;
    let service = Service::discover(cwd)?;
    let repo = service
        .descriptor()
        .project_root
        .clone()
        .unwrap_or_else(|| cwd.to_path_buf());
    match c.command {
        TaskSub::Spawn(a) => {
            let outcome = task::spawn(&repo, &a.name, a.isolation())?;
            let data = jval!({
                "name": outcome.name,
                "isolation": outcome.isolation.as_str(),
                "worktree": outcome.worktree.as_ref().map(|w| jval!({
                    "path": w.path.display().to_string(),
                    "branch": w.branch,
                })),
                "directory": outcome.directory.display().to_string(),
                "note": outcome.note,
            });
            Ok(reply(&service, data, vec![]))
        }
        TaskSub::Close(a) => {
            // Close by name and let the task module detect the isolation; a
            // shared task (the default) has no worktree to remove.
            let tree = task::detect_task(&repo, &a.name)?;
            task::close_task(&repo, &a.name, a.force)?;
            let kind = match tree {
                task::TaskTree::Shared => "shared",
                task::TaskTree::Directory(_) => "directory",
                task::TaskTree::Worktree(_) => "worktree",
            };
            let data = jval!({ "closed": a.name, "forced": a.force, "isolation": kind });
            Ok(reply(&service, data, vec![]))
        }
        TaskSub::List(_) => Err(not_implemented("task list")),
    }
}

fn cmd_bypass(cwd: &std::path::Path, c: BypassCmd) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    match c.command {
        BypassSub::Issue(a) => {
            if a.reason.as_deref().map(str::trim).unwrap_or("").is_empty() {
                return Err(AikitError::new(
                    "cli.usage",
                    "a bypass must carry a reason; pass --reason",
                ));
            }
            let id = service.issue_bypass(&a.scope, a.reason.as_deref(), a.capability.as_deref())?;
            let data = jval!({
                "bypass_id": id,
                "scope": a.scope,
                "reason": a.reason,
                "capability": a.capability,
            });
            Ok(reply(&service, data, vec![]))
        }
        BypassSub::List(_) => cmd_bypasses(cwd),
        BypassSub::Revoke(_) => Err(not_implemented("bypass revoke")),
    }
}

fn cmd_bypasses(cwd: &std::path::Path) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    Ok(reply(&service, jval!({ "bypasses": bypass_summaries(&service)? }), vec![]))
}

fn cmd_hook(cwd: &std::path::Path, c: HookCmd) -> Result<Reply> {
    let HookSub::Dispatch(a) = c.command;
    let service = Service::discover(cwd)?;

    let payload: Value = read_stdin_json();
    let event: HookEvent = hook::normalize(&a.client, &a.event, payload);
    let decision = service.dispatch_hook(&event)?;

    let data = jval!({
        "event": a.event,
        "client": a.client,
        "allowed": decision.allowed,
        "denial": decision.denial.as_ref().map(|d| d.describe()),
        "injected": decision.injected_text(),
        "bypassed": decision.was_bypassed(),
        "warnings": decision.warnings,
    });
    Ok(reply(&service, data, vec![]))
}

fn cmd_capabilities(cwd: &std::path::Path, c: CapabilitiesCmd) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    match c.command {
        CapabilitiesSub::List(_) => {
            let commands: Vec<Value> = service
                .resolved()
                .exported_commands()
                .into_iter()
                .map(|(name, id)| jval!({ "command": name, "capability": id.to_string() }))
                .collect();
            Ok(reply(&service, jval!({ "commands": commands }), vec![]))
        }
        CapabilitiesSub::Read(a) => {
            let id = CapsuleId::parse(&a.capability)?;
            let capsule = service.snapshot();
            let cap = aikit_core::catalog::Catalog::get(capsule, &id).ok_or_else(|| {
                AikitError::new(
                    "resolution.unknown_capability",
                    format!("{id} is not in the catalogue"),
                )
            })?;
            let data = jval!({
                "id": id.to_string(),
                "name": cap.name,
                "description": cap.description,
                "kind": cap.kind.as_str(),
                "tags": cap.tags,
            });
            Ok(reply(&service, data, vec![]))
        }
    }
}

fn cmd_session(cwd: &std::path::Path, c: SessionCmd) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    match c.command {
        SessionSub::Up(a) => {
            let result = service.session_up(SessionRequest { spec: a.spec })?;
            let data = jval!({ "summary": result.summary });
            Ok(reply(&service, data, result.warnings))
        }
        _ => Err(not_implemented("session")),
    }
}

/// `aikit inbox` — list the messages the system and agents have addressed to the
/// user (Spec II §2). Pending by default; `--all` includes resolved items kept for
/// audit. Speaks the JSON envelope so the broker and agents can read it.
fn cmd_inbox(cwd: &std::path::Path, a: InboxArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let items = service.inbox_items(a.all)?;
    let rows: Vec<Value> = items
        .iter()
        .map(|item| {
            jval!({
                "id": item.id.to_string(),
                "kind": item.kind.as_str(),
                "title": item.title,
                "body": item.body,
                "state": item.state,
                "project": item.project.as_ref().map(|p| p.to_string()),
                "evidence": item.evidence,
                "proposal": item.proposal.as_ref().map(|p| p.to_string()),
                "created": item.created.to_string(),
            })
        })
        .collect();
    let data = jval!({ "items": rows, "count": items.len() });
    Ok(reply(&service, data, vec![]))
}

fn cmd_promote(cwd: &std::path::Path, a: PromoteArgs) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    let id = a.id.as_deref().map(CapsuleId::parse).transpose()?;
    let promoted = service.promote(PromoteRequest {
        candidate: a.candidate,
        id,
    })?;
    let data = jval!({
        "id": promoted.id.to_string(),
        "root": promoted.root.display().to_string(),
        "manifest": promoted.manifest_path.display().to_string(),
    });
    Ok(reply(&service, data, vec![]))
}

fn cmd_shell(c: ShellCmd) -> Result<Reply> {
    use aikit_adapters::shells::{init_snippet, Shell};
    let ShellSub::Init(a) = c.command;
    let shell = match a.shell.as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        other => {
            return Err(AikitError::new(
                "cli.usage",
                format!("`{other}` is not a supported shell; use bash, zsh or fish"),
            ))
        }
    };
    Ok(Reply::Text(init_snippet(shell)))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn bypass_summaries(service: &Service) -> Result<Vec<Value>> {
    Ok(service
        .open_bypasses()?
        .into_iter()
        .map(|record| {
            jval!({
                "bypass_id": record.bypass_id,
                "scope": record.token.scope.as_str(),
                "reason": record.token.reason,
                "capability": record.token.issued_for.as_ref().map(|c| c.to_string()),
            })
        })
        .collect())
}

fn resolve_scope(service: &Service, scope: Option<&str>) -> Result<ScopeKind> {
    match scope {
        None => Ok(service.descriptor().default_mutation_scope()),
        Some(raw) => parse_scope(raw),
    }
}

fn parse_scope(raw: &str) -> Result<ScopeKind> {
    ScopeKind::ALL
        .into_iter()
        .find(|s| s.as_str() == raw)
        .ok_or_else(|| {
            AikitError::new("cli.usage", format!("`{raw}` is not a scope"))
                .with("scope", raw.to_string())
        })
}

fn read_stdin_json() -> Value {
    use std::io::Read;
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
        serde_json::from_str(&buf).unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}

fn not_implemented(command: &str) -> AikitError {
    AikitError::new(
        "command.not_implemented",
        format!("`{command}` is recognised but not wired up in this build"),
    )
    .with("command", command.to_string())
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Init(_) => "init",
        Command::Collate(_) => "collate",
        Command::Ui(_) => "ui",
        Command::Search(_) => "search",
        Command::Status(_) => "status",
        Command::Explain(_) => "explain",
        Command::Diff(_) => "diff",
        Command::Doctor(_) => "doctor",
        Command::Run(_) => "run",
        Command::Enable(_) => "enable",
        Command::Disable(_) => "disable",
        Command::Use(_) => "use",
        Command::Apply(_) => "apply",
        Command::Rollback(_) => "rollback",
        Command::Context(_) => "context",
        Command::Session(_) => "session",
        Command::Task(_) => "task",
        Command::Inbox(_) => "inbox",
        Command::Capture(_) => "capture",
        Command::Promote(_) => "promote",
        Command::Prune(_) => "prune",
        Command::Bypass(_) => "bypass",
        Command::Client(_) => "client",
        Command::Mux(_) => "mux",
        Command::Hook(_) => "hook",
        Command::Capabilities(_) => "capabilities",
        Command::Jobs(_) => "jobs",
        Command::Recent(_) => "recent",
        Command::Stats(_) => "stats",
        Command::Log(_) => "log",
        Command::Shell(_) => "shell",
        Command::Unused(_) => "unused",
        Command::Failures(_) => "failures",
        Command::Bypasses(_) => "bypasses",
    }
}
