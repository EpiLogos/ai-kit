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
    // `--json` implies a dry run for `z`: a machine reading candidates is deciding,
    // not delegating the decision.
    let json_mode = cli.json;
    match cli.command {
        None => open_palette(cwd, None, false),
        Some(Command::Init(a)) => cmd_init(cwd, a),
        Some(Command::Collate(a)) => cmd_collate(cwd, a),
        Some(Command::Z(a)) => cmd_z(cwd, a, json_mode),
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
        Some(Command::Capture(a)) => cmd_capture(cwd, a),
        Some(Command::Diff(_)) => cmd_diff(cwd),
        Some(Command::Doctor(a)) => cmd_doctor(cwd, a),
        Some(Command::Use(a)) => cmd_use(cwd, a),
        Some(Command::Recent(a)) => cmd_recent(cwd, a.limit),
        Some(Command::Failures(a)) => cmd_failures(cwd, a.limit),
        Some(Command::Stats(_)) => cmd_stats(cwd),
        Some(Command::Unused(_)) => cmd_unused(cwd),
        Some(Command::Jobs(_)) => cmd_jobs(cwd),
        Some(Command::Log(c)) => cmd_log(cwd, c),
        Some(Command::Client(c)) => cmd_client(cwd, c),
        Some(Command::Mux(c)) => cmd_mux(cwd, c),
        Some(Command::Shell(c)) => cmd_shell(c),
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

/// `aikit z <words…>` — the single verb.
///
/// Acts on one clear winner, opens the palette pre-filtered when the top is
/// contested, and reports nothing rather than guessing. It NEVER activates: the
/// action for a script is to run it, for a skill to show it. Making something
/// active is a different act, and a fuzzy match is not consent to it.
fn cmd_z(cwd: &std::path::Path, a: ZArgs, json_mode: bool) -> Result<Reply> {
    use aikit_cli::jump::{self, JumpAction};
    use aikit_core::frecency::Jump;

    let mut service = Service::discover(cwd)?;
    let query = a.words.join(" ");
    let plan = jump::plan(&service, &query)?;
    let dry_run = a.dry_run || json_mode;

    let candidates: Vec<Value> = plan
        .ranked
        .iter()
        .take(20)
        .map(|c| {
            jval!({
                "capability": c.id.to_string(),
                "score": c.score,
                "exact_export_name": c.exact_export_name,
                "active": c.active_in_context,
                "in_current_project": c.in_current_project,
                "successful_runs": c.usage.successful_runs,
            })
        })
        .collect();

    match &plan.jump {
        Jump::Nothing => {
            let data = jval!({
                "query": plan.query,
                "decision": "nothing",
                "candidates": candidates,
            });
            Ok(reply(&service, data, vec![]))
        }
        Jump::Disambiguate { candidates: ids } => {
            let data = jval!({
                "query": plan.query,
                "decision": "disambiguate",
                "candidates": candidates,
                "tied": ids.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
            });
            if dry_run {
                return Ok(reply(&service, data, vec![]));
            }
            // Ambiguity is the interactive case, not an error: open the palette
            // pre-filtered to what was meant.
            ui::run(&mut service, Some(plan.query.clone()), false)?;
            Ok(Reply::Silent)
        }
        Jump::Act { capsule } => {
            let action = plan.action(&service).expect("an Act decision has an action");
            let data = jval!({
                "query": plan.query,
                "decision": "act",
                "action": action.as_str(),
                "capability": capsule.to_string(),
                "candidates": candidates,
                "activated": false,
            });
            if dry_run {
                return Ok(reply(&service, data, vec![]));
            }
            match action {
                JumpAction::Run { capsule } => {
                    let handle = service.run(RunRequest {
                        name: capsule.to_string(),
                        args: vec![],
                        export: None,
                        confirmed: false,
                    })?;
                    for line in &handle.report.output {
                        println!("{line}");
                    }
                    Ok(Reply::Status(handle.report.status))
                }
                // Showing a capability is reading it, never enabling it.
                JumpAction::Open { capsule } | JumpAction::Session { capsule } => {
                    let explanation = service.resolved().explain(&capsule).ok_or_else(|| {
                        AikitError::new(
                            "resolution.unknown_capability",
                            format!("{capsule} is not in the catalogue for this context"),
                        )
                    })?;
                    Ok(Reply::Text(explanation.render()))
                }
            }
        }
    }
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
        ContextSub::List(_) => {
            use aikit_store::state::StateStore;
            let store = StateStore::new(service.index());
            let bindings = store.bindings()?;
            let rows: Vec<Value> = store
                .contexts()?
                .into_iter()
                .map(|c| {
                    let bound = bindings.iter().find(|b| b.context_id == c.context_id);
                    jval!({
                        "context_id": c.context_id.to_string(),
                        "session_id": c.session_id.as_ref().map(|s| s.to_string()),
                        "project_root": c.project_root.as_ref().map(|p| p.display().to_string()),
                        "task": c.task,
                        "isolation": c.isolation.as_str(),
                        "current": c.context_id == service.descriptor().context_id,
                        "bound_to": bound.map(|b| jval!({
                            "mux": b.mux.as_str(),
                            "mux_session": b.mux_session,
                            "mux_surface": b.mux_surface,
                        })),
                    })
                })
                .collect();
            Ok(reply(&service, jval!({ "contexts": rows }), vec![]))
        }
        ContextSub::Bind(a) => {
            use aikit_adapters::mux::{plain::Plain, tmux::Tmux, MuxAdapter};
            use aikit_core::context::ContextBinding;
            use aikit_store::state::StateStore;

            let descriptor = service.descriptor();
            // Ask the real multiplexer where we are rather than accepting a claim:
            // a binding that says "pane %7" when the pane is gone is worse than none.
            let tmux = Tmux::system();
            let location = if tmux.detect().map(|p| p.inside).unwrap_or(false) {
                tmux.current_location()?
            } else {
                Plain::new().current_location()?
            };

            let session = match a.session.as_deref() {
                Some(raw) => aikit_core::SessionId::parse(raw)?,
                None => descriptor.session_id.clone().ok_or_else(|| {
                    AikitError::new(
                        "context.no_session",
                        "this context has no AIKit session; pass --session to name one",
                    )
                })?,
            };

            let binding = ContextBinding {
                context_id: descriptor.context_id.clone(),
                session_id: session,
                mux: location.kind,
                mux_session: location.session.clone(),
                mux_surface: location.surface.clone(),
                project_root: descriptor.project_root.clone(),
                isolation: descriptor.isolation,
            };
            StateStore::new(service.index()).bind_context(&binding)?;

            let data = jval!({
                "context_id": binding.context_id.to_string(),
                "session_id": binding.session_id.to_string(),
                "mux": binding.mux.as_str(),
                "mux_session": binding.mux_session,
                "mux_surface": binding.mux_surface,
            });
            Ok(reply(&service, data, vec![]))
        }
        ContextSub::Reset(_) => {
            use aikit_store::state::StateStore;
            // Forgetting a binding is not forgetting the context: the generations,
            // the overlay and the history all survive.
            let forgotten =
                StateStore::new(service.index()).unbind_context(&service.descriptor().context_id)?;
            let data = jval!({
                "context_id": service.descriptor().context_id.to_string(),
                "forgot_binding": forgotten,
            });
            Ok(reply(&service, data, vec![]))
        }
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
        TaskSub::List(_) => {
            let tasks = task::list(&repo)?;
            let rows: Vec<Value> = tasks
                .iter()
                .map(|t| {
                    jval!({
                        "name": t.name,
                        "isolation": t.isolation.as_str(),
                        "path": t.path.display().to_string(),
                        "branch": t.branch,
                        "dirty": t.dirty,
                    })
                })
                .collect();
            Ok(reply(&service, jval!({ "tasks": rows, "count": rows.len() }), vec![]))
        }
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
        BypassSub::Revoke(a) => {
            service.index().revoke_bypass(&a.id)?;
            let data = jval!({ "bypass_id": a.id, "revoked": true });
            Ok(reply(&service, data, vec![]))
        }
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
        SessionSub::List(_) => {
            use aikit_store::state::StateStore;
            let rows: Vec<Value> = StateStore::new(service.index())
                .sessions()?
                .into_iter()
                .map(|s| {
                    jval!({
                        "session_id": s.session_id.to_string(),
                        "name": s.name,
                        "project_root": s.project_root.as_ref().map(|p| p.display().to_string()),
                        "mux": s.mux.as_str(),
                        "mux_session": s.mux_session,
                        "state": s.state.as_str(),
                        "last_seen": s.last_seen.to_string(),
                    })
                })
                .collect();
            Ok(reply(&service, jval!({ "sessions": rows, "count": rows.len() }), vec![]))
        }
        SessionSub::Attach(a) => {
            let (argv, mux) = session_argv(&service, &a.session, "attach")?;
            let data = jval!({ "session": a.session, "mux": mux, "command": argv });
            Ok(reply(&service, data, vec![]))
        }
        SessionSub::Diff(a) => {
            let outcome = service.session_diff(a.session.as_deref())?;
            let data = jval!({
                "session": outcome.session,
                "mux": outcome.mux,
                "matches_spec": outcome.differences.is_empty(),
                "differences": outcome.differences,
            });
            Ok(reply(&service, data, outcome.warnings))
        }
        SessionSub::Reconcile(a) => {
            let outcome = service.session_reconcile(a.session.as_deref(), a.destructive)?;
            let data = jval!({
                "session": outcome.session,
                "mux": outcome.mux,
                "destructive": a.destructive,
                "actions": outcome.actions,
                "preserved": outcome.preserved,
            });
            Ok(reply(&service, data, outcome.warnings))
        }
        SessionSub::Down(a) => {
            let (argv, mux) = session_argv(&service, &a.session, "kill")?;
            let data = jval!({
                "session": a.session,
                "mux": mux,
                "command": argv,
                "note": "AIKit prints the teardown command rather than running it: closing a \
                         session can discard work in a pane AIKit never started",
            });
            Ok(reply(&service, data, vec![]))
        }
    }
}

