//! Routine dispatch through the real stores in an isolated AIKit home
//! (parent Acceptance: "Agent-authored lands as Routine + ledger entry, not a
//! bare cron entry"; dispatcher restart exactly-once; clock-moved-back
//! suppression; catch-up behaviour; event triggers).
//!
//! The dispatcher's three seams are fixtures here: a table-driven occurrence
//! source stands in for Central's `central.time.occurrences`, a closure
//! resolver stands in for the catalogue, and a recording runner stands in for
//! the resident encounter owner. The stores, the ledger, the planner and the
//! admission gate are all the real ones.

use std::collections::BTreeMap;

use aikit_cli::routine_dispatch::{
    MethodResolver, OccurrenceReading, OccurrenceSource, OwnerOccurrence, RoutineDispatcher,
    RoutineRunOutcome, RoutineRunRequest, RunStatus,
};
use aikit_core::method::Method;
use aikit_core::recurrence::CatchUpPolicy;
use aikit_core::resource::routine::{
    ProvenMethodBasis, Routine, RoutineAuthority, RoutineSchedulerBinding, RoutineSchedulerState,
    RoutineTrigger, METHOD_PROOF_VERSION,
};
use aikit_core::resource::{ProviderRef, ResourceRef, SourceRef, SourceRevision};
use aikit_core::schedule::{ScheduleRecord, ScheduleShape};
use aikit_store::{AikitHome, RoutineInvocationStore, RoutineStore, StoredRoutine};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

// -- fixtures --------------------------------------------------------------

fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}

fn rev(s: &str) -> SourceRevision {
    SourceRevision::parse(s).unwrap()
}

struct FixtureMethodResolver {
    revisions: BTreeMap<String, SourceRevision>,
}

impl MethodResolver for FixtureMethodResolver {
    fn resolve(&self, method_ref: &ResourceRef) -> aikit_core::Result<Method> {
        let revision = self
            .revisions
            .get(method_ref.as_str())
            .cloned()
            .unwrap_or_else(|| rev("method-rev-1"));
        Ok(Method {
            id: method_ref.clone(),
            source: SourceRef::parse("source:fixture:method").unwrap(),
            revision: Some(revision),
            name: "Fixture Method".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![],
            actions: vec![r("action/capability/run")],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![r("verification:fixture")],
            expected_resolve: None,
            expected_return_forms: vec![],
        })
    }
}

