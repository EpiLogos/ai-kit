//! Durable familiarity evidence on AIKit's existing append-only event log.
//!
//! Familiarity is derived operational memory, not canonical resource state. We
//! therefore persist `resource-use` observations and explicit reset events in the
//! same `usage_events` stream that already backs History/usage evidence, then
//! reconstruct the learned-accessibility store by replay. Unknown familiarity
//! schemas explicitly invalidate only the learned replay; unrelated events and
//! canonical Resource identities remain untouched.

use std::collections::BTreeMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use aikit_core::{
    AikitError, FamiliarityObservation, FamiliarityStore, ForgetScope, Result,
    FAMILIARITY_SCHEMA_VERSION,
};

use crate::{Event, EventAction, Index, Timestamp};

pub const FAMILIARITY_OBSERVATION_EVENT: &str = "resource-use";
pub const FAMILIARITY_RESET_EVENT: &str = "familiarity-reset";
const FAMILIARITY_PAYLOAD_KEY: &str = "familiarity";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredObservation {
    schema: String,
    observation: FamiliarityObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredReset {
    schema: String,
    scope: ForgetScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamiliarityReplay {
    Loaded {
        store: FamiliarityStore,
        observation_events: usize,
        reset_events: usize,
        observations_removed_by_resets: usize,
    },
    Invalidated {
        found_schema: String,
        event_kind: String,
        event_id: String,
        reason: String,
    },
}

impl FamiliarityReplay {
    pub fn store(&self) -> Option<&FamiliarityStore> {
        match self {
            Self::Loaded { store, .. } => Some(store),
            Self::Invalidated { .. } => None,
        }
    }
}

/// Build the normal AIKit event corresponding to one learned-use observation.
///
/// The event gets its own monotonic `EventId`; the observation retains the
/// originating trace/event id in its payload. Only ResourceRefs and explicit
/// familiarity metadata are stored here — never prompt text or secret values.
pub fn familiarity_observation_event(observation: FamiliarityObservation) -> Result<Event> {
    let at = timestamp_from_ms(observation.observed_at_ms);
    let payload = encode(&StoredObservation {
        schema: FAMILIARITY_SCHEMA_VERSION.to_string(),
        observation,
    })?;
    let mut event = Event::new(EventAction::ResourceUse).at(at);
    event
        .arguments
        .insert(FAMILIARITY_PAYLOAD_KEY.to_string(), payload);
    Ok(event)
}

/// Build the event representing an explicit learned-ease reset.
pub fn familiarity_reset_event(scope: ForgetScope, at_ms: u64) -> Result<Event> {
    let payload = encode(&StoredReset {
        schema: FAMILIARITY_SCHEMA_VERSION.to_string(),
        scope,
    })?;
    let mut event = Event::new(EventAction::FamiliarityReset).at(timestamp_from_ms(at_ms));
    event
        .arguments
        .insert(FAMILIARITY_PAYLOAD_KEY.to_string(), payload);
    Ok(event)
}

/// Append one learned-use observation to the SQLite event stream.
///
/// Application paths that also own the JSONL sink should prefer building the
/// event with [`familiarity_observation_event`] and passing it through
/// `EventRecorder`; this helper is useful for store-level tests and migrations.
pub fn append_familiarity_observation(
    index: &Index,
    observation: FamiliarityObservation,
) -> Result<()> {
    index.record_event(&familiarity_observation_event(observation)?)
}

/// Append a scoped forgetting event. Replay after restart therefore cannot
/// resurrect learned influence that the user explicitly reset.
pub fn append_familiarity_reset(index: &Index, scope: ForgetScope, at_ms: u64) -> Result<()> {
    index.record_event(&familiarity_reset_event(scope, at_ms)?)
}

/// Rebuild current learned accessibility from the append-only event stream.
///
/// Only the two familiarity event kinds are queried, so ordinary Run/Apply/etc.
/// evidence remains entirely independent. A schema mismatch returns
/// `Invalidated` immediately instead of mixing old and new ranking semantics.
pub fn replay_familiarity(index: &Index) -> Result<FamiliarityReplay> {
    let mut stmt = index
        .conn()
        .prepare(
            "SELECT event_id, action, arguments
             FROM usage_events
             WHERE action IN (?1, ?2)
             ORDER BY timestamp_ns ASC, event_id ASC",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map(
            params![FAMILIARITY_OBSERVATION_EVENT, FAMILIARITY_RESET_EVENT],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(db_error)?;

    let mut learned = FamiliarityStore::new();
    let mut observation_events = 0usize;
    let mut reset_events = 0usize;
    let mut observations_removed_by_resets = 0usize;

    for row in rows {
        let (event_id, kind, arguments_json) = row.map_err(db_error)?;
        let arguments: BTreeMap<String, String> =
            serde_json::from_str(&arguments_json).map_err(|error| {
                decode_error(
                    &kind,
                    &event_id,
                    format!("invalid event arguments: {error}"),
                )
            })?;
        let payload = arguments.get(FAMILIARITY_PAYLOAD_KEY).ok_or_else(|| {
            decode_error(
                &kind,
                &event_id,
                format!("missing `{FAMILIARITY_PAYLOAD_KEY}` payload"),
            )
        })?;

        match kind.as_str() {
            FAMILIARITY_OBSERVATION_EVENT => {
                let stored: StoredObservation = serde_json::from_str(payload)
                    .map_err(|error| decode_error(&kind, &event_id, error.to_string()))?;
                if stored.schema != FAMILIARITY_SCHEMA_VERSION {
                    return Ok(invalidated(stored.schema, &kind, &event_id));
                }
                learned.record(stored.observation)?;
                observation_events += 1;
            }
            FAMILIARITY_RESET_EVENT => {
                let stored: StoredReset = serde_json::from_str(payload)
                    .map_err(|error| decode_error(&kind, &event_id, error.to_string()))?;
                if stored.schema != FAMILIARITY_SCHEMA_VERSION {
                    return Ok(invalidated(stored.schema, &kind, &event_id));
                }
                observations_removed_by_resets += learned.forget(&stored.scope);
                reset_events += 1;
            }
            _ => unreachable!("SQL query restricts familiarity event kinds"),
        }
    }

    Ok(FamiliarityReplay::Loaded {
        store: learned,
        observation_events,
        reset_events,
        observations_removed_by_resets,
    })
}

fn timestamp_from_ms(value: u64) -> Timestamp {
    let capped = value.min(i64::MAX as u64 / 1_000_000);
    Timestamp::from_nanos((capped as i64).saturating_mul(1_000_000))
}

fn invalidated(found_schema: String, event_kind: &str, event_id: &str) -> FamiliarityReplay {
    FamiliarityReplay::Invalidated {
        reason: format!(
            "learned accessibility event schema changed; expected {FAMILIARITY_SCHEMA_VERSION}; no familiarity evidence from this replay was applied"
        ),
        found_schema,
        event_kind: event_kind.to_string(),
        event_id: event_id.to_string(),
    }
}

fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| {
        AikitError::new(
            "familiarity.event_encode_failed",
            format!("could not encode familiarity event payload: {error}"),
        )
    })
}

fn decode_error(kind: &str, event_id: &str, detail: impl Into<String>) -> AikitError {
    AikitError::new(
        "familiarity.event_decode_failed",
        format!(
            "could not decode {kind} event {event_id}: {}",
            detail.into()
        ),
    )
    .with("event_kind", kind)
    .with("event_id", event_id)
}

fn db_error(error: rusqlite::Error) -> AikitError {
    AikitError::new(
        "familiarity.event_query_failed",
        format!("could not replay familiarity events: {error}"),
    )
}
