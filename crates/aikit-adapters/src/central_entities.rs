//! Compiled entity materialisation (W10 V3): Central's bounded-entity
//! carriers compile into `pasu` wiki entity nodes behind the
//! [`SemanticWikiIndex`](aikit_core::SemanticWikiIndex) rebuild.
//!
//! Entity identity is stable across rebuilds: the node ref derives from the
//! carrier's own identity refs, never from content. Provenance is exact —
//! each provenance entry names the carrier source and its authored revision;
//! sourced-file content revisions ride the `aikit.pasu/v1` extension so an
//! identity-source edit changes the entity's relations without minting a
//! second subject (CASE 15). Per W10 rev 4, the wiki entity node_type is
//! `pasu` — the general grammar — with the form (`nara`/`agent`/`agent-set`)
//! carried in fields; the nara-form entity adopts the existing
//! `wiki:node:identity` anchor ref unchanged.

use aikit_core::{
    ResourceRef, SemanticRevision, SourceRef, WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject,
    WikiProvenanceRef, WikiSpace,
};

fn resource_ref(value: &str) -> ResourceRef {
    ResourceRef::parse(value).expect("entity refs are valid resource refs")
}

fn source_ref(value: String) -> SourceRef {
    SourceRef::parse(value).expect("carrier source refs are valid source refs")
}
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

pub const ENTITY_PRODUCER_REF: &str = "aikit/central-entity-materialisation/v1";
pub const PASU_EXTENSION: &str = "aikit.pasu/v1";
/// The existing root-wiki anchor node ref; the nara-form entity adopts it.
pub const NARA_ENTITY_REF: &str = "wiki:node:identity";
pub const PASU_EXTENSION_KEYS_BANNED: &[&str] = &["provider_id", "row_id"];

pub struct CentralEntityReading {
    pub objects: Vec<WikiObject>,
    pub absences: Vec<String>,
}

/// Materialise entity objects from the canonical Central carriers at
/// `root`. Never throws for absent carriers — absence is disclosed in
/// `absences`; only unreadable *present* carriers surface there too.
pub fn materialise_central_entities(root: &Path) -> CentralEntityReading {
    let mut objects = Vec::new();
    let mut absences = Vec::new();

    match read_identity_entity(root) {
        Ok(Some(node)) => objects.push(WikiObject::Node(node)),
        Ok(None) => absences.push("Central identity manifest absent; no nara entity materialised".into()),
        Err(error) => absences.push(error),
    }

    let agent_nodes = read_agent_entities(root, &mut absences).unwrap_or_default();

    let (mut set_nodes, set_edges) = read_agent_set_entities(root, &mut absences);

    // Member edges only target entities that exist in this same pass, so the
    // compiled graph never dangles; unresolved members are disclosed.
    let mut known_refs: BTreeSet<String> = agent_nodes.iter().map(|n| n.ref_id.as_str().to_owned()).collect();
    known_refs.extend(set_nodes.iter().map(|n| n.ref_id.as_str().to_owned()));
    if let Some(WikiObject::Node(nara)) = objects.first() {
        known_refs.insert(nara.ref_id.as_str().to_owned());
    }
    let mut edges = Vec::new();
    let mut spaces = Vec::new();
    for edge in set_edges {
        if known_refs.contains(edge.to_ref.as_str()) {
            edges.push(edge);
        } else {
            absences.push(format!(
                "AgentSet member {} has no materialised entity; membership edge omitted",
                edge.to_ref.as_str()
            ));
        }
    }

    // W10 V4: each agent-set entity is a bounded local whole — its local
    // space anchors on the entity and carries exactly the materialised
    // membership, so `local_space_ref` resolves at rebuild and navigation
    // traverses the whole through the bounded relation faculty.
    for node in &mut set_nodes {
        // The whole set ref must survive the derivation. Central permits
        // colon-bearing set refs (`validate_ref` rejects only empty, untrimmed
        // and NUL values — ctrl/src/agent_set_store.rs:533), so taking the
        // final colon-separated fragment would map two distinct permitted sets
        // such as `team:review` and `other:review` onto one local space. The
        // form prefix is stripped instead, leaving the full ref (and, for
        // colon-free refs, the identical ref this produced before).
        let set_ref = node
            .extensions
            .get(PASU_EXTENSION)
            .and_then(|form| form.get("subject_ref"))
            .and_then(|value| value.as_str())
            .and_then(|subject| subject.strip_prefix("central:pasu:agent-set:"))
            .unwrap_or_default()
            .to_owned();
        if set_ref.is_empty() {
            continue;
        }
        let local_space_ref = format!("wiki:space:pasu-local:{set_ref}");
        let members: Vec<ResourceRef> = edges
            .iter()
            .filter(|edge| edge.from_ref.as_str() == node.ref_id.as_str())
            .map(|edge| edge.to_ref.clone())
            .collect();
        node.local_space_ref = Some(resource_ref(&local_space_ref));
        spaces.push(WikiObject::Space(WikiSpace {
            profile: "okf-wiki/v1".into(),
            ref_id: resource_ref(&local_space_ref),
            revision: 1,
            provenance: Vec::new(),
            title: Some(format!("Local whole: {set_ref}")),
            parent_space_refs: Vec::new(),
            child_space_refs: Vec::new(),
            node_refs: members,
            anchor_ref: Some(node.ref_id.clone()),
            extensions: BTreeMap::new(),
        }));
    }
    objects.extend(agent_nodes.into_iter().map(WikiObject::Node));
    objects.extend(set_nodes.into_iter().map(WikiObject::Node));
    objects.extend(spaces);
    objects.extend(edges.into_iter().map(WikiObject::Edge));

    CentralEntityReading { objects, absences }
}

