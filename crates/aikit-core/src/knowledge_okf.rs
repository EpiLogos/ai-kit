//! Open Knowledge Format v0.2 data model and validation.
//!
//! OKF is intentionally open: `type` is the only universally required field and
//! unknown producer keys/types are data to preserve, not schema errors. Text/YAML
//! codecs live in `aikit-adapters`; core owns only the validated, I/O-free model.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::knowledge_wiki::{SemanticRevision, WikiEdge, WikiEdgeOrigin, WikiProvenanceRef};
use crate::resource::{ResourceRef, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const OKF_VERSION: &str = "0.2";
pub const AUTHORED_RELATION_PROFILE: &str = "aikit.authored-relation/v1";
pub const AUTHORED_REFERENCE_RELATION: &str = "references";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkfDocument {
    /// Complete frontmatter mapping, including keys AIKit does not understand.
    pub metadata: Map<String, Value>,
    pub body: String,
}

impl OkfDocument {
    pub fn new(metadata: Map<String, Value>, body: impl Into<String>) -> Result<Self> {
        validate_okf(&metadata)?;
        Ok(Self {
            metadata,
            body: body.into(),
        })
    }

    pub fn object_type(&self) -> &str {
        self.metadata
            .get("type")
            .and_then(Value::as_str)
            .expect("validated OKF documents always have a string type")
    }

    pub fn title(&self) -> Option<&str> {
        self.metadata.get("title").and_then(Value::as_str)
    }

    pub fn sources(&self) -> &[Value] {
        self.metadata
            .get("sources")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Which authored source channel made a relation explicit.
///
/// Body links and metadata properties are both source language. Keeping their
/// channels distinct lets Explain/History retain the exact authored evidence
/// even when navigation later coalesces several observations into one edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthoredRelationChannel {
    Body,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredRelationAnchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_byte: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_byte: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
}

impl AuthoredRelationAnchor {
    pub fn body(start_byte: usize, end_byte: usize) -> Self {
        Self {
            start_byte: Some(start_byte),
            end_byte: Some(end_byte),
            field_path: None,
        }
    }

    pub fn metadata(field_path: impl Into<String>) -> Self {
        Self {
            start_byte: None,
            end_byte: None,
            field_path: Some(field_path.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AuthoredRelationResolution {
    Unresolved,
    Ambiguous { candidate_refs: Vec<ResourceRef> },
    Resolved { target_ref: ResourceRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredRelationEvidence {
    pub profile: String,
    pub source_ref: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
    pub relation: String,
    pub raw_target: String,
    pub raw_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<String>,
    pub channel: AuthoredRelationChannel,
    pub anchor: AuthoredRelationAnchor,
    pub resolution: AuthoredRelationResolution,
}

impl AuthoredRelationEvidence {
    pub fn body_reference(
        source_ref: SourceRef,
        source_revision: Option<SourceRevision>,
        raw_target: impl Into<String>,
        raw_token: impl Into<String>,
        display: Option<String>,
        fragment: Option<String>,
        anchor: AuthoredRelationAnchor,
    ) -> Self {
        Self {
            profile: AUTHORED_RELATION_PROFILE.to_string(),
            source_ref,
            source_revision,
            relation: AUTHORED_REFERENCE_RELATION.to_string(),
            raw_target: raw_target.into(),
            raw_token: raw_token.into(),
            display,
            fragment,
            channel: AuthoredRelationChannel::Body,
            anchor,
            resolution: AuthoredRelationResolution::Unresolved,
        }
    }
}

/// One deterministic target candidate supplied by an existing owner/index.
///
/// `locators` are provider-known paths/addresses and do not become canonical
/// identity. `ref_id` remains the stable semantic identity returned on success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredRelationCandidate {
    pub ref_id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub locators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkfWikiSourceProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<SourceRef>,
    #[serde(default)]
    pub relations: Vec<AuthoredRelationEvidence>,
}

/// Interpret optional open `okf-wiki` source properties carried by an OKF
/// document. This does not change universal OKF v0.2 validation: aliases and
/// typed relations are profile-level extensions and unknown fields remain intact
/// on `OkfDocument.metadata`.
pub fn okf_wiki_source_profile(
    document: &OkfDocument,
    source_ref: &SourceRef,
    source_revision: Option<&SourceRevision>,
) -> Result<OkfWikiSourceProfile> {
    let resource_ref = document
        .metadata
        .get("resource")
        .and_then(Value::as_str)
        .map(ResourceRef::parse)
        .transpose()?;
    let title = document.title().map(ToOwned::to_owned);
    let aliases = string_list_property(document.metadata.get("aliases"), "aliases")?;

    let source_refs = document
        .sources()
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let resource = value
                .as_object()
                .and_then(|source| source.get("resource"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AikitError::new(
                        "knowledge.okf_wiki_invalid_source",
                        format!("sources[{index}].resource must be a string"),
                    )
                })?;
            SourceRef::parse(resource)
        })
        .collect::<Result<Vec<_>>>()?;

    let mut relations = Vec::new();
    if let Some(value) = document.metadata.get("relations") {
        let relation_map = value.as_object().ok_or_else(|| {
            AikitError::new(
                "knowledge.okf_wiki_invalid_relations",
                "relations must be a mapping from relation name to target(s)",
            )
        })?;
        for (relation, targets) in relation_map {
            if relation.trim().is_empty() {
                return Err(AikitError::new(
                    "knowledge.okf_wiki_invalid_relations",
                    "relation names cannot be empty",
                ));
            }
            match targets {
                Value::Array(values) => {
                    for (index, target) in values.iter().enumerate() {
                        relations.push(metadata_relation(
                            source_ref,
                            source_revision,
                            relation,
                            target,
                            format!("relations.{relation}[{index}]"),
                        )?);
                    }
                }
                other => relations.push(metadata_relation(
                    source_ref,
                    source_revision,
                    relation,
                    other,
                    format!("relations.{relation}"),
                )?),
            }
        }
    }

    Ok(OkfWikiSourceProfile {
        resource_ref,
        title,
        aliases,
        source_refs,
        relations,
    })
}

fn string_list_property(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::String(value) if !value.trim().is_empty() => Ok(vec![value.clone()]),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        AikitError::new(
                            "knowledge.okf_wiki_invalid_property",
                            format!("{field}[{index}] must be a non-empty string"),
                        )
                    })
            })
            .collect(),
        _ => Err(AikitError::new(
            "knowledge.okf_wiki_invalid_property",
            format!("{field} must be a string or list of strings"),
        )),
    }
}

