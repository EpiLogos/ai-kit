//! Parse parity: every old root spelling dispatches to the same semantic
//! handler as its canonical group path.
//!
//! The convergence classification (§1) moved every AIKit root under eleven
//! everyday heads while keeping the old roots callable. "Keeping them
//! callable" is not enough — they must reach the SAME handler, with the same
//! argument envelope, or a compatibility spelling would be a second
//! implementation. This test pins that for all 76 roots:
//!
//! * both spellings parse;
//! * both parse to the same [`Command::route`] — the stable name of the
//!   dispatch arm — and therefore to the same handler, flags and JSON schema;
//! * the real-binary counterparts live in `tests/cli_binary.rs` and the
//!   every-command family, which run the read-only surface end to end.

use aikit_cli::cli::Cli;
use clap::Parser;

/// Parse `aikit <argv>` and return the route its command dispatches to.
fn route_of(argv: &[&str]) -> String {
    let cli = Cli::try_parse_from(std::iter::once("aikit").chain(argv.iter().copied()))
        .unwrap_or_else(|error| panic!("`aikit {}` must parse: {error}", argv.join(" ")));
    match cli.command {
        Some(command) => command.route().to_owned(),
        None => "bare".to_owned(),
    }
}

/// One old root, the canonical group path it is compatible with, and enough
/// argv for the parser to reach the leaf. Both spellings are asserted to
/// carry the same handler route.
const PARITY: &[(&[&str], &[&str])] = &[
    (&["source", "show", "src"], &["system", "source", "show", "src"]),
    (
        &["skill", "overlay", "show", "script/a/b"],
        &["praxis", "skill", "overlay", "show", "script/a/b"],
    ),
    (&["project", "list"], &["world", "project", "list"]),
    (&["init"], &["system", "init"]),
    (&["collate"], &["system", "collate"]),
    (&["adopt", "/tmp/foreign-root"], &["system", "adopt", "/tmp/foreign-root"]),
    (&["procedure", "list"], &["system", "procedure", "list"]),
    (&["profile", "diff", "p"], &["compose", "profile", "diff", "p"]),
    (
        &["harness-profile", "validate", "/tmp/h.toml"],
        &["system", "harness-profile", "validate", "/tmp/h.toml"],
    ),
    (&["set", "list"], &["praxis", "set", "list"]),
    (&["development-field"], &["world", "development-field"]),
    (
        &["worktree", "project", "--repo", "/tmp/r"],
        &["system", "worktree", "project", "--repo", "/tmp/r"],
    ),
    (&["flow", "preflight"], &["knowledge", "flow", "preflight"]),
    (
        &["wiki", "validate", "/tmp/w.json"],
        &["knowledge", "wiki", "validate", "/tmp/w.json"],
    ),
    (
        &["wiki-shape", "validate", "--file", "/tmp/w.json"],
        &["knowledge", "wiki-shape", "validate", "--file", "/tmp/w.json"],
    ),
    (
        &["wiki-construct", "inspect", "--file", "/tmp/f.json", "FRAME"],
        &["knowledge", "wiki-construct", "inspect", "--file", "/tmp/f.json", "FRAME"],
    ),
    (&["status"], &["world", "status"]),
    (&["family"], &["praxis", "family"]),
    (&["config-contribution"], &["system", "config-contribution"]),
    (
        &["config", "validate", "--setting", "ai-kit:s:k"],
        &["system", "config", "validate", "--setting", "ai-kit:s:k"],
    ),
    (&["diff"], &["compose", "diff"]),
    (&["doctor"], &["system", "doctor"]),
    (&["credential", "list"], &["system", "credential", "list"]),
    (&["run", "name"], &["praxis", "run", "name"]),
    (&["enable", "cap"], &["compose", "enable", "cap"]),
    (&["disable", "cap"], &["compose", "disable", "cap"]),
    (&["use", "profile"], &["compose", "use", "profile"]),
    (&["apply"], &["compose", "apply"]),
    (&["rollback"], &["compose", "rollback"]),
    (&["context", "current"], &["world", "context", "current"]),
    (&["continuity", "commands"], &["work", "continuity", "commands"]),
    (&["session", "list"], &["work", "session", "list"]),
    (&["session-space", "list"], &["work", "space", "list"]),
    (&["model-resolve"], &["compose", "model", "resolve"]),
    (&["model-catalogue", "show"], &["system", "model-catalogue", "show"]),
    (&["task", "list"], &["work", "task", "list"]),
    (&["inbox"], &["praxis", "inbox"]),
    (&["capture", "title"], &["praxis", "capture", "title"]),
    (&["promote", "candidate"], &["praxis", "promote", "candidate"]),
    (&["prune"], &["system", "generations", "prune"]),
    (&["bypass", "list"], &["system", "bypass", "list"]),
    (&["client", "status"], &["system", "client", "status"]),
    (
        &["client", "launch", "claude"],
        &["work", "client", "launch", "claude"],
    ),
    (&["harness", "disclose"], &["work", "harness", "disclose"]),
    (&["alias", "list"], &["compose", "alias", "list"]),
    (&["mux", "detect"], &["system", "mux", "detect"]),
    (
        &["hook", "dispatch", "claude", "PreToolUse"],
        &["system", "hook", "dispatch", "claude", "PreToolUse"],
    ),
    (&["capabilities", "list"], &["praxis", "capabilities", "list"]),
    (&["jobs"], &["work", "jobs"]),
    (&["method", "list"], &["praxis", "method", "list"]),
    (
        &["a2a", "card", "--participation-json", "{}", "--interface-url", "http://x"],
        &[
            "world",
            "a2a",
            "card",
            "--participation-json",
            "{}",
            "--interface-url",
            "http://x",
        ],
    ),
    (&["routine", "list"], &["praxis", "routine", "list"]),
    (
        &[
            "jev",
            "validate",
            "--request-file",
            "/tmp/q.json",
            "--response-file",
            "/tmp/a.json",
        ],
        &[
            "knowledge",
            "jev",
            "validate",
            "--request-file",
            "/tmp/q.json",
            "--response-file",
            "/tmp/a.json",
        ],
    ),
    (
        &["now-context", "status", "--config-file", "/tmp/n.toml"],
        &["world", "now-context", "status", "--config-file", "/tmp/n.toml"],
    ),
    (
        &[
            "factory",
            "start-work",
            "--state",
            "/tmp/s.json",
            "--request-file",
            "/tmp/req.json",
        ],
        &[
            "work",
            "factory",
            "start-work",
            "--state",
            "/tmp/s.json",
            "--request-file",
            "/tmp/req.json",
        ],
    ),
    (&["trust", "show", "script/a/b"], &["system", "trust", "show", "script/a/b"]),
    (&["recent"], &["history", "recent"]),
    (&["stats"], &["history", "stats"]),
    (&["log", "export"], &["history", "log", "export"]),
    (&["shell", "init", "bash"], &["system", "shell", "init", "bash"]),
    (&["unused"], &["praxis", "unused"]),
    (&["failures"], &["history", "failures"]),
    (&["bypasses"], &["history", "bypasses"]),
    (&["gateway", "status"], &["system", "gateway", "status"]),
    (&["whoami"], &["world", "whoami"]),
    (&["refocus"], &["world", "refocus"]),
    (
        &["inhabit", "--position", "central:position:x", "--reason", "why"],
        &["world", "inhabit", "--position", "central:position:x", "--reason", "why"],
    ),
];

