//! Shape-derived project scoping: the Work member the invocation stands in
//! scopes its knowledge context, whatever markers, registration or manifests
//! say. The discovery matrix pins scoped-vs-world behaviour per directory
//! shape under a world root that carries the only `.aikit` on the walk up —
//! the collapse case — alongside the rescued shapes that already worked.
//!
//! Assertions are deliberately invariant to whether a `ctrl` executable is
//! available to answer world-binding questions: the scoping decided here (the
//! derived member, the lowered query scope, the per-scope rollup and edge
//! attribution) is upstream of that read, and the withholding law itself is
//! pinned in `project_scoped_withhold.rs` and the adapter tests.

use std::fs;
use std::path::Path;

use aikit_cli::app::Service;
use aikit_core::KnowledgeAddress;
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// One project's ProjectCentral register: a manifest, one disclosed authored
/// source, a note linking a resolvable target (`name`) and a pending one, and
/// the canonical wiki holding that target as a node.
fn projectcentral(project: &Path, project_id: &str, target: &str) {
    let target_lower = target.to_lowercase();
    write(
        &project.join("ProjectCentral/project.json"),
        &format!(
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
        ),
    );
    let source = format!(
        "source:{}:alpha",
        project_id.rsplit('/').next().unwrap_or(project_id)
    );
    write(
        &project.join("ProjectCentral/relations/source-relations.json"),
        &format!(
            r#"{{
              "schema":"central.project.ground-relations/v1",
              "project_id":"{project_id}",
              "relations":[{{
                "ref":"{source}",
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
        &format!("Alpha explicitly links [[{target}]] and [[Future Concept]].\n"),
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        &format!(
            r#"{{
              "profile":"okf-wiki/v1",
              "objects":[{{
                "profile":"okf-wiki/v1",
                "object":"node",
                "ref":"wiki:node:{target_lower}",
                "revision":1,
                "provenance":[],
                "type":"Concept",
                "title":"{target}",
                "space_refs":[],
                "source_refs":[]
              }}]
            }}"#
        ),
    );
}

/// A Central world whose root carries the only `.aikit` on the walk up from
/// every member: a member without a marker of its own has its project root
/// resolved to the world root by profile discovery, which is exactly the
/// collapse the shape-derived scoping exists to answer.
fn collapse_world() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".aikit")).unwrap();
    fs::create_dir_all(root.join("Control/nested")).unwrap();
    fs::create_dir_all(root.join("Work")).unwrap();
    // Manifest only: the ProjectCentral rescue applies (unchanged behaviour).
    projectcentral(&root.join("Work/manifest"), "epilogos/manifest", "Beta");
    // Marker only: no rescue, no manifest — scoping must come from shape.
    fs::create_dir_all(root.join("Work/marked/.aikit")).unwrap();
    // Neither marker nor manifest: the same shape answer, honestly disclosed.
    fs::create_dir_all(root.join("Work/bare")).unwrap();
    // Both: the rescue applies and the marker joins the chain.
    fs::create_dir_all(root.join("Work/both/.aikit")).unwrap();
    projectcentral(&root.join("Work/both"), "epilogos/both", "Gamma");
    // A nested worktree under O-I: the immediate Work member classifies it.
    projectcentral(&root.join("Work/O-I"), "epilogos/o-i", "Delta");
    fs::create_dir_all(root.join("Work/O-I/.worktrees/main/.aikit")).unwrap();
    projectcentral(
        &root.join("Work/O-I/.worktrees/main"),
        "epilogos/o-i-worktree",
        "Epsilon",
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

fn authored_edge_hits(service: &Service, query: &str) -> Vec<String> {
    service
        .knowledge_search(query, 50)
        .expect("search materialises")
        .hits
        .into_iter()
        .filter_map(|hit| match hit.address {
            KnowledgeAddress::Wiki(resource) => resource
                .as_str()
                .starts_with("wiki:edge:authored:")
                .then(|| resource.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

fn pending_absences(service: &Service, query: &str) -> Vec<String> {
    service
        .knowledge_search(query, 50)
        .expect("search materialises")
        .absences
        .into_iter()
        .filter(|absence| absence.contains("pending"))
        .collect()
}

#[test]
fn the_discovery_matrix_scopes_by_directory_shape() {
    let temp = collapse_world();
    let root = temp.path();

    // ProjectCentral only: rescued by the manifest; scoped to its own graph.
    let service = open_service(&temp, &root.join("Work/manifest"));
    assert_eq!(
        service.context().project_root.as_ref().unwrap(),
        &root.join("Work/manifest").canonicalize().unwrap()
    );
    assert_eq!(
        authored_edge_hits(&service, "beta").len(),
        1,
        "own authored edge reachable from its own project"
    );
    let pendings = pending_absences(&service, "alpha");
    assert_eq!(pendings.len(), 1, "{pendings:?}");
    assert!(pendings[0].starts_with("Work/manifest:"), "{pendings:?}");

    // Marker only: profile discovery collapses the project root onto the
    // world root, and the context is still the project's — the manifest edge
    // from the sibling stays out of the reply, and the missing manifest is
    // disclosed instead of guessed around.
    let service = open_service(&temp, &root.join("Work/marked"));
    assert_eq!(
        service.context().project_root.as_ref().unwrap(),
        &root.canonicalize().unwrap(),
        "the collapse is real: discovery resolves the world root"
    );
    assert!(
        authored_edge_hits(&service, "beta").is_empty(),
        "a scoped context keeps the sibling project's edges out"
    );
    let status = service.knowledge_status().unwrap();
    assert!(
        status.absences.iter().any(|absence| {
            absence.contains("Work/marked")
                && absence.contains("has no ProjectCentral manifest")
                && absence.contains("project:marked")
        }),
        "{:?}",
        status.absences
    );

    // Neither marker nor manifest: the same shape answer as marker-only.
    let service = open_service(&temp, &root.join("Work/bare"));
    assert_eq!(
        service.context().project_root.as_ref().unwrap(),
        &root.canonicalize().unwrap()
    );
    assert!(authored_edge_hits(&service, "beta").is_empty());
    let status = service.knowledge_status().unwrap();
    assert!(
        status
            .absences
            .iter()
            .any(|absence| absence.contains("Work/bare") && absence.contains("project:bare")),
        "{:?}",
        status.absences
    );
    // Scoping must not depend on the wiki being populated: a bare member's
    // search still materialises and carries no other project's rollup.
    let search = service.knowledge_search("alpha", 50).unwrap();
    assert!(
        search
            .absences
            .iter()
            .all(|absence| !absence.contains("Work/manifest")),
        "{:?}",
        search.absences
    );

    // Both marker and manifest: the rescue applies; the marker joins the
    // chain but does not change the project.
    let service = open_service(&temp, &root.join("Work/both"));
    assert_eq!(
        service.context().project_root.as_ref().unwrap(),
        &root.join("Work/both").canonicalize().unwrap()
    );
    assert_eq!(
        authored_edge_hits(&service, "gamma").len(),
        1,
        "own authored edge reachable from its own project"
    );
    assert!(
        authored_edge_hits(&service, "beta").is_empty(),
        "the manifest project's edge stays out of both's scope"
    );

    // A worktree nested under O-I classifies as O-I: the immediate Work
    // member is the project, and its scope is the worktree's scope.
    let service = open_service(&temp, &root.join("Work/O-I/.worktrees/main"));
    assert_eq!(
        service.context().project_root.as_ref().unwrap(),
        &root.join("Work/O-I").canonicalize().unwrap()
    );
    let pendings = pending_absences(&service, "alpha");
    assert_eq!(pendings.len(), 1, "{pendings:?}");
    assert!(pendings[0].starts_with("Work/O-I:"), "{pendings:?}");
    assert!(
        authored_edge_hits(&service, "beta").is_empty(),
        "the manifest project's edge stays out of O-I's scope"
    );

    // A Control location is never guessed into a project: world-root
    // behaviour — no scope, so no rollup line and no edge filtering.
    let service = open_service(&temp, &root.join("Control/nested"));
    assert_eq!(
        service.context().project_root.as_ref().unwrap(),
        &root.canonicalize().unwrap()
    );
    let search = service.knowledge_search("alpha", 50).unwrap();
    assert!(
        search
            .absences
            .iter()
            .all(|absence| !absence.contains("pending")),
        "an unscoped world reply carries no project rollup: {:?}",
        search.absences
    );
    assert_eq!(
        authored_edge_hits(&service, "beta").len(),
        1,
        "unscoped world behaviour keeps the graph whole"
    );
}

#[test]
fn an_explicit_scope_in_the_query_overrides_the_derived_default() {
    let temp = collapse_world();
    // Standing in manifest, the derived default is manifest; the explicit
    // `: both (…)` must win — manifest's own edge leaves the reply, exactly
    // as it would from inside both. The grammar is the surface; no flags.
    let service = open_service(&temp, &root_of(&temp).join("Work/manifest"));
    let scoped = service
        .knowledge_search(": both ( beta )", 50)
        .expect("scoped search materialises");
    let edge_hits = scoped
        .hits
        .into_iter()
        .filter(|hit| matches!(&hit.address, KnowledgeAddress::Wiki(resource) if resource.as_str().starts_with("wiki:edge:authored:")))
        .count();
    assert_eq!(edge_hits, 0, "the explicit scope governed the reply");
    // And the explicit scope carries both's own pending rollup, not
    // manifest's.
    let scoped = service.knowledge_search(": both ( alpha )", 50).unwrap();
    let pendings: Vec<&String> = scoped
        .absences
        .iter()
        .filter(|absence| absence.contains("pending"))
        .collect();
    assert_eq!(pendings.len(), 1, "{:?}", scoped.absences);
    assert!(
        pendings[0].starts_with("Work/both:"),
        "{:?}",
        scoped.absences
    );
}

fn root_of(temp: &TempDir) -> std::path::PathBuf {
    temp.path().to_path_buf()
}
