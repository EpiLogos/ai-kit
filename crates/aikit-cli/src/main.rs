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
use aikit_cli::{hook, multicall, run, ui};
use aikit_tui::{application_service::ApplicationService, ExplainHistoryApplicationService};

use aikit_core::hooks::HookEvent;
use aikit_core::id::{CapsuleId, Revision};
use aikit_core::profile::SkillUsageOverlayPatch;
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
            return if e.use_stderr() {
                json::EXIT_USAGE
            } else {
                json::EXIT_OK
            };
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

fn diagnostic_warnings(service: &Service) -> Vec<String> {
    let mut warnings = service.load_warnings();
    warnings.extend(service.resolved().warnings.clone());
    warnings
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
        Some(Command::Source(c)) => cmd_source(cwd, c),
        Some(Command::Skill(c)) => cmd_skill(cwd, c),
        Some(Command::Project(c)) => cmd_project(cwd, c),
        None => open_palette(cwd, None, false),
        Some(Command::Init(a)) => cmd_init(cwd, a),
        Some(Command::Collate(a)) => cmd_collate(cwd, a),
        Some(Command::Adopt(a)) => cmd_adopt(cwd, a),
        Some(Command::Procedure(c)) => cmd_procedure(cwd, c),
        Some(Command::Profile(c)) => cmd_profile(cwd, c),
        Some(Command::Z(a)) => cmd_z(cwd, a, json_mode),
        Some(Command::Set(c)) => cmd_set(cwd, c),
        Some(Command::Tree(a)) => cmd_tree(cwd, a, json_mode),
        Some(Command::Ui(a)) => open_surface(cwd, a.query, a.fullscreen, a.tree),

        Some(Command::Search(a)) => cmd_search(cwd, a),
        Some(Command::Knowledge(c)) => cmd_knowledge(cwd, c),
        Some(Command::Status(a)) => cmd_status(cwd, a),
        Some(Command::Explain(a)) => cmd_explain(cwd, a),
        Some(Command::History(a)) => cmd_history(cwd, a),
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

fn cmd_skill(cwd: &std::path::Path, command: SkillCmd) -> Result<Reply> {
    let SkillSub::Overlay(overlay) = command.command;
    match overlay.command {
        SkillOverlaySub::Set(args) => {
            let mut service = Service::discover(cwd)?;
            let id = CapsuleId::parse(&args.capability)?;
            require_agent_skill(&service, &id)?;
            let scope = resolve_scope(&service, args.scope.as_deref())?;
            let guidance = match (args.guidance, args.guidance_file) {
                (Some(text), None) => Some(text),
                (None, Some(path)) => Some(std::fs::read_to_string(&path).map_err(|error| {
                    AikitError::new(
                        "skill_overlay.guidance_unreadable",
                        format!("could not read {}: {error}", path.display()),
                    )
                    .with("path", path.display().to_string())
                })?),
                (None, None) => None,
                (Some(_), Some(_)) => unreachable!("clap rejects conflicting guidance inputs"),
            };
            let overlay = SkillUsageOverlayPatch {
                inherit: !args.no_inherit,
                description: args.description,
                guidance,
                reviewed_against: args.reviewed_against.map(Revision::from_raw),
            };
            overlay.validate(&id)?;
            let applied = service.set_skill_usage_overlay(&id, scope, &overlay)?;
            let effective = service
                .resolved()
                .skill_usage_overlays
                .get(&id)
                .cloned()
                .unwrap_or_default();
            Ok(reply(
                &service,
                jval!({
                    "capability": id.to_string(),
                    "scope": scope.as_str(),
                    "generation": applied.id.to_string(),
                    "overlays": effective,
                    "effects": applied.effects.iter().map(|effect| jval!({
                        "target": effect.target.as_str(),
                        "effect": effect.effect.describe(),
                    })).collect::<Vec<_>>(),
                }),
                applied.warnings,
            ))
        }
        SkillOverlaySub::Show(args) => {
            let service = Service::discover(cwd)?;
            let id = CapsuleId::parse(&args.capability)?;
            require_agent_skill(&service, &id)?;
            let effective = service
                .resolved()
                .skill_usage_overlays
                .get(&id)
                .cloned()
                .unwrap_or_default();
            Ok(reply(
                &service,
                jval!({ "capability": id.to_string(), "overlays": effective }),
                diagnostic_warnings(&service),
            ))
        }
        SkillOverlaySub::Clear(args) => {
            let mut service = Service::discover(cwd)?;
            let id = CapsuleId::parse(&args.capability)?;
            require_agent_skill(&service, &id)?;
            let scope = resolve_scope(&service, args.scope.as_deref())?;
            let applied = service.clear_skill_usage_overlay(&id, scope)?;
            let effective = service
                .resolved()
                .skill_usage_overlays
                .get(&id)
                .cloned()
                .unwrap_or_default();
            Ok(reply(
                &service,
                jval!({
                    "capability": id.to_string(),
                    "scope": scope.as_str(),
                    "generation": applied.id.to_string(),
                    "overlays": effective,
                    "effects": applied.effects.iter().map(|effect| jval!({
                        "target": effect.target.as_str(),
                        "effect": effect.effect.describe(),
                    })).collect::<Vec<_>>(),
                }),
                applied.warnings,
            ))
        }
    }
}

fn require_agent_skill(service: &Service, id: &CapsuleId) -> Result<()> {
    let capsule = aikit_core::catalog::Catalog::get(service.snapshot(), id).ok_or_else(|| {
        AikitError::new(
            "skill_overlay.unknown_skill",
            format!("{id} is not in the catalogue"),
        )
        .with("capability", id.to_string())
    })?;
    if capsule.kind != aikit_core::capsule::Kind::Skill {
        return Err(AikitError::new(
            "skill_overlay.not_a_skill",
            format!("{id} is {}, not a skill", capsule.kind.as_str()),
        )
        .with("capability", id.to_string()));
    }
    Ok(())
}

fn cmd_project(cwd: &std::path::Path, command: ProjectCmd) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    match command.command {
        ProjectSub::Bind(args) => {
            let spec = aikit_cli::projects::bind(
                service.home(),
                &args.id,
                &args.directories,
                &args.repositories,
                &args.skill_sets,
                !args.no_default_skill_sets,
            )?;
            Ok(reply(
                &service,
                jval!({
                    "project": spec.id,
                    "directories": spec.directories.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                    "repositories": spec.repositories,
                    "skill_sets": spec.skill_sets,
                    "inherit_default_skill_sets": spec.inherit_default_skill_sets,
                }),
                vec![],
            ))
        }
        ProjectSub::Show(_) => {
            let matched = aikit_cli::projects::resolve(service.home(), cwd)?.ok_or_else(|| {
                AikitError::new(
                    "project.not_matched",
                    format!(
                        "{} is not matched by a Project Specification",
                        cwd.display()
                    ),
                )
            })?;
            Ok(reply(
                &service,
                jval!({
                    "project": matched.spec.id,
                    "root": matched.root.display().to_string(),
                    "matched_by": matched.matched_by,
                    "skill_sets": matched.spec.skill_sets,
                    "inherit_default_skill_sets": matched.spec.inherit_default_skill_sets,
                }),
                vec![],
            ))
        }
        ProjectSub::Defaults(args) => {
            let defaults = aikit_cli::projects::set_defaults(service.home(), &args.skill_sets)?;
            Ok(reply(
                &service,
                jval!({ "default_skill_sets": defaults.default_skill_sets }),
                vec![],
            ))
        }
    }
}

