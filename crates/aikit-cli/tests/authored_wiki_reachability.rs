//! CASE 19 / W10 V9.4 — authored ProjectCentral Markdown reachability.
//!
//! `projectcentral_authored_wiki` (crates/aikit-adapters) was a complete,
//! tested pipeline with zero call sites: a project's `ProjectCentral/user/**`
//! Markdown never reached `materialize_knowledge_runtime`, so `[[wikilinks]]`
//! contributed nothing to `aikit knowledge search/relations/sources/status`.
//! These acceptance tests exercise the chained-in path end to end: resolved
//! `[[links]]` become ordinary Compiled/Authored `WikiEdge`s reachable
//! through the ordinary knowledge surface, unresolved links stay disclosed
//! as absences (never synthetic edges), and a broken project or file
//! degrades the whole command fail-open rather than aborting it.

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
fn authored_markdown_wikilinks_reach_the_knowledge_surface() {
    let temp = central_world_with_one_project();
    let project = temp.path().join("Work/demo");
    let service = open_service(&temp, &project);

    // status: the pending [[Future Concept]] link is disclosed as this
    // project's structured rollup — one line in absences, per-target detail
    // in the structured field — never a synthetic edge.
    let status = service.knowledge_status().expect("status materialises");
    assert_eq!(status.authored_pending.len(), 1, "{:?}", status.absences);
    assert_eq!(status.authored_pending[0].project, "Work/demo");
    assert_eq!(status.authored_pending[0].unresolved_targets, 1);
    assert_eq!(status.authored_pending[0].occurrences, 1);
    assert_eq!(
        status.authored_pending[0].targets[0].target,
        "Future Concept"
    );
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("Work/demo")
                && absence.contains("1 unresolved target across 1 occurrence pending")),
        "expected the pending rollup line, got {:?}",
        status.absences
    );
    assert!(
        status
            .absences
            .iter()
            .all(|absence| !absence.contains("Future Concept")),
        "per-target detail is status-structured, not absence noise: {:?}",
        status.absences
    );

    // search: the resolved [[Beta]] link is reachable as an ordinary
    // Authored WikiEdge through the same search surface as everything else.
    let hits = service
        .knowledge_search("beta", 50)
        .expect("search materialises")
        .hits;
    let edge_hit = hits
        .iter()
        .find(|hit| match &hit.address {
            KnowledgeAddress::Wiki(resource) => {
                resource.as_str().starts_with("wiki:edge:authored:")
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("expected an authored edge hit for 'beta', got {hits:?}"));

    // relations: the same edge is reachable from the relation surface, and
    // names its authored subject through the edge's own provenance. (The
    // resolved target `wiki:node:beta` itself is only materialised as a
    // full graph node once the canonical project Wiki is loaded — the
    // separate, pre-existing `central_wiki`/`ctrl` path this fixture does
    // not exercise; this change is responsible for the edge, not for that.)
    let relations = service
        .knowledge_relations(&edge_hit.address, 1, 50, 50)
        .expect("relations materialises");
    let node_refs: Vec<String> = relations
        .nodes
        .iter()
        .map(|node| node.resource.as_str().to_owned())
        .collect();
    assert!(
        node_refs
            .iter()
            .any(|reference| reference == "source:demo:alpha"),
        "expected the authored subject among relation nodes, got {node_refs:?}"
    );
    assert!(
        relations
            .edges
            .iter()
            .any(|edge| edge.to.as_str() == "source:demo:alpha"),
        "expected a relation edge naming the authored source, got {:?}",
        relations.edges
    );

    // sources: the edge's own provenance names the exact authored Markdown
    // source that carried the link.
    let sources = service
        .knowledge_sources(&edge_hit.address)
        .expect("sources materialises");
    assert!(
        sources
            .sources
            .iter()
            .any(|source_ref| source_ref.as_str() == "source:demo:alpha"),
        "expected the authored source among disclosed sources, got {:?}",
        sources.sources
    );
}

#[test]
fn relative_links_resolve_and_scoped_searches_see_only_their_scope() {
    let temp = central_world_with_one_project();
    let project = temp.path().join("Work/demo");
    // A repo file outside the ProjectCentral register, cited by the note via
    // an ordinary relative Markdown link, and repeated identical pendings.
    fs::write(project.join("README.md"), "# demo repo\n").unwrap();
    fs::write(
        project.join("ProjectCentral/user/alpha.md"),
        "Alpha links [[Beta]], cites [the README](../../README.md) and pends [[Future Concept]], [[Future Concept]], [[Future Concept]].\n",
    )
    .unwrap();
    let service = open_service(&temp, &project);

    // The relative link to the real repo file resolves to a real authored
    // edge; the identical pendings collapse into one counted rollup.
    let status = service.knowledge_status().expect("status materialises");
    assert_eq!(status.authored_pending.len(), 1, "{:?}", status.absences);
    assert_eq!(status.authored_pending[0].unresolved_targets, 1);
    assert_eq!(status.authored_pending[0].occurrences, 3);
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("3 occurrences pending")),
        "{:?}",
        status.absences
    );
    let search = service.knowledge_search("readme", 50).unwrap();
    assert!(
        search.hits.iter().any(|hit| matches!(
            &hit.address,
            KnowledgeAddress::Wiki(resource)
                if resource.as_str().starts_with("wiki:edge:authored:")
        )),
        "expected a resolved edge to the repo README, got {:?}",
        search.hits
    );
    // A search reply carries its own scope's rollup — not noise per
    // occurrence.
    assert_eq!(
        search
            .absences
            .iter()
            .filter(|absence| absence.contains("pending"))
            .count(),
        1,
        "{:?}",
        search.absences
    );
}

