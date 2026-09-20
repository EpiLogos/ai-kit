//! A Central project keeps corpus-local SourcePool material it owns; it does
//! not let a copied shard impersonate a live Central control-root source.

use std::fs;
use std::path::Path;

use aikit_cli::app::Service;
use aikit_core::KnowledgeAddress;
use aikit_store::AikitHome;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn world() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("Control")).unwrap();
    fs::create_dir_all(root.join("Work")).unwrap();
    let project = root.join("Work/demo");
    write(
        &project.join("ProjectCentral/project.json"),
        r#"{
          "schema":"central.project/v1",
          "project_id":"epilogos/demo",
          "human_source":"ProjectCentral/user",
          "wiki":{
            "profile":"okf-wiki/v1",
            "source":"ProjectCentral/agents/wiki/wiki.json",
            "adopted_sources":[]
          }
        }"#,
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        r#"{
          "profile":"okf-wiki/v1",
          "objects":[{
            "profile":"okf-wiki/v1",
            "object":"node",
            "ref":"wiki:node:evidence",
            "revision":1,
            "provenance":[],
            "type":"Evidence",
            "title":"Evidence",
            "space_refs":[],
            "source_refs":[
              "central:source:corpus:local",
              "central:source:control:root:live"
            ]
          }]
        }"#,
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.sources/corpus-000.json"),
        r#"[
          {
            "binding": {
              "source":"central:source:corpus:local",
              "revision":"r1",
              "title":"Local corpus evidence",
              "tags":[],
              "visibility":"public",
              "owners":[],
              "media_type":"text/markdown",
              "locator":{"kind":"path","value":"local.md"},
              "metadata":{"origin":"project-corpus"}
            },
            "body":"Generated interpretation is in this local corpus evidence."
          },
          {
            "binding": {
              "source":"central:source:control:root:live",
              "revision":"stale",
              "title":"Impersonated live source",
              "tags":[],
              "visibility":"public",
              "owners":[],
              "media_type":"text/markdown",
              "locator":{"kind":"path","value":"stale.md"},
              "metadata":{"origin":"stale-copy"}
            },
            "body":"A stale copied live body must not bypass Central."
          }
        ]"#,
    );
    temp
}

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, &temp.path().join("Work/demo"), |_| None)
        .expect("open production application service")
}

#[test]
fn central_keeps_corpus_local_sources_and_withholds_control_root_copies() {
    std::env::set_var("CENTRAL_CTRL_BIN", "/nonexistent/aikit-test-ctrl");
    let temp = world();
    let service = open_service(&temp);

    let search = service
        .knowledge_search("generated interpretation", 50)
        .unwrap();
    let local = search.hits.iter().find(|hit| {
        matches!(&hit.address, KnowledgeAddress::Source(source)
            if source.as_str() == "central:source:corpus:local")
    });
    assert!(local.is_some(), "corpus-local source stayed searchable");

    let live = search.hits.iter().find(|hit| {
        matches!(&hit.address, KnowledgeAddress::Source(source)
            if source.as_str() == "central:source:control:root:live")
    });
    assert!(
        live.is_none()
            || matches!(live, Some(hit) if matches!(&hit.address, KnowledgeAddress::Wiki(_))),
        "a control-root body copy must not become a SourcePool hit"
    );

    let local_source = aikit_core::SourceRef::parse("central:source:corpus:local").unwrap();
    let reading = service
        .knowledge_read(&KnowledgeAddress::Source(local_source))
        .unwrap();
    assert_eq!(reading.revision.as_deref(), Some("r1"));
    assert!(reading
        .content
        .as_deref()
        .is_some_and(|body| body.contains("Generated interpretation")));

    let live_source = aikit_core::SourceRef::parse("central:source:control:root:live").unwrap();
    let error = service
        .knowledge_read(&KnowledgeAddress::Source(live_source))
        .expect_err("a copied live source remains unreadable without its owner");
    assert_eq!(error.code(), "knowledge.source_cited_but_unmaterialised");

    std::env::remove_var("CENTRAL_CTRL_BIN");
}
