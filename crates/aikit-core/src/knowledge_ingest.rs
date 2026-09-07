//! Authored-source ingestion (W10 V9.4): the essay on-ramp — the authored
//! half of W4's compiled/authored producer seam. Rooms, records,
//! `[[wikilinks]]`, tags and register frontmatter compile into Authored
//! edges/nodes resolved against record and article nodes; backlinks ride the
//! ordinary index and tags are first-class objects.
//!
//! The Return of Zero corpus is the design input and first client: records
//! declare `record_id`, `record_type`, `register`, `claim_status` (and
//! optionally `source_ids`) in frontmatter; `[[links]]` address other
//! records; the first path segment under the corpus root is the room.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::knowledge_wiki::{
    WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef, WikiSpace, OKF_WIKI_PROFILE,
};
use crate::{AikitError, ResourceRef, Result, SourceRef, SemanticRevision};

pub const INGEST_VERSION: &str = "aikit.knowledge-ingest/v1";
pub const INGEST_EXTENSION: &str = "aikit.ingest/v1";
pub const INGEST_PRODUCER_REF: &str = "aikit/authored-source-ingest/v1";
/// The authority order of the essay corpus rides provenance as the source
/// revision; registers are declared data preserved verbatim.
pub const INGEST_REGISTERS: [&str; 4] = [
    "philological-descent",
    "attested-semantic-field",
    "operational-homology",
    "poetic-phonic-reentry",
];

/// One ingestable authored record.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestedRecord {
    pub record_ref: ResourceRef,
    pub record_id: String,
    pub record_type: String,
    pub register: Option<String>,
    pub claim_status: Option<String>,
    pub tags: Vec<String>,
    pub source_ids: Vec<String>,
    pub wikilinks: Vec<String>,
    pub title: Option<String>,
    pub room: Option<String>,
    /// The content revision of the source text (FNV-1a, deterministic).
    pub content_revision: String,
}

fn content_revision(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Parse the flat frontmatter block the corpus uses (a YAML subset: `key:
/// value`, `key: [a, b]`, `key:` followed by `- item`).
pub fn strip_frontmatter(text: &str) -> (BTreeMap<String, String>, Vec<String>, &str) {
    let mut map = BTreeMap::new();
    let mut list = Vec::new();
    let Some(rest) = text.strip_prefix("---\n") else {
        return (map, list, text);
    };
    let mut lines = rest.lines();
    let mut body_start = 4usize;
    let mut in_list = false;
    let mut current_key = String::new();
    for line in lines.by_ref() {
        body_start += line.len() + 1;
        if line.trim_end() == "---" {
            break;
        }
        if let Some(item) = line.trim().strip_prefix("- ") {
            if in_list {
                list.push(item.trim().to_owned());
                continue;
            }
        }
        in_list = false;
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_owned();
            let value = value.trim();
            if value.is_empty() {
                in_list = true;
                current_key = key;
                continue;
            }
            map.insert(key, value.to_owned());
        }
    }
    // Named list keys (`source_ids:`) fold into the map as JSON-ish text.
    if !current_key.is_empty() && !list.is_empty() {
        map.insert(current_key, list.join(", "));
    }
    let body = &text[body_start..];
    (map, list, body)
}

