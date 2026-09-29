use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::ResourceRef;
use aikit_store::now_context::{
    NowContextBasis, NowContextChange, NowContextItem, NowDeliveryReceipt, PreparedNowContext,
    RedisNowConfig, RedisNowStore, NOW_DELIVERY_SCHEMA, NOW_PREPARED_SCHEMA,
    NOW_REDIS_CONFIG_SCHEMA,
};
use std::collections::BTreeMap;

fn config(address: String) -> RedisNowConfig {
    RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: format!("aikit-now-test-{}", ulid::Ulid::generate()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    }
}
fn view(version: u64, disclosure: &str, change_cursor: u64) -> PreparedNowContext {
    let source = ResourceRef::parse("context-source/docs").unwrap();
    let basis = NowContextBasis {
        source_revisions: BTreeMap::from([(source.to_string(), format!("r{version}"))]),
        dependency_revisions: BTreeMap::from([("factory/run".into(), format!("f{version}"))]),
        disclosure_revision: disclosure.into(),
        factory_revision: Some(format!("factory-{version}")),
        change_cursor,
    };
    PreparedNowContext {
        schema: NOW_PREPARED_SCHEMA.into(),
        project_ref: ResourceRef::parse("project/test").unwrap(),
        now_ref: ResourceRef::parse("now/test").unwrap(),
        participant_ref: ResourceRef::parse("agent/implementer").unwrap(),
        agent_session: ResourceRef::parse("agent-session/implementer").unwrap(),
        version,
        basis_digest: basis.digest().unwrap(),
        basis,
        concern: "implement the bounded Factory change".into(),
        practice_refs: vec![ResourceRef::parse("skill/project-author").unwrap()],
        items: vec![NowContextItem {
            source_ref: source,
            source_revision: format!("r{version}"),
            title: "Document operation".into(),
            excerpt: "Exact source-backed passage".into(),
            route: Some("wiki/node/document-operation".into()),
            agent_visibility: AgentVisibility::Payload,
            external_egress: ExternalEgress::Allowed,
        }],
        neighbours: vec![],
        factory: None,
        knowledge_frames: vec![],
        continuation: Some("continue from retained Run evidence".into()),
        jev_invocation_ref: Some(ResourceRef::parse("invocation/jev-test").unwrap()),
        prepared_at_unix_ms: 1,
    }
}

#[test]
fn redis_preserves_versioned_participant_context_changes_revocation_and_delivery() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("AIKIT_TEST_REDIS_ADDR absent; real Redis integration is exercised by the dedicated workflow");
        return;
    };
    let store = RedisNowStore::new(config(address)).unwrap();
    let status = store.status(None).unwrap();
    assert!(status.available);
    assert!(status.redis_version.is_some());

    let first = view(1, "disclosure-1", 0);
    assert_eq!(store.publish(&first, 0, None).unwrap(), 1);
    assert_eq!(
        store.current_version(&first.participant_ref, None).unwrap(),
        1
    );
    assert_eq!(
        store
            .read_prepared(&first.participant_ref, true, None)
            .unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        store.publish(&first, 0, None).unwrap_err().code(),
        "now_context.stale"
    );

    let change = NowContextChange {
        change_id: "return-1".into(),
        kind: "factory-return".into(),
        source_ref: ResourceRef::parse("return/factory-1").unwrap(),
        source_revision: "return-r1".into(),
        detail: "related worker returned a dependency revision".into(),
        observed_at_unix_ms: 2,
    };
    let cursor = store
        .append_change(&first.participant_ref, &change, None)
        .unwrap();
    assert_eq!(
        store
            .append_change(&first.participant_ref, &change, None)
            .unwrap(),
        cursor
    );
    let changes = store
        .read_changes(&first.participant_ref, 0, 10, None)
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].cursor, cursor);
    assert_eq!(store.ack_cursor(&first.participant_ref, None).unwrap(), 0);
    assert_eq!(
        store
            .ack_changes(&first.participant_ref, cursor, None)
            .unwrap(),
        cursor
    );
    assert_eq!(
        store.ack_cursor(&first.participant_ref, None).unwrap(),
        cursor
    );
    // A replayed older acknowledgement can never move this participant's
    // independent consumption position backwards.
    assert_eq!(
        store
            .ack_changes(&first.participant_ref, cursor.saturating_sub(1), None)
            .unwrap(),
        cursor
    );

    let delivery = NowDeliveryReceipt {
        schema: NOW_DELIVERY_SCHEMA.into(),
        participant_ref: first.participant_ref.clone(),
        agent_session: first.agent_session.clone(),
        prepared_version: 1,
        prepared_digest: first.digest().unwrap(),
        basis_digest: first.basis_digest.clone(),
        change_cursor: cursor,
        delivered_at_unix_ms: 3,
    };
    store.mark_delivered(&delivery, None).unwrap();
    assert_eq!(
        store.last_delivery(&first.participant_ref, None).unwrap(),
        Some(delivery)
    );

    store
        .revoke(&first.participant_ref, "disclosure-1", None)
        .unwrap();
    assert_eq!(
        store
            .read_prepared(&first.participant_ref, true, None)
            .unwrap_err()
            .code(),
        "now_context.disclosure_revoked"
    );

    let second = view(2, "disclosure-2", cursor);
    assert_eq!(store.publish(&second, 1, None).unwrap(), 2);
    assert_eq!(
        store
            .read_prepared(&second.participant_ref, true, None)
            .unwrap(),
        Some(second)
    );
}

