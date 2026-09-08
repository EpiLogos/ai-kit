//! Authored-source ingestion (W10 V9.4): the essay on-ramp — the authored
//! half of W4's compiled/authored producer seam. Rooms, records, links, tags
//! and register frontmatter compile into Authored edges/nodes resolved
//! against record and article nodes; backlinks ride the ordinary index and
//! tags are first-class objects.
//!
//! The Return of Zero corpus is the design input and first client: records
//! declare `record_id`, `record_type`, `register`, `claim_status` (and
//! optionally `source_ids`) in frontmatter; the first path segment under the
//! corpus root is the room.
//!
//! ## Two link forms, one resolution
//!
//! The live corpus cites overwhelmingly through ordinary markdown links —
//! `[A24](../../arguments/A24-Arbitration-and-the-Usurpation-of-Measure.md)`
//! — and reserves `[[wikilink]]` syntax for a minority of records (chiefly
//! the protected historical carriers). Both compile the same way: a link
//! resolves against a target record_id, then a title, then the corpus-
//! relative path it names (tried both as given and relative to the citing
//! record's own directory), then finally an unambiguous bare filename stem.
//! A target that resolves through none of those is disclosed as an absence,
//! never silently dropped — see [`ingest_corpus`].
//!
//! ## Selection over a real, mixed directory
//!
//! [`ingest_corpus`] itself still takes a corpus a caller has already
//! curated to records (it falls back to a file's name as its `record_id`
//! when frontmatter declares none — the right default for hand-picked
//! fixtures). A real corpus directory is not curated: most files in it are
//! not records at all, and the same `record_id` can legitimately recur
//! across working checkpoints and snapshots of a canonical file.
//! [`select_ingestable_records`] is the filter between a raw directory walk
//! and [`ingest_corpus`]'s input — see its doc comment for the corpus
//! evidence behind the two rules it enforces.

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
    /// `[text](path.md)` targets that address another corpus record. The
    /// corpus's dominant citation form; resolved the same way wikilinks are.
    pub markdown_links: Vec<String>,
    pub title: Option<String>,
    pub room: Option<String>,
    /// The content revision of the source text (FNV-1a, deterministic).
    pub content_revision: String,
    /// The corpus-relative path this record was read from. Carried so link
    /// resolution can address a target relative to the record that cites
    /// it, the way the corpus's own relative links do.
    pub relative: String,
}

fn content_revision(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Strip one layer of matching `"..."` or `'...'` quoting. The real corpus
/// quotes scalar values freely (`claim_status: "Argued"`, `register:
/// "episteme"`); a value carried with its quote marks still attached is a
/// mis-parse, not a stylistic choice to preserve.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_owned()
    } else {
        value.to_owned()
    }
}

/// Parse the flat frontmatter block the corpus uses (a YAML subset: `key:
/// value`, `key: [a, b]`, `key:` followed by `- item`).
///
/// Every `key:` that opens a multi-line `- item` list is tracked by its own
/// key, not one shared buffer: real records carry several list-valued keys
/// in one frontmatter block (`source_ids`, `transverse_threads`, `tags`
/// side by side is the common case, not the exception), and a single shared
/// accumulator silently folds every list but the last one into whichever
/// key happened to close the block — corrupting, for example, a record's
/// `tags` with its unrelated `source_ids`. Each list is joined into `map`
/// under its own key; the returned `Vec<String>` is the last list's items,
/// kept for callers that only ever see one (single-list frontmatter, the
/// common shape outside the corpus's richer records).
pub fn strip_frontmatter(text: &str) -> (BTreeMap<String, String>, Vec<String>, &str) {
    let mut map = BTreeMap::new();
    let mut lists: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut list_order: Vec<String> = Vec::new();
    let Some(rest) = text.strip_prefix("---\n") else {
        return (map, Vec::new(), text);
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
                lists.entry(current_key.clone()).or_default().push(unquote(item));
                continue;
            }
        }
        in_list = false;
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_owned();
            let value = value.trim();
            if value.is_empty() {
                in_list = true;
                if !lists.contains_key(&key) {
                    list_order.push(key.clone());
                }
                current_key = key;
                continue;
            }
            map.insert(key, unquote(value));
        }
    }
    // Every list-valued key folds into the map under its own name, as
    // comma-joined text (the shape `parse_list` already expects).
    for key in &list_order {
        if let Some(items) = lists.get(key) {
            if !items.is_empty() {
                map.insert(key.clone(), items.join(", "));
            }
        }
    }
    let last_list = list_order
        .last()
        .and_then(|key| lists.get(key))
        .cloned()
        .unwrap_or_default();
    let body = &text[body_start..];
    (map, last_list, body)
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

