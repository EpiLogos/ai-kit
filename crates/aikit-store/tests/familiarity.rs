use aikit_core::resource::ResourceRef;
use aikit_core::{
    FamiliarityContext, FamiliarityObservation, ForgetScope, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
    FAMILIARITY_SCHEMA_VERSION,
};
use aikit_store::{
    append_familiarity_observation, append_familiarity_reset, replay_familiarity, Event,
    EventAction, FamiliarityReplay, Index, Timestamp, FAMILIARITY_OBSERVATION_EVENT,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn observations_and_resets_replay_across_index_reopen() {
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
        let index = Index::open(&db).unwrap();
        // Unrelated durable events are ignored by familiarity replay.
        index
            .record_event(
                &Event::new(EventAction::RegistrySync)
                    .at(Timestamp::from_nanos(500_000_000)),
            )
            .unwrap();
        append_familiarity_observation(
            &index,
            FamiliarityObservation::destination(
                "trace/auth/1",
                destination.clone(),
                context.clone(),
                1_000,
            ),
        )
        .unwrap();
        append_familiarity_observation(
            &index,
            FamiliarityObservation::destination(
                "trace/session/1",
                route_destination.clone(),
                context.clone(),
                2_000,
            ),
        )
        .unwrap();
        append_familiarity_reset(&index, ForgetScope::Destination(destination.clone()), 3_000)
            .unwrap();
    }

    let reopened = Index::open(&db).unwrap();
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
fn unknown_persisted_schema_invalidates_only_learned_replay() {
    let dir = tempfile::tempdir().unwrap();
    let index = Index::open(&dir.path().join("aikit.db")).unwrap();
    index
        .record_event(
            &Event::new(EventAction::RegistrySync).at(Timestamp::from_nanos(500_000_000)),
        )
        .unwrap();

    let mut old = Event::new(EventAction::ResourceUse).at(Timestamp::from_nanos(1_000_000_000));
    old.arguments.insert(
        "familiarity".into(),
        serde_json::json!({
            "schema": "aikit.familiarity/v1",
            "observation": {
                "observation_id": "trace/old/1",
                "destination": "knowledge-node/auth",
                "context": {},
                "use_kind": {"kind":"destination"},
                "observed_at_ms": 1000
            }
        })
        .to_string(),
    );
    index.record_event(&old).unwrap();

    match replay_familiarity(&index).unwrap() {
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

    // Invalidation discards only learned influence. The unrelated event and the
    // incompatible familiarity evidence both remain in the durable event stream.
    assert_eq!(index.event_count().unwrap(), 2);
}
