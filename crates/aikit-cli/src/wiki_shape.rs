//! `aikit wiki-shape` — CASE 18's product surface over the QL shape contract.
//!
//! `crates/aikit-core/src/knowledge_wiki_shape.rs` and
//! `knowledge_wiki_shape_v2.rs` carry the structural floor for QL-shaped
//! `WikiConstellation`s (families A–C, D1/D2/D3 completion degrees, node
//! stance as declared data, the 6+6′ compression through the 0 // 1 trinity)
//! but had no caller anywhere in the binary: every proof of that module ran
//! inside `cargo test`, never through a dispatched command. This module is
//! that missing product surface — `declare` writes a QL-shaped constellation,
//! `validate` runs the contract's structural floor over a whole Wiki file,
//! `compress` proves the 6+6′ formation — so the capability is exercisable
//! from the real binary, not only from its own unit tests.
//!
//! Deliberately a sibling of `wiki` (`aikit wiki-shape …`, not
//! `aikit wiki shape …`): `crates/aikit-cli/src/wiki.rs` is being worked
//! concurrently in a sibling worktree on this same case family, so this
//! surface owns its own small file end to end rather than adding a match arm
//! to `wiki::run`'s dispatch.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json as jval, Value};
use sha2::Digest;

use aikit_core::knowledge_wiki_shape_v2::{
    compress_six_plus_six_prime, validate_constellation_shape_declaration, wiki_node_stance,
    QL_SHAPE_DECLARATION_EXTENSION,
};
use aikit_core::knowledge_wiki_write::{
    apply_wiki_mutation, WikiDocument, WikiMutationLedger, WikiMutationOutcome,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{wiki_constellation_grain, AikitError, Result, WikiObject};

use crate::cli::{
    WikiShapeCmd, WikiShapeCompressArgs, WikiShapeDeclareArgs, WikiShapeSub, WikiShapeValidateArgs,
};
use crate::json;

/// What a `wiki-shape` command did, before the envelope is wrapped around it.
pub struct WikiShapeOutcome {
    pub data: Value,
    pub warnings: Vec<String>,
    pub exit_code: i32,
}

impl WikiShapeOutcome {
    fn wrote(data: Value, outcome: &WikiMutationOutcome) -> Self {
        Self {
            data,
            warnings: outcome.warnings.clone(),
            exit_code: json::EXIT_OK,
        }
    }

    fn reported(data: Value, warnings: Vec<String>, exit_code: i32) -> Self {
        Self {
            data,
            warnings,
            exit_code,
        }
    }
}

/// Dispatch one `aikit wiki-shape` invocation.
pub fn run(command: WikiShapeCmd) -> Result<WikiShapeOutcome> {
    match command.command {
        WikiShapeSub::Declare(args) => declare(&args),
        WikiShapeSub::Validate(args) => validate(&args),
        WikiShapeSub::Compress(args) => compress(&args),
    }
}

// ---------------------------------------------------------------------------
// declare
// ---------------------------------------------------------------------------

/// Write a WikiFrame carrying one QL-shaped WikiConstellation. The whole
/// Frame JSON body comes from stdin — the same convention `wiki node create
/// --stdin` already uses — because a Frame's constellations, members and
/// return canon are too rich a shape for a flag surface, and this command
/// must never guess at declared data.
///
/// Contract-level structural validation runs *before* the write lands:
/// conjugate-requires-direct (via [`wiki_constellation_grain`], which walks
/// `positioned_members` and refuses a conjugate position with no matching
/// direct one), and — for any constellation that names a
/// `aikit.ql-shape/v1` declaration — the declared `shape_ref`/`grain` against
/// the contract's own computed field
/// ([`validate_constellation_shape_declaration`]). `WikiObject::parse`
/// already runs `WikiConstellation::validate` (position range/uniqueness,
/// return-through-declared-anchor, `ground_kind` vocabulary) as part of
/// parsing the stdin body, so that floor is enforced twice over: once at
/// parse, once again explicitly here for the QL-specific parity law that
/// `WikiConstellation::validate` alone does not reach.
fn declare(args: &WikiShapeDeclareArgs) -> Result<WikiShapeOutcome> {
    let body = read_stdin()?;
    let value: Value = serde_json::from_str(&body).map_err(|error| {
        AikitError::new("cli.usage", format!("invalid frame JSON on stdin: {error}"))
    })?;
    let object = WikiObject::parse(&value)?;
    let kind = kind_of(&object);
    let WikiObject::Frame(frame) = object else {
        return Err(AikitError::new(
            "cli.usage",
            format!("`wiki-shape declare` writes a frame; stdin carried a {kind}"),
        ));
    };
    let requested = ResourceRef::parse(&args.frame_ref)?;
    if frame.ref_id != requested {
        return Err(AikitError::new(
            "cli.usage",
            format!(
                "the body names {} but the command was given {requested}; a write never rewrites identity",
                frame.ref_id
            ),
        )
        .with("body_ref", frame.ref_id.to_string())
        .with("requested_ref", requested.to_string()));
    }

    let mut validated_shape_fields = 0usize;
    let mut unknown_shape_refs = Vec::new();
    let mut grains = Vec::new();
    for constellation in &frame.constellations {
        // Enforces conjugate-requires-direct from the contract (v1's
        // `positioned_members`): a conjugate positional participation
        // requires the same direct position, per the pinned structural
        // contract's own §4.
        let grain = wiki_constellation_grain(constellation)?;
        grains.push(jval!({
            "anchor": constellation.anchor_ref.to_string(),
            "grain": grain.as_str(),
        }));
        // Enforces the declared shape_ref against the contract's own field,
        // grain agreement, and the whole-anchor-never-a-seventh-member law.
        // Unknown/unversioned refs are preserved, not refused — the openness
        // law that lets the theory evolve shape refs without an engine
        // release.
        let fields = validate_constellation_shape_declaration(constellation)?;
        if let Some(declaration) = constellation.extensions.get(QL_SHAPE_DECLARATION_EXTENSION) {
            if let Some(shape_ref) = declaration.get("shape_ref").and_then(Value::as_str) {
                if fields.is_empty() {
                    unknown_shape_refs.push(shape_ref.to_string());
                }
            }
        }
        validated_shape_fields += fields.len();
    }

    let file = &args.file;
    let frame_for_write = frame.clone();
    let outcome = mutate_file(file, |doc, ledger| {
        let created = doc.create_object(WikiObject::Frame(frame_for_write))?;
        ledger.record(created);
        Ok(())
    })?;

    Ok(WikiShapeOutcome::wrote(
        jval!({
            "command": "wiki-shape.declare",
            "file": file.display().to_string(),
            "ref": frame.ref_id.to_string(),
            "constellations": frame.constellations.len(),
            "grains": grains,
            "validated_shape_fields": validated_shape_fields,
            "unknown_or_unversioned_shape_refs_preserved": unknown_shape_refs,
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

/// Read-only: run every constellation any WikiFrame in `file` holds through
/// the contract's structural floor, and report node stance (declared data,
/// never inferred) for the anchor and every positioned member this file also
/// holds as a WikiNode.
fn validate(args: &WikiShapeValidateArgs) -> Result<WikiShapeOutcome> {
    let input = read(&args.file)?;
    let document = WikiDocument::parse(&input)?;

    let mut frames = Vec::new();
    let mut errors = Vec::new();
    for object in document.objects() {
        let WikiObject::Frame(frame) = object else {
            continue;
        };
        let mut constellations = Vec::new();
        for constellation in &frame.constellations {
            let grain = match wiki_constellation_grain(constellation) {
                Ok(grain) => grain,
                Err(error) => {
                    errors.push(jval!({
                        "frame": frame.ref_id.to_string(),
                        "anchor": constellation.anchor_ref.to_string(),
                        "error": error.to_string(),
                    }));
                    continue;
                }
            };
            let fields = match validate_constellation_shape_declaration(constellation) {
                Ok(fields) => fields,
                Err(error) => {
                    errors.push(jval!({
                        "frame": frame.ref_id.to_string(),
                        "anchor": constellation.anchor_ref.to_string(),
                        "error": error.to_string(),
                    }));
                    continue;
                }
            };

            let anchor_stance = node_stance_of(&document, &constellation.anchor_ref)?;
            let mut members = Vec::new();
            for member in &constellation.members {
                let stance = node_stance_of(&document, &member.ref_id)?;
                members.push(jval!({
                    "ref": member.ref_id.to_string(),
                    "position": member.position,
                    "conjugate": member.conjugate,
                    "stance": stance,
                }));
            }
            let returns: Vec<Value> = constellation
                .returns
                .iter()
                .map(|entry| {
                    jval!({
                        "through_anchor_ref": entry.through_anchor_ref.to_string(),
                        "ground_ref": entry.ground_ref.to_string(),
                        "ground_kind": entry.ground_kind,
                    })
                })
                .collect();

            constellations.push(jval!({
                "anchor": constellation.anchor_ref.to_string(),
                "anchor_stance": anchor_stance,
                "grain": grain.as_str(),
                "validated_shape_fields": fields.len(),
                "shape_field_refs": fields
                    .iter()
                    .filter_map(|field| field.shape_ref.clone())
                    .collect::<Vec<_>>(),
                "returns": returns,
                "members": members,
            }));
        }
        frames.push(jval!({
            "frame": frame.ref_id.to_string(),
            "constellations": constellations,
        }));
    }

    let exit_code = if errors.is_empty() {
        json::EXIT_OK
    } else {
        json::EXIT_GENERIC
    };
    Ok(WikiShapeOutcome::reported(
        jval!({
            "command": "wiki-shape.validate",
            "file": args.file.display().to_string(),
            "valid": errors.is_empty(),
            "frames": frames,
            "errors": errors,
        }),
        Vec::new(),
        exit_code,
    ))
}

fn node_stance_of(document: &WikiDocument, resource: &ResourceRef) -> Result<Value> {
    match document.object(resource) {
        Some(WikiObject::Node(node)) => match wiki_node_stance(node)? {
            Some(stance) => Ok(jval!(stance.as_str())),
            None => Ok(jval!(null)),
        },
        Some(_) => Ok(jval!(null)),
        None => Ok(jval!("not-held")),
    }
}

// ---------------------------------------------------------------------------
// compress
// ---------------------------------------------------------------------------

/// Read-only: find the constellation whose whole-anchor is `anchor_ref`, read
/// its declared six generated-relation refs (the `"generated"` map inside its
/// `aikit.ql-shape/v1` declaration — declared data, never inferred from
/// prose) and compress the direct/conjugate sixfold plus that generated field
/// through the 0 // 1 trinity.
fn compress(args: &WikiShapeCompressArgs) -> Result<WikiShapeOutcome> {
    let input = read(&args.file)?;
    let document = WikiDocument::parse(&input)?;
    let anchor = ResourceRef::parse(&args.anchor_ref)?;

    let constellation = document
        .objects()
        .iter()
        .filter_map(|object| match object {
            WikiObject::Frame(frame) => Some(frame),
            _ => None,
        })
        .flat_map(|frame| frame.constellations.iter())
        .find(|constellation| constellation.anchor_ref == anchor)
        .cloned()
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_shape_anchor_not_found",
                format!(
                    "{} declares no constellation with this whole-anchor in this file",
                    anchor
                ),
            )
            .with("anchor", anchor.to_string())
        })?;

    let declaration = constellation
        .extensions
        .get(QL_SHAPE_DECLARATION_EXTENSION)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_shape_no_declaration",
                "compression requires a declared `aikit.ql-shape/v1` shape declaration carrying the six generated-relation refs",
            )
        })?;
    let generated_obj = declaration
        .get("generated")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_shape_no_generated_declaration",
                "the shape declaration carries no `generated` map of position -> generated-relation ref",
            )
        })?;
    let mut generated = BTreeMap::new();
    for (position_raw, value) in generated_obj {
        let position: u8 = position_raw.parse().map_err(|_| {
            AikitError::new(
                "knowledge.wiki_shape_invalid_generated_position",
                format!("`generated` position `{position_raw}` is not 0..=5"),
            )
        })?;
        let resource = value.as_str().ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_shape_invalid_generated_ref",
                format!("`generated.{position_raw}` must be a ref string"),
            )
        })?;
        generated.insert(position, ResourceRef::parse(resource)?);
    }

    let compression = compress_six_plus_six_prime(&constellation, &generated)?;
    let data = serde_json::to_value(&compression).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_shape_serialization_failed",
            format!("failed to serialize the compression: {error}"),
        )
    })?;
    Ok(WikiShapeOutcome::reported(data, Vec::new(), json::EXIT_OK))
}