/// The argv that attaches to or tears down a session in whichever multiplexer is
/// actually present.
///
/// AIKit prints these rather than running them: attaching replaces the current
/// process, and tearing down can discard work in a pane AIKit never started.
/// Handing the user the exact command keeps both decisions theirs.
fn session_argv(service: &Service, session: &str, verb: &str) -> Result<(Vec<String>, String)> {
    use aikit_adapters::mux::{tmux::Tmux, MuxAdapter};
    let tmux = Tmux::system();
    let present = tmux.detect().map(|p| p.installed).unwrap_or(false);
    if !present {
        return Err(AikitError::new(
            "mux.none_detected",
            format!("no multiplexer is available to {verb} `{session}`"),
        )
        .with("session", session.to_string()));
    }
    let _ = service;
    let argv = match verb {
        "attach" => vec!["tmux".into(), "attach-session".into(), "-t".into(), session.to_string()],
        _ => vec!["tmux".into(), "kill-session".into(), "-t".into(), session.to_string()],
    };
    Ok((argv, "tmux".to_string()))
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

/// `aikit capture` — put something observed into the inbox, scanned first.
fn cmd_capture(cwd: &std::path::Path, a: CaptureArgs) -> Result<Reply> {
    use aikit_store::inbox::{Capture, Inbox};
    let service = Service::discover(cwd)?;

    let body = match a.body {
        Some(text) => text,
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).map_err(|e| {
                AikitError::new("cli.stdin_unreadable", format!("could not read stdin: {e}"))
            })?;
            buf
        }
    };
    if body.trim().is_empty() {
        return Err(AikitError::new(
            "cli.usage",
            "nothing to capture; pass --body or pipe the text in",
        ));
    }

    let inbox = Inbox::new(service.home(), service.index());
    let capture = Capture {
        title: a.title,
        body,
        suggested_kind: None,
        exports: vec![],
        project_root: service.descriptor().project_root.clone(),
        session: service.descriptor().session_id.clone(),
    };
    // Scanned before storage, never before display: a secret must not reach a file.
    let outcome = inbox.capture_against(capture, service.snapshot())?;

    let data = jval!({
        "candidate": outcome.candidate.id,
        "kind": outcome.candidate.kind.as_str(),
        "state": outcome.candidate.state.as_str(),
        "quarantined": outcome.candidate.state == aikit_store::inbox::CandidateState::Quarantined,
        "findings": outcome.candidate.findings.iter().map(|f| jval!({
            "rule": f.rule, "preview": f.preview,
        })).collect::<Vec<_>>(),
        "duplicate_of": outcome.duplicate_of,
        "similar": outcome.similar.iter().map(|s| jval!({
            "other": s.other, "percentage": s.percentage, "summary": s.summary,
        })).collect::<Vec<_>>(),
    });
    Ok(reply(&service, data, vec![]))
}

