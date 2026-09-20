//! Native constructive actions. Preparation is not persistence. The file
//! path writes through this Wiki owner, never Central's ordinary-file writer.
use aikit_core::knowledge_construction::{self as native, Request};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use clap::{Args, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct ConstructArgs {
    #[command(subcommand)]
    pub command: ConstructSub,
}
#[derive(Debug, Subcommand)]
pub enum ConstructSub {
    /// Prepare a native transaction from {document,request} on stdin.
    /// With --file, save and read back. Optional basis_content is the exact
    /// native register bytes the caller inspected, not a replacement document.
    Apply {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Read the complete native construction and its independent relations.
    Inspect {
        frame_ref: String,
        #[arg(long)]
        file: PathBuf,
    },
    Capabilities,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    document: Option<Value>,
    #[serde(default)]
    basis_content: Option<String>,
    request: Request,
}
fn error(code: &'static str, detail: impl Into<String>) -> AikitError {
    AikitError::new(code, detail)
}
fn io_error(e: impl std::fmt::Display) -> AikitError {
    error("knowledge.constellation_io", e.to_string())
}
fn read_bounded(path: &Path) -> Result<String> {
    let file = fs::File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(error("knowledge.constellation_budget", "Wiki file exceeds the 16 MiB transaction budget"));
    }
    String::from_utf8(bytes).map_err(io_error)
}
fn input() -> Result<Input> {
    let mut data = String::new();
    std::io::stdin().take(16 * 1024 * 1024 + 1).read_to_string(&mut data).map_err(io_error)?;
    if data.len() > 16 * 1024 * 1024 {
        return Err(error("knowledge.constellation_budget", "request exceeds 16 MiB"));
    }
    serde_json::from_str(&data).map_err(|e| error("cli.usage", format!("expected native {{document?,basis_content?,request}} on stdin: {e}")))
}
pub fn run(args: ConstructArgs) -> Result<Value> {
    match args.command {
        ConstructSub::Capabilities => Ok(json!({
            "schema":native::ACTION,"owner":"ai-kit","action_ref":"aikit.constellation.apply",
            "native_carriers":["WikiFrame","WikiConstellation","WikiEdge","WikiProvenanceRef"],
            "changes":["create","inquiry_set","frame_set","member_add","member_remove","role_set","sources_set","relation_put","relation_retract","alternative_add","alternative_remove","composition_attach","place_set"],
            "revision_required":true,"operation_identity_required":true,"exact_file_basis_supported":true,
            "preparation_is_not_persistence":true,"ql_grammar_is_interpretive_truth":false,"automatic_publication":false
        })),
        ConstructSub::Inspect { frame_ref, file } => {
            let reading = native::inspect(&read_bounded(&file)?, &ResourceRef::parse(frame_ref)?)?;
            Ok(json!({"state":"read","file":file,"reading":reading}))
        }
        ConstructSub::Apply { file } => {
            let Input { document, basis_content, request } = input()?;
            match file {
                Some(path) => {
                    if document.is_some() {
                        return Err(error("cli.usage", "--file and a supplied document are mutually exclusive"));
                    }
                    persist(&path, &request, basis_content.as_deref())
                }
                None => {
                    if basis_content.is_some() {
                        return Err(error("cli.usage", "basis_content addresses an actual --file, not prepared content"));
                    }
                    let document = document.ok_or_else(|| error("cli.usage", "preparation requires the caller's native document"))?;
                    let applied = native::apply(&document.to_string(), &request)?;
                    Ok(json!({"state":"prepared","persisted":false,"applied":applied,
                        "required_return":"native Wiki save followed by independent Wiki/search readback"}))
                }
            }
        }
    }
}
/// A sidecar advisory lock serialises this native writer. Exact caller bytes
/// are compared under the lock, and a final digest check detects intervening
/// writes. Writers outside this lock retain their native concurrency laws.
fn persist(path: &Path, request: &Request, basis: Option<&str>) -> Result<Value> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.permissions().readonly() {
        return Err(error("policy.denied", "construction file is not a writable ordinary native file"));
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| error("cli.usage", "file has no native name"))?.to_string_lossy();
    let lock_path = parent.join(format!(".{name}.construction.lock"));
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(error("policy.denied", "construction lock must not be a symbolic link"));
    }
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&lock_path).map_err(io_error)?;
    lock.lock().map_err(|e| error("lock.unavailable", e.to_string()))?;
    let before = read_bounded(path)?;
    let digest = blake3::hash(before.as_bytes());
    let applied = native::apply(&before, request)?;
    if !applied.idempotent {
        // A lost reply may safely replay the same operation; it changes no
        // bytes. Every new effect must still match the caller's file basis.
        if basis.is_some_and(|expected| expected != before) {
            return Err(error("knowledge.wiki_concurrent_write", "the inspected Wiki register changed; reconcile before writing"));
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
        temp.as_file().set_permissions(metadata.permissions()).map_err(io_error)?;
        temp.write_all(applied.content.as_bytes()).map_err(io_error)?;
        temp.as_file().sync_all().map_err(io_error)?;
        if blake3::hash(read_bounded(path)?.as_bytes()) != digest {
            return Err(error("knowledge.wiki_concurrent_write", "the native file changed; nothing was saved"));
        }
        temp.persist(path).map_err(io_error)?;
        #[cfg(unix)]
        fs::File::open(parent).and_then(|dir| dir.sync_all()).map_err(io_error)?;
    }
    let after = read_bounded(path)?;
    let reading = native::inspect(&after, &request.frame_ref)?;
    if reading["frame"]["revision"] != json!(applied.revision) {
        return Err(error("knowledge.constellation_readback", "native readback does not match the applied revision; do not retry the write automatically"));
    }
    Ok(json!({"state":if applied.idempotent{"unchanged"}else{"saved"},"persisted":true,
        "file":path,"frame_ref":request.frame_ref,"revision":applied.revision,"operation_ref":request.operation_ref,
        "objects_changed":applied.objects_changed,"warnings":applied.warnings,"reading":reading,
        "content_digest":blake3::hash(after.as_bytes()).to_hex().to_string(),"indexed_availability_proven":false}))
}
