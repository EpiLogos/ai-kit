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

/// Content-derived invalidation key for one canonical Wiki register.
///
/// `register` is owner-authored identity (for example Central's root or Project
/// Wiki space ref). `revision` describes the exact canonical JSON bytes read for
/// that register; it is never used as identity and a materialisation store must
/// not replace either field with its own row or transaction id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiRegisterRevision {
    pub register: ResourceRef,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticWikiProviderStatus {
    pub provider: ProviderRef,
    pub available: bool,
    pub index: WikiIndexStatus,
    #[serde(default)]
    pub registers: Vec<WikiRegisterRevision>,
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

/// Replaceable read surface for one materialised Wiki projection.
///
/// Implementations may use the in-memory semantic index, SQLite, or another
/// deletable projection. They must preserve the canonical Wiki refs and
/// register revisions supplied by authored storage; provider-local keys are
/// never identity.
pub trait WikiProvider {
    fn status(&self) -> SemanticWikiProviderStatus;
    fn discover(&self) -> Vec<ResourceRef>;
    fn search(&self, query: &str, limit: usize) -> Vec<WikiSearchHit>;
    fn resolve(&self, resource: &ResourceRef) -> Option<WikiObject>;
    fn read(&self, resource: &ResourceRef) -> Result<KnowledgeReading>;
    fn neighbours(&self, resource: &ResourceRef, limit: usize) -> Vec<WikiNeighbour>;
    fn relations(&self, query: RelationQuery) -> Result<KnowledgeRelationView>;
    fn frame(&self, resource: &ResourceRef) -> Option<WikiFrame>;
    fn sources(&self, resource: &ResourceRef) -> Vec<SourceRef>;
    fn provenance(&self, resource: &ResourceRef) -> Vec<WikiProvenanceRef>;
    fn explain(&self, resource: &ResourceRef) -> Result<WikiExplanation>;
}

impl<T: WikiProvider + ?Sized> WikiProvider for &T {
    fn status(&self) -> SemanticWikiProviderStatus {
        (**self).status()
    }
    fn discover(&self) -> Vec<ResourceRef> {
        (**self).discover()
    }
    fn search(&self, query: &str, limit: usize) -> Vec<WikiSearchHit> {
        (**self).search(query, limit)
    }
    fn resolve(&self, resource: &ResourceRef) -> Option<WikiObject> {
        (**self).resolve(resource)
    }
    fn read(&self, resource: &ResourceRef) -> Result<KnowledgeReading> {
        (**self).read(resource)
    }
    fn neighbours(&self, resource: &ResourceRef, limit: usize) -> Vec<WikiNeighbour> {
        (**self).neighbours(resource, limit)
    }
    fn relations(&self, query: RelationQuery) -> Result<KnowledgeRelationView> {
        (**self).relations(query)
    }
    fn frame(&self, resource: &ResourceRef) -> Option<WikiFrame> {
        (**self).frame(resource)
    }
    fn sources(&self, resource: &ResourceRef) -> Vec<SourceRef> {
        (**self).sources(resource)
    }
    fn provenance(&self, resource: &ResourceRef) -> Vec<WikiProvenanceRef> {
        (**self).provenance(resource)
    }
    fn explain(&self, resource: &ResourceRef) -> Result<WikiExplanation> {
        (**self).explain(resource)
    }
}

/// Native application surface over the rebuildable SemanticWiki index.
///
/// The index remains derived state and relation names remain Wiki vocabulary.
/// This provider only projects those native semantics into AIKit's common
/// Knowledge application contracts.
pub struct SemanticWikiProvider<'a> {
    index: &'a SemanticWikiIndex,
    provider: ProviderRef,
    registers: Vec<WikiRegisterRevision>,
}

impl<'a> SemanticWikiProvider<'a> {
    pub fn new(index: &'a SemanticWikiIndex) -> Self {
        Self {
            index,
            provider: ProviderRef::parse(NATIVE_SEMANTIC_WIKI_PROVIDER)
                .expect("static SemanticWiki provider ref must be valid"),
            registers: Vec::new(),
        }
    }

