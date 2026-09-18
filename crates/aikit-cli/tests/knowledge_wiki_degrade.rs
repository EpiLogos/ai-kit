//! The read side degrades with named absences; the write gate stays strict.
//!
//! The commissioning fault: ONE dangling `child_space_refs` entry used to
//! abort the whole SemanticWiki materialisation, switching the knowledge
//! faculty off for the project. Here, one dangling ref in an otherwise
//! healthy world still yields Wiki search hits, and the repair is disclosed
//! as a named absence carrying kind, declaring space and unresolved ref.

use std::fs;

use aikit_cli::app::Service;
use aikit_core::KnowledgeAddress;
use aikit_store::AikitHome;
use tempfile::TempDir;

#[test]
fn one_dangling_wiki_ref_degrades_to_a_named_absence_and_keeps_search_alive() {
    let temp = TempDir::new().unwrap();
    let wiki = r#"{
      "objects": [
        {
          "profile": "okf-wiki/v1",
          "object": "space",
          "ref": "wiki:space:project",
          "revision": 1,
          "title": "Project",
          "child_space_refs": ["wiki:space:ghost"],
          "node_refs": ["wiki:node:authentication"]
        },
        {
          "profile": "okf-wiki/v1",
          "object": "node",
          "ref": "wiki:node:authentication",
          "revision": 1,
          "type": "Concept",
          "title": "Authentication architecture",
          "space_refs": ["wiki:space:project"],
          "source_refs": ["source:paper:authentication"]
        }
      ]
    }"#;
    fs::write(temp.path().join("semantic-wiki.json"), wiki).unwrap();

    let home = AikitHome::at(temp.path().join("aikit-home"));
    let service = Service::open(home, temp.path(), |_| None).expect("open service");

    // The Wiki faculty is ON, and the repair is disclosed by kind, declaring
    // space and unresolved ref.
    let status = service.knowledge_status().unwrap();
    assert!(
        status.wiki.is_some(),
        "one dangling ref must not switch the Wiki faculty off: {status:?}"
    );
    let repair_line = status
        .absences
        .iter()
        .find(|line| line.contains("knowledge.wiki_space_missing_child"))
        .expect("the repair is disclosed as a named absence");
    assert!(
        repair_line.contains("wiki:space:project") && repair_line.contains("wiki:space:ghost"),
        "the absence names the declaring space and the dangling ref: {repair_line}"
    );

    // Search answers through the repaired read index.
    let result = service.knowledge_search("authentication", 50).unwrap();
    assert!(
        result
            .hits
            .iter()
            .any(|hit| matches!(hit.address, KnowledgeAddress::Wiki(_))),
        "Wiki search still answers past one dangling ref: {:?}",
        result.hits
    );
}
