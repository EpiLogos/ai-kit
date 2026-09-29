//! The Routine schedule record: a declarative trigger time-shape, never a
//! calendar authority.
//!
//! `aikit.time-schedule/v1` names *what shape of time* a Schedule-triggered
//! Routine recurs over. It never resolves instants: Central's civil-time policy
//! owns all calendar meaning, and the dispatcher asks
//! `central.time.occurrences` for owner-resolved occurrences. AIKit validates
//! the record, keeps it in the Routine record, and passes the schedule through
//! to Central verbatim. A one-shot (`once`) expresses a conversational timer —
//! exactly one occurrence, after which the resolution is empty.

use serde::{Deserialize, Serialize};

use crate::recurrence::CatchUpPolicy;
use crate::resource::{ResourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const TIME_SCHEDULE_VERSION: &str = "aikit.time-schedule/v1";

/// The time-shapes a Routine's schedule trigger may declare. The grammar is
/// owned with Central: these are exactly the shapes `central.time.occurrences`
/// resolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScheduleShape {
    /// Local wall-clock recurrence at `hh:mm` in the policy timezone.
    Daily { time: String },
    /// 5-field cron over the policy timezone's local wall time.
    Cron { expression: String },
    /// Fixed-interval recurrence anchored at Unix-epoch multiples, so a slid
    /// horizon never mints a second series.
    Every { interval_ms: u64 },
    /// Exactly one occurrence, named as a Unix instant or an RFC 3339
    /// timestamp. After delivery the resolution is empty.
    Once {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        due_unix_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rfc3339: Option<String>,
    },
}

impl ScheduleShape {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Daily { time } => {
                parse_hh_mm(time)?;
            }
            Self::Cron { expression } => {
                let fields = expression.split_whitespace().count();
                if fields != 5 {
                    return Err(AikitError::new(
                        "schedule.cron_field_count",
                        format!(
                            "cron expression must have exactly 5 fields (minute hour day-of-month month day-of-week), got {fields}"
                        ),
                    ));
                }
            }
            Self::Every { interval_ms } => {
                if *interval_ms == 0 {
                    return Err(AikitError::new(
                        "schedule.interval_zero",
                        "every interval_ms must be a positive integer",
                    ));
                }
            }
            Self::Once {
                due_unix_ms,
                rfc3339,
            } => match (due_unix_ms, rfc3339) {
                (Some(_), None) => {}
                (None, Some(raw)) => {
                    raw.parse::<jiff::Timestamp>().map_err(|error| {
                        AikitError::new(
                            "schedule.invalid_timestamp",
                            format!("once rfc3339 `{raw}` is not an RFC 3339 timestamp: {error}"),
                        )
                    })?;
                }
                _ => {
                    return Err(AikitError::new(
                        "schedule.once_ambiguous",
                        "once requires exactly one of due_unix_ms or rfc3339",
                    ))
                }
            },
        }
        Ok(())
    }

    /// The due instant of a `once` shape in Unix milliseconds. Other shapes
    /// have none — Central resolves their instants.
    pub fn once_due_unix_ms(&self) -> Result<Option<i64>> {
        match self {
            Self::Once {
                due_unix_ms,
                rfc3339,
            } => {
                if let Some(due) = due_unix_ms {
                    return Ok(Some(*due));
                }
                let raw = rfc3339.as_deref().ok_or_else(|| {
                    AikitError::new(
                        "schedule.once_ambiguous",
                        "once requires exactly one of due_unix_ms or rfc3339",
                    )
                })?;
                let timestamp: jiff::Timestamp = raw.parse().map_err(|error| {
                    AikitError::new(
                        "schedule.invalid_timestamp",
                        format!("once rfc3339 `{raw}` is not an RFC 3339 timestamp: {error}"),
                    )
                })?;
                // jiff's `as_millisecond` is the whole instant in Unix
                // milliseconds; `as_second` * 1000 would double-count it.
                Ok(Some(timestamp.as_millisecond()))
            }
            _ => Ok(None),
        }
    }

    /// The wire form Central's `central.time.occurrences` action consumes.
    /// The schedule passes through verbatim — Central owns the calendar.
    /// Serialization of this plain-data shape cannot fail; `Value::Null` is
    /// unreachable in practice and callers re-validate before dispatch.
    pub fn to_occurrence_request_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

