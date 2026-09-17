//! The Claude hook-map grammar: settings that spell hooks as
//! `{ hooks: { <Event>: [ { matcher?, hooks: [...] } ] } }` (claude-code's
//! settings.json; codex's per-project `.codex/hooks.json` reads the same
//! shape, as observed on real installs).
//!
//! Everything that is not AIKit's is preserved: unrelated top-level keys,
//! foreign events, and the user's own hooks inside events AIKit also uses. A
//! previous AIKit entry is *not* preserved, because leaving one behind next to
//! a new one would fire the whole chain twice — that law, and the sweep and
//! pruning that enforce it, live once in the engine (`super::merge_hook_entries`).

use aikit_core::hooks::HookEventKind;

use super::{
    LayerMergeError, MatcherPolicy, MergeReport, document_for_merging, hooks_block_of,
    merge_hook_entries,
};

/// Merge AIKit's dispatch entries into a Claude-grammar settings document.
pub fn claude_hook_map(
    existing: &serde_json::Value,
    events: &[(HookEventKind, String)],
    matchers: MatcherPolicy,
    client: &str,
) -> Result<(serde_json::Value, MergeReport), LayerMergeError> {
    let mut document =
        document_for_merging(existing, "the existing settings are not a JSON object")?;
    let report = merge_hook_entries(
        hooks_block_of(&mut document)?,
        events,
        client,
        "hooks",
        matchers,
    )?;
    Ok((document, report))
}

#[cfg(test)]
mod tests {
    use aikit_core::hooks::HookEventKind;

    use super::*;

    fn events(pairs: &[(&str, &str)]) -> Vec<(HookEventKind, String)> {
        pairs
            .iter()
            .map(|(event, native)| (HookEventKind::parse(event), native.to_string()))
            .collect()
    }

    #[test]
    fn a_null_document_merges_as_if_the_file_were_fresh() {
        let (merged, report) = claude_hook_map(
            &serde_json::Value::Null,
            &events(&[("stop", "Stop")]),
            MatcherPolicy::StarForTools,
            "claude",
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "aikit hook dispatch claude Stop" } ] } ] } })
        );
        assert_eq!(report.added, vec!["Stop".to_string()]);
    }

    #[test]
    fn a_document_that_is_not_an_object_refuses_with_the_settings_wording() {
        let error = claude_hook_map(
            &serde_json::json!([1]),
            &events(&[("stop", "Stop")]),
            MatcherPolicy::StarForTools,
            "claude",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert_eq!(
            error.message(),
            "the existing settings are not a JSON object"
        );
    }

    #[test]
    fn a_hooks_block_that_is_not_an_object_refuses() {
        let error = claude_hook_map(
            &serde_json::json!({ "hooks": "nope" }),
            &events(&[("stop", "Stop")]),
            MatcherPolicy::StarForTools,
            "claude",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert_eq!(
            error.message(),
            "the existing `hooks` value is not an object"
        );
    }

    #[test]
    fn an_event_whose_existing_value_is_not_an_array_refuses_naming_the_event() {
        let error = claude_hook_map(
            &serde_json::json!({ "hooks": { "Stop": 5 } }),
            &events(&[("stop", "Stop")]),
            MatcherPolicy::StarForTools,
            "claude",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert_eq!(
            error.message(),
            "the existing `hooks.Stop` value is not an array"
        );
    }

    #[test]
    fn the_report_names_a_replaced_stale_entry_and_a_pruned_event() {
        let existing = serde_json::json!({
            "hooks": {
                "Stop": [ { "hooks": [ { "type": "command", "command": "aikit hook dispatch claude Stopp" } ] } ],
                "SessionStart": [ { "hooks": [ { "type": "command", "command": "aikit hook dispatch claude SessionStart" } ] } ]
            }
        });
        let (_, report) = claude_hook_map(
            &existing,
            &events(&[("stop", "Stop")]),
            MatcherPolicy::StarForTools,
            "claude",
        )
        .unwrap();

        assert_eq!(
            report,
            MergeReport {
                added: vec![],
                replaced: vec!["Stop".to_string()],
                removed: vec!["SessionStart".to_string()],
                kept_foreign: vec![],
            }
        );
    }

    #[test]
    fn the_report_names_events_that_kept_foreign_hooks() {
        let existing = serde_json::json!({
            "hooks": {
                "PreCompact": [ { "hooks": [ { "type": "command", "command": "my-own-compactor" } ] } ],
                "PreToolUse": [ { "matcher": "Bash", "hooks": [ { "type": "command", "command": "my-own-guard" } ] } ]
            }
        });
        let (_, report) = claude_hook_map(
            &existing,
            &events(&[("pre-tool-use", "PreToolUse")]),
            MatcherPolicy::StarForTools,
            "claude",
        )
        .unwrap();

        assert_eq!(
            report,
            MergeReport {
                added: vec!["PreToolUse".to_string()],
                replaced: vec![],
                removed: vec![],
                kept_foreign: vec!["PreCompact".to_string(), "PreToolUse".to_string()],
            }
        );
    }

    #[test]
    fn the_omitted_policy_omits_every_matcher_even_on_tool_events() {
        let (merged, _) = claude_hook_map(
            &serde_json::json!({}),
            &events(&[("pre-tool-use", "PreToolUse")]),
            MatcherPolicy::Omitted,
            "codex",
        )
        .unwrap();

        assert!(
            merged["hooks"]["PreToolUse"][0].get("matcher").is_none(),
            "a matcher AIKit invented would narrow what the dispatcher sees"
        );
    }
}
