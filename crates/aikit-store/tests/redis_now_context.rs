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
