use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::knowledge::{
    KnowledgeReading, KnowledgeRelationView, RelationDirection, RelationEdge, RelationNode,
    RelationOrigin, RelationQuery,
};
use crate::knowledge_wiki::{WikiEdgeOrigin, WikiFrame, WikiObject, WikiProvenanceRef};
use crate::knowledge_wiki_index::{
    SemanticWikiIndex, WikiIndexStatus, WikiNeighbour, WikiRelationDirection, WikiSearchHit,
};
use crate::resource::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority, SourceRef};
use crate::{AikitError, Result};

pub const NATIVE_SEMANTIC_WIKI_PROVIDER: &str = "provider/semantic-wiki/native";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticWikiProviderStatus {
    pub provider: ProviderRef,
    pub available: bool,
    pub index: WikiIndexStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiExplanation {
    pub resource: ResourceRef,
    pub provider: ProviderRef,
    pub object_kind: String,
    pub revision: u64,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
    #[serde(default)]
    pub relations: Vec<WikiNeighbour>,
}

/// Native application surface over the rebuildable SemanticWiki index.
///
/// The index remains derived state and relation names remain Wiki vocabulary.
/// This provider only projects those native semantics into AIKit's common
/// Knowledge application contracts.
pub struct SemanticWikiProvider<'a> {
    index: &'a SemanticWikiIndex,
    provider: ProviderRef,
}

impl<'a> SemanticWikiProvider<'a> {
    pub fn new(index: &'a SemanticWikiIndex) -> Self {
        Self {
            index,
            provider: ProviderRef::parse(NATIVE_SEMANTIC_WIKI_PROVIDER)
                .expect("static SemanticWiki provider ref must be valid"),
        }
    }

    pub fn status(&self) -> SemanticWikiProviderStatus {
        SemanticWikiProviderStatus {
            provider: self.provider.clone(),
            available: true,
            index: self.index.status(),
        }
    }