/// Extract `[text](target)` markdown-link targets that address another
/// record rather than the outside world: relative paths ending `.md`
/// (an optional `#anchor` is stripped, same as a wikilink's). The real
/// corpus cites overwhelmingly through this form — `[A24](../../arguments/
/// A24-….md)` — not `[[wikilinks]]`, which it reserves for a minority of
/// records (mostly the protected historical carriers). An absolute URL
/// (`http(s)://`, `mailto:`) is not a corpus reference and is excluded.
pub fn parse_markdown_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(bracket_start) = rest.find('[') {
        // A `[[...]]` wikilink is not a markdown link; skip past both.
        if rest[bracket_start..].starts_with("[[") {
            rest = &rest[bracket_start + 2..];
            continue;
        }
        let Some(bracket_end) = rest[bracket_start..].find(']') else {
            break;
        };
        let after_bracket = bracket_start + bracket_end + 1;
        if !rest[after_bracket..].starts_with('(') {
            rest = &rest[after_bracket..];
            continue;
        }
        let paren_start = after_bracket + 1;
        let Some(paren_len) = rest[paren_start..].find(')') else {
            break;
        };
        let target = &rest[paren_start..paren_start + paren_len];
        // A title suffix (`path "Title"`) is not part of the address.
        let target = target.split_whitespace().next().unwrap_or(target);
        let target = target.split('#').next().unwrap_or(target);
        if target.ends_with(".md")
            && !target.contains("://")
            && !target.starts_with("mailto:")
        {
            links.push(target.to_owned());
        }
        rest = &rest[paren_start + paren_len + 1..];
    }
    links
}

/// The directory portion of a `/`-separated relative path (`""` at the
/// corpus root).
fn dirname(relative: &str) -> &str {
    match relative.rfind('/') {
        Some(index) => &relative[..index],
        None => "",
    }
}