/// Adopt compiled entities into a discovered wiki object set: a discovered
/// node whose ref collides with a materialised entity is replaced (the
/// de-facto stand-in adopts the entity convention); everything else is
/// appended. Later V3+ rebuilds see the same result for the same carriers.
pub fn adopt_into(discovered: &mut Vec<WikiObject>, entities: Vec<WikiObject>) {
    let entity_refs: BTreeSet<String> = entities
        .iter()
        .map(|object| object.ref_id().as_str().to_owned())
        .collect();
    discovered.retain(|object| !entity_refs.contains(object.ref_id().as_str()));
    discovered.extend(entities);
}

fn pasu_extension(form: &str, subject_ref: &str, extra: Value) -> BTreeMap<String, Value> {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        PASU_EXTENSION.to_owned(),
        json!({"form": form, "subject_ref": subject_ref, "extra": extra}),
    );
    // W10 rev 3: the paśu identity grammar is the first typed family of
    // 0/1 anchors — stance is declared data, never an engine kind.
    extensions.insert(
        "aikit.ql-stance/v1".to_owned(),
        json!({"stance": "0/1"}),
    );
    extensions
}

fn entity_ref(form: &str, subject: &str) -> String {
    format!("wiki:node:pasu:{form}:{subject}")
}

/// The canonical paśu subject of the agent form for an existing agent ref:
/// `central:pasu:agent:<agent_ref>`, the subject id being the opaque agent
/// ref itself. Mirrors `ctrl::pasu::PasuRef::for_agent` (ctrl/src/pasu.rs:121)
/// — AIKit consumes Central's grammar, it does not mint a parallel one. The
/// raw `agent_ref` rides alongside in the extension so both addresses answer.
fn pasu_agent_ref(agent_ref: &str) -> String {
    format!("central:pasu:agent:{agent_ref}")
}

/// The canonical paśu subject of the agent-set form for an existing set ref.
/// Mirrors `ctrl::pasu::PasuRef::for_agent_set`.
fn pasu_agent_set_ref(set_ref: &str) -> String {
    format!("central:pasu:agent-set:{set_ref}")
}

