//! The broker: one small generic skill for clients that cannot be handed a
//! directory of their own.
//!
//! Its whole value is that it is *small*. An index that grew with the catalogue
//! would spend the context window it was supposed to save, and an index that
//! inlined instructions would be a worse version of the native projection it
//! exists to replace. So the two properties tested hardest are: the index is
//! bounded, and it never contains a payload body.

mod common;

use common::*;

use std::path::Path;

use aikit_adapters::clients::broker::{BrokerAdapter, IndexBudget};
use aikit_adapters::clients::ClientAdapter;
use aikit_core::context::Isolation;
use aikit_core::projection::{ProjectionItem, ResolvedContext, TargetAdapter};
use aikit_core::resolve::AppliedSkillUsageOverlay;
use aikit_core::scope::{LayerOrigin, ScopeKind};

/// A context whose skill payloads contain a marker no index may ever carry.
fn context(registry: &Path, skills: usize) -> ResolvedContext {
    let mut builder = ContextBuilder::new().isolation(Isolation::Shared);
    for i in 0..skills {
        let id = format!("skill/rust/s{i:03}");
        let root = registry.join(&id);
        write_payload_skill(&root, &format!("s{i:03}"), "Does a thing.");
        builder = builder.project_skill(
            &id,
            "Reviews Rust for correctness and performance, at length, with examples.",
            &root,
        );
    }
    builder.build()
}

fn index_of(context: &ResolvedContext) -> String {
    BrokerAdapter::new().index(context)
}

// ---------------------------------------------------------------------------
// What the skill tells the model
// ---------------------------------------------------------------------------

#[test]
fn the_broker_skill_documents_the_three_commands_and_nothing_else() {
    let markdown = BrokerAdapter::new().skill_markdown();

    assert!(markdown.contains("aikit capabilities list --context current --agent-index"));
    assert!(markdown.contains("aikit capabilities read <id>"));
    assert!(markdown.contains("aikit run <id>"));
    assert!(
        markdown.starts_with("---"),
        "the broker is itself an Agent Skill and needs frontmatter"
    );
    assert!(markdown.contains("name: aikit"));
}

#[test]
fn the_broker_skill_is_a_valid_agent_skill_when_projected() {
    let registry = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 2);

    let plan = BrokerAdapter::new().plan(&context).unwrap();
    materialize(&plan.items, target.path());

    let skill = aikit_adapters::clients::agent_skills::validate(
        &target.path().join(".agents/skills/aikit"),
    )
    .expect("the broker must satisfy the same validation as any other skill");
    assert_eq!(skill.name, "aikit");
    assert!(!skill.description.is_empty());
}

#[test]
fn the_projection_is_the_skill_and_its_index_and_nothing_more() {
    let registry = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 3);

    let plan = BrokerAdapter::new().plan(&context).unwrap();
    let destinations: Vec<String> = plan
        .items
        .iter()
        .filter_map(|i| i.destination())
        .map(|p| p.display().to_string())
        .collect();

    assert_eq!(
        destinations,
        vec![
            ".agents/skills/aikit/SKILL.md",
            ".agents/skills/aikit/INDEX.md",
        ]
    );
    assert!(
        plan.items
            .iter()
            .all(|i| matches!(i, ProjectionItem::Write { .. })),
        "the broker generates its files; it never links a capsule payload"
    );
}

// ---------------------------------------------------------------------------
// The index is metadata only
// ---------------------------------------------------------------------------

#[test]
fn the_index_lists_the_id_the_name_and_a_one_line_description() {
    let registry = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 2);
    let index = index_of(&context);

    assert!(index.contains("skill/rust/s000"), "got:\n{index}");
    assert!(index.contains("s000"));
    assert!(
        index.contains("Reviews Rust for correctness"),
        "got:\n{index}"
    );
}

#[test]
fn the_index_never_contains_a_payload_body() {
    // The payloads on disk all carry a marker. If the broker ever started reading
    // instructions instead of metadata, this is where it would show up.
    let registry = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 5);
    let index = index_of(&context);

    assert!(
        !index.contains("AIKIT-PAYLOAD-BODY-MARKER"),
        "the index inlined a payload body:\n{index}"
    );
    assert!(!index.contains("#!/bin/sh"));
}

#[test]
fn a_multi_sentence_description_is_reduced_to_its_first_sentence() {
    let registry = tempfile::tempdir().unwrap();
    let root = registry.path().join("skill/rust/verbose");
    write_payload_skill(&root, "verbose", "Short.");
    let context = ContextBuilder::new()
        .project_skill(
            "skill/rust/verbose",
            "Reviews Rust. Then it does a great many other things which the model does not need to know about up front.",
            &root,
        )
        .build();

    let index = index_of(&context);
    assert!(index.contains("Reviews Rust."));
    assert!(
        !index.contains("great many other things"),
        "the rest is what `aikit capabilities read` is for:\n{index}"
    );
}

