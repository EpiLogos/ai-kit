//! W10 V4 extension — capability-matrix compilation: matrix records become
//! wiki objects (origin Compiled) in both placements, the verification
//! relation points at the functional-requirements field (never tests or dev
//! docs), and a matrix revision change raises BasisChanged on dependents.

use aikit_adapters::capability_matrix::{compile_capability_matrix, compile_world_matrices};
use aikit_core::{
    deterministic_knowledge_impact, KnowledgeChangeHorizon, KnowledgeDependency,
    KnowledgeObservedSource, SemanticWikiIndex, SourceRevision, WikiObject,
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture_matrix(csv: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aikit-capability-matrix-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("capability-matrix.json"),
        json!({
            "protocol": "ql-capability-matrix/1",
            "matrix_id": "matrix.test",
            "anchor_ref": "test:doc:overview",
            "default_view": "product-field",
            "views": []
        })
        .to_string(),
    )
    .unwrap();
    fs::write(dir.join("capability-matrix.csv"), csv).unwrap();
    dir
}

const CSV_V1: &str = r#"id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question
cap.test.root,capability,,,,[],A person needs a durable root.,ctrl resolves the root.,The root contains Control and Work.,source-inspected,agent-inference,src/a.md,ctrl/src/root.rs,ctrl/tests/a.rs,test.html#q1,,seeds,"{""cli_commands"": [""central.root""]}",
rel.test.1,relation,product-field,q1,S1,["cap.test.root"],,,,,,,,,,the root seeds the field,H,"{}",
"#;