/// Deterministic edge ref: endpoints + relation (never content, never order
/// of discovery).
fn member_edge_ref(from: &str, to: &str) -> String {
    format!("wiki:edge:{from}->{to}:member")
}

/// FNV-1a 64-bit over bytes, hex — a deterministic content revision for
/// sourced files (the same derivation Central's stores use for CAS keys).
fn content_revision(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn central_source_ref(relative: &str) -> String {
    format!("central:source:control:root:{relative}")
}

/// Read a canonical relative file with the central_wiki discipline:
/// plain relative components, no symlink redirection, bounded size.
/// The root itself is canonicalised first (hosts may mount the world under
/// a symlinked parent); redirection is judged against the canonical root.
fn read_canonical(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    let relative_path = Path::new(relative);
    if !relative_path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(format!("carrier `{relative}` is not a plain relative path"));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        format!("central root is unavailable: {error}")
    })?;
    let expected = canonical_root.join(relative_path);
    let actual = expected
        .canonicalize()
        .map_err(|error| format!("carrier `{relative}` is unavailable: {error}"))?;
    if !actual.starts_with(&canonical_root) || actual != expected {
        return Err(format!("carrier `{relative}` redirects through a symlink"));
    }
    let metadata =
        fs::metadata(&expected).map_err(|error| format!("carrier `{relative}`: {error}"))?;
    if metadata.len() > 1024 * 1024 {
        return Err(format!("carrier `{relative}` exceeds the bounded read size"));
    }
    fs::read(&expected).map_err(|error| format!("carrier `{relative}`: {error}"))
}

fn read_json(root: &Path, relative: &str) -> Result<Value, String> {
    let bytes = read_canonical(root, relative)?;
    serde_json::from_slice(&bytes).map_err(|error| format!("carrier `{relative}`: {error}"))
}

/// The nara-form entity from the identity manifest carrier
/// (`Control/user/identity/manifest.json`, central.pasu.identity-manifest/v1).
fn read_identity_entity(root: &Path) -> Result<Option<WikiNode>, String> {
    let manifest_path = "Control/user/identity/manifest.json";
    if !root.join(manifest_path).is_file() {
        return Ok(None);
    }
    let manifest = read_json(root, manifest_path)?;
    if manifest["schema"] != "central.pasu.identity-manifest/v1" {
        return Err(format!(
            "carrier `{manifest_path}` has an unsupported schema"
        ));
    }
    let subject_ref = manifest["subject"]["ref"]
        .as_str()
        .ok_or_else(|| format!("carrier `{manifest_path}` names no subject ref"))?
        .to_owned();
    let revision = manifest["revision"]
        .as_str()
        .unwrap_or("unversioned")
        .to_owned();

    let mut sourced = Vec::new();
    let mut source_refs = Vec::new();
    if let Some(entries) = manifest["identity_source"]["sources"].as_array() {
        for entry in entries {
            let Some(path) = entry["path"].as_str() else {
                continue;
            };
            let content = match read_canonical(root, path) {
                Ok(bytes) => bytes,
                Err(absence) => {
                    sourced.push(json!({"path": path, "present": false, "reason": absence}));
                    continue;
                }
            };
            source_refs.push(source_ref(central_source_ref(path)));
            sourced.push(json!({
                "path": path,
                "present": true,
                "content_revision": content_revision(&content),
                "standing": entry["standing"].as_str().unwrap_or("unspecified"),
            }));
        }
    }

    Ok(Some(WikiNode {
        profile: "okf-wiki/v1".into(),
        ref_id: resource_ref(NARA_ENTITY_REF),
        revision: 1,
        provenance: vec![WikiProvenanceRef {
            source_ref: source_ref(subject_ref.clone()),
            source_revision: Some(SemanticRevision::Text(revision.clone())),
            producer_ref: Some(resource_ref(ENTITY_PRODUCER_REF)),
            generation_ref: None,
            extensions: BTreeMap::new(),
        }],
        node_type: "pasu".into(),
        title: manifest["subject"]["title"]
            .as_str()
            .map(str::to_owned)
            .or(Some("Pasu entity (nara form)".into())),
        space_refs: Vec::new(),
        source_refs,
        local_space_ref: None,
        extensions: pasu_extension(
            "nara",
            &subject_ref,
            json!({"manifest_revision": revision, "sourced": sourced}),
        ),
    }))
}

