use std::path::PathBuf;

use aikit_cli::app::Service;
use aikit_cli::SessionSpaceServiceOps;
use aikit_core::project::ProjectRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceMutation, SessionSpacePreview,
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
    /// List persisted SessionSpaces.
    List,
    /// Show one canonical SessionSpace semantic state.
    Show { space: String },
    /// Open persisted semantic state without claiming provider-native recovery.
    Open { space: String },
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
    Stage {
        #[arg(long)]
        space: Option<String>,
        #[arg(long = "intent-json", value_name = "JSON|@FILE")]
        intent_json: String,
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

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {}", error.code(), error.message());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = match cli.cwd {
        Some(cwd) => cwd,
        None => std::env::current_dir().map_err(|error| {
            AikitError::new("cli.cwd_unavailable", format!("could not read cwd: {error}"))
        })?,
    };
    let service = Service::discover(&cwd)?;

    match cli.command {
        Command::List => emit(&service.session_space_list()?),
        Command::Show { space } => emit(&service.session_space_show(&space_ref(&space)?)?),
        Command::Open { space } => emit(&service.session_space_open(&space_ref(&space)?)?),
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
        Command::Stage { space, intent_json } => {
            let intent: SessionSpaceMutation = parse_json_arg(&intent_json)?;
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
        Command::Reconstruct { space } => emit(&service.session_space_reconstruct(
            &space_ref(&space)?,
            None,
            &[],
            &[],
        )?),
        Command::Reconcile { space } => emit(&service.session_space_reconcile(
            &space_ref(&space)?,
            None,
            &[],
            &[],
        )?),
        Command::Explain { space } => {
            emit(&service.session_space_explain(&space_ref(&space)?, None)?)
        }
    }
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
