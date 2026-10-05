//! W1/W3, CASE 05 — file context end to end at the engine seam: an operation
//! about to touch a file arrives with the wiki relations that cite it and the
//! guidance of any domain whose declared path patterns address it; unchanged
//! files re-inject nothing; standing rules stay exempt by classification.

use std::{fs, path::PathBuf};

use aikit_cli::domain_activation::load_domains;
use aikit_cli::file_context::{file_path_of, load_project_wiki, run};
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_store::index::Index;

/// The reactions hand back classified blocks now, so the pressure stage can
/// bound ordinary payload without touching standing guidance. These tests
/// assert on what a session actually sees, which is the rendered block.
fn rendered(result: (Vec<aikit_core::pressure::Block>, Vec<String>)) -> (Vec<String>, Vec<String>) {
    (
        result
            .0
            .iter()
            .map(aikit_core::pressure::Block::render)
            .collect(),
        result.1,
    )
}

fn node(ref_id: &str, node_type: &str, title: &str, source_refs: &[&str]) -> String {
    let sources: Vec<String> = source_refs.iter().map(|s| format!("\"{s}\"")).collect();
    format!(
        r#"{{"profile": "okf-wiki/v1", "object": "node", "ref": "{ref_id}", "type": "{node_type}", "title": "{title}", "source_refs": [{}], "provenance": [{{"source_ref": "source:test"}}]}}"#,
        sources.join(",")
    )
}

fn edge(ref_id: &str, from: &str, to: &str, relation: &str) -> String {
    format!(
        r#"{{"profile": "okf-wiki/v1", "object": "edge", "ref": "{ref_id}", "from_ref": "{from}", "to_ref": "{to}", "relation": "{relation}", "origin": "authored", "provenance": [{{"source_ref": "source:test"}}]}}"#
    )
}

/// A project whose wiki knows two things about `crates/demo/src/lib.rs` (the
/// node citing it and one authored link) and one thing about nothing else.
fn fixture() -> (PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap().keep();
    let domains = root.join(".aikit/domains");
    fs::create_dir_all(&domains).unwrap();
    fs::write(
        domains.join("demo.toml"),
        r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/demo-crate"
title = "Demo crate discipline"
revision = "r1"
source = "central:source:project:demo:.aikit/domains/demo.toml"
triggers = ["demo"]
path_patterns = ["crates/demo/**"]

[horizon_range]
min = 2
max = 4

[[guidance]]
rule = "Keep the demo crate's public surface documented"
provenance = "central:source:project:demo:ProjectCentral/user/demo.md"
classification = "ordinary"

[[guidance]]
rule = "The demo crate never ships a panic in a public fn"
rationale = "demonstrations teach by their calm"
provenance = "central:source:project:demo:ProjectCentral/user/demo.md"
classification = "standing"
"#,
    )
    .unwrap();
    let wiki_dir = root.join("ProjectCentral/agents/wiki");
    fs::create_dir_all(&wiki_dir).unwrap();
    let wiki = format!(
        r#"{{"objects": [{}, {}, {}, {}]}}"#,
        node(
            "wiki:node:demo-lib",
            "Module",
            "Demo library",
            &["source:project:crates/demo/src/lib.rs"]
        ),
        node(
            "wiki:node:demo-usage",
            "Guide",
            "Using the demo",
            &["source:project:docs/demo.md"]
        ),
        edge(
            "wiki:edge:demo-1",
            "wiki:node:demo-usage",
            "wiki:node:demo-lib",
            "documents"
        ),
        node(
            "wiki:node:unrelated",
            "Note",
            "Unrelated garden note",
            &["source:project:docs/garden.md"]
        ),
    );
    fs::write(wiki_dir.join("wiki.json"), wiki).unwrap();
    let index = Index::open(&root.join("state/aikit.sqlite3")).unwrap();
    drop(index);
    (root.clone(), root.join("state/aikit.sqlite3"))
}

fn index(path: &std::path::Path) -> Index {
    Index::open(path).unwrap()
}

fn scope() -> String {
    "session-file-context-test".to_owned()
}