fn parse_hh_mm(raw: &str) -> Result<()> {
    let invalid = || {
        AikitError::new(
            "schedule.invalid_daily_time",
            format!("daily time `{raw}` must be a valid hh:mm 24-hour wall time"),
        )
    };
    let Some((hour, minute)) = raw.split_once(':') else {
        return Err(invalid());
    };
    let hour: u32 = hour.parse().map_err(|_| invalid())?;
    let minute: u32 = minute.parse().map_err(|_| invalid())?;
    if hour > 23 || minute > 59 {
        return Err(invalid());
    }
    Ok(())
}

/// The durable schedule record carried inside a Routine record: the identity of
/// the schedule, its time-shape, and the Routine-level catch-up setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRecord {
    pub schema: String,
    pub schedule_ref: ResourceRef,
    pub schedule: ScheduleShape,
    /// Routine-level catch-up policy; the planner default is `skip-missed`.
    /// A clock moved backward is always suppressed regardless of this setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catch_up: Option<CatchUpPolicy>,
}

impl ScheduleRecord {
    /// Assemble and validate one schedule record.
    pub fn new(
        schedule_ref: ResourceRef,
        schedule: ScheduleShape,
        catch_up: Option<CatchUpPolicy>,
    ) -> Result<Self> {
        let record = Self {
            schema: TIME_SCHEDULE_VERSION.into(),
            schedule_ref,
            schedule,
            catch_up,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != TIME_SCHEDULE_VERSION {
            return Err(AikitError::new(
                "schedule.unsupported_version",
                format!(
                    "schedule record must use the public {TIME_SCHEDULE_VERSION} contract, found {}",
                    self.schema
                ),
            ));
        }
        self.schedule.validate()?;
        if let Some(CatchUpPolicy::Bounded { max, .. }) = self.catch_up {
            if max == 0 || max > 4096 {
                return Err(AikitError::new(
                    "schedule.catch_up_bound",
                    "bounded catch-up maximum must be 1-4096",
                ));
            }
        }
        Ok(())
    }

    /// The catch-up policy the planner sees: the declared setting or the
    /// `skip-missed` default.
    pub fn effective_catch_up(&self) -> CatchUpPolicy {
        self.catch_up.unwrap_or(CatchUpPolicy::SkipMissed)
    }
}

/// The event-trigger reference grammar: a stable, readable string encoding of
/// `aikit.routine-event/v1` `{client, kind, filter}`. The hook dispatcher
/// derives the same string from the observed event, so matching is exact.
pub fn routine_event_ref(client: &str, kind: &str, filter: Option<&str>) -> Result<ResourceRef> {
    if client.trim().is_empty() || kind.trim().is_empty() {
        return Err(AikitError::new(
            "routine.event_ref_empty",
            "event trigger requires a non-empty client and hook kind",
        ));
    }
    if client.contains(':') || kind.contains(':') {
        return Err(AikitError::new(
            "routine.event_ref_unrepresentable",
            "event client and kind must not contain `:`",
        ));
    }
    match filter {
        None => ResourceRef::parse(format!("aikit.routine-event/v1:{client}:{kind}")),
        Some(filter) => {
            ResourceRef::parse(format!("aikit.routine-event/v1:{client}:{kind}:{filter}"))
        }
    }
}

/// The revision of a Routine's own source record: a content hash over the exact
/// stored body, so any mutation moves the revision.
pub fn routine_source_revision(body: &impl Serialize) -> Result<SourceRevision> {
    let bytes = serde_json::to_vec(body)
        .map_err(|error| AikitError::new("routine.revision_encoding", error.to_string()))?;
    SourceRevision::parse(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_shapes_validate() {
        ScheduleShape::Daily {
            time: "06:00".into(),
        }
        .validate()
        .unwrap();
        ScheduleShape::Cron {
            expression: "30 1 * * *".into(),
        }
        .validate()
        .unwrap();
        ScheduleShape::Every {
            interval_ms: 900_000,
        }
        .validate()
        .unwrap();
        ScheduleShape::Once {
            due_unix_ms: Some(1_789_900_000_000),
            rfc3339: None,
        }
        .validate()
        .unwrap();
        ScheduleShape::Once {
            due_unix_ms: None,
            rfc3339: Some("2026-09-23T18:00:00+01:00".into()),
        }
        .validate()
        .unwrap();
        for shape in [
            ScheduleShape::Daily {
                time: "24:00".into(),
            },
            ScheduleShape::Daily {
                time: "0600".into(),
            },
            ScheduleShape::Cron {
                expression: "* * * *".into(),
            },
            ScheduleShape::Every { interval_ms: 0 },
            ScheduleShape::Once {
                due_unix_ms: None,
                rfc3339: None,
            },
            ScheduleShape::Once {
                due_unix_ms: Some(1),
                rfc3339: Some("2026-09-23T18:00:00+01:00".into()),
            },
        ] {
            assert!(shape.validate().is_err(), "{shape:?} must be refused");
        }
    }

    #[test]
    fn once_due_unix_ms_resolves_from_either_field() {
        assert_eq!(
            ScheduleShape::Once {
                due_unix_ms: Some(1_789_900_000_000),
                rfc3339: None,
            }
            .once_due_unix_ms()
            .unwrap(),
            Some(1_789_900_000_000)
        );
        assert_eq!(
            ScheduleShape::Once {
                due_unix_ms: None,
                rfc3339: Some("2026-09-23T18:00:00+01:00".into()),
            }
            .once_due_unix_ms()
            .unwrap(),
            // 2026-09-23T17:00:00Z in Unix milliseconds.
            Some(1_790_182_800_000)
        );
    }

    #[test]
    fn record_validation_and_catch_up_default() {
        let record = ScheduleRecord::new(
            ResourceRef::parse("schedule:daily-0600").unwrap(),
            ScheduleShape::Daily {
                time: "06:00".into(),
            },
            None,
        )
        .unwrap();
        assert_eq!(record.schema, TIME_SCHEDULE_VERSION);
        assert_eq!(record.effective_catch_up(), CatchUpPolicy::SkipMissed);
        let bad = ScheduleRecord {
            schema: "aikit.time-schedule/v0".into(),
            schedule_ref: ResourceRef::parse("schedule:x").unwrap(),
            schedule: ScheduleShape::Every {
                interval_ms: 900_000,
            },
            catch_up: None,
        };
        assert_eq!(
            bad.validate().unwrap_err().code(),
            "schedule.unsupported_version"
        );
        let unbounded = ScheduleRecord::new(
            ResourceRef::parse("schedule:x").unwrap(),
            ScheduleShape::Every {
                interval_ms: 900_000,
            },
            Some(CatchUpPolicy::Bounded {
                within_ms: 1,
                max: 5000,
            }),
        );
        assert_eq!(unbounded.unwrap_err().code(), "schedule.catch_up_bound");
    }

    #[test]
    fn event_ref_grammar_is_stable_and_exact() {
        assert_eq!(
            routine_event_ref("claude", "PreToolUse", None)
                .unwrap()
                .as_str(),
            "aikit.routine-event/v1:claude:PreToolUse"
        );
        assert_eq!(
            routine_event_ref("zcode", "SessionStart", Some("project/demo"))
                .unwrap()
                .as_str(),
            "aikit.routine-event/v1:zcode:SessionStart:project/demo"
        );
        assert!(routine_event_ref("", "PreToolUse", None).is_err());
        assert!(routine_event_ref("a:b", "PreToolUse", None).is_err());
    }

    #[test]
    fn schedule_passes_through_to_central_unchanged() {
        let shape = ScheduleShape::Daily {
            time: "01:30".into(),
        };
        let value = shape.to_occurrence_request_value();
        assert_eq!(value["kind"], "daily");
        assert_eq!(value["time"], "01:30");
    }
}