fn metadata_relation(
    source_ref: &SourceRef,
    source_revision: Option<&SourceRevision>,
    relation: &str,
    target: &Value,
    field_path: String,
) -> Result<AuthoredRelationEvidence> {
    let (raw_target, display, fragment, resolution) = match target {
        Value::String(raw_target) if !raw_target.trim().is_empty() => {
            let (target, fragment) = split_fragment(raw_target);
            (
                target.to_string(),
                None,
                fragment.map(ToOwned::to_owned),
                AuthoredRelationResolution::Unresolved,
            )
        }
        Value::Object(object) => {
            let explicit_ref = object.get("resource").and_then(Value::as_str);
            let raw_target = object
                .get("target")
                .and_then(Value::as_str)
                .or(explicit_ref)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    AikitError::new(
                        "knowledge.okf_wiki_invalid_relation_target",
                        format!("{field_path} requires `target` or `resource`"),
                    )
                })?;
            let (target, inline_fragment) = split_fragment(raw_target);
            let fragment = object
                .get("fragment")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| inline_fragment.map(ToOwned::to_owned));
            let display = object
                .get("display")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let resolution = explicit_ref
                .map(ResourceRef::parse)
                .transpose()?
                .map(|target_ref| AuthoredRelationResolution::Resolved { target_ref })
                .unwrap_or(AuthoredRelationResolution::Unresolved);
            (target.to_string(), display, fragment, resolution)
        }
        _ => {
            return Err(AikitError::new(
                "knowledge.okf_wiki_invalid_relation_target",
                format!("{field_path} must be a non-empty string or mapping"),
            ))
        }
    };

    Ok(AuthoredRelationEvidence {
        profile: AUTHORED_RELATION_PROFILE.to_string(),
        source_ref: source_ref.clone(),
        source_revision: source_revision.cloned(),
        relation: relation.to_string(),
        raw_target,
        raw_token: target.to_string(),
        display,
        fragment,
        channel: AuthoredRelationChannel::Metadata,
        anchor: AuthoredRelationAnchor::metadata(field_path),
        resolution,
    })
}

