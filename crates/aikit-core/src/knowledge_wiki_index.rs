//! Rebuildable SemanticWiki index over canonical `okf-wiki/v1` objects.
//!
//! The index is derived state. It never becomes canonical identity or relation
//! authority, and rebuilding it from the same Wiki objects is deterministic.
//! Authored and derived relations retain their own [`WikiEdgeOrigin`] rather than
//! being flattened into an undifferentiated graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::knowledge_wiki::{
    WikiEdge, WikiEdgeOrigin, WikiFrame, WikiNode, WikiObject, WikiProvenanceRef, WikiReading,
    WikiSpace,
};
use crate::resource::{ResourceRef, SourceRef};
use crate::{AikitError, Result};

pub const SEMANTIC_WIKI_INDEX_VERSION: &str = "aikit.semantic-wiki-index/v1";
pub const DEFAULT_WIKI_SEARCH_LIMIT: usize = 64;
pub const DEFAULT_WIKI_NEIGHBOUR_LIMIT: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiRelationDirection {
    Outgoing,
    Incoming,
}

/// The address one search hit resolves through. `Curated` and `AuthoredSource`
/// carry different ref types on purpose: a [`SourceRef`] can never be mistaken
/// for, coerced into, or resolved as a [`ResourceRef`] naming a curated Wiki
/// object. Citing a source from a WikiNode makes that source findable; it does
/// not — and structurally cannot — promote the source into curated identity.
/// Promotion stays a human Recognition act, out of scope for this index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WikiSearchAddress {
    Curated { resource: ResourceRef },
    AuthoredSource { source: SourceRef },
}

impl WikiSearchAddress {
    pub fn hit_kind(&self) -> WikiSearchHitKind {
        match self {
            Self::Curated { .. } => WikiSearchHitKind::Curated,
            Self::AuthoredSource { .. } => WikiSearchHitKind::AuthoredSource,
        }
    }

    pub fn as_curated(&self) -> Option<&ResourceRef> {
        match self {
            Self::Curated { resource } => Some(resource),
            Self::AuthoredSource { .. } => None,
        }
    }