struct FixtureOccurrences {
    /// schedule JSON -> the instants Central would resolve (as refs + dues).
    table: BTreeMap<String, Vec<(&'static str, i64)>>,
    policy_revision: String,
}

impl OccurrenceSource for FixtureOccurrences {
    fn occurrences(
        &self,
        schedule: &serde_json::Value,
        window_from_unix_ms: i64,
        window_to_unix_ms: i64,
    ) -> aikit_core::Result<OccurrenceReading> {
        let key = schedule.to_string();
        let instants = self.table.get(&key).cloned().unwrap_or_default();
        Ok(OccurrenceReading {
            time_policy_ref: r("central:time-policy:fixture"),
            time_policy_revision: rev(&self.policy_revision),
            occurrences: instants
                .into_iter()
                .filter(|(_, due)| *due >= window_from_unix_ms && *due <= window_to_unix_ms)
                .map(|(occurrence, due)| OwnerOccurrence {
                    occurrence_ref: r(occurrence),
                    due_unix_ms: due,
                })
                .collect(),
        })
    }
}

#[derive(Clone)]
struct RecordingRunner {
    runs: std::sync::Arc<std::sync::Mutex<Vec<RoutineRunRequest>>>,
    outcome: RunStatus,
}

impl aikit_cli::routine_dispatch::RoutineRunner for RecordingRunner {
    fn run(&self, request: RoutineRunRequest) -> RoutineRunOutcome {
        self.runs.lock().unwrap().push(request);
        RoutineRunOutcome {
            status: self.outcome,
            detail: format!("fixture run #{}", self.runs.lock().unwrap().len()),
        }
    }
}

fn fixture_method(revision: &str) -> Method {
    Method {
        id: r("method:fixture"),
        source: SourceRef::parse("source:fixture:method").unwrap(),
        revision: Some(rev(revision)),
        name: "Fixture Method".into(),
        description: String::new(),
        focus: vec![],
        project_domain: vec![],
        skills: vec![],
        actions: vec![r("action/capability/run")],
        capabilities: vec![],
        context_sources: vec![],
        verification: vec![r("verification:fixture")],
        expected_resolve: None,
        expected_return_forms: vec![],
    }
}

fn proof(method: &Method) -> ProvenMethodBasis {
    ProvenMethodBasis {
        version: METHOD_PROOF_VERSION.into(),
        method: method.id.clone(),
        method_revision: method.revision.clone().unwrap(),
        proof_ref: r("proof:fixture"),
        context_resolution_ref: r("context-resolution:fixture"),
        activity_refs: vec![r("activity:fixture:1")],
        return_refs: vec![r("return:fixture:1")],
        evidence_refs: vec![r("evidence:fixture:1")],
        verification_refs: vec![r("verification:fixture:1")],
    }
}

fn authority() -> RoutineAuthority {
    RoutineAuthority {
        authority_ref: r("authority:fixture"),
        revision: Some(rev("authority-rev-1")),
        action_refs: vec![r("action/capability/run")],
        granted: true,
        unattended: true,
    }
}

/// An Enabled daily-06:00 Routine bound to the gateway dispatcher, over an
/// isolated home. The schedule resolves one occurrence per fixture day.
fn world() -> (
    tempfile::TempDir,
    AikitHome,
    RoutineStore,
    RoutineInvocationStore,
    Routine,
) {
    let dir = tempfile::tempdir().unwrap();
    let home = AikitHome::at(dir.path().join("home"));
    let method = fixture_method("method-rev-1");
    let routine = Routine::new(
        r("routine/daily-demo"),
        SourceRef::parse("source:aikit:routines/routine/daily-demo").unwrap(),
        None,
        "Daily demo",
        "",
        &method,
        proof(&method),
        RoutineTrigger::Schedule {
            schedule_ref: "schedule/daily-demo".into(),
        },
        authority(),
        None,
        vec![],
    )
    .unwrap();
    let mut record = StoredRoutine::new(
        routine,
        Some(
            ScheduleRecord::new(
                r("schedule/daily-demo"),
                ScheduleShape::Daily {
                    time: "06:00".into(),
                },
                None,
            )
            .unwrap(),
        ),
        None,
    )
    .unwrap();
    record
        .routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
            provider_job_id: None,
            observed_state: RoutineSchedulerState::Planned,
        })
        .unwrap();
    record
        .routine
        .enable(&fixture_method("method-rev-1"))
        .unwrap();
    let store = RoutineStore::new(home.clone());
    store.put(record.clone()).unwrap();
    let ledger = RoutineInvocationStore::new(home.clone());
    (dir, home, store, ledger, record.routine)
}

fn fixture_daily_table(
    occurrence: &'static str,
    due: i64,
) -> BTreeMap<String, Vec<(&'static str, i64)>> {
    let mut table = BTreeMap::new();
    table.insert(
        serde_json::to_string(&ScheduleShape::Daily {
            time: "06:00".into(),
        })
        .unwrap(),
        vec![(occurrence, due)],
    );
    table
}

