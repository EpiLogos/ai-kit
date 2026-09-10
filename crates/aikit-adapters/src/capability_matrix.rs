//! Capability-matrix compilation (W10 V4 extension, QL V9.5): matrix records
//! compile into wiki objects (origin Compiled) in both placements — each
//! product's project space and the Central root composition.
//!
//! The verification relation is honest by law: a capability's `verification`
//! edge points at the matrix's functional-requirements field (the governing
//! account unit / seed questions — the need/operation/outcome ground), never
//! at test definitions or development docs. Code and test references stay
//! typed extension fields; they do not become verification edges. A matrix
//! revision change flows through the exact provenance so dependents see
//! `BasisChanged` via the living seam.

use aikit_core::{
    ResourceRef, SemanticRevision, SourceRef, WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject,
    WikiProvenanceRef,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

pub const MATRIX_PROTOCOL: &str = "ql-capability-matrix/1";
pub const MATRIX_PRODUCER_REF: &str = "aikit/capability-matrix-compiler/v1";
pub const MATRIX_EXTENSION: &str = "aikit.capability-matrix/v1";
/// The relation that names functional verification (the account field).
pub const VERIFICATION_RELATION: &str = "verification";
pub const FIELD_CONTRIBUTION_RELATION: &str = "field-contribution";

pub struct MatrixReading {
    pub objects: Vec<WikiObject>,
    pub absences: Vec<String>,
}

/// Compile the matrix at `matrix_dir` (`capability-matrix.json` manifest +
/// `capability-matrix.csv` records). `space_ref` places the compiled objects
/// in the requesting wiki (project space or the Central root).
pub fn compile_capability_matrix(matrix_dir: &Path, space_ref: Option<String>) -> MatrixReading {
    let mut absences = Vec::new();
    let manifest_relative = "capability-matrix.json";
    let csv_relative = "capability-matrix.csv";
    let manifest_bytes = match fs::read(matrix_dir.join(manifest_relative)) {
        Ok(bytes) => bytes,
        Err(error) => {
            return MatrixReading {
                objects: Vec::new(),
                absences: vec![format!("capability matrix manifest unavailable: {error}")],
            }
        }
    };
    let manifest: Value = match serde_json::from_slice(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return MatrixReading {
                objects: Vec::new(),
                absences: vec![format!("capability matrix manifest unreadable: {error}")],
            }
        }
    };
    if manifest["protocol"] != MATRIX_PROTOCOL {
        return MatrixReading {
            objects: Vec::new(),
            absences: vec![format!(
                "capability matrix declares unsupported protocol {}",
                manifest["protocol"].as_str().unwrap_or_default()
            )],
        };
    }
    let Some(matrix_id) = manifest["matrix_id"].as_str().map(str::to_owned) else {
        return MatrixReading {
            objects: Vec::new(),
            absences: vec!["capability matrix manifest names no matrix_id".into()],
        };
    };
    let Ok(csv_text) = fs::read_to_string(matrix_dir.join(csv_relative)) else {
        return MatrixReading {
            objects: Vec::new(),
            absences: vec![format!(
                "capability matrix records unavailable: {csv_relative}"
            )],
        };
    };
    let csv_bytes = csv_text.as_bytes();
    let csv_revision = content_revision(csv_bytes);
    let manifest_revision = content_revision(&manifest_bytes);

    let matrix_ref = format!("wiki:node:capability-matrix:{matrix_id}");
    let mut objects = vec![WikiObject::Node(WikiNode {
        profile: "okf-wiki/v1".into(),
        ref_id: resource_ref(&matrix_ref),
        revision: 1,
        provenance: vec![
            matrix_provenance(manifest_relative, &manifest_revision),
            matrix_provenance(csv_relative, &csv_revision),
        ],
        node_type: "capability-matrix".into(),
        title: manifest["matrix_id"]
            .as_str()
            .map(|id| format!("Capability matrix ({id})")),
        space_refs: space_refs(&space_ref),
        source_refs: vec![
            source_ref_of(matrix_dir, manifest_relative),
            source_ref_of(matrix_dir, csv_relative),
        ],
        local_space_ref: None,
        extensions: matrix_extension(
            &matrix_id,
            json!({
                "manifest_revision": manifest_revision,
                "records_revision": csv_revision,
                "default_view": manifest["default_view"].clone(),
            }),
        ),
    })];

    let records = match parse_csv(&csv_text) {
        Ok(records) => records,
        Err(error) => {
            absences.push(format!("capability matrix CSV unreadable: {error}"));
            return MatrixReading { objects, absences };
        }
    };
    let header = match records.first() {
        Some(header) => header.clone(),
        None => {
            absences.push("capability matrix CSV carries no header".into());
            return MatrixReading { objects, absences };
        }
    };
    let column = |name: &str| header.iter().position(|value| value == name);

    for record in records.iter().skip(1) {
        let field = |name: &str| {
            column(name)
                .and_then(|index| record.get(index))
                .map(|value| value.as_str())
                .unwrap_or("")
        };
        let record_type = field("record_type");
        let id = field("id");
        if id.is_empty() {
            continue;
        }
        match record_type {
            "capability" => {
                let capability_ref = format!("wiki:node:capability:{id}");
                let need = field("need");
                objects.push(WikiObject::Node(WikiNode {
                    profile: "okf-wiki/v1".into(),
                    ref_id: resource_ref(&capability_ref),
                    revision: 1,
                    provenance: vec![matrix_provenance(csv_relative, &csv_revision)],
                    node_type: "capability".into(),
                    title: Some(format!("Capability {id}: {}", first_clause(need))),
                    space_refs: space_refs(&space_ref),
                    source_refs: vec![source_ref_of(matrix_dir, csv_relative)],
                    local_space_ref: None,
                    extensions: matrix_extension(
                        &matrix_id,
                        json!({
                            "capability_id": id,
                            "need": need,
                            "operation": field("operation"),
                            "outcome": field("outcome"),
                            "implementation_status": field("implementation_status"),
                            "standing": field("standing"),
                            "code_refs": field("code_refs"),
                            "test_refs": field("test_refs"),
                            "cli_commands": parse_json_field(field("extensions"), "cli_commands"),
                            "cli_exposure": parse_json_field(field("extensions"), "cli_exposure"),
                        }),
                    ),
                }));
                // The verification relation: capability -> the matrix's
                // functional-requirements field (the governing account), not
                // its tests or dev docs.
                objects.push(WikiObject::Edge(WikiEdge {
                    profile: "okf-wiki/v1".into(),
                    ref_id: resource_ref(&format!(
                        "wiki:edge:{capability_ref}->{matrix_ref}:verification"
                    )),
                    revision: 1,
                    provenance: Vec::new(),
                    from_ref: resource_ref(&capability_ref),
                    to_ref: resource_ref(&matrix_ref),
                    relation: VERIFICATION_RELATION.into(),
                    origin: WikiEdgeOrigin::Compiled,
                    origin_ref: Some(resource_ref(MATRIX_PRODUCER_REF)),
                    extensions: BTreeMap::new(),
                }));
            }
            "relation" => {
                for capability in split_refs(field("capability_refs")) {
                    let capability_ref = format!("wiki:node:capability:{capability}");
                    // Several relations can share one cell (same view/row/
                    // column, different assertions); the relation text is
                    // part of the deterministic identity.
                    let relation_digest = content_revision(
                        format!(
                            "{}\0{}\0{}\0{}\0{}",
                            field("view_id"),
                            field("row_id"),
                            field("column_id"),
                            field("coverage"),
                            field("relation")
                        )
                        .as_bytes(),
                    );
                    objects.push(WikiObject::Edge(WikiEdge {
                        profile: "okf-wiki/v1".into(),
                        ref_id: resource_ref(&format!(
                            "wiki:edge:{capability_ref}->{matrix_ref}:field-contribution:{}:{}:{}",
                            field("view_id"),
                            field("row_id"),
                            &relation_digest[..12]
                        )),
                        revision: 1,
                        provenance: Vec::new(),
                        from_ref: resource_ref(&capability_ref),
                        to_ref: resource_ref(&matrix_ref),
                        relation: FIELD_CONTRIBUTION_RELATION.into(),
                        origin: WikiEdgeOrigin::Compiled,
                        origin_ref: Some(resource_ref(MATRIX_PRODUCER_REF)),
                        extensions: {
                            let mut extensions = BTreeMap::new();
                            extensions.insert(
                                "aikit.capability-matrix/v1".to_owned(),
                                json!({
                                    "view_id": field("view_id"),
                                    "row_id": field("row_id"),
                                    "column_id": field("column_id"),
                                    "coverage": field("coverage"),
                                    "relation": field("relation"),
                                }),
                            );
                            extensions
                        },
                    }));
                }
            }
            other => absences.push(format!(
                "capability matrix record {id} has unsupported record_type `{other}`"
            )),
        }
    }

    MatrixReading { objects, absences }
}

