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
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
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
    assert_eq!(verification, vec!["wiki:node:capability-matrix:matrix.test"]);
}

#[test]
fn matrix_revision_change_raises_basis_changed_on_dependents() {
    let dir = fixture_matrix(CSV_V1);
    let first = compile_capability_matrix(&dir, None);
    let index = SemanticWikiIndex::rebuild(first.objects).expect("rebuild");
    let node = index
        .node(
            &aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root")
                .unwrap(),
        )
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
        CSV_V1.replace("A person needs a durable root.", "A person needs a durable, inspectable root."),
    )
    .unwrap();
    let second = compile_capability_matrix(&dir, None);
    let second_index = SemanticWikiIndex::rebuild(second.objects).unwrap();
    let second_node = second_index
        .node(
            &aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root")
                .unwrap(),
        )
        .unwrap();
    let second_semantic = second_node.provenance[0]
        .source_revision
        .clone()
        .unwrap();
    assert_ne!(second_semantic, semantic_revision, "the compiled basis moved");
    let second_revision = SourceRevision::parse(
        match &second_semantic {
            aikit_core::SemanticRevision::Text(text) => text.clone(),
            other => panic!("unexpected revision {other:?}"),
        },
    )
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
    let impact = deterministic_knowledge_impact(&horizon, std::slice::from_ref(&dependency)).unwrap();
    assert!(impact
        .affected
        .iter()
        .any(|entry| entry.resource == dependent
            && entry.freshness == aikit_core::KnowledgeFreshness::BasisChanged));
}

#[test]
fn world_discovery_compiles_project_matrices_in_their_project_space() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "aikit-matrix-world-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    let project_user = root.join("Work/garden/ProjectCentral/user");
    fs::create_dir_all(&project_user).unwrap();
    fs::write(project_user.join("capability-matrix.json"),
        json!({
            "protocol": "ql-capability-matrix/1",
            "matrix_id": "matrix.garden",
            "anchor_ref": "garden:doc:overview",
            "default_view": "product-field",
            "views": []
        }).to_string()).unwrap();
    fs::write(project_user.join("capability-matrix.csv"), CSV_V1).unwrap();

    let reading = compile_world_matrices(&root);
    assert!(reading.absences.is_empty(), "{:?}", reading.absences);
    let index = SemanticWikiIndex::rebuild(reading.objects).expect("rebuild");
    let node = index
        .node(
            &aikit_core::ResourceRef::parse("wiki:node:capability:cap.test.root")
                .unwrap(),
        )
        .expect("project capability compiled");
    assert_eq!(
        node.space_refs[0].as_str(),
        "central:wiki:project:garden",
        "project placement rides the project wiki space"
    );
}