fn cmd_source(cwd: &std::path::Path, command: SourceCmd) -> Result<Reply> {
    use aikit_cli::skill_sources;

    let service = Service::discover(cwd)?;
    let home = service.home();
    match command.command {
        SourceSub::AddDirectory(args) => {
            let spec = skill_sources::add_directory(home, &args.id, &args.directory)?;
            Ok(reply(
                &service,
                jval!({
                    "id": spec.id,
                    "kind": spec.kind.label(),
                    "portable": spec.kind.portable(),
                }),
                vec![],
            ))
        }
        SourceSub::AddGit(args) => {
            let spec = skill_sources::add_git(
                home,
                &args.id,
                &args.repository,
                &args.revision,
                &args.root,
            )?;
            Ok(reply(
                &service,
                jval!({
                    "id": spec.id,
                    "kind": spec.kind.label(),
                    "portable": spec.kind.portable(),
                }),
                vec![],
            ))
        }
        SourceSub::SetRevision(args) => {
            let spec = skill_sources::set_revision(home, &args.id, &args.revision)?;
            let revision = match spec.kind {
                skill_sources::SourceKind::Git { revision, .. } => revision,
                skill_sources::SourceKind::Directory { .. } => unreachable!(),
            };
            Ok(reply(
                &service,
                jval!({
                    "id": spec.id,
                    "revision": revision,
                    "candidate_snapshot": null,
                }),
                vec![],
            ))
        }
        SourceSub::Sync(args) => {
            let snapshot = skill_sources::sync(home, &args.id)?;
            let status = skill_sources::status(home, &args.id)?;
            Ok(reply(
                &service,
                jval!({
                    "id": args.id,
                    "candidate_snapshot": snapshot.digest,
                    "active_snapshot": status.state.active_snapshot,
                    "git_commit": snapshot.git_commit,
                    "skills": snapshot.skills.len(),
                }),
                vec![],
            ))
        }
        SourceSub::Show(args) => {
            let status = skill_sources::status(home, &args.id)?;
            let active_registry = skill_sources::active_registry(home, &args.id)?;
            Ok(reply(
                &service,
                jval!({
                    "id": status.spec.id,
                    "kind": status.spec.kind.label(),
                    "portable": status.spec.kind.portable(),
                    "candidate_snapshot": status.state.candidate_snapshot,
                    "active_snapshot": status.state.active_snapshot,
                    "active_registry": active_registry.map(|path| path.display().to_string()),
                    "candidate_skills": status.candidate.as_ref().map(|record| record.skills.len()),
                    "active_skills": status.active.as_ref().map(|record| record.skills.len()),
                    "rollback_points": status.state.history,
                }),
                vec![],
            ))
        }
        SourceSub::Promote(args) => {
            let (snapshot, trusted) =
                skill_sources::promote(home, &args.id, args.trust, &args.trust_skills)?;
            Ok(reply(
                &service,
                jval!({
                    "id": args.id,
                    "active_snapshot": snapshot.digest,
                    "skills": snapshot.skills.len(),
                    "trusted_skills": trusted,
                }),
                vec![],
            ))
        }
        SourceSub::Rollback(args) => {
            let snapshot = skill_sources::rollback(home, &args.id)?;
            Ok(reply(
                &service,
                jval!({
                    "id": args.id,
                    "active_snapshot": snapshot.digest,
                    "skills": snapshot.skills.len(),
                }),
                vec![],
            ))
        }
    }
}

/// `aikit profile` — project lenses over reusable base profiles.
fn cmd_profile(cwd: &std::path::Path, c: ProfileCmd) -> Result<Reply> {
    use aikit_core::ProfileId;

    let service = Service::discover(cwd)?;
    match c.command {
        ProfileSub::Fork(a) => {
            let base = ProfileId::parse(&a.base)?;
            let fork = aikit_cli::profile_ops::plan_fork(
                &service,
                &base,
                a.name.as_deref(),
                &a.scope,
                &a.params,
            )?;
            let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
            let diff = runner.diff(&fork.procedure)?;
            if !a.yes {
                runner.save(&fork.procedure)?;
                return Ok(reply(
                    &service,
                    jval!({
                        "base": fork.base.to_string(),
                        "fork": fork.fork.to_string(),
                        "path": fork.path.display().to_string(),
                        "diff": diff.render(),
                        "review_digest": fork.review_digest.as_str(),
                        "procedure": fork.procedure.id.to_string(),
                        "digest": fork.procedure.digest.as_str(),
                        "applied": false,
                        "note": format!(
                            "run the exact saved plan with `aikit procedure run {} --expect-digest {}`",
                            fork.procedure.id, fork.procedure.digest
                        ),
                    }),
                    vec![],
                ));
            }
            let expected = a.expect_digest.as_deref().ok_or_else(|| {
                AikitError::new(
                    "procedure.review_required",
                    "profile fork requires the review digest printed by the preview",
                )
                .with("actual", fork.review_digest.as_str())
            })?;
            if expected != fork.review_digest.as_str() {
                return Err(AikitError::new(
                    "procedure.review_mismatch",
                    "the base profile or project declaration changed since it was reviewed; preview it again",
                )
                .with("expected", expected)
                .with("actual", fork.review_digest.as_str()));
            }
            let outcome = runner.run(&fork.procedure)?;
            Ok(reply(
                &service,
                jval!({
                    "base": fork.base.to_string(),
                    "fork": fork.fork.to_string(),
                    "path": fork.path.display().to_string(),
                    "procedure": fork.procedure.id.to_string(),
                    "applied": true,
                    "edits": outcome.applied,
                    "undo": format!("aikit procedure undo {}", fork.procedure.id),
                }),
                vec![],
            ))
        }
        ProfileSub::Diff(a) => {
            let profile = ProfileId::parse(&a.profile)?;
            let data = aikit_cli::profile_ops::diff(&service, &profile)?;
            Ok(reply(&service, data, vec![]))
        }
    }
}

