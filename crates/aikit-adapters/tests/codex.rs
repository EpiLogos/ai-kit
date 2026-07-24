//! The Codex adapter, and the shared-tree problem it exists to be honest about.
//!
//! Codex discovers `.agents/skills` by walking from the working directory up to
//! the repository root. Two Codex sessions in the **same tree** therefore see the
//! **same skills** — there is no per-session flag that changes it.
//!
//! That single fact drives everything here. When a task has its own tree, AIKit
//! projects into it and the skills are genuinely per-task. When the task shares
//! the session's tree — which is the default, because a worktree costs a
//! checkout, a branch, disk and a teardown decision — writing a per-task
//! `.agents/skills` would silently change what a *sibling* task sees. So it does
//! not. It falls back, in a stated order, and says which fallback it took.
//!
//! The refusal at the end of that chain is the important one: a shared-tree
//! projection is possible, and it is available, but only to a caller that has
//! explicitly accepted the consequence.

mod common;

use common::*;

use std::path::Path;

use aikit_adapters::clients::codex::{CodexAdapter, SharedTreeStrategy};
use aikit_adapters::clients::ClientAdapter;
use aikit_core::context::Isolation;
use aikit_core::projection::{ActivationEffect, ResolvedContext, TargetAdapter};
use aikit_core::platform::TargetId;

/// A context with one project-stable skill and one session-only delta.
fn context(registry: &Path, isolation: Isolation) -> ResolvedContext {
    let stable = registry.join("skill/rust/code-review");
    let delta = registry.join("skill/rust/perf");
    write_payload_skill(&stable, "code-review", "Reviews Rust for correctness.");
    write_payload_skill(&delta, "perf", "Finds hot loops.");

    ContextBuilder::new()
        .isolation(isolation)
        .project_skill("skill/rust/code-review", "Reviews Rust.", &stable)
        .session_skill("skill/rust/perf", "Finds hot loops.", &delta)
        .build()
}

/// A context whose skills are all project-stable: nothing is session-only.
fn stable_only_context(registry: &Path, isolation: Isolation) -> ResolvedContext {
    let stable = registry.join("skill/rust/code-review");
    write_payload_skill(&stable, "code-review", "Reviews Rust for correctness.");

    ContextBuilder::new()
        .isolation(isolation)
        .project_skill("skill/rust/code-review", "Reviews Rust.", &stable)
        .build()
}

fn destinations(plan: &aikit_core::projection::ProjectionPlan) -> Vec<String> {
    plan.items
        .iter()
        .filter_map(|i| i.destination())
        .map(|p| p.display().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Branch 1: the task has its own tree
// ---------------------------------------------------------------------------

#[test]
fn an_isolated_task_gets_a_real_per_task_agents_skills_directory() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);

    let plan = CodexAdapter::new(tree.path()).plan(&context).unwrap();

    assert_eq!(plan.target, TargetId::codex());
    assert_eq!(
        destinations(&plan),
        vec![".agents/skills/code-review", ".agents/skills/perf"],
        "an isolated task gets everything, deltas included"
    );
    assert_eq!(plan.effect, ActivationEffect::LiveReloadExpected);
    assert!(
        plan.notes.iter().any(|n| n.contains("own working tree")),
        "the palette prints these verbatim: {:?}",
        plan.notes
    );
}

#[test]
fn a_dedicated_directory_counts_as_isolation_even_without_a_git_worktree() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Directory);

    let plan = CodexAdapter::new(tree.path()).plan(&context).unwrap();
    assert_eq!(plan.items.len(), 2);
    assert_eq!(plan.effect, ActivationEffect::LiveReloadExpected);
}

#[test]
fn the_projected_tree_for_an_isolated_codex_task_is_exactly_this() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);

    let plan = CodexAdapter::new(tree.path())
        .with_materialization(aikit_core::projection::MaterializationMode::Copy)
        .plan(&context)
        .unwrap();
    materialize(&plan.items, tree.path());

    assert_eq!(
        tree_of(&tree.path().join(".agents/skills")),
        vec![
            "code-review/".to_string(),
            "code-review/SKILL.md".to_string(),
            "code-review/references/".to_string(),
            "code-review/references/deep.md".to_string(),
            "code-review/scripts/".to_string(),
            "code-review/scripts/check.sh".to_string(),
            "perf/".to_string(),
            "perf/SKILL.md".to_string(),
            "perf/references/".to_string(),
            "perf/references/deep.md".to_string(),
            "perf/scripts/".to_string(),
            "perf/scripts/check.sh".to_string(),
        ]
    );
}

