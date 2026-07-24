//! The clap tree is part of the contract too: the command set in
//! ARCHITECTURE.md's §12 discussion and the task-brief must all parse, every
//! substantive command must accept `--json`, and — the load-bearing one —
//! `task spawn` must default to a **shared** working tree, with `--worktree` the
//! only thing that asks for a git worktree.

use aikit_cli::cli::{BypassSub, Cli, Command, ContextSub, HookSub, Isolation, TaskSub};
use clap::Parser;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|e| panic!("`{args:?}` should parse: {e}"))
}

#[test]
fn task_spawn_defaults_to_a_shared_tree_not_a_worktree() {
    let cli = parse(&["aikit", "task", "spawn", "review"]);
    let Some(Command::Task(task)) = cli.command else {
        panic!("expected a task command");
    };
    let TaskSub::Spawn(spawn) = task.command else {
        panic!("expected task spawn");
    };
    assert_eq!(spawn.name, "review");
    assert_eq!(spawn.agent, "claude");
    assert_eq!(
        spawn.isolation(),
        Isolation::Shared,
        "the default is shared; a worktree is opt-in"
    );
}

#[test]
fn task_spawn_worktree_is_opt_in() {
    let cli = parse(&["aikit", "task", "spawn", "review", "--worktree"]);
    let Some(Command::Task(task)) = cli.command else {
        panic!("expected a task command");
    };
    let TaskSub::Spawn(spawn) = task.command else {
        panic!("expected task spawn");
    };
    assert_eq!(spawn.isolation(), Isolation::Worktree);
}

#[test]
fn task_spawn_directory_and_shared_select_their_isolation() {
    let dir = parse(&["aikit", "task", "spawn", "x", "--directory"]);
    let Some(Command::Task(t)) = dir.command else { unreachable!() };
    let TaskSub::Spawn(s) = t.command else { unreachable!() };
    assert_eq!(s.isolation(), Isolation::Directory);

    let shared = parse(&["aikit", "task", "spawn", "x", "--shared"]);
    let Some(Command::Task(t)) = shared.command else { unreachable!() };
    let TaskSub::Spawn(s) = t.command else { unreachable!() };
    assert_eq!(s.isolation(), Isolation::Shared);
}

#[test]
fn conflicting_isolation_flags_are_a_usage_error() {
    let result = Cli::try_parse_from(["aikit", "task", "spawn", "x", "--worktree", "--directory"]);
    assert!(result.is_err(), "two isolation modes at once must be rejected");
}

#[test]
fn json_is_accepted_on_substantive_commands() {
    for args in [
        vec!["aikit", "search", "test", "--json"],
        vec!["aikit", "status", "--json"],
        vec!["aikit", "--json", "status"],
        vec!["aikit", "explain", "skill/rust/review", "--json"],
        vec!["aikit", "apply", "--json"],
        vec!["aikit", "context", "current", "--json"],
        vec!["aikit", "session", "list", "--json"],
    ] {
        let cli = parse(&args);
        assert!(cli.json, "`{args:?}` should set --json");
    }
}

#[test]
fn no_subcommand_means_open_the_palette() {
    let cli = parse(&["aikit"]);
    assert!(cli.command.is_none());
}

#[test]
fn the_nested_command_groups_all_parse() {
    let cli = parse(&["aikit", "context", "reset"]);
    assert!(matches!(cli.command, Some(Command::Context(c)) if matches!(c.command, ContextSub::Reset(_))));

    let cli = parse(&["aikit", "bypass", "issue", "--reason", "debugging a flake"]);
    let Some(Command::Bypass(b)) = cli.command else { unreachable!() };
    let BypassSub::Issue(issue) = b.command else { unreachable!() };
    assert_eq!(issue.reason.as_deref(), Some("debugging a flake"));

    let cli = parse(&["aikit", "hook", "dispatch", "claude", "PreToolUse"]);
    let Some(Command::Hook(h)) = cli.command else { unreachable!() };
    let HookSub::Dispatch(d) = h.command;
    assert_eq!(d.client, "claude");
    assert_eq!(d.event, "PreToolUse");
}
