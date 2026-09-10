//! W2/CASE 08 — the close-out verification reads the objects back.
//!
//! The verification asks the native owners the same questions the routing law
//! answers: is there a continuation, is exactly one standing open, was learned
//! material captured, was deferred work registered. It is machine-checkable
//! because it reports per clause, with the owner's own evidence, and it fails
//! rather than skipping when a clause could be asked and did not hold.

use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use aikit_adapters::runner::{CommandRunner, Output};
use aikit_cli::closeout::{verify, ClauseState, FactoryBinding};
use serde_json::{json, Value};

/// Answers both owners: `ctrl projectcentral.now.inspect` and
/// `factory development observations`.
struct Owners {
    field: Value,
    observations: Option<u64>,
    factory_fails: bool,
    seen: Mutex<Vec<Vec<String>>>,
}

impl Owners {
    fn new(field: Value) -> Self {
        Self {
            field,
            observations: None,
            factory_fails: false,
            seen: Mutex::new(Vec::new()),
        }
    }

    fn with_observations(mut self, count: u64) -> Self {
        self.observations = Some(count);
        self
    }

    fn with_failing_factory(mut self) -> Self {
        self.factory_fails = true;
        self
    }

    fn asked(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|argv| argv.join(" "))
            .collect()
    }
}

impl CommandRunner for Owners {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.seen.lock().unwrap().push(argv.to_vec());
        let ok = |value: Value| {
            Ok(Output {
                status: 0,
                stdout: value.to_string(),
                stderr: String::new(),
            })
        };
        if argv.iter().any(|arg| arg == "observations") {
            if self.factory_fails {
                return Ok(Output {
                    status: 2,
                    stdout: String::new(),
                    stderr: "factory: no such ledger root".into(),
                });
            }
            return ok(json!({
                "contract": "factory.project-development/v1",
                "runRef": "run:01ARZ3NDEKTSV4RRFFQ69G5FCB",
                "observationCount": self.observations.unwrap_or(0),
                "observations": [],
            }));
        }
        ok(json!({"ok": true, "status": "success", "data": self.field.clone()}))
    }
}

fn item(id: &str, kind: &str, recorded: i64) -> Value {
    json!({
        "id": id,
        "kind": kind,
        "subject": format!("{kind} {id}"),
        "result": "…",
        "status": "active",
        "recorded_at_unix_seconds": recorded,
    })
}

fn field(items: Vec<Value>) -> Value {
    json!({
        "exists": true,
        "active_items": items,
        "open_questions": [],
        "invalid_items": [],
        "human_scratch": [],
    })
}

fn root() -> PathBuf {
    PathBuf::from("/tmp/central-world")
}

fn states(verification: &aikit_cli::closeout::Verification) -> BTreeMap<&str, ClauseState> {
    verification
        .clauses
        .iter()
        .map(|clause| (clause.clause, clause.state))
        .collect()
}

const LEDGER: FactoryBinding<'static> = FactoryBinding {
    binary: "factory",
    ledger_root: "/tmp/ledger",
    run_ref: "run:01ARZ3NDEKTSV4RRFFQ69G5FCB",
};

#[test]
fn a_complete_close_out_verifies_against_the_owners_own_records() {
    let owners = Owners::new(field(vec![
        item("handoff-2", "handoff", 200),
        item("learning-1", "learning", 150),
        item("note-1", "note", 100),
    ]))
    .with_observations(2);
    let verification = verify(&owners, &root(), "A", Some(100), Some(LEDGER)).unwrap();
    assert!(verification.verified, "{:?}", verification.clauses);
    assert_eq!(
        states(&verification)["continuation-registered"],
        ClauseState::Pass
    );
    assert_eq!(
        states(&verification)["deferred-work-registered"],
        ClauseState::Pass
    );
    // It asked both owners, in their own commands — not the filesystem.
    let asked = owners.asked();
    assert!(asked
        .iter()
        .any(|call| call.contains("projectcentral.now.inspect")));
    assert!(asked
        .iter()
        .any(|call| call.contains("factory development observations /tmp/ledger")));
}