fn lib_path(root: &std::path::Path) -> String {
    root.join("crates/demo/src/lib.rs")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_file_operation_arrives_with_its_wiki_relations_and_domain_guidance() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let (domains, warnings) = load_domains(&tmp);
    assert!(warnings.is_empty());
    let (objects, warnings) = load_project_wiki(&tmp);
    assert!(warnings.is_empty(), "wiki warnings: {:?}", warnings);
    let (blocks, warnings) = rendered(run(
        &index,
        &scope(),
        &tmp,
        &lib_path(&tmp),
        &domains,
        objects,
    ));
    assert!(warnings.is_empty(), "warnings: {:?}", warnings);
    assert_eq!(blocks.len(), 2, "{blocks:?}");

    let wiki_block = &blocks[0];
    assert!(
        wiki_block
            .starts_with("[continuity/file-context] wiki relations for crates/demo/src/lib.rs"),
        "{wiki_block}"
    );
    assert!(
        wiki_block.contains("wiki:node:demo-lib — Demo library"),
        "{wiki_block}"
    );
    assert!(
        wiki_block.contains("cited by: wiki:node:demo-usage (documents)"),
        "{wiki_block}"
    );
    assert!(
        !wiki_block.contains("unrelated"),
        "unrelated wiki material must not arrive: {wiki_block}"
    );

    let domain_block = &blocks[1];
    assert!(
        domain_block.starts_with(
            "[continuity/file-context] domain domain/demo-crate armed for crates/demo/src/lib.rs"
        ),
        "{domain_block}"
    );
    assert!(domain_block.contains("horizon: @2–@4"), "{domain_block}");
    assert!(
        domain_block.contains("source: central:source:project:demo"),
        "{domain_block}"
    );
    assert!(domain_block.contains("[ordinary]"), "{domain_block}");
    assert!(
        domain_block.contains("[standing — dedup-exempt by classification]"),
        "{domain_block}"
    );
}

#[test]
fn an_unchanged_file_reinjects_nothing_but_standing_rules_reassert() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let ctx = scope();
    let (domains, _) = load_domains(&tmp);
    let path = lib_path(&tmp);

    let (first, _) = rendered(run(
        &index,
        &ctx,
        &tmp,
        &path,
        &domains,
        load_project_wiki(&tmp).0,
    ));
    assert_eq!(first.len(), 2);

    // Same file, same relations: the ordinary payload dedups and the wiki
    // block stays out entirely; the standing rule reasserts, visibly exempt.
    let (second, _) = rendered(run(
        &index,
        &ctx,
        &tmp,
        &path,
        &domains,
        load_project_wiki(&tmp).0,
    ));
    assert_eq!(second.len(), 1, "{second:?}");
    assert!(
        second[0].contains("domain/demo-crate armed"),
        "{:?}",
        second[0]
    );
    assert!(
        second[0].contains("ordinary payload deduped"),
        "{:?}",
        second[0]
    );
    assert!(
        second[0].contains("standing rules reasserted"),
        "{:?}",
        second[0]
    );
    assert!(!second[0].contains("[ordinary]"), "{:?}", second[0]);
    assert!(!second[0].contains("wiki relations for"), "{:?}", second[0]);
}

#[test]
fn changed_wiki_relations_re_arm_injection() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let ctx = scope();
    let (domains, _) = load_domains(&tmp);
    let path = lib_path(&tmp);
    let (first, _) = rendered(run(
        &index,
        &ctx,
        &tmp,
        &path,
        &domains,
        load_project_wiki(&tmp).0,
    ));
    assert_eq!(first.len(), 2);

    // The wiki learns a new relation for the file: the rendered content
    // changes, so the next operation on the same file injects again.
    let wiki_file = tmp.join("ProjectCentral/agents/wiki/wiki.json");
    let updated = fs::read_to_string(&wiki_file)
        .unwrap()
        .replace("Demo library", "Demo library, revised");
    fs::write(&wiki_file, updated).unwrap();
    let (second, _) = rendered(run(
        &index,
        &ctx,
        &tmp,
        &path,
        &domains,
        load_project_wiki(&tmp).0,
    ));
    assert_eq!(second.len(), 2, "{second:?}");
    assert!(
        second[0].contains("Demo library, revised"),
        "{:?}",
        second[0]
    );
}

