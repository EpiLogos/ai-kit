use std::path::PathBuf;

use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_tui::{ApplicationService, ExplainHistoryApplicationService};
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "aikit-explain-history",
    about = "Inspect evidence-classified Explain and History reads without creating a second history authority"
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
    /// Explain one canonical Resource with authored/observed/derived/learned/generated evidence kept distinct.
    Explain { resource: String },
    /// Read cross-domain History; optionally restrict it to one canonical Resource.
    History {
        #[arg(long)]
        resource: Option<String>,
    },
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
    let mut service = Service::discover(&cwd)?;
    let application = ApplicationService::new(&mut service);

    match cli.command {
        Command::Explain { resource } => {
            emit(&application.explain_evidence(&ResourceRef::parse(&resource)?)?)
        }
        Command::History { resource } => {
            let resource = resource.as_deref().map(ResourceRef::parse).transpose()?;
            emit(&application.history_evidence(resource.as_ref())?)
        }
    }
}

fn emit<T: Serialize>(value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(|error| {
        AikitError::new(
            "cli.explain_history_json_failed",
            format!("could not encode Explain/History result: {error}"),
        )
    })?;
    println!("{text}");
    Ok(())
}
