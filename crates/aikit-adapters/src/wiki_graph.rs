//! Metadata-only graph projection of an already admitted native reading.
//! No source bytes, index writes, hidden-member expansion or inferred edges.
use aikit_core::knowledge_navigation::KnowledgeSearchHit;
use aikit_core::knowledge_source_pool::SourceMaterial;
use aikit_core::knowledge_wiki::WikiObject;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[path = "wiki_shape_catalog.rs"]
mod authoring_forms;

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
    let source_nodes: BTreeMap<String, Value> = nodes
        .iter()
        .filter_map(|node| Some((node["resource"].as_str()?.to_owned(), node.clone())))
        .collect();
    let mut participations = BTreeMap::<String, Value>::new();
    for object in objects.values() {
        let WikiObject::Frame(frame) = object else {
            continue;
        };
        if !allowed.contains(frame.ref_id.as_str()) {
            continue;
        }
        for whole in &frame.constellations {
            for member in &whole.members {
                let Some(source) = source_nodes.get(member.ref_id.as_str()) else {
                    continue;
                };
                let Some(participation) = member
                    .extensions
                    .get("aikit.constellation-participation/v1")
                else {
                    continue;
                };
                let Some(reference) = participation["participation_ref"].as_str() else {
                    continue;
                };
                if participations.contains_key(reference) {
                    continue;
                }
                if nodes.len() >= max_nodes {
                    truncated = true;
                    continue;
                }
                let node = json!({"resource":reference,"address":source["address"],"subject_ref":member.ref_id,"kind":"constellation-member","label":source["label"],
                    "provider":"provider/semantic-wiki","authority":"authored","revision":frame.revision.to_string(),"tags":source["tags"],"aliases":source["aliases"],"frame_ref":frame.ref_id});
                participations.insert(reference.to_owned(), node.clone());
                nodes.push(node);
            }
        }
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
                let relation = edge.extensions.get("aikit.constellation-relation/v1");
                if let Some(relation) = relation {
                    if !edge
                        .origin_ref
                        .as_ref()
                        .is_some_and(|r| allowed.contains(r.as_str()))
                        || !relation["from_participation_ref"]
                            .as_str()
                            .is_some_and(|r| participations.contains_key(r))
                        || !relation["to_participation_ref"]
                            .as_str()
                            .is_some_and(|r| participations.contains_key(r))
                    {
                        continue;
                    }
                }
                let from = relation
                    .and_then(|v| v["from_participation_ref"].as_str())
                    .filter(|r| participations.contains_key(*r))
                    .unwrap_or(edge.from_ref.as_str());
                let to = relation
                    .and_then(|v| v["to_participation_ref"].as_str())
                    .filter(|r| participations.contains_key(*r))
                    .unwrap_or(edge.to_ref.as_str());
                let ql = edge.origin_ref.as_ref().and_then(|r|objects.get(r.as_str())).is_some_and(|o|matches!(o,WikiObject::Frame(frame) if frame.extensions.get("aikit.constellation/v1").and_then(|v|v.get("frame")).is_some_and(Value::is_object)));
                add(
                    json!({"from":from,"to":to,"from_subject_ref":edge.from_ref,"to_subject_ref":edge.to_ref,"relation":edge.relation,"reference":edge.ref_id,
                    "origin":{"provider":"provider/semantic-wiki","lens":"semantic-wiki","authority":edge.origin,"revision":edge.revision.to_string()},
                    "authored_relation":edge.extensions.get("authored_relation"),"family":if relation.is_some(){if ql{"ql-authored"}else{"constellation-relation"}}else if edge.extensions.contains_key("authored_relation"){"source-occurrence"}else{"native-semantic"},
                    "standing":relation.and_then(|v|v.get("standing")),"from_participation_ref":relation.and_then(|v|v.get("from_participation_ref")),"to_participation_ref":relation.and_then(|v|v.get("to_participation_ref"))}),
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
                    let form = frame
                        .extensions
                        .get("aikit.constellation/v1")
                        .and_then(|v| v.get("frame"));
                    let members:Vec<_>=constellation.members.iter().filter(|m|allowed.contains(m.ref_id.as_str())).filter(|m| {
                        // A budgeted-out contextual occurrence is a partial whole,
                        // not permission to collapse it onto its source vertex.
                        m.extensions.get("aikit.constellation-participation/v1").and_then(|v|v["participation_ref"].as_str()).is_none_or(|r|participations.contains_key(r))
                    }).map(|m| {
                        let participation=m.extensions.get("aikit.constellation-participation/v1");
                        let role_ref=participation.and_then(|v|v.get("role_ref")).and_then(Value::as_str);
                        let role=form.and_then(|v|v.get("roles")).and_then(Value::as_array).and_then(|roles|roles.iter().find(|r|r["role_ref"].as_str()==role_ref && role_ref.is_some()));
                        let reference=participation.and_then(|v|v["participation_ref"].as_str()).filter(|r|participations.contains_key(*r));
                        json!({"ref":reference.unwrap_or(m.ref_id.as_str()),"subject_ref":m.ref_id,"role":role_ref.map(str::to_owned).or_else(||m.position.map(|p|p.to_string())),"conjugate":m.conjugate,
                            "participation_ref":participation.and_then(|v|v.get("participation_ref")),"address":role.and_then(|r|r.get("address")).map(public_role_address)})
                    }).collect();
                    for member in &members {
                        add(
                            json!({"from":frame.ref_id,"to":member["ref"],"relation":"constellation-member","family":"constellation-membership","containment":"encloses",
                            "origin":{"provider":"provider/semantic-wiki","authority":"authored","revision":frame.revision.to_string()}}),
                        );
                        if member["ref"] != member["subject_ref"] {
                            add(
                                json!({"from":member["ref"],"to":member["subject_ref"],"relation":"participates-as","family":"constellation-membership",
                            "origin":{"provider":"provider/semantic-wiki","authority":"authored","revision":frame.revision.to_string()}}),
                            );
                        }
                    }
                    // Role coordinates and whole structure are native metadata;
                    // private member refs, source quotes and evidence never enter
                    // this graph projection merely to complete a shape.
                    formations.push(json!({"ref":frame.ref_id,"revision":frame.revision,
                        "shape_ref":form.and_then(|v|v.get("shape_ref")).or_else(||constellation.extensions.get("aikit.ql-shape/v1").and_then(|v|v.get("shape_ref"))),
                        "contract_ref":form.and_then(|v|v.get("contract_ref")),
                        "anchor_ref":if allowed.contains(constellation.anchor_ref.as_str()){Some(&constellation.anchor_ref)}else{None},"members":members,"partial":members.len()!=constellation.members.len()}));
                }
            }
            _ => {}
        }
    }
    json!({"schema":SCHEMA,"nodes":nodes,"edges":edges,"formations":formations,"truncated":truncated,"limits":{"nodes":max_nodes,"edges":max_edges},"absences":absences,"shape_catalog":authoring_forms::catalog(),"basis":"admitted native knowledge horizon; metadata only"})
}
fn public_role_address(value: &Value) -> Value {
    let mut address = serde_json::Map::new();
    for key in ["position", "conjugate", "face"] {
        if let Some(value) = value
            .get(key)
            .filter(|v| v.is_number() || v.is_boolean() || v.is_string())
        {
            address.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(layout) = value.get("layout") {
        let mut position = serde_json::Map::new();
        for key in ["x", "y", "z"] {
            if let Some(number) = layout[key]
                .as_f64()
                .filter(|v| v.is_finite() && v.abs() <= 100.0)
            {
                position.insert(key.to_owned(), json!(number));
            }
        }
        if position.len() == 3 {
            address.insert("layout".into(), Value::Object(position));
        }
    }
    Value::Object(address)
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

#[cfg(test)]
#[path = "wiki_graph_tests.rs"]
mod tests;
