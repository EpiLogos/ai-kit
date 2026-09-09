//! Authored-source ingestion (W10 V9.4): the essay on-ramp — the authored
//! half of W4's compiled/authored producer seam. Rooms, records, links and
//! register frontmatter compile into Authored edges/nodes resolved against
//! record and article nodes; backlinks ride the ordinary index.
//!
//! The Return of Zero corpus is the design input and first client: records
//! declare `record_id`, `record_type`, `register`, `claim_status` (and
//! optionally `source_ids`) in frontmatter; the first path segment under the
//! corpus root is the room.
//!
//! ## A tag is a property of a Source, not a curated node
//!
//! Ingest does not mint a Wiki node per declared tag. AIKit already has one
//! tag carrier — [`SourceBinding::tags`] — searched by every SourcePool
//! provider through `SourcePoolProvider::search`'s tag filter, which the
//! native baseline and the bkmr provider both implement. A second tag
//! namespace living as `wiki:node:tag/*` objects would be a parallel
//! facility with its own (arbitrary, first-declarer-wins) provenance, no
//! vocabulary discipline, and no relation to the tags the corpus's own
//! bibliography already carries. Tags therefore compile into the
//! [`SourceMaterial`] this module emits beside its Wiki objects, and reach
//! the reader through the SourcePool the rest of Knowledge already reads.
//!
//! ## Two populations, one corpus
//!
//! A real corpus tree holds two kinds of addressable file, and ingest reads
//! both:
//!
//! * a **record** declares `record_id` and compiles to a curated WikiNode;
//! * a **source** declares `source_id` and compiles to a SourcePool binding
//!   — never to a curated node. Citing a source does not curate it.
//!
//! Records cite sources two ways, and both resolve to the same stable
//! `central:source:corpus:<id>` ref: by declaring `source_ids:` in
//! frontmatter, and by ordinary markdown links into the corpus's own
//! bibliography tree. A record's effective tags are its own declared tags
//! together with the tags of every source it cites — the vocabulary is read
//! off the register and the bibliography that already author it, never
//! generated from a record's own `record_type` or `register`.
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

use crate::knowledge_source_pool::{
    SourceBinding, SourceMaterial, SourceVisibility,
};
use crate::knowledge_wiki::{
    WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef, WikiSpace, OKF_WIKI_PROFILE,
};
use crate::resource::{ResourceLocator, SourceRevision};
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

/// The stable source namespace every corpus citation addresses. A record's
/// own text and the bibliography it cites are both authored corpus material,
/// so both are addressed here; a `record_id` and a `source_id` never collide.
pub const CORPUS_SOURCE_PREFIX: &str = "central:source:corpus:";

/// The stable `SourceRef` for one corpus id.
pub fn corpus_source_ref(id: &str) -> Result<SourceRef> {
    SourceRef::parse(format!("{CORPUS_SOURCE_PREFIX}{id}")).map_err(|error| {
        AikitError::new("knowledge.ingest_invalid_record", error.to_string())
    })
}

/// One ingestable authored source: a bibliography page declaring `source_id`.
///
/// A source is evidence, not curated identity, so this compiles to a
/// [`SourceBinding`] and never to a `WikiNode`. Its `tags` are the corpus's
/// own authored vocabulary — `source-bank/...`, `argument-map/...` — and are
/// carried verbatim into the SourcePool, where every provider's tag filter
/// (native baseline and bkmr alike) can already read them.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestedSource {
    pub source_ref: SourceRef,
    pub source_id: String,
    /// What the corpus calls this kind of source (`book`, `journal-article`,
    /// `reference`, `concept`…), read from `record_type` or `node_type`.
    pub source_type: String,
    pub title: String,
    pub tags: Vec<String>,
    /// The corpus-relative path, both as the binding's locator and as the
    /// address records cite it by when they link rather than declare.
    pub relative: String,
    pub content_revision: String,
    pub body: String,
}

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
    /// `[text](path.md)` targets that address another corpus record.
    /// Resolved the same way wikilinks are; the corpus uses both forms at
    /// rough parity (see `parse_markdown_links`).
    pub markdown_links: Vec<String>,
    pub title: Option<String>,
    pub room: Option<String>,
    /// The content revision of the source text (FNV-1a, deterministic).
    pub content_revision: String,
    /// The corpus-relative path this record was read from. Carried so link
    /// resolution can address a target relative to the record that cites
    /// it, the way the corpus's own relative links do.
    pub relative: String,
    /// The record's own text, verbatim. A record is authored source material
    /// as well as curated identity, and its SourcePool binding carries this
    /// as the body a full-text or tag search actually reads.
    pub body: String,
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
/// (an optional `#anchor` is stripped, same as a wikilink's). An absolute
/// URL (`http(s)://`, `mailto:`) is not a corpus reference and is excluded.
///
/// This form and `[[wikilinks]]` are both load-bearing, at rough parity:
/// measured over the Return of Zero corpus's `episteme/arguments` records,
/// 36 of 37 files carry a markdown link and 31 carry a wikilink, with 469
/// and 489 occurrences respectively. Individual rooms lean hard either way
/// — the Arbitration cluster cites entirely through markdown links, `A04`
/// entirely through wikilinks — so neither form can be treated as the
/// exception. Parsing only one would lose about half the citation graph.
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
        body: text.to_owned(),
    })
}

