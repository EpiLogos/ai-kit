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

/// One dangling reference the read-side rebuild set aside instead of refusing
/// the whole index. `code` is the stable error code the strict rebuild raises
/// for the same fault, so a read-side repair and a write-time refusal name the
/// same thing in the same vocabulary; `subject` is the space (or node) that
/// declared the unresolved reference and `other` the reference that did not
/// resolve.
///
/// Repairs are disclosure, not healing: nothing canonical is rewritten. The
/// repaired entry is dropped from the derived read index only, and the strict
/// `rebuild` keeps refusing, so every write-time gate is unweakened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiIndexRepair {
    pub code: String,
    pub subject: String,
    pub other: String,
}

impl WikiIndexRepair {
    /// The exact error the strict rebuild raises for this fault: same code,
    /// same message, same details, in the same detail keys. `rebuild` delegates
    /// to the degrading path and converts the first repair back through here,
    /// which is what makes "strict is unchanged" provable rather than claimed.
    fn strict_error(&self) -> AikitError {
        match self.code.as_str() {
            "knowledge.wiki_space_missing_child" => AikitError::new(
                "knowledge.wiki_space_missing_child",
                "WikiSpace child ref does not resolve inside the SemanticWiki",
            )
            .with("space", self.subject.clone())
            .with("child", self.other.clone()),
            "knowledge.wiki_space_asymmetry" => AikitError::new(
                "knowledge.wiki_space_asymmetry",
                "WikiSpace child relation is not reciprocated by parent_space_refs",
            )
            .with("space", self.subject.clone())
            .with("child", self.other.clone()),
            "knowledge.wiki_space_missing_node" => AikitError::new(
                "knowledge.wiki_space_missing_node",
                "WikiSpace member ref does not resolve to a WikiNode",
            )
            .with("space", self.subject.clone())
            .with("node", self.other.clone()),
            "knowledge.wiki_local_space_missing" => AikitError::new(
                "knowledge.wiki_local_space_missing",
                "WikiNode local_space_ref does not resolve to a WikiSpace",
            )
            .with("node", self.subject.clone())
            .with("space", self.other.clone()),
            "knowledge.wiki_local_space_anchor" => AikitError::new(
                "knowledge.wiki_local_space_anchor",
                "Node-as-local-whole requires the local WikiSpace anchor_ref to be the node",
            )
            .with("node", self.subject.clone())
            .with("space", self.other.clone()),
            other => unreachable!("no repair carries the unknown code {other}"),
        }
    }

    /// The one named-absence line a repair discloses: kind, declaring ref and
    /// unresolved ref, plus what the read side did about it.
    pub fn absence_line(&self) -> String {
        let detail = match self.code.as_str() {
            "knowledge.wiki_space_missing_child" => format!(
                "{} declares child {}, which does not resolve; entry dropped from the read index",
                self.subject, self.other
            ),
            "knowledge.wiki_space_asymmetry" => format!(
                "{} declares child {} without reciprocation; entry dropped from the read index",
                self.subject, self.other
            ),
            "knowledge.wiki_space_missing_node" => format!(
                "{} declares member {}, which does not resolve; entry dropped from the read index",
                self.subject, self.other
            ),
            "knowledge.wiki_local_space_missing" => format!(
                "{} anchors local space {}, which does not resolve; local whole set aside",
                self.subject, self.other
            ),
            "knowledge.wiki_local_space_anchor" => format!(
                "{}'s local space {} is not anchored on the node; local whole set aside",
                self.subject, self.other
            ),
            _ => format!("{} {}", self.subject, self.other),
        };
        format!("SemanticWiki read repaired [{}]: {}", self.code, detail)
    }
}

