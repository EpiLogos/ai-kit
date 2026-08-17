//! Structured events, to both sinks.
//!
//! The load-bearing test here is `a_recorded_run_event_contains_no_secret_argument`.
//! Everything else in AIKit can be rebuilt from canonical files; a secret written
//! into `logs/events.jsonl` cannot be unwritten, because by the time anyone finds
//! it, it has been backed up, grepped and possibly synced.

mod common;

use common::*;

use std::time::Duration;

use aikit_core::arg::{ArgSpec, ArgType, ArgValue, ArgValues, PathKind};
use aikit_core::{GenerationId, Revision, ScopeKind, TargetId};
use aikit_store::events::{Event, EventAction, EventRecorder, Outcome, Timestamp};
use aikit_store::index::Index;
use aikit_store::AikitHome;

fn spec(name: &str, ty: ArgType, secret: bool) -> ArgSpec {
    ArgSpec {
        name: name.to_string(),
        label: None,
        help: None,
        ty,
        position: None,
        flag: None,
        required: None,
        default: None,
        default_from: None,
        choices: vec![],
        must_exist: false,
        path_kind: PathKind::Any,
        min: None,
        max: None,
        pattern: None,
        repeatable: false,
        secret,
    }
}

fn setup(dir: &std::path::Path) -> (AikitHome, Index) {
    let home = AikitHome::at(dir);
    home.ensure_layout().unwrap();
    let index = Index::open(&home.database()).unwrap();
    (home, index)
}

// ---------------------------------------------------------------------------
// Secrets never enter the record
// ---------------------------------------------------------------------------

#[test]
fn a_recorded_run_event_contains_no_secret_argument() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let recorder = EventRecorder::new(&index, home.event_log());

    // Two ways a manifest can declare a secret, and both have to be honoured:
    // the dedicated type, and the flag on an otherwise ordinary string.
    let specs = vec![
        spec("registry", ArgType::String, false),
        spec("token", ArgType::Secret, false),
        spec("webhook", ArgType::String, true),
    ];
    let mut values = ArgValues::new();
    values.insert("registry".into(), ArgValue::String("crates.io".into()));
    values.insert("token".into(), ArgValue::Secret("ghp_realsecret999".into()));
    values.insert(
        "webhook".into(),
        ArgValue::String("https://hooks.example/T0PS3CR3T".into()),
    );

    let event = Event::new(EventAction::Run)
        .for_capsule(cid("script/release/publish"), Revision::from_raw("rev-1"))
        .with_arguments(&specs, &values)
        .with_outcome(Outcome::Success);

    recorder.record(&event).unwrap();

    let line = std::fs::read_to_string(home.event_log()).unwrap();
    for secret in ["ghp_realsecret999", "T0PS3CR3T"] {
        assert!(
            !line.contains(secret),
            "the event log must not contain `{secret}`:\n{line}"
        );
    }
    assert!(
        line.contains("crates.io"),
        "a non-secret argument is still worth recording"
    );
    assert_eq!(
        event.arguments.get("token").map(String::as_str),
        Some("••••••")
    );
    assert_eq!(
        event.arguments.get("webhook").map(String::as_str),
        Some("••••••")
    );
}

#[test]
fn an_argument_nobody_declared_is_masked_rather_than_trusted() {
    let mut values = ArgValues::new();
    values.insert(
        "mystery".into(),
        ArgValue::String("possibly-a-token".into()),
    );
    let event = Event::new(EventAction::Run).with_arguments(&[], &values);
    assert_eq!(
        event.arguments.get("mystery").map(String::as_str),
        Some("••••••"),
        "an undeclared value is one nobody has said is safe"
    );
}

