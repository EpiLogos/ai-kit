//! Folder subjects, end to end: a project context materialises its own
//! ProjectCentral register as the directory basis of its knowledge graph —
//! folder nodes under the project-root anchor, `contains`-wired to the
//! file-level subjects in the same materialised set — and never another
//! project's. `.no-agent-retrieval` subtrees get no folder subjects.
//!
//! `CENTRAL_CTRL_BIN` is pointed at a nonexistent executable so the run is
//! deterministic in any environment: the inherited world graph is withheld
//! and the project context rebuilds its own ground (nodes, spaces, authored
//! edges) from its public filesystem binding — exactly the surface the
//! folder basis hangs from.

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

fn manifest(project_id: &str) -> String {
    format!(
        r#"{{
          "schema":"central.project/v1",
          "project_id":"{project_id}",
          "human_source":"ProjectCentral/user",
          "wiki":{{
            "profile":"okf-wiki/v1",
            "source":"ProjectCentral/agents/wiki/wiki.json",
            "adopted_sources":[]
          }}
        }}"#
    )
}

/// One project's ProjectCentral register: manifest, an accepted ground
/// relation for `ProjectCentral/user/alpha.md`, the note itself, and the
/// canonical wiki carrying the project space, the project-root anchor and
/// one content node.
fn projectcentral(project: &Path, project_id: &str, work_name: &str) {
    write(
        &project.join("ProjectCentral/project.json"),
        &manifest(project_id),
    );
    write(
        &project.join("ProjectCentral/relations/source-relations.json"),
        &format!(
            r#"{{
              "schema":"central.project.ground-relations/v1",
              "project_id":"{project_id}",
              "relations":[{{
                "ref":"source:{work_name}:alpha",
                "path":"ProjectCentral/user/alpha.md",
                "provenance":"human-authored",
                "standing":"authored-human-position",
                "roles":["working-note"],
                "treatment":"projectcentral-user",
                "recognition":"human-accepted source relation",
                "recorded_at_unix_seconds":1
              }}]
            }}"#
        ),
    );
    write(
        &project.join("ProjectCentral/user/alpha.md"),
        "Alpha explicitly links [[Beta]] and [[Future Concept]].\n",
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        &format!(
            r#"{{
              "profile":"okf-wiki/v1",
              "objects":[
                {{
                  "profile":"okf-wiki/v1",
                  "object":"space",
                  "ref":"central:wiki:project:{project_id}",
                  "revision":1,
                  "provenance":[],
                  "title":"{work_name}",
                  "parent_space_refs":[],
                  "child_space_refs":[],
                  "node_refs":["wiki:node:project-root/{work_name}"],
                  "anchor_ref":"wiki:node:project-root/{work_name}"
                }},
                {{
                  "profile":"okf-wiki/v1",
                  "object":"node",
                  "ref":"wiki:node:project-root/{work_name}",
                  "revision":1,
                  "provenance":[],
                  "type":"project-root",
                  "title":"{work_name}",
                  "space_refs":["central:wiki:project:{project_id}"],
                  "source_refs":["ProjectCentral/project.json"]
                }},
                {{
                  "profile":"okf-wiki/v1",
                  "object":"node",
                  "ref":"wiki:node:beta",
                  "revision":1,
                  "provenance":[],
                  "type":"Concept",
                  "title":"Beta",
                  "space_refs":[],
                  "source_refs":[]
                }}
              ]
            }}"#
        ),
    );
}

fn world() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".aikit")).unwrap();
    fs::create_dir_all(root.join("Control/nested")).unwrap();
    fs::create_dir_all(root.join("Work")).unwrap();
    projectcentral(&root.join("Work/demo"), "epilogos/demo", "demo");
    projectcentral(&root.join("Work/other"), "epilogos/other", "other");
    // A withheld subtree inside demo's register: no folder subject may ever
    // name it.
    write(
        &root.join("Work/demo/ProjectCentral/user/private/secret.md"),
        "private\n",
    );
    write(
        &root.join("Work/demo/ProjectCentral/user/private/.no-agent-retrieval"),
        "",
    );
    temp
}

fn open_service(temp: &TempDir, cwd: &Path) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    let root = temp.path().display().to_string();
    Service::open(home, cwd, |key| {
        (key == "CENTRAL_ROOT").then(|| root.clone())
    })
    .expect("open production application service")
}

fn address_of(service: &Service, resource: &str) -> Option<KnowledgeAddress> {
    service
        .knowledge_address(&ResourceRef::parse(resource).unwrap())
        .unwrap()
}

