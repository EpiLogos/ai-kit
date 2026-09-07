//! W1/CASE 03 — domain activation end to end at the engine seam: declared
//! domains load from the project layer, activation is deterministic, the
//! ordinary payload dedups on rendered content, and standing rules are
//! exempt by classification with that classification visible.

use std::{fs, path::PathBuf};

use aikit_cli::domain_activation::{load_domains, prompt_of, run};
use aikit_core::ContextId;
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_store::index::Index;

fn fixture() -> (PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap().keep();
    let domains = root.join(".aikit/domains");
    fs::create_dir_all(&domains).unwrap();
    fs::write(
        domains.join("release.toml"),
        r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/release"
title = "Release discipline"
revision = "r1"
source = "central:source:project:demo:.aikit/domains/release.toml"
triggers = ["release"]

[horizon_range]
min = 3
max = 5

[[guidance]]
rule = "Run the verification suite before tagging"
rationale = "tags are the immutable boundary"
provenance = "central:source:project:demo:ProjectCentral/user/release.md"
classification = "ordinary"

[[guidance]]
rule = "A red gate is a stop, never a note"
provenance = "central:source:project:demo:ProjectCentral/user/release.md"
classification = "standing"
"#,
    )
    .unwrap();
    let index = Index::open(&root.join("state/aikit.sqlite3")).unwrap();
    drop(index);
    (root.clone(), root.join("state/aikit.sqlite3"))
}

fn index(path: &std::path::Path) -> Index {
    Index::open(path).unwrap()
}

fn context() -> ContextId {
    ContextId::parse("ctx_domain_test").unwrap()
}

#[test]
fn declared_domains_load_from_the_project_layer_and_invalid_ones_are_disclosed() {
    let (tmp, _) = fixture();
    fs::write(
        tmp.join(".aikit/domains/broken.toml"),
        "schema = \"aikit.knowledge-domain/v1\"\nid = \"\"\n",
    )
    .unwrap();
    let (domains, warnings) = load_domains(&tmp);
    assert_eq!(domains.len(), 1, "the valid declaration loads");
    assert_eq!(domains[0].id, "domain/release");
    assert!(
        warnings.iter().any(|w| w.contains("broken.toml") && w.contains("refused")),
        "{warnings:?}"
    );
}

#[test]
fn a_matching_prompt_activates_and_the_explanation_names_trigger_horizon_source() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let (domains, warnings) = load_domains(&tmp);
    assert!(warnings.is_empty());
    let (blocks, warnings) = run(&index, &context(), &domains, Some("please prepare this RELEASE"));
    assert!(warnings.is_empty());
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert!(block.starts_with("[continuity/domain-activation] domain domain/release activated"));
    assert!(block.contains("trigger: \"release\""), "{block}");
    assert!(block.contains("horizon: @3–@5"), "{block}");
    assert!(block.contains("source: central:source:project:demo"), "{block}");
    assert!(block.contains("[ordinary]"), "{block}");
    assert!(block.contains("[standing — dedup-exempt by classification]"), "{block}");
}

#[test]
fn unchanged_ordinary_payload_dedups_but_standing_rules_reassert() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let ctx = context();
    let (domains, _) = load_domains(&tmp);

    let (first, _) = run(&index, &ctx, &domains, Some("prepare this release"));
    assert_eq!(first.len(), 1);
    assert!(!first[0].contains("deduped"));

    // Same prompt again: the ordinary payload is deduped; the standing rule
    // is exempt by classification and reasserts with its exemption visible.
    let (second, _) = run(&index, &ctx, &domains, Some("prepare this release"));
    assert_eq!(second.len(), 1, "only the standing rule reasserts");
    assert!(second[0].contains("ordinary payload deduped"), "{:?}", second[0]);
    assert!(second[0].contains("standing rules reasserted"), "{:?}", second[0]);
    assert!(second[0].contains("[standing — dedup-exempt by classification]"), "{:?}", second[0]);
    assert!(!second[0].contains("[ordinary]"), "the ordinary line does not re-inject: {:?}", second[0]);

    // A third ask changes nothing about the verdict.
    let (third, _) = run(&index, &ctx, &domains, Some("prepare this release"));
    assert_eq!(third.len(), 1);
}

#[test]
fn changed_guidance_content_re_arms_injection() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let ctx = context();
    let (domains, _) = load_domains(&tmp);
    let (first, _) = run(&index, &ctx, &domains, Some("prepare this release"));
    assert_eq!(first.len(), 1);

    // Edit the ordinary guidance: the rendered content changes, so the next
    // matching prompt injects the full payload again.
    let domain_file = tmp.join(".aikit/domains/release.toml");
    let updated = fs::read_to_string(&domain_file)
        .unwrap()
        .replace("before tagging", "before cutting");
    fs::write(&domain_file, updated).unwrap();
    let (domains, _) = load_domains(&tmp);
    let (second, _) = run(&index, &ctx, &domains, Some("prepare this release"));
    assert_eq!(second.len(), 1);
    assert!(!second[0].contains("deduped"), "changed content re-arms: {:?}", second[0]);
}

#[test]
fn a_non_matching_prompt_activates_nothing() {
    let (tmp, db) = fixture();
    let index = index(&db);
    let (domains, _) = load_domains(&tmp);
    let (blocks, warnings) = run(&index, &context(), &domains, Some("water the garden"));
    assert!(blocks.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn the_prompt_is_read_from_the_submit_payload() {
    let event = HookEvent::new(
        "zcode",
        HookEventKind::UserPromptSubmit,
        serde_json::json!({"prompt": "prepare this release"}),
    );
    assert_eq!(prompt_of(&event).as_deref(), Some("prepare this release"));
    let empty = HookEvent::new("zcode", HookEventKind::UserPromptSubmit, serde_json::json!({}));
    assert!(prompt_of(&empty).is_none());
}