/// `aikit adopt` — move a foreign Agent Skills tree into AIKit ownership.
fn cmd_adopt(cwd: &std::path::Path, a: AdoptArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let adoption = aikit_cli::adopt::plan(service.home(), &a.root, a.namespace.as_deref())?;
    let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
    let diff = runner.diff(&adoption.procedure)?;

    if !a.yes {
        runner.save(&adoption.procedure)?;
        return Ok(reply(
            &service,
            jval!({
                "source": adoption.source.display().to_string(),
                "namespace": adoption.namespace,
                "skills": adoption.capsules.len(),
                "capsules": adoption.capsules.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "diff": diff.render(),
                "review_digest": adoption.review_digest.as_str(),
                "procedure": adoption.procedure.id.to_string(),
                "digest": adoption.procedure.digest.as_str(),
                "applied": false,
                "note": format!(
                    "run the exact saved plan with `aikit procedure run {} --expect-digest {}`",
                    adoption.procedure.id, adoption.procedure.digest
                ),
            }),
            vec![],
        ));
    }

    let expected = a.expect_digest.as_deref().ok_or_else(|| {
        AikitError::new(
            "procedure.review_required",
            "adoption requires the review digest printed by the preview",
        )
        .with("actual", adoption.review_digest.as_str())
    })?;
    if expected != adoption.review_digest.as_str() {
        return Err(AikitError::new(
            "procedure.review_mismatch",
            "the foreign root changed since it was reviewed; preview it again",
        )
        .with("expected", expected)
        .with("actual", adoption.review_digest.as_str()));
    }

    let outcome = runner.run(&adoption.procedure)?;
    Ok(reply(
        &service,
        jval!({
            "source": adoption.source.display().to_string(),
            "namespace": adoption.namespace,
            "skills": adoption.capsules.len(),
            "capsules": adoption.capsules.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "procedure": adoption.procedure.id.to_string(),
            "ownership": "adopted",
            "applied": true,
            "edits": outcome.applied,
            "undo": format!("aikit procedure undo {}", adoption.procedure.id),
        }),
        vec![],
    ))
}