fn parse_list(raw: Option<String>) -> Vec<String> {
    raw.map(|value| {
        let trimmed = value.trim();
        let inner = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(trimmed);
        inner
            .split(',')
            .map(|entry| entry.trim().to_owned())
            .filter(|entry| !entry.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Extract `[[link]]` targets (display text and embeds excluded).
pub fn parse_wikilinks(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(end) = rest[start..].find("]]") else {
            break;
        };
        let raw = &rest[start + 2..start + end];
        let target = raw.split('|').next().unwrap_or(raw);
        let target = target.split('#').next().unwrap_or(target);
        let target = target.trim();
        if !target.is_empty() {
            links.push(target.to_owned());
        }
        rest = &rest[start + end + 2..];
    }
    links
}

fn title_from_body(body: &str) -> Option<String> {
    body.lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line[2..].trim().to_owned())
}

/// Parse one authored record. The room is the first path segment under the
/// corpus root.
pub fn parse_ingestable_record(relative: &str, text: &str) -> Result<IngestedRecord> {
    let (front, _list, body) = strip_frontmatter(text);
    let record_id = front
        .get("record_id")
        .cloned()
        .unwrap_or_else(|| {
            std::path::Path::new(relative)
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    if record_id.is_empty() {
        return Err(AikitError::new(
            "knowledge.ingest_invalid_record",
            format!("record `{relative}` carries no record_id"),
        ));
    }
    let record_type = front
        .get("record_type")
        .cloned()
        .unwrap_or_else(|| "record".to_owned());
    let tags = parse_list(front.get("tags").cloned());
    let source_ids = parse_list(front.get("source_ids").cloned());
    let wikilinks = parse_wikilinks(text);
    let room = relative
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned);
    Ok(IngestedRecord {
        record_ref: ResourceRef::parse(format!("wiki:node:record/{record_id}"))
            .map_err(|error| AikitError::new("knowledge.ingest_invalid_record", error.to_string()))?,
        record_id,
        record_type,
        register: front.get("register").cloned(),
        claim_status: front.get("claim_status").cloned(),
        tags,
        source_ids,
        wikilinks,
        title: title_from_body(body),
        room,
        content_revision: content_revision(text.as_bytes()),
    })
}

fn ingest_provenance(record: &IngestedRecord) -> WikiProvenanceRef {
    WikiProvenanceRef {
        source_ref: SourceRef::parse(format!("central:source:corpus:{}", record.record_id))
            .expect("record source refs are valid"),
        source_revision: Some(SemanticRevision::Text(record.content_revision.clone())),
        producer_ref: Some(
            ResourceRef::parse(INGEST_PRODUCER_REF).expect("producer ref is valid"),
        ),
        generation_ref: None,
        extensions: BTreeMap::new(),
    }
}

fn ingest_extension(record: &IngestedRecord) -> BTreeMap<String, Value> {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        INGEST_EXTENSION.to_owned(),
        json!({
            "record_id": record.record_id,
            "register": record.register,
            "claim_status": record.claim_status,
            "source_ids": record.source_ids,
            "room": record.room,
        }),
    );
    extensions
}

fn authored_edge(from: &ResourceRef, to: ResourceRef, relation: &str) -> WikiObject {
    WikiObject::Edge(WikiEdge {
        profile: OKF_WIKI_PROFILE.into(),
        ref_id: ResourceRef::parse(format!(
            "wiki:edge:{}->{to}:{relation}",
            from.as_str()
        ))
        .expect("ingest edge refs are valid"),
        revision: 1,
        provenance: Vec::new(),
        from_ref: from.clone(),
        to_ref: to,
        relation: relation.to_owned(),
        origin: WikiEdgeOrigin::Authored,
        origin_ref: Some(
            ResourceRef::parse(INGEST_PRODUCER_REF).expect("producer ref is valid"),
        ),
        extensions: BTreeMap::new(),
    })
}

/// Ingest a corpus of authored records (relative path + text). Records
/// compile first, then `[[wikilinks]]` resolve against the record set by
/// record id or title; unresolved links are disclosed, never silently
/// dropped. Tags compile as first-class nodes with Authored `tagged` edges;
/// rooms compile as spaces carrying their records.
pub fn ingest_corpus(corpus: &[(String, String)]) -> Result<(Vec<WikiObject>, Vec<String>)> {
    let mut objects = Vec::new();
    let mut absences = Vec::new();
    let mut records = Vec::new();
    for (relative, text) in corpus {
        let record = parse_ingestable_record(relative, text)?;
        records.push(record);
    }
    // Title index for link resolution (first title wins deterministically).
    let mut by_title: BTreeMap<String, ResourceRef> = BTreeMap::new();
    let mut by_id: BTreeMap<String, ResourceRef> = BTreeMap::new();
    for record in &records {
        by_id.insert(record.record_id.clone(), record.record_ref.clone());
        if let Some(title) = &record.title {
            by_title
                .entry(title.to_lowercase())
                .or_insert_with(|| record.record_ref.clone());
        }
    }

    // Room spaces: first path segment groups its records.
    let mut rooms: BTreeMap<String, Vec<ResourceRef>> = BTreeMap::new();

    let mut emitted_tags: Vec<String> = Vec::new();
    for record in &records {
        let node = WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: record.record_ref.clone(),
            revision: 1,
            provenance: vec![ingest_provenance(record)],
            node_type: record.record_type.clone(),
            title: record.title.clone().or(Some(record.record_id.clone())),
            space_refs: Vec::new(),
            source_refs: vec![
                SourceRef::parse(format!("central:source:corpus:{}", record.record_id))
                    .expect("record source refs are valid")
            ],
            local_space_ref: None,
            extensions: ingest_extension(record),
        };
        objects.push(WikiObject::Node(node));
        if let Some(room) = &record.room {
            rooms
                .entry(room.clone())
                .or_default()
                .push(record.record_ref.clone());
        }
        // Tags are first-class: one tag node per declared tag across the
        // corpus, Authored edges from every record that carries it.
        for tag in &record.tags {
            let tag_ref = ResourceRef::parse(format!("wiki:node:tag/{tag}")).map_err(|error| {
                AikitError::new("knowledge.ingest_invalid_record", error.to_string())
            })?;
            if !emitted_tags.contains(tag) {
                emitted_tags.push(tag.clone());
                objects.push(WikiObject::Node(WikiNode {
                    profile: OKF_WIKI_PROFILE.into(),
                    ref_id: tag_ref.clone(),
                    revision: 1,
                    provenance: vec![ingest_provenance(record)],
                    node_type: "tag".into(),
                    title: Some(tag.clone()),
                    space_refs: Vec::new(),
                    source_refs: Vec::new(),
                    local_space_ref: None,
                    extensions: BTreeMap::new(),
                }));
            }
            objects.push(authored_edge(&record.record_ref, tag_ref, "tagged"));
        }
        // Wikilinks resolve against record ids, then titles. Repeated links
        // to the same target within one record are one relation; targets
        // that resolve to nothing are disclosed, never silently dropped.
        let mut linked: Vec<ResourceRef> = Vec::new();
        for link in &record.wikilinks {
            let target = by_id
                .get(link)
                .or_else(|| by_title.get(&link.to_lowercase()))
                .cloned();
            match target {
                Some(target) => {
                    if !linked.contains(&target) {
                        linked.push(target);
                    }
                }
                None => absences.push(format!(
                    "record {} links `{link}`, which resolves to no record; edge omitted",
                    record.record_id
                )),
            }
        }
        for target in linked {
            objects.push(authored_edge(&record.record_ref, target, "references"));
        }
    }

    for (room, members) in rooms {
        objects.push(WikiObject::Space(WikiSpace {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse(format!("wiki:space:room/{room}"))
                .map_err(|error| AikitError::new("knowledge.ingest_invalid_record", error.to_string()))?,
            revision: 1,
            provenance: Vec::new(),
            title: Some(format!("Room: {room}")),
            parent_space_refs: Vec::new(),
            child_space_refs: Vec::new(),
            node_refs: members,
            anchor_ref: None,
            extensions: BTreeMap::new(),
        }));
    }

    Ok((objects, absences))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SemanticWikiIndex;

    /// The Return of Zero corpus is the design input (W10 rev 3, V9.4):
    /// these fixtures mirror the Arbitration cluster's real frontmatter,
    /// register discipline and link shape.
    fn arbitration_corpus() -> Vec<(String, String)> {
        vec![
            (
                "etymologies/arbitration/WHOLE-FIELD.md".to_owned(),
                "---\ntitle: \"Arbitration, Hybris, Regard, and Anamnesis\"\nrecord_id: t09-whole-field\nrecord_type: etymological-cluster\nregister: operational-homology\nclaim_status: living\ntags: [arbitration, measure, return]\n---\n\n# Arbitration, Hybris, Regard, and Anamnesis\n\nThis cluster preserves [[t09-history|the lexical carrier]] and the conjugate field. The generated relations are [[t09-history]] again at the slash.\n"
                    .to_owned(),
            ),
            (
                "etymologies/arbitration/HISTORY.md".to_owned(),
                "---\ntitle: \"Arbitration — HISTORY\"\nrecord_id: t09-history\nrecord_type: etymological-history\nregister: philological-descent\nclaim_status: mixed-attested-and-source-debt\ntags: [arbitration, latin, greek]\n---\n\n# Arbitration — HISTORY\n\nThe arbiter is first witness, then judge. Cite [[A24]] for the argument.\n"
                    .to_owned(),
            ),
            (
                "arguments/A24-Arbitration-and-the-Usurpation-of-Measure.md".to_owned(),
                "---\ntitle: \"A24 — Arbitration and the Usurpation of Measure\"\nrecord_id: A24\nrecord_type: argument\nregister: episteme\nclaim_status: Argued\n---\n\n# A24\n\nThe whole field generates the relations consumed here: [[t09-whole-field]] and the criterion's conjugate [[A24p]].\n"
                    .to_owned(),
            ),
        ]
    }

    #[test]
    fn corpus_ingests_as_authored_records_with_register_frontmatter() {
        let corpus = arbitration_corpus();
        let (objects, absences) = ingest_corpus(&corpus).unwrap();
        // The one absence is honest: A24 links [[A24p]], which is not in
        // this corpus slice; the disclosure is the contract.
        assert_eq!(absences.len(), 1, "{absences:?}");
        assert!(absences[0].contains("A24p"));
        let index = SemanticWikiIndex::rebuild(objects).expect("ingested corpus rebuilds");

        let a24 = index
            .node(
                &ResourceRef::parse("wiki:node:record/A24").unwrap(),
            )
            .expect("A24 record");
        assert_eq!(a24.node_type, "argument");
        let extension = &a24.extensions[INGEST_EXTENSION];
        assert_eq!(extension["register"], "episteme");
        assert_eq!(extension["claim_status"], "Argued");
        // The authority order rides provenance: the source revision is the
        // record's content revision.
        assert!(a24.provenance[0].source_revision.is_some());

        // Room structure compiles: both arbitration records share a room.
        let room = index
            .space(
                &ResourceRef::parse("wiki:space:room/etymologies").unwrap(),
            )
            .expect("room space");
        assert_eq!(room.node_refs.len(), 2);
    }

    #[test]
    fn wikilinks_compile_as_authored_edges_resolved_against_records() {
        let corpus = arbitration_corpus();
        let (objects, absences) = ingest_corpus(&corpus).unwrap();
        assert_eq!(absences.len(), 1, "{absences:?}");
        let index = SemanticWikiIndex::rebuild(objects).unwrap();

        // Backlinks are first-class: the whole-field sees the incoming
        // authored references.
        let whole = ResourceRef::parse("wiki:node:record/t09-whole-field").unwrap();
        let backlinks = index.backlinks(&whole);
        assert!(
            backlinks
                .iter()
                .any(|neighbour| neighbour.resource.as_str() == "wiki:node:record/A24"),
            "A24 backlink is first-class"
        );
    }

    #[test]
    fn tags_are_first_class_objects_in_the_ingested_field() {
        let corpus = arbitration_corpus();
        let (objects, _absences) = ingest_corpus(&corpus).unwrap();
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        let arbitration_tag = index
            .node(
                &ResourceRef::parse("wiki:node:tag/arbitration").unwrap(),
            )
            .expect("tag node is first-class");
        assert_eq!(arbitration_tag.node_type, "tag");
        // Both arbitration records ride the tag.
        let backlinks = index.backlinks(
            &ResourceRef::parse("wiki:node:tag/arbitration").unwrap(),
        );
        assert_eq!(backlinks.len(), 2);
    }

    #[test]
    fn unresolved_links_are_disclosed_never_silently_dropped() {
        let corpus = vec![(
            "arguments/A24.md".to_owned(),
            "---\nrecord_id: A24\nrecord_type: argument\n---\n\n# A24\n\nSee [[t09-history]].\n".to_owned(),
        )];
        let (_objects, absences) = ingest_corpus(&corpus).unwrap();
        assert!(absences.iter().any(|a| a.contains("t09-history")));
    }
}