fn world_projection(version: u64, subject: &str) -> aikit_store::now_context::WorldProjection {
    use aikit_core::inhabitation::{FacetState, InhabitationIdentity};
    let identity = InhabitationIdentity {
        local_world_ref: Some("control:root".into()),
        project_world_ref: Some("project:O-I".into()),
        position_ref: Some(subject.into()),
        occupant_generation: Some("actuation:generation:g1".into()),
        root_now_ref: Some("central:now:control:root:r".into()),
        root_now_revision: Some(format!("rev-{version}")),
        ..InhabitationIdentity::default()
    };
    aikit_store::now_context::WorldProjection {
        schema: aikit_store::now_context::WORLD_PROJECTION_SCHEMA.into(),
        subject: subject.into(),
        version,
        identity_digest: identity.digest(),
        identity,
        facet_states: BTreeMap::from([("position".to_owned(), FacetState::Present)]),
        facet_summaries: BTreeMap::from([("position".to_owned(), subject.to_owned())]),
        published_at_unix_ms: 1,
    }
}

#[test]
fn redis_world_projection_is_cas_versioned_refs_only_and_survives_loss() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("AIKIT_TEST_REDIS_ADDR absent; real Redis integration is exercised by the dedicated workflow");
        return;
    };
    let store = RedisNowStore::new(config(address)).unwrap();
    let subject = "central:position:project:O-I:aikit-guardian";
    assert_eq!(store.world_version(subject, None).unwrap(), 0);
    assert_eq!(store.read_world(subject, None).unwrap(), None);

    let first = world_projection(1, subject);
    assert_eq!(store.publish_world(&first, 0, None).unwrap(), 1);
    assert_eq!(
        store.read_world(subject, None).unwrap(),
        Some(first.clone())
    );
    // A late writer holding the old version is refused, never last-wins.
    assert_eq!(
        store.publish_world(&first, 0, None).unwrap_err().code(),
        "now_context.stale"
    );
    let second = world_projection(2, subject);
    assert_eq!(store.publish_world(&second, 1, None).unwrap(), 2);
    assert_eq!(store.world_version(subject, None).unwrap(), 2);

    // A projection whose digest does not match its identity is refused.
    let mut forged = world_projection(3, subject);
    forged.identity.position_ref = Some("central:position:project:O-I:other".into());
    assert_eq!(
        store.publish_world(&forged, 2, None).unwrap_err().code(),
        "world_projection.invalid"
    );

    // Loss: both keys go; the next publish starts again from version 1.
    store.delete_world(subject, None).unwrap();
    assert_eq!(store.world_version(subject, None).unwrap(), 0);
    assert_eq!(store.read_world(subject, None).unwrap(), None);
    let rebuilt = world_projection(1, subject);
    assert_eq!(store.publish_world(&rebuilt, 0, None).unwrap(), 1);
    assert_eq!(
        store.read_world(subject, None).unwrap().unwrap().identity,
        first.identity,
        "semantic identity is recomputed, not remembered"
    );
    store.delete_world(subject, None).unwrap();
}