fn dispatcher(
    home: &AikitHome,
    table: BTreeMap<String, Vec<(&'static str, i64)>>,
    runner: RecordingRunner,
) -> RoutineDispatcher<FixtureOccurrences, FixtureMethodResolver, RecordingRunner> {
    RoutineDispatcher::new(
        home.clone(),
        FixtureOccurrences {
            table,
            policy_revision: "policy-rev-1".into(),
        },
        FixtureMethodResolver {
            revisions: BTreeMap::new(),
        },
        runner,
    )
}

// -- the scheduled path ------------------------------------------------------

/// A due occurrence is admitted with the owner occurrence's delivery ref, the
/// fixture run executes, and the outcome merges into the same ledger entry.
/// No invocation exists without evidence.
#[test]
fn due_occurrence_is_admitted_then_run_then_recorded() {
    let (_dir, home, _store, ledger, _routine) = world();
    let due = 1_790_016_000_000_i64; // a 06:00 London instant, any day
    let now = due + 10_000;
    let runner_runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let runner = RecordingRunner {
        runs: runner_runs.clone(),
        outcome: RunStatus::Completed,
    };
    let dispatcher = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        runner,
    );

    let report = dispatcher.tick(now).unwrap();

    assert_eq!(report.considered, vec!["routine/daily-demo"]);
    assert_eq!(report.due.len(), 1);
    assert_eq!(report.dispatched.len(), 1);
    assert_eq!(report.dispatched[0].admission, "applied");
    assert_eq!(
        report.dispatched[0].outcome.as_ref().unwrap().status,
        RunStatus::Completed
    );
    // One run actually executed.
    assert_eq!(runner_runs.lock().unwrap().len(), 1);
    // The ledger carries exactly one invocation, with the gate delivery plus
    // the outcome delivery merged into it.
    let invocations = ledger.list().unwrap();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].routine_ref.as_str(), "routine/daily-demo");
    assert_eq!(invocations[0].provider_deliveries.len(), 2);
    // The trigger observation carries the Routine's schedule trigger, and the
    // payload packet rode into the run (A-1 discipline applied to schedules).
    assert!(matches!(
        invocations[0].trigger,
        RoutineTrigger::Schedule { .. }
    ));
    let run_request = runner_runs.lock().unwrap()[0].clone();
    assert!(run_request.prompt.contains("routine/daily-demo"));
    assert!(run_request
        .observation_payload
        .unwrap()
        .get("occurrence_ref")
        .is_some());
}

/// The same window replays nothing: delivered suppression is exactly-once,
/// across fresh dispatcher instances (gateway restart) and repeated ticks.
#[test]
fn replayed_windows_and_restarts_never_double_run() {
    let (_dir, home, _store, ledger, _routine) = world();
    let due = 1_790_016_000_000_i64;
    let now = due + 10_000;
    let runner_runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let make_runner = || RecordingRunner {
        runs: runner_runs.clone(),
        outcome: RunStatus::Completed,
    };

    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        make_runner(),
    )
    .tick(now)
    .unwrap();
    assert_eq!(report.dispatched.len(), 1);

    // Second tick, same process.
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        make_runner(),
    )
    .tick(now + 1000)
    .unwrap();
    assert!(report.dispatched.is_empty());
    assert_eq!(report.suppressed.len(), 1);

    // Gateway restart: a brand-new dispatcher instance replays the window.
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        make_runner(),
    )
    .tick(now + 2000)
    .unwrap();
    assert!(report.dispatched.is_empty());
    assert_eq!(report.suppressed.len(), 1);

    // Exactly one executed run and exactly one ledger invocation, ever.
    assert_eq!(runner_runs.lock().unwrap().len(), 1);
    assert_eq!(ledger.list().unwrap().len(), 1);
}

/// A clock moved backward suppresses every due occurrence until the wall clock
/// catches up (simulated sleep across a backwards step).
#[test]
fn clock_moved_back_suppresses_until_the_clock_catches_up() {
    let (_dir, home, _store, ledger, _routine) = world();
    let due = 1_790_016_000_000_i64;
    let now = due + 10_000;
    let runner_runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let runner = RecordingRunner {
        runs: runner_runs.clone(),
        outcome: RunStatus::Completed,
    };

    // Tick 1 establishes the watermark at `now`.
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        runner,
    )
    .tick(now)
    .unwrap();
    assert_eq!(report.dispatched.len(), 1);

    // The clock steps back (a simulated sleep misadventure). Everything is
    // suppressed and the report says why.
    let earlier = now - 3 * DAY_MS;
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        RecordingRunner {
            runs: runner_runs.clone(),
            outcome: RunStatus::Completed,
        },
    )
    .tick(earlier)
    .unwrap();
    assert!(report.clock_moved_back);
    assert!(report.dispatched.is_empty());
    assert_eq!(ledger.list().unwrap().len(), 1);
    assert_eq!(runner_runs.lock().unwrap().len(), 1);
}

