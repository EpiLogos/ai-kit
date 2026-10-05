//! Real GitNexus Project-scope acceptance.
//! These are committed repositories and the installed GitNexus CLI, not a
//! stand-in provider. The test process owns an isolated HOME and AIKit home.

use std::fs;
use std::path::Path;
use std::process::Command;

use aikit_cli::app::Service;
use aikit_core::resource::{parse_or_search_expression, ResolveExpression};
use aikit_core::{KnowledgeAddress, SourceRef};
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

fn native_action(world: &Path, name: &str, input: serde_json::Value) -> serde_json::Value {
    let mut command = Command::new(aikit_adapters::central_file_map::executable());
    command
        .args(["--json", "--root"])
        .arg(world)
        .args(["action", "run", name])
        .arg(input.to_string());
    let output = aikit_adapters::runner::SystemRunner::new()
        .with_timeout(std::time::Duration::from_secs(30))
        .with_output_limit_bytes(8 * 1024 * 1024)
        .with_strict_utf8()
        .capture_command(&mut command)
        .unwrap();
    let envelope: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert!(
        output.ok() && envelope["ok"] == true,
        "actual {name} refused: {envelope}; stderr={}",
        output.stderr
    );
    if let Some(operation) = name.strip_prefix("central.file-map.") {
        assert_eq!(envelope["data"]["schema"], "central.file-map/v1");
        assert_eq!(envelope["data"]["operation"], operation);
        assert!(envelope["data"]["result"].is_object());
        envelope["data"]["result"].clone()
    } else {
        assert!(envelope["data"].is_object());
        envelope["data"].clone()
    }
}

fn register_now_source(world: &Path, project: Option<&str>, member: &str) -> SourceRef {
    let inspected = native_action(
        world,
        "central.file-map.inspect",
        serde_json::json!({"project":project,"resources":false}),
    );
    assert_eq!(inspected["provider"]["available"], false);
    assert!(
        !Path::new(inspected["database"].as_str().unwrap()).exists(),
        "metadata qualification must not depend on an initialized BKMR index"
    );
    let registered = native_action(
        world,
        "central.file-map.register",
        serde_json::json!({"project":project,"path":member,
            "expected_revision":inspected["revision"].as_str().unwrap()}),
    );
    let source = SourceRef::parse(registered["source_ref"].as_str().unwrap()).unwrap();
    let reading = native_action(
        world,
        "central.file-map.resolve",
        serde_json::json!({"project":project,"federated":false,
            "source_ref":source,"binding_only":true}),
    );
    assert_eq!(reading["ownership"], "owned");
    assert_eq!(reading["binding_only"], true);
    assert_eq!(reading["kind"], "file");
    assert_eq!(reading["source"]["ref"], source.as_str());
    assert_eq!(reading["source"]["path"], member);
    assert_eq!(reading["project"], serde_json::json!(project));
    assert_eq!(
        reading["world_ref"],
        project.map_or_else(
            || "control:root".to_owned(),
            |project| format!("project:{project}")
        )
    );
    let owner_root = project.map_or_else(
        || world.to_path_buf(),
        |project| world.join("Work").join(project),
    );
    assert_eq!(reading["path"], serde_json::json!(owner_root.join(member)));
    assert!(reading["content"].is_null());
    assert!(reading["revision"].is_null());
    assert!(reading["relation_revision"].is_string());
    source
}