// ---------------------------------------------------------------------------
// Branch 2: shared tree, project-stable skills only (the default)
// ---------------------------------------------------------------------------

#[test]
fn a_shared_task_projects_only_the_project_stable_skills() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);

    let plan = CodexAdapter::new(tree.path()).plan(&context).unwrap();

    assert_eq!(
        destinations(&plan),
        vec![".agents/skills/code-review"],
        "the session-only delta must not be written into a tree a sibling task shares"
    );
}

#[test]
fn the_note_explains_the_fallback_in_words_a_person_can_act_on() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);

    let plan = CodexAdapter::new(tree.path()).plan(&context).unwrap();
    let notes = plan.notes.join("\n");

    assert!(notes.contains("shared"), "got: {notes}");
    assert!(
        notes.contains("perf"),
        "the note has to name what did not make it: {notes}"
    );
    assert!(
        notes.contains("--worktree") || notes.contains("worktree"),
        "and what the user could do about it: {notes}"
    );
}

#[test]
fn the_undeliverable_deltas_make_the_effect_brokered() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);

    let plan = CodexAdapter::new(tree.path()).plan(&context).unwrap();
    match &plan.effect {
        ActivationEffect::Brokered { reason } => {
            assert!(reason.contains("shared"), "got: {reason}")
        }
        other => panic!("session-only skills that were not projected are brokered, got {other:?}"),
    }
}

#[test]
fn a_shared_task_with_nothing_session_only_is_simply_in_effect() {
    // No delta, no dishonesty: the project-stable skills are the tree's normal
    // contents and every sibling task sees the same ones anyway.
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = stable_only_context(registry.path(), Isolation::Shared);

    let plan = CodexAdapter::new(tree.path()).plan(&context).unwrap();
    assert_eq!(destinations(&plan), vec![".agents/skills/code-review"]);
    match &plan.effect {
        ActivationEffect::Immediate { via } => assert!(via.contains("project")),
        other => panic!("expected an immediate effect, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Branch 3: broker everything
// ---------------------------------------------------------------------------

#[test]
fn brokering_everything_writes_nothing_into_the_shared_tree() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);

    let plan = CodexAdapter::new(tree.path())
        .with_strategy(SharedTreeStrategy::BrokerAll)
        .plan(&context)
        .unwrap();

    assert!(plan.is_empty(), "not one file: {:?}", destinations(&plan));
    assert!(matches!(plan.effect, ActivationEffect::Brokered { .. }));
    assert!(plan.notes.iter().any(|n| n.contains("aikit capabilities")));
}

#[test]
fn brokering_is_available_even_when_the_task_is_isolated() {
    // Isolation makes a native projection *possible*, not mandatory.
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);

    let plan = CodexAdapter::new(tree.path())
        .with_strategy(SharedTreeStrategy::BrokerAll)
        .plan(&context)
        .unwrap();
    assert!(plan.is_empty());
}

// ---------------------------------------------------------------------------
// Branch 4 and the refusal: an explicitly accepted shared projection
// ---------------------------------------------------------------------------

#[test]
fn a_shared_projection_is_refused_unless_it_was_explicitly_accepted() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);

    let error = CodexAdapter::new(tree.path())
        .with_strategy(SharedTreeStrategy::SharedProjection)
        .plan(&context)
        .unwrap_err();

    assert_eq!(error.code(), "projection.shared_tree_conflict");
    assert!(
        error.message().contains("sibling") || error.message().contains("other tasks"),
        "the refusal has to say what the consequence would be: {}",
        error.message()
    );
    assert_eq!(
        error.details().get("isolation").map(String::as_str),
        Some("shared")
    );
    assert!(
        !tree.path().join(".agents").exists(),
        "a refused plan must not have written anything"
    );
}

#[test]
fn an_accepted_shared_projection_writes_everything_and_says_who_asked_for_it() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);

    let plan = CodexAdapter::new(tree.path())
        .with_strategy(SharedTreeStrategy::SharedProjection)
        .accepting_shared_projection(true)
        .plan(&context)
        .unwrap();

    assert_eq!(
        destinations(&plan),
        vec![".agents/skills/code-review", ".agents/skills/perf"]
    );
    let notes = plan.notes.join("\n");
    assert!(notes.contains("accepted"), "got: {notes}");
    assert!(
        notes.contains("sibling") || notes.contains("other tasks"),
        "an accepted risk is still a risk worth restating: {notes}"
    );
    assert_eq!(plan.effect, ActivationEffect::LiveReloadExpected);
}

#[test]
fn accepting_a_shared_projection_changes_nothing_for_an_isolated_task() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);

    let with_flag = CodexAdapter::new(tree.path())
        .accepting_shared_projection(true)
        .plan(&context)
        .unwrap();
    let without = CodexAdapter::new(tree.path()).plan(&context).unwrap();

    assert_eq!(with_flag.digest(), without.digest());
}