#[test]
fn an_unrelated_file_gets_nothing() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let (domains, _) = load_domains(&tmp);
    // A file the wiki holds nothing about: no relations may arrive.
    let stranger = tmp.join("docs/untracked.md").to_string_lossy().into_owned();
    let (blocks, warnings) = rendered(run(
        &index,
        &scope(),
        &tmp,
        &stranger,
        &domains,
        load_project_wiki(&tmp).0,
    ));
    assert!(blocks.is_empty(), "{blocks:?}");
    assert!(warnings.is_empty());
}

#[test]
fn events_without_an_explicit_file_path_stay_inert() {
    let event = HookEvent::new(
        "zcode",
        HookEventKind::PreToolUse,
        serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}),
    );
    assert!(file_path_of(&event).is_none());
    let reading = HookEvent::new(
        "zcode",
        HookEventKind::PreToolUse,
        serde_json::json!({"tool_name": "Read", "tool_input": {"file_path": "/tmp/a.rs"}}),
    );
    assert_eq!(file_path_of(&reading).as_deref(), Some("/tmp/a.rs"));
}

#[test]
fn a_path_addressed_domain_may_declare_no_triggers() {
    let text = r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/file-only"
title = "File-only discipline"
revision = "r1"
source = "central:source:project:demo:.aikit/domains/file-only.toml"
path_patterns = ["**/release/**"]

[[guidance]]
rule = "Name the release owner in every change"
provenance = "central:source:project:demo:ProjectCentral/user/release.md"
"#;
    let domain = aikit_core::domain::KnowledgeDomain::from_toml_str(text).expect("parses");
    assert!(domain.triggers.is_empty());
    assert_eq!(domain.path_patterns, vec!["**/release/**"]);

    let bad = text.replace(
        "path_patterns = [\"**/release/**\"]",
        "path_patterns = [\"\"]",
    );
    assert!(aikit_core::domain::KnowledgeDomain::from_toml_str(&bad).is_err());
}


// Physical/admission regressions below own RAII fixtures. They exercise the
// independent loader and real selected relation engine, not a fabricated
// Central source.read response or retained Service Project identity claim.
fn native_current_read_tempdir() -> tempfile::TempDir {
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&scratch).unwrap();
    tempfile::Builder::new().prefix("file-context-current-read-").tempdir_in(&scratch).unwrap()
}

fn owned_read_fixture(root: &std::path::Path) -> std::path::PathBuf {
    let wiki = root.join("ProjectCentral/agents/wiki/wiki.json");
    fs::create_dir_all(wiki.parent().unwrap()).unwrap();
    let objects = format!(r#"{{"objects":[{}]}}"#,
        node("wiki:node:r4-file", "Module", "R4current Wiki relation",
            &["source:project:src/lib.rs"]));
    fs::write(&wiki, objects).unwrap();
    wiki
}

#[test]
fn current_wiki_read_withdrawal_preserves_source_bytes_and_selected_engine_behavior() {
    let owned = native_current_read_tempdir();
    let project = owned.path().join("project");
    let wiki = owned_read_fixture(&project);
    let before = fs::read(&wiki).unwrap();
    let (objects, warnings) = load_project_wiki(&project);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(objects.len(), 1);
    let index = Index::open(&owned.path().join("state.sqlite3")).unwrap();
    let (blocks, warnings) = rendered(run(&index, "r4-before", &project, "src/lib.rs", &[], objects));
    assert!(warnings.is_empty());
    assert!(blocks.iter().any(|block| block.contains("R4current Wiki relation")));
    fs::write(project.join(".no-agent-retrieval"), "owner withdrew this aperture").unwrap();
    let (objects, warnings) = load_project_wiki(&project);
    assert!(objects.is_empty());
    assert!(warnings.iter().any(|warning| warning.contains("file_context.wiki_withheld")));
    let (blocks, _) = rendered(run(&index, "r4-after", &project, "src/lib.rs", &[], objects));
    assert!(blocks.is_empty());
    assert_eq!(fs::read(wiki).unwrap(), before);
    assert!(load_project_wiki(&project).0.is_empty());
}