/// The named-absence lines for a repair set: one line per distinct
/// `(code, subject, other)`, so a many-offender world cannot flood a reply,
/// in deterministic first-occurrence order.
pub fn repair_absence_lines(repairs: &[WikiIndexRepair]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for repair in repairs {
        if seen.insert((
            repair.code.as_str(),
            repair.subject.as_str(),
            repair.other.as_str(),
        )) {
            lines.push(repair.absence_line());
        }
    }
    lines
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
    /// The strict rebuild. Write-time gates (`WikiDocument::validate`, the
    /// ProjectCentral maintenance planner, `wiki validate`) run exactly this:
    /// the first dangling reference refuses the whole index. It is the
    /// degrading path with every repair converted back into the error the
    /// strict walk raised, so its behaviour cannot drift from the read side.
    pub fn rebuild(objects: impl IntoIterator<Item = WikiObject>) -> Result<Self> {
        let (index, repairs) = Self::rebuild_with_repairs(objects)?;
        match repairs.first() {
            Some(repair) => Err(repair.strict_error()),
            None => Ok(index),
        }
    }

    /// The read-side rebuild: materialise the index, setting dangling
    /// references aside as named [`WikiIndexRepair`]s instead of refusing the
    /// whole knowledge faculty. One dangling `child_space_refs` entry costs
    /// that entry, never the field. Duplicates and invalid objects stay fatal —
    /// those are canonical-corruption faults, not topology drift.
    pub fn rebuild_with_repairs(
        objects: impl IntoIterator<Item = WikiObject>,
    ) -> Result<(Self, Vec<WikiIndexRepair>)> {
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
        let repairs = index.repair_topology();
        Ok((index, repairs))
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

    /// The topology walk behind both rebuild faces, in the strict walk's exact
    /// order so `repairs[0]` is always the fault the strict rebuild refuses
    /// first. Each repair names its drop and the drop is applied to the read
    /// index: a dangling child or membership ref leaves its space (the space
    /// stays), an unreciprocated child edge is dropped from the declaring
    /// parent, a broken local whole is set aside (the reader already handles
    /// `None`). Nothing canonical is touched — repairs re-derive identically
    /// on every rebuild from the same objects.
    fn repair_topology(&mut self) -> Vec<WikiIndexRepair> {
        let mut repairs = Vec::new();
        let mut drop_children: BTreeMap<ResourceRef, Vec<ResourceRef>> = BTreeMap::new();
        let mut drop_members: BTreeMap<ResourceRef, Vec<ResourceRef>> = BTreeMap::new();
        let mut drop_local_wholes: Vec<ResourceRef> = Vec::new();

        for (space_ref, space) in &self.spaces {
            for child in &space.child_space_refs {
                let resolved = self.spaces.get(child);
                if resolved.is_none() {
                    repairs.push(WikiIndexRepair {
                        code: "knowledge.wiki_space_missing_child".into(),
                        subject: space_ref.to_string(),
                        other: child.to_string(),
                    });
                    drop_children
                        .entry(space_ref.clone())
                        .or_default()
                        .push(child.clone());
                    continue;
                }
                let child_space = resolved.expect("resolved child checked above");
                if !child_space.parent_space_refs.contains(&space.ref_id) {
                    repairs.push(WikiIndexRepair {
                        code: "knowledge.wiki_space_asymmetry".into(),
                        subject: space_ref.to_string(),
                        other: child.to_string(),
                    });
                    drop_children
                        .entry(space_ref.clone())
                        .or_default()
                        .push(child.clone());
                }
            }
            for node_ref in &space.node_refs {
                if !self.nodes.contains_key(node_ref) {
                    repairs.push(WikiIndexRepair {
                        code: "knowledge.wiki_space_missing_node".into(),
                        subject: space_ref.to_string(),
                        other: node_ref.to_string(),
                    });
                    drop_members
                        .entry(space_ref.clone())
                        .or_default()
                        .push(node_ref.clone());
                }
            }
        }
        for node in self.nodes.values() {
            let Some(space_ref) = &node.local_space_ref else {
                continue;
            };
            let repaired = match self.spaces.get(space_ref) {
                None => {
                    repairs.push(WikiIndexRepair {
                        code: "knowledge.wiki_local_space_missing".into(),
                        subject: node.ref_id.to_string(),
                        other: space_ref.to_string(),
                    });
                    true
                }
                Some(space) if space.anchor_ref.as_ref() != Some(&node.ref_id) => {
                    repairs.push(WikiIndexRepair {
                        code: "knowledge.wiki_local_space_anchor".into(),
                        subject: node.ref_id.to_string(),
                        other: space_ref.to_string(),
                    });
                    true
                }
                Some(_) => false,
            };
            if repaired {
                drop_local_wholes.push(node.ref_id.clone());
            }
        }

        for (space_ref, children) in drop_children {
            if let Some(space) = self.spaces.get_mut(&space_ref) {
                space
                    .child_space_refs
                    .retain(|child| !children.contains(child));
            }
        }
        for (space_ref, members) in drop_members {
            if let Some(space) = self.spaces.get_mut(&space_ref) {
                space.node_refs.retain(|member| !members.contains(member));
            }
        }
        for node_ref in drop_local_wholes {
            if let Some(node) = self.nodes.get_mut(&node_ref) {
                node.local_space_ref = None;
            }
        }
        repairs
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
            // Admitted aliases are discovery aids over the same ref: a query
            // matching an alias must resolve the canonical node, never a
            // second identity. Aliases ride the conventional `aliases`
            // extension (string or array of strings).
            let aliases = extension_aliases(&node.extensions);
            let searchable = if aliases.is_empty() {
                format!("{} {} {}", label, node.node_type, node.source_refs.len())
            } else {
                format!(
                    "{} {} {} {}",
                    label,
                    node.node_type,
                    node.source_refs.len(),
                    aliases.join(" ")
                )
            };
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
            // An authored edge's own link spelling (`raw_target`, display) is
            // part of its evidence: a relative link resolved to a stable
            // hashed file address must still be findable by what its author
            // actually wrote.
            let authored = extension_authored_text(&edge.extensions);
            return (
                "edge",
                edge.relation.clone(),
                format!(
                    "{} {} {} {}",
                    edge.relation, edge.from_ref, edge.to_ref, authored
                ),
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

/// Admitted aliases carried on the conventional `aliases` node/space
/// extension (a string or an array of strings). Aliases widen what a query
/// can match; they never become identity.
fn extension_aliases(
    extensions: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Vec<String> {
    match extensions.get("aliases") {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => {
            vec![value.clone()]
        }
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// The authored link spelling carried in an edge's `authored_relation`
/// evidence, when present: the raw target and its display text.
fn extension_authored_text(
    extensions: &std::collections::BTreeMap<String, serde_json::Value>,
) -> String {
    let Some(value) = extensions.get("authored_relation") else {
        return String::new();
    };
    ["raw_target", "display"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(serde_json::Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>()
        .join(" ")
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

    #[test]
    fn node_aliases_are_searchable_over_the_canonical_ref() {
        let mut subject = node("wiki:node:quay", "The Quay wall", &[], None);
        if let WikiObject::Node(value) = &mut subject {
            value.extensions.insert(
                "aliases".into(),
                serde_json::json!(["quay-wall", "harbour-quay"]),
            );
        }
        let index =
            SemanticWikiIndex::rebuild(vec![subject, node("wiki:node:other", "Beta", &[], None)])
                .unwrap();

        // The alias resolves the canonical node.
        let hits = index.search("quay-wall", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].address.as_curated().unwrap().as_str(),
            "wiki:node:quay"
        );

        // A token that only the other node carries does not match through
        // the alias-bearing node's searchable text (curated hits only; the
        // authored-source facet is a separate hit class over cited labels).
        assert_eq!(
            index
                .search("beta", 10)
                .iter()
                .filter_map(|hit| hit.address.as_curated())
                .map(|resource| resource.as_str())
                .collect::<Vec<_>>(),
            vec!["wiki:node:other"]
        );
    }

    /// The commissioning case: ONE dangling child ref must never switch off
    /// the whole knowledge faculty. The read side materialises past it with
    /// exactly one named repair while the healthy sibling stays traversable;
    /// the strict rebuild refuses with the identical first-offender error.
    #[test]
    fn one_dangling_child_degrades_the_read_index_and_strict_rebuild_still_refuses() {
        let objects = vec![
            space(
                "wiki:space:root",
                "Root",
                &[],
                &["wiki:space:ghost", "wiki:space:health"],
                &["wiki:node:a"],
                Some("wiki:node:a"),
            ),
            space(
                "wiki:space:health",
                "Health",
                &["wiki:space:root"],
                &[],
                &["wiki:node:b"],
                None,
            ),
            node("wiki:node:a", "Alpha", &["wiki:space:root"], None),
            node("wiki:node:b", "Beta", &["wiki:space:health"], None),
        ];
        let (index, repairs) = SemanticWikiIndex::rebuild_with_repairs(objects.clone()).unwrap();
        assert_eq!(
            repairs.len(),
            1,
            "one dangling ref, one repair: {repairs:?}"
        );
        assert_eq!(repairs[0].code, "knowledge.wiki_space_missing_child");
        assert_eq!(repairs[0].subject, "wiki:space:root");
        assert_eq!(repairs[0].other, "wiki:space:ghost");

        // The healthy sibling is traversable; the ghost is gone from the read
        // index, and healthy content still answers search.
        assert_eq!(
            index.subspaces(&r("wiki:space:root"), 4),
            vec![r("wiki:space:health")]
        );
        assert!(index.space(&r("wiki:space:ghost")).is_none());
        assert_eq!(
            index
                .search("Beta", 8)
                .iter()
                .filter_map(|hit| hit.address.as_curated())
                .map(|resource| resource.as_str())
                .collect::<Vec<_>>(),
            vec!["wiki:node:b"]
        );

        // Write-gate canary: strict rebuild errs, first offender, same code,
        // same details the strict walk always carried.
        let error = SemanticWikiIndex::rebuild(objects).unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_space_missing_child");
        assert_eq!(
            error.details().get("space").map(String::as_str),
            Some("wiki:space:root")
        );
        assert_eq!(
            error.details().get("child").map(String::as_str),
            Some("wiki:space:ghost")
        );
    }

    /// Every repair kind names its fault, and the absence lines dedupe to one
    /// line per distinct (code, subject, other) so a many-offender world
    /// cannot flood a reply.
    #[test]
    fn repairs_name_each_kind_and_absence_lines_dedupe() {
        let objects = vec![
            // Declares a missing child AND an unreciprocated one.
            space(
                "wiki:space:root",
                "Root",
                &[],
                &["wiki:space:ghost", "wiki:space:stranger"],
                &["wiki:node:missing-member"],
                None,
            ),
            // Exists but does not list root as its parent: asymmetry.
            space("wiki:space:stranger", "Stranger", &[], &[], &[], None),
            // Anchors a local space that does not exist.
            node(
                "wiki:node:anchor",
                "Anchor",
                &["wiki:space:root"],
                Some("wiki:space:nowhere"),
            ),
        ];
        let (index, repairs) = SemanticWikiIndex::rebuild_with_repairs(objects).unwrap();
        let codes: Vec<&str> = repairs.iter().map(|repair| repair.code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "knowledge.wiki_space_missing_child",
                "knowledge.wiki_space_asymmetry",
                "knowledge.wiki_space_missing_node",
                "knowledge.wiki_local_space_missing",
            ],
            "strict walk order: children, then members, then local wholes"
        );

        // The unreciprocated edge is dropped from the declaring parent; the
        // local whole is set aside (the reader already handles None).
        assert_eq!(
            index.space(&r("wiki:space:root")).unwrap().child_space_refs,
            Vec::<ResourceRef>::new()
        );
        assert!(index
            .node(&r("wiki:node:anchor"))
            .unwrap()
            .local_space_ref
            .is_none());
        let whole = index.local_whole(&r("wiki:node:anchor")).unwrap();
        assert!(whole.local_space.is_none());

        let mut doubled = repairs.clone();
        doubled.extend(repairs.clone());
        let lines = repair_absence_lines(&doubled);
        assert_eq!(lines.len(), repairs.len(), "one line per distinct fault");
        assert!(lines[0].contains("knowledge.wiki_space_missing_child"));
        assert!(lines[0].contains("wiki:space:root"));
        assert!(lines[0].contains("wiki:space:ghost"));
        assert!(lines[3].contains("local whole set aside"), "{lines:?}");
    }

    /// Repairs never mask canonical-corruption faults: duplicates and invalid
    /// objects stay fatal on the degrading path too.
    #[test]
    fn duplicates_stay_fatal_on_the_degrading_path() {
        let duplicated = vec![
            node("wiki:node:a", "A", &[], None),
            node("wiki:node:a", "Again", &[], None),
        ];
        assert_eq!(
            SemanticWikiIndex::rebuild_with_repairs(duplicated)
                .unwrap_err()
                .code(),
            "knowledge.wiki_duplicate_ref"
        );
    }
}