    pub fn discover(&self) -> Vec<ResourceRef> {
        self.index.discover()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<WikiSearchHit> {
        self.index.search(query, limit)
    }

    pub fn resolve(&self, resource: &ResourceRef) -> Option<WikiObject> {
        self.index.resolve(resource)
    }

    pub fn read(&self, resource: &ResourceRef) -> Result<KnowledgeReading> {
        let object = self.resolve(resource).ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_object_missing",
                format!("Wiki object {resource} is not indexed"),
            )
        })?;
        let content = serialize_object(&object)?;
        Ok(KnowledgeReading {
            resource: resource.clone(),
            provider: Some(self.provider.clone()),
            lens: Some("semantic-wiki".into()),
            revision: Some(object.revision().to_string()),
            freshness: None,
            authority: authority_for_object(&object),
            content: Some(content),
            evidence: self.index.sources(resource),
            why_selected: "selected from the canonical project SemanticWiki".into(),
        })
    }

    pub fn neighbours(&self, resource: &ResourceRef, limit: usize) -> Vec<WikiNeighbour> {
        self.index.neighbours(resource, limit)
    }

    pub fn relations(&self, query: RelationQuery) -> Result<KnowledgeRelationView> {
        query.validate()?;
        let focus = self.relation_node(&query.focus)?;
        let mut view = KnowledgeRelationView::focus_only(query.clone(), focus)?;
        if query.depth == 0 {
            return Ok(view);
        }

        let mut seen = BTreeSet::from([query.focus.clone()]);
        let mut seen_edges = BTreeSet::new();
        let mut queue = VecDeque::from([(query.focus.clone(), 0u8)]);
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= query.depth {
                continue;
            }
            let remaining = query.max_edges.saturating_sub(view.edges.len());
            if remaining == 0 {
                view.truncated = true;
                break;
            }
            for neighbour in self.index.neighbours(&current, remaining) {
                let other = neighbour.resource.clone();
                let (from, to, direction) = match neighbour.direction {
                    WikiRelationDirection::Outgoing => (
                        current.clone(),
                        other.clone(),
                        RelationDirection::Outgoing,
                    ),
                    WikiRelationDirection::Incoming => (
                        other.clone(),
                        current.clone(),
                        RelationDirection::Incoming,
                    ),
                };
                let edge_key = format!("{}\0{}\0{}", from, to, neighbour.edge_ref);
                if !seen_edges.insert(edge_key) {
                    continue;
                }
                if !view.nodes.iter().any(|node| node.resource == other)
                    && !view.push_node(self.relation_node(&other)?)
                {
                    continue;
                }
                view.push_edge(RelationEdge::new(
                    from,
                    to,
                    neighbour.relation.clone(),
                    direction,
                    RelationOrigin::new(authority_for_neighbour(&neighbour))
                        .from_provider(self.provider.clone())
                        .in_lens("semantic-wiki"),
                ))?;
                if seen.insert(other.clone()) {
                    queue.push_back((other, depth + 1));
                }
            }
        }
        Ok(view)
    }

    pub fn frame(&self, resource: &ResourceRef) -> Option<WikiFrame> {
        self.index.frame(resource).cloned()
    }

    pub fn sources(&self, resource: &ResourceRef) -> Vec<SourceRef> {
        self.index.sources(resource)
    }

    pub fn provenance(&self, resource: &ResourceRef) -> Vec<WikiProvenanceRef> {
        self.index.provenance(resource)
    }

    pub fn explain(&self, resource: &ResourceRef) -> Result<WikiExplanation> {
        let object = self.resolve(resource).ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_object_missing",
                format!("Wiki object {resource} is not indexed"),
            )
        })?;
        Ok(WikiExplanation {
            resource: resource.clone(),
            provider: self.provider.clone(),
            object_kind: object_kind(&object).into(),
            revision: object.revision(),
            authority: authority_for_object(&object),
            sources: self.index.sources(resource),
            provenance: self.index.provenance(resource),
            relations: self.index.neighbours(resource, 64),
        })
    }

    fn relation_node(&self, resource: &ResourceRef) -> Result<RelationNode> {
        if let Some(object) = self.resolve(resource) {
            return Ok(RelationNode::new(
                resource.clone(),
                resource_kind(&object),
                object_label(&object),
            ));
        }

        // Source-authored edges may deliberately terminate at a stable resource
        // which is not itself a canonical Wiki object (for example a FlowRef or
        // retained SourceRef). Presence in the relation index is enough to make
        // that endpoint traversable; it does not promote the endpoint into Wiki
        // storage or give it a new semantic identity.
        if !self.index.neighbours(resource, 1).is_empty() {
            return Ok(RelationNode::new(
                resource.clone(),
                external_relation_kind(self.index, resource),
                resource.to_string(),
            ));
        }

        Err(AikitError::new(
            "knowledge.wiki_object_missing",
            format!("Wiki object or relation endpoint {resource} is not indexed"),
        ))
    }
}

fn serialize_object(object: &WikiObject) -> Result<String> {
    let value = match object {
        WikiObject::Space(value) => serde_json::to_value(value),
        WikiObject::Node(value) => serde_json::to_value(value),
        WikiObject::Edge(value) => serde_json::to_value(value),
        WikiObject::Frame(value) => serde_json::to_value(value),
        WikiObject::Reading(value) => serde_json::to_value(value),
    }
    .map_err(|error| {
        AikitError::new(
            "knowledge.wiki_serialization",
            format!("could not serialize Wiki object: {error}"),
        )
    })?;
    serde_json::to_string_pretty(&value).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_serialization",
            format!("could not render Wiki object: {error}"),
        )
    })
}

fn object_kind(object: &WikiObject) -> &'static str {
    match object {
        WikiObject::Space(_) => "space",
        WikiObject::Node(_) => "node",
        WikiObject::Edge(_) => "edge",
        WikiObject::Frame(_) => "frame",
        WikiObject::Reading(_) => "reading",
    }
}

fn resource_kind(object: &WikiObject) -> ResourceKind {
    match object {
        WikiObject::Space(_) => ResourceKind::KnowledgeSpace,
        WikiObject::Node(_) => ResourceKind::KnowledgeNode,
        WikiObject::Edge(_) => ResourceKind::KnowledgeNode,
        WikiObject::Frame(_) => ResourceKind::KnowledgeFrame,
        WikiObject::Reading(_) => ResourceKind::KnowledgeNode,
    }
}