    pub fn with_register_revisions(
        mut self,
        revisions: impl IntoIterator<Item = WikiRegisterRevision>,
    ) -> Self {
        self.registers = revisions.into_iter().collect();
        self.registers
            .sort_by(|left, right| left.register.cmp(&right.register));
        self.registers
            .dedup_by(|left, right| left.register == right.register);
        self
    }

    pub fn with_provider_ref(mut self, provider: ProviderRef) -> Self {
        self.provider = provider;
        self
    }

    pub fn status(&self) -> SemanticWikiProviderStatus {
        SemanticWikiProviderStatus {
            provider: self.provider.clone(),
            available: true,
            index: self.index.status(),
            registers: self.registers.clone(),
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
            // WikiSpace membership is already a canonical source assertion.
            // Expose it through the same bounded relation faculty as WikiEdges;
            // consumers must not reconstruct a private graph from node_refs.
            if let Some(space) = self.index.space(&current) {
                for (other, relation) in space
                    .node_refs
                    .iter()
                    .map(|r| (r, "member"))
                    .chain(space.child_space_refs.iter().map(|r| (r, "child-space")))
                {
                    let key = format!("membership\0{}\0{}\0{}", current, other, relation);
                    if seen_edges.contains(&key) {
                        continue;
                    }
                    if view.edges.len() >= query.max_edges {
                        view.truncated = true;
                        break;
                    }
                    if !view.nodes.iter().any(|n| &n.resource == other)
                        && !view.push_node(self.relation_node(other)?)
                    {
                        continue;
                    }
                    seen_edges.insert(key);
                    view.push_edge(RelationEdge::new(
                        current.clone(),
                        other.clone(),
                        relation,
                        RelationDirection::Outgoing,
                        RelationOrigin::new(SourceAuthority::Authored)
                            .from_provider(self.provider.clone())
                            .in_lens("semantic-wiki")
                            .at_revision(space.revision.to_string()),
                    ))?;
                    if seen.insert(other.clone()) {
                        queue.push_back((other.clone(), depth + 1));
                    }
                }
            }
            // The enclosing direction of that same authored membership.
            // `WikiNode::space_refs` and `WikiSpace::parent_space_refs` are
            // canonical assertions that were previously readable only from
            // the containing end: a node focus could never reach its own
            // Space, so the enclosing band of any node-focused view was
            // structurally empty and consumers were pushed toward
            // reconstructing containment from raw refs — the drift this
            // faculty exists to prevent.
            //
            // The same logical edge is emitted (space -> node, space ->
            // child space), oriented Incoming relative to the focus and
            // keyed identically to the outgoing projection above, so
            // reaching both ends of one membership yields one edge, not two.
            // Only refs that resolve in this index are projected; a ref that
            // resolves in a peer Wiki is the federated norm and is left to
            // the federation seam, matching the local-whole rule below.
            let enclosing: Vec<(ResourceRef, &'static str)> = self
                .index
                .node(&current)
                .map(|node| {
                    node.space_refs
                        .iter()
                        .map(|r| (r.clone(), "member"))
                        .collect::<Vec<_>>()
                })
                .or_else(|| {
                    self.index.space(&current).map(|space| {
                        space
                            .parent_space_refs
                            .iter()
                            .map(|r| (r.clone(), "child-space"))
                            .collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();
            for (container, relation) in enclosing {
                let Some(container_space) = self.index.space(&container) else {
                    continue;
                };
                let key = format!("membership\0{}\0{}\0{}", container, current, relation);
                if seen_edges.contains(&key) {
                    continue;
                }
                if view.edges.len() >= query.max_edges {
                    view.truncated = true;
                    break;
                }
                if !view.nodes.iter().any(|n| n.resource == container)
                    && !view.push_node(self.relation_node(&container)?)
                {
                    continue;
                }
                seen_edges.insert(key);
                view.push_edge(RelationEdge::new(
                    container.clone(),
                    current.clone(),
                    relation,
                    RelationDirection::Incoming,
                    RelationOrigin::new(SourceAuthority::Authored)
                        .from_provider(self.provider.clone())
                        .in_lens("semantic-wiki")
                        .at_revision(container_space.revision.to_string()),
                ))?;
                if seen.insert(container.clone()) {
                    queue.push_back((container, depth + 1));
                }
            }
            // A bounded local whole (W10 V4): a node carrying
            // `local_space_ref` contributes its whole's membership through
            // the same relation faculty, so navigation traverses the set
            // without any consumer reconstructing a private graph.
            if let Some(node) = self.index.node(&current) {
                if let Some(local_ref) = &node.local_space_ref {
                    if let Some(local_space) = self.index.space(local_ref) {
                        for (other, relation) in
                            local_space.node_refs.iter().map(|r| (r, "local-member"))
                        {
                            let key = format!("local-whole\0{}\0{}\0{}", current, other, relation);
                            if seen_edges.contains(&key) {
                                continue;
                            }
                            if view.edges.len() >= query.max_edges {
                                view.truncated = true;
                                break;
                            }
                            if !view.nodes.iter().any(|n| &n.resource == other)
                                && !view.push_node(self.relation_node(other)?)
                            {
                                continue;
                            }
                            seen_edges.insert(key);
                            view.push_edge(RelationEdge::new(
                                current.clone(),
                                other.clone(),
                                relation,
                                RelationDirection::Outgoing,
                                RelationOrigin::new(SourceAuthority::Derived)
                                    .from_provider(self.provider.clone())
                                    .in_lens("semantic-wiki")
                                    .at_revision(local_space.revision.to_string()),
                            ))?;
                            if seen.insert(other.clone()) {
                                queue.push_back((other.clone(), depth + 1));
                            }
                        }
                    }
                }
            }
            let remaining = query.max_edges.saturating_sub(view.edges.len());
            for neighbour in self.index.neighbours(&current, remaining) {
                let other = neighbour.resource.clone();
                let (from, to, direction) = match neighbour.direction {
                    WikiRelationDirection::Outgoing => {
                        (current.clone(), other.clone(), RelationDirection::Outgoing)
                    }
                    WikiRelationDirection::Incoming => {
                        (other.clone(), current.clone(), RelationDirection::Incoming)
                    }
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

impl WikiProvider for SemanticWikiProvider<'_> {
    fn status(&self) -> SemanticWikiProviderStatus {
        SemanticWikiProvider::status(self)
    }

    fn discover(&self) -> Vec<ResourceRef> {
        SemanticWikiProvider::discover(self)
    }

    fn search(&self, query: &str, limit: usize) -> Vec<WikiSearchHit> {
        SemanticWikiProvider::search(self, query, limit)
    }

    fn resolve(&self, resource: &ResourceRef) -> Option<WikiObject> {
        SemanticWikiProvider::resolve(self, resource)
    }

    fn read(&self, resource: &ResourceRef) -> Result<KnowledgeReading> {
        SemanticWikiProvider::read(self, resource)
    }

    fn neighbours(&self, resource: &ResourceRef, limit: usize) -> Vec<WikiNeighbour> {
        SemanticWikiProvider::neighbours(self, resource, limit)
    }

    fn relations(&self, query: RelationQuery) -> Result<KnowledgeRelationView> {
        SemanticWikiProvider::relations(self, query)
    }

    fn frame(&self, resource: &ResourceRef) -> Option<WikiFrame> {
        SemanticWikiProvider::frame(self, resource)
    }

    fn sources(&self, resource: &ResourceRef) -> Vec<SourceRef> {
        SemanticWikiProvider::sources(self, resource)
    }

    fn provenance(&self, resource: &ResourceRef) -> Vec<WikiProvenanceRef> {
        SemanticWikiProvider::provenance(self, resource)
    }

    fn explain(&self, resource: &ResourceRef) -> Result<WikiExplanation> {
        SemanticWikiProvider::explain(self, resource)
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
        let provider = SemanticWikiProvider::new(&index).with_register_revisions([
            WikiRegisterRevision {
                register: ResourceRef::parse("wiki:space:register-b").unwrap(),
                revision: "blake3:b".into(),
            },
            WikiRegisterRevision {
                register: ResourceRef::parse("wiki:space:register-a").unwrap(),
                revision: "blake3:a".into(),
            },
        ]);
        let status = provider.status();
        assert!(status.available);
        assert_eq!(status.registers.len(), 2);
        assert_eq!(
            status.registers[0].register.as_str(),
            "wiki:space:register-a"
        );
        assert_eq!(provider.discover().len(), 4);
        // "Alpha" finds the curated node by its own title, and also finds
        // `source:canon` — the source that node cites — through the same
        // citing-node label (CASE 19). The cited source never joins
        // `discover()`, which stays at 4 curated objects above.
        let alpha_hits = provider.search("Alpha", 10);
        assert_eq!(alpha_hits.len(), 2);
        assert!(alpha_hits
            .iter()
            .any(|hit| hit.address.as_curated().map(ResourceRef::as_str) == Some("wiki:node:a")));
        assert!(alpha_hits.iter().any(|hit| hit
            .address
            .as_authored_source()
            .map(SourceRef::as_str)
            == Some("source:canon")));
        let alpha = ResourceRef::parse("wiki:node:a").unwrap();
        assert_eq!(
            provider.read(&alpha).unwrap().revision.as_deref(),
            Some("2")
        );
        assert_eq!(provider.sources(&alpha)[0].as_str(), "source:canon");
        assert_eq!(provider.provenance(&alpha).len(), 1);
        assert_eq!(provider.explain(&alpha).unwrap().relations.len(), 1);
    }

    #[test]
    fn relation_expansion_preserves_wiki_relation_authority() {
        let index = fixture();
        let provider = SemanticWikiProvider::new(&index);
        let view = provider
            .relations(RelationQuery::local(
                ResourceRef::parse("wiki:node:a").unwrap(),
            ))
            .unwrap();
        // Three nodes and two edges: Beta through the authored `develops`
        // edge, and Root through the enclosing half of the membership the
        // Space asserts. Before the enclosing projection existed this read
        // 2 and 1 — the node could not reach its own Space at all.
        assert_eq!(view.nodes.len(), 3);
        assert_eq!(view.edges.len(), 2);
        let develops = view
            .edges
            .iter()
            .find(|e| e.relation == "develops")
            .expect("the authored edge still arrives");
        assert_eq!(develops.origin.authority, SourceAuthority::Authored);
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
        assert!(index
            .resolve(&ResourceRef::parse(source_id).unwrap())
            .is_none());
        assert!(index
            .resolve(&ResourceRef::parse("flow:thread:1").unwrap())
            .is_none());

        let provider = SemanticWikiProvider::new(&index);
        let source_view = provider
            .relations(RelationQuery::local(ResourceRef::parse(source_id).unwrap()))
            .unwrap();
        assert_eq!(source_view.nodes[0].kind, ResourceKind::KnowledgeSource);
        assert_eq!(
            source_view.edges[0].origin.authority,
            SourceAuthority::Learned
        );

        let flow_view = provider
            .relations(RelationQuery::local(
                ResourceRef::parse("flow:thread:1").unwrap(),
            ))
            .unwrap();
        assert_eq!(flow_view.nodes[0].kind, ResourceKind::ContextSource);
        assert_eq!(flow_view.edges[0].relation, "references");
    }
    /// The enclosing half of membership. A node focus must reach its own
    /// Space: `WikiNode::space_refs` is a canonical assertion, and before
    /// this projection existed it was readable only from the Space end, so
    /// any node-focused view had a structurally empty enclosing band and
    /// consumers were pushed toward rebuilding containment from raw refs.
    #[test]
    fn a_node_reaches_its_enclosing_space_through_the_relation_faculty() {
        let index = fixture();
        let provider = SemanticWikiProvider::new(&index);
        let focus = ResourceRef::parse("wiki:node:a").unwrap();
        let view = provider
            .relations(RelationQuery::local(focus.clone()))
            .unwrap();

        let enclosing: Vec<_> = view
            .edges
            .iter()
            .filter(|e| e.relation == "member")
            .collect();
        assert_eq!(enclosing.len(), 1, "exactly one enclosing Space: {view:?}");
        let edge = enclosing[0];

        // The same logical edge the Space asserts: oriented from the Space,
        // Incoming relative to this focus, carrying the Space's revision.
        assert_eq!(edge.from.as_str(), "wiki:space:root");
        assert_eq!(edge.to, focus);
        assert_eq!(edge.direction, RelationDirection::Incoming);
        assert_eq!(edge.origin.authority, SourceAuthority::Authored);
        assert_eq!(edge.origin.revision.as_deref(), Some("1"));
        assert!(view
            .nodes
            .iter()
            .any(|n| n.resource.as_str() == "wiki:space:root"));
    }

    /// Reaching both ends of one membership must yield one edge, not two.
    /// The enclosing projection keys identically to the containing one, so
    /// a traversal deep enough to arrive from either side still sees a
    /// single assertion.
    #[test]
    fn one_membership_is_one_edge_however_the_traversal_arrives() {
        let index = fixture();
        let provider = SemanticWikiProvider::new(&index);
        let mut query = RelationQuery::local(ResourceRef::parse("wiki:space:root").unwrap());
        query.depth = 2;
        let view = provider.relations(query).unwrap();

        for node in ["wiki:node:a", "wiki:node:b"] {
            let count = view
                .edges
                .iter()
                .filter(|e| {
                    e.relation == "member"
                        && e.from.as_str() == "wiki:space:root"
                        && e.to.as_str() == node
                })
                .count();
            assert_eq!(count, 1, "membership duplicated for {node}: {view:?}");
        }
    }

    /// A Space reaches its own parent for the same reason a node reaches its
    /// Space: `parent_space_refs` is the enclosing half of `child_space_refs`.
    #[test]
    fn a_space_reaches_its_parent_space() {
        let objects = parse_wiki_objects(
            r#"{"objects":[
              {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":3,
               "provenance":[],"title":"Root","parent_space_refs":[],
               "child_space_refs":["wiki:space:child"],"node_refs":[]},
              {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:child","revision":1,
               "provenance":[],"title":"Child","parent_space_refs":["wiki:space:root"],
               "child_space_refs":[],"node_refs":[]}
            ]}"#,
        )
        .unwrap();
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        let provider = SemanticWikiProvider::new(&index);
        let focus = ResourceRef::parse("wiki:space:child").unwrap();
        let view = provider
            .relations(RelationQuery::local(focus.clone()))
            .unwrap();

        let parents: Vec<_> = view
            .edges
            .iter()
            .filter(|e| e.relation == "child-space")
            .collect();
        assert_eq!(parents.len(), 1, "the child reaches its parent: {view:?}");
        assert_eq!(parents[0].from.as_str(), "wiki:space:root");
        assert_eq!(parents[0].to, focus);
        assert_eq!(parents[0].direction, RelationDirection::Incoming);
        assert_eq!(parents[0].origin.revision.as_deref(), Some("3"));
    }

    #[test]
    fn space_membership_is_native_bounded_revision_bearing_relation() {
        let index = fixture();
        let before = index.status();
        let provider = SemanticWikiProvider::new(&index);
        let mut query = RelationQuery::local(ResourceRef::parse("wiki:space:root").unwrap());
        let view = provider.relations(query.clone()).unwrap();
        assert_eq!(view.nodes.len(), 3);
        assert_eq!(view.edges.len(), 2);
        assert!(view
            .edges
            .iter()
            .all(|e| e.relation == "member" && e.from == query.focus));
        assert!(view
            .edges
            .iter()
            .all(|e| e.origin.authority == SourceAuthority::Authored
                && e.origin.revision.as_deref() == Some("1")));
        query.max_nodes = 2;
        let bounded = provider.relations(query).unwrap();
        assert_eq!(bounded.nodes.len(), 2);
        assert_eq!(bounded.edges.len(), 1);
        assert!(bounded.truncated);
        assert_eq!(
            index.status(),
            before,
            "reading never mutates canonical/index membership"
        );
    }
}