/// Catch-up is the Routine's own setting: skip-missed never fires a missed
/// run, bounded catch-up fires it once inside its window.
#[test]
fn catch_up_setting_decides_missed_runs() {
    // skip-missed: the occurrence came due 90 seconds ago, outside the tick's
    // on-time window — suppressed forever, never run.
    let (_dir, home, _store, ledger, _routine) = world();
    let now = 1_790_016_000_000_i64 + 90 * 1000;
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", now - 90 * 1000),
        RecordingRunner {
            runs: std::sync::Arc::default(),
            outcome: RunStatus::Completed,
        },
    )
    .tick(now)
    .unwrap();
    assert!(report.dispatched.is_empty());
    assert!(ledger.list().unwrap().is_empty());

    // bounded catch-up: the same missed run, within a 2-hour window, fires.
    let (_dir, home, _store, ledger, _routine) = world();
    let mut record = RoutineStore::new(home.clone())
        .get(&r("routine/daily-demo"))
        .unwrap();
    record.time_schedule.as_mut().unwrap().catch_up = Some(CatchUpPolicy::Bounded {
        within_ms: 2 * 60 * 60 * 1000,
        max: 1,
    });
    RoutineStore::new(home.clone()).put(record).unwrap();
    let now = 1_790_016_000_000_i64 + 90 * 1000;
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", now - 90 * 1000),
        RecordingRunner {
            runs: std::sync::Arc::default(),
            outcome: RunStatus::Completed,
        },
    )
    .tick(now)
    .unwrap();
    assert_eq!(report.dispatched.len(), 1);
    assert_eq!(ledger.list().unwrap().len(), 1);
}

/// A Method revision change flips the Routine to StaleProof at the next tick;
/// the flip is persisted and no run is minted.
#[test]
fn method_drift_stales_proof_and_runs_nothing() {
    let (_dir, home, store, ledger, _routine) = world();
    let due = 1_790_016_000_000_i64;
    let now = due + 10_000;
    let mut resolver = FixtureMethodResolver {
        revisions: BTreeMap::new(),
    };
    // The catalogue now answers with a newer Method revision.
    resolver
        .revisions
        .insert("method:fixture".into(), rev("method-rev-2"));
    let dispatcher = RoutineDispatcher::new(
        home.clone(),
        FixtureOccurrences {
            table: fixture_daily_table("central:occurrence/daily-one", due),
            policy_revision: "policy-rev-1".into(),
        },
        resolver,
        RecordingRunner {
            runs: std::sync::Arc::default(),
            outcome: RunStatus::Completed,
        },
    );
    let report = dispatcher.tick(now).unwrap();
    assert!(report
        .stale_proofs
        .contains(&"routine/daily-demo".to_string()));
    assert!(report.dispatched.is_empty());
    assert_eq!(report.failures.len(), 1);
    // The stale flip is durable.
    let stored = store.get(&r("routine/daily-demo")).unwrap();
    assert_eq!(
        stored.routine.state,
        aikit_core::resource::routine::RoutineState::StaleProof
    );
    assert!(ledger.list().unwrap().is_empty());
}

/// Only Enabled Schedule Routines bound to the gateway dispatcher are
/// considered: disabled, draft and foreign-bound Routines are invisible to
/// the tick, and a foreign timer never becomes AIKit's work.
#[test]
fn the_tick_considers_only_enabled_gateway_bound_schedules() {
    let (_dir, home, store, ledger, _routine) = world();

    // A foreign-bound Routine (an unadopted import): never dispatched.
    let mut foreign_record = store.get(&r("routine/daily-demo")).unwrap();
    foreign_record
        .routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:openclaw-cron").unwrap(),
            provider_job_id: Some("job-1".into()),
            observed_state: RoutineSchedulerState::Active,
        })
        .unwrap();
    store.put(foreign_record).unwrap();
    let due = 1_790_016_000_000_i64;
    let now = due + 10_000;
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        RecordingRunner {
            runs: std::sync::Arc::default(),
            outcome: RunStatus::Completed,
        },
    )
    .tick(now)
    .unwrap();
    assert!(report.considered.is_empty());
    assert!(ledger.list().unwrap().is_empty());

    // Re-bind to the gateway and disable: still invisible to the tick.
    let mut record = store.get(&r("routine/daily-demo")).unwrap();
    record
        .routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
            provider_job_id: None,
            observed_state: RoutineSchedulerState::Planned,
        })
        .unwrap();
    record.routine.disable();
    store.put(record).unwrap();
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        RecordingRunner {
            runs: std::sync::Arc::default(),
            outcome: RunStatus::Completed,
        },
    )
    .tick(now)
    .unwrap();
    assert!(report.considered.is_empty());
}

