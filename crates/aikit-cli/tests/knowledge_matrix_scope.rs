//! Project-scope acceptance for compiled capability-matrix material.
//!
//! A Project's Knowledge preparation keeps a sibling Project's matrix
//! objects — the compiled nodes, the verification and field-contribution
//! edges, and the raw carrier paths the objects cite — out of its replies,
//! while the root composition stays visible (the root lineage is a
//! distinct, legitimately broader aperture) and each Project's own scope
//! keeps its own matrix. Regression for the ProjectWorld isolation packet
//! (2026-09-25): the sibling channel an installed binary leaked through,
//! which the earlier review could not reproduce because its needles never
//! matched sibling matrix text.
//!
//! The native world-binding owner is stubbed (`CENTRAL_CTRL_BIN`) with the
//! one contract the scoping needs: a Project scope whose declaration is
//! absent (so the root lineage applies by convention) and a root scope that
//! answers. Matrix compilation itself is pure filesystem — the stub only
//! keeps the inherited graph from being withheld as unavailable.

use std::fs;
use std::path::{Path, PathBuf};

use aikit_cli::app::Service;
use aikit_core::knowledge_navigation::KnowledgeSearchHit;
use aikit_store::AikitHome;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A minimal ProjectCentral register: the manifest is what discovery and
/// the project wiki space need.
fn manifest(project: &Path, project_id: &str) {
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
}

/// A valid capability matrix in `dir`: one capability and one relation, the
/// relation carrying `needle` so a text search can reach the compiled
/// field-contribution edge.
fn matrix(dir: &Path, matrix_id: &str, capability: &str, needle: &str) {
    write(
        &dir.join("capability-matrix.json"),
        &format!(
            r#"{{
              "protocol": "ql-capability-matrix/1",
              "matrix_id": "{matrix_id}",
              "anchor_ref": "doc:overview",
              "default_view": "product-field",
              "views": []
            }}"#
        ),
    );
    write(
        &dir.join("capability-matrix.csv"),
        &format!(
            r#"id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question
{capability},capability,,,,[],A {matrix_id} need.,an {matrix_id} operation.,an {matrix_id} outcome.,source-inspected,agent-inference,src/a.md,_,_,_,,,seeds,"{{}}",
rel.1,relation,product-field,q1,S1,["{capability}"],,,,,,,,,,{needle} contribution relation,H,"{{}}",
"#
        ),
    );
}

/// The native owner stub: a Project scope's declaration is absent (the
/// documented root-lineage convention answers), the root scope answers with
/// one available Control source. Any other action fails honestly.
fn stub_ctrl(world: &Path) -> PathBuf {
    let bin = world.join("bin/ctrl-stub");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    write(
        &bin,
        r#"#!/bin/sh
# argv: --json --root <root> action run central.world.effective-sources '<input>'
input="$7"
case "$input" in
  *'"scope":"root"'*)
    printf '%s' '{"ok":true,"data":{"world_ref":"control:root","sources":[{"ref":"central:source:control:root:Control","state":"available","effective_revision":"stub-root-rev-1","propagation_path":["control:root"]}]}}'
    exit 0
    ;;
  *)
    printf '%s' '{"ok":false,"error":{"code":"central.world_declaration_absent","message":"missing World project:stub"}}'
    exit 2
    ;;
esac
"#,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn isolate_home(temp: &TempDir) {
    let home = temp.path().join("isolated-home");
    fs::create_dir_all(&home).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join("xdg-config"));
    std::env::set_var("XDG_CACHE_HOME", temp.path().join("xdg-cache"));
    std::env::set_var("XDG_DATA_HOME", temp.path().join("xdg-data"));
    std::env::set_var("AIKIT_BKMR_CONFIG_DIR", temp.path().join("bkmr-config"));
}

/// Open the production service inside `world`, with code intelligence pinned
/// to a nonexistent binary so the fixture repositories are never indexed and
/// the test stays hermetic.
fn open_service(temp: &TempDir, cwd: &Path, world: &Path) -> Service {
    let root_text = world.display().to_string();
    let no_gitnexus = "/nonexistent/aikit-matrix-scope-test-no-gitnexus";
    Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        cwd,
        move |key| {
            if key == "CENTRAL_ROOT" {
                Some(root_text.clone())
            } else if key == "AIKIT_GITNEXUS_BIN" {
                Some(no_gitnexus.to_string())
            } else {
                None
            }
        },
    )
    .expect("open production application service")
}

/// A two-Project world: cedar keeps no matrix of its own, larch carries one,
/// and the root composition carries a third. The needle text exists only in
/// the matrices, so every hit below comes from matrix compilation.
fn matrix_world() -> TempDir {
    let temp = TempDir::new().unwrap();
    let world = temp.path().join("world");
    fs::create_dir_all(world.join(".aikit")).unwrap();
    fs::create_dir_all(world.join("Control")).unwrap();
    fs::create_dir_all(world.join("Work")).unwrap();
    manifest(&world.join("Work/cedar"), "epilogos/cedar");
    manifest(&world.join("Work/larch"), "epilogos/larch");
    matrix(
        &world.join("Work/larch/ProjectCentral/user"),
        "matrix.larch",
        "cap.larch.unique",
        "larchNeedleMatrixScope",
    );
    matrix(
        &world.join("ProjectCentral/user"),
        "matrix.rootworld",
        "cap.rootworld.unique",
        "rootNeedleMatrixScope",
    );
    // Cedar's own NOW material: the positive check that its own scope still
    // discloses its own Project.
    write(
        &world
            .join("Work/cedar/ProjectCentral/now/returns")
            .join("own.md"),
        "cedarMatrixScopeNeedle from cedar NOW\n",
    );
    isolate_home(&temp);
    temp
}

