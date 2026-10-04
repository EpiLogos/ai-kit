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
    fs,
    io::Read,
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
fn io_error(e: std::io::Error) -> AikitError {
    error("knowledge.constellation_io", e.to_string())
        .with("cause_kind", format!("{:?}", e.kind()))
        .with("cause_raw_os_error", json!(e.raw_os_error()).to_string())
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
    String::from_utf8(bytes).map_err(|e| error("knowledge.constellation_io", e.to_string()))
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
/// Construct semantics remain with this owner. Publication uses the common
/// Wiki physical-file lock and exact observed basis through durable rename;
/// the retired construction-specific lock is never a second write authority.
fn persist_owner(path: &Path, request: &OwnerRequest<'_>, basis: Option<&str>) -> Result<Value> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(error(
            "policy.denied",
            "construction file is not an ordinary native file",
        ));
    }
    let before = read_bounded(path)?;
    let publication_basis =
        aikit_adapters::projectcentral::publication::content_hash(before.as_bytes());
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
    }
    // Exact operation replay still admits the current physical source through
    // the common owner. Its exact-byte no-op requires no source write access.
    let published = aikit_adapters::projectcentral::publication::publish_wiki(
        path,
        &applied.content,
        &publication_basis,
    )
    .map_err(|cause| cause.with("operation_ref", request.operation_ref().to_string()))?;
    let after = read_bounded(path).map_err(|cause| {
        readback_error(cause, published, request.operation_ref(), applied.revision)
    })?;
    let reading = request.inspect(&after).map_err(|cause| {
        readback_error(cause, published, request.operation_ref(), applied.revision)
    })?;
    if request.read_revision(&reading) != json!(applied.revision) {
        return Err(readback_error(
            error(
                "knowledge.constellation_readback",
                "native readback does not match the applied revision",
            ),
            published,
            request.operation_ref(),
            applied.revision,
        ));
    }
    let mut result = json!({"state":if applied.idempotent{"unchanged"}else{"saved"},"persisted":true,
        "published":published,
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

fn readback_error(
    cause: AikitError,
    published: bool,
    operation_ref: &ResourceRef,
    revision: u64,
) -> AikitError {
    let returned = if published {
        error("knowledge.constellation_readback",
            format!("native publication committed but independent construction readback failed: {cause}"))
            .with("cause_code", cause.code())
            .with("original_error", json!({
                "code":cause.code(), "message":cause.message(), "details":cause.details(),
            }).to_string())
            .with("automatic_retry", "false")
    } else {
        cause
    };
    returned.with("published", published.to_string())
        .with("operation_ref", operation_ref.to_string())
        .with("expected_revision", revision.to_string())
        .with("instruction", if published {
            "may have committed; do not retry the write automatically; inspect the original operation and retained source"
        } else {
            "this invocation published no new effect; inspect the original operation and retained source before retry"
        })
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

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn source_and_request() -> (tempfile::TempDir, PathBuf, Request, String) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wiki.json");
        let source = json!({"profile":"okf-wiki/v1","retained_header":{"native":true},"objects":[
            {"object":"space","profile":"okf-wiki/v1","ref":"wiki:project","revision":1,"node_refs":[]}
        ]}).to_string();
        fs::write(&path, &source).unwrap();
        let request = serde_json::from_value(json!({
            "schema":native::ACTION,"frame_ref":"wiki:publication-readback","expected_revision":0,
            "actor_ref":"human:author","operation_ref":"operation:publication-readback",
            "changes":[{"change":"create","anchor_ref":"wiki:publication-anchor","title":"Source readback",
                "inquiry":{"question":"Does this operation retain its source?"},"space_refs":["wiki:project"]}]
        })).unwrap();
        (directory, path, request, source)
    }

    #[test]
    fn real_construction_replay_on_readonly_file_publishes_no_new_effect() {
        let (_directory, path, request, _) = source_and_request();
        let owner = OwnerRequest::Construction(&request);
        let saved = persist_owner(&path, &owner, None).unwrap();
        assert_eq!(saved["published"], true);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let before = fs::read(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let replay = persist_owner(&path, &owner, None).unwrap();
        assert_eq!(replay["state"], "unchanged");
        assert_eq!(replay["published"], false);
        assert_eq!(replay["operation_ref"], json!(request.operation_ref));
        assert_eq!(fs::read(&path).unwrap(), before);
        let after = fs::metadata(&path).unwrap();
        assert_eq!(
            (after.dev(), after.ino(), after.mtime(), after.mtime_nsec()),
            (
                metadata.dev(),
                metadata.ino(),
                metadata.mtime(),
                metadata.mtime_nsec()
            )
        );
    }

    #[test]
    fn actual_idempotent_replay_requires_single_link_physical_admission() {
        let (_directory, path, request, _) = source_and_request();
        let owner = OwnerRequest::Construction(&request);
        assert_eq!(
            persist_owner(&path, &owner, None).unwrap()["published"],
            true
        );
        let alias = path.with_extension("actual-hardlink.json");
        fs::hard_link(&path, &alias).unwrap();
        let before = fs::read(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.nlink(), 2);
        let failure = persist_owner(&path, &owner, None).unwrap_err();
        assert_eq!(failure.code(), "knowledge.wiki_publication_identity");
        assert_eq!(
            failure.details()["operation_ref"],
            request.operation_ref.as_str()
        );
        assert!(!failure.details().contains_key("published"));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(&alias).unwrap(), before);
        let after = fs::metadata(&path).unwrap();
        assert_eq!(
            (after.dev(), after.ino(), after.mtime(), after.mtime_nsec()),
            (
                metadata.dev(),
                metadata.ino(),
                metadata.mtime(),
                metadata.mtime_nsec()
            )
        );
    }

    #[test]
    fn lost_file_after_real_native_commit_returns_original_operation_and_uncertainty() {
        let (_directory, path, request, _) = source_and_request();
        let saved = persist_owner(&path, &OwnerRequest::Construction(&request), None).unwrap();
        assert_eq!(saved["published"], true);
        let retained = path.with_extension("retained.json");
        fs::rename(&path, &retained).unwrap();
        let cause = read_bounded(&path).unwrap_err();
        assert_eq!(cause.code(), "knowledge.constellation_io");
        let actual_os_error = fs::File::open(&path).unwrap_err();
        assert_eq!(actual_os_error.kind(), std::io::ErrorKind::NotFound);
        assert!(actual_os_error.raw_os_error().is_some());
        assert_eq!(cause.details()["cause_kind"], "NotFound");
        assert_eq!(
            cause.details()["cause_raw_os_error"],
            json!(actual_os_error.raw_os_error()).to_string()
        );
        let original_error = json!({
            "code":cause.code(), "message":cause.message(), "details":cause.details(),
        });
        let failure = readback_error(cause, true, &request.operation_ref, 1);
        assert_eq!(failure.code(), "knowledge.constellation_readback");
        assert_eq!(
            failure.details()["cause_code"],
            "knowledge.constellation_io"
        );
        assert_eq!(failure.details()["published"], "true");
        assert_eq!(failure.details()["automatic_retry"], "false");
        assert_eq!(
            failure.details()["operation_ref"],
            request.operation_ref.as_str()
        );
        assert_eq!(
            serde_json::from_str::<Value>(&failure.details()["original_error"]).unwrap(),
            original_error
        );
        assert_eq!(original_error["details"]["cause_kind"], "NotFound");
        assert_eq!(
            original_error["details"]["cause_raw_os_error"],
            json!(actual_os_error.raw_os_error()).to_string()
        );
        assert!(failure.details()["instruction"].contains("do not retry"));
        assert_eq!(
            native::inspect(&read_bounded(&retained).unwrap(), &request.frame_ref).unwrap()
                ["frame"]["revision"],
            1
        );
    }

    #[test]
    fn replaced_semantic_source_after_commit_preserves_parse_failure_and_replay_distinction() {
        let (_directory, path, request, original) = source_and_request();
        let owner = OwnerRequest::Construction(&request);
        assert_eq!(
            persist_owner(&path, &owner, None).unwrap()["published"],
            true
        );
        let retained = path.with_extension("retained.json");
        fs::rename(&path, &retained).unwrap();
        fs::write(&path, &original).unwrap();
        let cause = owner.inspect(&read_bounded(&path).unwrap()).unwrap_err();
        let code = cause.code();
        let original_error = json!({
            "code":cause.code(), "message":cause.message(), "details":cause.details(),
        });
        let failure = readback_error(cause.clone(), true, &request.operation_ref, 1);
        assert_eq!(failure.code(), "knowledge.constellation_readback");
        assert_eq!(failure.details()["cause_code"], code);
        assert_eq!(failure.details()["published"], "true");
        assert_eq!(
            serde_json::from_str::<Value>(&failure.details()["original_error"]).unwrap(),
            original_error
        );
        let unchanged = readback_error(cause, false, &request.operation_ref, 1);
        assert_eq!(unchanged.code(), code);
        assert_eq!(unchanged.details()["published"], "false");
        assert_eq!(
            unchanged.details()["operation_ref"],
            request.operation_ref.as_str()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            native::inspect(&read_bounded(&retained).unwrap(), &request.frame_ref).unwrap()
                ["frame"]["revision"],
            1
        );
    }
}
