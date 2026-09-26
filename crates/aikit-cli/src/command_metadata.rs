//! The generated command reference: `aikit system commands --json`.
//!
//! The tree is produced by walking the actual clap parser — the same table
//! [`Cli`] parses from — so the reference cannot drift from what parses. It is
//! the complete surface: everyday heads, operator depth, protocol aliases and
//! the compatibility roots, each with its help text and flags. Group members
//! and their old root spellings both appear; the parse-parity test pins the
//! two spellings to the same handler.

use clap::CommandFactory;

use crate::cli::Cli;

/// The complete command tree of this binary, generated from the parser.
pub fn command_tree() -> serde_json::Value {
    let mut command = Cli::command();
    command.build();
    walk(&command)
}

fn walk(command: &clap::Command) -> serde_json::Value {
    let mut node = serde_json::json!({
        "name": command.get_name(),
        "about": command.get_about().map(|about| about.to_string()),
    });
    let mut aliases: Vec<String> = command
        .get_visible_aliases()
        .map(str::to_owned)
        .collect();
    aliases.sort();
    if !aliases.is_empty() {
        node["aliases"] = serde_json::Value::from(aliases);
    }

    let flags: Vec<serde_json::Value> = command
        .get_arguments()
        .filter(|argument| !argument.is_positional() && argument.get_id() != "help")
        .map(|argument| {
            let mut flag = serde_json::json!({
                "long": argument.get_long(),
            });
            if let Some(short) = argument.get_short() {
                flag["short"] = serde_json::Value::from(short.to_string());
            }
            if let Some(value_name) = argument.get_value_names().and_then(|names| names.first()) {
                flag["value"] = serde_json::Value::from(value_name.to_string());
            }
            if argument.is_global_set() {
                flag["global"] = serde_json::Value::from(true);
            }
            if let Some(help) = argument.get_help() {
                flag["help"] = serde_json::Value::from(help.to_string());
            }
            flag
        })
        .collect();
    if !flags.is_empty() {
        node["flags"] = serde_json::Value::from(flags);
    }

    let positionals: Vec<serde_json::Value> = command
        .get_positionals()
        .map(|argument| {
            let mut positional = serde_json::json!({
                "name": argument.get_id().to_string(),
            });
            if let Some(value_name) = argument.get_value_names().and_then(|names| names.first()) {
                positional["value"] = serde_json::Value::from(value_name.to_string());
            }
            if let Some(help) = argument.get_help() {
                positional["help"] = serde_json::Value::from(help.to_string());
            }
            positional
        })
        .collect();
    if !positionals.is_empty() {
        node["positionals"] = serde_json::Value::from(positionals);
    }

    let mut subcommands: Vec<serde_json::Value> =
        command.get_subcommands().map(walk).collect();
    if !subcommands.is_empty() {
        subcommands.sort_by(|left, right| {
            left["name"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["name"].as_str().unwrap_or_default())
        });
        node["subcommands"] = serde_json::Value::from(subcommands);
        node["subcommand_required"] = serde_json::Value::from(command.is_subcommand_required_set());
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference is the parser's own table, so the two group spellings of
    /// one command must both appear: the canonical member under its group and
    /// the old root at the top level.
    #[test]
    fn the_reference_contains_the_canonical_groups_and_the_old_roots() {
        let tree = command_tree();
        assert_eq!(tree["name"], "aikit");

        let names: Vec<String> = tree["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["name"].as_str().unwrap_or_default().to_owned())
            .collect();

        for head in [
            "world", "search", "act", "compose", "work", "knowledge", "praxis", "history",
            "system", "explain", "ui",
        ] {
            assert!(names.contains(&head.to_owned()), "missing everyday head {head}");
        }
        for root in [
            "status", "whoami", "refocus", "source", "init", "config", "session-space", "run",
            "inhabit",
        ] {
            assert!(names.contains(&root.to_owned()), "missing old root {root}");
        }

        let group = |name: &str| {
            tree["subcommands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["name"] == name)
                .unwrap_or_else(|| panic!("group {name} absent"))
                .clone()
        };
        let member_names = |node: &serde_json::Value| -> Vec<String> {
            node["subcommands"]
                .as_array()
                .map(|members| {
                    members
                        .iter()
                        .map(|member| member["name"].as_str().unwrap_or_default().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };

        // world: orientation and binding.
        let world = member_names(&group("world"));
        for member in [
            "project", "status", "context", "development-field", "a2a", "now-context", "whoami",
            "refocus", "inhabit",
        ] {
            assert!(world.contains(&member.to_owned()), "world lacks {member}");
        }
        // work: entry into real work, including the folded space forward.
        let work = member_names(&group("work"));
        for member in ["session", "space", "task", "jobs", "harness", "factory", "client"] {
            assert!(work.contains(&member.to_owned()), "work lacks {member}");
        }
        // system: the operator family, the folded generations group and the
        // generated reference itself.
        let system = member_names(&group("system"));
        for member in [
            "source", "init", "collate", "adopt", "procedure", "config", "config-contribution",
            "doctor", "credential", "client", "mux", "hook", "model-catalogue", "trust", "gateway",
            "shell", "generations", "commands",
        ] {
            assert!(system.contains(&member.to_owned()), "system lacks {member}");
        }
        let generations = group("system")["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == "generations")
            .cloned()
            .unwrap();
        assert!(member_names(&generations).contains(&"prune".to_owned()));

        // compose, knowledge, praxis, history: grouped members.
        let compose = member_names(&group("compose"));
        for member in ["plan", "profile", "diff", "enable", "disable", "use", "apply", "rollback", "alias", "model"] {
            assert!(compose.contains(&member.to_owned()), "compose lacks {member}");
        }
        let knowledge = member_names(&group("knowledge"));
        for member in ["flow", "wiki", "wiki-shape", "wiki-construct", "jev"] {
            assert!(knowledge.contains(&member.to_owned()), "knowledge lacks {member}");
        }
        let praxis = member_names(&group("praxis"));
        for member in ["skill", "set", "family", "run", "inbox", "capture", "promote", "capabilities", "method", "routine", "unused"] {
            assert!(praxis.contains(&member.to_owned()), "praxis lacks {member}");
        }
        let history = member_names(&group("history"));
        for member in ["recent", "stats", "log", "failures", "bypasses"] {
            assert!(history.contains(&member.to_owned()), "history lacks {member}");
        }
        // act: the bounded doorway.
        let act = member_names(&group("act"));
        for member in ["describe", "invoke", "discover"] {
            assert!(act.contains(&member.to_owned()), "act lacks {member}");
        }
    }
}
