use aikit_core::resource::{
    Eligibility, PreferenceIntent, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
    SourceRef,
};
use aikit_core::{
    AccessibilitySignalClass, FamiliarityContext, FamiliarityObservation, FamiliaritySnapshot,
    FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence, ForgetScope, RouteStepEvidence,
    DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn context(project: &str, actor: &str, focus: &str) -> FamiliarityContext {
    FamiliarityContext {
        project: Some(r(project)),
        actor: Some(r(actor)),
        agency: None,
        focus: Some(focus.into()),
    }
}

#[test]
fn the_same_destination_learns_different_accessibility_in_different_contexts() {
    let destination = r("knowledge-node/authentication");
    let payments = context("project/payments", "agent/design", "login");
    let docs = context("project/docs", "agent/writer", "guide");
    let now = 2 * DEFAULT_FAMILIARITY_HALF_LIFE_MS;
    let mut store = FamiliarityStore::new();

    store
        .record(FamiliarityObservation::destination(
            "evt/payments/recent",
            destination.clone(),
            payments.clone(),
            now - 1_000,
        ))
        .unwrap();
    store
        .record(FamiliarityObservation::destination(
            "evt/docs/old",
            destination.clone(),
            docs.clone(),
            0,
        ))
        .unwrap();

    let payments_score = store.assess_destination(
        &destination,
        &payments,
        now,
        DEFAULT_FAMILIARITY_HALF_LIFE_MS,
    );
    let docs_score =
        store.assess_destination(&destination, &docs, now, DEFAULT_FAMILIARITY_HALF_LIFE_MS);

    assert_eq!(payments_score.observations, 2);
    assert_eq!(docs_score.observations, 2);
    assert_eq!(payments_score.contextual_observations, 1);
    assert_eq!(docs_score.contextual_observations, 1);
    assert!(payments_score.contextual_frecency > docs_score.contextual_frecency);
    assert!(payments_score
        .signals
        .iter()
        .any(|signal| signal.class == AccessibilitySignalClass::Context));
}

#[test]
fn outcome_fitness_is_explained_separately_from_frequency_and_recency() {
    let destination = r("capability/code/review");
    let ctx = context("project/app", "agent/developer", "review");
    let mut store = FamiliarityStore::new();
    store
        .record(
            FamiliarityObservation::destination(
                "evt/review/1",
                destination.clone(),
                ctx.clone(),
                1_000,
            )
            .with_fitness(FitnessEvidence::new(800, "accepted review outcome").unwrap()),
        )
        .unwrap();
    store
        .record(FamiliarityObservation::destination(
            "evt/review/2",
            destination.clone(),
            ctx.clone(),
            2_000,
        ))
        .unwrap();

    let assessment = store.assess_destination(&destination, &ctx, 3_000, 10_000);

    assert_eq!(assessment.contextual_fitness_milli, Some(800.0));
    assert_eq!(
        assessment
            .signals
            .iter()
            .filter(|signal| signal.class == AccessibilitySignalClass::Fitness)
            .count(),
        1
    );
    assert_eq!(
        assessment
            .signals
            .iter()
            .find(|signal| signal.class == AccessibilitySignalClass::Frecency)
            .unwrap()
            .evidence_count,
        2
    );
}

#[test]
fn route_familiarity_preserves_the_route_steps_provider_lens_and_revision_evidence() {
    let route = r("knowledge-route/auth-to-session");
    let destination = r("knowledge-node/session");
    let ctx = context("project/app", "agent/research", "auth");
    let steps = vec![
        RouteStepEvidence {
            resource: r("knowledge-node/auth"),
            provider: Some(r("provider/wiki")),
            lens: Some("L4-prime".into()),
            revision: Some("wiki-r41".into()),
        },
        RouteStepEvidence {
            resource: destination.clone(),
            provider: Some(r("provider/code-index")),
            lens: Some("code".into()),
            revision: Some("code-r17".into()),
        },
    ];
    let observation = FamiliarityObservation::route(
        "evt/route/1",
        route.clone(),
        destination.clone(),
        steps.clone(),
        ctx.clone(),
        10_000,
    )
    .unwrap();
    let mut store = FamiliarityStore::new();
    store.record(observation.clone()).unwrap();
    store
        .record(
            FamiliarityObservation::route(
                "evt/route/2",
                route.clone(),
                destination.clone(),
                steps.clone(),
                ctx.clone(),
                11_000,
            )
            .unwrap(),
        )
        .unwrap();

    let assessment = store.assess_route(&route, &destination, &ctx, 12_000, 100_000);

    assert_eq!(assessment.observations, 2);
    assert_eq!(assessment.contextual_observations, 2);
    assert_eq!(
        store.route_steps(&route),
        std::collections::BTreeSet::from([vec![r("knowledge-node/auth"), destination.clone(),]])
    );
    match observation.use_kind {
        aikit_core::FamiliarityUse::Route {
            steps: observed, ..
        } => {
            assert_eq!(observed[0].provider, Some(r("provider/wiki")));
            assert_eq!(observed[0].lens.as_deref(), Some("L4-prime"));
            assert_eq!(observed[0].revision.as_deref(), Some("wiki-r41"));
        }
        _ => panic!("expected route evidence"),
    }
}

#[test]
fn resetting_one_learned_scope_does_not_touch_other_evidence_or_canonical_identity() {
    let destination = r("knowledge-node/auth");
    let route = r("knowledge-route/auth");
    let ctx = context("project/app", "agent/research", "auth");
    let mut store = FamiliarityStore::new();
    store
        .record(FamiliarityObservation::destination(
            "evt/destination",
            destination.clone(),
            ctx.clone(),
            1,
        ))
        .unwrap();
    store
        .record(
            FamiliarityObservation::route(
                "evt/route",
                route.clone(),
                destination.clone(),
                vec![RouteStepEvidence {
                    resource: destination.clone(),
                    provider: None,
                    lens: None,
                    revision: None,
                }],
                ctx.clone(),
                2,
            )
            .unwrap(),
        )
        .unwrap();

    assert_eq!(
        store.forget(&ForgetScope::Destination(destination.clone())),
        1
    );
    assert!(store
        .assess_destination(&destination, &ctx, 3, 100)
        .is_empty());
    assert_eq!(
        store
            .assess_route(&route, &destination, &ctx, 3, 100)
            .observations,
        1
    );
    assert_eq!(destination.as_str(), "knowledge-node/auth");
}

#[test]
fn repeated_use_has_no_authority_to_change_eligibility_preference_or_trust_like_state() {
    let destination = r("capability/restricted");
    let mut record = ResourceRecord::new(ResourceDescriptor::new(
        destination.clone(),
        ResourceKind::Capability,
        "restricted",
        "restricted capability",
    ));
    record.eligibility = Eligibility::Ineligible {
        reasons: vec!["policy denied".into()],
    };
    record.preference = Some(PreferenceIntent {
        source: SourceRef::parse("source:user-preference").unwrap(),
        rank: -100,
        rationale: Some("explicitly avoid".into()),
    });
    let before = record.clone();
    let mut store = FamiliarityStore::new();
    let ctx = FamiliarityContext::default();
    for index in 0..50 {
        store
            .record(FamiliarityObservation::destination(
                format!("evt/{index}"),
                destination.clone(),
                ctx.clone(),
                index,
            ))
            .unwrap();
    }

    let assessment = store.assess_destination(&destination, &ctx, 100, 10_000);

    assert!(assessment.frecency > 40.0);
    assert_eq!(record, before);
    assert!(assessment.signals.iter().all(|signal| matches!(
        signal.class,
        AccessibilitySignalClass::Frecency
            | AccessibilitySignalClass::Context
            | AccessibilitySignalClass::Fitness
    )));
}

#[test]
fn snapshot_schema_change_explicitly_invalidates_only_learned_influence() {
    let destination = r("knowledge-node/auth");
    let snapshot = FamiliaritySnapshot {
        schema: "aikit.familiarity/v1".into(),
        observations: vec![FamiliarityObservation::destination(
            "evt/old",
            destination.clone(),
            FamiliarityContext::default(),
            1,
        )],
    };

    match FamiliarityStore::load(snapshot).unwrap() {
        FamiliaritySnapshotLoad::Invalidated {
            found_schema,
            observations_discarded,
            reason,
        } => {
            assert_eq!(found_schema, "aikit.familiarity/v1");
            assert_eq!(observations_discarded, 1);
            assert!(reason.contains(FAMILIARITY_SCHEMA_VERSION));
        }
        FamiliaritySnapshotLoad::Loaded(_) => {
            panic!("old schema should not silently influence ranking")
        }
    }
    assert_eq!(destination.as_str(), "knowledge-node/auth");
}
