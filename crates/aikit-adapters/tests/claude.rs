//! The Claude Code adapter.
//!
//! Claude Code takes an arbitrary extra directory on the command line, which is
//! the whole reason it can give two sessions in one checkout different skills.
//! AIKit builds that directory inside the generation and points Claude at it.
//!
//! The prohibition tested hardest here is negative: **nothing** this adapter
//! emits may touch `~/.claude/skills` or the project's own `.claude/skills`.
//! Those belong to the user and to the repository. A context-scoped router that
//! wrote into either would be a global mutable active set wearing a disguise.

mod common;

use common::*;

use std::path::{Path, PathBuf};

use aikit_adapters::clients::claude::ClaudeAdapter;
use aikit_adapters::clients::ClientAdapter;
use aikit_core::context::Isolation;
use aikit_core::projection::{
    ActivationEffect, MaterializationMode, ProjectionItem, TargetAdapter,
};
use aikit_core::platform::TargetId;

fn adapter(generation: &Path) -> ClaudeAdapter {
    ClaudeAdapter::new(generation)
}

/// A context with two skills whose payloads are real directories on disk.
fn context_with_skills(registry: &Path) -> aikit_core::projection::ResolvedContext {
    let review = registry.join("skill/rust/code-review");
    let perf = registry.join("skill/rust/perf");
    write_payload_skill(&review, "code-review", "Reviews Rust for correctness.");
    write_payload_skill(&perf, "perf", "Finds hot loops.");

    ContextBuilder::new()
        .project_skill("skill/rust/code-review", "Reviews Rust.", &review)
        .project_skill("skill/rust/perf", "Finds hot loops.", &perf)
        .project_script("script/test/cargo-nextest", "Runs tests.", registry.join("script"))
        .build()
}

// ---------------------------------------------------------------------------
// Where the projection goes
// ---------------------------------------------------------------------------

#[test]
fn the_projection_root_is_inside_the_generation() {
    let generation = PathBuf::from("/home/u/.aikit/state/contexts/ctx_x/generations/gen_ab12");
    assert_eq!(
        adapter(&generation).projection_root(),
        generation.join("projections/claude")
    );
}

#[test]
fn one_skill_becomes_one_entry_under_dot_claude_skills() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());

    let plan = adapter(generation.path()).plan(&context).unwrap();

    let destinations: Vec<String> = plan
        .items
        .iter()
        .filter_map(|i| i.destination())
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(
        destinations,
        vec![".claude/skills/code-review", ".claude/skills/perf"],
        "the active script is not a skill and must not be projected here"
    );
    assert_eq!(plan.target, TargetId::claude_code());
}

#[test]
fn skills_are_symlinked_to_the_capsule_payload_so_they_cannot_drift() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());

    let plan = adapter(generation.path()).plan(&context).unwrap();
    match &plan.items[0] {
        ProjectionItem::Link { from, .. } => assert_eq!(
            from,
            &registry.path().join("skill/rust/code-review/payload"),
            "the link must point at the capsule's own payload"
        ),
        other => panic!("expected a link, got {other:?}"),
    }
}

#[test]
fn a_filesystem_without_symlinks_falls_back_to_copies_of_the_whole_tree() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());

    let plan = adapter(generation.path())
        .with_materialization(MaterializationMode::Copy)
        .plan(&context)
        .unwrap();

    let destinations: Vec<String> = plan
        .items
        .iter()
        .filter_map(|i| i.destination())
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(
        destinations,
        vec![
            ".claude/skills/code-review/SKILL.md",
            ".claude/skills/code-review/references/deep.md",
            ".claude/skills/code-review/scripts/check.sh",
            ".claude/skills/perf/SKILL.md",
            ".claude/skills/perf/references/deep.md",
            ".claude/skills/perf/scripts/check.sh",
        ],
        "a copy fallback must still preserve progressive disclosure"
    );
}