/// SourceRef is opaque and is not required to use a `source:` spelling. Prefer
/// exact edge provenance over lexical prefixes when deciding whether an external
/// relation endpoint is a retained knowledge source. This keeps Central's
/// `central:project-source:*` identities and other conforming source houses native.
fn external_relation_kind(index: &SemanticWikiIndex, resource: &ResourceRef) -> ResourceKind {
    let proven_source = index.neighbours(resource, 16).iter().any(|neighbour| {
        neighbour
            .provenance
            .iter()
            .any(|provenance| provenance.source_ref.as_str() == resource.as_str())
    });
    if proven_source || resource.as_str().starts_with("source:") {
        ResourceKind::KnowledgeSource
    } else {
        ResourceKind::ContextSource
    }
}

fn object_label(object: &WikiObject) -> String {
    match object {
        WikiObject::Space(value) => value
            .title
            .clone()
            .unwrap_or_else(|| value.ref_id.to_string()),
        WikiObject::Node(value) => value
            .title
            .clone()
            .unwrap_or_else(|| value.ref_id.to_string()),
        WikiObject::Edge(value) => value.relation.clone(),
        WikiObject::Frame(value) => format!("Frame {}", value.ref_id),
        WikiObject::Reading(value) => value.reading_type.clone(),
    }
}

fn authority_for_object(object: &WikiObject) -> SourceAuthority {
    match object {
        WikiObject::Edge(value) => authority_for_edge_origin(value.origin),
        WikiObject::Reading(_) => SourceAuthority::Derived,
        _ => SourceAuthority::Authored,
    }
}

/// `WikiEdgeOrigin::Authored` means the relation was explicitly present in source
/// language. It does not by itself prove that the owning source was human-authored.
/// Source compilers may therefore carry the owner's epistemic standing on exact
/// provenance; common Knowledge relation views prefer that standing when present.
fn authority_for_neighbour(neighbour: &WikiNeighbour) -> SourceAuthority {
    neighbour
        .provenance
        .iter()
        .filter_map(|provenance| provenance.extensions.get("source_authority"))
        .find_map(|value| serde_json::from_value::<SourceAuthority>(value.clone()).ok())
        .unwrap_or_else(|| authority_for_edge_origin(neighbour.origin))
}

