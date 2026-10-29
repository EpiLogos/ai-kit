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
//! O-I #742 (the Canvas's Form page) adds the rest of the construction
//! chain's Wiki link: `validate --stdin` checks a draft frame through the same
//! floor with nothing written; `declare --expected-revision N` changes the
//! roles of a frame the file holds, revision-checked; and `target` projects
//! one constellation as QL-MEF's Technē target, carrying the QL shape binding
//! the constellation keeps.
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

use aikit_core::knowledge_construction::CONSTRUCTION;
use aikit_core::knowledge_wiki_shape_v2::{
    compress_six_plus_six_prime, validate_constellation_shape_declaration, wiki_node_stance,
    QL_SHAPE_DECLARATION_EXTENSION,
};
use aikit_core::knowledge_wiki_write::{
    apply_wiki_mutation, WikiDocument, WikiMutationLedger, WikiMutationOutcome,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{wiki_constellation_grain, AikitError, Result, WikiFrame, WikiObject};

use crate::cli::{
    WikiShapeCmd, WikiShapeCompressArgs, WikiShapeDeclareArgs, WikiShapeSub, WikiShapeTargetArgs,
    WikiShapeValidateArgs,
};

/// The extension a constellation keeps QL-MEF's shape binding under (QL-MEF
/// `ql_wiki::WIKI_SHAPE_BINDING_EXTENSION`, `ql shape bind`): carried, never
/// interpreted here, and handed to QL as its Technē target's extension.
pub const QL_SHAPE_BINDING_EXTENSION: &str = "ql-mef/shape-binding/v1";
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
        WikiShapeSub::Target(args) => target(&args),
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
    let frame = frame_of_body(&value)?;
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
    let checked = floor(frame)?;

    let file = &args.file;
    let frame_ref = checked.frame.ref_id.clone();
    let constellations = checked.frame.constellations.len();
    let frame_for_write = checked.frame;
    let outcome = mutate_file(file, |doc, ledger| {
        put_frame(doc, ledger, frame_for_write, args.expected_revision)
    })?;
    // Read back what the file now holds.
    let revision = WikiDocument::parse(&read(file)?)?
        .object(&frame_ref)
        .map(WikiObject::revision);

    Ok(WikiShapeOutcome::wrote(
        jval!({
            "command": "wiki-shape.declare",
            "file": file.display().to_string(),
            "ref": frame_ref.to_string(),
            "change": if args.expected_revision.is_some() { "update" } else { "create" },
            "revision": revision,
            "constellations": constellations,
            "grains": checked.grains,
            "validated_shape_fields": checked.validated_shape_fields,
            "unknown_or_unversioned_shape_refs_preserved": checked.unknown_shape_refs,
            "outcome": mutation_outcome(&outcome),
        }),
        &outcome,
    ))
}

/// The frame a body carries. `WikiObject::parse` runs
/// `WikiConstellation::validate` (position range and uniqueness,
/// return-through-declared-anchor, `ground_kind` vocabulary) as it parses.
fn frame_of_body(value: &Value) -> Result<WikiFrame> {
    let object = WikiObject::parse(value)?;
    let kind = kind_of(&object);
    let WikiObject::Frame(frame) = object else {
        return Err(AikitError::new(
            "cli.usage",
            format!("`wiki-shape` declares a frame; the body carried a {kind}"),
        ));
    };
    Ok(frame)
}

/// A frame that passed the structural floor, with what the floor read.
struct Checked {
    frame: WikiFrame,
    grains: Vec<Value>,
    validated_shape_fields: usize,
    unknown_shape_refs: Vec<String>,
}

