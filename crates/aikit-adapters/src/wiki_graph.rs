//! Metadata-only graph projection of an already admitted native reading.
//! No source bytes, index writes, hidden-member expansion or inferred edges.
use aikit_core::knowledge_navigation::KnowledgeSearchHit;
use aikit_core::knowledge_source_pool::SourceMaterial;
use aikit_core::knowledge_wiki::WikiObject;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "aikit.knowledge-graph/v1";

pub fn project_graph(
    hits: &[KnowledgeSearchHit],
    objects: &[WikiObject],
    material: &[SourceMaterial],
    max_nodes: usize,
    max_edges: usize,
    absences: &[String],
) -> Value {
    let mut nodes = Vec::new();
    let mut allowed = BTreeSet::new();
    let sources: BTreeMap<_, _> = material
        .iter()
        .map(|item| (item.binding.source.as_str(), item))
        .collect();
    let objects: BTreeMap<_, _> = objects
        .iter()
        .map(|item| (item.ref_id().as_str(), item))
        .collect();
    let mut truncated = false;
    for hit in hits {
        // Edges are their own disclosures, not fake document vertices.
        if matches!(
            objects.get(hit.resource.as_str()),
            Some(WikiObject::Edge(_))
        ) {
            continue;
        }
        if allowed.contains(hit.resource.as_str()) {
            continue;
        }
        if nodes.len() == max_nodes {
            truncated = true;
            continue;
        }
        allowed.insert(hit.resource.as_str());
        let source = sources.get(hit.resource.as_str());
        let object = objects.get(hit.resource.as_str());
        let extension = match object {
            Some(WikiObject::Node(node)) => Some(&node.extensions),
            Some(WikiObject::Space(space)) => Some(&space.extensions),
            _ => None,
        };
        let revision = source
            .map(|item| item.binding.revision.to_string())
            .or_else(|| object.map(|item| item.revision().to_string()));
        let mut tags = source
            .map(|item| item.binding.tags.clone())
            .unwrap_or_default();
        let mut aliases = Vec::new();
        if let Some(source) =
            source.filter(|item| crate::wiki_document::is_markdown(item) && !item.body.is_empty())
        {
            let document = crate::markdown_document::parse_markdown_document(&source.body);
            tags.extend(document.tags);
            if let Some(value) = document.properties.get("aliases") {
                aliases = string_values(value);
            }
        }
        if let Some(extension) = extension {
            if let Some(value) = extension.get("tags") {
                tags.extend(string_values(value));
            }
            if let Some(value) = extension.get("aliases") {
                aliases.extend(string_values(value));
            }
        }
        tags.sort();
        tags.dedup();
        aliases.sort();
        aliases.dedup();
        nodes.push(json!({"resource":hit.resource,"address":source.map(|item|json!({"kind":"source","value":item.binding.source})).unwrap_or_else(||json!(hit.address)),"kind":hit.kind,"label":hit.label,"provider":hit.provider,"authority":hit.authority,"revision":revision,"tags":tags,"aliases":aliases}));
    }
    let mut edges = Vec::new();
    let mut formations = Vec::new();
    let mut add = |value: Value| {
        if edges.len() < max_edges {
            edges.push(value);
        } else {
            truncated = true;
        }
    };
    for object in objects.values() {
        match object {
            WikiObject::Edge(edge)
                if allowed.contains(edge.from_ref.as_str())
                    && allowed.contains(edge.to_ref.as_str()) =>
            {
                add(
                    json!({"from":edge.from_ref,"to":edge.to_ref,"relation":edge.relation,"reference":edge.ref_id,"origin":{"provider":"provider/semantic-wiki","lens":"semantic-wiki","authority":edge.origin,"revision":edge.revision.to_string()},"authored_relation":edge.extensions.get("authored_relation")}),
                );
            }
            WikiObject::Space(space) if allowed.contains(space.ref_id.as_str()) => {
                for (member, relation) in space
                    .node_refs
                    .iter()
                    .map(|r| (r, "member"))
                    .chain(space.child_space_refs.iter().map(|r| (r, "child-space")))
                {
                    if allowed.contains(member.as_str()) {
                        add(
                            json!({"from":space.ref_id,"to":member,"relation":relation,"containment":"encloses","origin":{"provider":"provider/semantic-wiki","lens":"semantic-wiki","authority":"authored","revision":space.revision.to_string()}}),
                        );
                    }
                }
            }
            WikiObject::Frame(frame) if allowed.contains(frame.ref_id.as_str()) => {
                for constellation in &frame.constellations {
                    // Never send a private ref, position count or label. A partial
                    // owner reading is explicitly incomplete, not a smaller form.
                    let members:Vec<_>=constellation.members.iter().filter(|m|allowed.contains(m.ref_id.as_str())).map(|m|json!({"ref":m.ref_id,"role":m.position.map(|p|p.to_string()),"conjugate":m.conjugate})).collect();
                    formations.push(json!({"ref":frame.ref_id,"revision":frame.revision,"anchor_ref":if allowed.contains(constellation.anchor_ref.as_str()){Some(&constellation.anchor_ref)}else{None},"members":members,"partial":members.len()!=constellation.members.len()}));
                }
            }
            _ => {}
        }
    }
    json!({"schema":SCHEMA,"nodes":nodes,"edges":edges,"formations":formations,"truncated":truncated,"limits":{"nodes":max_nodes,"edges":max_edges},"absences":absences,"basis":"admitted native knowledge horizon; metadata only"})
}
fn string_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}