/// `aikit procedure` — read and apply the inverse records produced by Procedures.
fn cmd_procedure(cwd: &std::path::Path, c: ProcedureCmd) -> Result<Reply> {
    use aikit_core::ProcedureId;

    let service = Service::discover(cwd)?;
    let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
    match c.command {
        ProcedureSub::Plan(a) => {
            let (procedure, review_digest) = match a.command {
                ProcedurePlanSub::Adopt(args) => {
                    let adoption = aikit_cli::adopt::plan(
                        service.home(),
                        &args.root,
                        args.namespace.as_deref(),
                    )?;
                    (adoption.procedure, Some(adoption.review_digest))
                }
                ProcedurePlanSub::ProfileFork(args) => {
                    let base = aikit_core::ProfileId::parse(&args.base)?;
                    let fork = aikit_cli::profile_ops::plan_fork(
                        &service,
                        &base,
                        args.name.as_deref(),
                        &args.scope,
                        &args.params,
                    )?;
                    (fork.procedure, Some(fork.review_digest))
                }
            };
            runner.save(&procedure)?;
            let diff = runner.diff(&procedure)?;
            Ok(reply(
                &service,
                jval!({
                    "procedure": procedure.id.to_string(),
                    "kind": procedure.kind.as_str(),
                    "digest": procedure.digest.as_str(),
                    "review_digest": review_digest.map(|digest| digest.as_str().to_string()),
                    "diff": diff.render(),
                    "applied": false,
                    "run": format!(
                        "aikit procedure run {} --expect-digest {}",
                        procedure.id, procedure.digest
                    ),
                }),
                vec![],
            ))
        }
        ProcedureSub::Diff(a) => {
            let id = ProcedureId::parse(&a.procedure)?;
            let procedure = runner.load(&id)?;
            let diff = runner.diff(&procedure)?;
            Ok(reply(
                &service,
                jval!({
                    "procedure": id.to_string(),
                    "kind": procedure.kind.as_str(),
                    "digest": procedure.digest.as_str(),
                    "diff": diff.render(),
                }),
                vec![],
            ))
        }
        ProcedureSub::Run(a) => {
            let id = ProcedureId::parse(&a.procedure)?;
            let procedure = runner.load(&id)?;
            if a.expect_digest != procedure.digest.as_str() {
                return Err(AikitError::new(
                    "procedure.review_mismatch",
                    "the expected digest does not identify this persisted Procedure",
                )
                .with("expected", a.expect_digest)
                .with("actual", procedure.digest.as_str()));
            }
            let outcome = runner.run(&procedure)?;
            Ok(reply(
                &service,
                jval!({
                    "procedure": id.to_string(),
                    "digest": procedure.digest.as_str(),
                    "applied": outcome.applied,
                    "already_satisfied": outcome.already_satisfied,
                    "undo": format!("aikit procedure undo {id}"),
                }),
                vec![],
            ))
        }
        ProcedureSub::Undo(a) => {
            let id = ProcedureId::parse(&a.procedure)?;
            let procedure = runner.load(&id)?;
            let undone = runner.undo(&id)?;
            let warnings = aikit_cli::mux_install::activate_undo(&procedure)?;
            Ok(reply(
                &service,
                jval!({ "procedure": id.to_string(), "undone": undone }),
                warnings,
            ))
        }
        ProcedureSub::List(_) => {
            let procedures = runner.list()?;
            Ok(reply(
                &service,
                jval!({
                    "procedures": procedures.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "count": procedures.len(),
                }),
                vec![],
            ))
        }
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
    let mut roots = foreign::roots_for(&home, cwd);
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
    let npx = foreign::survey_npx_skills(&home, cwd);
    let npx_locks: Vec<Value> = npx
        .locks
        .iter()
        .map(|lock| {
            let entries: Vec<Value> = lock
                .entries
                .iter()
                .map(|entry| {
                    jval!({
                        "name": entry.name,
                        "source": entry.source,
                        "source_type": entry.source_type,
                        "source_url": entry.source_url,
                        "ref": entry.reference,
                        "skill_path": entry.skill_path,
                        "expected_hash": entry.expected_hash,
                        "actual_hash": entry.actual_hash,
                        "hash_matches": entry.hash_matches,
                        "installed_path": entry.installed_path.as_ref().map(|path| path.display().to_string()),
                        "installed": entry.installed,
                    })
                })
                .collect();
            jval!({
                "scope": lock.scope.as_str(),
                "path": lock.path.display().to_string(),
                "version": lock.version,
                "supported": lock.supported,
                "entries": entries,
                "entry_count": lock.entries.len(),
                "note": lock.note,
            })
        })
        .collect();

    let data = jval!({
        "roots": rows,
        "root_count": found.len(),
        "total_skills": total_skills,
        "total_problems": total_problems,
        "npx_skills": {
            "locks": npx_locks,
            "lock_count": npx.locks.len(),
            "entry_count": npx.entries(),
            "authority": "foreign-read-only",
        },
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

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut roots: Vec<collate::ForeignRootRef> = foreign::roots_for(&home, cwd)
        .into_iter()
        .filter(|(_, path)| path.exists())
        .map(|(label, path)| collate::ForeignRootRef { label, path })
        .collect();
    for extra in &a.roots {
        roots.push(collate::ForeignRootRef {
            label: extra
                .file_name()
                .map(|n| format!("@{}", n.to_string_lossy()))
                .unwrap_or_else(|| "@root".into()),
            path: extra.clone(),
        });
    }

    let (report, clusters) = collate::collate(service.index(), &roots)?;

    // Plugin caches keep `<plugin>/<version>/` side by side, which is where the
    // version conflicts that matter actually live. Surveyed separately because a
    // plugin declares its own name and version in a manifest (PRIOR-ART #33),
    // rather than being inferred from a skill's frontmatter.
    let plugin_roots: Vec<PathBuf> = [
        ".claude/plugins",
        ".codex/plugins",
        ".codex",
        ".agents",
        ".hermes",
    ]
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
            let action = plan
                .action(&service)
                .expect("an Act decision has an action");
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

/// `aikit tree` — the organising view.
///
/// Read-only: assembling the tree touches nothing. Prints the rendered rows in
/// human mode and the structured rows under `--json`, so an agent and a screen
/// reader get the same one-line description a person sees.
fn cmd_tree(cwd: &std::path::Path, a: TreeArgs, json_mode: bool) -> Result<Reply> {
    use aikit_tui::tree::{Root, TreeGlyphs};

    let service = Service::discover(cwd)?;
    let mut state = aikit_cli::tree_build::build(&service)?;

    if a.all {
        for root in Root::ALL {
            state.expanded.insert(root.as_str().to_string());
        }
    }
    for path in &a.expand {
        state.expanded.insert(path.clone());
        // Expanding `a/b` is meaningless unless `a` is open too.
        let mut prefix = String::new();
        for segment in path.split('/') {
            prefix = if prefix.is_empty() {
                segment.to_string()
            } else {
                format!("{prefix}/{segment}")
            };
            state.expanded.insert(prefix.clone());
        }
    }
    if let Some(filter) = &a.filter {
        state.filter = filter.clone();
    }

    let glyphs = if a.ascii {
        TreeGlyphs::ascii()
    } else {
        TreeGlyphs::for_glyphs(aikit_tui::layout::Glyphs::from_env())
    };

    if !json_mode {
        return Ok(Reply::Text(
            aikit_tui::tree::render_lines(&state, glyphs).join("\n"),
        ));
    }

    let rows: Vec<Value> = state
        .rows()
        .iter()
        .map(|row| {
            jval!({
                "path": row.path,
                "depth": row.depth,
                "expanded": row.expanded,
                "expandable": row.node.expandable,
                // The same one line a screen reader gets.
                "summary": row.node.summary,
            })
        })
        .collect();
    let data = jval!({ "rows": rows, "count": rows.len() });
    Ok(reply(&service, data, vec![]))
}

/// `aikit set` — skill-sets, the unit you point a harness at.
fn cmd_set(cwd: &std::path::Path, c: SetCmd) -> Result<Reply> {
    use aikit_core::skillset;
    use aikit_store::skillsets;

    let service = Service::discover(cwd)?;
    let home = service.home();
    let view = service.resolved();

    match c.command {
        SetSub::List(_) => {
            let sets = skillsets::load_all(home)?;
            let rows: Vec<Value> = sets
                .iter()
                .map(|set| {
                    let projection = skillset::project(set, view);
                    jval!({
                        "name": set.label(),
                        "provenance": set.provenance.as_str(),
                        "members": set.len(),
                        "projected": projection.projected.len(),
                        "withheld": projection.withheld.len(),
                        "summary": projection.summarize(&format!("sets/{}", set.name)),
                    })
                })
                .collect();
            Ok(reply(
                &service,
                jval!({ "sets": rows, "count": rows.len() }),
                vec![],
            ))
        }
        SetSub::Show(a) => {
            let set = skillsets::load(home, &a.name)?;
            let projection = skillset::project(&set, view);
            // The reply to the request: what projects, and what does not, with the
            // resolver's own reason for each withholding.
            let data = jval!({
                "name": set.label(),
                "provenance": set.provenance.as_str(),
                "description": set.description,
                "members": set.len(),
                "summary": projection.summarize(&format!("sets/{}", set.name)),
                "projected": projection.projected.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                "withheld": projection.withheld.iter().map(|w| jval!({
                    "capability": w.capsule.to_string(),
                    "reason": w.reason.describe(),
                })).collect::<Vec<_>>(),
                "complete": projection.is_complete(),
                "children": set.children.iter().map(|c| jval!({
                    "name": c.name, "members": c.len(),
                })).collect::<Vec<_>>(),
                "patterns": set.patterns,
            });
            Ok(reply(&service, data, vec![]))
        }
        SetSub::Create(a) => {
            let mut ids: Vec<CapsuleId> = a
                .ids
                .iter()
                .map(|raw| CapsuleId::parse(raw))
                .collect::<Result<Vec<_>>>()?;

            // Globs expand NOW, to explicit ids. If sets matched dynamically,
            // syncing a registry would silently change what a harness sees —
            // precisely the failure rule 6 exists to prevent.
            for glob in &a.globs {
                for id in view.catalog_index.keys() {
                    if skillset::glob_matches(glob, &id.to_string()) && !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
            }
            ids.sort();

            let procedure = skillsets::plan_create(home, &a.name, &ids, &a.globs)?;
            let runner = aikit_store::procedure::ProcedureRunner::new(home);
            let diff = runner.diff(&procedure)?;
            let outcome = runner.run(&procedure)?;
            let set = skillsets::load(home, &a.name)?;
            let data = jval!({
                "name": set.label(),
                "members": set.len(),
                "expanded_from": a.globs,
                "path": skillsets::dir(home, &a.name).display().to_string(),
                "procedure": procedure.id.to_string(),
                "edits": outcome.applied,
                "diff": diff.render(),
                "undo": format!("aikit procedure undo {}", procedure.id),
            });
            Ok(reply(&service, data, vec![]))
        }
        SetSub::Add(a) => {
            let ids: Vec<CapsuleId> = a
                .ids
                .iter()
                .map(|r| CapsuleId::parse(r))
                .collect::<Result<_>>()?;
            let procedure = skillsets::plan_add(home, &a.name, &ids)?;
            let runner = aikit_store::procedure::ProcedureRunner::new(home);
            let outcome = runner.run(&procedure)?;
            let set = skillsets::load(home, &a.name)?;
            Ok(reply(
                &service,
                jval!({
                    "name": set.label(),
                    "members": set.len(),
                    "procedure": procedure.id.to_string(),
                    "edits": outcome.applied,
                    "undo": format!("aikit procedure undo {}", procedure.id),
                }),
                vec![],
            ))
        }
        SetSub::Remove(a) => {
            let ids: Vec<CapsuleId> = a
                .ids
                .iter()
                .map(|r| CapsuleId::parse(r))
                .collect::<Result<_>>()?;
            let procedure = skillsets::plan_remove(home, &a.name, &ids)?;
            let runner = aikit_store::procedure::ProcedureRunner::new(home);
            let outcome = runner.run(&procedure)?;
            let set = skillsets::load(home, &a.name)?;
            // Removing from a set never deletes the capability: a set is a view.
            Ok(reply(
                &service,
                jval!({
                    "name": set.label(),
                    "members": set.len(),
                    "procedure": procedure.id.to_string(),
                    "edits": outcome.applied,
                    "undo": format!("aikit procedure undo {}", procedure.id),
                }),
                vec![],
            ))
        }
        SetSub::Rename(a) => {
            let procedure = skillsets::plan_rename(home, &a.from, &a.to)?;
            let outcome = aikit_store::procedure::ProcedureRunner::new(home).run(&procedure)?;
            let set = skillsets::load(home, &a.to)?;
            Ok(reply(
                &service,
                jval!({
                    "name": set.label(),
                    "members": set.len(),
                    "procedure": procedure.id.to_string(),
                    "edits": outcome.applied,
                    "undo": format!("aikit procedure undo {}", procedure.id),
                }),
                vec![],
            ))
        }
        SetSub::Delete(a) => {
            let (procedure, recovery) = skillsets::plan_delete(home, &a.name)?;
            let outcome = aikit_store::procedure::ProcedureRunner::new(home).run(&procedure)?;
            Ok(reply(
                &service,
                jval!({
                    "name": a.name,
                    "deleted": true,
                    "recovery": recovery.display().to_string(),
                    "procedure": procedure.id.to_string(),
                    "edits": outcome.applied,
                    "undo": format!("aikit procedure undo {}", procedure.id),
                }),
                vec![],
            ))
        }
    }
}

fn open_palette(cwd: &std::path::Path, query: Option<String>, fullscreen: bool) -> Result<Reply> {
    open_surface(cwd, query, fullscreen, false)
}

fn open_surface(
    cwd: &std::path::Path,
    query: Option<String>,
    fullscreen: bool,
    opening_tree: bool,
) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    let outcome = ui::run_surface(&mut service, query, fullscreen, opening_tree)?;
    match outcome {
        aikit_tui::PaletteOutcome::Run(intent) => {
            let command = service.plan_run_intent(&intent)?;
            if command.mode.needs_mux() {
                spawn_palette_command(&command)?;
                return Ok(Reply::Status(0));
            }
            #[cfg(unix)]
            if command.mode == aikit_core::capsule::ExecMode::Replace {
                return Err(run::exec_replace(&command));
            }
            let report = run::execute(&command)?;
            for line in &report.output {
                println!("{line}");
            }
            Ok(Reply::Status(report.status))
        }
        aikit_tui::PaletteOutcome::Closed
        | aikit_tui::PaletteOutcome::Tree
        | aikit_tui::PaletteOutcome::Applied(_)
        | aikit_tui::PaletteOutcome::Promoted(_) => Ok(Reply::Silent),
    }
}

fn spawn_palette_command(command: &run::ScriptCommand) -> Result<()> {
    use aikit_adapters::mux::SpawnRequest;
    use aikit_core::capsule::ExecMode;
    use aikit_core::session::Placement;

    let placement = match command.mode {
        ExecMode::NewPane => Placement::NewPane,
        ExecMode::NewView => Placement::NewView,
        _ => {
            return Err(AikitError::new(
                "run.invalid_mux_mode",
                format!(
                    "{} is not a multiplexer execution mode",
                    command.mode.as_str()
                ),
            ))
        }
    };
    let mut argv = vec![command.program.clone()];
    argv.extend(command.argv.clone());
    let mut request = SpawnRequest::new(placement, argv).in_dir(command.cwd.clone());
    request.env = command.env.clone();
    let stack = detected_system_stack(None)?;
    request.target = Some(stack.current_location()?.target());
    stack.spawn(request)?;
    Ok(())
}

fn detected_system_stack(
    forced: Option<aikit_core::MuxKind>,
) -> Result<aikit_adapters::mux::stack::MuxStack> {
    use aikit_adapters::mux::{cmux::Cmux, plain::Plain, stack::MuxStack, tmux::Tmux};
    MuxStack::detect(
        vec![
            Box::new(Cmux::system()),
            Box::new(Tmux::system()),
            Box::new(Plain::new()),
        ],
        forced,
    )
}

fn executable_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| {
            std::fs::canonicalize(&candidate)
                .unwrap_or(candidate)
                .display()
                .to_string()
        })
}

fn cmd_knowledge(cwd: &std::path::Path, c: KnowledgeCmd) -> Result<Reply> {
    use aikit_core::resource::ResourceRef;
    use aikit_core::ForgetScope;

    let mut service = Service::discover(cwd)?;
    let mut warnings = diagnostic_warnings(&service);
    let data = match c.command {
        KnowledgeSub::Search(a) => {
            let result = service.knowledge_search(&a.query, a.limit)?;
            warnings.extend(result.absences.clone());
            jval!(result)
        }
        KnowledgeSub::Read(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_read(&address)?)
        }
        KnowledgeSub::Relations(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_relations(&address, a.depth, a.max_nodes, a.max_edges)?)
        }
        KnowledgeSub::Route(a) => {
            let addresses = a
                .addresses
                .iter()
                .map(|raw| parse_knowledge_address(raw))
                .collect::<Result<Vec<_>>>()?;
            jval!(service.knowledge_route(a.query.as_deref(), &addresses)?)
        }
        KnowledgeSub::Frame(a) => {
            let addresses = a
                .addresses
                .iter()
                .map(|raw| parse_knowledge_address(raw))
                .collect::<Result<Vec<_>>>()?;
            let frame = service.knowledge_frame(a.query.as_deref(), &addresses)?;
            warnings.extend(frame.absences.clone());
            jval!(frame)
        }
        KnowledgeSub::Sources(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_sources(&address)?)
        }
        KnowledgeSub::Explain(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_explain(&address)?)
        }
        KnowledgeSub::History(a) => {
            let resource = a.resource.as_deref().map(ResourceRef::parse).transpose()?;
            jval!(service.knowledge_history(resource.as_ref())?)
        }
        KnowledgeSub::Status(_) => {
            let status = service.knowledge_status()?;
            warnings.extend(status.absences.clone());
            jval!(status)
        }
        KnowledgeSub::Forget(a) => {
            let scope = match a.command {
                KnowledgeForgetSub::Destination(a) => {
                    ForgetScope::Destination(ResourceRef::parse(&a.resource)?)
                }
                KnowledgeForgetSub::Route(a) => {
                    ForgetScope::Route(ResourceRef::parse(&a.resource)?)
                }
                KnowledgeForgetSub::Project(a) => {
                    ForgetScope::Project(ResourceRef::parse(&a.resource)?)
                }
                KnowledgeForgetSub::All(_) => ForgetScope::All,
            };
            service.knowledge_forget(scope.clone())?;
            jval!({
                "forgot": scope,
                "preserved": ["canonical-resource-identity", "provider-truth", "knowledge-operation-history"]
            })
        }
    };
    Ok(reply(&service, data, warnings))
}

