//! Real GitNexus Project-scope acceptance.
//! These are committed repositories and the installed GitNexus CLI, not a
//! stand-in provider. The test process owns an isolated HOME and AIKit home.

use std::fs;
use std::path::Path;
use std::process::Command;

use aikit_cli::app::Service;
use aikit_core::resource::{parse_or_search_expression, ResolveExpression};
use aikit_core::KnowledgeAddress;
use aikit_store::AikitHome;
use tempfile::TempDir;

const NEEDLE: &str = "larchUniqueLocator";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git command is available");
    assert!(
        output.status.success(),
        "git {:?} failed with status {:?}",
        args,
        output.status.code()
    );
}

fn project(world: &Path, name: &str, source: &str, committed_repo: bool) {
    let root = world.join("Work").join(name);
    write(
        &root.join("ProjectCentral/project.json"),
        &format!(
            r#"{{"schema":"central.project/v1","project_id":"{name}","human_source":"ProjectCentral/user","wiki":{{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json","adopted_sources":[]}}}}"#
        ),
    );
    write(
        &root.join("ProjectCentral/agents/wiki/wiki.json"),
        r#"{"profile":"okf-wiki/v1","objects":[]}"#,
    );
    write(
        &root.join("package.json"),
        &format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
    );
    write(&root.join("src/owner.ts"), source);
    if !committed_repo {
        return;
    }
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "user.name=AIKit proof",
            "-c",
            "user.email=proof@example.invalid",
            "commit",
            "-qm",
            "source",
        ],
    );
}

fn has_larch_code(result: &aikit_core::KnowledgeSearchResult) -> bool {
    result.hits.iter().any(|hit| {
        matches!(&hit.address, KnowledgeAddress::Code(reference)
            if reference.source.as_str() == "source:project-code:larch")
    })
}

fn has_larch_project_source(result: &aikit_core::KnowledgeSearchResult) -> bool {
    result
        .hits
        .iter()
        .any(|hit| hit.resource.as_str().starts_with("source:project:larch:"))
}