#[test]
fn matrix_compiles_to_capability_objects_with_honest_verification() {
    let dir = fixture_matrix(CSV_V1);
    let reading = compile_capability_matrix(&dir, Some("central:wiki:root".into()));
    assert!(reading.absences.is_empty(), "{:?}", reading.absences);
    let index = SemanticWikiIndex::rebuild(reading.objects).expect("compiled matrix rebuilds");

    // The capability node carries the functional account.
    let capability = index
        .node(&aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root").unwrap())
        .expect("capability node");
    assert_eq!(capability.node_type, "capability");
    let extension = &capability.extensions["aikit.capability-matrix/v1"]["extra"];
    assert_eq!(extension["need"], "A person needs a durable root.");
    assert_eq!(extension["cli_commands"][0], "central.root");
    assert_eq!(capability.space_refs[0].as_str(), "central:wiki:root");

    // Verification points at the matrix's functional-requirements field —
    // the code and test refs stay extension data, never verification edges.
    let verification: Vec<_> = index
        .discover()
        .iter()
        .filter_map(|reference| index.resolve(reference))
        .filter_map(|object| match object {
            WikiObject::Edge(edge)
                if edge.from_ref.as_str() == "wiki:node:capability:cap.test.root"
                    && edge.relation == "verification" =>
            {
                Some(edge.to_ref.as_str().to_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        verification,
        vec!["wiki:node:capability-matrix:matrix.test"]
    );
}

#[test]
fn matrix_revision_change_raises_basis_changed_on_dependents() {
    let dir = fixture_matrix(CSV_V1);
    let first = compile_capability_matrix(&dir, None);
    let index = SemanticWikiIndex::rebuild(first.objects).expect("rebuild");
    let node = index
        .node(&aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root").unwrap())
        .unwrap();
    let semantic_revision = node.provenance[0]
        .source_revision
        .clone()
        .expect("exact provenance carries the matrix record revision");
    let semantic_text = match &semantic_revision {
        aikit_core::SemanticRevision::Text(text) => text.clone(),
        other => panic!("unexpected revision {other:?}"),
    };
    let basis_revision = SourceRevision::parse(semantic_text.clone()).unwrap();

    // A reading depends on the capability at the compiled basis.
    let dependent = aikit_core::ResourceRef::parse("wiki:reading:orientation").unwrap();
    let dependency = KnowledgeDependency {
        dependent: dependent.clone(),
        source: node.provenance[0].source_ref.clone(),
        basis_revision: Some(basis_revision.clone()),
        relation: "orientation-basis".into(),
        provenance_ref: None,
        integrative: false,
    };
    let unchanged = deterministic_knowledge_impact(
        &KnowledgeChangeHorizon {
            provider: "test".into(),
            cursor: 0,
            sources: Vec::new(),
            changes: Vec::new(),
        },
        std::slice::from_ref(&dependency),
    )
    .unwrap();
    assert!(unchanged
        .affected
        .iter()
        .all(|entry| entry.freshness != aikit_core::KnowledgeFreshness::BasisChanged));

    // The matrix CSV changes: a new revision compiles, and the horizon that
    // observes the matrix source now flags the dependent BasisChanged.
    fs::write(
        dir.join("capability-matrix.csv"),
        CSV_V1.replace(
            "A person needs a durable root.",
            "A person needs a durable, inspectable root.",
        ),
    )
    .unwrap();
    let second = compile_capability_matrix(&dir, None);
    let second_index = SemanticWikiIndex::rebuild(second.objects).unwrap();
    let second_node = second_index
        .node(&aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root").unwrap())
        .unwrap();
    let second_semantic = second_node.provenance[0].source_revision.clone().unwrap();
    assert_ne!(
        second_semantic, semantic_revision,
        "the compiled basis moved"
    );
    let second_revision = SourceRevision::parse(match &second_semantic {
        aikit_core::SemanticRevision::Text(text) => text.clone(),
        other => panic!("unexpected revision {other:?}"),
    })
    .unwrap();

    let horizon = KnowledgeChangeHorizon {
        provider: "test".into(),
        cursor: 1,
        sources: vec![KnowledgeObservedSource {
            source: node.provenance[0].source_ref.clone(),
            revision: Some(second_revision),
            available: true,
        }],
        changes: Vec::new(),
    };
    let impact =
        deterministic_knowledge_impact(&horizon, std::slice::from_ref(&dependency)).unwrap();
    assert!(impact
        .affected
        .iter()
        .any(|entry| entry.resource == dependent
            && entry.freshness == aikit_core::KnowledgeFreshness::BasisChanged));
}

#[test]
fn world_discovery_compiles_project_matrices_in_their_project_space() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "aikit-matrix-world-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    let project_user = root.join("Work/garden/ProjectCentral/user");
    fs::create_dir_all(&project_user).unwrap();
    fs::write(
        project_user.join("capability-matrix.json"),
        json!({
            "protocol": "ql-capability-matrix/1",
            "matrix_id": "matrix.garden",
            "anchor_ref": "garden:doc:overview",
            "default_view": "product-field",
            "views": []
        })
        .to_string(),
    )
    .unwrap();
    fs::write(project_user.join("capability-matrix.csv"), CSV_V1).unwrap();

    let reading = compile_world_matrices(&root);
    assert!(reading.absences.is_empty(), "{:?}", reading.absences);
    let index = SemanticWikiIndex::rebuild(reading.objects).expect("rebuild");
    let node = index
        .node(&aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root").unwrap())
        .expect("project capability compiled");
    assert_eq!(
        node.space_refs[0].as_str(),
        "central:wiki:project:garden",
        "project placement rides the project wiki space"
    );
}

#[test]
fn world_compilation_attributes_project_matrix_objects_and_carriers() {
    // A world with a root composition matrix and one project matrix. The
    // project's objects — and the plain filesystem carrier paths they cite —
    // must carry their owning Project display; the root composition stays
    // unattributed, because the root lineage is a legitimately broader
    // aperture, never a Project's own.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "aikit-matrix-attribution-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    let root_user = root.join("ProjectCentral/user");
    let project_user = root.join("Work/garden/ProjectCentral/user");
    fs::create_dir_all(&root_user).unwrap();
    fs::create_dir_all(&project_user).unwrap();
    let manifest = json!({
        "protocol": "ql-capability-matrix/1",
        "matrix_id": "matrix.attribution",
        "anchor_ref": "test:doc:overview",
        "default_view": "product-field",
        "views": []
    })
    .to_string();
    // Distinct matrix ids: identical ids would collide on first-home-wins
    // and the project home would compile nothing to attribute.
    fs::write(root_user.join("capability-matrix.json"), &manifest).unwrap();
    fs::write(root_user.join("capability-matrix.csv"), CSV_V1).unwrap();
    let project_manifest = json!({
        "protocol": "ql-capability-matrix/1",
        "matrix_id": "matrix.garden",
        "anchor_ref": "garden:doc:overview",
        "default_view": "product-field",
        "views": []
    })
    .to_string();
    fs::write(
        project_user.join("capability-matrix.json"),
        project_manifest,
    )
    .unwrap();
    fs::write(project_user.join("capability-matrix.csv"), CSV_GARDEN).unwrap();

    let reading = compile_world_matrices(&root);
    assert!(reading.absences.is_empty(), "{:?}", reading.absences);
    let carrier_csv = project_user
        .join("capability-matrix.csv")
        .to_string_lossy()
        .replace('\\', "/");
    let carrier_json = project_user
        .join("capability-matrix.json")
        .to_string_lossy()
        .replace('\\', "/");

    // Every garden object and carrier path names Work/garden.
    let matrix_ref = "wiki:node:capability-matrix:matrix.garden";
    assert_eq!(
        reading.object_projects.get(matrix_ref).map(String::as_str),
        Some("Work/garden"),
        "the project matrix node is attributed to its project"
    );
    assert_eq!(
        reading
            .object_projects
            .get("wiki:node:capability:cap.garden.root")
            .map(String::as_str),
        Some("Work/garden"),
        "project capability nodes are attributed"
    );
    for carrier in [&carrier_csv, &carrier_json] {
        assert_eq!(
            reading.object_projects.get(carrier).map(String::as_str),
            Some("Work/garden"),
            "carrier path {carrier} is attributed — plain paths carry no ownership in their text"
        );
    }
    assert!(
        reading.object_projects.keys().any(
            |reference| reference.starts_with("wiki:edge:wiki:node:capability:cap.garden.root")
        ),
        "compiled matrix edges are attributed too"
    );

    // The root composition stays unattributed.
    assert!(
        !reading
            .object_projects
            .contains_key("wiki:node:capability-matrix:matrix.attribution"),
        "root composition objects belong to no Project"
    );
    assert!(
        !reading
            .object_projects
            .keys()
            .any(|reference| reference.contains("matrix.attribution")),
        "no root-composition ref carries a Project attribution"
    );
}

const CSV_GARDEN: &str = r#"id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question
cap.garden.root,capability,,,,[],A person needs a durable root.,ctrl resolves the root.,The root contains Control and Work.,source-inspected,agent-inference,src/a.md,ctrl/src/root.rs,ctrl/tests/a.rs,test.html#q1,,seeds,"{""cli_commands"": [""central.root""]}",
rel.garden.1,relation,product-field,q1,S1,["cap.garden.root"],,,,,,,,,,the root seeds the field,H,"{}",
"#;

#[test]
fn world_compilation_attributes_project_matrices_and_leaves_the_root_composition_unattributed() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "aikit-matrix-attribution-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    // The root composition matrix and one Work Project's matrix, in the two
    // placements the world compiler walks.
    let root_user = root.join("ProjectCentral/user");
    fs::create_dir_all(&root_user).unwrap();
    fs::write(
        root_user.join("capability-matrix.json"),
        json!({
            "protocol": "ql-capability-matrix/1",
            "matrix_id": "matrix.rootcomp",
            "anchor_ref": "root:doc:overview",
            "default_view": "product-field",
            "views": []
        })
        .to_string(),
    )
    .unwrap();
    fs::write(root_user.join("capability-matrix.csv"), CSV_ROOTCOMP).unwrap();
    let garden_user = root.join("Work/garden/ProjectCentral/user");
    fs::create_dir_all(&garden_user).unwrap();
    fs::write(
        garden_user.join("capability-matrix.json"),
        json!({
            "protocol": "ql-capability-matrix/1",
            "matrix_id": "matrix.garden",
            "anchor_ref": "garden:doc:overview",
            "default_view": "product-field",
            "views": []
        })
        .to_string(),
    )
    .unwrap();
    fs::write(garden_user.join("capability-matrix.csv"), CSV_GARDEN).unwrap();

    let reading = compile_world_matrices(&root);
    assert!(reading.absences.is_empty(), "{:?}", reading.absences);
    assert!(
        reading.project_absences.is_empty(),
        "{:?}",
        reading.project_absences
    );

    // Every object the garden home compiled — nodes, the compiled
    // verification edge and the relation edge — and the carrier paths those
    // objects cite, ride the garden's Work-relative display.
    for reference in [
        "wiki:node:capability-matrix:matrix.garden",
        "wiki:node:capability:cap.garden.root",
        "wiki:edge:wiki:node:capability:cap.garden.root->wiki:node:capability-matrix:matrix.garden:verification",
    ] {
        assert_eq!(
            reading.object_projects.get(reference),
            Some(&"Work/garden".to_string()),
            "{reference} is not attributed to its compiling Project"
        );
    }
    // The relation edge's identity carries a content digest, so it is
    // matched by its stable prefix.
    assert!(reading.object_projects.iter().any(|(reference, project)| {
        reference.starts_with(
            "wiki:edge:wiki:node:capability:cap.garden.root->wiki:node:capability-matrix:matrix.garden:field-contribution:",
        ) && project == "Work/garden"
    }));
    let garden_manifest = garden_user
        .join("capability-matrix.json")
        .to_string_lossy()
        .to_string();
    let garden_csv = garden_user
        .join("capability-matrix.csv")
        .to_string_lossy()
        .to_string();
    assert_eq!(
        reading.object_projects.get(garden_manifest.as_str()),
        Some(&"Work/garden".to_string()),
        "the carrier manifest path is not attributed to its Project"
    );
    assert_eq!(
        reading.object_projects.get(garden_csv.as_str()),
        Some(&"Work/garden".to_string()),
        "the carrier csv path is not attributed to its Project"
    );

    // The root composition stays unattributed: the root lineage is a
    // distinct, legitimately broader aperture, never a Project's.
    assert!(!reading
        .object_projects
        .contains_key("wiki:node:capability-matrix:matrix.rootcomp"));
    assert!(!reading
        .object_projects
        .contains_key("wiki:node:capability:cap.rootcomp"));
    let root_manifest = root_user
        .join("capability-matrix.json")
        .to_string_lossy()
        .to_string();
    assert!(!reading
        .object_projects
        .contains_key(root_manifest.as_str()));
    // And nothing at all is attributed to a path outside the garden home.
    for key in reading.object_projects.keys() {
        assert!(
            key.contains("/Work/garden/") || key.starts_with("wiki:"),
            "unexpected attribution key {key}"
        );
    }
}

const CSV_ROOTCOMP: &str = r#"id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question
cap.rootcomp,capability,,,,[],The composition needs a root.,resolve the root.,The root holds the composition.,source-inspected,agent-inference,src/r.md,_,_,_,,,seeds,"{}",
"#;
