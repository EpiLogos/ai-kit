//! The zcode hook-wrapper grammar: a configuration document that spells hooks
//! as a top-level block
//! `{ hooks: { enabled: true, events: { <Event>: [ { matcher?, hooks: [...] } ] } } }`.
//!
//! Configuration-file hooks are inert without `enabled: true`, so the merged
//! document carries the flag — unless the user disabled hooks explicitly, in
//! which case the merge refuses rather than silently re-enabling the user's
//! own hooks alongside AIKit's. AIKit's entries never carry a matcher: an
//! omitted matcher matches everything, and one AIKit invented would narrow
//! what the dispatcher sees. The sweep, the pruning and the reporting live
//! once in the engine (`super::merge_hook_entries`).

use aikit_core::hooks::HookEventKind;

use super::{
    LayerMergeError, MatcherPolicy, MergeReport, document_for_merging, hooks_block_of,
    merge_hook_entries,
};

/// Merge AIKit's dispatch entries into a zcode configuration document.
pub fn zcode_hook_wrapper(
    existing: &serde_json::Value,
    events: &[(HookEventKind, String)],
    client: &str,
) -> Result<(serde_json::Value, MergeReport), LayerMergeError> {
    let mut document = document_for_merging(
        existing,
        "the existing zcode configuration is not a JSON object",
    )?;
    let hooks = hooks_block_of(&mut document)?;

    if hooks.get("enabled") == Some(&serde_json::Value::Bool(false)) {
        return Err(LayerMergeError::new(
            "client.hooks_disabled_by_user",
            "zcode's configuration-file hooks are explicitly disabled \
             (`hooks.enabled: false`); enabling the runner would also activate hooks the \
             user kept disabled, so AIKit refuses instead of flipping the flag",
        ));
    }
    hooks.insert("enabled".to_string(), serde_json::Value::Bool(true));

    let events_map = hooks
        .entry("events".to_string())
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            LayerMergeError::new(
                "client.settings_unreadable",
                "the existing `hooks.events` value is not an object",
            )
        })?;

    let report = merge_hook_entries(
        events_map,
        events,
        client,
        "hooks.events",
        MatcherPolicy::Omitted,
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
    fn a_null_document_merges_as_if_the_configuration_were_fresh() {
        let (merged, report) = zcode_hook_wrapper(
            &serde_json::Value::Null,
            &events(&[("stop", "Stop")]),
            "zcode",
        )
        .unwrap();

        assert_eq!(
            merged,
            serde_json::json!({ "hooks": { "enabled": true, "events": { "Stop": [ { "hooks": [ { "type": "command", "command": "aikit hook dispatch zcode Stop" } ] } ] } } })
        );
        assert_eq!(report.added, vec!["Stop".to_string()]);
    }

    #[test]
    fn a_document_that_is_not_an_object_refuses_with_the_configuration_wording() {
        let error = zcode_hook_wrapper(
            &serde_json::json!("just a string"),
            &events(&[("stop", "Stop")]),
            "zcode",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert_eq!(
            error.message(),
            "the existing zcode configuration is not a JSON object"
        );
    }

    #[test]
    fn a_hooks_events_value_that_is_not_an_object_refuses() {
        let error = zcode_hook_wrapper(
            &serde_json::json!({ "hooks": { "enabled": true, "events": 5 } }),
            &events(&[("stop", "Stop")]),
            "zcode",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert_eq!(
            error.message(),
            "the existing `hooks.events` value is not an object"
        );
    }

    #[test]
    fn an_event_whose_existing_value_is_not_an_array_refuses_naming_the_full_path() {
        let error = zcode_hook_wrapper(
            &serde_json::json!({ "hooks": { "events": { "Stop": 5 } } }),
            &events(&[("stop", "Stop")]),
            "zcode",
        )
        .unwrap_err();

        assert_eq!(error.code(), "client.settings_unreadable");
        assert_eq!(
            error.message(),
            "the existing `hooks.events.Stop` value is not an array"
        );
    }

    #[test]
    fn the_report_names_a_replaced_stale_entry_and_a_pruned_event() {
        let existing = serde_json::json!({
            "hooks": {
                "enabled": true,
                "events": {
                    "Stop": [ { "hooks": [ { "type": "command", "command": "aikit hook dispatch zcode Stopp" } ] } ],
                    "SessionStart": [ { "hooks": [ { "type": "command", "command": "mine" } ] } ]
                }
            }
        });
        let (_, report) = zcode_hook_wrapper(
            &existing,
            &events(&[("session-start", "SessionStart")]),
            "zcode",
        )
        .unwrap();

        assert_eq!(
            report,
            MergeReport {
                added: vec!["SessionStart".to_string()],
                replaced: vec![],
                removed: vec!["Stop".to_string()],
                kept_foreign: vec!["SessionStart".to_string()],
            }
        );
    }

    #[test]
    fn the_user_disabled_refusal_names_the_flag_and_the_reason() {
        let existing = serde_json::json!({
            "hooks": { "enabled": false, "events": { "Stop": [ { "hooks": [ { "type": "command", "command": "mine" } ] } ] } }
        });
        let error =
            zcode_hook_wrapper(&existing, &events(&[("stop", "Stop")]), "zcode").unwrap_err();

        assert_eq!(error.code(), "client.hooks_disabled_by_user");
        assert!(
            error.message().contains("`hooks.enabled: false`"),
            "the refusal must name the flag it will not flip: {error}"
        );
    }
}
