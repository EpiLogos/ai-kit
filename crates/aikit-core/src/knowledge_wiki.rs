//! Portable `okf-wiki/v1` semantic objects graduated from Glade.
//!
//! These types preserve the language-neutral identity/revision/provenance
//! semantics proven by Glade. Provider/index/database identifiers are explicitly
//! forbidden from defining canonical Wiki identity. Project ontology stays open:
//! node `type` values and unknown extension fields are preserved rather than
//! translated into AIKit-specific kinds.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::resource::{ResourceRef, SourceRef};
use crate::{AikitError, Result};

pub const OKF_WIKI_PROFILE: &str = "okf-wiki/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SemanticRevision {
    Number(u64),
    Text(String),
}

impl SemanticRevision {
    fn validate(&self, field: &str) -> Result<()> {
        match self {
            Self::Number(_) => Ok(()),
            Self::Text(value) if !value.trim().is_empty() => Ok(()),
            Self::Text(_) => Err(AikitError::new(
                "knowledge.wiki_invalid_revision",
                format!("{field} must be non-empty when textual"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiProvenanceRef {
    pub source_ref: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SemanticRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_ref: Option<ResourceRef>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

impl WikiProvenanceRef {
    fn validate(&self) -> Result<()> {
        if let Some(revision) = &self.source_revision {
            revision.validate("provenance.source_revision")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WikiEdgeOrigin {
    #[serde(rename = "authored")]
    Authored,
    #[serde(rename = "mechanical")]
    Mechanical,
    #[serde(rename = "compiled")]
    Compiled,
    #[serde(rename = "inferred")]
    Inferred,
    #[serde(rename = "learned")]
    Learned,
    #[serde(rename = "QL-derived")]
    QlDerived,
    #[serde(rename = "MEF-derived")]
    MefDerived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WikiSurfaceKind {
    Wiki,
    Source,
    Code,
    Run,
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiConstellationMember {
    #[serde(rename = "ref")]
    pub ref_id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u8>,
    #[serde(default)]
    pub conjugate: bool,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiConstellationReturn {
    pub through_anchor_ref: ResourceRef,
    pub ground_ref: ResourceRef,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiConstellation {
    pub anchor_ref: ResourceRef,
    #[serde(default)]
    pub members: Vec<WikiConstellationMember>,
    #[serde(default, rename = "returns")]
    pub returns: Vec<WikiConstellationReturn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conjugate_ref: Option<ResourceRef>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

impl WikiConstellation {
    pub fn validate(&self) -> Result<()> {
        let mut positions = BTreeSet::new();
        for member in &self.members {
            if member.position.is_some_and(|position| position > 5) {
                return Err(AikitError::new(
                    "knowledge.wiki_invalid_constellation",
                    "constellation member position must be 0..=5 when present",
                )
                .with("member", member.ref_id.to_string()));
            }
            if let Some(position) = member.position {
                if !positions.insert((position, member.conjugate)) {
                    return Err(AikitError::new(
                        "knowledge.wiki_invalid_constellation",
                        "constellation positions must be unique per conjugate face",
                    ));
                }
            }
        }
        for return_path in &self.returns {
            if return_path.through_anchor_ref != self.anchor_ref {
                return Err(AikitError::new(
                    "knowledge.wiki_invalid_constellation",
                    "constellation return must route through its own anchor",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiSpace {
    #[serde(default = "wiki_profile")]
    pub profile: String,
    #[serde(rename = "ref")]
    pub ref_id: ResourceRef,
    #[serde(default = "initial_revision")]
    pub revision: u64,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub parent_space_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub child_space_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub node_refs: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_ref: Option<ResourceRef>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiNode {
    #[serde(default = "wiki_profile")]
    pub profile: String,
    #[serde(rename = "ref")]
    pub ref_id: ResourceRef,
    #[serde(default = "initial_revision")]
    pub revision: u64,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub space_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub source_refs: Vec<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_space_ref: Option<ResourceRef>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiEdge {
    #[serde(default = "wiki_profile")]
    pub profile: String,
    #[serde(rename = "ref")]
    pub ref_id: ResourceRef,
    #[serde(default = "initial_revision")]
    pub revision: u64,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
    pub from_ref: ResourceRef,
    pub to_ref: ResourceRef,
    pub relation: String,
    pub origin: WikiEdgeOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_ref: Option<ResourceRef>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiFrame {
    #[serde(default = "wiki_profile")]
    pub profile: String,
    #[serde(rename = "ref")]
    pub ref_id: ResourceRef,
    #[serde(default = "initial_revision")]
    pub revision: u64,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry_ref: Option<ResourceRef>,
    #[serde(default)]
    pub space_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub member_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub external_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub constellations: Vec<WikiConstellation>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiReading {
    #[serde(default = "wiki_profile")]
    pub profile: String,
    #[serde(rename = "ref")]
    pub ref_id: ResourceRef,
    #[serde(default = "initial_revision")]
    pub revision: u64,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
    pub frame_ref: ResourceRef,
    pub reading_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_by_ref: Option<ResourceRef>,
    #[serde(flatten, default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WikiObject {
    Space(WikiSpace),
    Node(WikiNode),
    Edge(WikiEdge),
    Frame(WikiFrame),
    Reading(WikiReading),
}

impl WikiObject {
    pub fn parse(value: &Value) -> Result<Self> {
        let object = value.as_object().ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_object",
                "OKF Wiki object must be a JSON/YAML mapping",
            )
        })?;
        require_profile(object)?;
        reject_provider_identity(object)?;
        let kind = object
            .get("object")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "knowledge.wiki_invalid_object",
                    "OKF Wiki object requires an `object` discriminator",
                )
            })?;
        let mut payload = value.clone();
        payload
            .as_object_mut()
            .expect("validated mapping")
            .remove("object");
        let parsed = match kind {
            "space" => Self::Space(from_value(payload, "space")?),
            "node" => Self::Node(from_value(payload, "node")?),
            "edge" => Self::Edge(from_value(payload, "edge")?),
            "frame" => Self::Frame(from_value(payload, "frame")?),
            "reading" => Self::Reading(from_value(payload, "reading")?),
            other => {
                return Err(AikitError::new(
                    "knowledge.wiki_unknown_object",
                    format!("unsupported okf-wiki/v1 object `{other}`"),
                ))
            }
        };
        parsed.validate()?;
        Ok(parsed)
    }

    pub fn ref_id(&self) -> &ResourceRef {
        match self {
            Self::Space(value) => &value.ref_id,
            Self::Node(value) => &value.ref_id,
            Self::Edge(value) => &value.ref_id,
            Self::Frame(value) => &value.ref_id,
            Self::Reading(value) => &value.ref_id,
        }
    }

    pub fn revision(&self) -> u64 {
        match self {
            Self::Space(value) => value.revision,
            Self::Node(value) => value.revision,
            Self::Edge(value) => value.revision,
            Self::Frame(value) => value.revision,
            Self::Reading(value) => value.revision,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.revision() < 1 {
            return Err(AikitError::new(
                "knowledge.wiki_invalid_revision",
                "Wiki object revision must be >= 1",
            )
            .with("ref", self.ref_id().to_string()));
        }
        let (profile, provenance, extensions) = match self {
            Self::Space(value) => (&value.profile, &value.provenance, &value.extensions),
            Self::Node(value) => (&value.profile, &value.provenance, &value.extensions),
            Self::Edge(value) => (&value.profile, &value.provenance, &value.extensions),
            Self::Frame(value) => (&value.profile, &value.provenance, &value.extensions),
            Self::Reading(value) => (&value.profile, &value.provenance, &value.extensions),
        };
        validate_profile(profile, extensions)?;
        for entry in provenance {
            entry.validate()?;
        }
        match self {
            Self::Node(value) if value.node_type.trim().is_empty() => Err(AikitError::new(
                "knowledge.wiki_invalid_node",
                "WikiNode type cannot be empty",
            )),
            Self::Edge(value) if value.relation.trim().is_empty() => Err(AikitError::new(
                "knowledge.wiki_invalid_edge",
                "WikiEdge relation cannot be empty",
            )),
            Self::Frame(value) => {
                for constellation in &value.constellations {
                    constellation.validate()?;
                }
                Ok(())
            }
            Self::Reading(value) if value.reading_type.trim().is_empty() => Err(AikitError::new(
                "knowledge.wiki_invalid_reading",
                "WikiReading reading_type cannot be empty",
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OkfWikiBundle {
    pub okf: Value,
    pub wiki: WikiObject,
    pub extensions: BTreeMap<String, Value>,
}

impl OkfWikiBundle {
    pub fn parse_json(input: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(input).map_err(|error| {
            AikitError::new(
                "knowledge.wiki_invalid_json",
                format!("invalid OKF Wiki JSON: {error}"),
            )
        })?;
        let mut object = value.as_object().cloned().ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_bundle",
                "OKF Wiki fixture must be a JSON object",
            )
        })?;
        let okf = object.remove("okf").unwrap_or(Value::Null);
        let wiki_value = object.remove("wiki").ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_bundle",
                "OKF Wiki bundle is missing `wiki`",
            )
        })?;
        let wiki = WikiObject::parse(&wiki_value)?;
        Ok(Self {
            okf,
            wiki,
            extensions: object.into_iter().collect(),
        })
    }
}

pub fn parse_wiki_objects(input: &str) -> Result<Vec<WikiObject>> {
    let value: Value = serde_json::from_str(input).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_invalid_json",
            format!("invalid OKF Wiki JSON: {error}"),
        )
    })?;
    let objects = value
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_bundle",
                "Wiki object collection requires an `objects` list",
            )
        })?;
    objects.iter().map(WikiObject::parse).collect()
}

fn wiki_profile() -> String {
    OKF_WIKI_PROFILE.to_string()
}

fn initial_revision() -> u64 {
    1
}

fn require_profile(object: &serde_json::Map<String, Value>) -> Result<()> {
    let profile = object
        .get("profile")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_profile_unsupported",
                format!("Wiki object must declare profile `{OKF_WIKI_PROFILE}`"),
            )
        })?;
    if profile == OKF_WIKI_PROFILE {
        Ok(())
    } else {
        Err(AikitError::new(
            "knowledge.wiki_profile_unsupported",
            format!("unsupported Wiki profile `{profile}`; expected {OKF_WIKI_PROFILE}"),
        ))
    }
}

fn reject_provider_identity(object: &serde_json::Map<String, Value>) -> Result<()> {
    const FORBIDDEN: [&str; 6] = [
        "provider_id",
        "providerId",
        "row_id",
        "rowId",
        "db_id",
        "dbId",
    ];
    if let Some(key) = FORBIDDEN.into_iter().find(|key| object.contains_key(*key)) {
        return Err(AikitError::new(
            "knowledge.wiki_provider_identity_forbidden",
            format!("provider/index field `{key}` cannot define canonical Wiki identity"),
        ));
    }
    Ok(())
}

fn validate_profile(profile: &str, extensions: &BTreeMap<String, Value>) -> Result<()> {
    if profile != OKF_WIKI_PROFILE {
        return Err(AikitError::new(
            "knowledge.wiki_profile_unsupported",
            format!("unsupported Wiki profile `{profile}`"),
        ));
    }
    const FORBIDDEN: [&str; 6] = [
        "provider_id",
        "providerId",
        "row_id",
        "rowId",
        "db_id",
        "dbId",
    ];
    if let Some(key) = FORBIDDEN
        .into_iter()
        .find(|key| extensions.contains_key(*key))
    {
        return Err(AikitError::new(
            "knowledge.wiki_provider_identity_forbidden",
            format!("provider/index field `{key}` cannot define canonical Wiki identity"),
        ));
    }
    Ok(())
}

fn from_value<T: for<'de> Deserialize<'de>>(value: Value, kind: &str) -> Result<T> {
    serde_json::from_value(value).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_invalid_object",
            format!("invalid Wiki {kind}: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_non_glade_node_preserves_unknown_extensions() {
        let fixture = r#"{
          "okf": {"type":"Research Note","producer_extension":{"kept":true}},
          "wiki": {
            "profile":"okf-wiki/v1","object":"node",
            "ref":"example:knowledge:portable-node","revision":3,
            "provenance":[{"source_ref":"example:source:paper","source_revision":"sha256:abc"}],
            "type":"Research Note","title":"A portable node",
            "space_refs":["example:space:research"],"source_refs":["example:source:paper"],
            "producer_extension":{"unknown":"preserve me"}
          }
        }"#;
        let bundle = OkfWikiBundle::parse_json(fixture).unwrap();
        let WikiObject::Node(node) = bundle.wiki else {
            panic!("expected node")
        };
        assert_eq!(node.ref_id.as_str(), "example:knowledge:portable-node");
        assert_eq!(node.revision, 3);
        assert_eq!(
            node.extensions["producer_extension"]["unknown"],
            Value::String("preserve me".into())
        );
    }

    #[test]
    fn provider_specific_identity_is_rejected() {
        let fixture = r#"{
          "profile":"okf-wiki/v1","object":"node","ref":"wiki:node:canonical",
          "revision":1,"provenance":[],"type":"Concept","space_refs":[],
          "source_refs":[],"provider_id":"sqlite-row-42"
        }"#;
        let error = WikiObject::parse(&serde_json::from_str(fixture).unwrap()).unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_provider_identity_forbidden");
    }

    #[test]
    fn shared_nested_cross_space_shape_preserves_authority_and_anchor_returns() {
        let fixture = r#"{
          "objects":[
            {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":1,
             "provenance":[],"title":"Root","parent_space_refs":[],
             "child_space_refs":["wiki:space:child"],"node_refs":["wiki:node:a"],"anchor_ref":"wiki:node:a"},
            {"profile":"okf-wiki/v1","object":"edge","ref":"wiki:edge:a-b","revision":2,
             "provenance":[{"source_ref":"wiki:run:compile-7","source_revision":2}],
             "from_ref":"wiki:node:a","to_ref":"wiki:node:b","relation":"develops",
             "origin":"inferred","origin_ref":"wiki:run:compile-7"},
            {"profile":"okf-wiki/v1","object":"frame","ref":"wiki:frame:cross-space","revision":1,
             "provenance":[],"inquiry_ref":"prompt:compare-a-b","space_refs":["wiki:space:root"],
             "member_refs":["wiki:node:a","wiki:node:b"],"external_refs":["source:paper:17"],
             "constellations":[{"anchor_ref":"wiki:node:a","members":[{"ref":"wiki:node:a","position":0},
             {"ref":"wiki:node:b","position":3,"conjugate":true}],
             "returns":[{"through_anchor_ref":"wiki:node:a","ground_ref":"wiki:space:root"}]}]}
          ]
        }"#;
        let objects = parse_wiki_objects(fixture).unwrap();
        assert_eq!(objects.len(), 3);
        let WikiObject::Edge(edge) = &objects[1] else {
            panic!("expected edge")
        };
        assert_eq!(edge.origin, WikiEdgeOrigin::Inferred);
        assert!(matches!(
            edge.provenance[0].source_revision,
            Some(SemanticRevision::Number(2))
        ));
        let WikiObject::Frame(frame) = &objects[2] else {
            panic!("expected frame")
        };
        frame.constellations[0].validate().unwrap();
        assert_eq!(
            frame.constellations[0].returns[0].ground_ref.as_str(),
            "wiki:space:root"
        );
    }
}