fn project(world: &Path, name: &str, source: &str, committed_repo: bool) {
    let root = world.join("Work").join(name);
    fs::create_dir_all(&root).unwrap();
    native_action(
        world,
        "projectcentral.init",
        serde_json::json!({"project":name,"project_id":name}),
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

fn has_project_source(result: &aikit_core::KnowledgeSearchResult, project: &str) -> bool {
    let expected =
        aikit_adapters::work_file_source_ref(project, Path::new("src/owner.ts")).unwrap();
    result.hits.iter().any(|hit| {
        matches!(&hit.address, KnowledgeAddress::Source(source) if source == &expected)
            && hit.resource.as_str() == expected.as_str()
    })
}

fn has_larch_project_source(result: &aikit_core::KnowledgeSearchResult) -> bool {
    has_project_source(result, "larch")
}

fn has_now_source(result: &aikit_core::KnowledgeSearchResult, expected: &SourceRef) -> bool {
    result.hits.iter().any(|hit| {
        matches!(&hit.address, KnowledgeAddress::Source(source) if source == expected)
            && hit.resource.as_str() == expected.as_str()
    })
}

fn code_query_failed_for(result: &aikit_core::KnowledgeSearchResult, project: &Path) -> bool {
    result.absences.iter().any(|absence| {
        absence.starts_with("ProjectMap code search degraded:")
            && absence.contains(&format!("--repo {}", project.display()))
    })
}

fn work_repos_search_failed(result: &aikit_core::KnowledgeSearchResult) -> bool {
    result.absences.iter().any(|absence| {
        // The real denied file below identifies the producer. Its public
        // absence carries the native error code, not private paths/output.
        absence == "SourcePool search unavailable (search.ripgrep_failed)"
    })
}

fn now_field_search_failed(result: &aikit_core::KnowledgeSearchResult) -> bool {
    result
        .absences
        .iter()
        .any(|absence| absence == "SourcePool search unavailable (search.ripgrep_failed)")
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

/// Copy an installed tool cache (not index state) into the isolated HOME.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Snapshot only the real owner indexes built above. Reads run against private
/// query copies, so even metadata rewrites on the originals are a regression.
type OwnerIndexSnapshot =
    std::collections::BTreeMap<std::path::PathBuf, (u64, std::time::SystemTime, blake3::Hash)>;
fn owner_indexes(world: &Path) -> OwnerIndexSnapshot {
    fn visit(path: &Path, rows: &mut OwnerIndexSnapshot) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, rows);
            } else {
                let metadata = fs::metadata(&path).unwrap();
                rows.insert(
                    path.clone(),
                    (
                        metadata.len(),
                        metadata.modified().unwrap(),
                        blake3::hash(&fs::read(path).unwrap()),
                    ),
                );
            }
        }
    }
    let mut rows = OwnerIndexSnapshot::new();
    for name in ["cedar", "larch"] {
        visit(&world.join("Work").join(name).join(".gitnexus"), &mut rows);
    }
    rows
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
    let world = world.canonicalize().unwrap();
    // The real Central root also carries an AIKit marker. Its topmost profile
    // must not make a nested Git worktree outside Work/ a root-wide query.
    fs::create_dir_all(world.join(".aikit")).unwrap();
    native_action(&world, "central.init", serde_json::json!({}));
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
    // A declared Project without an owner index must disclose its actual
    // admission failure only in that Project; reads must not create one.
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
    write(
        &world.join("Control/agents/now/clearings/public-proof/audience.md"),
        "public fixture clearing is not a common Project temporal record\n",
    );
    // Real registration supplies the current literal temporal identities.
    // The fixtures are authored records, not fabricated native NOW receipts.
    let cedar_now_source =
        register_now_source(&world, Some("cedar"), "ProjectCentral/now/returns/own.md");
    let larch_now_source = register_now_source(
        &world,
        Some("larch"),
        "ProjectCentral/now/returns/sibling.md",
    );
    let common_now_source = register_now_source(&world, None, "Control/agents/now/flows/common.md");
    let clearing_source = register_now_source(
        &world,
        None,
        "Control/agents/now/clearings/public-proof/audience.md",
    );
    assert_ne!(cedar_now_source, larch_now_source);
    assert_ne!(cedar_now_source, common_now_source);
    assert_ne!(larch_now_source, common_now_source);
    // Common Control is explicitly linked by its owner into cedar; a
    // participating Root is not by itself a cross-Project read grant.
    let inspected = native_action(
        &world,
        "central.file-map.inspect",
        serde_json::json!({"project":"cedar","resources":false}),
    );
    let linked = native_action(
        &world,
        "central.file-map.link",
        serde_json::json!({"project":"cedar","path":"common/now.md",
            "source_ref":common_now_source,"owner":"test:gitnexus-scope",
            "expected_revision":inspected["revision"].as_str().unwrap()}),
    );
    assert_eq!(linked["source_ref"], common_now_source.as_str());
    assert!(fs::symlink_metadata(world.join("Work/cedar/common/now.md"))
        .unwrap()
        .file_type()
        .is_symlink());
    let inspected = native_action(
        &world,
        "central.file-map.inspect",
        serde_json::json!({"project":"cedar","resources":false}),
    );
    let linked_clearing = native_action(
        &world,
        "central.file-map.link",
        serde_json::json!({"project":"cedar","path":"common/clearing.md",
            "source_ref":clearing_source,"owner":"test:gitnexus-scope",
            "expected_revision":inspected["revision"].as_str().unwrap()}),
    );
    assert_eq!(linked_clearing["source_ref"], clearing_source.as_str());
    assert!(
        fs::symlink_metadata(world.join("Work/cedar/common/clearing.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // The same native attachment lists admitted metadata without a BKMR
    // map or body copies, refuses external disclosure, and charges the real
    // retained roster before crossing its explicit caller allowance.
    let metadata_owner = std::sync::Arc::new(
        aikit_adapters::central_file_map::CentralFileMapProvider::connect(
            aikit_adapters::runner::SystemRunner::new()
                .with_timeout(std::time::Duration::from_secs(30))
                .with_strict_utf8(),
            aikit_adapters::central_file_map::executable(),
            &world,
            None,
        )
        .unwrap(),
    );
    let metadata_now = aikit_adapters::now_field::NowFieldSourcePoolProvider::connect(
        aikit_adapters::now_field::default_runner(&world),
        aikit_adapters::ripgrep::executable(),
        aikit_adapters::now_field::NowFieldScope::standard(&world),
    )
    .unwrap()
    .with_native_owner(metadata_owner);
    assert!(metadata_now.descriptors().is_empty());
    let mut current_refs = std::collections::BTreeSet::new();
    metadata_now
        .visit_current_native_sources_for(
            aikit_core::context_source::RetrievalTarget::LocalAgent,
            None,
            16,
            |source, _| {
                current_refs.insert(source.clone());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        current_refs,
        [
            cedar_now_source.clone(),
            larch_now_source.clone(),
            common_now_source.clone(),
            clearing_source.clone()
        ]
        .into_iter()
        .collect()
    );
    let mut cedar_refs = std::collections::BTreeSet::new();
    metadata_now
        .visit_current_native_sources_for(
            aikit_core::context_source::RetrievalTarget::LocalAgent,
            Some("cedar"),
            16,
            |source, _| {
                cedar_refs.insert(source.clone());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        cedar_refs,
        [cedar_now_source.clone(), common_now_source.clone()]
            .into_iter()
            .collect()
    );
    assert!(!cedar_refs.contains(&clearing_source));
    assert!(!cedar_refs.contains(&larch_now_source));
    let mut externally_disclosed = 0usize;
    let refused_target = metadata_now
        .visit_current_native_sources_for(
            aikit_core::context_source::RetrievalTarget::ExternalProvider,
            None,
            16,
            |_, _| {
                externally_disclosed += 1;
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(refused_target.code(), "knowledge.source_target_withheld");
    assert_eq!(externally_disclosed, 0);
    let mut admitted_before_capacity = 0usize;
    let refused_capacity = metadata_now
        .visit_current_native_sources_for(
            aikit_core::context_source::RetrievalTarget::LocalAgent,
            None,
            1,
            |_, _| {
                admitted_before_capacity += 1;
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(refused_capacity.code(), "now_field.source_roster_budget");
    assert_eq!(admitted_before_capacity, 1);
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
    // GitNexus's full-text index needs the LadybugDB FTS extension, which
    // LadybugDB resolves under `$HOME/.lbdb/extension`. In a fresh HOME every
    // `analyze` downloads it with a 15 s bound and, when that download fails
    // or times out, still exits 0 with no full-text index — so cedar (indexed
    // first) intermittently lost its own Code while larch kept its. The
    // extension is an installed tool, not index or registry state: carry the
    // real HOME's installation (CI pre-installs it) into the isolated HOME so
    // indexing never depends on the network.
    if let Some(extensions) = std::env::var_os("HOME")
        .map(|home| Path::new(&home).join(".lbdb/extension"))
        .filter(|path| path.is_dir())
    {
        copy_tree(&extensions, &isolated_home.join(".lbdb/extension"));
    }
    std::env::set_var("HOME", &isolated_home);
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join("xdg-config"));
    std::env::set_var("XDG_CACHE_HOME", temp.path().join("xdg-cache"));
    std::env::set_var("XDG_DATA_HOME", temp.path().join("xdg-data"));
    std::env::set_var("AIKIT_BKMR_CONFIG_DIR", temp.path().join("bkmr-config"));
    std::env::set_var("GITNEXUS_WORKER_POOL_SIZE", "1");

    let gitnexus_home = isolated_home.join(".gitnexus");
    std::env::set_var("GITNEXUS_HOME", &gitnexus_home);
    // Explicit test setup indexes real repositories with the actual owner CLI.
    // Service reads below may admit/query these indexes, never build them.
    for name in ["cedar", "larch"] {
        let repo = world.join("Work").join(name);
        let output = Command::new(&binary)
            .arg("analyze")
            .arg(&repo)
            .args(["--index-only", "--name", name])
            .current_dir(&repo)
            .output()
            .expect("actual GitNexus index command starts");
        assert!(
            output.status.success(),
            "real index setup for {name} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            repo.join(".gitnexus/lbug").is_file(),
            "native index missing for {name}"
        );
        let meta: serde_json::Value =
            serde_json::from_slice(&fs::read(repo.join(".gitnexus/gitnexus.json")).unwrap())
                .unwrap();
        assert_eq!(
            meta.pointer("/capabilities/fts/status")
                .and_then(serde_json::Value::as_str),
            Some("available"),
            "GitNexus built {name} without its full-text index (LadybugDB FTS extension \
             not installed under the test HOME); keyword search cannot prove scope:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let indexed_before = owner_indexes(&world);
    let registry_before = fs::read(gitnexus_home.join("registry.json")).unwrap();

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
    // provider actually reads larch's real index. An empty result from a
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
    assert!(has_now_source(&cross, &larch_now_source));
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
        has_project_source(&own_positive, "cedar"),
        "cedar scope did not retain its own source"
    );
    assert!(has_now_source(&own_positive, &cedar_now_source));
    assert!(has_now_source(&own_positive, &common_now_source));
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
    assert!(own_graph_resources
        .iter()
        .any(|resource| resource == cedar_now_source.as_str()));
    assert!(
        !own_graph_resources
            .iter()
            .any(|resource| resource == larch_now_source.as_str()),
        "empty cedar graph disclosed sibling NOW roster"
    );
    assert!(!own_graph_resources
        .iter()
        .any(|resource| resource == clearing_source.as_str()));

    let own = service.knowledge_search(NEEDLE, 256).unwrap();
    assert!(!has_larch_code(&own), "cedar search leaked larch Code");
    assert!(
        !has_larch_project_source(&own),
        "cedar search leaked larch Source/ProjectMap material"
    );
    assert!(!has_now_source(&own, &larch_now_source));
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
    assert!(!has_now_source(&direct, &larch_now_source));

    // An empty subject reaches the real GitNexus query command after both
    // repositories indexed, and GitNexus refuses it. The failure itself must
    // stay scoped: silently dropping cedar's own error would be false health.
    let own_failure = service.knowledge_search("", 256).unwrap();
    assert!(
        code_query_failed_for(&own_failure, &world.join("Work/cedar")),
        "cedar's real GitNexus query failure was lost: {:?}",
        own_failure.absences
    );
    assert!(
        !code_query_failed_for(&own_failure, &world.join("Work/larch")),
        "cedar search disclosed larch's real GitNexus query failure"
    );
    let direct_failure = service
        .knowledge_resolve(&parse_or_search_expression("").unwrap(), 256)
        .unwrap();
    assert!(code_query_failed_for(
        &direct_failure,
        &world.join("Work/cedar")
    ));
    assert!(!code_query_failed_for(
        &direct_failure,
        &world.join("Work/larch")
    ));
    let cross_failure = service.knowledge_search(": larch", 256).unwrap();
    assert!(code_query_failed_for(
        &cross_failure,
        &world.join("Work/larch")
    ));
    assert!(!code_query_failed_for(
        &cross_failure,
        &world.join("Work/cedar")
    ));

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
            && !has_now_source(&wildcard, &larch_now_source),
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
    assert!(has_now_source(&global, &larch_now_source));
    let global_graph = root_service.knowledge_graph("", 4096, 16384).unwrap();
    assert!(graph_resources(&global_graph)
        .iter()
        .any(|resource| resource == larch_now_source.as_str()));
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
    assert!(has_now_source(&global_direct, &larch_now_source));
    let root_failure = root_service.knowledge_search("", 256).unwrap();
    assert!(code_query_failed_for(
        &root_failure,
        &world.join("Work/cedar")
    ));
    assert!(code_query_failed_for(
        &root_failure,
        &world.join("Work/larch")
    ));

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
            && !has_now_source(&worktree_cross, &larch_now_source),
        "real cedar Git worktree search was treated as root-wide"
    );
    let worktree_own = worktree_service
        .knowledge_search("cedarOwnedLocator", 256)
        .unwrap();
    assert!(has_project_source(&worktree_own, "cedar"));
    assert!(has_now_source(&worktree_own, &cedar_now_source));

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
            has_project_source(&own_with_sibling_error, "cedar"),
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
        assert_eq!(
            fs::File::open(&larch_now).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let metadata_graph = root_service.knowledge_graph("", 4096, 16384).unwrap();
        let temporal_node = metadata_graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["resource"] == larch_now_source.as_str())
            .unwrap();
        assert!(
            temporal_node["revision"].is_null(),
            "metadata is not a payload revision"
        );
        assert!(!metadata_graph
            .to_string()
            .contains("larchUniqueLocator from larch NOW"));
        let own_with_now_error = service.knowledge_search("cedarOwnedLocator", 256).unwrap();
        assert!(!now_field_search_failed(&own_with_now_error));
        assert!(has_now_source(&own_with_now_error, &common_now_source));
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
        let now_marker = larch_now.parent().unwrap().join(".no-agent-retrieval");
        write(&now_marker, "owner withholds this temporal subtree\n");
        let withdrawn_graph = root_service.knowledge_graph("", 4096, 16384).unwrap();
        assert!(!graph_resources(&withdrawn_graph)
            .iter()
            .any(|resource| resource == larch_now_source.as_str()));
        fs::remove_file(&now_marker).unwrap();
        let restored_graph = root_service.knowledge_graph("", 4096, 16384).unwrap();
        assert!(graph_resources(&restored_graph)
            .iter()
            .any(|resource| resource == larch_now_source.as_str()));
        assert_eq!(
            fs::read_to_string(&larch_now).unwrap(),
            "larchUniqueLocator from larch NOW\n"
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
    assert_eq!(
        owner_indexes(&world),
        indexed_before,
        "Knowledge reads changed a native owner index"
    );
    assert_eq!(
        fs::read(gitnexus_home.join("registry.json")).unwrap(),
        registry_before,
        "Knowledge reads rewrote the native owner registry"
    );
    assert!(
        !world.join("Work/broken/.gitnexus").exists(),
        "read rebuilt the unavailable Project index"
    );
}