#[test]
fn an_event_has_no_field_that_prompt_or_transcript_text_could_live_in() {
    // Redaction is a policy that can be forgotten. Absence is structural.
    let event = Event::new(EventAction::HookDispatch)
        .for_capsule(cid("hook/gate/secrets"), Revision::from_raw("rev-1"))
        .with_outcome(Outcome::Denied {
            code: "hook.denied".into(),
        })
        .with_bypass_reason("the gate is wrong about generated code");

    let json: serde_json::Value = serde_json::from_str(&event.to_json_line().unwrap()).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();

    for forbidden in [
        "prompt",
        "transcript",
        "message",
        "messages",
        "content",
        "stdout",
        "stderr",
        "input",
        "output",
        "body",
    ] {
        assert!(
            !keys.contains(&forbidden),
            "an event must have no `{forbidden}` field; keys are {keys:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Two sinks, one record
// ---------------------------------------------------------------------------

#[test]
fn an_event_reaches_both_the_database_and_the_json_log() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let recorder = EventRecorder::new(&index, home.event_log());

    recorder
        .record(
            &Event::new(EventAction::Run)
                .for_capsule(cid("script/test/nt"), Revision::from_raw("rev-1"))
                .with_outcome(Outcome::Success),
        )
        .unwrap();

    assert_eq!(index.event_count().unwrap(), 1);
    let log = std::fs::read_to_string(home.event_log()).unwrap();
    assert_eq!(log.lines().count(), 1);
    let parsed: Event = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(parsed.action, EventAction::Run);
    assert_eq!(parsed.capsule, Some(cid("script/test/nt")));
}

#[test]
fn the_log_is_appended_to_rather_than_replaced() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let recorder = EventRecorder::new(&index, home.event_log());

    for _ in 0..3 {
        recorder
            .record(&Event::new(EventAction::Apply).with_outcome(Outcome::Success))
            .unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(home.event_log())
            .unwrap()
            .lines()
            .count(),
        3
    );
    assert_eq!(index.event_count().unwrap(), 3);
}

#[test]
fn a_missing_log_directory_is_created_rather_than_dropping_the_event() {
    let tmp = tempfile::tempdir().unwrap();
    let index = Index::open(&tmp.path().join("state/aikit.sqlite3")).unwrap();
    let log = tmp.path().join("nowhere/at/all/events.jsonl");
    let recorder = EventRecorder::new(&index, &log);

    recorder
        .record(&Event::new(EventAction::Gc).with_outcome(Outcome::Success))
        .unwrap();
    assert!(log.is_file());
}

// ---------------------------------------------------------------------------
// The fields the specification asks for
// ---------------------------------------------------------------------------

#[test]
fn an_event_carries_every_dimension_the_specification_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let recorder = EventRecorder::new(&index, home.event_log());

    let parent = Event::new(EventAction::Apply).with_outcome(Outcome::Success);
    let generation = GenerationId::parse("gen_b71f2fdeadbeef01").unwrap();
    let child = Event::new(EventAction::HookDispatch)
        .for_capsule(cid("hook/gate/secrets"), Revision::from_raw("rev-9"))
        .in_context(
            aikit_core::ContextId::generate(),
            Some(aikit_core::SessionId::generate()),
        )
        .for_project(aikit_core::ProjectId::generate())
        .with_client(TargetId::claude_code())
        .with_mux(aikit_core::MuxKind::Tmux)
        .with_scope(ScopeKind::Session)
        .with_duration(Duration::from_millis(42))
        .with_generation(generation.clone())
        .with_bypass_reason("reviewed by hand")
        .caused_by(parent.event_id.clone())
        .with_outcome(Outcome::Denied {
            code: "hook.denied".into(),
        });

    recorder.record(&parent).unwrap();
    recorder.record(&child).unwrap();

    let log = std::fs::read_to_string(home.event_log()).unwrap();
    let restored: Event = serde_json::from_str(log.lines().nth(1).unwrap()).unwrap();

    assert_eq!(restored, child, "the log line is a faithful record");
    assert_eq!(restored.duration_ms, Some(42));
    assert_eq!(restored.generation, Some(generation));
    assert_eq!(restored.parent_event, Some(parent.event_id));
    assert_eq!(restored.kind, Some(aikit_core::Kind::Hook));
}

#[test]
fn a_bypass_is_visible_in_the_recorded_history() {
    // "A hook bypass is visible and recorded" — release-blocking case 9.
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let recorder = EventRecorder::new(&index, home.event_log());

    recorder
        .record(
            &Event::new(EventAction::HookDispatch)
                .for_capsule(cid("hook/gate/secrets"), Revision::from_raw("rev-1"))
                .with_bypass_reason("hotfix, reviewed by hand")
                .with_outcome(Outcome::Skipped {
                    reason: "bypassed".into(),
                }),
        )
        .unwrap();

    let recent = index.recent_events(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(
        recent[0].bypass_reason.as_deref(),
        Some("hotfix, reviewed by hand")
    );
}

#[test]
fn recent_events_come_back_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let recorder = EventRecorder::new(&index, home.event_log());

    let base = Timestamp::now();
    for (offset, action) in [
        (0, EventAction::Apply),
        (1_000_000, EventAction::Run),
        (2_000_000, EventAction::Gc),
    ] {
        recorder
            .record(
                &Event::new(action)
                    .at(Timestamp::from_nanos(base.as_nanos() + offset))
                    .with_outcome(Outcome::Success),
            )
            .unwrap();
    }

    let recent = index.recent_events(2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].action, EventAction::Gc);
    assert_eq!(recent[1].action, EventAction::Run);
}

#[test]
fn a_timestamp_round_trips_through_its_rendered_form() {
    let now = Timestamp::now();
    let rendered = now.to_string();
    let back: Timestamp = rendered.parse().unwrap();
    assert_eq!(back.as_nanos(), now.as_nanos());
    assert!(rendered.contains('T'), "RFC 3339, so a person can read it");
}

#[test]
fn an_outcome_distinguishes_a_denial_from_a_failure() {
    // §8: a system failure and a policy denial must stay distinguishable.
    let denied = Outcome::Denied {
        code: "hook.denied".into(),
    };
    let failed = Outcome::Failure {
        code: "hook.timeout".into(),
    };
    assert_ne!(denied.label(), failed.label());
    assert!(!denied.is_success());
    assert!(!failed.is_success());
}