#[test]
fn a_scoped_query_hides_other_projects_absences_and_edges() {
    let temp = central_world_with_one_project();
    // A second project whose note pends on its own target and links a real
    // sibling file.
    let sibling = temp.path().join("Work/other");
    fs::create_dir_all(sibling.join("ProjectCentral/user")).unwrap();
    fs::write(
        sibling.join("ProjectCentral/project.json"),
        r#"{
          "schema":"central.project/v1",
          "project_id":"epilogos/other",
          "human_source":"ProjectCentral/user",
          "wiki":{
            "profile":"okf-wiki/v1",
            "source":"ProjectCentral/agents/wiki/wiki.json",
            "adopted_sources":[]
          }
        }"#,
    )
    .unwrap();
    write(
        &sibling.join("ProjectCentral/agents/wiki/wiki.json"),
        r#"{ "profile":"okf-wiki/v1", "objects":[] }"#,
    );
    fs::write(
        sibling.join("ProjectCentral/user/note.md"),
        "Sibling pends [[Other Future]] and cites [sibling readme](../../README.md).\n",
    )
    .unwrap();
    fs::write(sibling.join("README.md"), "# other repo\n").unwrap();

    let project = temp.path().join("Work/demo");
    let service = open_service(&temp, &project);

    // A demo-scoped search (the lowered cwd scope) sees only demo's rollup:
    // one pending line, and it names demo's target — not the sibling's.
    let scoped = service.knowledge_search("alpha", 50).unwrap();
    let pending_lines: Vec<&String> = scoped
        .absences
        .iter()
        .filter(|absence| absence.contains("pending"))
        .collect();
    assert_eq!(pending_lines.len(), 1, "{:?}", scoped.absences);
    assert!(
        pending_lines[0].starts_with("Work/demo:"),
        "{:?}",
        scoped.absences
    );
    assert!(
        !scoped
            .absences
            .iter()
            .any(|absence| absence.contains("Work/other")),
        "other projects' pendings stay out of a scoped reply: {:?}",
        scoped.absences
    );

    // status is the diagnostic surface that carries every project.
    let status = service.knowledge_status().unwrap();
    assert_eq!(status.authored_pending.len(), 2, "{:?}", status.absences);
    assert!(status.authored_pending.iter().any(|pending| {
        pending.project == "Work/other"
            && pending
                .targets
                .iter()
                .any(|target| target.target == "Other Future")
    }));
}

#[test]
fn a_broken_project_or_file_degrades_fail_open_and_the_command_still_succeeds() {
    let temp = central_world_with_one_project();
    let project = temp.path().join("Work/demo");

    // Break the one eligible Markdown source's frontmatter: an opening
    // delimiter with no closing delimiter is a genuine parse failure.
    write(
        &project.join("ProjectCentral/user/alpha.md"),
        "---\ntype: Concept\nAlpha never closes its frontmatter.\n",
    );
    // A second Work project that never declared a ProjectCentral register
    // at all.
    fs::create_dir_all(temp.path().join("Work/bare")).unwrap();

    let service = open_service(&temp, &project);
    let status = service
        .knowledge_status()
        .expect("a malformed source or an absent ProjectCentral must not abort the command");

    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("alpha.md") && absence.contains("could not be parsed")),
        "expected the malformed source disclosed as an absence, got {:?}",
        status.absences
    );
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("Work/bare") || absence.contains("bare")),
        "expected the ProjectCentral-less project disclosed as an absence, got {:?}",
        status.absences
    );

    // The rest of the command still runs: search does not error either.
    service
        .knowledge_search("anything", 10)
        .expect("search still materialises despite the degraded authored wiki");
}