#[test]
fn factory_sensing_is_project_scoped_cas_guarded_and_rebuildable() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("AIKIT_TEST_REDIS_ADDR absent; real Redis integration is exercised by the dedicated workflow");
        return;
    };
    use aikit_store::now_context::{FactorySensingProjection, FACTORY_SENSING_PROJECTION_SCHEMA};
    let store = RedisNowStore::new(config(address)).unwrap();
    let field = |project: &str, revision: &str| {
        let sequence: u64 = revision[revision.len() - 1..].parse().unwrap();
        serde_json::json!({
            "schema": "factory.telemetry-field/v1",
            "project_world_ref": project,
            "source_revision": revision,
            "source_sequence": sequence,
            "observed_at_unix_ms": sequence * 10,
            "signals": [{"signal_ref":"factory:signal:s1", "source_refs":["factory:attempt:a1"],
                "classification":"verified-defect", "disposition":"investigate", "summary":"Attempt failed",
                "updated_at_unix_ms": 10}],
            "coverage": [], "owner_basis": {}, "absences": [], "cursor": revision,
            "counts": {"signals": 1}, "truncated": false
        })
    };
    let projection = |project: &str, revision: &str, version| FactorySensingProjection {
        schema: FACTORY_SENSING_PROJECTION_SCHEMA.into(),
        project_world_ref: project.into(),
        version,
        source_revision: revision.into(),
        field: field(project, revision),
        published_at_unix_ms: 11,
    };
    let a = "project:Alpha";
    let b = "project:Beta";
    assert_eq!(store.factory_sensing_version(a, None).unwrap(), 0);
    let a1 = projection(a, "blake3:a1", 1);
    let b1 = projection(b, "blake3:b1", 1);
    let mut missing_sequence = a1.clone();
    missing_sequence
        .field
        .as_object_mut()
        .unwrap()
        .remove("source_sequence");
    assert_eq!(
        store
            .publish_factory_sensing(&missing_sequence, 0, None)
            .unwrap_err()
            .code(),
        "factory_sensing.invalid"
    );
    assert_eq!(store.publish_factory_sensing(&a1, 0, None).unwrap(), 1);
    assert_eq!(store.publish_factory_sensing(&b1, 0, None).unwrap(), 1);
    assert_eq!(
        store.read_factory_sensing(a, None).unwrap(),
        Some(a1.clone())
    );
    assert_eq!(store.read_factory_sensing(b, None).unwrap(), Some(b1));
    assert_eq!(
        store
            .publish_factory_sensing(&a1, 0, None)
            .unwrap_err()
            .code(),
        "now_context.stale"
    );
    let a2 = projection(a, "blake3:a2", 2);
    assert_eq!(store.publish_factory_sensing(&a2, 1, None).unwrap(), 2);
    let mut crossed = projection(a, "blake3:a3", 3);
    crossed.field["project_world_ref"] = serde_json::json!(b);
    assert_eq!(
        store
            .publish_factory_sensing(&crossed, 2, None)
            .unwrap_err()
            .code(),
        "factory_sensing.invalid"
    );
    store.delete_factory_sensing(a, None).unwrap();
    assert_eq!(store.read_factory_sensing(a, None).unwrap(), None);
    assert_eq!(store.publish_factory_sensing(&a1, 0, None).unwrap(), 1);
    assert_eq!(
        store
            .read_factory_sensing(a, None)
            .unwrap()
            .unwrap()
            .source_revision,
        "blake3:a1"
    );
    // After loss the version restarts. A pre-loss publisher that captured
    // expected version 1 must not overwrite a newer post-loss version 1.
    store.delete_factory_sensing(a, None).unwrap();
    let fresh = projection(a, "blake3:a3", 1);
    assert_eq!(store.publish_factory_sensing(&fresh, 0, None).unwrap(), 1);
    let stale_pre_loss = projection(a, "blake3:a2", 2);
    assert_eq!(
        store
            .publish_factory_sensing(&stale_pre_loss, 1, None)
            .unwrap_err()
            .code(),
        "now_context.stale"
    );
    assert_eq!(store.read_factory_sensing(a, None).unwrap(), Some(fresh));
    let mut older_observation = projection(a, "blake3:a4", 2);
    older_observation.field["observed_at_unix_ms"] = serde_json::json!(29);
    assert_eq!(
        store
            .publish_factory_sensing(&older_observation, 1, None)
            .unwrap_err()
            .code(),
        "now_context.stale"
    );
    store.delete_factory_sensing(a, None).unwrap();
    store.delete_factory_sensing(b, None).unwrap();
}
