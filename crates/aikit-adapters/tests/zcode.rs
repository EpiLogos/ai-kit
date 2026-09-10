//! The ZCode adapter.
//!
//! ZCode reads hook dispatcher entries from the `hooks` block of its
//! configuration JSON, and configuration-file hooks are inert until
//! `hooks.enabled` is true — so the hardest assertions here are about the seam
//! staying exactly the descriptor's: the enabled flag, the events map, and
//! every foreign key the user already owns.

mod common;

use common::*;

use std::path::Path;

use aikit_adapters::actuation_harness_capability::{CapabilityOutcome, HarnessCapability};
use aikit_adapters::clients::zcode::ZcodeAdapter;
use aikit_adapters::clients::ClientAdapter;

fn adapter() -> ZcodeAdapter {
    ZcodeAdapter::new().with_capability(zcode_capability())
}

/// Actuation's declared zcode capability (catalog r4), as a test fixture: five
/// native events that map onto AIKit's dispatch boundaries, and two customs —
/// PermissionRequest and PostToolUseFailure — that do not.
fn zcode_capability() -> HarnessCapability {
    serde_json::from_value(serde_json::json!({
        "schema": "actuation.harness-capability/v1",
        "document": "capability",
        "harness_slug": "zcode",
        "native_events": [
            { "event": "session-start", "native_name": "SessionStart", "transport": "config-json-hooks-map", "can_block": false, "context_channel": "stdout-additional-context" },
            { "event": "user-prompt-submit", "native_name": "UserPromptSubmit", "transport": "config-json-hooks-map", "can_block": true, "context_channel": "stdout-additional-context" },
            { "event": "pre-tool-use", "native_name": "PreToolUse", "transport": "config-json-hooks-map", "can_block": true, "context_channel": "stdout-additional-context" },
            { "event": "post-tool-use", "native_name": "PostToolUse", "transport": "config-json-hooks-map", "can_block": false, "context_channel": "stdout-additional-context" },
            { "event": "stop", "native_name": "Stop", "transport": "config-json-hooks-map", "can_block": true, "context_channel": "none" },
            { "event": "custom", "native_name": "PermissionRequest", "transport": "config-json-hooks-map", "can_block": true, "context_channel": "exit-code-payload" },
            { "event": "custom", "native_name": "PostToolUseFailure", "transport": "config-json-hooks-map", "can_block": false, "context_channel": "none" }
        ],
        "injection_channel": { "kind": "stdout-additional-context", "mechanism": "strict JSON stdout; additionalContext is injected; any extra key fails validation" },
        "blocking_semantics": { "kind": "deny-and-block" },
        "wake_capability": { "kind": "none" },
        "install_seam": {
            "config_path": "~/.zcode/cli/config.json",
            "format": "json",
            "entry_shape": "top-level hooks block: { enabled: true, events: { <Event>: [ { matcher?, hooks: [{type: \"command\", command, ...}] } ] } }; configuration-file hooks stay disabled until hooks.enabled is true",
            "ownership_marker": "hook command resolves to the AIKit dispatch executable",
            "preserves_foreign_entries": true
        },
        "uninstall_seam": {
            "config_path": "~/.zcode/cli/config.json",
            "format": "json",
            "entry_shape": "entries whose command matches the ownership marker are removed and empty event keys pruned; foreign hooks and every other top-level key are untouched",
            "ownership_marker": "hook command resolves to the AIKit dispatch executable",
            "preserves_foreign_entries": true
        },
        "provenance": { "authored_by": "test", "source_refs": ["survey:test"] }
    }))
    .unwrap()
}

fn config_after_install(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap()).unwrap()
}

fn write_config(dir: &Path, contents: &str) {
    std::fs::write(dir.join("config.json"), contents).unwrap();
}

// ---------------------------------------------------------------------------
// The installed shape
// ---------------------------------------------------------------------------

#[test]
fn installing_writes_one_dispatcher_entry_per_mapped_descriptor_event() {
    let config = tempfile::tempdir().unwrap();

    let items = adapter().install(config.path()).unwrap();
    assert_eq!(items.len(), 1, "one configuration file");
    materialize(&items, config.path());

    let hooks = config_after_install(config.path());
    let events = hooks["hooks"]["events"].as_object().unwrap();
    let mut names: Vec<&String> = events.keys().collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "PostToolUse",
            "PreToolUse",
            "SessionStart",
            "Stop",
            "UserPromptSubmit",
        ],
        "the installed set is the descriptor's mapped events, not a hard-coded list"
    );

    for (event, entries) in events {
        let commands: Vec<&str> = entries
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert_eq!(
            commands,
            vec![format!("aikit hook dispatch zcode {event}")],
            "exactly one durable dispatcher entry per event"
        );
    }
}