/// Parse one authored source page — a corpus file declaring `source_id`.
///
/// The corpus's bibliography pages carry a fuller title in `title_full` than
/// in `title` (`title` is the short shelf label, `title_full` the cited one),
/// so the cited form wins where both are present. `record_type` names the
/// kind of source on a bibliography page; the register and reference pages
/// use `node_type` for the same purpose.
pub fn parse_ingestable_source(relative: &str, text: &str) -> Result<IngestedSource> {
    let (front, _list, body) = strip_frontmatter(text);
    let source_id = front
        .get("source_id")
        .map(String::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    if source_id.is_empty() {
        return Err(AikitError::new(
            "knowledge.ingest_invalid_source",
            format!("source `{relative}` carries no source_id"),
        ));
    }
    let title = front
        .get("title_full")
        .or_else(|| front.get("title"))
        .cloned()
        .or_else(|| title_from_body(body))
        .unwrap_or_else(|| source_id.clone());
    let source_type = front
        .get("record_type")
        .or_else(|| front.get("node_type"))
        .cloned()
        .unwrap_or_else(|| "source".to_owned());
    Ok(IngestedSource {
        source_ref: corpus_source_ref(&source_id)?,
        source_id,
        source_type,
        title,
        tags: parse_list(front.get("tags").cloned()),
        relative: relative.to_owned(),
        content_revision: content_revision(text.as_bytes()),
        body: text.to_owned(),
    })
}

/// The SourcePool binding one authored source page compiles to.
fn source_binding(source: &IngestedSource) -> Result<SourceMaterial> {
    Ok(SourceMaterial {
        binding: SourceBinding {
            source: source.source_ref.clone(),
            revision: SourceRevision::parse(&source.content_revision).map_err(|error| {
                AikitError::new("knowledge.ingest_invalid_source", error.to_string())
            })?,
            title: source.title.clone(),
            tags: source.tags.clone(),
            // Project-horizon visibility, not `Personal`. `Personal`
            // material is readable only by a named owner, and ingest has no
            // owner to name — every binding would be withheld by
            // `material_for_actor` and the whole corpus would vanish from
            // the pool without a word. `Team` is the honest reading: this is
            // the project's own authored ground, eligible to whoever holds
            // the project horizon.
            visibility: SourceVisibility::Team,
            owners: Vec::new(),
            media_type: "text/markdown".into(),
            locator: Some(ResourceLocator::Path(source.relative.clone().into())),
            metadata: BTreeMap::from([
                ("corpus_kind".to_owned(), json!("source")),
                ("source_type".to_owned(), json!(source.source_type)),
                ("relative_path".to_owned(), json!(source.relative)),
            ]),
        },
        body: source.body.clone(),
    })
}

/// The SourcePool binding one record compiles to, beside its curated node.
///
/// A record's own text is authored source material — `ingest_provenance`
/// already asserts `central:source:corpus:<record_id>` as the node's
/// provenance — so binding it here is what makes that assertion readable
/// rather than a dangling ref. Its tags are its own declared tags together
/// with the tags of every source it cites: the corpus authors its vocabulary
/// on the bibliography and register pages, and a record inherits the
/// vocabulary of what it stands on.
fn record_binding(
    record: &IngestedRecord,
    cited: &[SourceRef],
    source_tags: &BTreeMap<SourceRef, Vec<String>>,
) -> Result<SourceMaterial> {
    let mut tags = record.tags.clone();
    // `cited` is the record's whole `source_refs` list, its own ref included;
    // a record is not in `source_tags`, so its own entry contributes nothing
    // and needs no special case.
    for source in cited {
        if let Some(inherited) = source_tags.get(source) {
            for tag in inherited {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
        }
    }
    let mut metadata = BTreeMap::from([
        ("corpus_kind".to_owned(), json!("record")),
        ("record_type".to_owned(), json!(record.record_type)),
        ("relative_path".to_owned(), json!(record.relative)),
        (
            "declared_tags".to_owned(),
            json!(record.tags),
        ),
    ]);
    if let Some(register) = &record.register {
        metadata.insert("register".to_owned(), json!(register));
    }
    if let Some(claim_status) = &record.claim_status {
        metadata.insert("claim_status".to_owned(), json!(claim_status));
    }
    Ok(SourceMaterial {
        binding: SourceBinding {
            source: corpus_source_ref(&record.record_id)?,
            revision: SourceRevision::parse(&record.content_revision).map_err(|error| {
                AikitError::new("knowledge.ingest_invalid_record", error.to_string())
            })?,
            title: record
                .title
                .clone()
                .unwrap_or_else(|| record.record_id.clone()),
            tags,
            visibility: SourceVisibility::Team,
            owners: Vec::new(),
            media_type: "text/markdown".into(),
            locator: Some(ResourceLocator::Path(record.relative.clone().into())),
            metadata,
        },
        body: record.body.clone(),
    })
}

/// The authored sources a record node cites: its own text first, then the
/// bibliography it declares in `source_ids`.
///
/// Self-provenance alone was not the whole citation. A record that grounds
/// itself on Bratton or Ostrom is citing authored ground as surely as it
/// cites its own body, and leaving that out of `source_refs` left the
/// bibliography unfindable — invisible to the very search that exists to
/// surface what a curated node stands on. Both kinds address the same
/// corpus source namespace; a `record_id` and a `source_id` never collide.
///
/// Deduplicated and ordered so the same record always yields the same node.
/// Citing a source still never makes it a curated node.
///
/// `linked` carries the sources this record cited by ordinary markdown link
/// into the corpus's bibliography rather than by declaring `source_ids`.
/// Both forms are citation and both land here: over the Return of Zero
/// corpus, links into `episteme/sources/**` outnumber declared `source_ids`
/// by roughly two to one, and reading only the declared form dropped most of
/// the bibliography on the floor as unresolved links.
fn record_source_refs(record: &IngestedRecord, linked: &[SourceRef]) -> Vec<SourceRef> {
    let mut refs: Vec<SourceRef> = Vec::new();
    if let Ok(own) = corpus_source_ref(&record.record_id) {
        refs.push(own);
    }
    let mut cited: Vec<&str> = record
        .source_ids
        .iter()
        .map(String::as_str)
        .filter(|id| *id != record.record_id)
        .collect();
    cited.sort_unstable();
    cited.dedup();
    for id in cited {
        if let Ok(source) = corpus_source_ref(id) {
            if !refs.contains(&source) {
                refs.push(source);
            }
        }
    }
    for source in linked {
        if !refs.contains(source) {
            refs.push(source.clone());
        }
    }
    refs
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

/// What one ingestion run compiled: the Wiki objects, the SourcePool
/// material that carries the corpus's tags and bibliography, and the
/// absences neither could account for.
#[derive(Debug, Clone, Default)]
pub struct IngestedCorpus {
    pub objects: Vec<WikiObject>,
    /// One binding per record and per source page. This is where tags live:
    /// there is no `wiki:node:tag/*` object and no `tagged` edge.
    pub material: Vec<SourceMaterial>,
    pub absences: Vec<String>,
}

/// Ingest a corpus of authored records and the bibliography they cite.
///
/// Records compile first, then their links — `[[wikilinks]]` and the
/// corpus's more common `[text](path.md)` markdown links alike — resolve
/// against the record set by record id, title or corpus-relative path, and
/// then against the source set by the path the corpus cites it at. A link
/// that lands on a record becomes an Authored `references` edge; a link that
/// lands on a source becomes a citation in the record's `source_refs`, never
/// an edge and never a curated node. Unresolved links are disclosed, never
/// silently dropped. Rooms compile as spaces carrying their records.
///
/// Tags compile into [`SourceMaterial`], not into Wiki objects — see this
/// module's header for why.
pub fn ingest_corpus(
    records: &[(String, String)],
    sources: &[(String, String)],
    room_depth: usize,
) -> Result<IngestedCorpus> {
    let mut objects = Vec::new();
    let mut absences = Vec::new();
    let mut material = Vec::new();
    let mut parsed = Vec::new();
    for (relative, text) in records {
        parsed.push(parse_ingestable_record(relative, text)?);
    }
    let records = parsed;

    let mut parsed_sources = Vec::new();
    for (relative, text) in sources {
        parsed_sources.push(parse_ingestable_source(relative, text)?);
    }
    let sources = parsed_sources;

    // The bibliography's own indices: by declared id, by the corpus-relative
    // path records link it at, and by an unambiguous filename stem. A source
    // tree that files every entry as `SOURCE.md` makes the stem index
    // deliberately useless — which is correct: an ambiguous stem resolves to
    // nothing rather than guessing which of 139 sources was meant.
    let mut source_by_id: BTreeMap<String, SourceRef> = BTreeMap::new();
    let mut source_by_relpath: BTreeMap<String, SourceRef> = BTreeMap::new();
    let mut source_by_filestem: BTreeMap<String, Option<SourceRef>> = BTreeMap::new();
    let mut source_tags: BTreeMap<SourceRef, Vec<String>> = BTreeMap::new();
    for source in &sources {
        source_by_id.insert(source.source_id.clone(), source.source_ref.clone());
        source_by_relpath
            .entry(strip_md_extension(&source.relative).to_owned())
            .or_insert_with(|| source.source_ref.clone());
        let stem = strip_md_extension(&source.relative)
            .rsplit('/')
            .next()
            .unwrap_or(&source.relative)
            .to_owned();
        source_by_filestem
            .entry(stem)
            .and_modify(|existing| {
                if existing.as_ref() != Some(&source.source_ref) {
                    *existing = None;
                }
            })
            .or_insert_with(|| Some(source.source_ref.clone()));
        source_tags.insert(source.source_ref.clone(), source.tags.clone());
        material.push(source_binding(source)?);
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

    for record in &records {
        // Every link — `[[wikilink]]` or `[text](path.md)` alike — resolves
        // through the same address forms, against records first and then
        // against the bibliography. Repeated links to the same target within
        // one record are one relation; targets that resolve to nothing are
        // disclosed, never silently dropped.
        let mut linked: Vec<ResourceRef> = Vec::new();
        let mut cited: Vec<SourceRef> = Vec::new();
        for link in record.wikilinks.iter().chain(record.markdown_links.iter()) {
            if let Some(target) =
                resolve_link(record, link, &by_id, &by_title, &by_relpath, &by_filestem)
            {
                if !linked.contains(&target) {
                    linked.push(target);
                }
                continue;
            }
            if let Some(source) = resolve_source_link(
                record,
                link,
                &source_by_id,
                &source_by_relpath,
                &source_by_filestem,
            ) {
                if !cited.contains(&source) {
                    cited.push(source);
                }
                continue;
            }
            absences.push(format!(
                "record {} links `{link}`, which resolves to no record and no source; \
                 edge omitted",
                record.record_id
            ));
        }
        // A declared `source_ids:` entry that names no source page in this
        // corpus is a citation the reader cannot open. Ingest still asserts
        // the ref — the citation is the author's — but says so.
        for declared in &record.source_ids {
            if declared == &record.record_id {
                continue;
            }
            if !source_by_id.contains_key(declared) {
                absences.push(format!(
                    "record {} declares source_id `{declared}`, which no source page in this \
                     corpus claims; the citation is asserted but not materialised",
                    record.record_id
                ));
            }
        }

        let source_refs = record_source_refs(record, &cited);
        let node = WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: record.record_ref.clone(),
            revision: 1,
            provenance: vec![ingest_provenance(record)],
            node_type: record.record_type.clone(),
            title: record.title.clone().or(Some(record.record_id.clone())),
            space_refs: Vec::new(),
            source_refs: source_refs.clone(),
            local_space_ref: None,
            extensions: ingest_extension(record),
        };
        objects.push(WikiObject::Node(node));
        material.push(record_binding(record, &source_refs, &source_tags)?);
        if let Some(room) = room_of(&record.relative, room_depth) {
            rooms
                .entry(room)
                .or_default()
                .push(record.record_ref.clone());
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

    Ok(IngestedCorpus {
        objects,
        material,
        absences,
    })
}

/// The room a record belongs to: the first `depth` segments of its
/// corpus-relative path.
///
/// Depth is the caller's, because a room is relative to the root ingest was
/// pointed at. The Return of Zero corpus reads one way from its `symbolon/`
/// publication body — where the first segment names `episteme`, `mytheme`,
/// `matheme` — and another from the Obsidian vault root a directory above,
/// where that same structure sits one segment deeper and a depth of 1 would
/// collapse every record into a single `symbolon` room. Neither root is
/// wrong; the depth says which one is being read.
fn room_of(relative: &str, depth: usize) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let segments: Vec<&str> = relative
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    // A file needs at least one segment beyond the room, or it is not *in* a
    // room — it is a document sitting at that level.
    if segments.len() <= depth {
        return None;
    }
    Some(segments[..depth].join("/"))
}

/// Resolve one link target against the bibliography, in the same order the
/// record resolver uses: a declared `source_id`, the corpus-relative path as
/// given, that path relative to the citing record's own directory, and
/// finally an unambiguous filename stem.
fn resolve_source_link(
    record: &IngestedRecord,
    link: &str,
    by_id: &BTreeMap<String, SourceRef>,
    by_relpath: &BTreeMap<String, SourceRef>,
    by_filestem: &BTreeMap<String, Option<SourceRef>>,
) -> Option<SourceRef> {
    if let Some(source) = by_id.get(link) {
        return Some(source.clone());
    }
    let as_given = strip_md_extension(link);
    if let Some(source) = by_relpath.get(as_given) {
        return Some(source.clone());
    }
    let from_referrer = resolve_relative_path(dirname(&record.relative), link);
    let from_referrer = strip_md_extension(&from_referrer);
    if let Some(source) = by_relpath.get(from_referrer) {
        return Some(source.clone());
    }
    let stem = as_given.rsplit('/').next().unwrap_or(as_given);
    if let Some(Some(source)) = by_filestem.get(stem) {
        return Some(source.clone());
    }
    None
}

/// The addressable subset of a raw corpus tree, plus the honest accounting
/// of what was set aside and why.
///
/// A real corpus directory is a mixed tree: the Return of Zero corpus is
/// ~2,600 markdown files, of which a few hundred declare `record_id` and a
/// further couple of hundred declare `source_id`. The rest are READMEs,
/// indexes, working notes and other non-record prose that has no identity to
/// address it by. Selection is the filter between the raw walk and
/// [`ingest_corpus`]: a file counts as a record when it declares `record_id`,
/// as a source when it declares `source_id`, and otherwise is set aside.
///
/// ## Setting aside is not the same as saying nothing
///
/// This used to report the set-aside majority as a bare integer, on the
/// reasoning that itemising several hundred READMEs would bury the findings
/// that matter. That reasoning holds for inert prose and fails badly for
/// everything else: a file carrying `tags:`, `node_type:` or `aliases:` is
/// authored corpus material that ingest could not place, and a count hid
/// exactly that. It hid, in this corpus, a complete authored tag vocabulary
/// — `source-bank/...`, `argument-map/...` — sitting in files ingest walked
/// past in silence, which is how a reader came to conclude the corpus
/// declared no tags at all.
///
/// So the two are separated. Files with nothing to place are counted;
/// files carrying corpus metadata but no identity are named, with the
/// metadata they carry, so the gap is visible without the noise.
#[derive(Debug, Clone, Default)]
pub struct CorpusSelection {
    /// The records to hand to [`ingest_corpus`], in input order.
    pub records: Vec<(String, String)>,
    /// The source pages to hand to [`ingest_corpus`], in input order.
    pub sources: Vec<(String, String)>,
    /// How many input files declared no identity and carried no corpus
    /// metadata at all — ordinary prose, correctly and quietly set aside.
    pub skipped_inert: usize,
    /// One entry per file that carries corpus metadata but declares neither
    /// `record_id` nor `source_id`: authored material ingest cannot place,
    /// named rather than counted.
    pub skipped_unaddressable: Vec<String>,
    /// One entry per later occurrence of a `record_id` already claimed by an
    /// earlier file: which id, the path that was kept, the path set aside.
    pub duplicate_record_id: Vec<String>,
    /// The same accounting for a `source_id` claimed twice.
    pub duplicate_source_id: Vec<String>,
    /// A file whose frontmatter could not be read as frontmatter at all
    /// (empty after the `---` fence, for example) is set aside rather than
    /// guessed at; named here, distinct from the bulk inert count because it
    /// signals a malformed file rather than an ordinary non-record document.
    pub unparseable: Vec<String>,
}

/// The frontmatter keys that make a file authored corpus material even when
/// it declares no identity. A file carrying any of these was written into
/// the corpus's own vocabulary; setting it aside silently loses that.
const CORPUS_METADATA_KEYS: [&str; 7] = [
    "tags",
    "node_type",
    "aliases",
    "register",
    "claim_status",
    "record_type",
    "source_ids",
];

/// Filter a raw `(relative path, text)` corpus down to the records and
/// sources [`ingest_corpus`] should compile, in the input's own order (a
/// caller that wants deterministic, canonical-first selection sorts the
/// corpus before calling this, as the directory loader does).
///
/// A file declaring both identities is a record: curated identity is the
/// stronger claim, and its own text is bound as a source either way.
pub fn select_ingestable_records(corpus: &[(String, String)]) -> CorpusSelection {
    let mut selection = CorpusSelection::default();
    let mut claimed_records: BTreeMap<String, String> = BTreeMap::new();
    let mut claimed_sources: BTreeMap<String, String> = BTreeMap::new();
    for (relative, text) in corpus {
        let (front, _list, _body) = strip_frontmatter(text);
        if front.is_empty() && text.starts_with("---\n") {
            selection.unparseable.push(relative.clone());
            continue;
        }
        let record_id = front
            .get("record_id")
            .map(String::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty());
        if let Some(record_id) = record_id {
            match claimed_records.get(record_id) {
                Some(kept_path) => selection.duplicate_record_id.push(format!(
                    "record_id `{record_id}` is already claimed by `{kept_path}`; `{relative}` is set aside, not ingested"
                )),
                None => {
                    claimed_records.insert(record_id.to_owned(), relative.clone());
                    selection.records.push((relative.clone(), text.clone()));
                }
            }
            continue;
        }
        let source_id = front
            .get("source_id")
            .map(String::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty());
        if let Some(source_id) = source_id {
            match claimed_sources.get(source_id) {
                Some(kept_path) => selection.duplicate_source_id.push(format!(
                    "source_id `{source_id}` is already claimed by `{kept_path}`; `{relative}` is set aside, not ingested"
                )),
                None => {
                    claimed_sources.insert(source_id.to_owned(), relative.clone());
                    selection.sources.push((relative.clone(), text.clone()));
                }
            }
            continue;
        }
        let carried: Vec<&str> = CORPUS_METADATA_KEYS
            .iter()
            .copied()
            .filter(|key| front.contains_key(*key))
            .collect();
        if carried.is_empty() {
            selection.skipped_inert += 1;
        } else {
            selection.skipped_unaddressable.push(format!(
                "{relative}: declares {} but neither `record_id` nor `source_id`; its corpus \
                 metadata — tags included — cannot be placed",
                carried
                    .iter()
                    .map(|key| format!("`{key}:`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    selection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_source_pool::SourcePoolProvider as _;
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
        let compiled = ingest_corpus(&corpus, &[], 1).unwrap();
        // The one absence is honest: A24 links [[A24p]], which is not in
        // this corpus slice; the disclosure is the contract.
        let absences = compiled.absences;
        assert_eq!(absences.len(), 1, "{absences:?}");
        assert!(absences[0].contains("A24p"));
        let index =
            SemanticWikiIndex::rebuild(compiled.objects).expect("ingested corpus rebuilds");

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
        let compiled = ingest_corpus(&corpus, &[], 1).unwrap();
        assert_eq!(compiled.absences.len(), 1, "{:?}", compiled.absences);
        let index = SemanticWikiIndex::rebuild(compiled.objects).unwrap();

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

    /// The bibliography a record declares is authored ground it stands on,
    /// so it must reach `source_refs` — that is the field authored-source
    /// findability searches. Self-provenance alone left every cited work
    /// unfindable.
    #[test]
    fn declared_bibliography_becomes_citable_authored_sources() {
        let text = "---\nrecord_id: A25\nrecord_type: argument\nsource_ids:\n                      - bratton-2026-agentworld-brief\n  - ostrom-1990-governing-commons\n                    ---\n\n# Arbitration\n";
        let objects = ingest_corpus(&[("arguments/A25.md".into(), text.into())], &[], 1)
            .unwrap()
            .objects;

        let node = objects
            .iter()
            .find_map(|object| match object {
                WikiObject::Node(node) if node.ref_id.as_str().contains("A25") => Some(node),
                _ => None,
            })
            .expect("the record ingests as a node");

        let refs: Vec<&str> = node.source_refs.iter().map(SourceRef::as_str).collect();
        assert_eq!(
            refs,
            vec![
                "central:source:corpus:A25",
                "central:source:corpus:bratton-2026-agentworld-brief",
                "central:source:corpus:ostrom-1990-governing-commons",
            ],
            "own text first, then the declared bibliography in a stable order"
        );

        // The cited works are findable through the ordinary index, and none
        // of them became a curated node of its own.
        let index = crate::knowledge_wiki_index::SemanticWikiIndex::rebuild(objects).unwrap();
        let hits = index.search("bratton", 10);
        assert!(
            hits.iter().any(|hit| hit.address
                == crate::knowledge_wiki_index::WikiSearchAddress::AuthoredSource {
                    source: SourceRef::parse("central:source:corpus:bratton-2026-agentworld-brief")
                        .unwrap()
                }),
            "the cited work is findable as an authored source"
        );
        assert!(
            !index.contains(&ResourceRef::parse("central:source:corpus:bratton-2026-agentworld-brief").unwrap()),
            "citing a work never makes it a curated node"
        );
    }

    /// Tags ride the SourcePool, which is the one tag carrier AIKit has,
    /// and never appear as Wiki objects. A `wiki:node:tag/*` namespace was
    /// a second facility with arbitrary provenance, no vocabulary
    /// discipline, and no relation to the tags the corpus's bibliography
    /// already carried — see this module's header.
    #[test]
    fn tags_compile_into_source_material_not_into_wiki_nodes() {
        let corpus = arbitration_corpus();
        let compiled = ingest_corpus(&corpus, &[], 1).unwrap();

        assert!(
            !compiled
                .objects
                .iter()
                .any(|object| object.ref_id().as_str().starts_with("wiki:node:tag/")),
            "no tag node is minted"
        );
        assert!(
            !compiled.objects.iter().any(|object| matches!(
                object,
                WikiObject::Edge(edge) if edge.relation == "tagged"
            )),
            "no `tagged` edge is minted"
        );

        let history = compiled
            .material
            .iter()
            .find(|item| item.binding.source.as_str() == "central:source:corpus:t09-history")
            .expect("every record binds its own text as source material");
        assert_eq!(
            history.binding.tags,
            vec![
                "arbitration".to_owned(),
                "latin".to_owned(),
                "greek".to_owned()
            ],
            "the record's declared tags ride its binding verbatim"
        );

        // And the pool's own tag filter is what reads them back.
        let bindings = compiled
            .material
            .iter()
            .map(|item| item.binding.clone())
            .collect();
        let pool = crate::knowledge_source_pool::SourcePool::new("pool:test", bindings).unwrap();
        let visible = crate::knowledge_source_pool::material_for_actor(
            &pool,
            &compiled.material,
            None,
            true,
        )
        .expect("project-horizon material is eligible without a named owner");
        assert_eq!(
            visible.len(),
            compiled.material.len(),
            "no binding is silently withheld by the privacy membrane"
        );
        let mut provider = crate::knowledge_source_pool::NativeSourcePoolProvider::new();
        provider.rebuild(&visible).unwrap();
        let hits = provider
            .search(
                "",
                crate::knowledge_source_pool::SourceSearchMode::Fulltext,
                &["arbitration".to_owned()],
                10,
            )
            .unwrap();
        let tagged: Vec<&str> = hits.iter().map(|hit| hit.source.as_str()).collect();
        assert_eq!(
            tagged,
            vec![
                "central:source:corpus:t09-history",
                "central:source:corpus:t09-whole-field"
            ],
            "a tag filter alone browses the pool; both arbitration records ride the tag"
        );
    }

    /// A source page compiles to a binding, never to a curated node, and a
    /// record that cites it inherits its vocabulary. This is what "tags are
    /// picked up from the register and the sources" means concretely.
    #[test]
    fn a_cited_source_binds_its_own_tags_and_lends_them_to_its_citing_record() {
        let records = vec![(
            "arguments/A30.md".to_owned(),
            "---\nrecord_id: A30\nrecord_type: argument\nsource_ids:\n  - jung-1978-aion\n---\n\n# A30\n\nSee [the master](../episteme/sources/psychology/mcgilchrist/SOURCE.md).\n".to_owned(),
        )];
        let sources = vec![
            (
                "episteme/sources/psychology/jung/SOURCE.md".to_owned(),
                "---\nsource_id: jung-1978-aion\nnode_type: source-house\nrecord_type: book\ntitle_full: 'Aion'\ntags:\n  - source-bank/record\n  - source-bank/jung\n---\n\n# Aion\n".to_owned(),
            ),
            (
                "episteme/sources/psychology/mcgilchrist/SOURCE.md".to_owned(),
                "---\nsource_id: mcgilchrist-2009-master-emissary\nnode_type: source-house\nrecord_type: book\ntitle_full: 'The Master and His Emissary'\ntags:\n  - source-bank/record\n  - source-bank/psychology\n---\n\n# The Master\n".to_owned(),
            ),
        ];
        let compiled = ingest_corpus(&records, &sources, 1).unwrap();

        // Neither source became a curated node.
        let index = SemanticWikiIndex::rebuild(compiled.objects.clone()).unwrap();
        for source in ["jung-1978-aion", "mcgilchrist-2009-master-emissary"] {
            assert!(
                !index.contains(
                    &ResourceRef::parse(format!("central:source:corpus:{source}")).unwrap()
                ),
                "citing {source} never curates it"
            );
        }

        // The record cites both: one declared, one reached by link.
        let node = compiled
            .objects
            .iter()
            .find_map(|object| match object {
                WikiObject::Node(node) if node.ref_id.as_str().ends_with("/A30") => Some(node),
                _ => None,
            })
            .expect("the record ingests as a node");
        let refs: Vec<&str> = node.source_refs.iter().map(SourceRef::as_str).collect();
        assert_eq!(
            refs,
            vec![
                "central:source:corpus:A30",
                "central:source:corpus:jung-1978-aion",
                "central:source:corpus:mcgilchrist-2009-master-emissary",
            ],
            "a markdown link into the bibliography is a citation, not an unresolved link"
        );
        assert!(
            compiled.absences.is_empty(),
            "nothing is left unresolved: {:?}",
            compiled.absences
        );

        // And the record's binding carries the vocabulary of what it stands on.
        let binding = compiled
            .material
            .iter()
            .find(|item| item.binding.source.as_str() == "central:source:corpus:A30")
            .expect("the record binds its own text");
        assert_eq!(
            binding.binding.tags,
            vec![
                "source-bank/record".to_owned(),
                "source-bank/jung".to_owned(),
                "source-bank/psychology".to_owned(),
            ],
            "tags are inherited from the sources cited, deduplicated, in citation order"
        );
    }

    /// A declared `source_ids:` entry no source page claims is a citation
    /// the reader cannot open. It is still asserted — the citation is the
    /// author's — but it is never silently asserted.
    #[test]
    fn a_citation_with_no_source_page_is_disclosed() {
        let records = vec![(
            "arguments/A31.md".to_owned(),
            "---\nrecord_id: A31\nrecord_type: argument\nsource_ids:\n  - nobody-1900-missing\n---\n\n# A31\n".to_owned(),
        )];
        let compiled = ingest_corpus(&records, &[], 1).unwrap();
        assert!(
            compiled
                .absences
                .iter()
                .any(|absence| absence.contains("nobody-1900-missing")
                    && absence.contains("not materialised")),
            "{:?}",
            compiled.absences
        );
    }

    /// Selection used to report every non-record as one integer. That count
    /// hid this corpus's whole authored tag vocabulary: files carrying
    /// `tags:` but no identity walked past in silence, and a reader
    /// concluded the corpus declared no tags at all.
    #[test]
    fn files_carrying_corpus_metadata_are_named_not_counted() {
        let corpus = vec![
            (
                "README.md".to_owned(),
                "# Just prose\n\nNothing to place here.\n".to_owned(),
            ),
            (
                "episteme/concepts/reference-notes/blind-spot.md".to_owned(),
                "---\ntitle: 'Blind Spot'\nnode_type: reference\ntags:\n  - argument-map/reference\n---\n\n# Blind Spot\n".to_owned(),
            ),
            (
                "episteme/sources/jung/SOURCE.md".to_owned(),
                "---\nsource_id: jung-1978-aion\ntags: [source-bank/record]\n---\n\n# Aion\n".to_owned(),
            ),
            (
                "arguments/A24.md".to_owned(),
                "---\nrecord_id: A24\nrecord_type: argument\n---\n\n# A24\n".to_owned(),
            ),
        ];
        let selection = select_ingestable_records(&corpus);
        assert_eq!(selection.records.len(), 1);
        assert_eq!(selection.sources.len(), 1, "a source_id is an identity too");
        assert_eq!(selection.skipped_inert, 1, "ordinary prose is still counted");
        assert_eq!(selection.skipped_unaddressable.len(), 1);
        let disclosed = &selection.skipped_unaddressable[0];
        assert!(disclosed.contains("blind-spot.md"), "{disclosed}");
        assert!(disclosed.contains("`tags:`"), "{disclosed}");
        assert!(disclosed.contains("`node_type:`"), "{disclosed}");
    }

    /// A room is relative to the root ingest was pointed at. The same corpus
    /// read from its publication body and from the Obsidian vault root one
    /// directory above must not answer "one room called symbolon".
    #[test]
    fn room_depth_follows_the_root_ingest_was_pointed_at() {
        let from_vault = vec![
            (
                "symbolon/episteme/arguments/A24.md".to_owned(),
                "---\nrecord_id: A24\nrecord_type: argument\n---\n\n# A24\n".to_owned(),
            ),
            (
                "symbolon/mytheme/myth/uroboros.md".to_owned(),
                "---\nrecord_id: M01\nrecord_type: myth\n---\n\n# Uroboros\n".to_owned(),
            ),
        ];
        let one = ingest_corpus(&from_vault, &[], 1).unwrap();
        let rooms = |compiled: &IngestedCorpus| {
            let mut names: Vec<String> = compiled
                .objects
                .iter()
                .filter_map(|object| match object {
                    WikiObject::Space(space) => Some(space.ref_id.to_string()),
                    _ => None,
                })
                .collect();
            names.sort();
            names
        };
        assert_eq!(
            rooms(&one),
            vec!["wiki:space:room/symbolon"],
            "depth 1 from the vault root collapses the whole body into one room"
        );
        let two = ingest_corpus(&from_vault, &[], 2).unwrap();
        assert_eq!(
            rooms(&two),
            vec![
                "wiki:space:room/symbolon/episteme",
                "wiki:space:room/symbolon/mytheme"
            ],
            "depth 2 recovers the structure the corpus actually has there"
        );
        assert!(
            rooms(&ingest_corpus(&from_vault, &[], 0).unwrap()).is_empty(),
            "depth 0 compiles no rooms at all"
        );
        // A record with no segment beyond the room depth sits at that level
        // rather than naming a room of its own.
        let shallow = vec![(
            "README-record.md".to_owned(),
            "---\nrecord_id: R1\nrecord_type: document\n---\n\n# R\n".to_owned(),
        )];
        assert!(rooms(&ingest_corpus(&shallow, &[], 1).unwrap()).is_empty());
    }

    #[test]
    fn unresolved_links_are_disclosed_never_silently_dropped() {
        let corpus = vec![(
            "arguments/A24.md".to_owned(),
            "---\nrecord_id: A24\nrecord_type: argument\n---\n\n# A24\n\nSee [[t09-history]].\n".to_owned(),
        )];
        let absences = ingest_corpus(&corpus, &[], 1).unwrap().absences;
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
        let compiled = ingest_corpus(&corpus, &[], 1).unwrap();
        assert!(
            compiled.absences.is_empty(),
            "both markdown links resolve: {:?}",
            compiled.absences
        );
        let index = SemanticWikiIndex::rebuild(compiled.objects).unwrap();

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
        let compiled = ingest_corpus(&corpus, &[], 1).unwrap();
        assert!(compiled.absences.is_empty(), "{:?}", compiled.absences);
        let index = SemanticWikiIndex::rebuild(compiled.objects).unwrap();
        let backlinks = index.backlinks(&ResourceRef::parse("wiki:node:record/A31").unwrap());
        assert!(backlinks
            .iter()
            .any(|n| n.resource.as_str() == "wiki:node:record/etymology-arbitration-historical-branches"));
    }

    // -----------------------------------------------------------------
    // select_ingestable_records: the mixed-tree, real-directory condition.
    // -----------------------------------------------------------------

    #[test]
    fn selection_sets_aside_files_that_declare_no_identity() {
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
        // The README carries nothing to place; the movement carries
        // `node_type:` and `claim_status:` and so is named, not counted.
        assert_eq!(selection.skipped_inert, 1);
        assert_eq!(selection.skipped_unaddressable.len(), 1);
        assert!(selection.skipped_unaddressable[0].contains("01-immutable-gap.md"));
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
