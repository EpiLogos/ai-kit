//! Deterministic recurrence delivery planning over owner-resolved instants.
//!
//! Central's time policy resolves local calendars/Day boundaries/DST. AIKit
//! receives those instants; it never invents a competing timezone or rollover
//! policy. A due occurrence is still not an authority grant or an executed task.
use crate::{AikitError, ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedOccurrence {
    pub routine_ref: ResourceRef,
    pub schedule_ref: ResourceRef,
    /// Opaque owner occurrence identity (distinct instants in a DST fold remain distinct).
    pub occurrence_ref: ResourceRef,
    pub due_unix_ms: i64,
    pub time_policy_ref: ResourceRef,
    pub time_policy_revision: SourceRevision,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum CatchUpPolicy {
    SkipMissed,
    Latest { within_ms: u64 },
    Bounded { within_ms: u64, max: usize },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecurrenceReading {
    pub routine_ref: ResourceRef,
    pub schedule_ref: ResourceRef,
    pub time_policy_ref: ResourceRef,
    pub time_policy_revision: SourceRevision,
    pub previous_now_unix_ms: Option<i64>,
    pub now_unix_ms: i64,
    pub enabled: bool,
    pub catch_up: CatchUpPolicy,
    pub resolved: Vec<ResolvedOccurrence>,
    pub delivered: BTreeSet<ResourceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurrencePlan {
    pub due: Vec<ResolvedOccurrence>,
    pub suppressed: Vec<ResourceRef>,
    pub clock_moved_back: bool,
    pub next_due_unix_ms: Option<i64>,
    pub effects_performed: bool,
}
/// Stable delivery identity is derived from the owner occurrence, not provider
/// process, wake id or a retry's policy revision. Retry/restart cannot mint work.
pub fn occurrence_delivery_ref(occurrence: &ResolvedOccurrence) -> Result<ResourceRef> {
    let bytes = serde_json::to_vec(&(
        &occurrence.routine_ref,
        &occurrence.schedule_ref,
        &occurrence.occurrence_ref,
    ))
    .map_err(|e| AikitError::new("recurrence.identity", e.to_string()))?;
    ResourceRef::parse(format!(
        "delivery/recurrence-{}",
        blake3::hash(&bytes).to_hex()
    ))
}
pub fn plan_recurrence(reading: &RecurrenceReading) -> Result<RecurrencePlan> {
    if reading.resolved.len() > 4096 {
        return Err(invalid(
            "Owner time reading exceeds the bounded occurrence limit",
        ));
    }
    let mut seen = BTreeMap::new();
    for occurrence in &reading.resolved {
        if occurrence.routine_ref != reading.routine_ref
            || occurrence.schedule_ref != reading.schedule_ref
            || occurrence.time_policy_ref != reading.time_policy_ref
            || occurrence.time_policy_revision != reading.time_policy_revision
        {
            return Err(invalid(
                "Mixed Routine, schedule or stale owner time-policy basis",
            ));
        }
        if let Some(old) = seen.insert(occurrence.occurrence_ref.clone(), occurrence) {
            if old != occurrence {
                return Err(invalid(
                    "One occurrence identity was assigned different instants/bases",
                ));
            }
        }
    }
    let backward = reading
        .previous_now_unix_ms
        .is_some_and(|old| reading.now_unix_ms < old);
    let mut eligible: Vec<_> = seen.into_values().collect();
    eligible.sort_by(|a, b| {
        a.due_unix_ms
            .cmp(&b.due_unix_ms)
            .then(a.occurrence_ref.cmp(&b.occurrence_ref))
    });
    let next_due_unix_ms = eligible
        .iter()
        .filter(|o| o.due_unix_ms > reading.now_unix_ms)
        .map(|o| o.due_unix_ms)
        .min();
    let (window, max) = match reading.catch_up {
        CatchUpPolicy::SkipMissed => (0, 4096),
        CatchUpPolicy::Latest { within_ms } => (within_ms, 1),
        CatchUpPolicy::Bounded { within_ms, max } => {
            if max == 0 || max > 4096 {
                return Err(invalid("Catch-up maximum must be 1–4096"));
            }
            (within_ms, max)
        }
    };
    let mut due = Vec::new();
    let mut suppressed = Vec::new();
    for occurrence in eligible
        .into_iter()
        .filter(|o| o.due_unix_ms <= reading.now_unix_ms)
    {
        let age = (reading.now_unix_ms as i128) - (occurrence.due_unix_ms as i128);
        if !reading.enabled
            || backward
            || reading.delivered.contains(&occurrence.occurrence_ref)
            || age > window as i128
        {
            suppressed.push(occurrence.occurrence_ref.clone());
        } else {
            due.push(occurrence.clone());
        }
    }
    if due.len() > max {
        let split = due.len() - max;
        suppressed.extend(due.drain(..split).map(|o| o.occurrence_ref));
    }
    Ok(RecurrencePlan {
        due,
        suppressed,
        clock_moved_back: backward,
        next_due_unix_ms,
        effects_performed: false,
    })
}
fn invalid(message: &str) -> AikitError {
    AikitError::new("recurrence.invalid_owner_reading", message)
}