/// Agent entities from the AgentProfile store. The entity ref derives from
/// the stable `agent_ref`, so a profile revision change re-relates the same
/// entity instead of minting a second one (CASE 16).
fn read_agent_entities(root: &Path, absences: &mut Vec<String>) -> Result<Vec<WikiNode>, ()> {
    let dir = root.join("Control/agents/profiles");
    let mut nodes: BTreeMap<String, WikiNode> = BTreeMap::new();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) => {
            absences.push(format!("AgentProfile store unreadable: {error}"));
            return Err(());
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let record: Value = match serde_json::from_slice(&fs::read(&path).unwrap_or_default()) {
            Ok(record) => record,
            Err(error) => {
                absences.push(format!(
                    "AgentProfile {} unreadable: {error}",
                    path.display()
                ));
                continue;
            }
        };
        if record["schema"] != "central.agent-profile/v1" {
            absences.push(format!(
                "AgentProfile {} has an unsupported schema",
                path.display()
            ));
            continue;
        }
        let Some(agent_ref) = record["agent_ref"].as_str().map(str::to_owned) else {
            absences.push(format!("AgentProfile {} names no agent_ref", path.display()));
            continue;
        };
        // Central serialises the profile identifier as `ref` — ctrl's
        // `AgentProfile` carries `#[serde(rename = "ref")]` on `profile_ref`
        // (ctrl/src/agent_profile.rs:113). The legacy `profile_ref` spelling is
        // still accepted so older generated records keep reading, but the
        // identifier is never substituted while a real one is present: the
        // placeholder only marks a record that genuinely names no profile.
        let profile_ref = record["ref"]
            .as_str()
            .or_else(|| record["profile_ref"].as_str())
            .unwrap_or("unprofiled");
        let revision = record["revision"].as_str().unwrap_or("unversioned");
        let relative = path
            .strip_prefix(root)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let scope = record["scope"].as_str().unwrap_or("unspecified");
        let agent_subject = pasu_agent_ref(&agent_ref);
        let node = nodes.entry(agent_ref.clone()).or_insert_with(|| WikiNode {
            profile: "okf-wiki/v1".into(),
            ref_id: resource_ref(&entity_ref("agent", &agent_ref)),
            revision: 1,
            provenance: Vec::new(),
            node_type: "pasu".into(),
            title: Some(format!("Pasu entity (agent: {agent_ref})")),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: pasu_extension(
                "agent",
                &agent_subject,
                json!({"agent_ref": agent_ref.clone(), "profiles": []}),
            ),
        });
        node.provenance.push(WikiProvenanceRef {
            source_ref: source_ref(central_source_ref(&relative)),
            source_revision: Some(SemanticRevision::Text(revision.to_owned())),
            producer_ref: Some(resource_ref(ENTITY_PRODUCER_REF)),
            generation_ref: None,
            extensions: BTreeMap::new(),
        });
        node.source_refs.push(source_ref(central_source_ref(&relative)));
        if let Some(profiles) = node
            .extensions
            .get_mut(PASU_EXTENSION)
            .and_then(|value| value.get_mut("extra"))
            .and_then(|value| value.get_mut("profiles"))
            .and_then(|value| value.as_array_mut())
        {
            // Faithful intake: the profile relation carries the identifier,
            // residence and revision, and the generated-proposal provenance
            // block verbatim when the record has one. The intent is retained
            // with its standing (authorship + recognition) — an unrecognised
            // proposal must never become an adopted default merely by being
            // discovered, so the standing travels with the text.
            let mut entry = json!({
                "profile_ref": profile_ref,
                "scope": scope,
                "revision": revision,
                "source": central_source_ref(&relative),
            });
            if let Some(provenance) = record.get("intent_provenance") {
                entry["intent_provenance"] = provenance.clone();
            }
            profiles.push(entry);
        }
    }
    Ok(nodes.into_values().collect())
}