fn leak_scan(hits: &[KnowledgeSearchHit], scope: &str) -> Vec<String> {
    hits.iter()
        .filter_map(|hit| {
            let resource = hit.resource.as_str();
            let leaks_larch = resource.contains("larch") || resource.contains("Larch");
            let raw_path = resource.starts_with('/');
            (leaks_larch || raw_path)
                .then(|| format!("{scope} hit names sibling material or a raw path: {resource}"))
        })
        .collect()
}

/// The isolation relation, end to end over one service per scope. One test
/// function because the native-owner stub and the home isolation are
/// process-environment state that must not race between tests.
#[test]
fn projectworld_matrix_isolation_over_real_scopes() {
    // The cedar-scope positive control answers through the real NOW-field
    // provider, which searches with real ripgrep. Like `now_field_real`, the
    // test skips honestly when ripgrep is absent and hardens when the
    // environment declares real-ripgrep acceptance (`AIKIT_REQUIRE_RIPGREP_REAL`).
    if !aikit_adapters::ripgrep::available() {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_RIPGREP_REAL").is_none(),
            "real ProjectWorld matrix-isolation conformance requires ripgrep"
        );
        eprintln!(
            "ripgrep is not installed; the ProjectWorld matrix isolation acceptance test skipped"
        );
        return;
    }
    let temp = matrix_world();
    let world = temp.path().join("world");
    let stub = stub_ctrl(&world);
    std::env::set_var("CENTRAL_CTRL_BIN", &stub);
    let cedar = open_service(&temp, &world.join("Work/cedar"), &world);

    // Every matrix-reaching needle from cedar's scope: the sibling matrix
    // identity, the sibling capability, the relation needle, and the generic
    // carrier name the installed binary once leaked raw sibling paths
    // through.
    for needle in [
        "matrix.larch",
        "cap.larch.unique",
        "larchNeedleMatrixScope",
        "matrix.rootworld",
        "capability-matrix",
    ] {
        let result = cedar.knowledge_search(needle, 256).unwrap();
        let leaks = leak_scan(&result.hits, "cedar");
        assert!(
            leaks.is_empty(),
            "needle {needle:?} leaked into the cedar scope:\n{}",
            leaks.join("\n")
        );
        for absence in &result.absences {
            assert!(
                !absence.contains("larch") && !absence.contains("Larch"),
                "cedar scope absence names the sibling Project: {absence}"
            );
        }
    }

    // The root composition passes untouched: the root lineage is a distinct,
    // legitimately broader aperture, and the attribution must not over-drop
    // it from a Project's reply.
    let root_composition = cedar.knowledge_search("matrix.rootworld", 256).unwrap();
    assert!(
        root_composition
            .hits
            .iter()
            .any(|hit| hit.resource.as_str().contains("matrix.rootworld")),
        "cedar scope lost the root composition matrix: {root_composition:#?}"
    );

    // Cedar's own Project material is still discoverable: the sibling fix
    // must not be a global narrowing. When the NOW-field provider is
    // unavailable in this environment, the reply must disclose that
    // unavailability as an absence — an honest horizon, not a silently
    // emptied one — and the positive control holds only where the provider
    // actually answered.
    let own = cedar
        .knowledge_search("cedarMatrixScopeNeedle", 256)
        .unwrap();
    let own_hit = own.hits.iter().any(|hit| hit
        .resource
        .as_str()
        .ends_with("Work/cedar/ProjectCentral/now/returns/own.md"));
    let now_field_unavailable = own
        .absences
        .iter()
        .any(|absence| absence.contains("provider/source-pool/now-field"));
    assert!(
        own_hit || now_field_unavailable,
        "cedar scope lost its own NOW material: {own:#?}"
    );

    // Larch's own scope keeps its own matrix: the fix is attribution, not
    // global suppression.
    let larch = open_service(&temp, &world.join("Work/larch"), &world);
    let own_matrix = larch.knowledge_search("matrix.larch", 256).unwrap();
    assert!(
        own_matrix
            .hits
            .iter()
            .any(|hit| hit.resource.as_str().contains("matrix.larch")),
        "larch scope lost its own matrix: {own_matrix:#?}"
    );

    // The root scope keeps every Project's matrix: world-level Knowledge is
    // legitimately broader and must stay so.
    let root = open_service(&temp, &world, &world);
    let world_view = root.knowledge_search("matrix.larch", 256).unwrap();
    assert!(
        world_view
            .hits
            .iter()
            .any(|hit| hit.resource.as_str().contains("matrix.larch")),
        "root scope lost a Project's matrix: {world_view:#?}"
    );

    // The graph obeys the same scope: an empty-query graph at cedar's scope
    // keeps the sibling's compiled matrix objects and wiki space out of its
    // reply. Regression for the landing replay of the ProjectWorld isolation
    // packet (2026-09-25): the graph's source-citation nodes served
    // clearing files — including sibling-Project copies — and a sibling
    // wiki-space node at Project scope, a channel the search repair's law
    // had not reached.
    let graph = cedar
        .knowledge_graph("", 2_000, 4_000)
        .expect("cedar graph reply");
    let text = graph.to_string();
    for marker in ["larch", "Larch", "central:wiki:project:larch"] {
        assert!(
            !text.contains(marker),
            "cedar graph reply names sibling material ({marker})"
        );
    }
    // The root composition still shows in the graph: scope narrows siblings,
    // it does not blind the Project to root-lineage material.
    assert!(
        text.contains("cap.rootworld.unique"),
        "cedar graph lost the root composition matrix"
    );

    std::env::remove_var("CENTRAL_CTRL_BIN");
}
