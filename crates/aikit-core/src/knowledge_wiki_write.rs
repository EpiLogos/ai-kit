//! The write side of the `okf-wiki/v1` Agent Wiki.
//!
//! The read side treats a Wiki file as immutable input to a rebuildable index.
//! This module is the one place that turns an explicit caller intent into a new
//! canonical revision:
//!
//! ```text
//! parse → mutate in memory → validate the whole → render → caller persists atomically
//! ```
//!
//! Wiki tooling is AVAILABLE, NOT ENFORCED. Nothing here discovers a file to
//! mutate, runs on a schedule, or couples to a bootstrap process: a caller names
//! the file, the mutation is recorded as a [`WikiMutationProposal`], and the
//! post-mutation whole must validate before anything is rendered. A mutation
//! that refuses leaves the caller with nothing to write.
//!
//! Refs that resolve in a *peer* Wiki file are the federated norm, not an error:
//! a project Space points at `central:wiki:root`, and the root Space points back
//! at every project. [`WikiDocument::report`] publishes them as findings.
//! Validation still runs the strict in-bundle topology check behind
//! [`SemanticWikiIndex::rebuild`], satisfied by projecting the *minimum* peer
//! each dangling ref implies — never by rewriting authored data, and never for a
//! ref that resolves in the file, so real asymmetry still fails loudly.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::knowledge_wiki::{WikiNode, WikiObject, WikiSpace};
use crate::knowledge_wiki_index::{SemanticWikiIndex, WikiMutationProposal};
use crate::resource::ResourceRef;
use crate::{AikitError, Result};

/// The federated Space refs are ontology, so they are defined once in the
/// canonical Wiki module and re-exported here for the write path.
pub use crate::knowledge_wiki::{
    project_id_from_space_ref, project_wiki_space_ref, PROJECT_WIKI_SPACE_REF_PREFIX,
    ROOT_WIKI_SPACE_REF,
};

/// One Wiki file as canonical objects plus the document fields around them.
///
/// Only the `objects` list is a mutation's business; every other top-level field
/// is carried through verbatim so a write cannot silently widen the document.
#[derive(Debug, Clone, Default)]
pub struct WikiDocument {
    header: serde_json::Map<String, Value>,
    objects: Vec<WikiObject>,
}

/// The refs a mutation may append to or retract from a WikiSpace.
const SPACE_REF_FIELDS: [&str; 3] = ["child_space_refs", "parent_space_refs", "node_refs"];