/// `aikit diff` — what applying the current declarations would change.
///
/// Diff before write, always (STANDARDS §5). This is the same staging path the
/// palette previews with, so the two can never disagree.
fn cmd_diff(cwd: &std::path::Path) -> Result<Reply> {
    use aikit_cli::app::StageRequest;
    let service = Service::discover(cwd)?;
    let scope = service.descriptor().default_mutation_scope();
    let staged = service.stage(StageRequest { scope, toggles: vec![] })?;

    let clean = staged.added_dependencies.is_empty()
        && staged.dropped_dependencies.is_empty()
        && staged.still_unavailable.is_empty();
    let data = jval!({
        "scope": scope.as_str(),
        "clean": clean,
        // Dependencies that would come or go without being asked for: the part of
        // an apply a user does not see coming, so it leads.
        "would_add": staged.added_dependencies.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "would_drop": staged.dropped_dependencies.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "still_unavailable": staged.still_unavailable.iter().map(|(c, why)| jval!({
            "capability": c.to_string(),
            "reason": why,
        })).collect::<Vec<_>>(),
        "effects": staged.client_effects.iter().map(|e| jval!({
            "target": e.target.as_str(),
            "effect": e.effect.describe(),
        })).collect::<Vec<_>>(),
        "active_after": staged.projected.active.len(),
    });
    Ok(reply(&service, data, vec![]))
}

