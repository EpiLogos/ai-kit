//! Targeted facts on existing Wiki objects, without replacing their other
//! properties or requiring an otherwise unrelated construction. Persistence
//! uses the same native Wiki transaction owner as constructive actions.
use crate::knowledge_facets::{
    parse_facets_from_extensions, write_facets_to_extensions, PlaceFacet, TemporalFacet,
    TECHNE_FACET_EXTENSION,
};
use crate::knowledge_wiki_write::{apply_wiki_mutation, WikiDocument};
use crate::resource::ResourceRef;
use crate::{AikitError, Result, WikiObject};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ACTION: &str = "aikit.wiki-facts-action/v1";
pub const READING: &str = "aikit.wiki-facts/v1";
const RECEIPTS: &str = "aikit.wiki-facts-operations/v1";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Node { r#ref: ResourceRef },
    Whole { r#ref: ResourceRef },
}
impl Target {
    pub fn reference(&self) -> &ResourceRef {
        match self {
            Self::Node { r#ref } | Self::Whole { r#ref } => r#ref,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    TemporalSet { temporal: Vec<TemporalFacet> },
    PlaceSet { places: Vec<PlaceFacet> },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub target: Target,
    pub expected_revision: u64,
    pub actor_ref: ResourceRef,
    pub operation_ref: ResourceRef,
    pub changes: Vec<Change>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    actor_ref: ResourceRef,
    request_digest: String,
    basis_revision: u64,
    result_revision: u64,
}
#[derive(Debug, Serialize)]
pub struct Applied {
    pub target: Target,
    pub revision: u64,
    pub idempotent: bool,
    pub content: String,
    pub objects_changed: Vec<String>,
    pub warnings: Vec<String>,
}
fn err(message: impl Into<String>) -> AikitError {
    AikitError::new("knowledge.wiki_facts_refused", message)
}
fn conflict() -> AikitError {
    AikitError::new(
        "knowledge.wiki_facts_revision_conflict",
        "the exact native target changed; reread before editing its facts",
    )
}
pub(crate) fn require_writable(extensions: &BTreeMap<String, Value>) -> Result<()> {
    if extensions.get("read_only") == Some(&Value::Bool(true))
        || extensions.contains_key("shared_projection_ref")
    {
        return Err(err(
            "read-only shared material requires an explicit local derivative",
        ));
    }
    Ok(())
}
fn extensions<'a>(
    object: &'a mut WikiObject,
    target: &Target,
) -> Result<&'a mut BTreeMap<String, Value>> {
    match (object, target) {
        (WikiObject::Node(row), Target::Node { .. }) => Ok(&mut row.extensions),
        (WikiObject::Frame(row), Target::Whole { .. }) => Ok(&mut row.extensions),
        _ => Err(err(
            "target kind does not match the actual native Wiki object",
        )),
    }
}
/// Shared validation/replacement for native node, whole and participation
/// actions. Only the selected facet family changes; unrelated declarations
/// and extension keys retain their exact values.
pub(crate) fn replace(extensions: &mut BTreeMap<String, Value>, change: &Change) -> Result<()> {
    let mut facets = parse_facets_from_extensions(extensions)?;
    match change {
        Change::TemporalSet { temporal } => {
            if temporal.len() > 256
                || temporal
                    .iter()
                    .any(|f| f.source_ref.as_deref().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(err(
                    "at most 256 temporal facts, each with a native source basis, are required",
                ));
            }
            facets.temporal = temporal.clone();
        }
        Change::PlaceSet { places } => {
            if places.len() > 256
                || places
                    .iter()
                    .any(|f| f.source_ref.as_deref().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(err(
                    "at most 256 place facts, each with a native source basis, are required",
                ));
            }
            facets.spatial = places.clone();
        }
    }
    // Validate into a separate map first, so callers never receive a partial
    // in-memory mutation when native validation refuses.
    let mut next = extensions.clone();
    next.remove(TECHNE_FACET_EXTENSION);
    write_facets_to_extensions(&mut next, &facets)?;
    *extensions = next;
    Ok(())
}
pub fn inspect(input: &str, reference: &ResourceRef) -> Result<Value> {
    let doc = WikiDocument::parse(input)?;
    let object = doc
        .object(reference)
        .ok_or_else(|| err("native facts target is absent"))?;
    let (kind, value) = match object {
        WikiObject::Node(row) => ("node", serde_json::to_value(row)),
        WikiObject::Frame(row) => ("frame", serde_json::to_value(row)),
        _ => {
            return Err(err(
                "facts target must be an existing Wiki node or whole frame",
            ))
        }
    };
    let mut object = value.map_err(|e| err(e.to_string()))?;
    object["object"] = json!(kind);
    Ok(
        json!({"schema":READING,"object":object,"native_owner":"ai-kit","actions":["aikit.wiki.facts.apply"]}),
    )
}
pub fn apply(input: &str, request: &Request) -> Result<Applied> {
    for reference in [
        request.target.reference(),
        &request.actor_ref,
        &request.operation_ref,
    ] {
        ResourceRef::parse(reference.as_str())?;
    }
    if request.schema != ACTION
        || request.expected_revision == 0
        || request.expected_revision >= 9_007_199_254_740_991
        || request.changes.is_empty()
        || request.changes.len() > 2
    {
        return Err(err(
            "facts require an existing exact target revision and one change per facet family",
        ));
    }
    if request.changes.len() == 2
        && std::mem::discriminant(&request.changes[0])
            == std::mem::discriminant(&request.changes[1])
    {
        return Err(err(
            "a facet family may be replaced only once per operation",
        ));
    }
    let digest = blake3::hash(
        serde_json::to_string(request)
            .map_err(|e| err(e.to_string()))?
            .as_bytes(),
    )
    .to_hex()
    .to_string();
    let doc = WikiDocument::parse(input)?;
    let mut object = doc
        .object(request.target.reference())
        .cloned()
        .ok_or_else(|| err("native facts target is absent"))?;
    let revision = object.revision();
    let ext = extensions(&mut object, &request.target)?;
    require_writable(ext)?;
    let mut receipts: BTreeMap<String, Receipt> = ext
        .get(RECEIPTS)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| err(e.to_string())))
        .transpose()?
        .unwrap_or_default();
    if let Some(prior) = receipts.get(request.operation_ref.as_str()) {
        if prior.request_digest != digest || prior.actor_ref != request.actor_ref {
            return Err(err(
                "operation identity was reused with different content or attribution",
            ));
        }
        return Ok(Applied {
            target: request.target.clone(),
            revision,
            idempotent: true,
            content: input.into(),
            objects_changed: vec![],
            warnings: vec![],
        });
    }
    if revision != request.expected_revision {
        return Err(conflict());
    }
    if receipts.len() >= 4096 {
        return Err(err(
            "fact operation receipt budget reached; preserve history before continuing",
        ));
    }
    for change in &request.changes {
        replace(ext, change)?;
    }
    receipts.insert(
        request.operation_ref.to_string(),
        Receipt {
            actor_ref: request.actor_ref.clone(),
            request_digest: digest,
            basis_revision: revision,
            result_revision: revision + 1,
        },
    );
    ext.insert(
        RECEIPTS.into(),
        serde_json::to_value(receipts).map_err(|e| err(e.to_string()))?,
    );
    let (content, outcome) = apply_wiki_mutation(input, |doc, ledger| {
        if doc
            .object(request.target.reference())
            .is_none_or(|o| o.revision() != request.expected_revision)
        {
            return Err(conflict());
        }
        ledger.record(doc.update_object(object)?);
        Ok(())
    })?;
    Ok(Applied {
        target: request.target.clone(),
        revision: revision + 1,
        idempotent: false,
        content,
        objects_changed: outcome.touched.into_iter().map(|t| t.resource).collect(),
        warnings: outcome.warnings,
    })
}