fn parse_knowledge_address(raw: &str) -> Result<aikit_core::KnowledgeAddress> {
    use aikit_core::resource::{ResourceRef, SourceRef};
    use aikit_core::KnowledgeAddress;

    let raw = raw.trim();
    if raw.starts_with('{') {
        return serde_json::from_str(raw).map_err(|error| {
            AikitError::new(
                "knowledge.invalid_address",
                format!("invalid typed Knowledge address JSON: {error}"),
            )
            .with("address", raw)
        });
    }
    if let Some(value) = raw.strip_prefix("wiki=") {
        return Ok(KnowledgeAddress::Wiki(ResourceRef::parse(value)?));
    }
    if let Some(value) = raw.strip_prefix("source=") {
        return Ok(KnowledgeAddress::Source(SourceRef::parse(value)?));
    }
    if let Some(value) = raw.strip_prefix("project=") {
        return Ok(KnowledgeAddress::ProjectMap(ResourceRef::parse(value)?));
    }
    Err(AikitError::new(
        "knowledge.invalid_address",
        "Knowledge address must be typed JSON from search, or wiki=REF, source=REF, project=REF",
    )
    .with("address", raw))
}

fn cmd_search(cwd: &std::path::Path, a: SearchArgs) -> Result<Reply> {
    let mut service = Service::discover(cwd)?;
    let resolved = {
        let application = ApplicationService::new(&mut service);
        application.resolve_search(&a.query)?
    };
    let rows: Vec<Value> = resolved
        .resources
        .resources
        .iter()
        .take(a.limit)
        .map(|row| {
            let capsule = CapsuleId::parse(row.resource.as_str()).ok();
            let package = capsule
                .as_ref()
                .and_then(|id| service.resolved().catalog_index.get(id));
            jval!({
                "id": row.resource.to_string(),
                "name": row.label,
                "kind": package.map(|entry| entry.kind.as_str()).unwrap_or(row.kind.as_str()),
                "resource_kind": row.kind.as_str(),
                "summary": row.summary,
                "active": capsule.as_ref().is_some_and(|id| service.resolved().is_active(id)),
                "runnable": capsule.as_ref().is_some_and(|id| service.resolved().can_run(id)),
            })
        })
        .collect();
    Ok(reply(
        &service,
        jval!({
            "expression": resolved.expression,
            "path": resolved.path,
            "rows": rows,
        }),
        diagnostic_warnings(&service),
    ))
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
    Ok(reply(&service, data, diagnostic_warnings(&service)))
}