/// `aikit doctor` — the health checks, and with `--fix`, a diff-first Procedure.
fn cmd_doctor(cwd: &std::path::Path, a: DoctorArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let findings = aikit_cli::doctor::run(&service)?;

    let rows: Vec<Value> = findings
        .iter()
        .map(|f| {
            jval!({
                "check": f.check,
                "severity": f.severity.as_str(),
                "summary": f.summary,
                "detail": f.detail,
                "fixable": f.fix.is_some(),
            })
        })
        .collect();

    if !a.fix {
        let data = jval!({
            "findings": rows,
            "count": findings.len(),
            "fixable": findings.iter().filter(|f| f.fix.is_some()).count(),
        });
        return Ok(reply(&service, data, vec![]));
    }

    // --fix is diff-first: the plan is shown before anything is written, and the
    // confirmation is explicit. `--yes` answers it in advance for a non-interactive
    // caller; it does not skip the plan.
    let plan = aikit_cli::doctor::plan_fixes(&service, &findings)?;
    let Some(procedure) = plan else {
        let data = jval!({ "findings": rows, "fixed": 0, "note": "nothing here is automatically fixable" });
        return Ok(reply(&service, data, vec![]));
    };

    let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
    let diff = runner.diff(&procedure)?;
    if !a.yes {
        let data = jval!({
            "findings": rows,
            "would_fix": diff.edits.len(),
            "diff": diff.render(),
            "applied": false,
            "note": "re-run with --yes to apply this plan",
        });
        return Ok(reply(&service, data, vec![]));
    }

    let outcome = runner.run(&procedure)?;
    let data = jval!({
        "findings": rows,
        "procedure": procedure.id.to_string(),
        "applied": true,
        "edits": outcome.applied,
        "undo": format!("aikit procedure undo {}", procedure.id),
    });
    Ok(reply(&service, data, vec![]))
}

