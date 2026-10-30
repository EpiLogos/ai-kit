use aikit_core::resource::{
    Eligibility, PreferenceIntent, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
    SourceRef,
};
use aikit_core::{
    AccessibilitySignalClass, FamiliarityContext, FamiliarityObservation, FamiliaritySnapshot,
    FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence, ForgetScope, OperativePathEvidence,
    RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,
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
fn resolve_path_familiarity_has_its_own_identity_and_reset_scope() {
    use aikit_core::resource::{
        parse_resolve_expression, resolve_path_identity, AddressHorizon, RelationOp,
    };

    let destination = r("action/verify");
    let route = r("knowledge-route/spec-to-verify");
    let ctx = context("project/app", "agent/operator", "verify");
    let expression = parse_resolve_expression("+ @5 action/verify").unwrap();
    let path_identity = resolve_path_identity(&expression);
    let steps = vec![RouteStepEvidence {
        resource: destination.clone(),
        provider: Some(r("provider/native")),
        lens: Some("action".into()),
        revision: Some("r7".into()),
    }];
    let mut store = FamiliarityStore::new();

    store
        .record(
            FamiliarityObservation::route(
                "evt/route/independent",
                route.clone(),
                destination.clone(),
                steps.clone(),
                ctx.clone(),
                1_000,
            )
            .unwrap(),
        )
        .unwrap();
    store
        .record(
            FamiliarityObservation::resolve_path(
                "evt/path/independent",
                Some(route.clone()),
                destination.clone(),
                steps,
                OperativePathEvidence {
                    path_identity: path_identity.clone(),
                    expression: expression.clone(),
                    relation_ops: vec![RelationOp::Affirm],
                    horizons: vec![AddressHorizon::H5],
                    method: Some(r("method/verify")),
                    action: Some(destination.clone()),
                    surface: Some(r("surface/aikit/tui")),
                    activity: None,
                    return_ref: None,
                },
                ctx.clone(),
                1_001,
            )
            .unwrap(),
        )
        .unwrap();

    assert_eq!(
        store
            .assess_route(&route, &destination, &ctx, 1_002, 10_000)
            .observations,
        1
    );
    let path = store.assess_resolve_path(&path_identity, &destination, &ctx, 1_002, 10_000);
    assert_eq!(path.observations, 1);
    assert_eq!(path.route, None);
    assert_eq!(path.path_identity.as_deref(), Some(path_identity.as_str()));
    assert_eq!(
        store.resolve_path_steps(&path_identity),
        std::collections::BTreeSet::from([vec![destination.clone()]])
    );

    assert_eq!(
        store.forget(&ForgetScope::ResolvePath(path_identity.clone())),
        1
    );
    assert!(store
        .assess_resolve_path(&path_identity, &destination, &ctx, 1_003, 10_000)
        .is_empty());
    assert_eq!(
        store
            .assess_route(&route, &destination, &ctx, 1_003, 10_000)
            .observations,
        1,
        "forgetting operative-path ease must not erase KnowledgeRoute evidence"
    );
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

/// O-I #741 (the search line's ✕, DLW §18.4: "Each world you work in keeps its own [history] … Press ✕ beside it and the line
/// forgets it"): one item is forgotten in one world. Every learned use of the destination recorded in that world's project
/// context goes — a plain use, a traversed route ending there and an operative path alike, since the line records its opens as
/// routes — and the same destination's uses in another world, and other destinations in the same world, stay.
#[test]
fn forgetting_one_destination_in_one_world_keeps_it_in_every_other_world() {
    let destination = r("knowledge-node/bollingen");
    let other = r("knowledge-node/kusnacht");
    let route = r("knowledge-route:bollingen");
    let here = context("project/earth", "agent/reader", "walk");
    let elsewhere = context("project/book", "agent/reader", "walk");
    let unscoped = FamiliarityContext::default();
    let step = |resource: &ResourceRef| RouteStepEvidence {
        resource: resource.clone(),
        provider: None,
        lens: None,
        revision: None,
    };
    let mut store = FamiliarityStore::new();
    let mut n = 0u64;
    let mut route_use =
        |store: &mut FamiliarityStore, to: &ResourceRef, ctx: &FamiliarityContext| {
            n += 1;
            store
                .record(
                    FamiliarityObservation::route(
                        format!("evt/route/{n}"),
                        route.clone(),
                        to.clone(),
                        vec![step(to)],
                        ctx.clone(),
                        n,
                    )
                    .unwrap(),
                )
                .unwrap();
        };
    route_use(&mut store, &destination, &here);
    route_use(&mut store, &destination, &here);
    route_use(&mut store, &destination, &elsewhere);
    route_use(&mut store, &destination, &unscoped);
    store
        .record(FamiliarityObservation::destination(
            "evt/destination/here",
            destination.clone(),
            here.clone(),
            10,
        ))
        .unwrap();
    store
        .record(FamiliarityObservation::destination(
            "evt/other/here",
            other.clone(),
            here.clone(),
            11,
        ))
        .unwrap();

    let forgotten = store.forget(&ForgetScope::DestinationIn {
        destination: destination.clone(),
        project: here.project.clone(),
    });
    assert_eq!(forgotten, 3, "both routes and the plain use in this world");
    assert!(
        store
            .assess_route(&route, &destination, &here, 20, 100)
            .contextual_observations
            == 0
    );
    assert!(store
        .assess_destination(&destination, &here, 20, 100)
        .is_empty());
    assert_eq!(
        store
            .assess_route(&route, &destination, &elsewhere, 20, 100)
            .contextual_observations,
        1,
        "the other world keeps its own"
    );
    assert_eq!(
        store
            .assess_destination(&other, &here, 20, 100)
            .contextual_observations,
        1,
        "another item in this world stays"
    );

    // The unscoped world (no project) is a world of its own: forgetting there leaves the scoped worlds.
    assert_eq!(
        store.forget(&ForgetScope::DestinationIn {
            destination: destination.clone(),
            project: None,
        }),
        1
    );
    assert_eq!(
        store
            .assess_route(&route, &destination, &elsewhere, 21, 100)
            .contextual_observations,
        1
    );
}

/// O-I #820 (DLW §18.4: "Each world you work in keeps its own [history]"): the project is the world a use was made in, and
/// no project is a world too — the person's own, where every use was recorded before worlds reached AIKit. A use made in a
/// project is not contextual for the person's world, and the person's own use is not contextual for a project: an unscoped
/// request never stands for every world. The other axes still narrow only when they are asked.
#[test]
fn an_unscoped_context_is_the_persons_own_world_not_every_world() {
    let destination = r("wiki:node:compass");
    let yours = FamiliarityContext::default();
    let alpha = FamiliarityContext {
        project: Some(r("project:Alpha")),
        ..FamiliarityContext::default()
    };
    let mut store = FamiliarityStore::new();
    for (id, ctx) in [
        ("evt/yours/1", &yours),
        ("evt/alpha/1", &alpha),
        ("evt/alpha/2", &alpha),
    ] {
        store
            .record(FamiliarityObservation::destination(
                id,
                destination.clone(),
                ctx.clone(),
                1,
            ))
            .unwrap();
    }
    let in_yours = store.assess_destination(&destination, &yours, 2, 100);
    let in_alpha = store.assess_destination(&destination, &alpha, 2, 100);
    assert_eq!(in_yours.observations, 3, "every use is still evidence");
    assert_eq!(
        in_yours.contextual_observations, 1,
        "the person's world counts its own use, not Alpha's"
    );
    assert_eq!(
        in_alpha.contextual_observations, 2,
        "Alpha counts its own two, not the person's"
    );

    // An actor-qualified use in the person's world still counts there when no actor is asked.
    let by_reader = FamiliarityContext {
        actor: Some(r("agent/reader")),
        ..FamiliarityContext::default()
    };
    store
        .record(FamiliarityObservation::destination(
            "evt/yours/reader",
            destination.clone(),
            by_reader,
            1,
        ))
        .unwrap();
    assert_eq!(
        store
            .assess_destination(&destination, &yours, 2, 100)
            .contextual_observations,
        2
    );
}
