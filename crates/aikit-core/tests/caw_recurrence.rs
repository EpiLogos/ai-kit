use aikit_core::{recurrence::*, ResourceRef, SourceRevision};
use std::collections::BTreeSet;
fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn reading() -> RecurrenceReading {
    let mut read = RecurrenceReading {
        routine_ref: r("routine/work"),
        schedule_ref: r("schedule/work"),
        time_policy_ref: r("central:time-policy:test"),
        time_policy_revision: SourceRevision::parse("rev/1").unwrap(),
        previous_now_unix_ms: Some(0),
        now_unix_ms: 300,
        enabled: true,
        catch_up: CatchUpPolicy::Bounded {
            within_ms: 400,
            max: 2,
        },
        resolved: vec![],
        delivered: BTreeSet::new(),
    };
    read.resolved = (1..=4)
        .map(|n| ResolvedOccurrence {
            routine_ref: read.routine_ref.clone(),
            schedule_ref: read.schedule_ref.clone(),
            occurrence_ref: r(&format!("occurrence/{n}")),
            due_unix_ms: n * 100,
            time_policy_ref: read.time_policy_ref.clone(),
            time_policy_revision: read.time_policy_revision.clone(),
        })
        .collect();
    read
}
#[test]
fn bounded_catchup_restart_and_disabled_do_not_mint_effects() {
    let mut r = reading();
    let p = plan_recurrence(&r).unwrap();
    assert_eq!(
        p.due.iter().map(|o| o.due_unix_ms).collect::<Vec<_>>(),
        vec![200, 300]
    );
    assert_eq!(p.next_due_unix_ms, Some(400));
    assert!(!p.effects_performed);
    r.delivered
        .extend(p.due.iter().map(|o| o.occurrence_ref.clone()));
    assert!(plan_recurrence(&r)
        .unwrap()
        .due
        .iter()
        .all(|o| o.due_unix_ms == 100));
    r.enabled = false;
    assert!(plan_recurrence(&r).unwrap().due.is_empty());
}
#[test]
fn changed_or_mixed_owner_policy_is_refused() {
    let mut r = reading();
    r.resolved[0].time_policy_revision = SourceRevision::parse("rev/stale").unwrap();
    assert!(plan_recurrence(&r).is_err());
    let mut r = reading();
    let mut drift = r.resolved[0].clone();
    drift.due_unix_ms += 1;
    r.resolved.push(drift);
    assert!(plan_recurrence(&r).is_err());
}
#[test]
fn owner_resolved_fold_instants_keep_distinct_identity_without_local_calendar_guessing() {
    let mut r = reading();
    r.resolved.truncate(2);
    let a = occurrence_delivery_ref(&r.resolved[0]).unwrap();
    let b = occurrence_delivery_ref(&r.resolved[1]).unwrap();
    assert_ne!(a, b);
    r.resolved[0].time_policy_revision = SourceRevision::parse("rev/new").unwrap();
    assert_eq!(a, occurrence_delivery_ref(&r.resolved[0]).unwrap());
}
#[test]
fn backward_clock_and_extreme_clock_arithmetic_are_bounded() {
    let mut r = reading();
    r.previous_now_unix_ms = Some(500);
    assert!(plan_recurrence(&r).unwrap().clock_moved_back);
    assert!(plan_recurrence(&r).unwrap().due.is_empty());
    r.previous_now_unix_ms = None;
    r.now_unix_ms = i64::MAX;
    r.resolved[0].due_unix_ms = i64::MIN;
    assert!(plan_recurrence(&r).unwrap().due.is_empty());
}
#[test]
fn latest_and_skip_are_explicit_not_implicit_day_actions() {
    let mut r = reading();
    r.catch_up = CatchUpPolicy::Latest { within_ms: 300 };
    assert_eq!(plan_recurrence(&r).unwrap().due.len(), 1);
    r.catch_up = CatchUpPolicy::SkipMissed;
    assert_eq!(plan_recurrence(&r).unwrap().due[0].due_unix_ms, 300);
    r.now_unix_ms = 301;
    assert!(plan_recurrence(&r).unwrap().due.is_empty());
    r.resolved.clear();
    assert!(plan_recurrence(&r).unwrap().due.is_empty());
}