#[test]
fn skill_routing_orientation_survives_upstream_first_sentence_compaction() {
    let registry = tempfile::tempdir().unwrap();
    let root = registry.path().join("skill/rust/wayfinder");
    write_payload_skill(&root, "wayfinder", "Plans long work.");
    let mut context = ContextBuilder::new()
        .project_skill(
            "skill/rust/wayfinder",
            "Plans long work. The remaining upstream detail belongs in the full read.",
            &root,
        )
        .build();
    context.view.skill_usage_overlays.insert(
        cid("skill/rust/wayfinder"),
        vec![AppliedSkillUsageOverlay {
            description: Some("Prefer for work spanning agent sessions.".into()),
            guidance: None,
            reviewed_against: None,
            scope: ScopeKind::Global,
            origin: LayerOrigin::new("test:user-baseline"),
            via_profile: None,
        }],
    );

    let index = index_of(&context);
    assert!(index.contains("Plans long work."), "got:\n{index}");
    assert!(
        index.contains("Prefer for work spanning agent sessions."),
        "the routing augmentation must remain in the compact broker index:\n{index}"
    );
    assert!(!index.contains("remaining upstream detail"), "got:\n{index}");
}

#[test]
fn the_index_says_how_to_get_the_full_instructions() {
    let registry = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 2);
    let index = index_of(&context);

    assert!(
        index.contains("aikit capabilities read"),
        "an index with no way out of it is a dead end:\n{index}"
    );
}

// ---------------------------------------------------------------------------
// The index is bounded
// ---------------------------------------------------------------------------

#[test]
fn a_large_catalogue_produces_a_bounded_index() {
    let registry = tempfile::tempdir().unwrap();
    let small = index_of(&context(registry.path(), 4));

    let big_registry = tempfile::tempdir().unwrap();
    let big = index_of(&context(big_registry.path(), 400));

    let budget = IndexBudget::default();
    assert!(
        big.len() <= budget.max_bytes,
        "the index grew to {} bytes, past the {} byte budget",
        big.len(),
        budget.max_bytes
    );
    assert!(
        big.len() > small.len(),
        "a bounded index should still say more when there is more to say"
    );
}

#[test]
fn a_truncated_index_says_how_many_it_left_out_and_how_to_see_them() {
    let registry = tempfile::tempdir().unwrap();
    let index = index_of(&context(registry.path(), 400));

    assert!(
        index.contains("more"),
        "a silently truncated list is a list a model will trust wrongly:\n{}",
        &index[index.len().saturating_sub(400)..]
    );
    assert!(index.contains("aikit capabilities list"));
}

#[test]
fn the_budget_is_configurable_and_is_actually_respected() {
    let registry = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 50);

    let tiny = BrokerAdapter::new()
        .with_budget(IndexBudget {
            max_entries: 3,
            max_bytes: 4096,
            max_description_chars: 40,
        })
        .index(&context);

    let entries = tiny.lines().filter(|l| l.starts_with("- ")).count();
    assert_eq!(entries, 3, "got:\n{tiny}");
    for line in tiny.lines().filter(|l| l.starts_with("- ")) {
        assert!(
            line.len() < 160,
            "a 40-character description budget produced a {}-character line: {line}",
            line.len()
        );
    }
}

#[test]
fn an_empty_context_produces_an_index_that_says_so_rather_than_an_empty_file() {
    let registry = tempfile::tempdir().unwrap();
    let root = registry.path().join("script/test/only");
    std::fs::create_dir_all(&root).unwrap();
    let context = ContextBuilder::new()
        .project_script("script/test/only", "Runs tests.", &root)
        .build();

    let index = index_of(&context);
    assert!(!index.trim().is_empty());
    assert!(
        index.contains("script/test/only"),
        "a runnable script is a capability the broker can expose:\n{index}"
    );
}

// ---------------------------------------------------------------------------
// Launching and installing
// ---------------------------------------------------------------------------

#[test]
fn the_broker_has_no_client_of_its_own_to_launch_or_configure() {
    let registry = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let context = context(registry.path(), 1);
    let broker = BrokerAdapter::new();

    assert!(
        broker.launch_command(&context).is_empty(),
        "the broker is a skill inside somebody else's client, not a client"
    );
    assert!(broker.install(config.path()).unwrap().is_empty());
}

#[test]
fn the_broker_works_in_a_shared_tree_because_it_writes_no_per_task_state() {
    let broker = BrokerAdapter::new();
    let caps = broker.capabilities();

    assert!(caps.brokered_fallback);
    assert!(
        !caps.requires_isolated_tree_for_isolation,
        "one generic skill is the same in every task; there is nothing to keep apart"
    );
}