/// A failed run is recorded as failed: the ledger keeps the invocation, the
/// outcome delivery says so, and no retry mints work.
#[test]
fn failed_runs_are_recorded_and_never_retried_into_new_work() {
    let (_dir, home, _store, ledger, _routine) = world();
    let due = 1_790_016_000_000_i64;
    let now = due + 10_000;
    let runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        RecordingRunner {
            runs: runs.clone(),
            outcome: RunStatus::Failed,
        },
    )
    .tick(now)
    .unwrap();
    assert_eq!(
        report.dispatched[0].outcome.as_ref().unwrap().status,
        RunStatus::Failed
    );
    let invocations = ledger.list().unwrap();
    assert_eq!(invocations.len(), 1);
    let deliveries = &invocations[0].provider_deliveries;
    assert_eq!(deliveries.len(), 2);
    // The outcome delivery is distinct from the gate delivery.
    assert_ne!(deliveries[0].delivery_ref, deliveries[1].delivery_ref);
    // And the next tick does not retry it.
    let report = dispatcher(
        &home,
        fixture_daily_table("central:occurrence/daily-one", due),
        RecordingRunner {
            runs: runs.clone(),
            outcome: RunStatus::Failed,
        },
    )
    .tick(now + 1000)
    .unwrap();
    assert!(report.dispatched.is_empty());
    assert_eq!(runs.lock().unwrap().len(), 1);
}

// -- the event path --------------------------------------------------------

/// A matching Enabled Event Routine observes the hook event and is admitted;
/// the payload packet is hashed into the observation ref and rides into the
/// run.
#[test]
fn matching_event_routines_observe_and_admit_with_the_payload() {
    let dir = tempfile::tempdir().unwrap();
    let home = AikitHome::at(dir.path().join("home"));
    let method = fixture_method("method-rev-1");
    let routine = Routine::new(
        r("routine/event-demo"),
        SourceRef::parse("source:aikit:routines/routine/event-demo").unwrap(),
        None,
        "Event demo",
        "",
        &method,
        proof(&method),
        RoutineTrigger::Event {
            event_ref: "aikit.routine-event/v1:claude:Stop".into(),
        },
        authority(),
        None,
        vec![],
    )
    .unwrap();
    let mut record = StoredRoutine::new(routine, None, None).unwrap();
    record
        .routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
            provider_job_id: None,
            observed_state: RoutineSchedulerState::Planned,
        })
        .unwrap();
    record
        .routine
        .enable(&fixture_method("method-rev-1"))
        .unwrap();
    RoutineStore::new(home.clone()).put(record).unwrap();

    let runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let dispatcher = dispatcher(
        &home,
        BTreeMap::new(),
        RecordingRunner {
            runs: runs.clone(),
            outcome: RunStatus::Completed,
        },
    );

    let payload = serde_json::json!({ "session_id": "abc", "reason": "turn-end" });
    let dispatched = dispatcher
        .event_pass("claude", "Stop", &payload, 1_790_016_000_000)
        .unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].admission, "applied");
    assert_eq!(runs.lock().unwrap().len(), 1);
    // The payload packet rode into the dispatched run (A-1).
    let request = runs.lock().unwrap()[0].clone();
    let packet = request.observation_payload.unwrap();
    assert_eq!(packet["client"], "claude");
    assert_eq!(packet["payload"]["session_id"], "abc");

    // The same event again is already admitted — one observation, one
    // invocation, no re-run.
    let dispatched = dispatcher
        .event_pass("claude", "Stop", &payload, 1_790_016_000_000 + 5)
        .unwrap();
    assert_eq!(dispatched[0].admission, "already-admitted");
    assert_eq!(runs.lock().unwrap().len(), 1);
    assert_eq!(
        RoutineInvocationStore::new(home.clone())
            .list()
            .unwrap()
            .len(),
        1
    );

    // A different payload hashes to a different observation ref: a distinct
    // occurrence is a distinct invocation.
    let other = serde_json::json!({ "session_id": "xyz" });
    let dispatched = dispatcher
        .event_pass("claude", "Stop", &other, 1_790_016_000_000 + 10)
        .unwrap();
    assert_eq!(dispatched[0].admission, "applied");
    assert_eq!(runs.lock().unwrap().len(), 2);
}