/// The QL floor every declared constellation passes before a write (and a
/// draft before its check): conjugate-requires-direct (via
/// [`wiki_constellation_grain`], which walks `positioned_members` and refuses
/// a conjugate position with no matching direct one), and — for any
/// constellation that names a `aikit.ql-shape/v1` declaration — the declared
/// `shape_ref`/`grain` against the contract's own computed field
/// ([`validate_constellation_shape_declaration`]).
fn floor(frame: WikiFrame) -> Result<Checked> {
    let mut validated_shape_fields = 0usize;
    let mut unknown_shape_refs = Vec::new();
    let mut grains = Vec::new();
    for constellation in &frame.constellations {
        let grain = wiki_constellation_grain(constellation)?;
        grains.push(jval!({
            "anchor": constellation.anchor_ref.to_string(),
            "grain": grain.as_str(),
        }));
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
    Ok(Checked {
        frame,
        grains,
        validated_shape_fields,
        unknown_shape_refs,
    })
}

/// The write `declare` makes. Without an expected revision it creates the
/// frame (and refuses one that exists). With one, it changes the roles of the
/// frame the document holds at exactly that revision: each constellation of
/// the body replaces the held constellation with the same whole-anchor (or
/// joins the frame), the body's member refs join the frame's, the rest of the
/// held frame stays, and the frame advances one revision. A construction
/// frame (`aikit.constellation/v1`) changes through `wiki-construct apply`,
/// which keeps its participations; an unchanged body writes nothing.
fn put_frame(
    doc: &mut WikiDocument,
    ledger: &mut WikiMutationLedger,
    frame: WikiFrame,
    expected_revision: Option<u64>,
) -> Result<()> {
    let Some(expected) = expected_revision else {
        ledger.record(doc.create_object(WikiObject::Frame(frame))?);
        return Ok(());
    };
    let reference = frame.ref_id.clone();
    let held = match doc.object(&reference) {
        Some(WikiObject::Frame(held)) => held.clone(),
        Some(other) => {
            return Err(AikitError::new(
                "knowledge.wiki_ref_exists",
                format!(
                    "{reference} is a {} in this document, not a frame",
                    kind_of(other)
                ),
            )
            .with("ref", reference.to_string()))
        }
        None => {
            return Err(AikitError::new(
                "knowledge.wiki_object_missing",
                format!(
                    "Wiki frame {reference} is not in this document; declare it without --expected-revision"
                ),
            )
            .with("ref", reference.to_string()))
        }
    };
    if held.revision != expected {
        return Err(AikitError::new(
            "knowledge.wiki_revision_conflict",
            format!(
                "{reference} stands at revision {}, not {expected}: read it again before changing its roles",
                held.revision
            ),
        )
        .with("ref", reference.to_string())
        .with("revision", held.revision.to_string()));
    }
    if held.extensions.contains_key(CONSTRUCTION) {
        return Err(AikitError::new(
            "knowledge.wiki_shape_construction_frame",
            format!(
                "{reference} is a construction; its roles change through `aikit wiki-construct apply` (role_set), which keeps its participations"
            ),
        )
        .with("ref", reference.to_string()));
    }
    let mut next = held;
    for constellation in frame.constellations {
        match next
            .constellations
            .iter_mut()
            .find(|held| held.anchor_ref == constellation.anchor_ref)
        {
            Some(slot) => *slot = constellation,
            None => next.constellations.push(constellation),
        }
    }
    for member in frame.member_refs {
        if !next.member_refs.contains(&member) {
            next.member_refs.push(member);
        }
    }
    let next = WikiObject::Frame(next);
    if doc.holds_equivalent(&next) {
        return Ok(());
    }
    ledger.record(doc.update_object(next)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

/// Read-only: run every constellation any WikiFrame in `file` holds through
/// the contract's structural floor, and report each frame's revision and
/// node stance (declared data, never inferred) for the anchor and every
/// positioned member this file also holds as a WikiNode. With `--stdin`, the
/// frame checked is a draft body instead ([`validate_draft`]).
fn validate(args: &WikiShapeValidateArgs) -> Result<WikiShapeOutcome> {
    if args.stdin {
        return validate_draft(args);
    }
    let input = read(&args.file)?;
    let document = WikiDocument::parse(&input)?;

    let mut frames = Vec::new();
    let mut errors = Vec::new();
    for object in document.objects() {
        let WikiObject::Frame(frame) = object else {
            continue;
        };
        frames.push(frame_report(&document, frame, &mut errors)?);
    }
    Ok(validation(&args.file, frames, errors, None))
}

/// The dry run of `declare` (O-I #742, the Canvas's Form page Check): a frame
/// body that is not written yet, run through the floor `declare` enforces and
/// the write simulated over the file (create, or with `--expected-revision`
/// the change of roles on the held frame), then reported as `validate`
/// reports a held frame. Nothing is written. A body the floor or the write
/// refuses is reported as the owner's errors, not as a failed command; only a
/// body that is not JSON is a usage error.
fn validate_draft(args: &WikiShapeValidateArgs) -> Result<WikiShapeOutcome> {
    let input = read(&args.file)?;
    let body = read_stdin()?;
    let value: Value = serde_json::from_str(&body).map_err(|error| {
        AikitError::new("cli.usage", format!("invalid frame JSON on stdin: {error}"))
    })?;
    let draft_ref = value.get("ref").and_then(Value::as_str).map(str::to_owned);
    let mut errors = Vec::new();
    let mut frames = Vec::new();
    let mut revision = None;
    let simulated = (|| -> Result<(WikiDocument, ResourceRef)> {
        let checked = floor(frame_of_body(&value)?)?;
        let reference = checked.frame.ref_id.clone();
        let (rendered, _) = apply_wiki_mutation(&input, |doc, ledger| {
            put_frame(doc, ledger, checked.frame, args.expected_revision)
        })?;
        Ok((WikiDocument::parse(&rendered)?, reference))
    })();
    match simulated {
        Ok((document, reference)) => {
            if let Some(WikiObject::Frame(frame)) = document.object(&reference) {
                revision = Some(frame.revision);
                frames.push(frame_report(&document, frame, &mut errors)?);
            }
        }
        Err(error) => errors.push(jval!({
            "frame": draft_ref,
            "code": error.code(),
            "error": error.to_string(),
        })),
    }
    let draft = jval!({
        "ref": draft_ref,
        "would": if args.expected_revision.is_some() { "update" } else { "create" },
        "basis_revision": args.expected_revision,
        "revision": revision,
    });
    Ok(validation(&args.file, frames, errors, Some(draft)))
}

fn validation(
    file: &Path,
    frames: Vec<Value>,
    errors: Vec<Value>,
    draft: Option<Value>,
) -> WikiShapeOutcome {
    let exit_code = if errors.is_empty() {
        json::EXIT_OK
    } else {
        json::EXIT_GENERIC
    };
    let mut data = jval!({
        "command": "wiki-shape.validate",
        "file": file.display().to_string(),
        "valid": errors.is_empty(),
        "frames": frames,
        "errors": errors,
    });
    if let Some(draft) = draft {
        data["draft"] = draft;
    }
    WikiShapeOutcome::reported(data, Vec::new(), exit_code)
}

/// One frame as `validate` reports it: its revision, and each constellation
/// through the structural floor (its failures pushed onto `errors`).
fn frame_report(
    document: &WikiDocument,
    frame: &WikiFrame,
    errors: &mut Vec<Value>,
) -> Result<Value> {
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

        let anchor_stance = node_stance_of(document, &constellation.anchor_ref)?;
        let mut members = Vec::new();
        for member in &constellation.members {
            let stance = node_stance_of(document, &member.ref_id)?;
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
    Ok(jval!({
        "frame": frame.ref_id.to_string(),
        "revision": frame.revision,
        "constellations": constellations,
    }))
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
// target
// ---------------------------------------------------------------------------

/// Read-only: the Wiki's own projection of the constellation whose
/// whole-anchor is `anchor_ref`, as QL-MEF's Technē reading takes it (`ql
/// techne reading <target.json>`, a `WikiRefractionTarget`; the Wiki is QL's
/// `WikiSubjectSource`). The target is the constellation's frame, at its
/// revision, with a snapshot hash of the frame as the file holds it, its
/// provenance, and the QL shape binding the constellation keeps under
/// `ql-mef/shape-binding/v1` (carried as QL wrote it; QL validates it). A
/// constellation with no binding projects without one, and QL then discloses
/// no bounded whole for it.
fn target(args: &WikiShapeTargetArgs) -> Result<WikiShapeOutcome> {
    let input = read(&args.file)?;
    let document = WikiDocument::parse(&input)?;
    let anchor = ResourceRef::parse(&args.anchor_ref)?;
    let (frame, constellation) = document
        .objects()
        .iter()
        .filter_map(|object| match object {
            WikiObject::Frame(frame) => Some(frame),
            _ => None,
        })
        .find_map(|frame| {
            frame
                .constellations
                .iter()
                .find(|constellation| constellation.anchor_ref == anchor)
                .map(|constellation| (frame, constellation))
        })
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
    let snapshot = serde_json::to_vec(frame).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_shape_serialization_failed",
            format!("failed to serialize the frame: {error}"),
        )
    })?;
    let mut extensions = serde_json::Map::new();
    let binding = constellation.extensions.get(QL_SHAPE_BINDING_EXTENSION);
    if let Some(binding) = binding {
        extensions.insert(QL_SHAPE_BINDING_EXTENSION.into(), binding.clone());
    }
    let provenance = serde_json::to_value(&frame.provenance).unwrap_or_default();
    Ok(WikiShapeOutcome::reported(
        jval!({
            "command": "wiki-shape.target",
            "file": args.file.display().to_string(),
            "anchor": anchor.to_string(),
            "frame": frame.ref_id.to_string(),
            "revision": frame.revision,
            "shape_binding": binding.is_some(),
            "target": {
                "kind": "frame",
                "target_ref": frame.ref_id.to_string(),
                "target_revision": frame.revision,
                "target_snapshot_hash": format!("sha256:{}", content_hash(&snapshot)),
                "provenance": provenance,
                "subjects": [],
                "relations": [],
                "material": {},
                "extensions": extensions,
            },
        }),
        Vec::new(),
        json::EXIT_OK,
    ))
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

/// Every native Wiki writer delegates exact-basis, metadata-preserving
/// publication to the shared physical-file lock protocol.
fn persist(path: &Path, rendered: &str, base_hash: &str) -> Result<()> {
    aikit_adapters::projectcentral::publication::publish_wiki(path, rendered, base_hash)?;
    Ok(())
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
