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
