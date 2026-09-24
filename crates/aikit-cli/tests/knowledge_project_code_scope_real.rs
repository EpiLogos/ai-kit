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

fn has_now_source(result: &aikit_core::KnowledgeSearchResult, relative: &str) -> bool {
    let expected = format!("central:source:control:root:{relative}");
    result
        .hits
        .iter()
        .any(|hit| hit.resource.as_str() == expected)
}

fn code_query_failed_for(result: &aikit_core::KnowledgeSearchResult, project: &str) -> bool {
    result.absences.iter().any(|absence| {
        absence.starts_with("ProjectMap code search degraded:")
            && absence.contains(&format!("--repo {project}"))
    })
}

fn work_repos_search_failed(result: &aikit_core::KnowledgeSearchResult) -> bool {
    result.absences.iter().any(|absence| {
        absence.starts_with("SourcePool search degraded for provider/source-pool/work-repos:")
    })
}

fn now_field_search_failed(result: &aikit_core::KnowledgeSearchResult) -> bool {
    result.absences.iter().any(|absence| {
        absence.starts_with("SourcePool search degraded for provider/source-pool/now-field:")
    })
}

#[cfg(unix)]
struct RestorePermissions {
    path: std::path::PathBuf,
    original: fs::Permissions,
}

#[cfg(unix)]
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
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
    // The real Central root also carries an AIKit marker. Its topmost profile
    // must not make a nested Git worktree outside Work/ a root-wide query.
    fs::create_dir_all(world.join(".aikit")).unwrap();
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
    // The real authored-wiki compiler rejects this bounded source while
    // retaining the rest of the World. Its diagnostic belongs to larch.
    write(
        &world.join("Work/larch/ProjectCentral/user/oversized.md"),
        &"x".repeat((4 * 1024 * 1024) + 1),
    );
    write(
        &world.join("Work/larch/ProjectCentral/user/capability-matrix.json"),
        r#"{"protocol":"larch-invalid","matrix_id":"larch-matrix"}"#,
    );
    write(
        &world.join("ProjectCentral/user/capability-matrix.json"),
        r#"{"protocol":"root-invalid","matrix_id":"root-matrix"}"#,
    );
    fs::create_dir_all(world.join("Work/unbound")).unwrap();
    write(
        &world.join("Work/cedar/ProjectCentral/now/returns/own.md"),
        "cedarOwnedLocator from cedar NOW\n",
    );
    write(
        &world.join("Work/larch/ProjectCentral/now/returns/sibling.md"),
        "larchUniqueLocator from larch NOW\n",
    );
    write(
        &world.join("Control/agents/now/flows/common.md"),
        "cedarOwnedLocator from common Control NOW\n",
    );
    let cedar_worktree = temp.path().join("external-cedar-scope");
    git(
        &world.join("Work/cedar"),
        &[
            "worktree",
            "add",
            "--detach",
            cedar_worktree.to_str().unwrap(),
            "HEAD",
        ],
    );
    assert!(
        cedar_worktree.join(".git").is_file(),
        "fixture is an actual Git worktree"
    );
    fs::create_dir_all(cedar_worktree.join(".aikit")).unwrap();
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
    assert!(has_now_source(
        &cross,
        "Work/larch/ProjectCentral/now/returns/sibling.md"
    ));
    assert!(cross
        .absences
        .iter()
        .any(|absence| { absence.contains("Work/larch") && absence.contains("oversized.md") }));
    assert!(cross
        .absences
        .iter()
        .any(|absence| absence.contains("larch-invalid")));
    assert!(cross
        .absences
        .iter()
        .any(|absence| absence.contains("root-invalid")));
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
    assert!(has_now_source(
        &own_positive,
        "Work/cedar/ProjectCentral/now/returns/own.md"
    ));
    assert!(has_now_source(
        &own_positive,
        "Control/agents/now/flows/common.md"
    ));
    let own_graph = service.knowledge_graph("", 4096, 16384).unwrap();
    let graph_resources = |graph: &serde_json::Value| -> Vec<String> {
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| node["resource"].as_str().map(str::to_owned))
            .collect()
    };
    let own_graph_resources = graph_resources(&own_graph);
    assert!(own_graph_resources.iter().any(|resource| {
        resource == "central:source:control:root:Work/cedar/ProjectCentral/now/returns/own.md"
    }));
    assert!(!own_graph_resources.iter().any(|resource| {
        resource == "central:source:control:root:Work/larch/ProjectCentral/now/returns/sibling.md"
    }), "empty cedar graph disclosed sibling NOW roster");

    let own = service.knowledge_search(NEEDLE, 256).unwrap();
    assert!(!has_larch_code(&own), "cedar search leaked larch Code");
    assert!(
        !has_larch_project_source(&own),
        "cedar search leaked larch Source/ProjectMap material"
    );
    assert!(!has_now_source(
        &own,
        "Work/larch/ProjectCentral/now/returns/sibling.md"
    ));
    assert!(
        !own.absences
            .iter()
            .any(|absence| absence.contains("GitNexus CodeIndex degraded for Work/broken")),
        "cedar search leaked the other Project's real GitNexus failure"
    );
    assert!(
        !own.absences
            .iter()
            .any(|absence| { absence.contains("Work/larch") || absence.contains("Work/unbound") }),
        "cedar search disclosed sibling authored-wiki or discovery diagnostics: {:?}",
        own.absences
    );
    assert!(!own
        .absences
        .iter()
        .any(|absence| absence.contains("larch-invalid")));
    assert!(own
        .absences
        .iter()
        .any(|absence| absence.contains("root-invalid")));
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
    assert!(!has_now_source(
        &direct,
        "Work/larch/ProjectCentral/now/returns/sibling.md"
    ));

    // An empty subject reaches the real GitNexus query command after both
    // repositories indexed, and GitNexus refuses it. The failure itself must
    // stay scoped: silently dropping cedar's own error would be false health.
    let own_failure = service.knowledge_search("", 256).unwrap();
    assert!(
        code_query_failed_for(&own_failure, "cedar"),
        "cedar's real GitNexus query failure was lost: {:?}",
        own_failure.absences
    );
    assert!(
        !code_query_failed_for(&own_failure, "larch"),
        "cedar search disclosed larch's real GitNexus query failure"
    );
    let direct_failure = service
        .knowledge_resolve(&parse_or_search_expression("").unwrap(), 256)
        .unwrap();
    assert!(code_query_failed_for(&direct_failure, "cedar"));
    assert!(!code_query_failed_for(&direct_failure, "larch"));
    let cross_failure = service.knowledge_search(": larch", 256).unwrap();
    assert!(code_query_failed_for(&cross_failure, "larch"));
    assert!(!code_query_failed_for(&cross_failure, "cedar"));

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
    let wildcard_scope = ResolveExpression::scope("*", ResolveExpression::subject(NEEDLE));
    let wildcard = service.knowledge_resolve(&wildcard_scope, 256).unwrap();
    assert!(
        !has_larch_code(&wildcard)
            && !has_larch_project_source(&wildcard)
            && !has_now_source(
                &wildcard,
                "Work/larch/ProjectCentral/now/returns/sibling.md"
            ),
        "an unknown wildcard scope searched discovered Work projects"
    );

    let invalid = ResolveExpression::scope("../larch", ResolveExpression::subject(NEEDLE));
    let failure = service.knowledge_resolve(&invalid, 256).unwrap_err();
    assert_eq!(failure.code(), "knowledge.scope_invalid");

    // A root World query has no implicit Project scope. It may discover the
    // larch source, as the explicit root operator's broader view permits.
    let root_text = world.display().to_string();
    let selected_binary = binary.clone();
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
    assert!(has_now_source(
        &global,
        "Work/larch/ProjectCentral/now/returns/sibling.md"
    ));
    let global_graph = root_service.knowledge_graph("", 4096, 16384).unwrap();
    assert!(graph_resources(&global_graph).iter().any(|resource| {
        resource == "central:source:control:root:Work/larch/ProjectCentral/now/returns/sibling.md"
    }));
    assert!(global
        .absences
        .iter()
        .any(|absence| absence.contains("Work/larch") && absence.contains("oversized.md")));
    assert!(global
        .absences
        .iter()
        .any(|absence| absence.contains("Work/unbound")));
    assert!(global
        .absences
        .iter()
        .any(|absence| absence.contains("larch-invalid")));
    assert!(global
        .absences
        .iter()
        .any(|absence| absence.contains("root-invalid")));
    let global_direct = root_service
        .knowledge_resolve(&cli_expression, 256)
        .unwrap();
    assert!(has_larch_code(&global_direct));
    assert!(has_larch_project_source(&global_direct));
    assert!(has_now_source(
        &global_direct,
        "Work/larch/ProjectCentral/now/returns/sibling.md"
    ));
    let root_failure = root_service.knowledge_search("", 256).unwrap();
    assert!(code_query_failed_for(&root_failure, "cedar"));
    assert!(code_query_failed_for(&root_failure, "larch"));

    let worktree_world = world.display().to_string();
    let worktree_binary = binary.clone();
    let worktree_service = Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        &cedar_worktree,
        move |key| match key {
            "CENTRAL_ROOT" => Some(worktree_world.clone()),
            "AIKIT_GITNEXUS_BIN" => Some(worktree_binary.clone()),
            _ => None,
        },
    )
    .unwrap();
    let worktree_cross = worktree_service.knowledge_search(NEEDLE, 256).unwrap();
    assert!(
        !has_larch_code(&worktree_cross)
            && !has_larch_project_source(&worktree_cross)
            && !has_now_source(
                &worktree_cross,
                "Work/larch/ProjectCentral/now/returns/sibling.md"
            ),
        "real cedar Git worktree search was treated as root-wide"
    );
    let worktree_own = worktree_service
        .knowledge_search("cedarOwnedLocator", 256)
        .unwrap();
    assert!(worktree_own
        .hits
        .iter()
        .any(|hit| { hit.resource.as_str().starts_with("source:project:cedar:") }));
    assert!(has_now_source(
        &worktree_own,
        "Work/cedar/ProjectCentral/now/returns/own.md"
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // A real unreadable file makes the installed ripgrep return status 2.
        // It lives only in larch; cedar must not see that sibling failure,
        // while an explicit larch or root query must still report it.
        let larch_source = world.join("Work/larch/src/owner.ts");
        let original_permissions = fs::metadata(&larch_source).unwrap().permissions();
        let original_mode = original_permissions.mode();
        let restore = RestorePermissions {
            path: larch_source.clone(),
            original: original_permissions,
        };
        fs::set_permissions(&larch_source, fs::Permissions::from_mode(0o000)).unwrap();
        let ripgrep = Command::new(aikit_adapters::ripgrep::executable())
            .args(["--json", "cedarOwnedLocator"])
            .arg(&larch_source)
            .output()
            .unwrap();
        assert_eq!(
            ripgrep.status.code(),
            Some(2),
            "real unreadable sibling file must make ripgrep fail"
        );
        let own_with_sibling_error = service.knowledge_search("cedarOwnedLocator", 256).unwrap();
        assert!(
            !work_repos_search_failed(&own_with_sibling_error),
            "cedar search disclosed larch's real ripgrep failure: {:?}",
            own_with_sibling_error.absences
        );
        assert!(
            own_with_sibling_error
                .hits
                .iter()
                .any(|hit| hit.resource.as_str().starts_with("source:project:cedar:")),
            "cedar's healthy source should remain searchable"
        );
        let explicit_larch_error = service
            .knowledge_search(": larch cedarOwnedLocator", 256)
            .unwrap();
        assert!(
            work_repos_search_failed(&explicit_larch_error),
            "larch's own ripgrep failure was hidden"
        );
        let root_with_sibling_error = root_service
            .knowledge_search("cedarOwnedLocator", 256)
            .unwrap();
        assert!(
            work_repos_search_failed(&root_with_sibling_error),
            "the explicitly broad root query hid larch's real failure"
        );
        drop(restore);
        assert_eq!(
            fs::metadata(&larch_source).unwrap().permissions().mode(),
            original_mode
        );

        // The separate live NOW provider has the same failure boundary. Its
        // root-style refs do not make a sibling Project's file common ground.
        let larch_now = world.join("Work/larch/ProjectCentral/now/returns/sibling.md");
        let now_original_permissions = fs::metadata(&larch_now).unwrap().permissions();
        let now_original_mode = now_original_permissions.mode();
        let restore_now = RestorePermissions {
            path: larch_now.clone(),
            original: now_original_permissions,
        };
        fs::set_permissions(&larch_now, fs::Permissions::from_mode(0o000)).unwrap();
        let ripgrep_now = Command::new(aikit_adapters::ripgrep::executable())
            .args(["--json", "cedarOwnedLocator"])
            .arg(&larch_now)
            .output()
            .unwrap();
        assert_eq!(ripgrep_now.status.code(), Some(2));
        let own_with_now_error = service.knowledge_search("cedarOwnedLocator", 256).unwrap();
        assert!(!now_field_search_failed(&own_with_now_error));
        assert!(has_now_source(
            &own_with_now_error,
            "Control/agents/now/flows/common.md"
        ));
        let explicit_larch_now_error = service
            .knowledge_search(": larch cedarOwnedLocator", 256)
            .unwrap();
        assert!(now_field_search_failed(&explicit_larch_now_error));
        let root_with_now_error = root_service
            .knowledge_search("cedarOwnedLocator", 256)
            .unwrap();
        assert!(now_field_search_failed(&root_with_now_error));
        drop(restore_now);
        assert_eq!(
            fs::metadata(&larch_now).unwrap().permissions().mode(),
            now_original_mode
        );
    }

    let worktree_manifest = cedar_worktree.join("ProjectCentral/project.json");
    let original_manifest = fs::read(&worktree_manifest).unwrap();
    let mut unmatched: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
    unmatched["project_id"] = serde_json::json!("unclaimed");
    fs::write(&worktree_manifest, serde_json::to_vec(&unmatched).unwrap()).unwrap();
    assert_eq!(
        worktree_service
            .knowledge_search("cedarOwnedLocator", 256)
            .unwrap_err()
            .code(),
        "knowledge.project_scope_unresolved",
        "an unmatched native Project identity must refuse a root-wide search"
    );
    fs::remove_file(&worktree_manifest).unwrap();
    assert_eq!(
        worktree_service
            .knowledge_search("cedarOwnedLocator", 256)
            .unwrap_err()
            .code(),
        "knowledge.project_scope_unresolved",
        "a Git worktree without ProjectCentral identity must refuse a root-wide search"
    );
    fs::write(&worktree_manifest, original_manifest).unwrap();
    let duplicate = world.join("Work/cedar-duplicate");
    project(
        &world,
        "cedar-duplicate",
        "export const duplicate = true;\n",
        true,
    );
    let duplicate_manifest = duplicate.join("ProjectCentral/project.json");
    let mut duplicate_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&duplicate_manifest).unwrap()).unwrap();
    duplicate_json["project_id"] = serde_json::json!("cedar");
    fs::write(
        &duplicate_manifest,
        serde_json::to_vec(&duplicate_json).unwrap(),
    )
    .unwrap();
    assert_eq!(
        worktree_service
            .knowledge_search("cedarOwnedLocator", 256)
            .unwrap_err()
            .code(),
        "knowledge.project_scope_unresolved",
        "ambiguous native Project identity must refuse a root-wide search"
    );
}