/// Resolve `target` against `base_dir` the way a filesystem path resolves a
/// relative link: `..` pops a segment, `.` and empty segments vanish. Pure
/// string arithmetic — no filesystem is consulted, so a target that walks
/// above the corpus root simply loses those segments rather than erroring;
/// the resulting candidate then either matches an ingested path or it
/// doesn't.
fn resolve_relative_path(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Strip a trailing `.md` for path-key comparison; both wikilinks (which
/// usually omit it) and markdown links (which always carry it) resolve
/// against the same key space.
fn strip_md_extension(path: &str) -> &str {
    path.strip_suffix(".md").unwrap_or(path)
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
    let markdown_links = parse_markdown_links(text);
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
        markdown_links,
        title: title_from_body(body),
        room,
        content_revision: content_revision(text.as_bytes()),
        relative: relative.to_owned(),
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

/// Resolve one link target (a wikilink or a markdown-link path) against the
/// record set, trying every address form the corpus actually uses, in order
/// from most to least specific: a declared `record_id`; a record's title
/// (case-insensitive); the target read as a corpus-relative path; the same
/// target resolved relative to the directory of the record that cites it
/// (`../../arguments/A24-….md` from an etymology three levels down); and
/// finally a bare filename stem, but only when that stem names exactly one
/// record corpus-wide — an ambiguous stem resolves to nothing rather than
/// guessing.
fn resolve_link(
    record: &IngestedRecord,
    link: &str,
    by_id: &BTreeMap<String, ResourceRef>,
    by_title: &BTreeMap<String, ResourceRef>,
    by_relpath: &BTreeMap<String, ResourceRef>,
    by_filestem: &BTreeMap<String, Option<ResourceRef>>,
) -> Option<ResourceRef> {
    if let Some(target) = by_id.get(link) {
        return Some(target.clone());
    }
    if let Some(target) = by_title.get(&link.to_lowercase()) {
        return Some(target.clone());
    }
    let as_given = strip_md_extension(link);
    if let Some(target) = by_relpath.get(as_given) {
        return Some(target.clone());
    }
    let from_referrer = resolve_relative_path(dirname(&record.relative), link);
    let from_referrer = strip_md_extension(&from_referrer);
    if let Some(target) = by_relpath.get(from_referrer) {
        return Some(target.clone());
    }
    let stem = as_given.rsplit('/').next().unwrap_or(as_given);
    if let Some(Some(target)) = by_filestem.get(stem) {
        return Some(target.clone());
    }
    None
}

/// Ingest a corpus of authored records (relative path + text). Records
/// compile first, then their links — `[[wikilinks]]` and the corpus's more
/// common `[text](path.md)` markdown links alike — resolve against the
/// record set by record id, title or corpus-relative path; unresolved links
/// are disclosed, never silently dropped. Tags compile as first-class nodes
/// with Authored `tagged` edges; rooms compile as spaces carrying their
/// records.
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
    // Path indices: the corpus cites overwhelmingly by relative path, not by
    // id or title, so both a full-path index and an unambiguous-filename-stem
    // fallback are built alongside the id/title indices above.
    let mut by_relpath: BTreeMap<String, ResourceRef> = BTreeMap::new();
    let mut by_filestem: BTreeMap<String, Option<ResourceRef>> = BTreeMap::new();
    for record in &records {
        by_id.insert(record.record_id.clone(), record.record_ref.clone());
        if let Some(title) = &record.title {
            by_title
                .entry(title.to_lowercase())
                .or_insert_with(|| record.record_ref.clone());
        }
        by_relpath
            .entry(strip_md_extension(&record.relative).to_owned())
            .or_insert_with(|| record.record_ref.clone());
        let stem = strip_md_extension(&record.relative)
            .rsplit('/')
            .next()
            .unwrap_or(&record.relative)
            .to_owned();
        by_filestem
            .entry(stem)
            .and_modify(|existing| {
                if existing.as_ref() != Some(&record.record_ref) {
                    *existing = None;
                }
            })
            .or_insert_with(|| Some(record.record_ref.clone()));
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
        // Every link — `[[wikilink]]` or `[text](path.md)` alike — resolves
        // through the same address forms. Repeated links to the same target
        // within one record are one relation; targets that resolve to
        // nothing are disclosed, never silently dropped.
        let mut linked: Vec<ResourceRef> = Vec::new();
        for link in record.wikilinks.iter().chain(record.markdown_links.iter()) {
            match resolve_link(record, link, &by_id, &by_title, &by_relpath, &by_filestem) {
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

/// The subset of a raw corpus tree that is actually a record, plus the
/// honest accounting of what was set aside and why.
///
/// A real corpus directory is a mixed tree: the Return of Zero corpus is
/// ~2,600 markdown files, of which only a few hundred declare `record_id`
/// in frontmatter — the rest are READMEs, indexes, working notes and other
/// non-record prose that `ingest_corpus` was never designed to swallow
/// (its own fallback, filename-stem-as-id, exists for a corpus a caller has
/// already curated down to records; run across an *uncurated* directory it
/// would mint one fabricated record per stray file and collide constantly
/// on generic stems like `README` or `SOURCE`). Selection is the filter
/// between the two: a file counts as a record only when it declares
/// `record_id` itself.
///
/// A second real-corpus condition selection also has to name: the same
/// `record_id` can legitimately appear more than once in a directory tree —
/// a `working/…/snapshots/…/before/` checkpoint is a deliberate point-in-time
/// copy of a canonical record, kept for diffing, not a second record. The
/// first occurrence in corpus order wins (callers that want the canonical
/// copy to win pass a corpus sorted so the canonical path sorts first, which
/// a plain lexicographic sort already achieves for this corpus's own layout
/// — `submission-package/` precedes `working/`); every later occurrence is
/// named in `duplicate_record_id` rather than silently dropped or left to
/// collide downstream in `SemanticWikiIndex::rebuild`.
#[derive(Debug, Clone, Default)]
pub struct CorpusSelection {
    /// The records to hand to [`ingest_corpus`], in input order.
    pub records: Vec<(String, String)>,
    /// How many input files declared no `record_id` and were set aside
    /// (a count, not a per-file list — usually the large majority of a real
    /// tree, and itemising each one would bury the findings that matter).
    pub skipped_no_record_id: usize,
    /// One entry per later occurrence of a `record_id` already claimed by an
    /// earlier file: which id, the path that was kept, the path set aside.
    pub duplicate_record_id: Vec<String>,
    /// A file whose frontmatter could not be read as frontmatter at all
    /// (empty after the `---` fence, for example) is set aside rather than
    /// guessed at; named here, distinct from the bulk no-`record_id` count
    /// because it signals a malformed file rather than an ordinary
    /// non-record document.
    pub unparseable: Vec<String>,
}

/// Filter a raw `(relative path, text)` corpus down to the records
/// [`ingest_corpus`] should actually compile, in the input's own order (a
/// caller that wants deterministic, canonical-first selection sorts the
/// corpus before calling this, as the directory loader does).
pub fn select_ingestable_records(corpus: &[(String, String)]) -> CorpusSelection {
    let mut selection = CorpusSelection::default();
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    for (relative, text) in corpus {
        let (front, _list, _body) = strip_frontmatter(text);
        if front.is_empty() && text.starts_with("---\n") {
            selection.unparseable.push(relative.clone());
            continue;
        }
        let Some(record_id) = front.get("record_id").filter(|id| !id.trim().is_empty()) else {
            selection.skipped_no_record_id += 1;
            continue;
        };
        match claimed.get(record_id) {
            Some(kept_path) => {
                selection.duplicate_record_id.push(format!(
                    "record_id `{record_id}` is already claimed by `{kept_path}`; `{relative}` is set aside, not ingested"
                ));
            }
            None => {
                claimed.insert(record_id.clone(), relative.clone());
                selection.records.push((relative.clone(), text.clone()));
            }
        }
    }
    selection
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

    // -----------------------------------------------------------------
    // The real corpus's frontmatter: several list-valued keys side by side,
    // and quoted scalars — neither exercised by `arbitration_corpus()`
    // above, both routine in the live Return of Zero argument records.
    // -----------------------------------------------------------------

    /// `strip_frontmatter` once shared one accumulator across every
    /// multi-line list in a frontmatter block, so only the *last* list key
    /// kept its own items — every earlier list's items silently rode along
    /// into it. A real argument record's `source_ids:` and `tags:` sitting
    /// side by side (exactly `08-deferential-intelligence.md`'s shape) is
    /// the corpus condition that trips it; this pins the fix.
    #[test]
    fn multiple_list_valued_frontmatter_keys_do_not_bleed_into_each_other() {
        let text = "---\nrecord_id: A08\nrecord_type: argument\nsource_ids:\n  - mcgoohan-markstein-1967-the-prisoner\n  - taylor-2026-core-theorems-pithy\ntransverse_threads:\n  - mono-poly-two-ones\ntags:\n  - epi-logos/antikythera-essay\n---\n\n# Deferential Intelligence\n";
        let record = parse_ingestable_record("arguments/A08.md", text).unwrap();
        assert_eq!(
            record.tags,
            vec!["epi-logos/antikythera-essay".to_owned()],
            "tags must carry only its own declared items, not source_ids or transverse_threads"
        );
        assert_eq!(
            record.source_ids,
            vec![
                "mcgoohan-markstein-1967-the-prisoner".to_owned(),
                "taylor-2026-core-theorems-pithy".to_owned(),
            ]
        );
    }

    /// The real corpus quotes scalar frontmatter values freely
    /// (`claim_status: "Argued"`, `register: "episteme"`); a record's
    /// declared data must ride without the quote marks still attached.
    #[test]
    fn quoted_scalar_frontmatter_values_are_unquoted() {
        let text = "---\nrecord_id: A24\nrecord_type: argument\nregister: \"episteme\"\nclaim_status: \"Argued\"\n---\n\n# A24\n";
        let record = parse_ingestable_record("arguments/A24.md", text).unwrap();
        assert_eq!(record.register.as_deref(), Some("episteme"));
        assert_eq!(record.claim_status.as_deref(), Some("Argued"));
    }

    /// The real corpus's dominant citation form. `HISTORICAL-BRANCHES.md`
    /// and `WHOLE-FIELD.md` in the actual Arbitration cluster cite their
    /// argument and concept consumers entirely through markdown links —
    /// `[A31](../../arguments/A31-Deferential-Intelligence.md)` — never
    /// `[[wikilinks]]`. A module that only parsed `[[wikilinks]]` would find
    /// zero of this cluster's real outbound edges.
    #[test]
    fn markdown_links_compile_as_authored_edges_like_wikilinks() {
        let corpus = vec![
            (
                "symbolon/episteme/etymologies/arbitration/WHOLE-FIELD.md".to_owned(),
                "---\nrecord_id: etymology-arbitration\nrecord_type: etymology-whole\nregister: episteme\n---\n\n# Whole Field\n\nSee [A24](../../arguments/A24-Arbitration-and-the-Usurpation-of-Measure.md) and [A19](../../arguments/A19-Complex-as-Local-Arbitration-Regime.md#the-crisis).\n".to_owned(),
            ),
            (
                "symbolon/episteme/arguments/A24-Arbitration-and-the-Usurpation-of-Measure.md".to_owned(),
                "---\nrecord_id: A24\nrecord_type: argument\nregister: episteme\nclaim_status: \"Argued\"\n---\n\n# A24 — Arbitration and the Usurpation of Measure\n".to_owned(),
            ),
            (
                "symbolon/episteme/arguments/A19-Complex-as-Local-Arbitration-Regime.md".to_owned(),
                "---\nrecord_id: A19\nrecord_type: argument\nregister: episteme\n---\n\n# A19\n".to_owned(),
            ),
        ];
        let (objects, absences) = ingest_corpus(&corpus).unwrap();
        assert!(absences.is_empty(), "both markdown links resolve: {absences:?}");
        let index = SemanticWikiIndex::rebuild(objects).unwrap();

        let a24 = index.backlinks(&ResourceRef::parse("wiki:node:record/A24").unwrap());
        assert!(
            a24.iter()
                .any(|n| n.resource.as_str() == "wiki:node:record/etymology-arbitration"),
            "A24 sees the whole-field's markdown-link citation as a backlink"
        );
        let a19 = index.backlinks(&ResourceRef::parse("wiki:node:record/A19").unwrap());
        assert!(
            a19.iter()
                .any(|n| n.resource.as_str() == "wiki:node:record/etymology-arbitration"),
            "the anchored markdown link (#the-crisis) still resolves"
        );
    }

    /// A record's own directory matters: `../../arguments/A24-….md` from a
    /// file three levels down the etymologies tree only resolves once it is
    /// read relative to *that* file's directory, not the corpus root.
    #[test]
    fn relative_markdown_links_resolve_against_the_citing_records_own_directory() {
        let corpus = vec![
            (
                "symbolon/episteme/etymologies/arbitration-hybris-regard-anamnesis/HISTORICAL-BRANCHES.md"
                    .to_owned(),
                "---\nrecord_id: etymology-arbitration-historical-branches\nrecord_type: etymology-historical-branches\nregister: episteme\n---\n\n# Historical Branches\n\n[A31](../../arguments/A31-Deferential-Intelligence.md)\n"
                    .to_owned(),
            ),
            (
                "symbolon/episteme/arguments/A31-Deferential-Intelligence.md".to_owned(),
                "---\nrecord_id: A31\nrecord_type: argument\nregister: episteme\n---\n\n# A31\n".to_owned(),
            ),
        ];
        let (objects, absences) = ingest_corpus(&corpus).unwrap();
        assert!(absences.is_empty(), "{absences:?}");
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        let backlinks = index.backlinks(&ResourceRef::parse("wiki:node:record/A31").unwrap());
        assert!(backlinks
            .iter()
            .any(|n| n.resource.as_str() == "wiki:node:record/etymology-arbitration-historical-branches"));
    }

    // -----------------------------------------------------------------
    // select_ingestable_records: the mixed-tree, real-directory condition.
    // -----------------------------------------------------------------

    #[test]
    fn selection_sets_aside_files_that_declare_no_record_id() {
        let corpus = vec![
            ("README.md".to_owned(), "# Read this first\n\nNo frontmatter at all.\n".to_owned()),
            (
                "symbolon/episteme/arguments/A24-Arbitration.md".to_owned(),
                "---\nrecord_id: A24\nrecord_type: argument\n---\n\n# A24\n".to_owned(),
            ),
            (
                "symbolon/episteme/section-rooms/movements/01-immutable-gap.md".to_owned(),
                "---\ntitle: \"Immutable Gap\"\nnode_type: warrant\nclaim_status: \"Argued\"\n---\n\n# Immutable Gap\n\nNo record_id: this movement has not yet been assigned one.\n".to_owned(),
            ),
        ];
        let selection = select_ingestable_records(&corpus);
        assert_eq!(selection.records.len(), 1);
        assert_eq!(selection.records[0].0, "symbolon/episteme/arguments/A24-Arbitration.md");
        assert_eq!(selection.skipped_no_record_id, 2);
        assert!(selection.duplicate_record_id.is_empty());
    }

    /// The exact real-corpus condition: a `working/…/snapshots/…/before/`
    /// checkpoint declares the same `record_id` as its canonical
    /// `submission-package/essay/…` original (verified directly against the
    /// Antykathera-Essay-Work corpus — `etymology-arbitration-hybris-regard-
    /// anamnesis` is declared five times across the tree). The canonical
    /// copy must win, and the checkpoint must be named, not silently merged
    /// or left to collide when the index is rebuilt.
    #[test]
    fn selection_keeps_the_first_claim_to_a_record_id_and_names_the_rest() {
        let corpus = vec![
            (
                "submission-package/essay/symbolon/episteme/etymologies/arbitration-hybris-regard-anamnesis/WHOLE-FIELD.md"
                    .to_owned(),
                "---\nrecord_id: etymology-arbitration-hybris-regard-anamnesis\nrecord_type: etymology-whole\n---\n\n# Canonical\n".to_owned(),
            ),
            (
                "working/p2-enrichment/snapshots/T22-checkpoint/before/expanded-E2.md".to_owned(),
                "---\nrecord_id: etymology-arbitration-hybris-regard-anamnesis\nrecord_type: etymology-whole\n---\n\n# Snapshot copy\n".to_owned(),
            ),
        ];
        let selection = select_ingestable_records(&corpus);
        assert_eq!(selection.records.len(), 1);
        assert_eq!(
            selection.records[0].0,
            "submission-package/essay/symbolon/episteme/etymologies/arbitration-hybris-regard-anamnesis/WHOLE-FIELD.md",
            "the first-claimed (canonical, lexicographically-first) path wins"
        );
        assert_eq!(selection.duplicate_record_id.len(), 1);
        assert!(selection.duplicate_record_id[0].contains("etymology-arbitration-hybris-regard-anamnesis"));
        assert!(selection.duplicate_record_id[0].contains("before/expanded-E2.md"));
    }
}
