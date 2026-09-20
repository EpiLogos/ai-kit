//! Full production Service path: no QL, model, Central installation or alternate
//! document resolver. The controlled source files are only test data.
use aikit_cli::app::Service;
use aikit_core::{resource::SourceRef, KnowledgeAddress};
use aikit_store::AikitHome;
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

fn corpus(temp: &TempDir) {
    let values = vec![
        json!({"binding":{"source":"source:a","revision":"r1","title":"Alpha","tags":["notes"],"visibility":"public","owners":[],"media_type":"text/markdown","locator":{"kind":"path","value":"/world/a.md"},"metadata":{}},"body":"---\ntitle: Alpha\naliases: [Opening]\ntags: [notes, research]\n---\n# Alpha\n\n🌱 Follow [[Beta#Part|the next note]] and [again](b.md#^claim).\n\nA **strong** relation, *with care*.\n\n- [x] Keep the source\n- [ ] Return\n\n| Relation | Kind |\n| --- | --- |\n| Alpha → Beta | authored |\n\n`[[not a link]]` \\#not-a-tag\n\n> An exact source matters.\n\n<script>window.__injected = true</script>\n\n![Remote image](https://example.invalid/image.png)\n\n[[Missing]] [[Same]]\n"}),
        json!({"binding":{"source":"source:b","revision":"r1","title":"Beta","tags":[],"visibility":"public","owners":[],"media_type":"text/markdown","locator":{"kind":"path","value":"/world/b.md"},"metadata":{}},"body":"# Part\n\nAn exact paragraph. ^claim\n\n[[Alpha]]\n"}),
        json!({"binding":{"source":"source:c","revision":"r1","title":"Same","tags":[],"visibility":"public","owners":[],"media_type":"text/markdown","metadata":{}},"body":"One interpretation."}),
        json!({"binding":{"source":"source:d","revision":"r1","title":"Same","tags":[],"visibility":"public","owners":[],"media_type":"text/markdown","metadata":{}},"body":"Another interpretation."}),
        json!({"binding":{"source":"source:private","revision":"r1","title":"PRIVATE_SENTINEL","tags":[],"visibility":"personal","owners":["another-actor"],"media_type":"text/markdown","metadata":{}},"body":"Never expose PRIVATE_SENTINEL."}),
    ];
    fs::write(
        temp.path().join("source-material.json"),
        serde_json::to_vec_pretty(&values).unwrap(),
    )
    .unwrap();
}
fn service(temp: &TempDir) -> Service {
    Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        temp.path(),
        |_| None,
    )
    .unwrap()
}
fn source(reference: &str) -> KnowledgeAddress {
    KnowledgeAddress::Source(SourceRef::parse(reference).unwrap())
}

#[test]
fn ordinary_corpus_reader_graph_backlinks_and_search_share_native_identity() {
    let temp = TempDir::new().unwrap();
    corpus(&temp);
    let service = service(&temp);
    let a = service
        .knowledge_read_document(&source("source:a"))
        .unwrap();
    assert_eq!(a["document"]["schema"], "aikit.markdown-reading/v1");
    assert_eq!(
        a["content"],
        service
            .knowledge_read(&source("source:a"))
            .unwrap()
            .content
            .unwrap()
    );
    let occurrence = a["document"]["occurrences"].as_array().unwrap();
    assert_eq!(
        occurrence
            .iter()
            .filter(|o| o["state"] == "resolved")
            .count(),
        2
    );
    assert!(occurrence.iter().any(|o| o["state"] == "ambiguous"));
    let related = service
        .knowledge_relations(&source("source:a"), 1, 32, 64)
        .unwrap();
    let own: Vec<_> = related
        .edges
        .iter()
        .filter(|e| e.from.as_str() == "source:a")
        .collect();
    assert_eq!(
        own.len(),
        2,
        "generic Source navigation carries actual authored links, not only Wiki citations"
    );
    assert!(own
        .iter()
        .all(|e| e.reference.is_some() && e.authored_relation.is_some()));
    assert_ne!(own[0].reference, own[1].reference);
    assert!(service
        .knowledge_relations(&source("source:a"), 0, 32, 64)
        .unwrap()
        .edges
        .is_empty());
    let b = service
        .knowledge_read_document(&source("source:b"))
        .unwrap();
    assert_eq!(
        b["document"]["incoming"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["from"] == "source:a")
            .count(),
        2
    );
    let graph = service.knowledge_graph("", 4096, 16384).unwrap();
    assert_eq!(graph["schema"], "aikit.knowledge-graph/v1");
    assert!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["resource"] == "source:a" && n["address"]["kind"] == "source"),
        "{graph}"
    );
    assert_eq!(
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["from"] == "source:a" && e["to"] == "source:b")
            .count(),
        2
    );
    assert!(!serde_json::to_string(&graph)
        .unwrap()
        .contains("PRIVATE_SENTINEL"));
    assert_eq!(
        service.knowledge_graph("", 1, 1).unwrap()["truncated"],
        true
    );
    assert!(service.knowledge_graph("", 0, 1).is_err());
    if let Some(path) = std::env::var_os("WIKI_READER_FIXTURE") {
        fs::write(path,serde_json::to_vec_pretty(&json!({"source:a":a,"source:b":b,"graph":graph,"evidence":"controlled corpus through actual AIKit Service; no installed-world or human acceptance"})).unwrap()).unwrap();
    }
}

#[test]
fn changed_source_rebuild_retracts_occurrences_without_changing_source_identity() {
    let temp = TempDir::new().unwrap();
    corpus(&temp);
    let before = service(&temp)
        .knowledge_read_document(&source("source:a"))
        .unwrap();
    let mut values: Value =
        serde_json::from_slice(&fs::read(temp.path().join("source-material.json")).unwrap())
            .unwrap();
    values[0]["binding"]["revision"] = json!("r2");
    values[0]["body"] = json!("# Changed\n\nNo links remain.\n");
    fs::write(
        temp.path().join("source-material.json"),
        serde_json::to_vec(&values).unwrap(),
    )
    .unwrap();
    let service = service(&temp);
    let after = service
        .knowledge_read_document(&source("source:a"))
        .unwrap();
    assert_eq!(before["resource"], after["resource"]);
    assert_eq!(after["revision"], "r2");
    assert!(after["document"]["occurrences"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        service
            .knowledge_read_document(&source("source:b"))
            .unwrap()["document"]["incoming"],
        json!([])
    );
}