/// Discover and compile every capability matrix disclosed by the world:
/// the Central root composition (`ProjectCentral/user/`) and each project's
/// own account (`Work/<project>/ProjectCentral/user/`).
pub fn compile_world_matrices(central_root: &Path) -> MatrixReading {
    let mut objects = Vec::new();
    let mut absences = Vec::new();
    let mut homes: Vec<(std::path::PathBuf, Option<String>)> = vec![(
        central_root.join("ProjectCentral/user"),
        Some("central:wiki:root".to_owned()),
    )];
    if let Ok(projects) = fs::read_dir(central_root.join("Work")) {
        let mut names: Vec<_> = projects
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .collect();
        names.sort();
        for project in names {
            homes.push((
                project.join("ProjectCentral/user"),
                Some(
                    aikit_core::project_wiki_space_ref(
                        project
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_default()
                            .as_str(),
                    )
                    .map(|reference| reference.as_str().to_owned())
                    .unwrap_or_default(),
                ),
            ));
        }
    }
    // The matrix_id is the stable identity (protocol law): two checkouts of
    // one project disclose one logical matrix, so a ref already compiled
    // from an earlier home is skipped with a disclosure — first home wins
    // (canonical order), and genuinely distinct matrices still compile.
    let mut seen_refs = std::collections::BTreeSet::new();
    for (dir, space_ref) in homes {
        if !dir.join("capability-matrix.json").is_file() {
            continue;
        }
        let home = dir.display().to_string();
        let mut reading = compile_capability_matrix(&dir, space_ref);
        for object in reading.objects.drain(..) {
            let object_ref = object.ref_id().as_str().to_owned();
            if seen_refs.insert(object_ref.clone()) {
                objects.push(object);
            } else {
                absences.push(format!(
                    "Capability matrix at {home} re-declares {object_ref} from an earlier home; kept the first"
                ));
            }
        }
        absences.append(&mut reading.absences);
    }
    MatrixReading { objects, absences }
}

