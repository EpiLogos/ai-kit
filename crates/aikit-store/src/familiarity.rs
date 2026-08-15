//! Durable familiarity evidence on the existing SQLite event log.
//!
//! Familiarity is derived state, so persistence records observations/resets rather
//! than a mutable ranking table. Replaying the ordered event stream reconstructs
//! the current learned-accessibility store. Unknown familiarity schemas invalidate
//! the learned replay explicitly; unrelated AIKit events and canonical Resource
//! identities are untouched.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use aikit_core::{
    AikitError, FamiliarityObservation, FamiliarityStore, ForgetScope, Result,
    FAMILIARITY_SCHEMA_VERSION,
};

use crate::{Event, SqliteStore};

pub const FAMILIARITY_OBSERVATION_EVENT: &str = "familiarity.observed";
pub const FAMILIARITY_RESET_EVENT: &str = "familiarity.reset";

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

/// Append one learned-use observation at the observation's own deterministic
/// timestamp. The durable Event gets its normal AIKit EventId; the payload retains
/// the caller's trace/event observation identity as well.
pub fn append_familiarity_observation(
    store: &SqliteStore,
    observation: FamiliarityObservation,
) -> Result<()> {
    let at = DateTime::<Utc>::from_timestamp_millis(observation.observed_at_ms as i64).ok_or_else(
        || {
            AikitError::new(
                "familiarity.invalid_observation_time",
                format!(
                    "{} is not a representable UTC millisecond timestamp",
                    observation.observed_at_ms
                ),
            )
        },
    )?;
    let payload = serde_json::to_value(StoredObservation {
        schema: FAMILIARITY_SCHEMA_VERSION.to_string(),
        observation,
    })
    .map_err(encode_error)?;
    store.append_event(&Event::new(FAMILIARITY_OBSERVATION_EVENT, payload, at))
}

/// Append a scoped forgetting event. Forgetting is itself durable evidence so a
/// replay after restart does not resurrect learned influence the user reset.
pub fn append_familiarity_reset(
    store: &SqliteStore,
    scope: ForgetScope,
    at: DateTime<Utc>,
) -> Result<()> {
    let payload = serde_json::to_value(StoredReset {
        schema: FAMILIARITY_SCHEMA_VERSION.to_string(),
        scope,
    })
    .map_err(encode_error)?;
    store.append_event(&Event::new(FAMILIARITY_RESET_EVENT, payload, at))
}

/// Rebuild current learned accessibility from the append-only event stream.
///
/// Familiarity events are intentionally sparse among all AIKit events; unrelated
/// kinds are skipped. A schema mismatch returns `Invalidated` immediately instead
/// of mixing old and new ranking semantics.
pub fn replay_familiarity(store: &SqliteStore) -> Result<FamiliarityReplay> {
    let events = store.events_since(0)?;
    let mut learned = FamiliarityStore::new();
    let mut observation_events = 0usize;
    let mut reset_events = 0usize;
    let mut observations_removed_by_resets = 0usize;

    for event in events {
        match event.event.kind.as_str() {
            FAMILIARITY_OBSERVATION_EVENT => {
                let stored: StoredObservation = serde_json::from_value(event.event.payload.clone())
                    .map_err(|error| decode_error(&event.event.kind, &event.event.id.to_string(), error))?;
                if stored.schema != FAMILIARITY_SCHEMA_VERSION {
                    return Ok(invalidated(
                        stored.schema,
                        &event.event.kind,
                        &event.event.id.to_string(),
                    ));
                }
                learned.record(stored.observation)?;
                observation_events += 1;
            }
            FAMILIARITY_RESET_EVENT => {
                let stored: StoredReset = serde_json::from_value(event.event.payload.clone())
                    .map_err(|error| decode_error(&event.event.kind, &event.event.id.to_string(), error))?;
                if stored.schema != FAMILIARITY_SCHEMA_VERSION {
                    return Ok(invalidated(
                        stored.schema,
                        &event.event.kind,
                        &event.event.id.to_string(),
                    ));
                }
                observations_removed_by_resets += learned.forget(&stored.scope);
                reset_events += 1;
            }
            _ => {}
        }
    }

    Ok(FamiliarityReplay::Loaded {
        store: learned,
        observation_events,
        reset_events,
        observations_removed_by_resets,
    })
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

fn encode_error(error: serde_json::Error) -> AikitError {
    AikitError::new(
        "familiarity.event_encode_failed",
        format!("could not encode familiarity event payload: {error}"),
    )
}

fn decode_error(kind: &str, event_id: &str, error: serde_json::Error) -> AikitError {
    AikitError::new(
        "familiarity.event_decode_failed",
        format!("could not decode {kind} event {event_id}: {error}"),
    )
    .with("event_kind", kind)
    .with("event_id", event_id)
}