#[test]
fn the_custom_events_are_disclosed_but_never_installed() {
    let outcome = CapabilityOutcome::Descriptor(Box::new(zcode_capability()));
    let (mapped, unrouted) = outcome.dispatch_events();

    assert_eq!(mapped.len(), 5, "five native events map onto AIKit boundaries");
    assert!(
        unrouted.iter().any(|u| u.contains("PermissionRequest")),
        "the customs stay disclosed beside the mapped events: {unrouted:?}"
    );
    assert!(
        unrouted.iter().any(|u| u.contains("PostToolUseFailure")),
        "the customs stay disclosed beside the mapped events: {unrouted:?}"
    );

    let config = tempfile::tempdir().unwrap();
    materialize(&adapter().install(config.path()).unwrap(), config.path());
    let document = config_after_install(config.path());
    let events = document["hooks"]["events"].as_object().unwrap();
    assert!(
        !events.contains_key("PermissionRequest") && !events.contains_key("PostToolUseFailure"),
        "a custom event is not a boundary AIKit dispatches: {events:?}"
    );
}

#[test]
fn no_entry_carries_a_matcher_because_omitted_matches_everything() {
    let config = tempfile::tempdir().unwrap();
    materialize(&adapter().install(config.path()).unwrap(), config.path());

    let document = config_after_install(config.path());
    let events = document["hooks"]["events"].as_object().unwrap();
    for (event, entries) in events {
        for matcher in entries.as_array().unwrap() {
            assert!(
                matcher.get("matcher").is_none(),
                "a matcher AIKit invented would narrow what the dispatcher sees: {event}"
            );
        }
    }
}

#[test]
fn hooks_are_enabled_because_configuration_file_hooks_are_inert_without_it() {
    let config = tempfile::tempdir().unwrap();
    materialize(&adapter().install(config.path()).unwrap(), config.path());
    assert_eq!(
        config_after_install(config.path())["hooks"]["enabled"],
        serde_json::Value::Bool(true),
        "the descriptor's seam says configuration-file hooks stay disabled until enabled"
    );

    // A pre-existing hooks block without the flag gains it too: an install whose
    // entries never run is not an install.
    let config = tempfile::tempdir().unwrap();
    write_config(
        config.path(),
        r#"{"hooks":{"events":{"Stop":[{"hooks":[{"type":"command","command":"my-own"}]}]}}}"#,
    );
    materialize(&adapter().install(config.path()).unwrap(), config.path());
    assert_eq!(
        config_after_install(config.path())["hooks"]["enabled"],
        serde_json::Value::Bool(true)
    );
}

#[test]
fn an_explicitly_disabled_hooks_block_refuses_instead_of_flipping_the_users_flag() {
    let config = tempfile::tempdir().unwrap();
    write_config(
        config.path(),
        r#"{"hooks":{"enabled":false,"events":{"Stop":[{"hooks":[{"type":"command","command":"mine"}]}]}}}"#,
    );
    let before = std::fs::read_to_string(config.path().join("config.json")).unwrap();

    let error = adapter().install(config.path()).unwrap_err();
    assert_eq!(error.code(), "client.hooks_disabled_by_user");
    assert_eq!(
        std::fs::read_to_string(config.path().join("config.json")).unwrap(),
        before,
        "enabling the runner would also activate hooks the user kept disabled"
    );
}

// ---------------------------------------------------------------------------
// Idempotence and preservation
// ---------------------------------------------------------------------------

#[test]
fn installing_twice_is_byte_for_byte_idempotent() {
    let config = tempfile::tempdir().unwrap();
    let zcode = adapter();

    materialize(&zcode.install(config.path()).unwrap(), config.path());
    let first = std::fs::read_to_string(config.path().join("config.json")).unwrap();

    materialize(&zcode.install(config.path()).unwrap(), config.path());
    let second = std::fs::read_to_string(config.path().join("config.json")).unwrap();

    assert_eq!(first, second);
}

#[test]
fn installing_merges_into_an_existing_config_without_destroying_anything() {
    let config = tempfile::tempdir().unwrap();
    write_config(
        config.path(),
        r#"{
  "plugins": { "enabledPlugins": { "github@zcode-plugins-official": true } },
  "mcp": { "servers": { "bimba": { "type": "stdio", "command": "node" } } },
  "hooks": {
    "events": {
      "Stop": [
        { "hooks": [{ "type": "command", "command": "my-own-stopper" }] }
      ]
    }
  }
}"#,
    );

    materialize(&adapter().install(config.path()).unwrap(), config.path());
    let config_json = config_after_install(config.path());

    assert_eq!(
        config_json["plugins"]["enabledPlugins"]["github@zcode-plugins-official"],
        serde_json::Value::Bool(true),
        "unrelated top-level keys are untouched"
    );
    assert_eq!(
        config_json["mcp"]["servers"]["bimba"]["command"],
        "node",
        "unrelated top-level keys are untouched"
    );
    let stop: Vec<&str> = config_json["hooks"]["events"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        stop.contains(&"my-own-stopper"),
        "the user's own hook must survive: {stop:?}"
    );
    assert!(stop.contains(&"aikit hook dispatch zcode Stop"));
}