fn cmd_explain(cwd: &std::path::Path, a: ExplainArgs) -> Result<Reply> {
    use aikit_core::resource::ResourceRef;

    let mut service = Service::discover(cwd)?;
    let capsule_candidate = CapsuleId::parse(&a.capability).ok();
    if let Some(id) = capsule_candidate.as_ref() {
        if let Some(explanation) = service.resolved().explain(id) {
            let data = jval!({
                "id": explanation.id.to_string(),
                "revision": explanation.revision.as_ref().map(|revision| revision.as_str()),
                "active": explanation.active,
                "declared_enabled": explanation.declared_enabled,
                "selected_by": explanation.selected_by,
                "required_by": explanation.required_by,
                "dependencies": explanation.dependencies,
                "exports": explanation.exports,
                "skill_usage_overlays": explanation.skill_usage_overlays,
                "unavailable": explanation.unavailable.as_ref().map(|r| r.describe()),
                "render": explanation.render(),
            });
            return Ok(reply(&service, data, vec![]));
        }
    }

    let resource = ResourceRef::parse(&a.capability)?;
    let evidence = {
        let application = ApplicationService::new(&mut service);
        application.explain_evidence(&resource)
    };
    match evidence {
        Ok(evidence) => Ok(reply(&service, jval!(evidence), vec![])),
        Err(error) if capsule_candidate.is_some() => {
            let id = capsule_candidate.expect("checked above");
            Err(AikitError::new(
                "resolution.unknown_capability",
                format!("{id} is not in the catalogue for this context"),
            )
            .with("capability", id.to_string())
            .with("evidence_error", error.code()))
        }
        Err(error) => Err(error),
    }
}