#[test]
fn the_projected_tree_is_exactly_this() {
    // The golden tree, materialized for real.
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());

    let plan = adapter(generation.path())
        .with_materialization(MaterializationMode::Copy)
        .plan(&context)
        .unwrap();
    let root = generation.path().join("projections/claude");
    materialize(&plan.items, &root);

    assert_eq!(
        tree_of(&root),
        vec![
            ".claude/".to_string(),
            ".claude/skills/".to_string(),
            ".claude/skills/code-review/".to_string(),
            ".claude/skills/code-review/SKILL.md".to_string(),
            ".claude/skills/code-review/references/".to_string(),
            ".claude/skills/code-review/references/deep.md".to_string(),
            ".claude/skills/code-review/scripts/".to_string(),
            ".claude/skills/code-review/scripts/check.sh".to_string(),
            ".claude/skills/perf/".to_string(),
            ".claude/skills/perf/SKILL.md".to_string(),
            ".claude/skills/perf/references/".to_string(),
            ".claude/skills/perf/references/deep.md".to_string(),
            ".claude/skills/perf/scripts/".to_string(),
            ".claude/skills/perf/scripts/check.sh".to_string(),
        ]
    );
}

// ---------------------------------------------------------------------------
// What it must never touch
// ---------------------------------------------------------------------------

#[test]
fn no_item_ever_reaches_the_users_home_or_the_projects_own_skill_directory() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());
    let project_root = context.view.context.project_root.clone().unwrap();

    for mode in [MaterializationMode::Link, MaterializationMode::Copy] {
        let plan = adapter(generation.path())
            .with_materialization(mode)
            .plan(&context)
            .unwrap();

        for item in &plan.items {
            let destination = item
                .destination()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            // Destinations are root-relative by construction; the danger is a
            // *resolved* destination landing outside the generation.
            let resolved = generation.path().join("projections/claude").join(&destination);
            assert!(
                resolved.starts_with(generation.path()),
                "{resolved:?} is outside the generation"
            );
            assert!(
                !resolved.starts_with(&project_root),
                "the project's own .claude/skills is not AIKit's to write: {resolved:?}"
            );
            assert!(
                !destination.contains(".."),
                "no destination may climb out: {destination}"
            );
        }
    }
}

#[test]
fn the_launch_command_points_claude_at_the_generations_projection() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());

    assert_eq!(
        adapter(generation.path()).launch_command(&context),
        vec![
            "claude".to_string(),
            "--add-dir".to_string(),
            generation
                .path()
                .join("projections/claude")
                .display()
                .to_string(),
        ]
    );
}

// ---------------------------------------------------------------------------
// Activation effect
// ---------------------------------------------------------------------------

#[test]
fn a_changed_projection_is_live_because_claude_watches_the_directory() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());
    let claude = adapter(generation.path());

    let plan = claude.plan(&context).unwrap();
    assert_eq!(plan.effect, ActivationEffect::LiveReloadExpected);
    assert_eq!(
        claude.activation_effect(None, &plan),
        ActivationEffect::LiveReloadExpected,
        "a first apply has something to load"
    );
}

#[test]
fn an_unchanged_projection_is_immediate_rather_than_pretending_to_reload() {
    let registry = tempfile::tempdir().unwrap();
    let generation = tempfile::tempdir().unwrap();
    let context = context_with_skills(registry.path());
    let claude = adapter(generation.path());

    let first = claude.plan(&context).unwrap();
    let second = claude.plan(&context).unwrap();
    assert_eq!(first.digest(), second.digest());

    match claude.activation_effect(Some(&first), &second) {
        ActivationEffect::Immediate { via } => assert!(!via.is_empty()),
        other => panic!("an unchanged projection is already in effect, got {other:?}"),
    }
}

#[test]
fn claude_can_isolate_a_shared_task_because_it_takes_an_arbitrary_directory() {
    let generation = tempfile::tempdir().unwrap();
    let caps = adapter(generation.path()).capabilities();

    assert!(caps.isolated_per_context);
    assert!(
        !caps.requires_isolated_tree_for_isolation,
        "Claude's skill directory is not in the tree, so a shared tree is no obstacle"
    );
    assert!(caps.can_isolate(Isolation::Shared));
    assert_eq!(caps.fallback_reason(Isolation::Shared), None);
    assert!(caps.watches_for_changes);
}

