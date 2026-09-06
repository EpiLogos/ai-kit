//! The Claude-grammar hooks map, shared by the harnesses whose settings spell
//! hooks as `{ <Event>: [ { matcher?, hooks: [...] } ] }` (claude-code's
//! settings.json; codex's per-project `.codex/hooks.json` reads the same
//! shape, as observed on real installs).
//!
//! Everything that is not AIKit's is preserved: unrelated top-level keys,
//! foreign events, and the user's own hooks inside events AIKit also uses. A
//! previous AIKit entry is *not* preserved, because leaving one behind next to
//! a new one would fire the whole chain twice.

use aikit_core::hooks::HookEventKind;
use aikit_core::{AikitError, Result};

/// Whether tool-carrying events carry a `matcher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherPolicy {
    /// claude-code matches tool names against a glob; `*` is its match-all.
    StarForTools,
    /// codex's hooks.json matches by regex and an omitted matcher matches
    /// everything; a matcher AIKit invented would narrow what the dispatcher
    /// sees.
    Omitted,
}

/// The command AIKit installs for one event.
pub fn dispatch_command(client: &str, event: &HookEventKind) -> String {
    format!("aikit hook dispatch {client} {event}")
}

/// Is this an AIKit dispatcher entry — including a stale one from an older
/// install that spelled the event differently?
fn is_aikit_entry(client: &str, command: &str) -> bool {
    command
        .trim()
        .starts_with(&format!("aikit hook dispatch {client}"))
}

pub fn merge_hook_map_entries(
    existing: Option<&str>,
    events: &[(HookEventKind, String)],
    client: &str,
    matchers: MatcherPolicy,
) -> Result<String> {
    let mut document: serde_json::Value = match existing {
        None => serde_json::json!({}),
        Some(raw) if raw.trim().is_empty() => serde_json::json!({}),
        Some(raw) => serde_json::from_str(raw).map_err(|e| {
            AikitError::new(
                "client.settings_unreadable",
                format!(
                    "the existing settings are not valid JSON ({e}); AIKit will not overwrite \
                     a file it cannot read"
                ),
            )
        })?,
    };

    if !document.is_object() {
        return Err(AikitError::new(
            "client.settings_unreadable",
            "the existing settings are not a JSON object",
        ));
    }

    let hooks = document
        .as_object_mut()
        .and_then(|o| {
            o.entry("hooks")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .ok_or_else(|| {
            AikitError::new(
                "client.settings_unreadable",
                "the existing `hooks` value is not an object",
            )
        })?;

    // A previous install may have written an entry under an event AIKit no
    // longer dispatches, or under a misspelling. Sweep those first, everywhere.
    for entries in hooks.values_mut() {
        if let Some(matcher_entries) = entries.as_array_mut() {
            for matcher in matcher_entries.iter_mut() {
                if let Some(list) = matcher.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    list.retain(|hook| {
                        !hook
                            .get("command")
                            .and_then(|c| c.as_str())
                            .is_some_and(|command| is_aikit_entry(client, command))
                    });
                }
            }
            matcher_entries.retain(|matcher| {
                matcher
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .is_none_or(|list| !list.is_empty())
            });
        }
    }

    for (event, native_name) in events {
        let mut entry = serde_json::Map::new();
        if matchers == MatcherPolicy::StarForTools && event.carries_tool_name() {
            entry.insert("matcher".to_string(), serde_json::json!("*"));
        }
        entry.insert(
            "hooks".to_string(),
            serde_json::json!([{
                "type": "command",
                "command": dispatch_command(client, event),
            }]),
        );

        let list = hooks
            .entry(native_name.clone())
            .or_insert_with(|| serde_json::json!([]));
        match list.as_array_mut() {
            Some(array) => array.push(serde_json::Value::Object(entry)),
            None => {
                return Err(AikitError::new(
                    "client.settings_unreadable",
                    format!("the existing `hooks.{event}` value is not an array"),
                ))
            }
        }
    }

    // Remove any event key that ended up empty after the sweep, so an old
    // install does not leave `"PreCompact": []` behind forever.
    hooks.retain(|_, entries| entries.as_array().is_none_or(|a| !a.is_empty()));

    let mut rendered = serde_json::to_string_pretty(&document).map_err(|e| {
        AikitError::new(
            "client.settings_unreadable",
            format!("could not render the merged settings: {e}"),
        )
    })?;
    rendered.push('\n');
    Ok(rendered)
}