    pub fn as_authored_source(&self) -> Option<&SourceRef> {
        match self {
            Self::AuthoredSource { source } => Some(source),
            Self::Curated { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiSearchHitKind {
    Curated,
    AuthoredSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiSearchHit {
    pub address: WikiSearchAddress,
    pub object: String,
    pub label: String,
    pub summary: String,
    pub score: u32,
}

impl WikiSearchHit {
    /// Derived from `address`, never stored redundantly — a stored copy could
    /// drift out of agreement with the address it is supposed to describe.
    pub fn hit_kind(&self) -> WikiSearchHitKind {
        self.address.hit_kind()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiNeighbour {
    pub edge_ref: ResourceRef,
    pub resource: ResourceRef,
    pub direction: WikiRelationDirection,
    pub relation: String,
    pub origin: WikiEdgeOrigin,
    #[serde(default)]
    pub provenance: Vec<WikiProvenanceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiIndexStatus {
    pub version: String,
    pub revision: String,
    pub spaces: usize,
    pub nodes: usize,
    pub edges: usize,
    pub frames: usize,
    pub readings: usize,
    pub backlinks: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiLocalWhole {
    pub node: WikiNode,
    #[serde(default)]
    pub local_space: Option<WikiSpace>,
    #[serde(default)]
    pub members: Vec<ResourceRef>,
    #[serde(default)]
    pub neighbours: Vec<WikiNeighbour>,
}

/// Explicit semantic proposal. The rebuildable index never applies proposals to
/// canonical Wiki storage itself; promotion belongs to the owning application
/// service/store and must result in a new canonical revision before rebuild.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum WikiMutationProposal {
    Upsert {
        object: WikiObjectEnvelope,
    },
    Remove {
        resource: ResourceRef,
        expected_revision: u64,
    },
}

/// Serializable proposal envelope without making `WikiObject` itself a tagged
/// storage contract. Portable canonical objects remain the profile structures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiObjectEnvelope {
    pub object_kind: String,
    pub resource: ResourceRef,
    pub revision: u64,
}

impl WikiObjectEnvelope {
    pub fn from_object(object: &WikiObject) -> Self {
        Self {
            object_kind: object_kind(object).to_string(),
            resource: object.ref_id().clone(),
            revision: object.revision(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SemanticWikiIndex {
    spaces: BTreeMap<ResourceRef, WikiSpace>,
    nodes: BTreeMap<ResourceRef, WikiNode>,
    edges: BTreeMap<ResourceRef, WikiEdge>,
    frames: BTreeMap<ResourceRef, WikiFrame>,
    readings: BTreeMap<ResourceRef, WikiReading>,
    outgoing: BTreeMap<ResourceRef, Vec<ResourceRef>>,
    incoming: BTreeMap<ResourceRef, Vec<ResourceRef>>,
    /// Authored sources cited through `WikiNode::source_refs`, keyed by the
    /// [`SourceRef`] itself and carrying the curated nodes that cite it. This
    /// facet is search-only: it is never consulted by `resolve`, `contains`,
    /// `discover` or `neighbours`, so a cited source's findability can never
    /// make it answerable as a curated Wiki object.
    authored_sources: BTreeMap<SourceRef, BTreeSet<ResourceRef>>,
    revision: String,
}

impl SemanticWikiIndex {
    pub fn rebuild(objects: impl IntoIterator<Item = WikiObject>) -> Result<Self> {
        let mut index = Self::default();
        let mut identities = BTreeSet::new();
        let mut revision_material = Vec::new();

        for object in objects {
            object.validate()?;
            let id = object.ref_id().clone();
            if !identities.insert(id.clone()) {
                return Err(AikitError::new(
                    "knowledge.wiki_duplicate_ref",
                    "SemanticWiki contains duplicate stable refs",
                )
                .with("resource", id.to_string()));
            }
            revision_material.push(format!(
                "{}:{}:{}",
                object_kind(&object),
                id,
                object.revision()
            ));
            match object {
                WikiObject::Space(value) => {
                    index.spaces.insert(id, value);
                }
                WikiObject::Node(value) => {
                    // The authored-source facet is derived strictly from
                    // `source_refs`, the citations a curated node itself
                    // declares. It never reaches into provenance, and it
                    // never touches `identities`/`revision_material` — a
                    // cited source carries no revision of its own here and
                    // must not perturb the deterministic index revision.
                    for source in &value.source_refs {
                        index
                            .authored_sources
                            .entry(source.clone())
                            .or_default()
                            .insert(id.clone());
                    }
                    index.nodes.insert(id, value);
                }
                WikiObject::Edge(value) => {
                    index.edges.insert(id, value);
                }
                WikiObject::Frame(value) => {
                    index.frames.insert(id, value);
                }
                WikiObject::Reading(value) => {
                    index.readings.insert(id, value);
                }
            }
        }

        revision_material.sort();
        index.revision = blake3::hash(revision_material.join("\n").as_bytes())
            .to_hex()
            .to_string();
        index.rebuild_relations();
        index.validate_space_topology()?;
        index.validate_local_wholes()?;
        Ok(index)
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn status(&self) -> WikiIndexStatus {
        WikiIndexStatus {
            version: SEMANTIC_WIKI_INDEX_VERSION.into(),
            revision: self.revision.clone(),
            spaces: self.spaces.len(),
            nodes: self.nodes.len(),
            edges: self.edges.len(),
            frames: self.frames.len(),
            readings: self.readings.len(),
            backlinks: self.incoming.values().map(Vec::len).sum(),
        }
    }

    pub fn discover(&self) -> Vec<ResourceRef> {
        self.all_refs().cloned().collect()
    }

    pub fn contains(&self, resource: &ResourceRef) -> bool {
        self.spaces.contains_key(resource)
            || self.nodes.contains_key(resource)
            || self.edges.contains_key(resource)
            || self.frames.contains_key(resource)
            || self.readings.contains_key(resource)
    }

    pub fn resolve(&self, resource: &ResourceRef) -> Option<WikiObject> {
        if let Some(value) = self.spaces.get(resource) {
            return Some(WikiObject::Space(value.clone()));
        }
        if let Some(value) = self.nodes.get(resource) {
            return Some(WikiObject::Node(value.clone()));
        }
        if let Some(value) = self.edges.get(resource) {
            return Some(WikiObject::Edge(value.clone()));
        }
        if let Some(value) = self.frames.get(resource) {
            return Some(WikiObject::Frame(value.clone()));
        }
        self.readings
            .get(resource)
            .cloned()
            .map(WikiObject::Reading)
    }

    pub fn node(&self, resource: &ResourceRef) -> Option<&WikiNode> {
        self.nodes.get(resource)
    }

    pub fn space(&self, resource: &ResourceRef) -> Option<&WikiSpace> {
        self.spaces.get(resource)
    }

    pub fn frame(&self, resource: &ResourceRef) -> Option<&WikiFrame> {
        self.frames.get(resource)
    }

    pub fn reading(&self, resource: &ResourceRef) -> Option<&WikiReading> {
        self.readings.get(resource)
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<WikiSearchHit> {
        if limit == 0 {
            return Vec::new();
        }
        let tokens = tokens(query);
        let mut hits = Vec::new();
        for resource in self.all_refs() {
            let (object, label, searchable, summary) = self.search_document(resource);
            let Some(score) = score(&tokens, resource.as_str(), &label, &searchable) else {
                continue;
            };
            hits.push(WikiSearchHit {
                address: WikiSearchAddress::Curated {
                    resource: resource.clone(),
                },
                object: object.into(),
                label,
                summary,
                score,
            });
        }
        hits.extend(self.search_authored_sources(&tokens));
        hits.sort_by(|left, right| {
            left.score
                .cmp(&right.score)
                .then_with(|| search_rank(&left.address).cmp(&search_rank(&right.address)))
                .then_with(|| {
                    search_address_key(&left.address).cmp(search_address_key(&right.address))
                })
        });
        hits.truncate(limit);
        hits
    }

    /// Second search pass over authored sources cited by curated nodes.
    /// Scored the same way as any other document: the source ref stands in
    /// for its own id/label, and the citing nodes' labels widen what a query
    /// can match without ever adding the source to `all_refs()`.
    fn search_authored_sources(&self, tokens: &[String]) -> Vec<WikiSearchHit> {
        let mut hits = Vec::new();
        for (source, citing_nodes) in &self.authored_sources {
            let labels: Vec<String> = citing_nodes
                .iter()
                .map(|node_ref| self.node_label(node_ref))
                .collect();
            let searchable = format!("{} {}", source.as_str(), labels.join(" "));
            let Some(score) = score(tokens, source.as_str(), source.as_str(), &searchable) else {
                continue;
            };
            hits.push(WikiSearchHit {
                address: WikiSearchAddress::AuthoredSource {
                    source: source.clone(),
                },
                object: "source".into(),
                label: source.to_string(),
                summary: format!(
                    "cited by {} node{}: {}",
                    citing_nodes.len(),
                    if citing_nodes.len() == 1 { "" } else { "s" },
                    labels.join(", ")
                ),
                score,
            });
        }
        hits
    }

    fn node_label(&self, node_ref: &ResourceRef) -> String {
        self.nodes
            .get(node_ref)
            .and_then(|node| node.title.clone())
            .unwrap_or_else(|| node_ref.to_string())
    }

    pub fn neighbours(&self, resource: &ResourceRef, limit: usize) -> Vec<WikiNeighbour> {
        if limit == 0 {
            return Vec::new();
        }
        let mut result = Vec::new();
        if let Some(edge_refs) = self.outgoing.get(resource) {
            for edge_ref in edge_refs {
                if let Some(edge) = self.edges.get(edge_ref) {
                    result.push(neighbour(edge, WikiRelationDirection::Outgoing));
                }
            }
        }
        if let Some(edge_refs) = self.incoming.get(resource) {
            for edge_ref in edge_refs {
                if let Some(edge) = self.edges.get(edge_ref) {
                    result.push(neighbour(edge, WikiRelationDirection::Incoming));
                }
            }
        }
        result.sort_by(|left, right| {
            left.relation
                .cmp(&right.relation)
                .then_with(|| left.resource.cmp(&right.resource))
                .then_with(|| left.edge_ref.cmp(&right.edge_ref))
        });
        result.truncate(limit);
        result
    }

    pub fn backlinks(&self, resource: &ResourceRef) -> Vec<WikiNeighbour> {
        self.incoming
            .get(resource)
            .into_iter()
            .flatten()
            .filter_map(|edge_ref| self.edges.get(edge_ref))
            .map(|edge| neighbour(edge, WikiRelationDirection::Incoming))
            .collect()
    }

    /// Recursive Space/subspace traversal. Cycles are tolerated in the authored
    /// graph but never cause an unbounded read.
    pub fn subspaces(&self, root: &ResourceRef, max_depth: usize) -> Vec<ResourceRef> {
        let Some(_) = self.spaces.get(root) else {
            return Vec::new();
        };
        let mut seen = BTreeSet::from([root.clone()]);
        let mut queue = VecDeque::from([(root.clone(), 0usize)]);
        let mut result = Vec::new();
        while let Some((space_ref, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            let Some(space) = self.spaces.get(&space_ref) else {
                continue;
            };
            for child in &space.child_space_refs {
                if seen.insert(child.clone()) {
                    result.push(child.clone());
                    queue.push_back((child.clone(), depth + 1));
                }
            }
        }
        result
    }

    /// A WikiNode may itself anchor a local Space. This returns the bounded local
    /// whole without promoting the local Space or its neighbours into a new
    /// canonical identity for the node.
    pub fn local_whole(&self, node_ref: &ResourceRef) -> Result<WikiLocalWhole> {
        let node = self.nodes.get(node_ref).cloned().ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_node_missing",
                format!("WikiNode {node_ref} is not indexed"),
            )
        })?;
        let local_space = node
            .local_space_ref
            .as_ref()
            .and_then(|space_ref| self.spaces.get(space_ref))
            .cloned();
        let members = local_space
            .as_ref()
            .map(|space| space.node_refs.clone())
            .unwrap_or_default();
        Ok(WikiLocalWhole {
            node,
            local_space,
            members,
            neighbours: self.neighbours(node_ref, DEFAULT_WIKI_NEIGHBOUR_LIMIT),
        })
    }

    pub fn sources(&self, resource: &ResourceRef) -> Vec<SourceRef> {
        match self.resolve(resource) {
            Some(WikiObject::Node(node)) => node.source_refs,
            Some(object) => provenance_for(&object)
                .into_iter()
                .map(|provenance| provenance.source_ref)
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn provenance(&self, resource: &ResourceRef) -> Vec<WikiProvenanceRef> {
        self.resolve(resource)
            .map(|object| provenance_for(&object))
            .unwrap_or_default()
    }

    /// The curated nodes that cite `source`, the inverse of [`Self::sources`].
    ///
    /// This is the one thing the Wiki genuinely knows about a source it does
    /// not hold: that it is cited, and by whom. A caller handed a source
    /// address it cannot open can still learn where the citation came from,
    /// which is the difference between an absence and a dead end. Reading a
    /// citation never makes the cited source a curated object.
    pub fn citing_nodes(&self, source: &SourceRef) -> Vec<ResourceRef> {
        self.authored_sources
            .get(source)
            .map(|nodes| nodes.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn proposal_to_upsert(&self, object: &WikiObject) -> WikiMutationProposal {
        WikiMutationProposal::Upsert {
            object: WikiObjectEnvelope::from_object(object),
        }
    }

    pub fn proposal_to_remove(&self, resource: ResourceRef) -> Result<WikiMutationProposal> {
        let object = self.resolve(&resource).ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_object_missing",
                format!("Wiki object {resource} is not indexed"),
            )
        })?;
        Ok(WikiMutationProposal::Remove {
            resource,
            expected_revision: object.revision(),
        })
    }

    fn rebuild_relations(&mut self) {
        self.outgoing.clear();
        self.incoming.clear();
        for (edge_ref, edge) in &self.edges {
            self.outgoing
                .entry(edge.from_ref.clone())
                .or_default()
                .push(edge_ref.clone());
            self.incoming
                .entry(edge.to_ref.clone())
                .or_default()
                .push(edge_ref.clone());
        }
        for edges in self.outgoing.values_mut() {
            edges.sort();
        }
        for edges in self.incoming.values_mut() {
            edges.sort();
        }
    }

    fn validate_space_topology(&self) -> Result<()> {
        for space in self.spaces.values() {
            for child in &space.child_space_refs {
                let Some(child_space) = self.spaces.get(child) else {
                    return Err(AikitError::new(
                        "knowledge.wiki_space_missing_child",
                        "WikiSpace child ref does not resolve inside the SemanticWiki",
                    )
                    .with("space", space.ref_id.to_string())
                    .with("child", child.to_string()));
                };
                if !child_space.parent_space_refs.contains(&space.ref_id) {
                    return Err(AikitError::new(
                        "knowledge.wiki_space_asymmetry",
                        "WikiSpace child relation is not reciprocated by parent_space_refs",
                    )
                    .with("space", space.ref_id.to_string())
                    .with("child", child.to_string()));
                }
            }
            for node_ref in &space.node_refs {
                if !self.nodes.contains_key(node_ref) {
                    return Err(AikitError::new(
                        "knowledge.wiki_space_missing_node",
                        "WikiSpace member ref does not resolve to a WikiNode",
                    )
                    .with("space", space.ref_id.to_string())
                    .with("node", node_ref.to_string()));
                }
            }
        }
        Ok(())
    }

    fn validate_local_wholes(&self) -> Result<()> {
        for node in self.nodes.values() {
            if let Some(space_ref) = &node.local_space_ref {
                let Some(space) = self.spaces.get(space_ref) else {
                    return Err(AikitError::new(
                        "knowledge.wiki_local_space_missing",
                        "WikiNode local_space_ref does not resolve to a WikiSpace",
                    )
                    .with("node", node.ref_id.to_string())
                    .with("space", space_ref.to_string()));
                };
                if space.anchor_ref.as_ref() != Some(&node.ref_id) {
                    return Err(AikitError::new(
                        "knowledge.wiki_local_space_anchor",
                        "Node-as-local-whole requires the local WikiSpace anchor_ref to be the node",
                    )
                    .with("node", node.ref_id.to_string())
                    .with("space", space_ref.to_string()));
                }
            }
        }
        Ok(())
    }

    fn all_refs(&self) -> impl Iterator<Item = &ResourceRef> {
        self.spaces
            .keys()
            .chain(self.nodes.keys())
            .chain(self.edges.keys())
            .chain(self.frames.keys())
            .chain(self.readings.keys())
    }

    fn search_document(&self, resource: &ResourceRef) -> (&'static str, String, String, String) {
        if let Some(space) = self.spaces.get(resource) {
            let label = space.title.clone().unwrap_or_else(|| resource.to_string());
            let searchable = format!("{} {}", label, space.node_refs.len());
            return (
                "space",
                label,
                searchable,
                format!(
                    "{} node refs · {} child spaces",
                    space.node_refs.len(),
                    space.child_space_refs.len()
                ),
            );
        }
        if let Some(node) = self.nodes.get(resource) {
            let label = node.title.clone().unwrap_or_else(|| resource.to_string());
            let searchable = format!("{} {} {}", label, node.node_type, node.source_refs.len());
            return (
                "node",
                label,
                searchable,
                format!(
                    "{} · {} source refs",
                    node.node_type,
                    node.source_refs.len()
                ),
            );
        }
        if let Some(edge) = self.edges.get(resource) {
            return (
                "edge",
                edge.relation.clone(),
                format!("{} {} {}", edge.relation, edge.from_ref, edge.to_ref),
                format!("{} → {} · {:?}", edge.from_ref, edge.to_ref, edge.origin),
            );
        }
        if let Some(frame) = self.frames.get(resource) {
            let label = frame
                .inquiry_ref
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| resource.to_string());
            return (
                "frame",
                label.clone(),
                format!("{} {}", label, frame.member_refs.len()),
                format!(
                    "{} members · {} spaces",
                    frame.member_refs.len(),
                    frame.space_refs.len()
                ),
            );
        }
        let reading = self
            .readings
            .get(resource)
            .expect("resource came from one of the index maps");
        (
            "reading",
            reading.reading_type.clone(),
            format!("{} {}", reading.reading_type, reading.frame_ref),
            format!("reading of {}", reading.frame_ref),
        )
    }
}

/// Curated objects outrank authored sources whenever the relevance score
/// cannot separate them. A scoreless query (the browse case: every document
/// scores alike) would otherwise be decided by raw ref spelling, and source
/// refs sort ahead of `wiki:` refs — pushing curated objects out of the
/// truncation window and quietly making the field look like its citations.
/// Findability never costs the curated field its precedence.
fn search_rank(address: &WikiSearchAddress) -> u8 {
    match address {
        WikiSearchAddress::Curated { .. } => 0,
        WikiSearchAddress::AuthoredSource { .. } => 1,
    }
}

/// Deterministic tie-break key shared by both search-hit addresses. A
/// [`SourceRef`] and a [`ResourceRef`] are distinct types with no shared
/// ordering, so hits must be compared by their address's own string form
/// rather than by a field that only one variant carries.
fn search_address_key(address: &WikiSearchAddress) -> &str {
    match address {
        WikiSearchAddress::Curated { resource } => resource.as_str(),
        WikiSearchAddress::AuthoredSource { source } => source.as_str(),
    }
}

fn neighbour(edge: &WikiEdge, direction: WikiRelationDirection) -> WikiNeighbour {
    WikiNeighbour {
        edge_ref: edge.ref_id.clone(),
        resource: match direction {
            WikiRelationDirection::Outgoing => edge.to_ref.clone(),
            WikiRelationDirection::Incoming => edge.from_ref.clone(),
        },
        direction,
        relation: edge.relation.clone(),
        origin: edge.origin,
        provenance: edge.provenance.clone(),
    }
}

fn provenance_for(object: &WikiObject) -> Vec<WikiProvenanceRef> {
    match object {
        WikiObject::Space(value) => value.provenance.clone(),
        WikiObject::Node(value) => value.provenance.clone(),
        WikiObject::Edge(value) => value.provenance.clone(),
        WikiObject::Frame(value) => value.provenance.clone(),
        WikiObject::Reading(value) => value.provenance.clone(),
    }
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

fn tokens(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == ':'))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn score(tokens: &[String], id: &str, label: &str, searchable: &str) -> Option<u32> {
    if tokens.is_empty() {
        return Some(100);
    }
    let id = id.to_lowercase();
    let label = label.to_lowercase();
    let searchable = searchable.to_lowercase();
    let mut total = 0u32;
    for token in tokens {
        if label == *token || id == *token {
            total += 0;
        } else if label.starts_with(token) || id.starts_with(token) {
            total += 1;
        } else if label.contains(token) || id.contains(token) {
            total += 2;
        } else if searchable.contains(token) {
            total += 3;
        } else {
            return None;
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_wiki::{SemanticRevision, WikiProvenanceRef};

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn space(
        id: &str,
        title: &str,
        parents: &[&str],
        children: &[&str],
        nodes: &[&str],
        anchor: Option<&str>,
    ) -> WikiObject {
        WikiObject::Space(WikiSpace {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: r(id),
            revision: 1,
            provenance: Vec::new(),
            title: Some(title.into()),
            parent_space_refs: parents.iter().map(|value| r(value)).collect(),
            child_space_refs: children.iter().map(|value| r(value)).collect(),
            node_refs: nodes.iter().map(|value| r(value)).collect(),
            anchor_ref: anchor.map(r),
            extensions: BTreeMap::new(),
        })
    }

    fn node(id: &str, title: &str, spaces: &[&str], local_space: Option<&str>) -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: r(id),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: SourceRef::parse("source:paper:17").unwrap(),
                source_revision: Some(SemanticRevision::Text("sha256:abc".into())),
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "Concept".into(),
            title: Some(title.into()),
            space_refs: spaces.iter().map(|value| r(value)).collect(),
            source_refs: vec![SourceRef::parse("source:paper:17").unwrap()],
            local_space_ref: local_space.map(r),
            extensions: BTreeMap::new(),
        })
    }

    /// Like `node`, but with an explicit, controllable `source_refs` list —
    /// `node` always cites `source:paper:17`, which is fine for relation
    /// fixtures but too coarse for exercising the authored-source facet
    /// directly against distinct, test-chosen sources.
    fn node_with_sources(
        id: &str,
        title: &str,
        spaces: &[&str],
        local_space: Option<&str>,
        sources: &[&str],
    ) -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: r(id),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Concept".into(),
            title: Some(title.into()),
            space_refs: spaces.iter().map(|value| r(value)).collect(),
            source_refs: sources
                .iter()
                .map(|value| SourceRef::parse(*value).unwrap())
                .collect(),
            local_space_ref: local_space.map(r),
            extensions: BTreeMap::new(),
        })
    }

    fn edge(id: &str, from: &str, to: &str, relation: &str, origin: WikiEdgeOrigin) -> WikiObject {
        WikiObject::Edge(WikiEdge {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: r(id),
            revision: 1,
            provenance: Vec::new(),
            from_ref: r(from),
            to_ref: r(to),
            relation: relation.into(),
            origin,
            origin_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    #[test]
    fn rebuild_is_deterministic_and_search_backlinks_preserve_authority() {
        let objects = vec![
            space(
                "wiki:space:root",
                "Root",
                &[],
                &[],
                &["wiki:node:a", "wiki:node:b"],
                Some("wiki:node:a"),
            ),
            node("wiki:node:a", "Semantic Wiki", &["wiki:space:root"], None),
            node("wiki:node:b", "Source Pool", &["wiki:space:root"], None),
            edge(
                "wiki:edge:a-b",
                "wiki:node:a",
                "wiki:node:b",
                "develops",
                WikiEdgeOrigin::Authored,
            ),
        ];
        let first = SemanticWikiIndex::rebuild(objects.clone()).unwrap();
        let second = SemanticWikiIndex::rebuild(objects).unwrap();
        assert_eq!(first.revision(), second.revision());
        assert_eq!(
            first.search("source pool", 10)[0]
                .address
                .as_curated()
                .unwrap()
                .as_str(),
            "wiki:node:b"
        );
        let backlinks = first.backlinks(&r("wiki:node:b"));
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].origin, WikiEdgeOrigin::Authored);
        assert_eq!(backlinks[0].resource.as_str(), "wiki:node:a");
    }

    #[test]
    fn recursive_spaces_and_node_as_local_whole_are_bounded() {
        let objects = vec![
            space(
                "wiki:space:root",
                "Root",
                &[],
                &["wiki:space:child"],
                &["wiki:node:whole"],
                Some("wiki:node:whole"),
            ),
            space(
                "wiki:space:child",
                "Child",
                &["wiki:space:root"],
                &[],
                &["wiki:node:whole", "wiki:node:part"],
                Some("wiki:node:whole"),
            ),
            node(
                "wiki:node:whole",
                "Whole",
                &["wiki:space:root", "wiki:space:child"],
                Some("wiki:space:child"),
            ),
            node("wiki:node:part", "Part", &["wiki:space:child"], None),
            edge(
                "wiki:edge:whole-part",
                "wiki:node:whole",
                "wiki:node:part",
                "contains",
                WikiEdgeOrigin::QlDerived,
            ),
        ];
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        assert_eq!(
            index.subspaces(&r("wiki:space:root"), 1),
            vec![r("wiki:space:child")]
        );
        let whole = index.local_whole(&r("wiki:node:whole")).unwrap();
        assert_eq!(
            whole.local_space.unwrap().ref_id.as_str(),
            "wiki:space:child"
        );
        assert_eq!(whole.members.len(), 2);
        assert_eq!(whole.neighbours[0].origin, WikiEdgeOrigin::QlDerived);
    }

    #[test]
    fn index_rejects_provider_identity_duplicates_and_broken_local_wholes() {
        let duplicated = vec![
            node("wiki:node:a", "A", &[], None),
            node("wiki:node:a", "Again", &[], None),
        ];
        assert_eq!(
            SemanticWikiIndex::rebuild(duplicated).unwrap_err().code(),
            "knowledge.wiki_duplicate_ref"
        );

        let broken = vec![node("wiki:node:a", "A", &[], Some("wiki:space:missing"))];
        assert_eq!(
            SemanticWikiIndex::rebuild(broken).unwrap_err().code(),
            "knowledge.wiki_local_space_missing"
        );
    }

    #[test]
    fn authored_source_is_findable_and_carries_its_citing_node_as_backlink() {
        let objects = vec![node_with_sources(
            "wiki:node:erp",
            "Encapsulation",
            &[],
            None,
            &["source:paper:erp-99"],
        )];
        let index = SemanticWikiIndex::rebuild(objects).unwrap();

        let hits = index.search("erp-99", 10);
        let hit = hits
            .iter()
            .find(|hit| hit.hit_kind() == WikiSearchHitKind::AuthoredSource)
            .expect("the cited source is findable by its own ref");
        assert_eq!(
            hit.address.as_authored_source().unwrap().as_str(),
            "source:paper:erp-99"
        );
        assert_eq!(hit.object, "source");
        assert!(
            hit.summary.contains("Encapsulation"),
            "backlink names the citing curated node: {}",
            hit.summary
        );

        // The citing node's own title is part of what makes the source
        // findable, not only the source ref string.
        let by_citing_label = index.search("Encapsulation", 10);
        assert!(by_citing_label
            .iter()
            .any(|hit| hit.hit_kind() == WikiSearchHitKind::AuthoredSource));
    }

    #[test]
    fn authored_source_never_resolves_contains_or_discovers_as_a_curated_object() {
        let objects = vec![node_with_sources(
            "wiki:node:erp",
            "Encapsulation",
            &[],
            None,
            &["source:paper:erp-99"],
        )];
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        let source_as_resource = ResourceRef::parse("source:paper:erp-99").unwrap();

        assert!(index.resolve(&source_as_resource).is_none());
        assert!(!index.contains(&source_as_resource));
        assert!(!index
            .discover()
            .iter()
            .any(|resource| resource.as_str() == "source:paper:erp-99"));

        let status = index.status();
        assert_eq!(status.nodes, 1, "citing a source adds no curated node");
        assert_eq!(status.spaces, 0);
        assert_eq!(status.edges, 0);
        assert_eq!(status.frames, 0);
        assert_eq!(status.readings, 0);
    }

    #[test]
    fn browse_keeps_curated_objects_ahead_of_the_sources_they_cite() {
        // A scoreless query scores every document alike, so the tie-break
        // decides the truncation window. Source refs sort ahead of `wiki:`
        // refs by raw spelling; curated objects must still come first, or a
        // browse of the field returns its citations instead of its content.
        let objects = vec![node_with_sources(
            "wiki:node:erp",
            "Encapsulation",
            &[],
            None,
            &["source:paper:erp-99"],
        )];
        let index = SemanticWikiIndex::rebuild(objects).unwrap();

        let hits = index.search("", 10);
        assert_eq!(hits.len(), 2, "browse returns the node and its source");
        assert_eq!(hits[0].hit_kind(), WikiSearchHitKind::Curated);
        assert_eq!(hits[1].hit_kind(), WikiSearchHitKind::AuthoredSource);

        // The precedence has to hold under truncation, not merely in order:
        // a one-hit window is the curated node, never the cited source.
        let narrowed = index.search("", 1);
        assert_eq!(narrowed.len(), 1);
        assert_eq!(
            narrowed[0].address.as_curated().unwrap().as_str(),
            "wiki:node:erp"
        );
    }

    #[test]
    fn authored_source_search_is_deterministic_across_rebuilds() {
        let objects = vec![
            node_with_sources("wiki:node:a", "Alpha", &[], None, &["source:paper:shared"]),
            node_with_sources("wiki:node:b", "Beta", &[], None, &["source:paper:shared"]),
        ];
        let first = SemanticWikiIndex::rebuild(objects.clone()).unwrap();
        let second = SemanticWikiIndex::rebuild(objects).unwrap();

        assert_eq!(first.revision(), second.revision());
        let first_hits = first.search("shared", 10);
        let second_hits = second.search("shared", 10);
        assert_eq!(first_hits, second_hits);
        let hit = first_hits
            .iter()
            .find(|hit| hit.hit_kind() == WikiSearchHitKind::AuthoredSource)
            .expect("the shared source is findable");
        assert!(hit.summary.contains("Alpha") && hit.summary.contains("Beta"));
    }
}