fn authority_for_edge_origin(origin: WikiEdgeOrigin) -> SourceAuthority {
    match origin {
        WikiEdgeOrigin::Authored => SourceAuthority::Authored,
        WikiEdgeOrigin::Learned => SourceAuthority::Learned,
        WikiEdgeOrigin::Mechanical
        | WikiEdgeOrigin::Compiled
        | WikiEdgeOrigin::Inferred
        | WikiEdgeOrigin::QlDerived
        | WikiEdgeOrigin::MefDerived => SourceAuthority::Derived,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::knowledge_wiki::{parse_wiki_objects, WikiEdge, WikiEdgeOrigin, WikiNode};
    use crate::knowledge_wiki_index::SemanticWikiIndex;

    use super::*;

    fn fixture() -> SemanticWikiIndex {
        let objects = parse_wiki_objects(
            r#"{"objects":[
              {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":1,
               "provenance":[],"title":"Root","parent_space_refs":[],"child_space_refs":[],
               "node_refs":["wiki:node:a","wiki:node:b"]},
              {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:a","revision":2,
               "provenance":[{"source_ref":"source:canon"}],"type":"Concept","title":"Alpha",
               "space_refs":["wiki:space:root"],"source_refs":["source:canon"]},
              {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:b","revision":1,
               "provenance":[],"type":"Concept","title":"Beta","space_refs":["wiki:space:root"],
               "source_refs":[]},
              {"profile":"okf-wiki/v1","object":"edge","ref":"wiki:edge:a-b","revision":1,
               "provenance":[],"from_ref":"wiki:node:a","to_ref":"wiki:node:b",
               "relation":"develops","origin":"authored"}
            ]}"#,
        )
        .unwrap();
        SemanticWikiIndex::rebuild(objects).unwrap()
    }

    #[test]
    fn provider_exposes_complete_native_application_surface() {
        let index = fixture();
        let provider = SemanticWikiProvider::new(&index);
        assert!(provider.status().available);
        assert_eq!(provider.discover().len(), 4);
        assert_eq!(provider.search("Alpha", 10).len(), 1);
        let alpha = ResourceRef::parse("wiki:node:a").unwrap();
        assert_eq!(provider.read(&alpha).unwrap().revision.as_deref(), Some("2"));
        assert_eq!(provider.sources(&alpha)[0].as_str(), "source:canon");
        assert_eq!(provider.provenance(&alpha).len(), 1);
        assert_eq!(provider.explain(&alpha).unwrap().relations.len(), 1);
    }

    #[test]
    fn relation_expansion_preserves_wiki_relation_authority() {
        let index = fixture();
        let provider = SemanticWikiProvider::new(&index);
        let view = provider
            .relations(RelationQuery::local(ResourceRef::parse("wiki:node:a").unwrap()))
            .unwrap();
        assert_eq!(view.nodes.len(), 2);
        assert_eq!(view.edges.len(), 1);
        assert_eq!(view.edges[0].relation, "develops");
        assert_eq!(view.edges[0].origin.authority, SourceAuthority::Authored);
        assert_eq!(
            provider.neighbours(&ResourceRef::parse("wiki:node:a").unwrap(), 10)[0].origin,
            WikiEdgeOrigin::Authored
        );
    }

    #[test]
    fn relation_expansion_keeps_external_source_and_flow_endpoints_outside_wiki_identity() {
        let target = WikiObject::Node(WikiNode {
            profile: crate::knowledge_wiki::OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse("wiki:node:living-wiki").unwrap(),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Concept".into(),
            title: Some("Living Wiki".into()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        });
        let source_id = "central:project-source:demo:alpha";
        let source_edge = WikiObject::Edge(WikiEdge {
            profile: crate::knowledge_wiki::OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse("wiki:edge:source-link").unwrap(),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: SourceRef::parse(source_id).unwrap(),
                source_revision: None,
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::from([(
                    "source_authority".into(),
                    serde_json::json!("learned"),
                )]),
            }],
            from_ref: ResourceRef::parse(source_id).unwrap(),
            to_ref: ResourceRef::parse("wiki:node:living-wiki").unwrap(),
            relation: "references".into(),
            origin: WikiEdgeOrigin::Authored,
            origin_ref: None,
            extensions: BTreeMap::new(),
        });
        let flow_edge = WikiObject::Edge(WikiEdge {
            profile: crate::knowledge_wiki::OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse("wiki:edge:flow-link").unwrap(),
            revision: 1,
            provenance: Vec::new(),
            from_ref: ResourceRef::parse("flow:thread:1").unwrap(),
            to_ref: ResourceRef::parse("wiki:node:living-wiki").unwrap(),
            relation: "references".into(),
            origin: WikiEdgeOrigin::Authored,
            origin_ref: None,
            extensions: BTreeMap::new(),
        });
        let index = SemanticWikiIndex::rebuild([target, source_edge, flow_edge]).unwrap();
        assert!(index.resolve(&ResourceRef::parse(source_id).unwrap()).is_none());
        assert!(index.resolve(&ResourceRef::parse("flow:thread:1").unwrap()).is_none());

        let provider = SemanticWikiProvider::new(&index);
        let source_view = provider
            .relations(RelationQuery::local(ResourceRef::parse(source_id).unwrap()))
            .unwrap();
        assert_eq!(source_view.nodes[0].kind, ResourceKind::KnowledgeSource);
        assert_eq!(source_view.edges[0].origin.authority, SourceAuthority::Learned);

        let flow_view = provider
            .relations(RelationQuery::local(ResourceRef::parse("flow:thread:1").unwrap()))
            .unwrap();
        assert_eq!(flow_view.nodes[0].kind, ResourceKind::ContextSource);
        assert_eq!(flow_view.edges[0].relation, "references");
    }
}
