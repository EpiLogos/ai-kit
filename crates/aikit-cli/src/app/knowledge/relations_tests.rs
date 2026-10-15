//! A relation view materialises only the SemanticWiki and ProjectMap it
//! reads, and answers exactly what the whole runtime answers — including the
//! edges compiled from discovered source material.

use super::*;
use aikit_store::AikitHome;

#[test]
fn a_relation_view_answers_exactly_what_the_whole_runtime_answers() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("notes")).unwrap();
    fs::write(
        project.join("semantic-wiki.json"),
        serde_json::json!({"objects": [
            {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:need","revision":1,
             "provenance":[],"title":"Need","parent_space_refs":[],"child_space_refs":[],
             "node_refs":["wiki:node:alpha","wiki:node:beta"]},
            {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:alpha","revision":1,
             "provenance":[],"type":"Concept","title":"Alpha","space_refs":["wiki:space:need"],
             "source_refs":[]},
            {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:beta","revision":1,
             "provenance":[],"type":"Concept","title":"Beta","space_refs":["wiki:space:need"],
             "source_refs":[]}
        ]})
        .to_string(),
    )
    .unwrap();
    // Discovered material whose Markdown links a wiki node: the walk's
    // contribution to the SemanticWiki a relation view must keep.
    fs::write(
        project.join("notes/note.json"),
        serde_json::json!({
            "binding": {
                "source": "source:need:note", "revision": "rev-1", "title": "Note",
                "tags": [], "visibility": "public", "owners": [],
                "media_type": "text/markdown", "metadata": {}
            },
            "body": "The note develops [[Alpha]]."
        })
        .to_string(),
    )
    .unwrap();
    let service = Service::open(AikitHome::at(temp.path().join("home")), &project, |_| None)
        .expect("a standalone project service");
    let address = KnowledgeAddress::Wiki(ResourceRef::parse("wiki:space:need").unwrap());

    let narrow = service.knowledge_relations(&address, 2, 96, 192).unwrap();
    let whole = service
        .with_knowledge(|_, application| application.relations(&address, 2, 96, 192))
        .unwrap();

    assert_eq!(
        serde_json::to_value(&narrow).unwrap(),
        serde_json::to_value(&whole).unwrap()
    );
    assert!(
        narrow
            .edges
            .iter()
            .any(|edge| edge.from.as_str() == "source:need:note"
                && edge.to.as_str() == "wiki:node:alpha"),
        "the material-compiled edge is part of the view: {:?}",
        narrow.edges
    );
}
