//! Native constructive actions. Preparation is not persistence. The file
//! path writes through this Wiki owner, never Central's ordinary-file writer.
use aikit_core::knowledge_construction::{self as native, Request};
use aikit_core::knowledge_wiki_facts as facts;
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
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

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
    /// Update only facts on an existing native node or whole frame.
    FactsApply {
        #[arg(long)]
        file: PathBuf,
    },
    /// Independently read the native object and its fact declarations.
    FactsInspect {
        target_ref: String,
        #[arg(long)]
        file: PathBuf,
    },
    Capabilities,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input<R = Request> {
    #[serde(default)]
    document: Option<Value>,
    #[serde(default)]
    basis_content: Option<String>,
    request: R,
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
    file.take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(error(
            "knowledge.constellation_budget",
            "Wiki file exceeds the 16 MiB transaction budget",
        ));
    }
    String::from_utf8(bytes).map_err(io_error)
}
fn input<R: serde::de::DeserializeOwned>() -> Result<Input<R>> {
    let mut data = String::new();
    std::io::stdin()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_string(&mut data)
        .map_err(io_error)?;
    if data.len() > MAX_INPUT_BYTES {
        return Err(error(
            "knowledge.constellation_budget",
            "request exceeds 16 MiB",
        ));
    }
    serde_json::from_str(&data).map_err(|e| {
        error(
            "cli.usage",
            format!("expected native {{document?,basis_content?,request}} on stdin: {e}"),
        )
    })
}
pub fn run(args: ConstructArgs) -> Result<Value> {
    match args.command {
        ConstructSub::Capabilities => Ok(json!({
            "schema":native::ACTION,"owner":"ai-kit","action_ref":"aikit.constellation.apply",
            "native_carriers":["WikiFrame","WikiConstellation","WikiEdge","WikiProvenanceRef"],
            "changes":["create","inquiry_set","frame_set","member_add","member_remove","role_set","sources_set","relation_put","relation_retract","alternative_add","alternative_remove","composition_attach","place_set","temporal_set"],
            "fact_targets":["participation","node","whole"],"facts_action":{"action_ref":"aikit.wiki.facts.apply","schema":facts::ACTION,"targets":["node","whole"],"changes":["temporal_set","place_set"],"command":["wiki-construct","facts-apply"]},
            "revision_required":true,"operation_identity_required":true,"exact_file_basis_supported":true,
            "preparation_is_not_persistence":true,"ql_grammar_is_interpretive_truth":false,"automatic_publication":false
        })),
        ConstructSub::FactsInspect { target_ref, file } => {
            let reading = facts::inspect(&read_bounded(&file)?, &ResourceRef::parse(target_ref)?)?;
            Ok(json!({"state":"read","file":file,"reading":reading}))
        }
        ConstructSub::FactsApply { file } => {
            let Input {
                document,
                basis_content,
                request,
            } = input::<facts::Request>()?;
            if document.is_some() {
                return Err(error(
                    "cli.usage",
                    "facts-apply requires an existing native file, never a replacement document",
                ));
            }
            persist_owner(
                &file,
                &OwnerRequest::Facts(&request),
                basis_content.as_deref(),
            )
        }
        ConstructSub::Inspect { frame_ref, file } => {
            let reading = native::inspect(&read_bounded(&file)?, &ResourceRef::parse(frame_ref)?)?;
            Ok(json!({"state":"read","file":file,"reading":reading}))
        }
        ConstructSub::Apply { file } => {
            let Input {
                document,
                basis_content,
                request,
            } = input()?;
            match file {
                Some(path) => {
                    if document.is_some() {
                        return Err(error(
                            "cli.usage",
                            "--file and a supplied document are mutually exclusive",
                        ));
                    }
                    persist_owner(
                        &path,
                        &OwnerRequest::Construction(&request),
                        basis_content.as_deref(),
                    )
                }
                None => {
                    if basis_content.is_some() {
                        return Err(error(
                            "cli.usage",
                            "basis_content addresses an actual --file, not prepared content",
                        ));
                    }
                    let document = document.ok_or_else(|| {
                        error(
                            "cli.usage",
                            "preparation requires the caller's native document",
                        )
                    })?;
                    let applied = native::apply(&document.to_string(), &request)?;
                    Ok(
                        json!({"state":"prepared","persisted":false,"applied":applied,
                        "required_return":"native Wiki save followed by independent Wiki/search readback"}),
                    )
                }
            }
        }
    }
}
/// A sidecar advisory lock serialises this native writer. Exact caller bytes
/// are compared under the lock, and a final digest check detects intervening
/// writes. Writers outside this lock retain their native concurrency laws.
fn persist_owner(path: &Path, request: &OwnerRequest<'_>, basis: Option<&str>) -> Result<Value> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.permissions().readonly()
    {
        return Err(error(
            "policy.denied",
            "construction file is not a writable ordinary native file",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| error("cli.usage", "file has no native name"))?
        .to_string_lossy();
    let lock_path = parent.join(format!(".{name}.construction.lock"));
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(error(
            "policy.denied",
            "construction lock must not be a symbolic link",
        ));
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(io_error)?;
    lock.lock()
        .map_err(|e| error("lock.unavailable", e.to_string()))?;
    let before = read_bounded(path)?;
    let digest = blake3::hash(before.as_bytes());
    let applied = request.apply(&before)?;
    if applied.content.len() > MAX_INPUT_BYTES {
        return Err(error(
            "knowledge.constellation_budget",
            "The resulting Wiki register exceeds the 16 MiB native read budget; nothing was saved",
        ));
    }
    if !applied.idempotent {
        // A lost reply may safely replay the same operation; it changes no
        // bytes. Every new effect must still match the caller's file basis.
        if basis.is_some_and(|expected| expected != before) {
            return Err(error(
                "knowledge.wiki_concurrent_write",
                "the inspected Wiki register changed; reconcile before writing",
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
        temp.as_file()
            .set_permissions(metadata.permissions())
            .map_err(io_error)?;
        temp.write_all(applied.content.as_bytes())
            .map_err(io_error)?;
        temp.as_file().sync_all().map_err(io_error)?;
        if blake3::hash(read_bounded(path)?.as_bytes()) != digest {
            return Err(error(
                "knowledge.wiki_concurrent_write",
                "the native file changed; nothing was saved",
            ));
        }
        temp.persist(path).map_err(io_error)?;
        #[cfg(unix)]
        fs::File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(io_error)?;
    }
    let after = read_bounded(path)?;
    let reading = request.inspect(&after)?;
    if request.read_revision(&reading) != json!(applied.revision) {
        return Err(error("knowledge.constellation_readback", "native readback does not match the applied revision; do not retry the write automatically"));
    }
    let mut result = json!({"state":if applied.idempotent{"unchanged"}else{"saved"},"persisted":true,
        "file":path,"revision":applied.revision,"operation_ref":request.operation_ref(),
        "objects_changed":applied.objects_changed,"warnings":applied.warnings,"reading":reading,
        "content_digest":blake3::hash(after.as_bytes()).to_hex().to_string(),"indexed_availability_proven":false});
    match request {
        OwnerRequest::Construction(r) => {
            result["frame_ref"] = json!(r.frame_ref);
        }
        OwnerRequest::Facts(r) => {
            result["target"] = json!(r.target);
        }
    }
    Ok(result)
}

/// Both Actions share the exact lock, byte-CAS, atomic save and readback path.
enum OwnerRequest<'a> {
    Construction(&'a Request),
    Facts(&'a facts::Request),
}
struct Commit {
    revision: u64,
    idempotent: bool,
    content: String,
    objects_changed: Vec<String>,
    warnings: Vec<String>,
}
impl OwnerRequest<'_> {
    fn apply(&self, input: &str) -> Result<Commit> {
        Ok(match self {
            Self::Construction(request) => {
                let a = native::apply(input, request)?;
                Commit {
                    revision: a.revision,
                    idempotent: a.idempotent,
                    content: a.content,
                    objects_changed: a.objects_changed,
                    warnings: a.warnings,
                }
            }
            Self::Facts(request) => {
                let a = facts::apply(input, request)?;
                Commit {
                    revision: a.revision,
                    idempotent: a.idempotent,
                    content: a.content,
                    objects_changed: a.objects_changed,
                    warnings: a.warnings,
                }
            }
        })
    }
    fn inspect(&self, input: &str) -> Result<Value> {
        match self {
            Self::Construction(r) => native::inspect(input, &r.frame_ref),
            Self::Facts(r) => facts::inspect(input, r.target.reference()),
        }
    }
    fn read_revision(&self, reading: &Value) -> Value {
        match self {
            Self::Construction(_) => reading["frame"]["revision"].clone(),
            Self::Facts(_) => reading["object"]["revision"].clone(),
        }
    }
    fn operation_ref(&self) -> &ResourceRef {
        match self {
            Self::Construction(r) => &r.operation_ref,
            Self::Facts(r) => &r.operation_ref,
        }
    }
}