#[test]
fn actual_containing_world_marker_withholds_while_external_project_keeps_its_root() {
    use aikit_cli::file_context::load_project_wiki_in_world;
    let owned = native_current_read_tempdir();
    let world = owned.path().join("world");
    let project = world.join("Work/Project");
    let wiki = owned_read_fixture(&project);
    fs::create_dir_all(world.join("Control")).unwrap();
    let external = owned.path().join("external-project");
    let external_wiki = owned_read_fixture(&external);
    let before = fs::read(&wiki).unwrap();
    let external_before = fs::read(&external_wiki).unwrap();
    assert_eq!(load_project_wiki_in_world(&project, Some(&world)).0.len(), 1);
    fs::write(world.join("Work/.no-agent-retrieval"), "owner withdrew Work descendants").unwrap();
    let (objects, warnings) = load_project_wiki_in_world(&project, Some(&world));
    assert!(objects.is_empty());
    assert!(warnings.iter().any(|warning| warning.contains("file_context.wiki_withheld")));
    let (objects, warnings) = load_project_wiki_in_world(&external, Some(&world));
    assert_eq!(objects.len(), 1);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(fs::read(wiki).unwrap(), before);
    assert_eq!(fs::read(external_wiki).unwrap(), external_before);
}

#[test]
fn unavailable_supplied_world_is_not_silently_reclassified_as_external_standalone() {
    use aikit_cli::file_context::load_project_wiki_in_world;
    let owned = native_current_read_tempdir();
    let project = owned.path().join("project");
    let wiki = owned_read_fixture(&project);
    let before = fs::read(&wiki).unwrap();
    let (objects, warnings) = load_project_wiki_in_world(&project, Some(&owned.path().join("missing-world")));
    assert!(objects.is_empty());
    assert!(warnings.iter().any(|warning| warning.contains("file_context.wiki_unavailable")));
    assert_eq!(fs::read(wiki).unwrap(), before);
    assert_eq!(load_project_wiki(&project).0.len(), 1);
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unchanged_project_and_world_root_aliases_keep_the_declared_native_read_aperture() {
    use aikit_cli::file_context::load_project_wiki_in_world;
    let owned = native_current_read_tempdir();
    let world = owned.path().join("world");
    let project = world.join("Work/Project");
    owned_read_fixture(&project);
    let world_alias = owned.path().join("world-alias");
    let project_alias = owned.path().join("project-alias");
    std::os::unix::fs::symlink(&world, &world_alias).unwrap();
    std::os::unix::fs::symlink(&project, &project_alias).unwrap();
    let (objects, warnings) = load_project_wiki_in_world(&project_alias, Some(&world_alias));
    assert_eq!(objects.len(), 1);
    assert!(warnings.is_empty(), "{warnings:?}");
    fs::write(world.join("Work/.no-agent-retrieval"), "owner withdrawal").unwrap();
    let (objects, warnings) = load_project_wiki_in_world(&project_alias, Some(&world_alias));
    assert!(objects.is_empty());
    assert!(warnings.iter().any(|warning| warning.contains("file_context.wiki_withheld")));
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn actual_final_source_aliases_and_fifo_are_withheld_without_reading_foreign_material() {
    use aikit_adapters::runner::{CommandRunner, SystemRunner};
    for form in ["symlink", "hardlink", "fifo"] {
        let owned = native_current_read_tempdir();
        let project = owned.path().join("project");
        let wiki = owned_read_fixture(&project);
        let retained = project.join("retained-wiki.json");
        fs::rename(&wiki, &retained).unwrap();
        let before = fs::read(&retained).unwrap();
        match form {
            "symlink" => std::os::unix::fs::symlink(&retained, &wiki).unwrap(),
            "hardlink" => fs::hard_link(&retained, &wiki).unwrap(),
            "fifo" => {
                let argv = vec!["mkfifo".to_string(), wiki.to_string_lossy().into_owned()];
                SystemRunner::new().with_timeout(std::time::Duration::from_secs(1))
                    .run(&argv).unwrap().require(&argv, "file_context.fixture_fifo").unwrap();
            }
            _ => unreachable!(),
        }
        let (objects, warnings) = load_project_wiki(&project);
        assert!(objects.is_empty(), "{form}");
        assert!(!warnings.is_empty(), "{form}");
        assert_eq!(fs::read(&retained).unwrap(), before);
        assert!(fs::symlink_metadata(wiki).is_ok());
    }
}

#[test]
fn oversize_actual_wiki_is_unavailable_without_truncation_or_source_deletion() {
    let owned = native_current_read_tempdir();
    let project = owned.path().join("project");
    let wiki = owned_read_fixture(&project);
    let file = fs::OpenOptions::new().write(true).open(&wiki).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    let (objects, warnings) = load_project_wiki(&project);
    assert!(objects.is_empty());
    assert!(warnings.iter().any(|warning| warning.contains("knowledge.wiki_publication_budget")));
    assert_eq!(fs::metadata(wiki).unwrap().len(), 16 * 1024 * 1024 + 1);
}

#[test]
fn actual_missing_wiki_is_honest_absence_but_malformed_existing_wiki_is_disclosed() {
    let owned = native_current_read_tempdir();
    let project = owned.path().join("project");
    fs::create_dir(&project).unwrap();
    let (objects, warnings) = load_project_wiki(&project);
    assert!(objects.is_empty());
    assert!(warnings.is_empty());
    let wiki = owned_read_fixture(&project);
    fs::write(&wiki, b"{not valid native Wiki}").unwrap();
    let (objects, warnings) = load_project_wiki(&project);
    assert!(objects.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(fs::read(wiki).unwrap(), b"{not valid native Wiki}");
}


#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn actual_first_party_selected_hook_observes_withdrawal_and_unselected_hook_stays_inert() {
    use aikit_cli::app::Service;
    use aikit_core::catalog::Catalog;
    use aikit_core::{CapsuleId, ContextId, TrustKey, TrustState};
    use aikit_store::{AikitHome, TrustStore};
    let owned = native_current_read_tempdir();
    let project = owned.path().join("project");
    let wiki = owned_read_fixture(&project);
    let retained = fs::read(&wiki).unwrap();
    let home = AikitHome::at(owned.path().join("home"));
    let native = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../registry/capsules/hook/continuity/file-context");
    let installed = home.root().join("registries/personal/capsules/hook/continuity/file-context");
    fs::create_dir_all(installed.join("payload")).unwrap();
    // Use exact existing first-party composition handle bytes. Its actual
    // native contract delegates the reaction to this engine; no owner result,
    // transport callback or model response is mocked by the fixture.
    for member in ["manifest.toml", "payload/file-context"] {
        fs::copy(native.join(member), installed.join(member)).unwrap();
        assert_eq!(fs::read(native.join(member)).unwrap(), fs::read(installed.join(member)).unwrap());
    }
    fs::create_dir_all(project.join(".aikit")).unwrap();
    fs::write(project.join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"hook/continuity/file-context\"]\n").unwrap();
    let context = ContextId::generate().to_string();
    let mut service = Service::open(home.clone(), &project, |key|
        (key == "AIKIT_CONTEXT_ID").then(|| context.clone())).unwrap();
    let id = CapsuleId::parse("hook/continuity/file-context").unwrap();
    let capsule = service.snapshot().get(&id).expect("actual native capsule was discovered");
    let key = TrustKey::new(capsule.source.clone().unwrap(), id, capsule.revision.clone().unwrap());
    TrustStore::new(service.index()).record(&key, TrustState::Trusted,
        Some("explicit owned first-party fixture review")).unwrap();
    service.refresh().unwrap();
    let event = |session: &str| HookEvent::new("codex", HookEventKind::PreToolUse,
        serde_json::json!({"session_id":session,"tool_name":"Read","tool_input":{"file_path":"src/lib.rs"}}));
    let before = service.dispatch_hook(&event("r4-selected-before")).unwrap();
    assert!(before.injected.iter().any(|block| block.contains("R4current Wiki relation")), "{before:?}");
    fs::write(project.join(".no-agent-retrieval"), "current owner withdrawal").unwrap();
    let after = service.dispatch_hook(&event("r4-selected-after")).unwrap();
    assert!(!after.injected.iter().any(|block| block.contains("R4current Wiki relation")), "{after:?}");
    assert!(after.warnings.iter().any(|warning| warning.contains("file_context.wiki_withheld")), "{after:?}");
    assert_eq!(fs::read(&wiki).unwrap(), retained);
    fs::write(project.join(".aikit/profile.toml"), "schema = 1\nenable = []\n").unwrap();
    fs::write(&wiki, b"invalid existing Wiki which unselected engine must not read").unwrap();
    let unselected = Service::open(home, &project, |_| None).unwrap();
    let inactive = unselected.dispatch_hook(&event("r4-unselected")).unwrap();
    assert!(!inactive.injected.iter().any(|block| block.contains("R4current Wiki relation")));
    assert!(!inactive.warnings.iter().any(|warning| warning.contains("continuity/file-context")), "{inactive:?}");
}


#[cfg(any(target_os = "linux", target_os = "macos"))]
fn invocation_relative_route(path: &std::path::Path) -> std::path::PathBuf {
    let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    for (up, ancestor) in cwd.ancestors().enumerate() {
        if let Ok(member) = path.strip_prefix(ancestor) {
            let mut relative = std::path::PathBuf::new();
            for _ in 0..up { relative.push(".."); }
            relative.push(member);
            return relative;
        }
    }
    panic!("actual Unix fixture and invocation must have a common root");
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn original_world_alias_ancestry_withholds_for_absolute_and_relative_selected_routes() {
    use aikit_cli::file_context::load_project_wiki_in_world;
    let owned = native_current_read_tempdir();
    let world = owned.path().join("world");
    let project = world.join("Work/allowed/Project");
    let wiki = owned_read_fixture(&project);
    fs::create_dir_all(world.join("Work/withheld")).unwrap();
    let world = fs::canonicalize(&world).unwrap();
    let project = world.join("Work/allowed/Project");
    std::os::unix::fs::symlink(&project, world.join("Work/withheld/alias")).unwrap();
    std::os::unix::fs::symlink(&world, world.join("Work/withheld/world-alias")).unwrap();
    let world_relative = invocation_relative_route(&world);
    let before = fs::read(&wiki).unwrap();
    let marker = world.join("Work/withheld/.no-agent-retrieval");
    for member in ["Work/withheld/alias", "Work/withheld/world-alias/Work/allowed/Project"] {
        let selected = world.join(member);
        let selected_relative = invocation_relative_route(&selected);
        let routes = [(&selected, &world), (&selected_relative, &world),
            (&selected, &world_relative), (&selected_relative, &world_relative)];
        for (selected, world) in routes {
            let (objects, warnings) = load_project_wiki_in_world(selected, Some(world));
            assert_eq!(objects.len(), 1, "{selected:?} {world:?}: {warnings:?}");
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        fs::write(&marker, b"actual owner withdrew this original World route").unwrap();
        for (selected, world) in routes {
            let (objects, warnings) = load_project_wiki_in_world(selected, Some(world));
            assert!(objects.is_empty(), "{selected:?} {world:?}: {objects:?}");
            assert!(warnings.iter().any(|warning| warning.contains("file_context.wiki_withheld")),
                "{warnings:?}");
            assert_eq!(fs::read(&wiki).unwrap(), before);
        }
        fs::remove_file(&marker).unwrap();
        for (selected, world) in routes {
            let (objects, warnings) = load_project_wiki_in_world(selected, Some(world));
            assert_eq!(objects.len(), 1, "useful unchanged alias must resume: {warnings:?}");
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }
    assert_eq!(fs::read(wiki).unwrap(), before);
}
