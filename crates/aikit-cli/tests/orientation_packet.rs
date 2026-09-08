//! W2/W1, CASE 02 + CASE 09 — the orientation packet at the engine seam: a
//! fresh session in a project is met by that project's own NOW field,
//! assembled through the ctrl inspect action; the @1 human horizon stays
//! closed until the composition opens it; other projects are structurally
//! absent; the budget bounds the packet; failure degrades to a warning.

use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

use aikit_adapters::runner::{CommandRunner, Output};
use aikit_cli::orientation_packet::{orientation_packet_in, OrientationConfig};
use serde_json::{json, Value};

/// Answers `projectcentral.now.inspect` for the requested project.
struct NowRunner {
    answers: BTreeMap<String, Value>,
    fail: bool,
    seen: Mutex<Vec<Vec<String>>>,
}

impl NowRunner {
    fn with_field(project: &str, field: Value) -> Self {
        Self {
            answers: BTreeMap::from([(project.to_owned(), field)]),
            fail: false,
            seen: Mutex::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            answers: BTreeMap::new(),
            fail: true,
            seen: Mutex::new(Vec::new()),
        }
    }

    fn requested_projects(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|argv| {
                let input = argv.last().unwrap();
                serde_json::from_str::<Value>(input).unwrap()["project"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }
}

impl CommandRunner for NowRunner {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.seen.lock().unwrap().push(argv.to_vec());
        if self.fail {
            return Ok(Output {
                status: 1,
                stdout: String::new(),
                stderr: "ctrl exploded".into(),
            });
        }
        let input = argv.last().unwrap();
        let project = serde_json::from_str::<Value>(input).unwrap()["project"]
            .as_str()
            .unwrap()
            .to_owned();
        let field = self.answers.get(&project).cloned().unwrap_or(json!({
            "exists": false,
        }));
        Ok(Output {
            status: 0,
            stdout: json!({"ok": true, "status": "success", "data": field}).to_string(),
            stderr: String::new(),
        })
    }
}

fn handoff(id: &str, subject: &str, result: &str, recorded: i64, kind: &str) -> Value {
    json!({
        "schema": "central.project-now.handoff/v1",
        "id": id,
        "provenance": "agent-authored-bounded-return",
        "actor": "agent-session-test",
        "kind": kind,
        "recorded_at_unix_seconds": recorded,
        "subject": subject,
        "result": result,
        "status": "active",
    })
}

/// A Central world with projects A (with a field) and B (cold, no field).
fn fixture() -> (PathBuf, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap().keep();
    let project_a = root.join("Work/A");
    let project_b = root.join("Work/B");
    fs::create_dir_all(project_a.join("src")).unwrap();
    fs::create_dir_all(&project_b).unwrap();
    (root, project_a, project_b)
}

fn field_with() -> Value {
    json!({
        "exists": true,
        "project_root": "/central/Work/A",
        "active_items": [
            handoff("h-old", "older note", "superseded context", 100, "note"),
            handoff("h-new", "the open continuation", "resume here: the engine awaits its packet", 200, "handoff"),
        ],
        "open_questions": [
            handoff("h-q", "is the aperture law testable?", "yes, by composition", 150, "question"),
        ],
        "invalid_items": ["now/agents/broken.json: schema mismatch"],
        "human_scratch": ["now/user/my-own-notes.md"],
    })
}

#[test]
fn a_fresh_session_is_met_by_the_projects_open_continuation_and_work() {
    let (root, a, _b) = fixture();
    let runner = NowRunner::with_field("A", field_with());
    let packet = orientation_packet_in(&runner, Some(&root), Some(&a), &OrientationConfig::default())
        .unwrap()
        .expect("the field is open, the packet must arrive");

    assert!(packet.starts_with("[continuity/orientation-packet] project A"), "{packet}");
    assert!(packet.contains("2 open item(s), 1 open question(s)"), "{packet}");
    // CASE 09: the newest handoff return is the continuation, in the packet.
    assert!(packet.contains("- continuation: the open continuation (actor: agent-session-test)"), "{packet}");
    assert!(packet.contains("resume here: the engine awaits its packet"), "{packet}");
    // Older work arrives as a subject, not a second continuation.
    assert!(packet.contains("- note: older note"), "{packet}");
    assert!(packet.contains("- question: is the aperture law testable?"), "{packet}");
    // Invalid records are disclosed, never dropped.
    assert!(packet.contains("- warning: invalid NOW record disclosed: now/agents/broken.json"), "{packet}");
}

#[test]
fn the_human_horizon_stays_closed_until_the_composition_opens_it() {
    let (root, a, _b) = fixture();
    let runner = NowRunner::with_field("A", field_with());

    let closed = orientation_packet_in(&runner, Some(&root), Some(&a), &OrientationConfig::default())
        .unwrap()
        .unwrap();
    assert!(!closed.contains("human scratch"), "@1 material leaked: {closed}");
    assert!(!closed.contains("my-own-notes.md"), "@1 material leaked: {closed}");

    let mut config = OrientationConfig::default();
    config.include_human_scratch = true;
    let open = orientation_packet_in(&runner, Some(&root), Some(&a), &config)
        .unwrap()
        .unwrap();
    assert!(open.contains("human scratch (aperture open by composition)"), "{open}");
    assert!(open.contains("my-own-notes.md"), "{open}");
}

#[test]
fn only_the_project_stood_in_is_ever_inspected() {
    let (root, a, b) = fixture();
    let mut runner = NowRunner::with_field("A", field_with());
    runner
        .answers
        .insert("B".into(), json!({"exists": true, "active_items": [
            handoff("h-b", "project B secret work", "never leaves B", 999, "handoff"),
        ]}));

    let packet = orientation_packet_in(&runner, Some(&root), Some(&a), &OrientationConfig::default())
        .unwrap()
        .unwrap();
    assert_eq!(runner.requested_projects(), vec!["A".to_owned()]);
    assert!(!packet.contains("project B secret work"), "B material leaked: {packet}");

    // The same law from the cold project: B is inspected for B.
    let cold = orientation_packet_in(&runner, Some(&root), Some(&b), &OrientationConfig::default())
        .unwrap();
    assert!(cold.is_none() || !cold.unwrap().contains("the open continuation"));
}

#[test]
fn nothing_open_or_no_project_is_an_honest_absence() {
    let (root, a, _b) = fixture();
    let empty = NowRunner::with_field("A", json!({
        "exists": true,
        "active_items": [],
        "open_questions": [],
        "invalid_items": [],
    }));
    assert!(orientation_packet_in(&empty, Some(&root), Some(&a), &OrientationConfig::default())
        .unwrap()
        .is_none());

    let missing = NowRunner::with_field("A", json!({"exists": false}));
    assert!(orientation_packet_in(&missing, Some(&root), Some(&a), &OrientationConfig::default())
        .unwrap()
        .is_none());

    // The Central root itself is not a project: nothing to inspect.
    let runner = NowRunner::with_field("A", field_with());
    assert!(orientation_packet_in(&runner, Some(&root), Some(&root), &OrientationConfig::default())
        .unwrap()
        .is_none());
    assert!(runner.requested_projects().is_empty(), "no ctrl call may fire without a project");
}

#[test]
fn a_failed_ctrl_call_degrades_to_a_reportable_warning() {
    let (root, a, _b) = fixture();
    let runner = NowRunner::failing();
    let error = orientation_packet_in(&runner, Some(&root), Some(&a), &OrientationConfig::default())
        .unwrap_err();
    assert!(error.contains("continuity/orientation-packet"), "{error}");
    assert!(error.contains("ctrl"), "{error}");
}

#[test]
fn the_budget_bounds_the_packet() {
    let (root, a, _b) = fixture();
    let items: Vec<Value> = (0..8)
        .map(|i| {
            handoff(
                &format!("h-{i}"),
                &format!("open thread {i}"),
                &"x".repeat(400),
                100 + i,
                "handoff",
            )
        })
        .collect();
    let field = json!({"exists": true, "active_items": items, "open_questions": [], "invalid_items": []});
    let runner = NowRunner::with_field("A", field);

    let mut config = OrientationConfig::default();
    config.max_items = 3;
    config.max_result_chars = 50;
    let packet = orientation_packet_in(&runner, Some(&root), Some(&a), &config)
        .unwrap()
        .unwrap();

    let item_lines = packet.lines().filter(|l| l.starts_with("- ")).count();
    assert!(item_lines <= 4, "budget exceeded: {packet}");
    assert!(packet.contains("withheld by the orientation budget"), "{packet}");
    let newest = packet.lines().find(|l| l.contains("open thread 7")).unwrap();
    assert!(newest.contains("continuation"), "{packet}");
    assert!(
        packet.matches("xxxxxxxxxx").count() <= 5,
        "result lines must be bounded to the tuned width: {packet}"
    );
}

#[test]
fn the_tunings_in_effect_are_the_compositions_not_ambient() {
    // Absent config keeps the closed, bounded defaults.
    assert_eq!(OrientationConfig::from_config(None), OrientationConfig::default());
    // Wrong types keep the defaults rather than half-tuning.
    let mut wrong = toml::value::Table::new();
    wrong.insert("include_human_scratch".into(), "yes".into());
    wrong.insert("max_items".into(), "three".into());
    assert_eq!(OrientationConfig::from_config(Some(&wrong)), OrientationConfig::default());

    // The composition's values are the values in effect.
    let mut tuned = toml::value::Table::new();
    tuned.insert("include_human_scratch".into(), true.into());
    tuned.insert("max_items".into(), 9.into());
    tuned.insert("max_result_chars".into(), 120.into());
    let config = OrientationConfig::from_config(Some(&tuned));
    assert!(config.include_human_scratch);
    assert_eq!(config.max_items, 9);
    assert_eq!(config.max_result_chars, 120);
}