// ---------------------------------------------------------------------------
// What Codex must never be given
// ---------------------------------------------------------------------------

#[test]
fn no_synthetic_home_is_ever_invented_for_the_client() {
    // A fake HOME would silently redirect git config, ssh config and
    // credentials. It is the kind of shortcut that works until it destroys
    // something, so it is tested for rather than merely avoided.
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);
    let adapter = CodexAdapter::new(tree.path());

    let plan = adapter.plan(&context).unwrap();
    for item in &plan.items {
        let rendered = format!("{item:?}");
        assert!(!rendered.contains("HOME"), "got: {rendered}");
    }
    assert!(
        !adapter.launch_command(&context).iter().any(|a| a.contains("HOME")),
        "got: {:?}",
        adapter.launch_command(&context)
    );
}

#[test]
fn the_capabilities_say_plainly_that_isolation_needs_a_tree() {
    let tree = tempfile::tempdir().unwrap();
    let caps = CodexAdapter::new(tree.path()).capabilities();

    assert!(caps.isolated_per_context);
    assert!(caps.requires_isolated_tree_for_isolation);
    assert!(!caps.can_isolate(Isolation::Shared));
    assert!(caps.can_isolate(Isolation::Worktree));
    assert!(caps
        .fallback_reason(Isolation::Shared)
        .unwrap()
        .contains("shared"));
}

#[test]
fn the_launch_command_relies_on_discovery_rather_than_on_a_flag_that_does_not_exist() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);

    assert_eq!(
        CodexAdapter::new(tree.path()).launch_command(&context),
        vec!["codex".to_string()],
        "Codex finds .agents/skills by walking up from the cwd; there is no directory flag \
         to pass, and inventing one would produce a command that fails"
    );
}

// ---------------------------------------------------------------------------
// Activation effect and installation
// ---------------------------------------------------------------------------

#[test]
fn an_unchanged_isolated_projection_is_immediate() {
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Worktree);
    let adapter = CodexAdapter::new(tree.path());

    let first = adapter.plan(&context).unwrap();
    let second = adapter.plan(&context).unwrap();
    assert!(matches!(
        adapter.activation_effect(Some(&first), &second),
        ActivationEffect::Immediate { .. }
    ));
}

#[test]
fn a_fallback_effect_survives_a_no_op_apply_rather_than_becoming_immediate() {
    // Nothing changed on disk, but the session deltas are still not in Codex.
    // Reporting "immediate" here would tell the user their toggle took effect.
    let registry = tempfile::tempdir().unwrap();
    let tree = tempfile::tempdir().unwrap();
    let context = context(registry.path(), Isolation::Shared);
    let adapter = CodexAdapter::new(tree.path());

    let first = adapter.plan(&context).unwrap();
    let second = adapter.plan(&context).unwrap();
    assert!(
        matches!(
            adapter.activation_effect(Some(&first), &second),
            ActivationEffect::Brokered { .. }
        ),
        "got {:?}",
        adapter.activation_effect(Some(&first), &second)
    );
}

#[test]
fn installing_writes_an_aikit_owned_dispatcher_file_and_is_idempotent() {
    let tree = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let adapter = CodexAdapter::new(tree.path());

    let items = adapter.install(config.path()).unwrap();
    materialize(&items, config.path());
    let first = std::fs::read_to_string(config.path().join("hooks/aikit.toml")).unwrap();

    materialize(&adapter.install(config.path()).unwrap(), config.path());
    let second = std::fs::read_to_string(config.path().join("hooks/aikit.toml")).unwrap();

    assert_eq!(first, second);
    for event in [
        "PreToolUse",
        "PostToolUse",
        "UserPromptSubmit",
        "SessionStart",
        "Stop",
        "SessionEnd",
    ] {
        assert!(
            first.contains(&format!("aikit hook dispatch codex {event}")),
            "missing {event} in {first}"
        );
    }
}

#[test]
fn installing_does_not_touch_the_users_own_codex_configuration() {
    let tree = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let own = "# my notes\nmodel = \"o3\"\n";
    std::fs::write(config.path().join("config.toml"), own).unwrap();

    materialize(
        &CodexAdapter::new(tree.path()).install(config.path()).unwrap(),
        config.path(),
    );

    assert_eq!(
        std::fs::read_to_string(config.path().join("config.toml")).unwrap(),
        own,
        "AIKit's entries live in their own file precisely so a hand-written config with \
         comments in it is never rewritten"
    );
}
