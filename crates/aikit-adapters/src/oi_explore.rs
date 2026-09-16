//! O:I Explore discovery materialisation (SharedField state/discovery SF4).
//!
//! The projected world's rebuildable Explore index exports a discovery seed
//! (`oi.explore-discovery/v1`, produced by O-I's surface-neutral Explore read
//! model — `shared-field/explore-surface.mjs::discoverySeed`). This adapter
//! compiles that seed into ordinary SemanticWiki objects behind the existing
//! Search/Resolve/Knowledge operations. It is deliberately not a second
//! search system, graph store or discovery ontology:
//!
//! - every entry becomes a wiki node carrying its OWN stable semantic ref —
//!   never a minted or transport-derived identity;
//! - admitted typed relations become edges (origin Compiled, explore origin
//!   preserved in the extension);
//! - the structured presentation join becomes derived `presents` edges from
//!   each WorldPresentation/Expression to the native subjects its own
//!   bindings name, with Being/Thing role and availability riding the edge
//!   extension. Roles are read only from the seed's structured bindings;
//!   labels and kinds are never evidence;
//! - SharedField membership becomes field nodes with `projected-in` edges,
//!   so a result can disclose the live field it is hosted in.
//!
//! Subject, presentation, Projection and SharedField occurrence stay
//! distinct categories over stable refs. An absent or unreadable seed is an
//! ordinary absence — nothing projected yet must not gate basic
//! addressability, and semantic/vector/graph enrichment never gates it
//! either.
//!
//! The materialisation joins the discovered wiki INSIDE the pre-existing
//! world-binding discipline: at a root register it materialises directly;
//! inside a Project context whose owner world disclosure is unavailable, the
//! enclosing withhold law clears central-adopted material including this
//! seed — an unavailable owner policy must never publish projected material
//! into a narrower context. When the world binding IS available, declared
//! exclusions keep applying per project.

use aikit_core::{
    ResourceRef, SourceRef, WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const EXPLORE_DISCOVERY_SCHEMA: &str = "oi.explore-discovery/v1";
pub const EXPLORE_PRODUCER_REF: &str = "aikit/oi-explore-discovery-materialisation/v1";
/// The extension carrying the entry's own explore facts on each node/edge.
pub const EXPLORE_EXTENSION: &str = "oi.explore/v1";
/// Derived presentation edge: a presentation names a native subject through
/// its own structured bindings. Derived, provenance-bearing, never admitted
/// relation state.
pub const RELATION_PRESENTS: &str = "oi.presentation/presents";
/// Derived membership edge: an indexed entry is hosted in one SharedField.
pub const RELATION_PROJECTED_IN: &str = "oi.explore/projected-in";

/// Where the discovery seed is expected for a Central root. The environment
/// overrides the location; an absent seed is ordinary (nothing projected).
pub fn discovery_seed_path(central_root: &Path) -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("OI_EXPLORE_DISCOVERY") {
        let path = PathBuf::from(explicit);
        return if path.is_file() { Some(path) } else { None };
    }
    let default = central_root.join("Work/O-I/site/public/data/explore-discovery.json");
    if default.is_file() {
        Some(default)
    } else {
        None
    }
}

pub struct ExploreDiscoveryReading {
    pub objects: Vec<WikiObject>,
    pub absences: Vec<String>,
}

/// Read and materialise one discovery seed file. Never throws for absent or
/// malformed content — every refusal is disclosed as an absence, and every
/// disclosed row is skipped, never silently invented.
pub fn read_explore_discovery(path: &Path) -> Result<ExploreDiscoveryReading, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{} ({error})", path.display()))?;
    let seed: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON ({error})", path.display()))?;
    Ok(materialise_explore_discovery(&seed))
}