/// `aikit use <profile>` — reference a profile from a scope's declaration.
fn cmd_use(cwd: &std::path::Path, a: UseArgs) -> Result<Reply> {
    use aikit_core::id::ProfileId;
    let mut service = Service::discover(cwd)?;
    let profile = ProfileId::parse(&a.profile)?;
    let scope = resolve_scope(&service, a.scope.as_deref())?;

    // Refuse a profile that does not exist rather than writing a declaration that
    // will fail to resolve on the next command.
    if aikit_core::catalog::Catalog::profile(service.snapshot(), &profile).is_none() {
        return Err(AikitError::new(
            "resolution.unknown_profile",
            format!("{profile} is not in any registry"),
        )
        .with("profile", profile.to_string()));
    }

    let applied = service.use_profile(&profile, scope)?;
    let data = jval!({
        "profile": profile.to_string(),
        "scope": scope.as_str(),
        "generation": applied.id.to_string(),
        "replaced": applied.replaced.as_ref().map(|g| g.to_string()),
    });
    Ok(reply(&service, data, applied.warnings))
}

/// `aikit recent` — recently run invocations, newest first.
fn cmd_recent(cwd: &std::path::Path, limit: u32) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let rows: Vec<Value> = service
        .index()
        .recent_events(limit)?
        .into_iter()
        .map(event_json)
        .collect();
    Ok(reply(&service, jval!({ "events": rows, "count": rows.len() }), vec![]))
}

/// `aikit failures` — recent hook and run failures, and denials.
///
/// A system failure and a policy denial are never conflated (ARCHITECTURE §8), so
/// each row says which it was.
fn cmd_failures(cwd: &std::path::Path, limit: u32) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    // Over-fetch, then keep the ones that went wrong: the event log is a single
    // stream and failures are the minority.
    let rows: Vec<Value> = service
        .index()
        .recent_events(limit.saturating_mul(10).max(limit))?
        .into_iter()
        .filter(|e| !e.outcome.is_success())
        .take(limit as usize)
        .map(event_json)
        .collect();
    Ok(reply(&service, jval!({ "failures": rows, "count": rows.len() }), vec![]))
}

fn event_json(e: aikit_store::index::EventSummary) -> Value {
    jval!({
        "event_id": e.event_id,
        "at": e.timestamp.to_string(),
        "action": e.action.as_str(),
        "capability": e.capsule.as_ref().map(|c| c.to_string()),
        "outcome": e.outcome.label(),
        "detail": e.outcome.detail(),
        "bypass_reason": e.bypass_reason,
    })
}

/// `aikit stats` — what is catalogued, and what actually gets used.
fn cmd_stats(cwd: &std::path::Path) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let index = service.index();
    let facets = index.facets()?;
    let view = service.resolved();

    let mut used = 0usize;
    let mut runs = 0u32;
    for row in index.capsules()? {
        let usage = index.usage(&row.id)?;
        if usage.successful_runs > 0 {
            used += 1;
        }
        runs += usage.successful_runs;
    }

    let data = jval!({
        "catalogued": view.catalog_index.len(),
        "active": view.active.len(),
        "unavailable": view.unavailable.len(),
        "ever_used": used,
        "successful_runs": runs,
        "events": index.event_count()?,
        "by_kind": facets.kinds.iter().map(|(k, n)| jval!({ "kind": k.as_str(), "count": n })).collect::<Vec<_>>(),
        "by_source": facets.sources.iter().map(|(s, n)| jval!({ "source": s, "count": n })).collect::<Vec<_>>(),
    });
    Ok(reply(&service, data, vec![]))
}

/// `aikit unused` — catalogued but never successfully used.
///
/// Usage suggests; it never promotes and never archives. This is a list to read,
/// not a list something acts on.
fn cmd_unused(cwd: &std::path::Path) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let index = service.index();
    let mut rows: Vec<Value> = Vec::new();
    for row in index.capsules()? {
        let usage = index.usage(&row.id)?;
        if usage.successful_runs == 0 {
            rows.push(jval!({
                "capability": row.id.to_string(),
                "kind": row.kind.as_str(),
                "name": row.name,
                "failed_runs": usage.failed_runs,
                "active": service.resolved().is_active(&row.id),
            }));
        }
    }
    Ok(reply(&service, jval!({ "unused": rows, "count": rows.len() }), vec![]))
}