/// AgentSet entities from the authored relation records, with Compiled
/// membership edges to the member entities (existing targets only).
fn read_agent_set_entities(
    root: &Path,
    absences: &mut Vec<String>,
) -> (Vec<WikiNode>, Vec<WikiEdge>) {
    let dir = root.join("Control/agents/agent-sets");
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    if !dir.is_dir() {
        return (nodes, edges);
    }
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) => {
            absences.push(format!("AgentSet store unreadable: {error}"));
            return (nodes, edges);
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let record: Value = match serde_json::from_slice(&fs::read(&path).unwrap_or_default()) {
            Ok(record) => record,
            Err(error) => {
                absences.push(format!("AgentSet {} unreadable: {error}", path.display()));
                continue;
            }
        };
        if record["schema"] != "central.agent-set/v1" {
            absences.push(format!(
                "AgentSet {} has an unsupported schema",
                path.display()
            ));
            continue;
        }
        let Some(set_ref) = record["ref"].as_str().map(str::to_owned) else {
            absences.push(format!("AgentSet {} carries no ref", path.display()));
            continue;
        };
        let revision = record["revision"].as_str().unwrap_or("unversioned").to_owned();
        let relative = path
            .strip_prefix(root)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let entity = WikiNode {
            profile: "okf-wiki/v1".into(),
            ref_id: resource_ref(&entity_ref("agent-set", &set_ref)),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: source_ref(central_source_ref(&relative)),
                source_revision: Some(SemanticRevision::Text(revision)),
                producer_ref: Some(resource_ref(ENTITY_PRODUCER_REF)),
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "pasu".into(),
            title: Some(format!("Pasu entity (agent-set: {set_ref})")),
            space_refs: Vec::new(),
            source_refs: vec![source_ref(central_source_ref(&relative))],
            local_space_ref: None,
            extensions: pasu_extension(
                "agent-set",
                &pasu_agent_set_ref(&set_ref),
                json!({"set_ref": set_ref, "members": record["members"].clone()}),
            ),
        };
        if let Some(members) = record["members"].as_array() {
            for member in members {
                match member["kind"].as_str() {
                    Some("agent") => {
                        if let Some(agent_ref) = member["agent_ref"].as_str() {
                            let from = entity_ref("agent-set", &set_ref);
                            let to = entity_ref("agent", agent_ref);
                            edges.push(WikiEdge {
                                profile: "okf-wiki/v1".into(),
                                ref_id: resource_ref(&member_edge_ref(&from, &to)),
                                revision: 1,
                                provenance: Vec::new(),
                                from_ref: resource_ref(&from),
                                to_ref: resource_ref(&to),
                                relation: "member".into(),
                                origin: WikiEdgeOrigin::Compiled,
                                origin_ref: Some(resource_ref(ENTITY_PRODUCER_REF)),
                                extensions: BTreeMap::new(),
                            });
                        }
                    }
                    Some("agent-set") => {
                        if let Some(nested) = member["agent_set_ref"].as_str() {
                            let from = entity_ref("agent-set", &set_ref);
                            let to = entity_ref("agent-set", nested);
                            edges.push(WikiEdge {
                                profile: "okf-wiki/v1".into(),
                                ref_id: resource_ref(&member_edge_ref(&from, &to)),
                                revision: 1,
                                provenance: Vec::new(),
                                from_ref: resource_ref(&from),
                                to_ref: resource_ref(&to),
                                relation: "member".into(),
                                origin: WikiEdgeOrigin::Compiled,
                                origin_ref: Some(resource_ref(ENTITY_PRODUCER_REF)),
                                extensions: BTreeMap::new(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        nodes.push(entity);
    }
    (nodes, edges)
}