fn matrix_provenance(carrier: &str, revision: &str) -> WikiProvenanceRef {
    WikiProvenanceRef {
        source_ref: SourceRef::parse(format!("central:source:capability-matrix:{carrier}"))
            .expect("matrix source refs are valid"),
        source_revision: Some(SemanticRevision::Text(revision.to_owned())),
        producer_ref: Some(resource_ref(MATRIX_PRODUCER_REF)),
        generation_ref: None,
        extensions: BTreeMap::new(),
    }
}

fn source_ref_of(dir: &Path, carrier: &str) -> SourceRef {
    SourceRef::parse(dir.join(carrier).to_string_lossy().replace('\\', "/"))
        .expect("matrix paths are valid source refs")
}

fn matrix_extension(matrix_id: &str, extra: Value) -> BTreeMap<String, Value> {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        MATRIX_EXTENSION.to_owned(),
        json!({"matrix_id": matrix_id, "extra": extra}),
    );
    extensions
}

fn parse_json_field(raw: &str, field: &str) -> Value {
    let parsed: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    parsed.get(field).cloned().unwrap_or(Value::Null)
}

fn split_refs(raw: &str) -> Vec<String> {
    let parsed: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    parsed
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn first_clause(text: &str) -> String {
    let clause: String = text.chars().take(80).collect();
    clause.trim_end().to_owned()
}

fn content_revision(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn resource_ref(value: &str) -> ResourceRef {
    ResourceRef::parse(value).expect("compiled refs are valid resource refs")
}

fn space_refs(space_ref: &Option<String>) -> Vec<ResourceRef> {
    space_ref
        .as_ref()
        .map(|value| vec![resource_ref(value)])
        .unwrap_or_default()
}

/// A bounded RFC 4180 reader: quoted fields, embedded commas/newlines/quotes,
/// CRLF tolerance. The matrices are small; no streaming is required.
fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut field_started = false;
    while let Some(current) = chars.next() {
        if in_quotes {
            match current {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => field.push(current),
            }
            continue;
        }
        match current {
            '"' if field.is_empty() && !field_started => {
                in_quotes = true;
                field_started = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                field_started = false;
            }
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                field_started = false;
            }
            _ => {
                field.push(current);
                field_started = true;
            }
        }
    }
    if in_quotes {
        return Err("CSV ends inside a quoted field".into());
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}