// ---------------------------------------------------------------------------
// The hook dispatcher entries
// ---------------------------------------------------------------------------

fn settings_after_install(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
}

#[test]
fn installing_writes_one_dispatcher_entry_per_event() {
    let generation = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    let items = adapter(generation.path()).install(config.path()).unwrap();
    assert_eq!(items.len(), 1, "one settings file");
    materialize(&items, config.path());

    let settings = settings_after_install(config.path());
    let hooks = settings["hooks"].as_object().unwrap();
    let mut events: Vec<&String> = hooks.keys().collect();
    events.sort();
    assert_eq!(
        events,
        vec![
            "PostToolUse",
            "PreToolUse",
            "SessionEnd",
            "SessionStart",
            "Stop",
            "UserPromptSubmit",
        ]
    );

    for (event, entries) in hooks {
        let commands: Vec<&str> = entries
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert_eq!(
            commands,
            vec![format!("aikit hook dispatch claude {event}")],
            "exactly one durable dispatcher entry per event"
        );
    }
}

#[test]
fn only_the_tool_events_carry_a_matcher() {
    let generation = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    materialize(
        &adapter(generation.path()).install(config.path()).unwrap(),
        config.path(),
    );

    let settings = settings_after_install(config.path());
    assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "*");
    assert!(
        settings["hooks"]["SessionStart"][0].get("matcher").is_none(),
        "a matcher on an event with no tool name is noise"
    );
}

#[test]
fn installing_twice_is_byte_for_byte_idempotent() {
    let generation = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let claude = adapter(generation.path());

    materialize(&claude.install(config.path()).unwrap(), config.path());
    let first = std::fs::read_to_string(config.path().join("settings.json")).unwrap();

    materialize(&claude.install(config.path()).unwrap(), config.path());
    let second = std::fs::read_to_string(config.path().join("settings.json")).unwrap();

    assert_eq!(first, second);
}

#[test]
fn installing_merges_into_an_existing_settings_file_without_destroying_anything() {
    let generation = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("settings.json"),
        r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(cargo test:*)"] },
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-own-guard" }] }
    ],
    "PreCompact": [
      { "hooks": [{ "type": "command", "command": "my-own-compactor" }] }
    ]
  }
}"#,
    )
    .unwrap();

    materialize(
        &adapter(generation.path()).install(config.path()).unwrap(),
        config.path(),
    );
    let settings = settings_after_install(config.path());

    assert_eq!(settings["model"], "opus");
    assert_eq!(settings["permissions"]["allow"][0], "Bash(cargo test:*)");
    assert!(
        settings["hooks"]["PreCompact"][0]["hooks"][0]["command"] == "my-own-compactor",
        "an event AIKit does not dispatch is none of its business"
    );

    let pre_tool: Vec<&str> = settings["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        pre_tool.contains(&"my-own-guard"),
        "the user's own hook must survive: {pre_tool:?}"
    );
    assert!(pre_tool.contains(&"aikit hook dispatch claude PreToolUse"));
}

#[test]
fn a_stale_aikit_entry_is_replaced_rather_than_joined_by_a_second_one() {
    let generation = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("settings.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch claude Stopp"}]}]}}"#,
    )
    .unwrap();

    materialize(
        &adapter(generation.path()).install(config.path()).unwrap(),
        config.path(),
    );
    let settings = settings_after_install(config.path());

    let stop: Vec<&str> = settings["hooks"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert_eq!(
        stop,
        vec!["aikit hook dispatch claude Stop"],
        "a typo'd old AIKit entry has to go, or every event fires twice: {stop:?}"
    );
}

#[test]
fn a_settings_file_that_is_not_json_is_refused_rather_than_overwritten() {
    let generation = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(config.path().join("settings.json"), "{ this is not json").unwrap();

    let error = adapter(generation.path()).install(config.path()).unwrap_err();
    assert_eq!(error.code(), "client.settings_unreadable");
    assert_eq!(
        std::fs::read_to_string(config.path().join("settings.json")).unwrap(),
        "{ this is not json",
        "a file AIKit could not understand is left exactly as it was"
    );
}