/// `aikit jobs` — background invocations AIKit started and has not seen finish.
///
/// There is no daemon, so a "job" is a recorded background run with no completion
/// event. Reporting that honestly beats inventing a process table.
fn cmd_jobs(cwd: &std::path::Path) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let rows: Vec<Value> = service
        .index()
        .recent_events(200)?
        .into_iter()
        .filter(|e| matches!(e.outcome, aikit_store::events::Outcome::Skipped { .. }))
        .map(event_json)
        .collect();
    let data = jval!({
        "jobs": rows,
        "count": rows.len(),
        "note": "AIKit runs no daemon; a job is a recorded background run awaiting its completion event",
    });
    Ok(reply(&service, data, vec![]))
}

/// `aikit log export` — the event log as JSON lines.
fn cmd_log(cwd: &std::path::Path, c: LogCmd) -> Result<Reply> {
    let LogSub::Export(a) = c.command;
    let service = Service::discover(cwd)?;
    // The envelope is the contract for `--json` on every substantive command
    // (STANDARDS §5), so the events ride inside it rather than bypassing it as
    // bare JSON lines. Anything wanting raw JSONL pipes `.data.events[]` out.
    let events: Vec<Value> = service
        .index()
        .recent_events(a.limit)?
        .into_iter()
        .map(event_json)
        .collect();
    let data = jval!({ "events": events, "count": events.len(), "limit": a.limit });
    Ok(reply(&service, data, vec![]))
}

/// `aikit client install|launch|status`.
fn cmd_client(cwd: &std::path::Path, c: ClientCmd) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    match c.command {
        ClientSub::Install(a) => {
            // Installing a client's dispatcher entries writes outside AIKit's own
            // state, so it is a Procedure: planned, diffed, reversible.
            let procedure = aikit_cli::client::plan_install(&service, &a.client)?;
            let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
            let diff = runner.diff(&procedure)?;
            let outcome = runner.run(&procedure)?;
            let data = jval!({
                "client": a.client,
                "procedure": procedure.id.to_string(),
                "edits": outcome.applied,
                "diff": diff.render(),
                "undo": format!("aikit procedure undo {}", procedure.id),
            });
            Ok(reply(&service, data, vec![]))
        }
        ClientSub::Launch(a) => {
            let argv = aikit_cli::client::launch_command(&service, &a.client)?;
            let data = jval!({ "client": a.client, "command": argv });
            Ok(reply(&service, data, vec![]))
        }
        ClientSub::Status(a) => {
            let rows = aikit_cli::client::status(&service, a.client.as_deref())?;
            Ok(reply(&service, jval!({ "clients": rows }), vec![]))
        }
    }
}

/// `aikit mux install|detect`.
fn cmd_mux(cwd: &std::path::Path, c: MuxCmd) -> Result<Reply> {
    use aikit_adapters::mux::{cmux::Cmux, plain::Plain, tmux::Tmux, MuxAdapter};
    let service = Service::discover(cwd)?;
    match c.command {
        MuxSub::Detect(_) => {
            let mut rows = Vec::new();
            for presence in [
                Tmux::system().detect(),
                Cmux::system().detect(),
                Plain::new().detect(),
            ]
            .into_iter()
            .flatten()
            {
                rows.push(jval!({
                    "mux": presence.kind.as_str(),
                    "installed": presence.installed,
                    "version": presence.version,
                    "server_running": presence.server_running,
                    "inside": presence.inside,
                    "detail": presence.detail,
                }));
            }
            let data = jval!({
                "detected": rows,
                "active": service.descriptor().mux.map(|m| m.as_str()),
            });
            Ok(reply(&service, data, vec![]))
        }
        MuxSub::Install(a) => {
            let procedure = aikit_cli::mux_install::plan(&service, a.mux.as_deref())?;
            let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
            let diff = runner.diff(&procedure)?;
            let outcome = runner.run(&procedure)?;
            let data = jval!({
                "procedure": procedure.id.to_string(),
                "edits": outcome.applied,
                "diff": diff.render(),
                "undo": format!("aikit procedure undo {}", procedure.id),
            });
            Ok(reply(&service, data, vec![]))
        }
    }
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

