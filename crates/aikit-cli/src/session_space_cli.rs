//! The folded `session-space` command surface.
//!
//! This is the standalone `aikit-session-space` binary's whole clap tree and
//! dispatch, moved verbatim into the library so both that binary and the main
//! `aikit` binary's `session-space` subcommand resolve to exactly one
//! implementation (O-I #376: `oi aikit-session-space` execs
//! `aikit session-space`, and the main binary previously had no such
//! subcommand). Output on stdout stays byte-identical to the standalone binary;
//! [`run_from_args`] returns an exit code instead of exiting the process, so the
//! main binary can forward to it. The `encounter-*-exec` variants still replace
//! the process via `exec()` and never return.

use std::path::PathBuf;

use crate::app::Service;
use crate::SessionSpaceServiceOps;
use aikit_core::project::ProjectRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    ContextResolutionEvidence, SessionSpaceMutation, SessionSpacePreview,
    SessionSpaceProjectContextBinding,
};
use aikit_core::{AikitError, Result};
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "aikit-session-space",
    about = "Operate durable SessionSpace semantics through AIKit's canonical application authority"
)]
struct Cli {
    /// Resolve the canonical AIKit Service as if invoked from this directory.
    #[arg(long = "cwd", short = 'C', global = true, value_name = "DIR")]
    cwd: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read the native ProjectBinding; no session or provider is created.
    AgentSessionScope,
    /// Read effective parent-Skill choices without activating or projecting.
    AgentSessionSkills,
    /// Prepare one accepted Central Agent and canonical Direct session.
    AgentSessionPrepare {
        #[arg(long)]
        request_json: String,
    },
    /// Read a preparation without opening a provider or replaying work.
    AgentSessionRead {
        #[arg(long)]
        agent_session: String,
    },
    /// Recover interrupted preparation by its original correlation.
    AgentSessionFind {
        #[arg(long)]
        request_id: String,
    },
    /// Internal scoped Model launch; raw credential material never enters JSON.
    EncounterModelExec {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        expected_model_basis: String,
    },
    /// Prepare a Central task and an exact Workcell boundary for this session.
    EncounterTaskConfigure {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        request_json: String,
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Read task preparation, including pending effects and native source bases.
    EncounterTaskRead {
        #[arg(long)]
        agent_session: String,
    },
    /// Internal native protocol launch; emits no wrapper bytes to stdout.
    EncounterTaskExec {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        expected_revision: String,
    },
    /// Start the native resident owner once, independently of this CLI client.
    #[cfg(unix)]
    EncounterStart,
    /// Run the resident generic ACP owner. Client exit never stops providers.
    #[cfg(unix)]
    EncounterServe {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Configure a native ACP provider. This operation is not exposed over IPC.
    EncounterConfigure {
        #[arg(long)]
        provider_json: String,
    },
    /// Configure the installable Epi-Logos Prime-QL body over Prime RPC.
    /// Paths are resolved now; mode selection later starts nothing until the
    /// ordinary Encounter open/first-Send boundary.
    EncounterEpiPrimeConfigure {
        #[arg(long, default_value = "epi-prime-ql")]
        provider_id: String,
        #[arg(long)]
        launcher: PathBuf,
        #[arg(long)]
        prime_bin: PathBuf,
        #[arg(long)]
        ql_bin: PathBuf,
        #[arg(long)]
        ql_revision: String,
        #[arg(long)]
        body_revision: String,
        #[arg(long)]
        skill_path: PathBuf,
        #[arg(long)]
        research_bin: PathBuf,
        #[arg(long)]
        faculty_config: PathBuf,
        #[arg(long)]
        ql_root: Option<PathBuf>,
    },
    /// Provision or withdraw a native Agency binding under an exact revision.
    /// This is an owner-only operation, not gateway/IPC input.
    EncounterAgencyConfigure {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        binding_json: String,
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Mint a fresh per-project native Agency chain from the owner's standing
    /// template, actualise it through the real native owner, and provision this
    /// session's binding. Owner-only, like encounter-agency-configure.
    EncounterAgencyMint {
        #[arg(long)]
        agent_session: String,
        /// The project directory whose canonical identity this agency is bound
        /// to; resolved through the same resolution `project-context` applies.
        #[arg(long = "project-cwd", value_name = "DIR")]
        project_cwd: PathBuf,
        /// Explicit agent identity; otherwise the Central AgentProfile for the
        /// project scope when exactly one exists, else a derived project agent.
        #[arg(long)]
        agent_ref: Option<String>,
    },
    /// Correlate operator-reviewed native evidence for a stuck delivery; never replay it.
    EncounterDeliveryReconcile {
        #[arg(long)]
        agent_session: String,
        #[arg(long)]
        delivery_ref: String,
        #[arg(long)]
        evidence_ref: String,
        #[arg(long)]
        expected_phase: String,
    },
    /// Apply a canonical encounter action to the resident owner.
    #[cfg(unix)]
    Encounter {
        #[arg(long)]
        request_json: String,
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Read the current canonical Project + ContextResolution binding for typed stage intent.
    ProjectContext,
    /// List persisted SessionSpaces.
    List,
    /// Show one canonical SessionSpace semantic state.
    Show { space: String },
    /// Open persisted semantic state without claiming provider-native recovery.
    Open { space: String },
    /// Read, open, or focus one exact persisted provider working Surface.
    WorkingSurface {
        #[command(subcommand)]
        command: WorkingSurfaceCommand,
    },
    /// Discover SessionSpaces, optionally by exact ProjectRef.
    Discover {
        #[arg(long)]
        project: Option<String>,
    },
    /// Stage a new SessionSpace. This is write-free and returns a preview.
    Create {
        id: String,
        #[arg(long)]
        label: Option<String>,
    },
    /// Stage any typed SessionSpace mutation from JSON. Prefix with @ to read a file.
    ///
    /// `--print-schema` prints documented JSON templates instead of staging:
    /// every mutation operation, each with the `intent` value to pass here and
    /// field notes beside it — including the complete SessionPlan template a
    /// bind-working-surface binding requires. Pass `--operation` for one.
    Stage {
        #[arg(long)]
        space: Option<String>,
        /// Print documented intent templates instead of staging.
        #[arg(long = "print-schema", default_value_t = false)]
        print_schema: bool,
        /// With --print-schema: restrict the output to one operation
        /// (kebab-case, e.g. bind-working-surface).
        #[arg(long = "operation", value_name = "OPERATION")]
        operation: Option<String>,
        #[arg(long = "intent-json", value_name = "JSON|@FILE")]
        intent_json: Option<String>,
    },
    /// Apply exactly a previously reviewed preview. Prefix with @ to read a file.
    Apply {
        #[arg(long = "preview-json", value_name = "JSON|@FILE")]
        preview_json: String,
    },
    /// Show immutable SessionSpace application receipts.
    History { space: String },
    /// Compare two receipt-backed semantic states.
    Compare {
        space: String,
        from_sequence: u64,
        to_sequence: u64,
    },
    /// Stage restoration from a prior receipt through current authority.
    RestorePreview { space: String, sequence: u64 },
    /// Reconstruct using persisted semantic state only; absent live evidence stays unavailable.
    Reconstruct { space: String },
    /// Reconcile as a read of canonical-vs-observed state; with no supplied observations this is non-mutating.
    Reconcile { space: String },
    /// Explain persisted SessionSpace state and the receipt that last changed it.
    Explain { space: String },
}

#[derive(Debug, Subcommand)]
enum WorkingSurfaceCommand {
    /// Read the persisted binding and its current provider observation.
    Observe { space: String, binding: String },
    /// Explicitly create-or-attach the persisted provider plan for this Surface.
    Open { space: String, binding: String },
    /// Focus only the currently live persisted Surface; this never recreates it.
    Focus { space: String, binding: String },
    /// Replace this terminal client with attachment to the exact live provider Surface.
    Attach { space: String, binding: String },
}

/// Parse `args` (argv, program name first) and run the folded SessionSpace
/// surface, returning a process exit code. Both the standalone
/// `aikit-session-space` binary and the main `aikit session-space` subcommand
/// enter here, so the surface has exactly one clap tree, dispatch and output
/// contract. Clap still exits the process itself on a parse error or `--help`,
/// exactly as the standalone binary always has.
pub fn run_from_args<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    match run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{}: {}", error.code(), error.message());
            1
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let cwd = match cli.cwd {
        Some(cwd) => cwd,
        None => std::env::current_dir().map_err(|error| {
            AikitError::new(
                "cli.cwd_unavailable",
                format!("could not read cwd: {error}"),
            )
        })?,
    };
    let service = Service::discover(&cwd)?;

    match cli.command {
        Command::AgentSessionSkills => emit(&crate::direct_agent_session::skills(&service)?),
        Command::AgentSessionScope => emit(&crate::direct_agent_session::scope(&service)?),
        Command::AgentSessionPrepare { request_json } => emit(
            &crate::direct_agent_session::prepare(&service, parse_json_arg(&request_json)?)?,
        ),
        Command::AgentSessionRead { agent_session } => emit(&crate::direct_agent_session::reading(
            service.home(),
            &aikit_core::ResourceRef::parse(agent_session)?,
        )?),
        Command::AgentSessionFind { request_id } => emit(&crate::direct_agent_session::find(
            service.home(),
            &request_id,
        )?),
        Command::EncounterModelExec {
            agent_session,
            provider,
            expected_model_basis,
        } => crate::encounter_service::EncounterService::exec_model(
            service.home(),
            &aikit_core::ResourceRef::parse(agent_session)?,
            &provider,
            &expected_model_basis,
        ),
        Command::EncounterTaskConfigure {
            agent_session,
            request_json,
            expected_revision,
        } => {
            let expected = expected_revision
                .as_deref()
                .map(aikit_core::SourceRevision::parse)
                .transpose()?;
            emit(&crate::encounter_service::EncounterService::configure_task(
                service.home(),
                &aikit_core::ResourceRef::parse(agent_session)?,
                parse_json_arg(&request_json)?,
                expected.as_ref(),
            )?)
        }
        Command::EncounterTaskRead { agent_session } => {
            emit(&crate::encounter_service::EncounterService::read_task(
                service.home(),
                &aikit_core::ResourceRef::parse(agent_session)?,
            )?)
        }
        Command::EncounterTaskExec {
            agent_session,
            expected_revision,
        } => crate::encounter_service::EncounterService::exec_task(
            service.home(),
            &aikit_core::ResourceRef::parse(agent_session)?,
            &aikit_core::SourceRevision::parse(expected_revision)?,
        ),
        #[cfg(unix)]
        Command::EncounterStart => emit(&crate::encounter_service::start(service.home(), &cwd)?),
        #[cfg(unix)]
        Command::EncounterServe { socket } => crate::encounter_service::serve(
            service.home().clone(),
            &socket.unwrap_or_else(|| crate::encounter_service::socket_path(service.home())),
        ),
        Command::EncounterConfigure { provider_json } => {
            crate::encounter_service::EncounterService::configure(
                service.home(),
                parse_json_arg(&provider_json)?,
            )?;
            emit(&serde_json::json!({"configured":true}))
        }
        Command::EncounterEpiPrimeConfigure {
            provider_id,
            launcher,
            prime_bin,
            ql_bin,
            ql_revision,
            body_revision,
            skill_path,
            research_bin,
            faculty_config,
            ql_root,
        } => {
            fn exact_revision(value: &str, label: &str) -> Result<()> {
                if value.len() == 40
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    Ok(())
                } else {
                    Err(AikitError::new(
                        "encounter.prime_configuration",
                        format!("{label} must be a lowercase 40-hex revision"),
                    ))
                }
            }
            fn file(path: PathBuf, label: &str) -> Result<PathBuf> {
                let path = path.canonicalize().map_err(|error| {
                    AikitError::new(
                        "encounter.prime_configuration",
                        format!("{label} is unavailable: {error}"),
                    )
                })?;
                if !path.is_file() {
                    return Err(AikitError::new(
                        "encounter.prime_configuration",
                        format!("{label} must be a file"),
                    ));
                }
                Ok(path)
            }
            fn directory(path: PathBuf, label: &str) -> Result<PathBuf> {
                let path = path.canonicalize().map_err(|error| {
                    AikitError::new(
                        "encounter.prime_configuration",
                        format!("{label} is unavailable: {error}"),
                    )
                })?;
                if !path.is_dir() {
                    return Err(AikitError::new(
                        "encounter.prime_configuration",
                        format!("{label} must be a directory"),
                    ));
                }
                Ok(path)
            }
            exact_revision(&ql_revision, "QL revision")?;
            exact_revision(&body_revision, "Actuation body revision")?;
            let launcher = file(launcher, "Prime-QL launcher")?;
            let prime_bin = file(prime_bin, "Prime Agent binary")?;
            let ql_bin = file(ql_bin, "QL binary")?;
            let skill_path = directory(skill_path, "Prime QL relational skill")?;
            let research_bin = file(research_bin, "Actuation research binary")?;
            let faculty_config = file(faculty_config, "Actuation faculty configuration")?;
            let ql_root = ql_root
                .map(|path| directory(path, "QL source root"))
                .transpose()?;
            let aikit_bin = std::env::current_exe().map_err(|error| {
                AikitError::new(
                    "encounter.prime_configuration",
                    format!("could not resolve the installed AIKit executable: {error}"),
                )
            })?;
            let mut argv = vec![
                launcher.display().to_string(),
                "--prime-bin".into(),
                prime_bin.display().to_string(),
                "--ql-bin".into(),
                ql_bin.display().to_string(),
                "--ql-revision".into(),
                ql_revision.clone(),
                "--skill-path".into(),
                skill_path.display().to_string(),
                "--research-bin".into(),
                research_bin.display().to_string(),
                "--faculty-config".into(),
                faculty_config.display().to_string(),
                "--aikit-bin".into(),
                aikit_bin.display().to_string(),
            ];
            if let Some(root) = ql_root {
                argv.extend(["--ql-root".into(), root.display().to_string()]);
            }
            crate::encounter_service::EncounterService::configure(
                service.home(),
                crate::encounter_service::EncounterProvider {
                    protocol: crate::encounter_service::EncounterProtocol::PrimeRpc,
                    id: provider_id.clone(),
                    label: "Epi-Logos Prime-QL".into(),
                    argv,
                    body_ref: Some("agent-body/epi-prime-ql".into()),
                    body_revision: Some(body_revision.clone()),
                    required_context: None,
                    model_policy: None,
                },
            )?;
            emit(&serde_json::json!({
                "configured":true,
                "provider":provider_id,
                "body_ref":"agent-body/epi-prime-ql",
                "body_revision":body_revision,
                "ql_revision":ql_revision,
                "model_selection":"Prime native configured model unless an explicit AIKit model policy overrides it",
                "standing":"configured-not-started"
            }))
        }
        Command::EncounterAgencyConfigure {
            agent_session,
            binding_json,
            expected_revision,
        } => {
            let expected = expected_revision
                .as_deref()
                .map(aikit_core::SourceRevision::parse)
                .transpose()?;
            crate::encounter_service::EncounterService::configure_agency(
                service.home(),
                &aikit_core::ResourceRef::parse(agent_session)?,
                &parse_json_arg(&binding_json)?,
                expected.as_ref(),
            )?;
            emit(
                &serde_json::json!({"configured":true,"standing":"native-owner-provisioning-not-default-selection"}),
            )
        }
        Command::EncounterAgencyMint {
            agent_session,
            project_cwd,
            agent_ref,
        } => {
            let agent_ref = agent_ref
                .as_deref()
                .map(aikit_core::ResourceRef::parse)
                .transpose()?;
            emit(&crate::encounter_service::mint_from_cli(
                service.home(),
                &project_cwd,
                &aikit_core::ResourceRef::parse(agent_session)?,
                agent_ref,
            )?)
        }
        Command::EncounterDeliveryReconcile {
            agent_session,
            delivery_ref,
            evidence_ref,
            expected_phase,
        } => emit(
            &aikit_store::encounter::EncounterStore::open(service.home())?.reconcile_delivery(
                &aikit_core::ResourceRef::parse(agent_session)?,
                &aikit_core::ResourceRef::parse(delivery_ref)?,
                &aikit_core::ResourceRef::parse(evidence_ref)?,
                &expected_phase,
            )?,
        ),
        #[cfg(unix)]
        Command::Encounter {
            request_json,
            socket,
        } => emit(&crate::encounter_service::request(
            &socket.unwrap_or_else(|| crate::encounter_service::socket_path(service.home())),
            &parse_json_arg(&request_json)?,
        )?),
        Command::ProjectContext => {
            let resolution = aikit_tui::project_world_service::context_resolution(&service)?;
            let context = ContextResolutionEvidence::from_resolution(&resolution)?;
            let binding =
                SessionSpaceProjectContextBinding::new(context.project().clone(), context)?;
            emit(&binding)
        }
        Command::List => emit(&service.session_space_list()?),
        Command::Show { space } => emit(&service.session_space_show(&space_ref(&space)?)?),
        Command::Open { space } => emit(&service.session_space_open(&space_ref(&space)?)?),
        Command::WorkingSurface { command } => match command {
            WorkingSurfaceCommand::Observe { space, binding } => {
                let state = service.session_space_show(&space_ref(&space)?)?;
                emit(&crate::session_space_working_surface::observe(
                    &state,
                    &aikit_core::ResourceRef::parse(binding)?,
                )?)
            }
            WorkingSurfaceCommand::Open { space, binding } => {
                let state = service.session_space_show(&space_ref(&space)?)?;
                let result = crate::session_space_working_surface::open(
                    &state,
                    &aikit_core::ResourceRef::parse(binding)?,
                )?;
                // An open that created provider-native material (a Herdr
                // workspace and its root pane) returns the binding carrying
                // it; persisting that evidence is this operation's separate
                // write, through the same staged mutation path as every
                // other SessionSpace change.
                if let Some(updated) = result.refreshed_binding.clone() {
                    let preview = service.session_space_stage(
                        Some(&space_ref(&space)?),
                        SessionSpaceMutation::BindWorkingSurface {
                            binding: Box::new(updated),
                        },
                    )?;
                    service.session_space_apply(&preview)?;
                }
                emit(&result)
            }
            WorkingSurfaceCommand::Focus { space, binding } => {
                let state = service.session_space_show(&space_ref(&space)?)?;
                emit(&crate::session_space_working_surface::focus(
                    &state,
                    &aikit_core::ResourceRef::parse(binding)?,
                )?)
            }
            WorkingSurfaceCommand::Attach { space, binding } => {
                let state = service.session_space_show(&space_ref(&space)?)?;
                match crate::session_space_working_surface::terminal_attachment(
                    &state,
                    &aikit_core::ResourceRef::parse(binding)?,
                )? {
                    crate::working_environment_field::WorkingEnvironmentTerminalAttachment::Attach {
                        argv,
                        ..
                    } => attach_terminal_client(argv),
                    crate::working_environment_field::WorkingEnvironmentTerminalAttachment::NotExposed {
                        reason,
                        ..
                    } => Err(AikitError::new(
                        "session_space.working_surface_attach_unavailable",
                        reason,
                    )),
                }
            }
        },
        Command::Discover { project } => {
            let project = project.as_deref().map(ProjectRef::parse).transpose()?;
            emit(&service.session_space_discover(project.as_ref())?)
        }
        Command::Create { id, label } => {
            let preview = service.session_space_stage(
                None,
                SessionSpaceMutation::Create {
                    id: space_ref(&id)?,
                    label,
                },
            )?;
            emit(&preview)
        }
        Command::Stage {
            space,
            print_schema,
            operation,
            intent_json,
        } => {
            if print_schema {
                return match operation.as_deref() {
                    Some(operation) => emit(
                        &crate::session_space_schema::operation_schema(operation).ok_or_else(
                            || {
                                AikitError::new(
                                    "cli.session_space_operation_unknown",
                                    format!(
                                        "`{operation}` is not a SessionSpace mutation operation; \
                                         run stage --print-schema with no --operation to list them"
                                    ),
                                )
                            },
                        )?,
                    ),
                    None => emit(&crate::session_space_schema::schema()),
                };
            }
            let Some(intent_json) = intent_json.as_deref() else {
                return Err(AikitError::new(
                    "cli.session_space_intent_missing",
                    "stage needs --intent-json, or --print-schema to print the templates",
                ));
            };
            let intent: SessionSpaceMutation = parse_json_arg(intent_json)?;
            let space = space.as_deref().map(space_ref).transpose()?;
            emit(&service.session_space_stage(space.as_ref(), intent)?)
        }
        Command::Apply { preview_json } => {
            let preview: SessionSpacePreview = parse_json_arg(&preview_json)?;
            emit(&service.session_space_apply(&preview)?)
        }
        Command::History { space } => emit(&service.session_space_history(&space_ref(&space)?)?),
        Command::Compare {
            space,
            from_sequence,
            to_sequence,
        } => emit(&service.session_space_compare_history(
            &space_ref(&space)?,
            from_sequence,
            to_sequence,
        )?),
        Command::RestorePreview { space, sequence } => {
            emit(&service.session_space_stage_restore(&space_ref(&space)?, sequence)?)
        }
        Command::Reconstruct { space } => {
            emit(&service.session_space_reconstruct(&space_ref(&space)?, None, &[], &[])?)
        }
        Command::Reconcile { space } => {
            emit(&service.session_space_reconcile(&space_ref(&space)?, None, &[], &[])?)
        }
        Command::Explain { space } => {
            emit(&service.session_space_explain(&space_ref(&space)?, None)?)
        }
    }
}

#[cfg(unix)]
fn attach_terminal_client(argv: Vec<String>) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let Some((program, args)) = argv.split_first() else {
        return Err(AikitError::new(
            "session_space.working_surface_attach_invalid",
            "working-environment provider returned an empty terminal attachment command",
        ));
    };
    let error = std::process::Command::new(program).args(args).exec();
    Err(AikitError::new(
        "session_space.working_surface_attach_failed",
        format!("could not attach terminal client through persisted working Surface: {error}"),
    ))
}

#[cfg(not(unix))]
fn attach_terminal_client(_argv: Vec<String>) -> Result<()> {
    Err(AikitError::new(
        "session_space.working_surface_attach_unsupported",
        "terminal attachment through a persisted working Surface is unsupported on this platform",
    ))
}

fn space_ref(raw: &str) -> Result<SessionSpaceRef> {
    SessionSpaceRef::parse(raw)
}

fn parse_json_arg<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let text = if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|error| {
            AikitError::new(
                "cli.session_space_json_unreadable",
                format!("could not read {path}: {error}"),
            )
        })?
    } else {
        raw.to_string()
    };
    serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "cli.session_space_json_invalid",
            format!("invalid SessionSpace JSON: {error}"),
        )
    })
}

fn emit<T: Serialize>(value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(|error| {
        AikitError::new(
            "cli.session_space_json_failed",
            format!("could not encode SessionSpace result: {error}"),
        )
    })?;
    println!("{text}");
    Ok(())
}