/// Materialise an already-parsed discovery seed into wiki objects.
pub fn materialise_explore_discovery(seed: &Value) -> ExploreDiscoveryReading {
    let mut absences = Vec::new();
    let mut objects = Vec::new();
    if seed.get("schema").and_then(Value::as_str) != Some(EXPLORE_DISCOVERY_SCHEMA) {
        absences.push(format!(
            "Explore discovery seed names no {EXPLORE_DISCOVERY_SCHEMA} contract; nothing materialised"
        ));
        return ExploreDiscoveryReading { objects, absences };
    }

    let producer = ResourceRef::parse(EXPLORE_PRODUCER_REF).expect("producer ref is valid");

    // Entries: one node per stable semantic ref.
    let mut nodes: BTreeMap<String, WikiNode> = BTreeMap::new();
    if let Some(entries) = seed.get("entries").and_then(Value::as_array) {
        for entry in entries {
            let Some(reference) = entry.get("ref").and_then(Value::as_str) else {
                absences.push("Explore entry without a ref is skipped".into());
                continue;
            };
            let Ok(resource) = ResourceRef::parse(reference) else {
                absences.push(format!("Explore entry ref {reference:?} is not a valid resource ref; skipped"));
                continue;
            };
            if nodes.contains_key(reference) {
                absences.push(format!("Duplicate explore entry ref {reference}; first kept"));
                continue;
            }
            let label = entry
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or(reference);
            let kind = entry.get("kind").and_then(Value::as_str).unwrap_or("object");
            let world_ref = entry
                .get("world_ref")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut provenance = Vec::new();
            if let Some(rows) = entry.get("provenance").and_then(Value::as_array) {
                for row in rows {
                    if let Some(source) = provenance_ref(row, producer.clone()) {
                        provenance.push(source);
                    }
                }
            }
            if provenance.is_empty() {
                provenance.push(WikiProvenanceRef {
                    source_ref: SourceRef::parse(format!("explore-entry:{reference}"))
                        .expect("synthesised source ref is valid"),
                    source_revision: None,
                    producer_ref: Some(producer.clone()),
                    generation_ref: None,
                    extensions: BTreeMap::new(),
                });
            }
            let mut extensions = BTreeMap::new();
            extensions.insert(
                EXPLORE_EXTENSION.to_owned(),
                serde_json::json!({
                    "kind": kind,
                    "world_ref": world_ref,
                    "entry_revision": entry.get("revision").cloned(),
                    "aliases": entry.get("aliases").cloned().unwrap_or(Value::Array(vec![])),
                    "projection_ref": entry
                        .get("projection_ref")
                        .or_else(|| entry.pointer("/meta/projection_ref"))
                        .cloned(),
                }),
            );
            nodes.insert(
                reference.to_owned(),
                WikiNode {
                    profile: "okf-wiki/v1".into(),
                    ref_id: resource,
                    revision: 1,
                    provenance,
                    node_type: kind.to_owned(),
                    title: Some(label.to_owned()),
                    space_refs: Vec::new(),
                    source_refs: Vec::new(),
                    local_space_ref: None,
                    extensions,
                },
            );
        }
    } else {
        absences.push("Explore discovery seed carries no entries array".into());
    }

    // Admitted typed relations: the index's only relation state.
    let mut seen_edges: BTreeSet<String> = BTreeSet::new();
    if let Some(relations) = seed.get("relations").and_then(Value::as_array) {
        for relation in relations {
            let from = relation.get("from").and_then(Value::as_str).unwrap_or_default();
            let to = relation.get("to").and_then(Value::as_str).unwrap_or_default();
            let name = relation.get("relation").and_then(Value::as_str).unwrap_or_default();
            if from.is_empty() || to.is_empty() || name.is_empty() {
                absences.push("Explore relation without endpoints or a name is skipped".into());
                continue;
            }
            if !nodes.contains_key(from) || !nodes.contains_key(to) {
                absences.push(format!(
                    "Explore relation {name} names an unavailable endpoint ({from} → {to}); edge omitted"
                ));
                continue;
            }
            let edge_ref = relation
                .get("relation_ref")
                .and_then(Value::as_str)
                .unwrap_or(&format!("{from}#{name}#{to}"))
                .to_owned();
            if !seen_edges.insert(edge_ref.clone()) {
                continue;
            }
            let (Ok(from_ref), Ok(to_ref)) = (ResourceRef::parse(from), ResourceRef::parse(to))
            else {
                absences.push(format!("Explore relation {name} has an invalid endpoint ref; edge omitted"));
                continue;
            };
            let origin = relation
                .get("origin")
                .and_then(Value::as_str)
                .unwrap_or("explore")
                .to_owned();
            let mut extensions = BTreeMap::new();
            extensions.insert(
                EXPLORE_EXTENSION.to_owned(),
                serde_json::json!({ "origin": origin }),
            );
            let mut provenance = Vec::new();
            if let Some(rows) = relation.get("provenance").and_then(Value::as_array) {
                for row in rows {
                    if let Some(source) = provenance_ref(row, producer.clone()) {
                        provenance.push(source);
                    }
                }
            }
            objects.push(WikiObject::Edge(WikiEdge {
                profile: "okf-wiki/v1".into(),
                ref_id: ResourceRef::parse(&edge_ref).unwrap_or_else(|_| {
                    ResourceRef::parse(format!("explore-relation:{}", seen_edges.len()))
                        .expect("synthesised relation ref is valid")
                }),
                revision: 1,
                provenance,
                from_ref,
                to_ref,
                relation: name.to_owned(),
                origin: WikiEdgeOrigin::Compiled,
                origin_ref: Some(producer.clone()),
                extensions,
            }));
        }
    }

    // The structured presentation join: derived `presents` edges, one per
    // subject the presentation's own bindings name.
    if let Some(presentations) = seed.get("presentations").and_then(Value::as_array) {
        for presentation in presentations {
            let Some(presentation_ref) = presentation.get("presentation_ref").and_then(Value::as_str)
            else {
                absences.push("Explore presentation without a presentation_ref is skipped".into());
                continue;
            };
            // The presentation is addressable in its own right; entries that
            // already carry the ref (an Expression entry) keep their node.
            if !nodes.contains_key(presentation_ref) {
                let Ok(resource) = ResourceRef::parse(presentation_ref) else {
                    absences.push(format!(
                        "Explore presentation ref {presentation_ref:?} is not a valid resource ref; skipped"
                    ));
                    continue;
                };
                let mut extensions = BTreeMap::new();
                extensions.insert(
                    EXPLORE_EXTENSION.to_owned(),
                    serde_json::json!({
                        "kind": "world-presentation",
                        "world_ref": presentation.get("world_ref").cloned().unwrap_or(Value::Null),
                        "projection_ref": presentation.get("projection_ref").cloned(),
                        "projection_revision": presentation.get("projection_revision").cloned(),
                        "projection_state": presentation.get("projection_state").cloned(),
                    }),
                );
                nodes.insert(
                    presentation_ref.to_owned(),
                    WikiNode {
                        profile: "okf-wiki/v1".into(),
                        ref_id: resource,
                        revision: presentation
                            .get("revision")
                            .and_then(Value::as_u64)
                            .unwrap_or(1),
                        provenance: vec![WikiProvenanceRef {
                            source_ref: SourceRef::parse(format!(
                                "explore-presentation:{presentation_ref}"
                            ))
                            .expect("synthesised source ref is valid"),
                            source_revision: None,
                            producer_ref: Some(producer.clone()),
                            generation_ref: None,
                            extensions: BTreeMap::new(),
                        }],
                        node_type: "world-presentation".into(),
                        title: presentation
                            .get("title")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        space_refs: Vec::new(),
                        source_refs: Vec::new(),
                        local_space_ref: None,
                        extensions,
                    },
                );
            }
            let Ok(from_ref) = ResourceRef::parse(presentation_ref) else {
                continue;
            };
            // The presentation presents its own world whole (an Expression
            // publication's WorldPresentation names the Expression ref) and
            // each native subject its bindings name — one derived relation,
            // all endpoints existing entries.
            let mut presented: Vec<(String, Value)> = Vec::new();
            if let Some(world_ref) = presentation.get("world_ref").and_then(Value::as_str) {
                if world_ref != presentation_ref {
                    presented.push((world_ref.to_owned(), serde_json::json!({"role": Value::Null})));
                }
            }
            if let Some(subjects) = presentation.get("subjects").and_then(Value::as_array) {
                for subject in subjects {
                    if let Some(subject_ref) = subject.get("ref").and_then(Value::as_str) {
                        presented.push((subject_ref.to_owned(), subject.clone()));
                    }
                }
            }
            for (subject_ref, subject) in presented {
                if !nodes.contains_key(&subject_ref) {
                    absences.push(format!(
                        "Presentation {presentation_ref} names subject {subject_ref} with no indexed entry; presents-edge omitted"
                    ));
                    continue;
                }
                let Ok(to_ref) = ResourceRef::parse(&subject_ref) else {
                    continue;
                };
                let edge_ref = format!("{presentation_ref}#presents#{subject_ref}");
                if !seen_edges.insert(edge_ref.clone()) {
                    continue;
                }
                let mut extensions = BTreeMap::new();
                extensions.insert(
                    EXPLORE_EXTENSION.to_owned(),
                    serde_json::json!({
                        "role": subject.get("role").cloned().unwrap_or(Value::Null),
                        "availability": subject.get("availability").cloned().unwrap_or(Value::Null),
                        "projection_ref": presentation.get("projection_ref").cloned(),
                        "projection_revision": presentation.get("projection_revision").cloned(),
                        "projection_state": presentation.get("projection_state").cloned(),
                    }),
                );
                objects.push(WikiObject::Edge(WikiEdge {
                    profile: "okf-wiki/v1".into(),
                    ref_id: ResourceRef::parse(&edge_ref).expect("derived edge ref is valid"),
                    revision: 1,
                    provenance: vec![WikiProvenanceRef {
                        source_ref: SourceRef::parse(format!(
                            "explore-presentation:{presentation_ref}"
                        ))
                        .expect("synthesised source ref is valid"),
                        source_revision: None,
                        producer_ref: Some(producer.clone()),
                        generation_ref: None,
                        extensions: BTreeMap::new(),
                    }],
                    from_ref: from_ref.clone(),
                    to_ref,
                    relation: RELATION_PRESENTS.to_owned(),
                    origin: WikiEdgeOrigin::Compiled,
                    origin_ref: Some(producer.clone()),
                    extensions,
                }));
            }
        }
    }

    // SharedField membership: field nodes plus derived `projected-in` edges.
    let mut field_titles: BTreeMap<String, String> = BTreeMap::new();
    if let Some(fields) = seed.get("fields").and_then(Value::as_array) {
        for field in fields {
            if let (Some(field_ref), Some(title)) = (
                field.get("field_ref").and_then(Value::as_str),
                field.get("title").and_then(Value::as_str),
            ) {
                field_titles.insert(field_ref.to_owned(), title.to_owned());
            }
        }
    }
    let mut memberships: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if let Some(membership) = seed.get("membership") {
        if let Some(entry_fields) = membership.get("entry_fields").and_then(Value::as_object) {
            for (entry_ref, field_value) in entry_fields {
                let Some(field_ref) = field_value.as_str() else {
                    continue;
                };
                if !nodes.contains_key(entry_ref) {
                    continue;
                }
                memberships
                    .entry(field_ref.to_owned())
                    .or_default()
                    .insert(entry_ref.to_owned());
            }
        }
    }
    for (field_ref, members) in &memberships {
        let Ok(field_resource) = ResourceRef::parse(field_ref) else {
            absences.push(format!("SharedField ref {field_ref:?} is not a valid resource ref; membership omitted"));
            continue;
        };
        if !nodes.contains_key(field_ref) {
            let mut extensions = BTreeMap::new();
            extensions.insert(
                EXPLORE_EXTENSION.to_owned(),
                serde_json::json!({ "kind": "shared-field" }),
            );
            nodes.insert(
                field_ref.clone(),
                WikiNode {
                    profile: "okf-wiki/v1".into(),
                    ref_id: field_resource.clone(),
                    revision: 1,
                    provenance: vec![WikiProvenanceRef {
                        source_ref: SourceRef::parse(format!("explore-field:{field_ref}"))
                            .expect("synthesised source ref is valid"),
                        source_revision: None,
                        producer_ref: Some(producer.clone()),
                        generation_ref: None,
                        extensions: BTreeMap::new(),
                    }],
                    node_type: "shared-field".into(),
                    title: field_titles.get(field_ref).cloned(),
                    space_refs: Vec::new(),
                    source_refs: Vec::new(),
                    local_space_ref: None,
                    extensions,
                },
            );
        }
        for member in members {
            let (Ok(from_ref), Ok(to_ref)) =
                (ResourceRef::parse(member), ResourceRef::parse(field_ref))
            else {
                continue;
            };
            let edge_ref = format!("{member}#projected-in#{field_ref}");
            if !seen_edges.insert(edge_ref.clone()) {
                continue;
            }
            objects.push(WikiObject::Edge(WikiEdge {
                profile: "okf-wiki/v1".into(),
                ref_id: ResourceRef::parse(&edge_ref).expect("derived edge ref is valid"),
                revision: 1,
                provenance: vec![WikiProvenanceRef {
                    source_ref: SourceRef::parse(format!("explore-field:{field_ref}"))
                        .expect("synthesised source ref is valid"),
                    source_revision: None,
                    producer_ref: Some(producer.clone()),
                    generation_ref: None,
                    extensions: BTreeMap::new(),
                }],
                from_ref,
                to_ref,
                relation: RELATION_PROJECTED_IN.to_owned(),
                origin: WikiEdgeOrigin::Compiled,
                origin_ref: Some(producer.clone()),
                extensions: BTreeMap::new(),
            }));
        }
    }

    objects.extend(nodes.into_values().map(WikiObject::Node));
    ExploreDiscoveryReading { objects, absences }
}

fn provenance_ref(row: &Value, producer: ResourceRef) -> Option<WikiProvenanceRef> {
    let reference = row.get("ref").and_then(Value::as_str)?;
    let source_ref = SourceRef::parse(reference).ok()?;
    let source_revision = row
        .get("revision")
        .and_then(Value::as_str)
        .filter(|revision| !revision.trim().is_empty())
        .map(|revision| aikit_core::SemanticRevision::Text(revision.to_owned()));
    Some(WikiProvenanceRef {
        source_ref,
        source_revision,
        producer_ref: Some(producer),
        generation_ref: None,
        extensions: BTreeMap::new(),
    })
}
