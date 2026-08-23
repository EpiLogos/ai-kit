//! Headless Living Knowledge diagnostic surface.
//!
//! This binary is intentionally a thin structured adapter over `aikit-core`. It owns no knowledge
//! semantics and cannot execute a Contemplate Agent/model invocation: actual execution remains the
//! host-supplied `ContemplateExecutor` seam. The commands here are deterministic query/preflight
//! operations suitable for Agent, CI and cross-product conformance callers.

use std::fs;
use std::path::PathBuf;

use aikit_core::{
    portable_contemplate_preflight, KnowledgeImpactRequest, PortableContemplatePreflightRequest,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Parser)]
#[command(name = "aikit-knowledge-living")]
#[command(about = "Headless deterministic Living Knowledge queries and Contemplate preflight")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Resolve bounded direct/transitive impact and dependency paths from a portable request.
    Impact(InputArgs),
    /// Return the exact resources currently pending integration from the same impact request.
    Pending(InputArgs),
    /// Resolve deterministic Contemplate preflight without invoking an Agent/model.
    ContemplatePreflight(InputArgs),
}

#[derive(Debug, clap::Args)]
struct InputArgs {
    /// UTF-8 JSON request file. Use `-` to read JSON from stdin.
    #[arg(long, value_name = "PATH")]
    input: PathBuf,
}

fn read_json<T: DeserializeOwned>(path: &PathBuf) -> Result<T> {
    let bytes = if path.as_os_str() == "-" {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)
            .context("read Living Knowledge request from stdin")?;
        bytes
    } else {
        fs::read(path).with_context(|| format!("read {}", path.display()))?
    };
    serde_json::from_slice(&bytes).with_context(|| format!("parse {} as JSON", path.display()))
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Impact(args) => {
            let request: KnowledgeImpactRequest = read_json(&args.input)?;
            print_json(&request.evaluate()?)?;
        }
        Command::Pending(args) => {
            let request: KnowledgeImpactRequest = read_json(&args.input)?;
            let impact = request.evaluate()?;
            print_json(&impact.pending_integration)?;
        }
        Command::ContemplatePreflight(args) => {
            let request: PortableContemplatePreflightRequest = read_json(&args.input)?;
            print_json(&portable_contemplate_preflight(&request)?)?;
        }
    }
    Ok(())
}