// ---------------------------------------------------------------------------
// Small helpers — deliberately local to this file rather than reused from
// `wiki.rs`'s equivalents (which are private to that module and that module
// is out of bounds for this case; see the module doc comment above).
// ---------------------------------------------------------------------------

fn mutate_file<F>(path: &Path, mutate: F) -> Result<WikiMutationOutcome>
where
    F: FnOnce(&mut WikiDocument, &mut WikiMutationLedger) -> Result<()>,
{
    let input = read(path)?;
    let base_hash = content_hash(input.as_bytes());
    let (rendered, outcome) = apply_wiki_mutation(&input, mutate)?;
    persist(path, &rendered, &base_hash)?;
    Ok(outcome)
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_file_unreadable",
            format!("could not read {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| {
            AikitError::new(
                "cli.stdin_unreadable",
                format!("could not read stdin: {error}"),
            )
        })?;
    if buffer.trim().is_empty() {
        return Err(AikitError::new(
            "cli.usage",
            "no frame JSON was piped in on stdin",
        ));
    }
    Ok(buffer)
}

fn content_hash(bytes: &[u8]) -> String {
    let mut digest = sha2::Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn current_hash(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(content_hash(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AikitError::new(
            "knowledge.wiki_file_unreadable",
            format!(
                "could not re-read {} to verify it is unchanged before writing: {error}",
                path.display()
            ),
        )
        .with("path", path.display().to_string())),
    }
}

fn persist(path: &Path, rendered: &str, base_hash: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "wiki.json".to_string());
    let temp = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&temp, rendered).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_write_failed",
            format!("could not write {}: {error}", temp.display()),
        )
        .with("path", temp.display().to_string())
    })?;

    if current_hash(path)?.as_deref() != Some(base_hash) {
        let _ = std::fs::remove_file(&temp);
        return Err(AikitError::new(
            "knowledge.wiki_concurrent_write",
            format!(
                "{} changed since it was read; a peer write landed first. Re-read the file and re-apply this mutation.",
                path.display()
            ),
        )
        .with("path", path.display().to_string()));
    }

    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        AikitError::new(
            "knowledge.wiki_write_failed",
            format!("could not replace {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

fn mutation_outcome(outcome: &WikiMutationOutcome) -> Value {
    jval!({
        "changed": outcome.changed,
        "proposals": serde_json::to_value(&outcome.proposals).unwrap_or_default(),
        "touched": serde_json::to_value(&outcome.touched).unwrap_or_default(),
    })
}

fn kind_of(object: &WikiObject) -> &'static str {
    match object {
        WikiObject::Space(_) => "space",
        WikiObject::Node(_) => "node",
        WikiObject::Edge(_) => "edge",
        WikiObject::Frame(_) => "frame",
        WikiObject::Reading(_) => "reading",
    }
}
