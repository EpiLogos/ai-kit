use chrono::{TimeZone, Utc};

use aikit_core::{
    FamiliarityContext, FamiliarityObservation, ForgetScope, ResourceRef,
    DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,
};
use aikit_store::{
    append_familiarity_observation, append_familiarity_reset, replay_familiarity, Event,
    FamiliarityReplay, SqliteStore, FAMILIARITY_OBSERVATION_EVENT,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn observations_and_resets_replay_across_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("aikit.db");
    let destination = r("knowledge-node/auth");
    let route_destination = r("knowledge-node/session");
    let context = FamiliarityContext {
        project: Some(r("project/app")),
        actor: Some(r("agent/research")),
        agency: None,
        focus: Some("auth".into()),
    };

    {
        let store = SqliteStore::open(&db).unwrap();
        // Unrelated durable events are ignored by familiarity replay.
        store
            .append_event(&Event::new(
                "project.opened",
                serde_json::json!({"project":"project/app"}),
                Utc.timestamp_millis_opt(500).single().unwrap(),
            ))
            .unwrap();
        append_familiarity_observation(
            &store,
            FamiliarityObservation::destination(
                "trace/auth/1",
                destination.clone(),
                context.clone(),
                1_000,
            ),
        )
        .unwrap();
        append_familiarity_observation(
            &store,
            FamiliarityObservation::destination(
                "trace/session/1",
                route_destination.clone(),
                context.clone(),
                2_000,
            ),
        )
        .unwrap();
        append_familiarity_reset(
            &store,
            ForgetScope::Destination(destination.clone()),
            Utc.timestamp_millis_opt(3_000).single().unwrap(),
        )
        .unwrap();
    }

    let reopened = SqliteStore::open(&db).unwrap();
    let replay = replay_familiarity(&reopened).unwrap();
    let FamiliarityReplay::Loaded {
        store,
        observation_events,
        reset_events,
        observations_removed_by_resets,
    } = replay
    else {
        panic!("current familiarity events should replay");
    };

    assert_eq!(observation_events, 2);
    assert_eq!(reset_events, 1);
    assert_eq!(observations_removed_by_resets, 1);
    assert!(store
        .assess_destination(
            &destination,
            &context,
            4_000,
            DEFAULT_FAMILIARITY_HALF_LIFE_MS,
        )
        .is_empty());
    assert_eq!(
        store
            .assess_destination(
                &route_destination,
                &context,
                4_000,
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            )
            .observations,
        1
    );
}

#[test]
fn unknown_persisted_schema_invalidates_the_whole_learned_replay_not_other_events() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("aikit.db")).unwrap();
    store
        .append_event(&Event::new(
            "project.opened",
            serde_json::json!({"project":"project/app"}),
            Utc.timestamp_millis_opt(500).single().unwrap(),
        ))
        .unwrap();
    store
        .append_event(&Event::new(
            FAMILIARITY_OBSERVATION_EVENT,
            serde_json::json!({
                "schema": "aikit.familiarity/v1",
                "observation": {
                    "observation_id": "trace/old/1",
                    "destination": "knowledge-node/auth",
                    "context": {},
                    "use_kind": {"kind":"destination"},
                    "observed_at_ms": 1000
                }
            }),
            Utc.timestamp_millis_opt(1_000).single().unwrap(),
        ))
        .unwrap();

    match replay_familiarity(&store).unwrap() {
        FamiliarityReplay::Invalidated {
            found_schema,
            event_kind,
            reason,
            ..
        } => {
            assert_eq!(found_schema, "aikit.familiarity/v1");
            assert_eq!(event_kind, FAMILIARITY_OBSERVATION_EVENT);
            assert!(reason.contains(FAMILIARITY_SCHEMA_VERSION));
        }
        FamiliarityReplay::Loaded { .. } => {
            panic!("unknown familiarity schema must not silently influence ranking")
        }
    }

    assert_eq!(store.events_since(0).unwrap().len(), 2);
}