fn split_fragment(raw: &str) -> (&str, Option<&str>) {
    match raw.split_once('#') {
        Some((target, fragment)) => (target, Some(fragment)),
        None => (raw, None),
    }
}

/// Resolve one authored target against candidates supplied by the current
/// owner/index. Explicitly resolved targets remain resolved. Ambiguity is a
/// first-class result and is never collapsed to an arbitrary candidate.
pub fn resolve_authored_relation(
    evidence: &AuthoredRelationEvidence,
    candidates: &[AuthoredRelationCandidate],
) -> AuthoredRelationEvidence {
    if matches!(
        evidence.resolution,
        AuthoredRelationResolution::Resolved { .. }
    ) {
        return evidence.clone();
    }

    let target = normalize_authored_target(&evidence.raw_target);
    let mut matches = BTreeSet::new();
    for candidate in candidates {
        if normalize_authored_target(candidate.ref_id.as_str()) == target
            || candidate
                .title
                .as_deref()
                .is_some_and(|value| normalize_authored_target(value) == target)
            || candidate
                .aliases
                .iter()
                .any(|value| normalize_authored_target(value) == target)
            || candidate
                .locators
                .iter()
                .any(|value| normalize_authored_target(value) == target)
        {
            matches.insert(candidate.ref_id.clone());
        }
    }

    let resolution = match matches.len() {
        0 => AuthoredRelationResolution::Unresolved,
        1 => AuthoredRelationResolution::Resolved {
            target_ref: matches.into_iter().next().expect("one match"),
        },
        _ => AuthoredRelationResolution::Ambiguous {
            candidate_refs: matches.into_iter().collect(),
        },
    };

    let mut resolved = evidence.clone();
    resolved.resolution = resolution;
    resolved
}

fn normalize_authored_target(raw: &str) -> String {
    let raw = raw.trim().replace('\\', "/");
    let raw = raw.strip_prefix("./").unwrap_or(&raw);
    raw.strip_suffix(".md").unwrap_or(raw).to_string()
}

/// Materialise a resolved authored relation into the existing `okf-wiki/v1`
/// edge model. The caller supplies canonical edge identity; this function owns
/// authority/provenance projection only.
pub fn materialize_authored_wiki_edge(
    edge_ref: ResourceRef,
    from_ref: ResourceRef,
    evidence: &AuthoredRelationEvidence,
) -> Result<WikiEdge> {
    let target_ref = match &evidence.resolution {
        AuthoredRelationResolution::Resolved { target_ref } => target_ref.clone(),
        _ => {
            return Err(AikitError::new(
                "knowledge.authored_relation_unresolved",
                "cannot materialize an unresolved or ambiguous authored relation",
            ))
        }
    };

    let mut extensions = std::collections::BTreeMap::new();
    extensions.insert(
        "authored_relation".to_string(),
        serde_json::to_value(evidence).map_err(|error| {
            AikitError::new(
                "knowledge.authored_relation_serialize",
                format!("could not serialize authored relation evidence: {error}"),
            )
        })?,
    );

    let provenance_extensions = std::collections::BTreeMap::from([
        (
            "channel".to_string(),
            serde_json::to_value(evidence.channel).map_err(|error| {
                AikitError::new(
                    "knowledge.authored_relation_serialize",
                    format!("could not serialize authored relation channel: {error}"),
                )
            })?,
        ),
        (
            "anchor".to_string(),
            serde_json::to_value(&evidence.anchor).map_err(|error| {
                AikitError::new(
                    "knowledge.authored_relation_serialize",
                    format!("could not serialize authored relation anchor: {error}"),
                )
            })?,
        ),
        (
            "raw_target".to_string(),
            Value::String(evidence.raw_target.clone()),
        ),
    ]);

    Ok(WikiEdge {
        profile: crate::knowledge_wiki::OKF_WIKI_PROFILE.to_string(),
        ref_id: edge_ref,
        revision: 1,
        provenance: vec![WikiProvenanceRef {
            source_ref: evidence.source_ref.clone(),
            source_revision: evidence
                .source_revision
                .as_ref()
                .map(|revision| SemanticRevision::Text(revision.to_string())),
            producer_ref: None,
            generation_ref: None,
            extensions: provenance_extensions,
        }],
        from_ref,
        to_ref: target_ref,
        relation: evidence.relation.clone(),
        origin: WikiEdgeOrigin::Authored,
        origin_ref: None,
        extensions,
    })
}

