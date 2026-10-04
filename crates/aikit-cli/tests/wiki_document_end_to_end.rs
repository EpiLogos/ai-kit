//! Full production Service path: no QL, model, Central installation or alternate
//! document resolver. The controlled source files are only test data.
use aikit_cli::app::{CurrentCorpusSelection, Service};
use aikit_core::{resource::SourceRef, KnowledgeAddress};
use aikit_store::AikitHome;
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn corpus(temp: &TempDir) {
    let root = temp.path().join("current-corpus");
    fs::create_dir_all(root.join("withheld")).unwrap();
    fs::write(root.join("withheld/.no-agent-retrieval"), "actual local input withdrawal").unwrap();
    let inputs = [
        ("a.md", "---\nsource_id: a\nrecord_type: book\ntitle_full: Alpha\ntitle: Alpha\naliases: [Opening]\ntags: [notes, research]\n---\n# Alpha\n\n🌱 Follow [[Beta#Part|the next note]] and [again](b.md#^claim).\n\nA **strong** relation, *with care*.\n\n- [x] Keep the source\n- [ ] Return\n\n| Relation | Kind |\n| --- | --- |\n| Alpha → Beta | authored |\n\n`[[not a link]]` \\#not-a-tag\n\n> An exact source matters.\n\n<script>window.__injected = true</script>\n\n![Remote image](https://example.invalid/image.png)\n\n[[Missing]] [[Same]]\n"),
        ("b.md", "---\nsource_id: b\nrecord_type: book\ntitle_full: Beta\n---\n# Part\n\nAn exact paragraph. ^claim\n\n[[Alpha]]\n"),
        ("c.md", "---\nsource_id: c\nrecord_type: book\ntitle_full: Same\n---\nOne interpretation."),
        ("d.md", "---\nsource_id: d\nrecord_type: book\ntitle_full: Same\n---\nAnother interpretation."),
        ("withheld/private.md", "---\nsource_id: private\nrecord_type: book\ntitle_full: PRIVATE_SENTINEL\n---\nNever expose PRIVATE_SENTINEL."),
    ];
    for (relative, body) in inputs {
        fs::write(root.join(relative), body).unwrap();
    }
}
fn service(temp: &TempDir) -> Service {
    Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        temp.path(),
        |_| None,
    )
    .unwrap()
    .with_current_corpus_selection(CurrentCorpusSelection {
        corpus: temp.path().join("current-corpus"),
        extension: "md".into(),
        room_depth: 1,
    })
    .expect("select actual input bytes; the marked private member remains withheld")
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
        .knowledge_read_document(&source("central:source:corpus:a"))
        .unwrap();
    assert_eq!(a["document"]["schema"], "aikit.markdown-reading/v1");
    assert_eq!(
        a["content"],
        service
            .knowledge_read(&source("central:source:corpus:a"))
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
        .knowledge_relations(&source("central:source:corpus:a"), 1, 32, 64)
        .unwrap();
    let own: Vec<_> = related
        .edges
        .iter()
        .filter(|e| e.from.as_str() == "central:source:corpus:a")
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
        .knowledge_relations(&source("central:source:corpus:a"), 0, 32, 64)
        .unwrap()
        .edges
        .is_empty());
    let b = service
        .knowledge_read_document(&source("central:source:corpus:b"))
        .unwrap();
    assert_eq!(
        b["document"]["incoming"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["from"] == "central:source:corpus:a")
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
            .any(|n| n["resource"] == "central:source:corpus:a" && n["address"]["kind"] == "source"),
        "{graph}"
    );
    assert_eq!(
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["from"] == "central:source:corpus:a" && e["to"] == "central:source:corpus:b")
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
        .knowledge_read_document(&source("central:source:corpus:a"))
        .unwrap();
    let changed = "---\nsource_id: a\nrecord_type: book\ntitle_full: Alpha\n---\n# Changed\n\nNo links remain.\n";
    fs::write(temp.path().join("current-corpus/a.md"), changed).unwrap();
    let revision = aikit_core::knowledge_ingest::corpus_content_revision(changed.as_bytes());
    let service = service(&temp);
    let after = service
        .knowledge_read_document(&source("central:source:corpus:a"))
        .unwrap();
    assert_eq!(before["resource"], after["resource"]);
    assert_eq!(after["revision"], revision);
    assert_ne!(before["revision"], after["revision"]);
    assert!(after["document"]["occurrences"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        service
            .knowledge_read_document(&source("central:source:corpus:b"))
            .unwrap()["document"]["incoming"],
        json!([])
    );
}