#[test]
fn a_stale_aikit_entry_is_replaced_rather_than_joined_by_a_second_one() {
    let config = tempfile::tempdir().unwrap();
    write_config(
        config.path(),
        r#"{"hooks":{"enabled":true,"events":{"Stop":[{"hooks":[{"type":"command","command":"aikit hook dispatch zcode Stopp"}]}]}}}"#,
    );

    materialize(&adapter().install(config.path()).unwrap(), config.path());
    let document = config_after_install(config.path());
    let stop: Vec<&str> = document["hooks"]["events"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert_eq!(
        stop,
        vec!["aikit hook dispatch zcode Stop"],
        "a typo'd old AIKit entry has to go, or every event fires twice: {stop:?}"
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[test]
fn a_config_file_that_is_not_json_is_refused_rather_than_overwritten() {
    let config = tempfile::tempdir().unwrap();
    write_config(config.path(), "{ this is not json");

    let error = adapter().install(config.path()).unwrap_err();
    assert_eq!(error.code(), "client.settings_unreadable");
    assert_eq!(
        std::fs::read_to_string(config.path().join("config.json")).unwrap(),
        "{ this is not json",
        "a file AIKit could not understand is left exactly as it was"
    );
}

#[test]
fn install_without_a_capability_descriptor_refuses_instead_of_guessing() {
    let config = tempfile::tempdir().unwrap();
    let bare = ZcodeAdapter::new();

    let error = bare.install(config.path()).unwrap_err();
    assert_eq!(error.code(), "client.capability_unavailable");
}

#[test]
fn the_config_file_name_comes_from_the_descriptor_seam() {
    // A seam that points somewhere else is honoured: the adapter does not
    // assume config.json.
    let config = tempfile::tempdir().unwrap();
    let mut capability = zcode_capability();
    capability.install_seam.config_path = "~/.zcode/other-name.json".to_string();
    let adapter = ZcodeAdapter::new().with_capability(capability);

    materialize(&adapter.install(config.path()).unwrap(), config.path());
    assert!(config.path().join("other-name.json").is_file());
}

// ---------------------------------------------------------------------------
// Harness admission (the integrated harness-adapter contract)
// ---------------------------------------------------------------------------

#[test]
fn zcode_admits_through_the_harness_adapter_contract_with_a_full_census() {
    use aikit_core::harness_admission::HarnessAdmissionAdapter;
    use aikit_adapters::clients::zcode::{ADAPTER_REF, PRODUCT};
    use aikit_core::harness_admission::{FacultySupport, HarnessFaculty, HARNESS_ADAPTER_SDK_VERSION};
    use aikit_core::platform::TargetId;

    let admission = adapter().admission();

    assert_eq!(admission.schema, HARNESS_ADAPTER_SDK_VERSION);
    assert_eq!(admission.adapter_ref, ADAPTER_REF);
    assert_eq!(admission.product, PRODUCT);
    assert_eq!(admission.target, TargetId::zcode());
    assert_ne!(admission.target, TargetId::claude_code());
    assert_ne!(admission.target, TargetId::codex());

    assert_eq!(admission.faculties.len(), 15);
    admission.validate().expect("admission must validate");
    for faculty in &admission.faculties {
        if faculty.support == FacultySupport::Supported {
            assert!(
                !faculty.evidence_refs.is_empty(),
                "{:?} must carry evidence",
                faculty.faculty
            );
        }
    }

    // Reload truth: config read at session start; no live reload, no restart.
    assert_eq!(
        admission.faculty(HarnessFaculty::LiveReload).unwrap().support,
        FacultySupport::Unsupported
    );
    assert_eq!(
        admission.faculty(HarnessFaculty::NextSessionReload).unwrap().support,
        FacultySupport::Supported
    );

    // The hook faculty cites the Actuation descriptor, not a restated list.
    let hook = admission.faculty(HarnessFaculty::SessionStartHook).unwrap();
    assert_eq!(hook.support, FacultySupport::Supported);
    assert!(
        hook.evidence_refs
            .iter()
            .any(|r| r.starts_with("actuation:harness-capability/zcode@r")),
        "the dispatch faculty cites Actuation's descriptor intake: {hook:?}"
    );
}

#[test]
fn zcode_activation_truth_rejects_an_overclaiming_observation() {
    use aikit_core::harness_admission::{
        HarnessActivationObservation, HarnessActivationState, verify_activation_truth,
        HARNESS_ADAPTER_SDK_VERSION,
    };
    use aikit_core::projection::{ActivationEffect, ProjectionPlan, TargetAdapter};

    let zcode = adapter();
    let plan = ProjectionPlan::new(
        zcode.target(),
        ActivationEffect::brokered("no native skill projection; dispatch via install"),
    );
    let observation = HarnessActivationObservation {
        schema: HARNESS_ADAPTER_SDK_VERSION.to_string(),
        target: zcode.target(),
        projection_digest: plan.digest(),
        state: HarnessActivationState::Loaded,
        evidence_refs: vec![],
        native_revision: None,
        note: None,
    };
    assert!(
        verify_activation_truth(&plan, &observation).is_err(),
        "a brokered plan must never be observed as Loaded"
    );
}