impl WikiDocument {
    pub fn parse(input: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(input).map_err(|error| {
            AikitError::new(
                "knowledge.wiki_invalid_json",
                format!("invalid OKF Wiki JSON: {error}"),
            )
        })?;
        let mut document = value.as_object().cloned().ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_document",
                "Wiki document must be a JSON object",
            )
        })?;
        let objects = document
            .remove("objects")
            .ok_or_else(|| {
                AikitError::new(
                    "knowledge.wiki_invalid_document",
                    "Wiki document requires an `objects` list",
                )
            })?
            .as_array()
            .cloned()
            .ok_or_else(|| {
                AikitError::new(
                    "knowledge.wiki_invalid_document",
                    "Wiki document `objects` must be a list",
                )
            })?;
        let objects = objects
            .iter()
            .map(WikiObject::parse)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            header: document,
            objects,
        })
    }

    pub fn objects(&self) -> &[WikiObject] {
        &self.objects
    }

    pub fn object(&self, resource: &ResourceRef) -> Option<&WikiObject> {
        self.objects
            .iter()
            .find(|object| object.ref_id() == resource)
    }

    /// Serialize the document back to its on-disk form: pretty JSON, one trailing
    /// newline, the `object` discriminator restored per entry.
    pub fn render(&self) -> Result<String> {
        let objects = self
            .objects
            .iter()
            .map(wiki_object_value)
            .collect::<Result<Vec<_>>>()?;
        let mut document = self.header.clone();
        document.insert("objects".into(), Value::Array(objects));
        let mut rendered =
            serde_json::to_string_pretty(&Value::Object(document)).map_err(|error| {
                AikitError::new(
                    "knowledge.wiki_serialize",
                    format!("could not serialize Wiki document: {error}"),
                )
            })?;
        rendered.push('\n');
        Ok(rendered)
    }

    /// The audit view of the document as it stands.
    pub fn report(&self) -> WikiValidationReport {
        let mut spaces = 0usize;
        let mut nodes = 0usize;
        let mut edges = 0usize;
        let mut frames = 0usize;
        let mut readings = 0usize;
        let mut space_refs = BTreeSet::new();
        let mut node_refs = BTreeSet::new();
        let mut identities: BTreeMap<&str, usize> = BTreeMap::new();
        for object in &self.objects {
            match object {
                WikiObject::Space(value) => {
                    spaces += 1;
                    space_refs.insert(value.ref_id.clone());
                }
                WikiObject::Node(value) => {
                    nodes += 1;
                    node_refs.insert(value.ref_id.clone());
                }
                WikiObject::Edge(_) => edges += 1,
                WikiObject::Frame(_) => frames += 1,
                WikiObject::Reading(_) => readings += 1,
            }
            *identities.entry(object.ref_id().as_str()).or_default() += 1;
        }

        let duplicates = identities
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(resource, _)| resource.to_string())
            .collect();

        let mut dangling = Vec::new();
        for object in &self.objects {
            match object {
                WikiObject::Space(space) => {
                    for child in &space.child_space_refs {
                        if !space_refs.contains(child) {
                            dangling.push(dangling_ref("child_space_refs", &space.ref_id, child));
                        }
                    }
                    for parent in &space.parent_space_refs {
                        if !space_refs.contains(parent) {
                            dangling.push(dangling_ref("parent_space_refs", &space.ref_id, parent));
                        }
                    }
                    for node_ref in &space.node_refs {
                        if !node_refs.contains(node_ref) {
                            dangling.push(dangling_ref("node_refs", &space.ref_id, node_ref));
                        }
                    }
                }
                WikiObject::Node(node) => {
                    // A source ref names a source outside any Wiki file by
                    // definition; it is published as a dependency, not punished.
                    for source in &node.source_refs {
                        dangling.push(WikiDanglingRef {
                            field: "source_refs".to_string(),
                            from: node.ref_id.to_string(),
                            to: source.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        dangling.sort_by(|left, right| {
            left.from
                .cmp(&right.from)
                .then_with(|| left.field.cmp(&right.field))
                .then_with(|| left.to.cmp(&right.to))
        });

        let mut errors = Vec::new();
        for object in &self.objects {
            if let Err(error) = object.validate() {
                errors.push(WikiDocumentError::from(&error));
            }
        }
        let index_revision = match SemanticWikiIndex::rebuild(federated_whole(&self.objects)) {
            Ok(index) => Some(index.revision().to_string()),
            Err(error) => {
                errors.push(WikiDocumentError::from(&error));
                None
            }
        };

        WikiValidationReport {
            objects: self.objects.len(),
            spaces,
            nodes,
            edges,
            frames,
            readings,
            duplicates,
            dangling,
            errors,
            index_revision,
        }
    }

    /// The write gate. Federated refs are findings, not errors; anything the
    /// document itself gets wrong — a duplicate ref, a broken topology, an
    /// invalid object — refuses the mutation before a byte is rendered.
    pub fn validate(&self) -> Result<()> {
        let report = self.report();
        if report.errors.is_empty() {
            return Ok(());
        }
        let summary = report
            .errors
            .iter()
            .map(|error| format!("{}: {}", error.code, error.message))
            .collect::<Vec<_>>()
            .join("; ");
        Err(AikitError::new(
            "knowledge.wiki_document_invalid",
            format!("Wiki document is not internally consistent: {summary}"),
        ))
    }

    /// Add an object the document does not hold yet, at the revision it carries.
    pub fn create_object(&mut self, object: WikiObject) -> Result<WikiMutationOutcome> {
        object.validate()?;
        let index = self.index()?;
        if self.object(object.ref_id()).is_some() {
            return Err(AikitError::new(
                "knowledge.wiki_ref_exists",
                format!("Wiki object {} already exists", object.ref_id()),
            )
            .with("ref", object.ref_id().to_string()));
        }
        let proposal = index.proposal_to_upsert(&object);
        let touched = touched_object(&object, object.revision(), object.revision());
        self.objects.push(object);
        Ok(WikiMutationOutcome {
            proposals: vec![proposal],
            touched: vec![touched],
            changed: true,
            warnings: Vec::new(),
        })
    }

    /// Replace an existing object wholesale, advancing its revision by one.
    /// Canonical identity is the one thing an update may not change.
    pub fn update_object(&mut self, object: WikiObject) -> Result<WikiMutationOutcome> {
        object.validate()?;
        let index = self.index()?;
        let position = self
            .objects
            .iter()
            .position(|existing| existing.ref_id() == object.ref_id())
            .ok_or_else(|| {
                AikitError::new(
                    "knowledge.wiki_object_missing",
                    format!("Wiki object {} is not in this document", object.ref_id()),
                )
                .with("ref", object.ref_id().to_string())
            })?;
        let revision_before = self.objects[position].revision();
        let object = with_revision(object, revision_before + 1);
        let proposal = index.proposal_to_upsert(&object);
        let touched = touched_object(&object, revision_before, object.revision());
        self.objects[position] = object;
        Ok(WikiMutationOutcome {
            proposals: vec![proposal],
            touched: vec![touched],
            changed: true,
            warnings: Vec::new(),
        })
    }

    /// Whether this document holds `resource` as a full object, as opposed to
    /// merely pointing at it across a federation boundary.
    pub fn holds(&self, resource: &ResourceRef) -> bool {
        self.object(resource).is_some()
    }

    /// Make every Space in this file hold exactly the membership `node` claims
    /// in its `space_refs` — adding the claims it lacks and retracting the ones
    /// it no longer claims. A claimed Space federated from a peer file is
    /// reported, not rewritten.
    pub fn sync_space_memberships(&mut self, node: &WikiNode) -> Result<WikiMutationOutcome> {
        let index = self.index()?;
        let spaces: Vec<ResourceRef> = self
            .objects
            .iter()
            .filter_map(|object| match object {
                WikiObject::Space(space) => Some(space.ref_id.clone()),
                _ => None,
            })
            .collect();
        let mut outcome = WikiMutationOutcome::unchanged();
        for space_ref in spaces {
            let Some(position) = self.space_position(&space_ref) else {
                continue;
            };
            let claims = node.space_refs.contains(&space_ref);
            let (revision_before, holds) = {
                let WikiObject::Space(space) = &self.objects[position] else {
                    continue;
                };
                (space.revision, space.node_refs.contains(&node.ref_id))
            };
            if claims == holds {
                continue;
            }
            {
                let WikiObject::Space(space) = &mut self.objects[position] else {
                    continue;
                };
                if claims {
                    space.node_refs.push(node.ref_id.clone());
                    space.node_refs.sort();
                } else {
                    space.node_refs.retain(|ref_id| ref_id != &node.ref_id);
                }
                space.revision += 1;
            }
            let object = self.objects[position].clone();
            outcome
                .touched
                .push(touched_object(&object, revision_before, object.revision()));
            outcome.proposals.push(index.proposal_to_upsert(&object));
            outcome.changed = true;
        }
        for space_ref in &node.space_refs {
            if !self.holds(space_ref) {
                outcome.warnings.push(format!(
                    "space {space_ref} is not in this file; its node_refs was not updated"
                ));
            }
        }
        Ok(outcome)
    }

    /// Federate `child` under `parent`, on whatever side of the federation this
    /// file holds: the parent's `child_space_refs`, the child's
    /// `parent_space_refs`, or both. A peer file is never opened — ctrl's root
    /// append and a project wiki's parent line are written one side at a time,
    /// and so is this. Errors only when this file holds neither side. Idempotent:
    /// an existing link is a no-op that advances no revision.
    pub fn link(
        &mut self,
        parent: &ResourceRef,
        child: &ResourceRef,
    ) -> Result<WikiMutationOutcome> {
        if parent == child {
            return Err(AikitError::new(
                "knowledge.wiki_space_self_parent",
                format!("a Space cannot federate itself: {parent}"),
            )
            .with("space", parent.to_string()));
        }
        let holds_parent = self.holds(parent);
        let holds_child = self.holds(child);
        if !holds_parent && !holds_child {
            return Err(AikitError::new(
                "knowledge.wiki_space_missing",
                format!(
                    "neither {parent} nor {child} lives in this file; write the federation from the file that holds a side of it"
                ),
            )
            .with("parent", parent.to_string())
            .with("child", child.to_string()));
        }
        if holds_parent {
            require_space(self.objects(), parent)?;
        }
        if holds_child {
            require_space(self.objects(), child)?;
        }
        let index = self.index()?;
        let mut outcome = WikiMutationOutcome::unchanged();
        if holds_parent {
            outcome.merge(self.attach_ref(&index, "child_space_refs", parent, child));
        } else {
            outcome.warnings.push(format!(
                "space {parent} is not in this file; its child_space_refs was not updated"
            ));
        }
        if holds_child {
            outcome.merge(self.attach_ref(&index, "parent_space_refs", child, parent));
        } else {
            outcome.warnings.push(format!(
                "space {child} is not in this file; its parent_space_refs was not updated"
            ));
        }
        if !outcome.changed {
            outcome
                .warnings
                .push(format!("{child} is already federated under {parent}"));
        }
        Ok(outcome)
    }

    /// Retract `child` from the Space that federates it — the mirror of the
    /// federation append. The federating Space is the canonical root when the
    /// document holds it, otherwise the only Space listing the child. When the
    /// child is a full object of this file the whole object is removed together
    /// with every ref the document held to it; when it federates from a peer file
    /// only the ref is.
    pub fn unlink(&mut self, child: &ResourceRef) -> Result<WikiMutationOutcome> {
        let mut federating = self.objects.iter().filter_map(|object| match object {
            WikiObject::Space(space) if space.child_space_refs.contains(child) => {
                Some(space.ref_id.clone())
            }
            _ => None,
        });
        let parent = match federating.next() {
            Some(parent) if parent.as_str() == ROOT_WIKI_SPACE_REF => parent,
            Some(first) => {
                if federating.next().is_some() {
                    return Err(AikitError::new(
                        "knowledge.wiki_space_ambiguous_child",
                        format!("{child} is federated by more than one Space in this file"),
                    )
                    .with("child", child.to_string()));
                }
                first
            }
            None => return Ok(WikiMutationOutcome::unchanged()),
        };

        let index = self.index()?;
        let mut outcome = WikiMutationOutcome::unchanged();
        if let Some(removed) = self.object(child).cloned() {
            let proposal = index.proposal_to_remove(child.clone())?;
            let position = self
                .objects
                .iter()
                .position(|object| object.ref_id() == child)
                .expect("the child was resolved from this document");
            self.objects.remove(position);
            outcome.proposals.push(proposal);
            outcome.touched.push(touched_object(
                &removed,
                removed.revision(),
                removed.revision(),
            ));
            outcome.warnings.push(format!(
                "removed Wiki object {child} and every ref this document held to it"
            ));
        } else {
            let expected_revision = self.object(&parent).map_or(0, WikiObject::revision);
            outcome.proposals.push(WikiMutationProposal::Remove {
                resource: child.clone(),
                expected_revision,
            });
        }

        let holders: Vec<ResourceRef> = self
            .objects
            .iter()
            .filter_map(|object| match object {
                WikiObject::Space(space) if space_holds(space, child) => Some(space.ref_id.clone()),
                _ => None,
            })
            .collect();
        for holder in holders {
            for field in SPACE_REF_FIELDS {
                outcome.merge(self.detach_ref(&index, field, &holder, child));
            }
        }
        Ok(outcome)
    }

    fn attach_ref(
        &mut self,
        index: &SemanticWikiIndex,
        field: &'static str,
        from: &ResourceRef,
        to: &ResourceRef,
    ) -> WikiMutationOutcome {
        let Some(position) = self.space_position(from) else {
            let mut outcome = WikiMutationOutcome::unchanged();
            outcome.warnings.push(format!(
                "space {from} is not in this file; its {field} was not updated"
            ));
            return outcome;
        };
        let revision_before = self.objects[position].revision();
        {
            let WikiObject::Space(space) = &mut self.objects[position] else {
                return WikiMutationOutcome::unchanged();
            };
            let refs = space_ref_field(space, field);
            if refs.contains(to) {
                return WikiMutationOutcome::unchanged();
            }
            refs.push(to.clone());
            space.revision += 1;
        }
        let mut outcome = WikiMutationOutcome::unchanged();
        let object = self.objects[position].clone();
        outcome
            .touched
            .push(touched_object(&object, revision_before, object.revision()));
        outcome.proposals.push(index.proposal_to_upsert(&object));
        outcome.changed = true;
        outcome
    }

    fn detach_ref(
        &mut self,
        index: &SemanticWikiIndex,
        field: &'static str,
        from: &ResourceRef,
        to: &ResourceRef,
    ) -> WikiMutationOutcome {
        let Some(position) = self.space_position(from) else {
            let mut outcome = WikiMutationOutcome::unchanged();
            outcome.warnings.push(format!(
                "space {from} is not in this file; its {field} was not updated"
            ));
            return outcome;
        };
        let (revision_before, removed) = {
            let WikiObject::Space(space) = &mut self.objects[position] else {
                return WikiMutationOutcome::unchanged();
            };
            let revision_before = space.revision;
            let refs = space_ref_field(space, field);
            let length = refs.len();
            refs.retain(|ref_id| ref_id != to);
            let removed = refs.len() != length;
            if removed {
                space.revision += 1;
            }
            (revision_before, removed)
        };
        let mut outcome = WikiMutationOutcome::unchanged();
        if !removed {
            return outcome;
        }
        let object = self.objects[position].clone();
        outcome
            .touched
            .push(touched_object(&object, revision_before, object.revision()));
        outcome.proposals.push(index.proposal_to_upsert(&object));
        outcome.changed = true;
        outcome
    }

    fn space_position(&self, resource: &ResourceRef) -> Option<usize> {
        self.objects.iter().position(
            |object| matches!(object, WikiObject::Space(space) if &space.ref_id == resource),
        )
    }

    fn index(&self) -> Result<SemanticWikiIndex> {
        SemanticWikiIndex::rebuild(federated_whole(&self.objects))
    }
}

/// The one write pipeline: parse → mutate → validate the whole → render.
///
/// The caller owns the file and the atomic rename; this function owns every
/// invariant a persisted Wiki document must hold. When the closure refuses, or
/// the post-mutation whole does not validate, nothing is rendered and the caller
/// has nothing to write. Every step the closure performs is recorded in the
/// ledger, so a mutation that touches several objects leaves one audit record
/// covering all of them.
pub fn apply_wiki_mutation<F>(input: &str, mutate: F) -> Result<(String, WikiMutationOutcome)>
where
    F: FnOnce(&mut WikiDocument, &mut WikiMutationLedger) -> Result<()>,
{
    let mut document = WikiDocument::parse(input)?;
    let mut ledger = WikiMutationLedger::default();
    mutate(&mut document, &mut ledger)?;
    document.validate()?;
    Ok((document.render()?, ledger.finish()))
}

/// The audit record of a multi-step mutation.
#[derive(Debug, Clone, PartialEq)]
pub struct WikiMutationLedger {
    outcome: WikiMutationOutcome,
}

impl Default for WikiMutationLedger {
    fn default() -> Self {
        Self {
            outcome: WikiMutationOutcome::unchanged(),
        }
    }
}

impl WikiMutationLedger {
    /// Record one step. A step that changed nothing contributes only its
    /// warnings, which is how an idempotent step stays visible.
    pub fn record(&mut self, outcome: WikiMutationOutcome) {
        self.outcome.merge(outcome);
    }

    pub fn outcome(&self) -> &WikiMutationOutcome {
        &self.outcome
    }

    fn finish(self) -> WikiMutationOutcome {
        self.outcome
    }
}

/// What a mutation changed, in the vocabulary the index already speaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiMutationOutcome {
    /// The proposal record of the mutation, for audit and for a caller that
    /// promotes proposals through its own application service.
    pub proposals: Vec<WikiMutationProposal>,
    /// Every object whose revision the mutation advanced (or, for a removal, the
    /// object the mutation retracted).
    pub touched: Vec<WikiTouchedObject>,
    pub changed: bool,
    pub warnings: Vec<String>,
}

impl WikiMutationOutcome {
    fn unchanged() -> Self {
        Self {
            proposals: Vec::new(),
            touched: Vec::new(),
            changed: false,
            warnings: Vec::new(),
        }
    }

    fn merge(&mut self, other: Self) {
        self.proposals.extend(other.proposals);
        self.touched.extend(other.touched);
        self.warnings.extend(other.warnings);
        self.changed |= other.changed;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiTouchedObject {
    pub resource: String,
    pub kind: String,
    pub revision_before: u64,
    pub revision_after: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiDanglingRef {
    /// The field that carries the ref: `child_space_refs`, `parent_space_refs`,
    /// `node_refs` or `source_refs`.
    pub field: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiDocumentError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

impl From<&AikitError> for WikiDocumentError {
    fn from(error: &AikitError) -> Self {
        Self {
            code: error.code().into(),
            message: error.message().into(),
            details: error.details().clone(),
        }
    }
}

/// What `wiki validate` reports. `dangling` means "does not resolve inside this
/// file": the federated norm between a root and a project Wiki, published rather
/// than punished. `errors` are the hard ones — a document holding any of them
/// refuses mutations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiValidationReport {
    pub objects: usize,
    pub spaces: usize,
    pub nodes: usize,
    pub edges: usize,
    pub frames: usize,
    pub readings: usize,
    pub duplicates: Vec<String>,
    pub dangling: Vec<WikiDanglingRef>,
    pub errors: Vec<WikiDocumentError>,
    /// Content hash of the whole when the strict in-bundle rebuild succeeds.
    pub index_revision: Option<String>,
}

impl WikiValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Project the reciprocal peer for every ref that resolves in a peer Wiki file.
///
/// The strict in-bundle topology check behind [`SemanticWikiIndex::rebuild`]
/// assumes one file holds the whole graph; a federated Wiki deliberately does
/// not. Rather than rewriting authored data, each ref that resolves nowhere in
/// the file is given the minimum peer it implies, so the check can run — and
/// every ref that *does* resolve in the file is still held to full reciprocity.
fn federated_whole(objects: &[WikiObject]) -> Vec<WikiObject> {
    let mut space_refs = BTreeSet::new();
    let mut node_refs = BTreeSet::new();
    for object in objects {
        match object {
            WikiObject::Space(space) => {
                space_refs.insert(space.ref_id.clone());
            }
            WikiObject::Node(node) => {
                node_refs.insert(node.ref_id.clone());
            }
            _ => {}
        }
    }

    #[derive(Clone, Default)]
    struct SpaceGap {
        parents: BTreeSet<ResourceRef>,
        children: BTreeSet<ResourceRef>,
        anchor: Option<ResourceRef>,
    }
    let mut spaces: BTreeMap<ResourceRef, SpaceGap> = BTreeMap::new();
    let mut nodes: BTreeSet<ResourceRef> = BTreeSet::new();

    for object in objects {
        match object {
            WikiObject::Space(space) => {
                for child in &space.child_space_refs {
                    if space_refs.contains(child) {
                        continue;
                    }
                    spaces
                        .entry(child.clone())
                        .or_default()
                        .parents
                        .insert(space.ref_id.clone());
                }
                for parent in &space.parent_space_refs {
                    if space_refs.contains(parent) {
                        continue;
                    }
                    spaces
                        .entry(parent.clone())
                        .or_default()
                        .children
                        .insert(space.ref_id.clone());
                }
                for node_ref in &space.node_refs {
                    if node_refs.contains(node_ref) {
                        continue;
                    }
                    nodes.insert(node_ref.clone());
                }
            }
            WikiObject::Node(node) => {
                if let Some(local_space) = &node.local_space_ref {
                    if space_refs.contains(local_space) {
                        continue;
                    }
                    spaces
                        .entry(local_space.clone())
                        .or_default()
                        .anchor
                        .get_or_insert_with(|| node.ref_id.clone());
                }
            }
            _ => {}
        }
    }

    let node_peers: BTreeSet<ResourceRef> = nodes
        .into_iter()
        .filter(|resource| !space_refs.contains(resource))
        .collect();
    let projected_spaces = spaces
        .into_iter()
        .filter(|(resource, _)| !node_peers.contains(resource))
        .map(|(resource, gap)| {
            WikiObject::Space(WikiSpace {
                profile: crate::OKF_WIKI_PROFILE.into(),
                ref_id: resource,
                revision: 1,
                provenance: Vec::new(),
                title: None,
                parent_space_refs: gap.parents.into_iter().collect(),
                child_space_refs: gap.children.into_iter().collect(),
                node_refs: Vec::new(),
                anchor_ref: gap.anchor,
                extensions: BTreeMap::new(),
            })
        });
    let projected_nodes = node_peers.iter().cloned().map(|resource| {
        WikiObject::Node(WikiNode {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: resource,
            revision: 1,
            provenance: Vec::new(),
            node_type: "External".into(),
            title: None,
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    });
    objects
        .iter()
        .cloned()
        .chain(projected_spaces)
        .chain(projected_nodes)
        .collect()
}

/// The revision a mutation leaves behind, preserving every other field.
fn with_revision(object: WikiObject, revision: u64) -> WikiObject {
    match object {
        WikiObject::Space(mut value) => {
            value.revision = revision;
            WikiObject::Space(value)
        }
        WikiObject::Node(mut value) => {
            value.revision = revision;
            WikiObject::Node(value)
        }
        WikiObject::Edge(mut value) => {
            value.revision = revision;
            WikiObject::Edge(value)
        }
        WikiObject::Frame(mut value) => {
            value.revision = revision;
            WikiObject::Frame(value)
        }
        WikiObject::Reading(mut value) => {
            value.revision = revision;
            WikiObject::Reading(value)
        }
    }
}

fn require_space(objects: &[WikiObject], resource: &ResourceRef) -> Result<()> {
    if objects
        .iter()
        .any(|object| matches!(object, WikiObject::Space(space) if &space.ref_id == resource))
    {
        return Ok(());
    }
    Err(AikitError::new(
        "knowledge.wiki_space_missing",
        format!("WikiSpace {resource} is not in this document"),
    )
    .with("space", resource.to_string()))
}

fn space_holds(space: &WikiSpace, resource: &ResourceRef) -> bool {
    space.child_space_refs.contains(resource)
        || space.parent_space_refs.contains(resource)
        || space.node_refs.contains(resource)
}

fn space_ref_field<'a>(space: &'a mut WikiSpace, field: &str) -> &'a mut Vec<ResourceRef> {
    match field {
        "child_space_refs" => &mut space.child_space_refs,
        "parent_space_refs" => &mut space.parent_space_refs,
        "node_refs" => &mut space.node_refs,
        other => unreachable!("unknown WikiSpace ref field `{other}`"),
    }
}

fn touched_object(
    object: &WikiObject,
    revision_before: u64,
    revision_after: u64,
) -> WikiTouchedObject {
    WikiTouchedObject {
        resource: object.ref_id().to_string(),
        kind: object_kind(object).to_string(),
        revision_before,
        revision_after,
    }
}

fn dangling_ref(field: &'static str, from: &ResourceRef, to: &ResourceRef) -> WikiDanglingRef {
    WikiDanglingRef {
        field: field.to_string(),
        from: from.to_string(),
        to: to.to_string(),
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

fn wiki_object_value(object: &WikiObject) -> Result<Value> {
    let (kind, value) = match object {
        WikiObject::Space(value) => ("space", serde_json::to_value(value)),
        WikiObject::Node(value) => ("node", serde_json::to_value(value)),
        WikiObject::Edge(value) => ("edge", serde_json::to_value(value)),
        WikiObject::Frame(value) => ("frame", serde_json::to_value(value)),
        WikiObject::Reading(value) => ("reading", serde_json::to_value(value)),
    };
    let mut value = value.map_err(|error| {
        AikitError::new(
            "knowledge.wiki_serialize",
            format!("could not serialize Wiki object: {error}"),
        )
    })?;
    let map = value.as_object_mut().ok_or_else(|| {
        AikitError::new(
            "knowledge.wiki_serialize",
            "Wiki object did not serialize as an object",
        )
    })?;
    map.insert("object".into(), Value::String(kind.into()));
    Ok(Value::Object(std::mem::take(map)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_wiki::{parse_wiki_objects, WikiEdge, WikiEdgeOrigin, WikiProvenanceRef};
    use crate::resource::SourceRef;

    const ROOT: &str = "central:wiki:root";
    const PROJECT: &str = "central:wiki:project:o-i";

    fn space_json(resource: &str, parents: &[&str], children: &[&str], nodes: &[&str]) -> Value {
        serde_json::json!({
            "profile": crate::OKF_WIKI_PROFILE,
            "object": "space",
            "ref": resource,
            "revision": 4,
            "provenance": [],
            "title": resource.rsplit(':').next().unwrap_or(resource),
            "parent_space_refs": parents,
            "child_space_refs": children,
            "node_refs": nodes,
        })
    }

    fn node_json(resource: &str, title: &str, spaces: &[&str], sources: &[&str]) -> Value {
        serde_json::json!({
            "profile": crate::OKF_WIKI_PROFILE,
            "object": "node",
            "ref": resource,
            "revision": 2,
            "provenance": [],
            "type": "Concept",
            "title": title,
            "space_refs": spaces,
            "source_refs": sources,
        })
    }

    fn document_text(objects: &[Value]) -> String {
        serde_json::json!({ "profile": crate::OKF_WIKI_PROFILE, "objects": objects }).to_string()
    }

    /// A self-contained Wiki file: every ref it carries resolves inside it.
    fn self_contained_text() -> String {
        document_text(&[
            space_json(
                "wiki:space:root",
                &[],
                &["wiki:space:child"],
                &["wiki:node:a"],
            ),
            space_json("wiki:space:child", &["wiki:space:root"], &[], &[]),
            node_json(
                "wiki:node:a",
                "Origin",
                &["wiki:space:root"],
                &["source:paper:17"],
            ),
        ])
    }

    /// A federated project Wiki file: its parent lives in the root file.
    fn project_text() -> String {
        document_text(&[
            space_json(PROJECT, &[ROOT], &[], &[]),
            node_json("wiki:node:origin", "Origin", &[PROJECT], &[]),
        ])
    }

    /// The federated Central root file as ctrl writes it.
    fn root_text(children: &[&str]) -> String {
        document_text(&[space_json(ROOT, &[], children, &[])])
    }

    fn node(ref_: &str, title: &str, spaces: &[&str]) -> WikiNode {
        WikiNode {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse(ref_).unwrap(),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: SourceRef::parse("source:paper:17").unwrap(),
                source_revision: None,
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "Concept".into(),
            title: Some(title.into()),
            space_refs: spaces
                .iter()
                .map(|value| ResourceRef::parse(value).unwrap())
                .collect(),
            source_refs: vec![SourceRef::parse("source:paper:17").unwrap()],
            local_space_ref: None,
            extensions: BTreeMap::new(),
        }
    }

    fn edge(ref_: &str, from: &str, to: &str, relation: &str) -> WikiObject {
        WikiObject::Edge(WikiEdge {
            profile: crate::OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse(ref_).unwrap(),
            revision: 1,
            provenance: Vec::new(),
            from_ref: ResourceRef::parse(from).unwrap(),
            to_ref: ResourceRef::parse(to).unwrap(),
            relation: relation.into(),
            origin: WikiEdgeOrigin::Authored,
            origin_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    fn document(text: &str) -> WikiDocument {
        WikiDocument::parse(text).unwrap()
    }

    fn refs(objects: &[WikiObject]) -> Vec<String> {
        objects
            .iter()
            .map(|object| object.ref_id().to_string())
            .collect()
    }

    fn root_ref() -> ResourceRef {
        ResourceRef::parse(ROOT).unwrap()
    }

    #[test]
    fn a_node_and_edge_written_through_the_pipeline_reparse_and_reindex_identically() {
        let (rendered, outcome) = apply_wiki_mutation(&self_contained_text(), |doc, ledger| {
            let added = node(
                "wiki:node:b",
                "Target",
                &["wiki:space:root", "wiki:space:child"],
            );
            ledger.record(doc.create_object(WikiObject::Node(added.clone()))?);
            ledger.record(doc.sync_space_memberships(&added)?);
            ledger.record(doc.create_object(edge(
                "wiki:edge:a-b",
                "wiki:node:a",
                "wiki:node:b",
                "develops",
            ))?);
            Ok(())
        })
        .unwrap();

        assert!(outcome.changed);
        assert_eq!(
            outcome.proposals.len(),
            4,
            "the node, both memberships and the edge are each proposed"
        );

        let reparsed = parse_wiki_objects(&rendered).unwrap();
        assert_eq!(
            refs(&reparsed)[..3],
            refs(document(&self_contained_text()).objects())[..],
            "everything the file held is preserved in order"
        );
        assert_eq!(reparsed.len(), 5, "two Spaces, two nodes and one edge");

        let index = SemanticWikiIndex::rebuild(reparsed).unwrap();
        let neighbours = index.neighbours(&ResourceRef::parse("wiki:node:a").unwrap(), 8);
        assert_eq!(neighbours.len(), 1);
        assert_eq!(neighbours[0].relation, "develops");
        assert_eq!(neighbours[0].resource.as_str(), "wiki:node:b");
        assert_eq!(neighbours[0].origin, WikiEdgeOrigin::Authored);
        let child = index
            .space(&ResourceRef::parse("wiki:space:child").unwrap())
            .unwrap();
        assert!(child
            .node_refs
            .contains(&ResourceRef::parse("wiki:node:b").unwrap()));
    }

    #[test]
    fn a_federated_ref_is_a_finding_while_a_broken_in_file_link_is_an_error() {
        let report = document(&project_text()).report();
        assert!(
            report.is_valid(),
            "the federated parent is the norm, not an error: {:?}",
            report.errors
        );
        assert!(
            report.index_revision.is_some(),
            "the whole still rebuilds through the projected peer"
        );
        assert!(report
            .dangling
            .iter()
            .any(|dangling| dangling.field == "parent_space_refs" && dangling.to == ROOT));

        let broken = document(&document_text(&[
            space_json("wiki:space:root", &[], &["wiki:space:child"], &[]),
            space_json("wiki:space:child", &[], &[], &[]),
        ]));
        let report = broken.report();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].code, "knowledge.wiki_space_asymmetry");
        assert!(broken.validate().is_err());
    }

    #[test]
    fn validate_names_a_duplicated_ref_and_refuses_the_document() {
        let duplicated = document(&document_text(&[
            node_json("wiki:node:a", "A", &[], &[]),
            node_json("wiki:node:a", "A again", &[], &[]),
        ]));
        let report = duplicated.report();
        assert_eq!(report.duplicates, vec!["wiki:node:a".to_string()]);
        assert!(report
            .errors
            .iter()
            .any(|error| error.code == "knowledge.wiki_duplicate_ref"));
        assert!(duplicated.validate().is_err());
    }

    #[test]
    fn a_mutation_that_breaks_the_whole_renders_nothing() {
        let result = apply_wiki_mutation(&self_contained_text(), |doc, ledger| {
            let index = doc.index()?;
            ledger.record(doc.attach_ref(
                &index,
                "child_space_refs",
                &ResourceRef::parse("wiki:space:child").unwrap(),
                &ResourceRef::parse("wiki:space:root").unwrap(),
            ));
            Ok(())
        });
        let error = result.unwrap_err();
        assert_eq!(
            error.code(),
            "knowledge.wiki_document_invalid",
            "a Space federating a parent that does not federate it back is refused"
        );

        let asymmetric = document_text(&[
            space_json("wiki:space:root", &[], &["wiki:space:child"], &[]),
            space_json("wiki:space:child", &[], &[], &[]),
        ]);
        let error = apply_wiki_mutation(&asymmetric, |doc, ledger| {
            ledger.record(doc.create_object(WikiObject::Node(node("wiki:node:b", "B", &[])))?);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(
            error.code(),
            "knowledge.wiki_space_asymmetry",
            "an already-broken document refuses new work, naming what is broken"
        );
        assert_eq!(
            asymmetric,
            document_text(&[
                space_json("wiki:space:root", &[], &["wiki:space:child"], &[]),
                space_json("wiki:space:child", &[], &[], &[]),
            ]),
            "a refused mutation leaves the source text as it was"
        );
    }

    #[test]
    fn an_update_keeps_identity_and_advances_the_revision_once() {
        let mut doc = document(&self_contained_text());
        let mut body = node_json(
            "wiki:node:a",
            "Renamed",
            &["wiki:space:root"],
            &["source:paper:17"],
        );
        body["revision"] = serde_json::json!(99);
        let outcome = doc
            .update_object(WikiObject::parse(&body).unwrap())
            .unwrap();
        assert_eq!(outcome.touched[0].resource, "wiki:node:a");
        assert_eq!(outcome.touched[0].revision_before, 2);
        assert_eq!(outcome.touched[0].revision_after, 3);

        let reparsed = parse_wiki_objects(&doc.render().unwrap()).unwrap();
        assert_eq!(
            refs(&reparsed),
            refs(document(&self_contained_text()).objects())
        );
        let WikiObject::Node(node) = &reparsed[2] else {
            panic!("expected the updated node")
        };
        assert_eq!(node.title.as_deref(), Some("Renamed"));
        assert_eq!(
            node.revision, 3,
            "a caller-supplied revision cannot jump the queue"
        );
    }

    #[test]
    fn an_update_refuses_an_object_this_document_does_not_hold() {
        let mut doc = document(&self_contained_text());
        let error = doc
            .update_object(WikiObject::Node(node("wiki:node:missing", "Missing", &[])))
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_object_missing");
    }

    #[test]
    fn a_create_refuses_to_overwrite_a_ref_it_already_holds() {
        let mut doc = document(&self_contained_text());
        let error = doc
            .create_object(WikiObject::Node(node("wiki:node:a", "Again", &[])))
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_ref_exists");
    }

    #[test]
    fn attaching_a_child_twice_advances_no_revision_the_second_time() {
        let mut doc = document(&root_text(&[]));
        let child = ResourceRef::parse(PROJECT).unwrap();
        let first = doc.link(&root_ref(), &child).unwrap();
        assert!(first.changed);
        assert_eq!(first.touched[0].resource, ROOT);
        assert_eq!(first.touched[0].revision_before, 4);
        assert_eq!(first.touched[0].revision_after, 5);

        let second = doc.link(&root_ref(), &child).unwrap();
        assert!(!second.changed);
        assert_eq!(
            doc.object(&root_ref()).unwrap().revision(),
            5,
            "an existing federation is a no-op, not another revision"
        );
    }

    #[test]
    fn a_prune_removes_exactly_one_ref_and_advances_the_revision_once() {
        let text = root_text(&[PROJECT, "central:wiki:project:ai-kit"]);
        let (rendered, outcome) = apply_wiki_mutation(&text, |doc, ledger| {
            ledger.record(doc.unlink(&ResourceRef::parse(PROJECT).unwrap())?);
            Ok(())
        })
        .unwrap();

        assert!(outcome.changed);
        let pruned = document(&rendered);
        let WikiObject::Space(space) = pruned.object(&root_ref()).unwrap() else {
            panic!("expected the root Space")
        };
        assert_eq!(
            space.child_space_refs,
            vec![ResourceRef::parse("central:wiki:project:ai-kit").unwrap()],
            "exactly one ref is retracted"
        );
        assert_eq!(space.revision, 5, "one retraction, one revision");
        assert_eq!(
            outcome.proposals[0],
            WikiMutationProposal::Remove {
                resource: ResourceRef::parse(PROJECT).unwrap(),
                expected_revision: 4,
            }
        );

        let (_, second) = apply_wiki_mutation(&rendered, |doc, ledger| {
            ledger.record(doc.unlink(&ResourceRef::parse(PROJECT).unwrap())?);
            Ok(())
        })
        .unwrap();
        assert!(!second.changed, "a second prune has nothing to retract");
    }

    #[test]
    fn pruning_a_child_this_file_holds_retracts_the_object_and_every_ref_to_it() {
        let text = document_text(&[
            space_json(ROOT, &[], &["wiki:space:project"], &["wiki:node:a"]),
            space_json("wiki:space:project", &[ROOT], &[], &["wiki:node:a"]),
            node_json("wiki:node:a", "A", &["wiki:space:project"], &[]),
        ]);
        let (rendered, outcome) = apply_wiki_mutation(&text, |doc, ledger| {
            ledger.record(doc.unlink(&ResourceRef::parse("wiki:space:project").unwrap())?);
            Ok(())
        })
        .unwrap();

        let written = document(&rendered);
        assert_eq!(
            written.objects().len(),
            2,
            "the held Space object went with it"
        );
        let report = written.report();
        assert!(report.is_valid(), "{:?}", report.errors);
        let WikiObject::Space(root) = written.object(&root_ref()).unwrap() else {
            panic!("expected the root Space")
        };
        assert!(
            !root
                .child_space_refs
                .contains(&ResourceRef::parse("wiki:space:project").unwrap()),
            "the federating ref is retracted with the object"
        );
        assert_eq!(
            outcome.proposals[0],
            WikiMutationProposal::Remove {
                resource: ResourceRef::parse("wiki:space:project").unwrap(),
                expected_revision: 4,
            }
        );
    }

    #[test]
    fn a_projected_peer_is_never_persisted() {
        let (rendered, _) = apply_wiki_mutation(&project_text(), |doc, ledger| {
            let added = node("wiki:node:added", "Added", &[PROJECT]);
            ledger.record(doc.create_object(WikiObject::Node(added.clone()))?);
            ledger.record(doc.sync_space_memberships(&added)?);
            Ok(())
        })
        .unwrap();
        let written = document(&rendered);
        assert_eq!(
            written.objects().len(),
            document(&project_text()).objects().len() + 1,
            "the federated parent is projected for validation, never written"
        );
        assert!(written.report().is_valid());
    }

    #[test]
    fn project_space_refs_round_trip_through_their_project_id() {
        let space_ref = project_wiki_space_ref("o-i").unwrap();
        assert_eq!(space_ref.as_str(), PROJECT);
        assert_eq!(project_id_from_space_ref(&space_ref), Some("o-i"));
        assert_eq!(project_id_from_space_ref(&root_ref()), None);
    }

    #[test]
    fn the_central_wiki_space_refs_are_defined_once_in_the_canonical_home() {
        // The write module only re-exports the ontology; both paths name the
        // same refs, and there is no second definition to drift.
        assert_eq!(
            super::ROOT_WIKI_SPACE_REF,
            crate::knowledge_wiki::ROOT_WIKI_SPACE_REF
        );
        assert_eq!(
            super::project_wiki_space_ref("o-i").unwrap(),
            crate::knowledge_wiki::project_wiki_space_ref("o-i").unwrap()
        );
        let project = crate::knowledge_wiki::project_wiki_space_ref("o-i").unwrap();
        assert_eq!(
            crate::knowledge_wiki::project_id_from_space_ref(&project),
            Some("o-i")
        );
    }

    #[test]
    fn a_document_field_owned_by_its_producer_survives_a_write() {
        let text = serde_json::json!({
            "profile": crate::OKF_WIKI_PROFILE,
            "producer_extension": {"kept": true},
            "objects": [node_json("wiki:node:a", "A", &[], &[])],
        })
        .to_string();
        let (rendered, _) = apply_wiki_mutation(&text, |doc, ledger| {
            ledger.record(doc.create_object(WikiObject::Node(node("wiki:node:b", "B", &[])))?);
            Ok(())
        })
        .unwrap();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["producer_extension"]["kept"], serde_json::json!(true));
        assert_eq!(value["objects"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn dangling_source_refs_are_published_as_dependencies_not_errors() {
        let report = document(&self_contained_text()).report();
        assert!(report.is_valid());
        assert!(report.dangling.iter().any(|dangling| {
            dangling.field == "source_refs" && dangling.to == "source:paper:17"
        }));
    }
}