#[test]
fn two_open_handoffs_fail_the_supersede_clause_and_name_both() {
    let owners = Owners::new(field(vec![
        item("handoff-1", "handoff", 100),
        item("handoff-2", "handoff", 200),
        item("learning-1", "learning", 150),
    ]));
    let verification = verify(&owners, &root(), "A", None, None).unwrap();
    assert!(!verification.verified);
    let clause = verification
        .clauses
        .iter()
        .find(|clause| clause.clause == "previous-handoff-superseded")
        .unwrap();
    assert_eq!(clause.state, ClauseState::Fail);
    assert!(clause.detail.contains("handoff-1"), "{}", clause.detail);
    assert!(clause.detail.contains("handoff-2"), "{}", clause.detail);
}

#[test]
fn a_continuation_from_before_this_close_out_does_not_satisfy_the_clause() {
    let owners = Owners::new(field(vec![
        item("handoff-old", "handoff", 50),
        item("learning-1", "learning", 150),
    ]));
    let verification = verify(&owners, &root(), "A", Some(100), None).unwrap();
    assert!(!verification.verified);
    let clause = verification
        .clauses
        .iter()
        .find(|clause| clause.clause == "continuation-registered")
        .unwrap();
    assert_eq!(clause.state, ClauseState::Fail);
    assert!(clause.detail.contains("before this close-out began"));
}

#[test]
fn learned_material_is_a_clause_of_its_own_not_folded_into_the_continuation() {
    let owners = Owners::new(field(vec![item("handoff-2", "handoff", 200)]));
    let verification = verify(&owners, &root(), "A", Some(100), None).unwrap();
    assert_eq!(
        states(&verification)["continuation-registered"],
        ClauseState::Pass
    );
    assert_eq!(
        states(&verification)["learned-material-recorded"],
        ClauseState::Fail
    );
    assert!(!verification.verified);
}

#[test]
fn an_unbound_factory_ledger_is_skipped_and_a_skip_is_not_a_pass() {
    let owners = Owners::new(field(vec![
        item("handoff-2", "handoff", 200),
        item("learning-1", "learning", 150),
    ]));
    let verification = verify(&owners, &root(), "A", None, None).unwrap();
    let clause = verification
        .clauses
        .iter()
        .find(|clause| clause.clause == "deferred-work-registered")
        .unwrap();
    assert_eq!(clause.state, ClauseState::Skipped);
    assert!(clause.detail.contains("not checked"));
    // Skipped clauses do not sink the verdict, and the reading says why.
    assert!(verification.verified);
    assert!(verification.describe().contains("[skipped]"));
    // Nothing was asked of Factory at all.
    assert!(owners
        .asked()
        .iter()
        .all(|call| !call.contains("observations")));
}

#[test]
fn a_factory_that_cannot_answer_fails_the_clause_rather_than_passing_it_quietly() {
    let owners = Owners::new(field(vec![
        item("handoff-2", "handoff", 200),
        item("learning-1", "learning", 150),
    ]))
    .with_failing_factory();
    let verification = verify(&owners, &root(), "A", None, Some(LEDGER)).unwrap();
    assert!(!verification.verified);
    let clause = verification
        .clauses
        .iter()
        .find(|clause| clause.clause == "deferred-work-registered")
        .unwrap();
    assert_eq!(clause.state, ClauseState::Fail);
    assert!(
        clause.detail.contains("no such ledger root"),
        "{}",
        clause.detail
    );
}

#[test]
fn a_project_with_no_now_field_is_an_error_not_a_verified_close_out() {
    let owners = Owners::new(json!({"exists": false}));
    let error = verify(&owners, &root(), "A", None, None).unwrap_err();
    assert!(error.contains("no NOW field"), "{error}");
}