fn cmd_history(cwd: &std::path::Path, a: HistoryArgs) -> Result<Reply> {
    use aikit_core::resource::ResourceRef;

    let mut service = Service::discover(cwd)?;
    let resource = a.resource.as_deref().map(ResourceRef::parse).transpose()?;
    let history = {
        let application = ApplicationService::new(&mut service);
        application.history_evidence(resource.as_ref())?
    };
    Ok(reply(&service, jval!(history), vec![]))
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
            use aikit_core::context::ContextBinding;
            use aikit_store::state::StateStore;

            let descriptor = service.descriptor();
            // Ask the real multiplexer where we are rather than accepting a claim:
            // a binding that says "pane %7" when the pane is gone is worse than none.
            let location = detected_system_stack(None)?.current_location()?;

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
        ContextSub::Env(a) => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let items = aikit_cli::env::project(service.resolved(), &home)?;
            // Raw shell syntax: this is eval'd by the shell integration, so it
            // must not be wrapped in an envelope.
            Ok(Reply::Text(aikit_cli::env::render_shell(&items, &a.shell)?))
        }
        ContextSub::Reset(_) => {
            use aikit_store::state::StateStore;
            // Forgetting a binding is not forgetting the context: the generations,
            // the overlay and the history all survive.
            let forgotten = StateStore::new(service.index())
                .unbind_context(&service.descriptor().context_id)?;
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
            Ok(reply(
                &service,
                jval!({ "tasks": rows, "count": rows.len() }),
                vec![],
            ))
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
            let id =
                service.issue_bypass(&a.scope, a.reason.as_deref(), a.capability.as_deref())?;
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
    Ok(reply(
        &service,
        jval!({ "bypasses": bypass_summaries(&service)? }),
        vec![],
    ))
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
            let instructions = if cap.kind == aikit_core::capsule::Kind::Skill {
                Some(service.effective_skill_markdown(&id)?)
            } else {
                None
            };
            let data = jval!({
                "id": id.to_string(),
                "name": cap.name,
                "description": cap.description,
                "kind": cap.kind.as_str(),
                "tags": cap.tags,
                "instructions": instructions,
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
            let data = jval!({
                "session": result.session,
                "mux": result.mux,
                "created": result.created,
                "actions": result.actions,
                "preserved": result.preserved,
                "summary": result.summary,
            });
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
            Ok(reply(
                &service,
                jval!({ "sessions": rows, "count": rows.len() }),
                vec![],
            ))
        }
        SessionSub::Attach(a) => {
            let (commands, mux) = session_commands(&service, &a.session, "attach")?;
            let data = jval!({
                "session": a.session,
                "mux": mux,
                "command": commands.first(),
                "commands": commands,
            });
            Ok(reply(&service, data, vec![]))
        }
        SessionSub::Diff(a) => {
            let outcome = service.session_diff(a.spec.as_deref())?;
            let data = jval!({
                "session": outcome.session,
                "mux": outcome.mux,
                "matches_spec": outcome.differences.is_empty(),
                "differences": outcome.differences,
            });
            Ok(reply(&service, data, outcome.warnings))
        }
        SessionSub::Reconcile(a) => {
            let outcome = service.session_reconcile(a.spec.as_deref(), a.destructive)?;
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
            let (commands, mux) = session_commands(&service, &a.session, "kill")?;
            let data = jval!({
                "session": a.session,
                "mux": mux,
                "command": commands.first(),
                "commands": commands,
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
fn session_commands(
    service: &Service,
    session: &str,
    verb: &str,
) -> Result<(Vec<Vec<String>>, String)> {
    use aikit_core::MuxKind;
    use aikit_store::state::StateStore;

    let records = StateStore::new(service.index()).sessions()?;
    let matches: Vec<_> = records
        .iter()
        .filter(|record| session_record_is_in_scope(service, record, session))
        .collect();
    if matches.len() > 1 {
        return Err(AikitError::new(
            "session.ambiguous",
            format!("more than one live AIKit session is named `{session}`"),
        )
        .with("session", session.to_string()));
    }

    let Some(record) = matches.first() else {
        let mux = aikit_cli::mux_install::choose_installed(service.descriptor().mux)?;
        if mux == MuxKind::Cmux {
            return Err(AikitError::new(
                "session.cmux_binding_missing",
                format!(
                    "AIKit has no durable cmux binding for session `{session}`; run `aikit \
                     session up` inside cmux first"
                ),
            )
            .with("session", session.to_string()));
        }
        let command = mux_session_command(mux, session.to_string(), verb)?;
        return Ok((vec![command], mux.as_str().to_string()));
    };

    let commands = if record.mux == MuxKind::Cmux {
        let targets =
            aikit_adapters::mux::cmux::Cmux::system().session_targets(&record.session_id)?;
        cmux_session_commands(record, targets, verb)?
    } else {
        let target = record
            .mux_session
            .clone()
            .unwrap_or_else(|| session.to_string());
        vec![mux_session_command(record.mux, target, verb)?]
    };
    Ok((commands, record.mux.as_str().to_string()))
}

fn session_record_is_in_scope(
    service: &Service,
    record: &aikit_store::state::SessionRecord,
    session: &str,
) -> bool {
    let descriptor = service.descriptor();
    session_record_matches(
        record,
        session,
        descriptor.project_id.as_ref(),
        descriptor.project_root.as_deref(),
        descriptor.mux,
    )
}

fn session_record_matches(
    record: &aikit_store::state::SessionRecord,
    session: &str,
    project_id: Option<&aikit_core::ProjectId>,
    project_root: Option<&std::path::Path>,
    declared_mux: Option<aikit_core::MuxKind>,
) -> bool {
    let project_matches = match project_id {
        Some(project_id) => record.project_marker.as_ref() == Some(project_id),
        None => {
            record.project_marker.is_none()
                && match (record.project_root.as_deref(), project_root) {
                    (Some(record_root), Some(active_root)) => {
                        same_filesystem_location(record_root, active_root)
                    }
                    (None, None) => true,
                    _ => false,
                }
        }
    };
    record.name == session
        && record.state.can_go_stale()
        && project_matches
        && declared_mux.is_none_or(|mux| record.mux == mux)
}

fn same_filesystem_location(left: &std::path::Path, right: &std::path::Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn cmux_session_commands(
    record: &aikit_store::state::SessionRecord,
    targets: aikit_adapters::mux::cmux::CmuxSessionTargets,
    verb: &str,
) -> Result<Vec<Vec<String>>> {
    let grouped = record
        .mux_session
        .as_deref()
        .is_some_and(|target| target.starts_with("window:"));
    if grouped {
        if verb != "attach" && targets.exclusive_window.is_none() {
            if targets.workspaces.is_empty() {
                return Err(AikitError::new(
                    "session.cmux_binding_missing",
                    format!("cmux has no owned workspaces for session `{}`", record.name),
                )
                .with("session_id", record.session_id.to_string()));
            }
            return targets
                .workspaces
                .into_iter()
                .map(|workspace| mux_session_command(aikit_core::MuxKind::Cmux, workspace, verb))
                .collect();
        }
        let window = if verb == "attach" {
            targets.common_window
        } else {
            targets.exclusive_window
        }
        .ok_or_else(|| {
            AikitError::new(
                "session.cmux_binding_ambiguous",
                format!(
                    "cmux no longer has one owned window for session `{}`",
                    record.name
                ),
            )
            .with("session_id", record.session_id.to_string())
        })?;
        return Ok(vec![mux_session_command(
            aikit_core::MuxKind::Cmux,
            window,
            verb,
        )?]);
    }

    if targets.workspaces.is_empty() {
        return Err(AikitError::new(
            "session.cmux_binding_missing",
            format!("cmux has no owned workspaces for session `{}`", record.name),
        )
        .with("session_id", record.session_id.to_string()));
    }

    let workspaces: Vec<_> = if verb == "attach" {
        targets.workspaces.into_iter().take(1).collect()
    } else {
        targets.workspaces
    };
    workspaces
        .into_iter()
        .map(|workspace| mux_session_command(aikit_core::MuxKind::Cmux, workspace, verb))
        .collect()
}

fn mux_session_command(
    mux: aikit_core::MuxKind,
    target: String,
    verb: &str,
) -> Result<Vec<String>> {
    let argv = match (mux, verb) {
        (aikit_core::MuxKind::Tmux, "attach") => {
            vec!["tmux".into(), "attach-session".into(), "-t".into(), target]
        }
        (aikit_core::MuxKind::Tmux, _) => {
            vec!["tmux".into(), "kill-session".into(), "-t".into(), target]
        }
        (aikit_core::MuxKind::Cmux, "attach") if target.starts_with("window:") => {
            vec![
                "cmux".into(),
                "focus-window".into(),
                "--window".into(),
                target,
            ]
        }
        (aikit_core::MuxKind::Cmux, _) if target.starts_with("window:") => {
            vec![
                "cmux".into(),
                "close-window".into(),
                "--window".into(),
                target,
            ]
        }
        (aikit_core::MuxKind::Cmux, "attach") => {
            vec![
                "cmux".into(),
                "select-workspace".into(),
                "--workspace".into(),
                target,
            ]
        }
        (aikit_core::MuxKind::Cmux, _) => {
            vec![
                "cmux".into(),
                "close-workspace".into(),
                "--workspace".into(),
                target,
            ]
        }
        (aikit_core::MuxKind::Plain, _) => {
            return Err(AikitError::new(
                "mux.none_detected",
                format!("a plain terminal cannot {verb} a named session"),
            ))
        }
    };
    Ok(argv)
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
    let staged = service.stage(StageRequest {
        scope,
        toggles: vec![],
    })?;

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
    Ok(reply(
        &service,
        jval!({ "events": rows, "count": rows.len() }),
        vec![],
    ))
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
    Ok(reply(
        &service,
        jval!({ "failures": rows, "count": rows.len() }),
        vec![],
    ))
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
    Ok(reply(
        &service,
        jval!({ "unused": rows, "count": rows.len() }),
        vec![],
    ))
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
                    "binary": match presence.kind {
                        aikit_core::MuxKind::Tmux => executable_path("tmux"),
                        aikit_core::MuxKind::Cmux => executable_path("cmux"),
                        aikit_core::MuxKind::Plain => None,
                    },
                    "installed": presence.installed,
                    "version": presence.version,
                    "server_running": presence.server_running,
                    "inside": presence.inside,
                    "detail": presence.detail,
                }));
            }
            let stack = detected_system_stack(None)?;
            let active_stack = stack
                .kinds()
                .into_iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>();
            let data = jval!({
                "detected": rows,
                "active": stack.topology_kind().as_str(),
                "active_stack": active_stack,
                "declared": service.descriptor().mux.map(|m| m.as_str()),
            });
            Ok(reply(&service, data, vec![]))
        }
        MuxSub::Install(a) => {
            let planned =
                aikit_cli::mux_install::plan(&service, a.mux.as_deref(), &a.key, a.replace_key)?;
            let runner = aikit_store::procedure::ProcedureRunner::new(service.home());
            let diff = runner.diff(&planned.procedure)?;
            let outcome = runner.run(&planned.procedure)?;
            let verification = aikit_cli::mux_install::activate(&planned)?;
            let data = jval!({
                "procedure": planned.procedure.id.to_string(),
                "edits": outcome.applied,
                "diff": diff.render(),
                "undo": format!("aikit procedure undo {}", planned.procedure.id),
                "mux": planned.mux.as_str(),
                "key": planned.key,
                "path": planned.path.display().to_string(),
                "live": verification.live,
                "verified": verification.verified,
                "binding": verification.binding,
            });
            Ok(reply(&service, data, verification.warnings))
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
        .find(|scope| scope.as_str() == raw || (*scope == ScopeKind::Global && raw == "user"))
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

#[cfg(test)]
mod session_command_tests {
    use super::{cmux_session_commands, mux_session_command, session_record_matches};
    use aikit_adapters::mux::cmux::CmuxSessionTargets;
    use aikit_core::{MuxKind, ProjectId, SessionId};
    use aikit_store::events::Timestamp;
    use aikit_store::state::{SessionRecord, SessionState};

    fn record(
        root: &str,
        project: Option<ProjectId>,
        mux: MuxKind,
        binding: &str,
    ) -> SessionRecord {
        SessionRecord {
            session_id: SessionId::generate(),
            name: "dev".into(),
            project_root: Some(root.into()),
            project_marker: project,
            mux,
            mux_session: Some(binding.into()),
            state: SessionState::Live,
            created_at: Timestamp::now(),
            last_seen: Timestamp::now(),
        }
    }

    #[test]
    fn grouped_cmux_sessions_use_window_lifecycle_commands() {
        assert_eq!(
            mux_session_command(MuxKind::Cmux, "window:7".into(), "attach").unwrap(),
            ["cmux", "focus-window", "--window", "window:7"]
        );
        assert_eq!(
            mux_session_command(MuxKind::Cmux, "window:7".into(), "kill").unwrap(),
            ["cmux", "close-window", "--window", "window:7"]
        );
    }

    #[test]
    fn ungrouped_cmux_sessions_keep_workspace_lifecycle_commands() {
        assert_eq!(
            mux_session_command(MuxKind::Cmux, "workspace:9".into(), "attach").unwrap(),
            ["cmux", "select-workspace", "--workspace", "workspace:9"]
        );
    }

    #[test]
    fn session_lookup_is_scoped_by_project_and_declared_mux() {
        let project_a = ProjectId::generate();
        let project_b = ProjectId::generate();
        let in_scope = record(
            "/work/a",
            Some(project_a.clone()),
            MuxKind::Cmux,
            "workspace:old",
        );
        let wrong_project = record("/work/b", Some(project_b), MuxKind::Cmux, "workspace:other");

        assert!(session_record_matches(
            &in_scope,
            "dev",
            Some(&project_a),
            Some(std::path::Path::new("/work/a")),
            Some(MuxKind::Cmux),
        ));
        assert!(!session_record_matches(
            &wrong_project,
            "dev",
            Some(&project_a),
            Some(std::path::Path::new("/work/a")),
            Some(MuxKind::Cmux),
        ));
        assert!(!session_record_matches(
            &in_scope,
            "dev",
            Some(&project_a),
            Some(std::path::Path::new("/work/a")),
            Some(MuxKind::Tmux),
        ));
    }

    #[test]
    fn a_project_without_a_marker_falls_back_to_its_exact_root() {
        let in_scope = record("/work/a", None, MuxKind::Tmux, "dev");
        let other_root = record("/work/b", None, MuxKind::Tmux, "dev");
        assert!(session_record_matches(
            &in_scope,
            "dev",
            None,
            Some(std::path::Path::new("/work/a")),
            None,
        ));
        assert!(!session_record_matches(
            &other_root,
            "dev",
            None,
            Some(std::path::Path::new("/work/a")),
            None,
        ));
    }

    #[test]
    fn cmux_commands_rebind_to_live_handles_and_close_every_owned_workspace() {
        let grouped = record("/work/a", None, MuxKind::Cmux, "window:stale");
        assert_eq!(
            cmux_session_commands(
                &grouped,
                CmuxSessionTargets {
                    workspaces: vec!["workspace:20".into(), "workspace:21".into()],
                    common_window: Some("window:live".into()),
                    exclusive_window: Some("window:live".into()),
                },
                "attach",
            )
            .unwrap(),
            [vec!["cmux", "focus-window", "--window", "window:live"]]
        );

        let ungrouped = record("/work/a", None, MuxKind::Cmux, "workspace:stale");
        assert_eq!(
            cmux_session_commands(
                &ungrouped,
                CmuxSessionTargets {
                    workspaces: vec!["workspace:20".into(), "workspace:21".into()],
                    common_window: None,
                    exclusive_window: None,
                },
                "kill",
            )
            .unwrap(),
            [
                vec!["cmux", "close-workspace", "--workspace", "workspace:20"],
                vec!["cmux", "close-workspace", "--workspace", "workspace:21"],
            ]
        );

        assert_eq!(
            cmux_session_commands(
                &grouped,
                CmuxSessionTargets {
                    workspaces: vec!["workspace:20".into(), "workspace:21".into()],
                    common_window: Some("window:live".into()),
                    exclusive_window: None,
                },
                "kill",
            )
            .unwrap(),
            [
                vec!["cmux", "close-workspace", "--workspace", "workspace:20"],
                vec!["cmux", "close-workspace", "--workspace", "workspace:21"],
            ],
            "a foreign workspace in the grouped window forces ownership-bounded teardown"
        );
    }
}
