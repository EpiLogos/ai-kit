//! CAW privacy regression: keep independently bound project-local authored edges
//! without restoring inherited World or sibling disclosure after owner failure.

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

fn open_service(temp: &TempDir, cwd: &Path) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, cwd, |_| None).expect("open production application service")
}

/// A minimal Central world: `Control/` + `Work/demo/ProjectCentral` with one
/// authored note that explicitly links a resolvable target (`[[Beta]]`,
/// matched against the project's own canonical Wiki) and an unresolvable one
/// (`[[Future Concept]]`).
fn central_world_with_one_project() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("Control")).unwrap();
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
fn unavailable_world_keeps_only_the_current_projects_independently_bound_authored_edges() {
    let temp = central_world_with_one_project();
    let project = temp.path().join("Work/demo");
    // A sibling has a valid, independently readable authored relation. It must
    // not leak into demo merely because the inherited World policy is down.
    let sibling = temp.path().join("Work/private");
    for relative in [
        "ProjectCentral/project.json",
        "ProjectCentral/relations/source-relations.json",
        "ProjectCentral/user/alpha.md",
        "ProjectCentral/agents/wiki/wiki.json",
    ] {
        let bytes = fs::read_to_string(project.join(relative)).unwrap();
        write(
            &sibling.join(relative),
            &bytes
                .replace("epilogos/demo", "epilogos/private")
                .replace("source:demo:alpha", "source:private:alpha")
                .replace("Beta", "PrivateTarget")
                .replace("wiki:node:beta", "wiki:node:private-target"),
        );
    }
    let service = open_service(&temp, &project);
    let status = service.knowledge_status().unwrap();
    assert!(status
        .absences
        .iter()
        .any(|a| a.contains("inherited Central graph withheld")));
    let allowed = service.knowledge_search("beta", 50).unwrap().hits;
    assert!(allowed.iter().any(|hit| matches!(&hit.address,
        KnowledgeAddress::Wiki(reference) if reference.as_str().starts_with("wiki:edge:authored:"))));
    assert!(service
        .knowledge_search("PrivateTarget", 50)
        .unwrap()
        .hits
        .is_empty());
    // Independently readable does not mean all local files: the native
    // no-retrieval marker must still take effect on the next materialisation.
    write(&project.join("ProjectCentral/user/.no-agent-retrieval"), "");
    let service = open_service(&temp, &project);
    assert!(service
        .knowledge_search("beta", 50)
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(
        fs::read_to_string(project.join("ProjectCentral/user/alpha.md")).unwrap(),
        "Alpha explicitly links [[Beta]] and [[Future Concept]].\n"
    );
}
