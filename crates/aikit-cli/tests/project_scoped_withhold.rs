//! The withholding law, end to end: when the world binding cannot be read
//! (a non-absence failure), the project context withholds the inherited
//! Central graph — never broadens it — and the local authored fallback
//! restores the project's OWN wiki: its nodes and spaces beside its compiled
//! edges, so a restored edge never points at a target the rebuild dropped.
//!
//! `CENTRAL_CTRL_BIN` is pointed at a nonexistent executable so the binding
//! read fails deterministically in any environment; the fixture root carries
//! the only `.aikit` on the walk up, so this also exercises the collapse
//! case (shape-derived member from the invocation cwd) through the withhold.

use std::fs;
use std::path::Path;

use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{KnowledgeAddress, KnowledgeProviderStatus};
use aikit_store::AikitHome;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// One project under a world root that carries the only marker: `Work/demo`
/// has neither a marker nor a rescue-worthy difference — the invocation cwd
/// is the only ground that names the project.
fn world() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".aikit")).unwrap();
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
        &project.join("ProjectCentral/relations/source-relations.json"),
        r#"{
          "schema":"central.project.ground-relations/v1",
          "project_id":"epilogos/demo",
          "relations":[{
            "ref":"source:demo:alpha",
            "path":"ProjectCentral/user/alpha.md",
            "provenance":"human-authored",
            "standing":"authored-human-position",
            "roles":["working-note"],
            "treatment":"projectcentral-user",
            "recognition":"human-accepted source relation",
            "recorded_at_unix_seconds":1
          }]
        }"#,
    );
    write(
        &project.join("ProjectCentral/user/alpha.md"),
        "Alpha explicitly links [[Beta]] and [[Future Concept]].\n",
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        r#"{
          "profile":"okf-wiki/v1",
          "objects":[{
            "profile":"okf-wiki/v1",
            "object":"node",
            "ref":"wiki:node:beta",
            "revision":1,
            "provenance":[],
            "type":"Concept",
            "title":"Beta",
            "space_refs":[],
            "source_refs":[]
          }]
        }"#,
    );
    temp
}

#[test]
fn a_withheld_binding_keeps_the_projects_own_nodes_and_edges() {
    // Deterministic non-absence failure: the binding executable does not
    // exist, so the world read is unavailable — withhold, never broaden.
    std::env::set_var("CENTRAL_CTRL_BIN", "/nonexistent/aikit-test-ctrl");
    let temp = world();
    let service = open_service(&temp);

    // The withhold is disclosed, not silent.
    let status: KnowledgeProviderStatus = service.knowledge_status().unwrap();
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("withheld, not broadened")),
        "{:?}",
        status.absences
    );

    // The project's own authored edge is still reachable (the fallback's
    // existing behaviour) — and its target node comes back WITH it (the
    // fixed gap): a restored edge must not point at a dropped target.
    let search = service.knowledge_search("beta", 50).unwrap();
    assert!(
        search.hits.iter().any(|hit| matches!(&hit.address, KnowledgeAddress::Wiki(resource) if resource.as_str().starts_with("wiki:edge:authored:"))),
        "expected the authored edge, got {:?}",
        search.hits
    );
    assert!(
        search.hits.iter().any(|hit| matches!(&hit.address, KnowledgeAddress::Wiki(resource) if resource.as_str() == "wiki:node:beta")),
        "expected the project's own wiki node among the hits, got {:?}",
        search.hits
    );

    // The node is directly addressable in this context.
    let address = service
        .knowledge_address(&ResourceRef::parse("wiki:node:beta").unwrap())
        .unwrap();
    assert!(matches!(address, Some(KnowledgeAddress::Wiki(_))));

    // And the restored node participates in the graph: its own relation view
    // reaches back to the authored subject through the restored edge.
    let node_address = address.expect("the restored node resolves to an address");
    let relations = service
        .knowledge_relations(&node_address, 1, 50, 50)
        .unwrap();
    let nodes: Vec<&str> = relations
        .nodes
        .iter()
        .map(|node| node.resource.as_str())
        .collect();
    assert!(
        nodes.contains(&"source:demo:alpha"),
        "expected the authored subject reachable from the restored node, got {nodes:?}"
    );

    // The pending rollup is this project's own, scoped as Work/demo.
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.starts_with("Work/demo:") && absence.contains("pending")),
        "{:?}",
        status.absences
    );

    std::env::remove_var("CENTRAL_CTRL_BIN");
}

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, &temp.path().join("Work/demo"), |_| None)
        .expect("open production application service")
}
