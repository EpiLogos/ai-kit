//! Public composition of native placement and material preparation, not a worker.
use aikit_adapters::{central_work::{prepare_boundary, CentralPlacement, CentralTask}, runner::SystemRunner};
use aikit_core::{AikitError, Result};
use clap::Parser;
use serde_json::json;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    request_json: String,
    #[arg(long)]
    workcell_boundary: PathBuf,
}
fn main() {
    match run() {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{}", json!({"ok":false,"code":error.code(),"message":error.message()}));
            std::process::exit(1);
        }
    }
}
fn run() -> Result<serde_json::Value> {
    let args = Args::parse();
    let input = match args.request_json.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path).map_err(|e| AikitError::new("placement.input", e.to_string()))?,
        None => args.request_json,
    };
    if input.len() > 1024 * 1024 {
        return Err(AikitError::new("placement.input", "Task request exceeds 1 MiB"));
    }
    let task: CentralTask = serde_json::from_str(&input).map_err(|e| AikitError::new("placement.input", e.to_string()))?;
    let runner = SystemRunner::new();
    let owner = CentralPlacement::new(&runner, task)?;
    let allocation = owner.allocate()?;
    // Central protects replacement/removal of the T directory itself. Ask
    // about an output inside its aperture without creating that output.
    let destination = allocation.basis.now.join(".aikit-admission-output");
    let validation = owner.validate(&allocation, &owner.task.root, &destination)?;
    if validation["allowed"] != true {
        return Err(AikitError::new("placement.denied", "Central no longer permits this task's NOW destination"));
    }
    let boundary = prepare_boundary(&runner, &args.workcell_boundary, &allocation)?;
    Ok(json!({"schema":"aikit.native-task-preparation/v1","allocation":allocation,"validation":validation,"boundary":boundary,"executed":false,"confinement_active":false}))
}