fn wiki_hits<'a>(
    hits: &'a [aikit_core::knowledge_navigation::KnowledgeSearchHit],
    needle: &str,
) -> Vec<&'a str> {
    hits.iter()
        .filter_map(|hit| match &hit.address {
            KnowledgeAddress::Wiki(resource) => resource
                .as_str()
                .contains(needle)
                .then(|| resource.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_project_context_materialises_its_own_folder_basis_under_the_anchor() {
    // Deterministic run: the world-binding executable does not exist.
    std::env::set_var("CENTRAL_CTRL_BIN", "/nonexistent/aikit-test-ctrl");
    let temp = world();
    let root = temp.path();

    // -- The project context (standing in Work/demo) -----------------------
    let service = open_service(&temp, &root.join("Work/demo"));

    // The deterministic withhold is in force, so what follows is proven
    // against the project's own rebuilt ground, not an inherited world read.
    let status: KnowledgeProviderStatus = service.knowledge_status().unwrap();
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("withheld, not broadened")),
        "{:?}",
        status.absences
    );

    // The register and its folders are addressable subjects in this context.
    assert!(matches!(
        address_of(&service, "wiki:node:project-dir/demo/ProjectCentral"),
        Some(KnowledgeAddress::Wiki(_))
    ));
    assert!(matches!(
        address_of(&service, "wiki:node:project-dir/demo/ProjectCentral/user"),
        Some(KnowledgeAddress::Wiki(_))
    ));

    // The register hangs from the project-root anchor: its relation view
    // reaches the anchor through the compiled parent edge.
    let register = address_of(&service, "wiki:node:project-dir/demo/ProjectCentral").unwrap();
    let relations = service.knowledge_relations(&register, 1, 50, 50).unwrap();
    let nodes: Vec<&str> = relations
        .nodes
        .iter()
        .map(|node| node.resource.as_str())
        .collect();
    assert!(
        nodes.contains(&"wiki:node:project-root/demo"),
        "expected the project-root anchor reachable from the register folder, got {nodes:?}"
    );

    // The user folder contains its immediate disclosed file-level subject —
    // the file's own source ref, never a second subject.
    let user = address_of(&service, "wiki:node:project-dir/demo/ProjectCentral/user").unwrap();
    let relations = service.knowledge_relations(&user, 1, 50, 50).unwrap();
    let nodes: Vec<&str> = relations
        .nodes
        .iter()
        .map(|node| node.resource.as_str())
        .collect();
    assert!(
        nodes.contains(&"source:demo:alpha"),
        "expected the disclosed file subject reachable from its folder, got {nodes:?}"
    );

    // Scoped to this project: the sibling's folder basis never materialises.
    assert_eq!(
        address_of(&service, "wiki:node:project-dir/other/ProjectCentral"),
        None,
        "another project's folder basis must not appear in this context"
    );

    // A withheld subtree gets no folder subject at all.
    assert_eq!(
        address_of(
            &service,
            "wiki:node:project-dir/demo/ProjectCentral/user/private"
        ),
        None,
        "a .no-agent-retrieval subtree must not become folder subjects"
    );

    // -- The world context (standing in Control/) --------------------------
    let service = open_service(&temp, &root.join("Control/nested"));
    assert!(matches!(
        address_of(&service, "wiki:node:project-dir/demo/ProjectCentral"),
        Some(KnowledgeAddress::Wiki(_))
    ));
    assert!(matches!(
        address_of(&service, "wiki:node:project-dir/other/ProjectCentral"),
        Some(KnowledgeAddress::Wiki(_))
    ));

    // An explicit scope in the query governs the folder basis: demo's scope
    // keeps the other project's folder subjects out of its reply.
    let scoped = service
        .knowledge_search(": demo ( ProjectCentral )", 50)
        .unwrap();
    let folder_hits = wiki_hits(&scoped.hits, "wiki:node:project-dir/");
    assert!(
        folder_hits
            .iter()
            .any(|reference| reference.contains("project-dir/demo/")),
        "expected demo's folder subjects in its scoped reply, got {folder_hits:?}"
    );
    assert!(
        folder_hits
            .iter()
            .all(|reference| !reference.contains("project-dir/other/")),
        "the other project's folder basis must stay out of demo's scope, got {folder_hits:?}"
    );

    let scoped = service
        .knowledge_search(": other ( ProjectCentral )", 50)
        .unwrap();
    let folder_hits = wiki_hits(&scoped.hits, "wiki:node:project-dir/");
    assert!(
        folder_hits
            .iter()
            .all(|reference| !reference.contains("project-dir/demo/")),
        "demo's folder basis must stay out of the other project's scope, got {folder_hits:?}"
    );
}