/// Every one of the 76 old roots, including the ten that keep their root seat.
const OLD_ROOTS: &[&str] = &[
    "source", "skill", "project", "init", "collate", "adopt", "procedure", "profile",
    "harness-profile", "z", "set", "tree", "ui", "search", "development-field", "worktree",
    "knowledge", "flow", "wiki", "wiki-shape", "wiki-construct", "status", "system", "family",
    "config-contribution", "config", "explain", "history", "diff", "doctor", "credential", "run",
    "enable", "disable", "use", "apply", "rollback", "context", "continuity", "session",
    "session-space", "compose", "model-resolve", "model-catalogue", "task", "inbox", "capture",
    "promote", "prune", "bypass", "client", "harness", "alias", "mux", "hook", "capabilities",
    "jobs", "method", "praxis", "a2a", "routine", "jev", "now-context", "factory", "trust",
    "recent", "stats", "log", "shell", "unused", "failures", "bypasses", "gateway", "whoami",
    "refocus", "inhabit",
];

/// Enough argv for each root to parse. The point is parse-level reachability
/// of the handler, not execution.
fn argv_that_parses(root: &'static str) -> Vec<&'static str> {
    let argv: &[&str] = match root {
        "source" => &["source", "show", "src"],
        "skill" => &["skill", "overlay", "show", "script/a/b"],
        "project" => &["project", "list"],
        "adopt" => &["adopt", "/tmp/foreign-root"],
        "procedure" => &["procedure", "list"],
        "set" => &["set", "list"],
        "profile" => &["profile", "diff", "p"],
        "harness-profile" => &["harness-profile", "validate", "/tmp/h.toml"],
        "worktree" => &["worktree", "project", "--repo", "/tmp/r"],
        "flow" => &["flow", "preflight"],
        "wiki" => &["wiki", "validate", "/tmp/w.json"],
        "wiki-shape" => &["wiki-shape", "validate", "--file", "/tmp/w.json"],
        "wiki-construct" => &["wiki-construct", "inspect", "--file", "/tmp/f.json", "FRAME"],
        "config" => &["config", "validate", "--setting", "ai-kit:s:k"],
        "credential" => &["credential", "list"],
        "run" => &["run", "name"],
        "enable" => &["enable", "cap"],
        "disable" => &["disable", "cap"],
        "use" => &["use", "profile"],
        "context" => &["context", "current"],
        "continuity" => &["continuity", "commands"],
        "session" => &["session", "list"],
        "session-space" => &["session-space", "list"],
        "model-catalogue" => &["model-catalogue", "show"],
        "task" => &["task", "list"],
        "capture" => &["capture", "title"],
        "promote" => &["promote", "candidate"],
        "bypass" => &["bypass", "list"],
        "client" => &["client", "status"],
        "harness" => &["harness", "disclose"],
        "alias" => &["alias", "list"],
        "mux" => &["mux", "detect"],
        "hook" => &["hook", "dispatch", "claude", "PreToolUse"],
        "capabilities" => &["capabilities", "list"],
        "method" => &["method", "list"],
        "a2a" => &[
            "a2a",
            "card",
            "--participation-json",
            "{}",
            "--interface-url",
            "http://x",
        ],
        "routine" => &["routine", "list"],
        "jev" => &[
            "jev",
            "validate",
            "--request-file",
            "/tmp/q.json",
            "--response-file",
            "/tmp/a.json",
        ],
        "now-context" => &["now-context", "status", "--config-file", "/tmp/n.toml"],
        "factory" => &[
            "factory",
            "start-work",
            "--state",
            "/tmp/s.json",
            "--request-file",
            "/tmp/req.json",
        ],
        "trust" => &["trust", "show", "script/a/b"],
        "log" => &["log", "export"],
        "shell" => &["shell", "init", "bash"],
        "gateway" => &["gateway", "status"],
        "inhabit" => &["inhabit", "--position", "central:position:x", "--reason", "why"],
        "z" => &["z", "greet"],
        "search" => &["search", "greet"],
        "knowledge" => &["knowledge", "status"],
        "praxis" => &["praxis", "list"],
        "explain" => &["explain", "script/a/b"],
        other => &[other],
    };
    argv.to_vec()
}

#[test]
fn every_old_root_still_parses() {
    assert_eq!(OLD_ROOTS.len(), 76, "the classification covered 76 roots");
    for root in OLD_ROOTS {
        let argv = argv_that_parses(root);
        route_of(&argv);
    }
}

#[test]
fn every_old_root_and_its_group_path_reach_the_same_handler() {
    // 66 roots moved under a canonical group; `client` contributes two pairs
    // (status under System, launch under Work), hence 67 rows.
    assert_eq!(PARITY.len(), 67, "66 moved roots, client twice");
    for (old, canonical) in PARITY {
        let old_route = route_of(old);
        let canonical_route = route_of(canonical);
        assert_eq!(
            old_route, canonical_route,
            "`aikit {}` and `aikit {}` must dispatch to the same handler",
            old.join(" "),
            canonical.join(" ")
        );
        // And never to a stub.
        assert_ne!(old_route, "command.not_implemented");
    }
}

/// The ten roots that keep their root seat parse and route to their own
/// handlers (the everyday heads and the two compatibility shortcuts).
#[test]
fn the_retained_roots_parse_to_their_own_handlers() {
    for (root, expected) in [
        ("z", "cmd_z"),
        ("tree", "cmd_tree"),
        ("ui", "open_surface"),
        ("search", "cmd_search"),
        ("knowledge", "cmd_knowledge_status"), // reached as `knowledge status`; bare `knowledge` prints its head help
        ("system", "cmd_system"),
        ("compose", "cmd_compose"),
        ("explain", "cmd_explain"),
        ("history", "cmd_history"),
        ("praxis", "cmd_praxis_list"), // reached as `praxis list`; bare `praxis` prints its head help
    ] {
        // `z` requires at least one word: it resolves what you meant.
        // `knowledge` is a head with required members; its `status` reading is
        // the retained everyday route. `explain` requires its subject.
        let argv: &[&str] = match root {
            "z" => &["z", "greet"],
            "knowledge" => &["knowledge", "status"],
            "explain" => &["explain", "script/a/b"],
            "praxis" => &["praxis", "list"],
            other => &[other],
        };
        assert_eq!(route_of(argv), expected, "`aikit {root}` moved seat");
    }
}

/// `compose plan` is the same launch-plan computation as bare `compose`.
#[test]
fn compose_plan_is_bare_compose() {
    assert_eq!(route_of(&["compose"]), route_of(&["compose", "plan"]));
}

/// Bare `aikit` parses to no command: the palette in a TTY, bounded
/// orientation otherwise.
#[test]
fn bare_invocation_parses_to_no_command() {
    assert_eq!(route_of(&[]), "bare");
    assert_eq!(route_of(&["--json"]), "bare");
}

/// The `act` doorway binds exactly three handlers, and bare `act` is discovery.
#[test]
fn the_act_doorway_routes() {
    assert_eq!(route_of(&["act"]), "cmd_act_discover");
    assert_eq!(route_of(&["act", "discover"]), "cmd_act_discover");
    assert_eq!(route_of(&["act", "describe", "script/a/b"]), "cmd_act_describe");
    assert_eq!(
        route_of(&["act", "invoke", "script/a/b"]),
        "cmd_act_invoke"
    );
}