#[test]
fn real_gitnexus_code_and_project_map_hits_obey_current_and_explicit_scope() {
    let binary = std::env::var("AIKIT_GITNEXUS_BIN").unwrap_or_else(|_| "gitnexus".into());
    let available = Command::new(&binary).arg("--version").output();
    if !available
        .as_ref()
        .is_ok_and(|output| output.status.success())
    {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_GITNEXUS_REAL").is_none(),
            "the real GitNexus conformance job requires an installed provider"
        );
        return;
    }

    let temp = TempDir::new().unwrap();
    let world = temp.path().join("Central");
    fs::create_dir_all(world.join("Control")).unwrap();
    project(
        &world,
        "cedar",
        "export function cedarOwnedLocator(): string {\n  return 'cedar';\n}\n",
        true,
    );
    project(
        &world,
        "larch",
        "export function larchUniqueLocator(): string {\n  return 'larch';\n}\n",
        true,
    );
    // A declared Project without a Git repository makes the real GitNexus
    // indexing command fail; its degradation must remain in that Project.
    project(&world, "broken", "export const broken = true;\n", false);
    let isolated_home = temp.path().join("home");
    fs::create_dir_all(&isolated_home).unwrap();
    std::env::set_var("HOME", &isolated_home);
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join("xdg-config"));
    std::env::set_var("XDG_CACHE_HOME", temp.path().join("xdg-cache"));
    std::env::set_var("XDG_DATA_HOME", temp.path().join("xdg-data"));
    std::env::set_var("AIKIT_BKMR_CONFIG_DIR", temp.path().join("bkmr-config"));
    std::env::set_var("GITNEXUS_WORKER_POOL_SIZE", "1");

    let cedar = world.join("Work/cedar");
    let root_text = world.display().to_string();
    let selected_binary = binary.clone();
    let service = Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        &cedar,
        move |key| match key {
            "CENTRAL_ROOT" => Some(root_text.clone()),
            "AIKIT_GITNEXUS_BIN" => Some(selected_binary.clone()),
            _ => None,
        },
    )
    .unwrap();

    // An explicit cross-Project query is allowed and must prove that the
    // provider actually indexed larch. Merely getting an empty result from a
    // broken provider is not a scoping proof.
    let cross = service
        .knowledge_search(&format!(": larch {NEEDLE}"), 256)
        .unwrap();
    assert!(
        has_larch_code(&cross),
        "real GitNexus did not surface larch Code; absences: {:?}",
        cross.absences
    );
    assert!(
        has_larch_project_source(&cross),
        "real WorkRepos provider did not surface larch source; absences: {:?}",
        cross.absences
    );
    let own_positive = service.knowledge_search("cedarOwnedLocator", 256).unwrap();
    assert!(
        own_positive.hits.iter().any(|hit| {
            matches!(&hit.address, KnowledgeAddress::Code(reference)
                if reference.source.as_str() == "source:project-code:cedar")
        }),
        "cedar scope did not retain its own indexed Code"
    );
    assert!(
        own_positive
            .hits
            .iter()
            .any(|hit| hit.resource.as_str().starts_with("source:project:cedar:")),
        "cedar scope did not retain its own source"
    );

    let own = service.knowledge_search(NEEDLE, 256).unwrap();
    assert!(!has_larch_code(&own), "cedar search leaked larch Code");
    assert!(
        !has_larch_project_source(&own),
        "cedar search leaked larch Source/ProjectMap material"
    );
    assert!(
        !own.absences
            .iter()
            .any(|absence| absence.contains("GitNexus CodeIndex degraded for Work/broken")),
        "cedar search leaked the other Project's real GitNexus failure"
    );
    let broken = service.knowledge_search(": broken broken", 256).unwrap();
    assert!(
        broken
            .absences
            .iter()
            .any(|absence| absence.contains("GitNexus CodeIndex degraded for Work/broken")),
        "real non-Git Project did not report its own GitNexus failure: {:?}",
        broken.absences
    );
    // The public `knowledge resolve` CLI uses an unscoped expression and
    // reaches this service method directly. It must inherit cedar's scope.
    let cli_expression = parse_or_search_expression(NEEDLE).unwrap();
    let direct = service.knowledge_resolve(&cli_expression, 256).unwrap();
    assert!(!has_larch_code(&direct), "cedar resolve leaked larch Code");
    assert!(
        !has_larch_project_source(&direct),
        "cedar resolve leaked larch Source/ProjectMap material"
    );

    // An unknown but syntactically valid Project names an empty Project
    // view. It cannot become a broad all-Projects query.
    let unknown = service
        .knowledge_search(&format!(": unknown-project {NEEDLE}"), 256)
        .unwrap();
    assert!(
        unknown.hits.is_empty(),
        "unknown Project scope admitted unrelated indexed or source hits: {:?}",
        unknown.hits
    );

    let invalid = ResolveExpression::scope("../larch", ResolveExpression::subject(NEEDLE));
    let failure = service.knowledge_resolve(&invalid, 256).unwrap_err();
    assert_eq!(failure.code(), "knowledge.scope_invalid");

    // A root World query has no implicit Project scope. It may discover the
    // larch source, as the explicit root operator's broader view permits.
    let root_text = world.display().to_string();
    let selected_binary = binary;
    let root_service = Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        &world,
        move |key| match key {
            "CENTRAL_ROOT" => Some(root_text.clone()),
            "AIKIT_GITNEXUS_BIN" => Some(selected_binary.clone()),
            _ => None,
        },
    )
    .unwrap();
    let global = root_service.knowledge_search(NEEDLE, 256).unwrap();
    assert!(has_larch_code(&global));
    assert!(has_larch_project_source(&global));
    let global_direct = root_service
        .knowledge_resolve(&cli_expression, 256)
        .unwrap();
    assert!(has_larch_code(&global_direct));
    assert!(has_larch_project_source(&global_direct));
}