pub fn validate_okf(metadata: &Map<String, Value>) -> Result<()> {
    let mut findings = Vec::new();
    match metadata.get("type").and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => {}
        _ => findings.push("type: required non-empty string (OKF v0.2)".to_string()),
    }

    for key in ["title", "description", "resource"] {
        if metadata.get(key).is_some_and(|value| !value.is_string()) {
            findings.push(format!("{key}: must be a string when present"));
        }
    }

    if let Some(tags) = metadata.get("tags") {
        match tags.as_array() {
            Some(values) if values.iter().all(Value::is_string) => {}
            _ => findings.push("tags: must be a list of strings".to_string()),
        }
    }

    if let Some(status) = metadata.get("status") {
        let valid = status
            .as_str()
            .is_some_and(|value| matches!(value, "draft" | "stable" | "deprecated"));
        if !valid {
            findings
                .push("status: must be one of draft, stable, deprecated (OKF v0.2)".to_string());
        }
    }

    if let Some(sources) = metadata.get("sources") {
        match sources.as_array() {
            None => findings.push("sources: must be a list".to_string()),
            Some(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    let Some(source) = entry.as_object() else {
                        findings.push(format!("sources[{index}]: must be a mapping"));
                        continue;
                    };
                    let resource_ok = source
                        .get("resource")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty());
                    if !resource_ok {
                        findings.push(format!(
                            "sources[{index}].resource: required non-empty string"
                        ));
                    }
                    if source.get("id").is_some_and(|value| !value.is_string()) {
                        findings.push(format!("sources[{index}].id: must be a string"));
                    }
                    if source
                        .get("usage_count")
                        .is_some_and(|value| value.as_u64().is_none())
                    {
                        findings.push(format!(
                            "sources[{index}].usage_count: must be a non-negative integer"
                        ));
                    }
                }
            }
        }
    }

    if findings.is_empty() {
        Ok(())
    } else {
        Err(AikitError::new(
            "knowledge.okf_invalid",
            findings.join("; "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_ref() -> SourceRef {
        SourceRef::parse("source:wiki:flow").unwrap()
    }

    #[test]
    fn unknown_types_and_extensions_are_valid_core_data() {
        let metadata = serde_json::json!({
            "type": "Future Knowledge Object",
            "title": "Portable",
            "producer_extension": {"nested": ["one", "two"], "future_flag": true}
        })
        .as_object()
        .unwrap()
        .clone();
        let parsed = OkfDocument::new(metadata.clone(), "# Body\n").unwrap();
        assert_eq!(parsed.object_type(), "Future Knowledge Object");
        assert_eq!(parsed.metadata, metadata);
        assert_eq!(
            parsed.metadata["producer_extension"]["future_flag"],
            Value::Bool(true)
        );
    }

    #[test]
    fn okf_floor_rejects_missing_type_but_not_unknown_fields() {
        let metadata = serde_json::json!({"title":"No type", "new_field":42})
            .as_object()
            .unwrap()
            .clone();
        let error = OkfDocument::new(metadata, "body").unwrap_err();
        assert_eq!(error.code(), "knowledge.okf_invalid");
    }

    #[test]
    fn wiki_profile_interprets_aliases_sources_and_open_typed_relations() {
        let metadata = serde_json::json!({
            "type": "Concept",
            "title": "Flow",
            "resource": "wiki:concept:flow",
            "aliases": ["Live Flow", "Linguistic Flow"],
            "relations": {
                "develops": ["Living Wiki"],
                "depends-on": [{"resource":"wiki:concept:change-horizon","display":"Change Horizon"}]
            },
            "sources": [{"resource":"source:oi-flow-design"}],
            "producer_extension": {"untouched": true}
        })
        .as_object()
        .unwrap()
        .clone();
        let document = OkfDocument::new(metadata.clone(), "[[Living Wiki]]\n").unwrap();
        let revision = SourceRevision::parse("rev-7").unwrap();
        let profile = okf_wiki_source_profile(&document, &source_ref(), Some(&revision)).unwrap();
        assert_eq!(profile.resource_ref.unwrap().as_str(), "wiki:concept:flow");
        assert_eq!(profile.aliases, vec!["Live Flow", "Linguistic Flow"]);
        assert_eq!(profile.source_refs[0].as_str(), "source:oi-flow-design");
        assert_eq!(profile.relations.len(), 2);
        let depends_on = profile
            .relations
            .iter()
            .find(|relation| relation.relation == "depends-on")
            .unwrap();
        assert!(matches!(
            depends_on.resolution,
            AuthoredRelationResolution::Resolved { .. }
        ));
        let develops = profile
            .relations
            .iter()
            .find(|relation| relation.relation == "develops")
            .unwrap();
        assert_eq!(develops.raw_target, "Living Wiki");
        assert_eq!(document.metadata, metadata);
    }

    #[test]
    fn resolver_returns_resolved_ambiguous_and_unresolved_truthfully() {
        let evidence = AuthoredRelationEvidence::body_reference(
            source_ref(),
            None,
            "knowledge/Flow.md",
            "[[knowledge/Flow.md]]",
            None,
            None,
            AuthoredRelationAnchor::body(0, 21),
        );
        let candidate = AuthoredRelationCandidate {
            ref_id: ResourceRef::parse("wiki:node:flow").unwrap(),
            title: Some("Flow".into()),
            aliases: vec!["Live Flow".into()],
            locators: vec!["knowledge/Flow.md".into()],
        };
        let resolved = resolve_authored_relation(&evidence, std::slice::from_ref(&candidate));
        assert!(matches!(
            resolved.resolution,
            AuthoredRelationResolution::Resolved { .. }
        ));

        let second = AuthoredRelationCandidate {
            ref_id: ResourceRef::parse("wiki:node:other-flow").unwrap(),
            title: None,
            aliases: vec![],
            locators: vec!["knowledge/Flow".into()],
        };
        let ambiguous = resolve_authored_relation(&evidence, &[candidate, second]);
        assert!(matches!(
            ambiguous.resolution,
            AuthoredRelationResolution::Ambiguous { .. }
        ));

        let missing = AuthoredRelationEvidence::body_reference(
            source_ref(),
            None,
            "Future Concept",
            "[[Future Concept]]",
            None,
            None,
            AuthoredRelationAnchor::body(0, 18),
        );
        assert!(matches!(
            resolve_authored_relation(&missing, &[]).resolution,
            AuthoredRelationResolution::Unresolved
        ));
    }

    #[test]
    fn resolved_authored_relation_materialises_as_authored_wiki_edge() {
        let evidence = AuthoredRelationEvidence {
            profile: AUTHORED_RELATION_PROFILE.to_string(),
            source_ref: source_ref(),
            source_revision: Some(SourceRevision::parse("rev-8").unwrap()),
            relation: "develops".into(),
            raw_target: "Living Wiki".into(),
            raw_token: "\"Living Wiki\"".into(),
            display: None,
            fragment: None,
            channel: AuthoredRelationChannel::Metadata,
            anchor: AuthoredRelationAnchor::metadata("relations.develops[0]"),
            resolution: AuthoredRelationResolution::Resolved {
                target_ref: ResourceRef::parse("wiki:node:living-wiki").unwrap(),
            },
        };
        let edge = materialize_authored_wiki_edge(
            ResourceRef::parse("wiki:edge:flow-living-wiki").unwrap(),
            ResourceRef::parse("wiki:node:flow").unwrap(),
            &evidence,
        )
        .unwrap();
        assert_eq!(edge.origin, WikiEdgeOrigin::Authored);
        assert_eq!(edge.relation, "develops");
        assert_eq!(edge.to_ref.as_str(), "wiki:node:living-wiki");
        assert_eq!(edge.provenance[0].source_ref, source_ref());
        assert_eq!(
            edge.provenance[0].extensions["channel"],
            serde_json::json!("metadata")
        );
    }
}