/// Non-matching and Disabled Event Routines observe nothing.
#[test]
fn non_matching_and_disabled_event_routines_observe_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let home = AikitHome::at(dir.path().join("home"));
    let method = fixture_method("method-rev-1");
    let enabled_routine = Routine::new(
        r("routine/event-match"),
        SourceRef::parse("source:aikit:routines/routine/event-match").unwrap(),
        None,
        "Event match",
        "",
        &method,
        proof(&method),
        RoutineTrigger::Event {
            event_ref: "aikit.routine-event/v1:claude:Stop".into(),
        },
        authority(),
        None,
        vec![],
    )
    .unwrap();
    let mut record = StoredRoutine::new(enabled_routine, None, None).unwrap();
    record
        .routine
        .enable(&fixture_method("method-rev-1"))
        .unwrap();
    record.routine.disable();
    let store = RoutineStore::new(home.clone());
    store.put(record).unwrap();

    let filtered_routine = Routine::new(
        r("routine/event-filtered"),
        SourceRef::parse("source:aikit:routines/routine/event-filtered").unwrap(),
        None,
        "Event filtered",
        "",
        &method,
        proof(&method),
        RoutineTrigger::Event {
            event_ref: "aikit.routine-event/v1:claude:Stop:project/demo".into(),
        },
        authority(),
        None,
        vec![],
    )
    .unwrap();
    let mut filtered = StoredRoutine::new(filtered_routine, None, None).unwrap();
    filtered
        .routine
        .set_scheduler_binding(RoutineSchedulerBinding {
            provider: ProviderRef::parse("provider:aikit-gateway").unwrap(),
            provider_job_id: None,
            observed_state: RoutineSchedulerState::Planned,
        })
        .unwrap();
    filtered
        .routine
        .enable(&fixture_method("method-rev-1"))
        .unwrap();
    store.put(filtered).unwrap();

    let runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let dispatcher = dispatcher(
        &home,
        BTreeMap::new(),
        RecordingRunner {
            runs: runs.clone(),
            outcome: RunStatus::Completed,
        },
    );

    // Wrong kind and wrong client observe nothing.
    let payload = serde_json::json!({ "note": "unrelated" });
    assert!(dispatcher
        .event_pass("claude", "PreToolUse", &payload, 1)
        .unwrap()
        .is_empty());
    assert!(dispatcher
        .event_pass("zcode", "Stop", &payload, 1)
        .unwrap()
        .is_empty());
    // Disabled routines observe nothing even on a match.
    assert!(dispatcher
        .event_pass("claude", "Stop", &payload, 1)
        .unwrap()
        .is_empty());
    // The filtered routine requires its filter in the payload.
    assert!(dispatcher
        .event_pass("claude", "Stop", &payload, 1)
        .unwrap()
        .is_empty());
    let matching = serde_json::json!({ "project": "project/demo" });
    let dispatched = dispatcher
        .event_pass("claude", "Stop", &matching, 1)
        .unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(runs.lock().unwrap().len(), 1);
}

// -- run-now ----------------------------------------------------------------

/// run-now is a manual observation through the same gate: admitted, run,
/// recorded — and refused outright for a Routine without authority.
#[test]
fn run_now_passes_the_same_gate() {
    let (_dir, home, store, ledger, _routine) = world();
    let runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let dispatcher = dispatcher(
        &home,
        BTreeMap::new(),
        RecordingRunner {
            runs: runs.clone(),
            outcome: RunStatus::Completed,
        },
    );
    let dispatch = dispatcher
        .run_now(&r("routine/daily-demo"), 1_790_016_000_000)
        .unwrap();
    assert_eq!(dispatch.admission, "applied");
    assert_eq!(runs.lock().unwrap().len(), 1);
    let invocations = ledger.list().unwrap();
    assert_eq!(invocations.len(), 1);
    assert!(matches!(
        invocations[0].trigger,
        RoutineTrigger::Schedule { .. }
    ));

    // A disabled Routine cannot run-now: the gate refuses, nothing runs.
    let mut record = store.get(&r("routine/daily-demo")).unwrap();
    record.routine.disable();
    store.put(record).unwrap();
    let before = runs.lock().unwrap().len();
    assert!(dispatcher
        .run_now(&r("routine/daily-demo"), 1_790_016_000_001)
        .is_err());
    assert_eq!(runs.lock().unwrap().len(), before);
}
