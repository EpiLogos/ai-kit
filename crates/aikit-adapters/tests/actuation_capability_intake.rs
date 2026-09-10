//! Intake of Actuation's capability descriptors, against a scripted runner.

use aikit_adapters::actuation_harness_capability::{
    intake_actuation_capability, CapabilityOutcome, ACTUATION_HARNESS_CAPABILITY_SCHEMA,
};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::Result;

/// A runner that answers with a scripted response and records the argv it
/// was asked to run — enough to pin the exact command intake issues.
struct ScriptedRunner {
    stdout: String,
    stderr: String,
    status: i32,
    seen_argv: std::sync::Mutex<Vec<Vec<String>>>,
}

impl ScriptedRunner {
    fn succeeding(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            status: 0,
            seen_argv: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn failing(stderr: impl Into<String>) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            status: 1,
            seen_argv: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn argv(&self) -> Vec<String> {
        self.seen_argv.lock().unwrap()[0].clone()
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(&self, argv: &[String]) -> Result<Output> {
        self.seen_argv.lock().unwrap().push(argv.to_vec());
        Ok(Output {
            status: self.status,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        })
    }
}

fn read_model_json() -> String {
    serde_json::json!({
        "schema": ACTUATION_HARNESS_CAPABILITY_SCHEMA,
        "document": "capability-read-model",
        "capability": {
            "schema": ACTUATION_HARNESS_CAPABILITY_SCHEMA,
            "document": "capability",
            "harness_slug": "claude-code",
            "native_events": [
                { "event": "pre-tool-use", "native_name": "PreToolUse", "transport": "settings-json-hooks-map", "can_block": true, "context_channel": "stdout-additional-context" },
                { "event": "custom", "native_name": "PermissionRequest", "transport": "config-json-hooks-map", "can_block": true, "context_channel": "exit-code-payload" }
            ],
            "injection_channel": { "kind": "stdout-additional-context", "mechanism": "hookSpecificOutcome.additionalContext" },
            "blocking_semantics": { "kind": "deny-and-block" },
            "wake_capability": { "kind": "none" },
            "install_seam": {
                "config_path": "~/.claude/settings.json",
                "format": "json",
                "entry_shape": "hooks.<EventName>[] entries",
                "ownership_marker": "command resolves to the AIKit dispatch executable",
                "preserves_foreign_entries": true
            },
            "uninstall_seam": {
                "config_path": "~/.claude/settings.json",
                "format": "json",
                "entry_shape": "marker-matched entries removed",
                "ownership_marker": "command resolves to the AIKit dispatch executable",
                "preserves_foreign_entries": true
            },
            "provenance": { "authored_by": "test", "source_refs": ["survey:test"] }
        }
    })
    .to_string()
}

#[test]
fn intake_runs_the_real_cli_route_and_returns_the_descriptor() {
    let runner = ScriptedRunner::succeeding(read_model_json());
    let outcome = intake_actuation_capability(&runner, "actuation", "claude-code");

    assert_eq!(runner.argv(), vec!["actuation", "harness", "capability", "claude-code", "--json"]);
    match outcome {
        CapabilityOutcome::Descriptor(capability) => {
            assert_eq!(capability.harness_slug, "claude-code");
            assert_eq!(capability.native_events.len(), 2);
            assert!(capability.install_seam.preserves_foreign_entries);
        }
        other => panic!("expected a descriptor, got {other:?}"),
    }
}

#[test]
fn dispatch_events_map_onto_boundaries_and_disclose_the_rest() {
    let runner = ScriptedRunner::succeeding(read_model_json());
    let outcome = intake_actuation_capability(&runner, "actuation", "claude-code");
    let (mapped, unrouted) = outcome.dispatch_events();

    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].0.as_str(), "PreToolUse");
    assert_eq!(mapped[0].1, "PreToolUse");
    assert_eq!(
        unrouted,
        vec!["PermissionRequest (custom) is outside AIKit's boundary vocabulary".to_string()],
        "a native event AIKit cannot route is disclosed, never silently dropped"
    );
}

#[test]
fn a_failing_cli_run_is_a_disclosed_unavailability_with_the_stderr() {
    let runner = ScriptedRunner::failing("no such harness 'nonexistent'; declared: claude-code, codex, zcode");
    match intake_actuation_capability(&runner, "actuation", "nonexistent") {
        CapabilityOutcome::Unavailable { reason } => {
            assert!(reason.contains("failed (1)"), "{reason}");
            assert!(reason.contains("no such harness"), "{reason}");
        }
        other => panic!("expected unavailability, got {other:?}"),
    }
}

#[test]
fn an_unparsable_or_wrong_schema_answer_is_disclosed_not_parsed_loosely() {
    let wrong_document = read_model_json().replace("capability-read-model", "capability-catalog");
    let runner = ScriptedRunner::succeeding(wrong_document);
    match intake_actuation_capability(&runner, "actuation", "claude-code") {
        CapabilityOutcome::Unavailable { reason } => {
            assert!(reason.contains("unexpected capability document"), "{reason}");
        }
        other => panic!("expected unavailability, got {other:?}"),
    }

    let runner = ScriptedRunner::succeeding("not json at all");
    match intake_actuation_capability(&runner, "actuation", "claude-code") {
        CapabilityOutcome::Unavailable { reason } => {
            assert!(reason.contains("unparsable"), "{reason}");
        }
        other => panic!("expected unavailability, got {other:?}"),
    }
}

#[test]
fn an_unknowable_event_name_never_maps_to_a_boundary() {
    // Guard the mapping table: only the eight declared boundaries route.
    let json = read_model_json().replace("\"event\":\"pre-tool-use\"", "\"event\":\"mid-tool-use\"");
    let runner = ScriptedRunner::succeeding(json);
    let outcome = intake_actuation_capability(&runner, "actuation", "claude-code");
    let (mapped, unrouted) = outcome.dispatch_events();
    assert!(mapped.is_empty(), "nothing may map: {mapped:?}");
    assert_eq!(
        unrouted.len(),
        2,
        "the unknowable event joins the disclosed unrouted set"
    );
}
