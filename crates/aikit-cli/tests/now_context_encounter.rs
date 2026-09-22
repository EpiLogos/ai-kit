//! Real EncounterService -> Redis prepared-NOW delivery over the controlled ACP peer.
//! The peer is not live inference; the consumer/store/harness path is production code.
#![cfg(unix)]

use aikit_cli::encounter_service::{
    EncounterNowContextConfig, EncounterProvider, EncounterRequest, EncounterService,
};
use aikit_core::{
    context_resolution::ScopeResolution,
    context_source::{AgentVisibility, ExternalEgress},
    project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef},
    scope::ScopeKind,
    session_space::SessionSpaceRef,
    session_space_application::{
        ContextResolutionBasis, ContextResolutionEvidence,
        SessionSpaceAgentAttachmentIntent, SessionSpaceMutation, SessionSpaceProjectContextBinding,
    },
    ProjectRef, ResourceRef,
};
use aikit_store::{
    AikitHome, NowContextBasis, NowContextChange, NowContextItem, PreparedNowContext,
    RedisNowConfig, RedisNowStore, SessionSpaceApplicationStore, NOW_PREPARED_SCHEMA,
    NOW_REDIS_CONFIG_SCHEMA,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn project_evidence(project: &ProjectRef) -> ContextResolutionEvidence {
    let binding = ProjectBinding::new(
        project.clone(),
        ProjectConstituentRef::parse("source:working-tree").unwrap(),
        ProjectBindingLocator::Remote {
            locator: "https://example.invalid/redis-now-proof".into(),
        },
    );
    let basis = ContextResolutionBasis {
        project_binding: binding,
        resolver_hash: "redis-now-proof".into(),
        catalog_revision: "catalog-proof".into(),
        scopes: vec![ScopeResolution {
            kind: ScopeKind::Project,
            depth: 0,
            origin: "controlled Redis NOW encounter proof".into(),
        }],
        context_sources: vec![],
        host: None,
        context_activations: vec![],
        observed_source_resources: vec![],
    };
    serde_json::from_value(serde_json::json!({
        "reference": "context-resolution/redis-now-proof",
        "basis": basis,
        "provenance": ["controlled Redis NOW encounter proof"],
    }))
    .unwrap()
}

fn prepared(
    project: &ResourceRef,
    participant: &ResourceRef,
    session: &ResourceRef,
    excerpt: &str,
) -> PreparedNowContext {
    let basis = NowContextBasis {
        source_revisions: BTreeMap::from([("context-source/proof".into(), "r1".into())]),
        dependency_revisions: BTreeMap::new(),
        disclosure_revision: "disclosure-1".into(),
        factory_revision: Some("factory-r1".into()),
        change_cursor: 0,
    };
    PreparedNowContext {
        schema: NOW_PREPARED_SCHEMA.into(),
        project_ref: project.clone(),
        now_ref: ResourceRef::parse("central:now:proof").unwrap(),
        participant_ref: participant.clone(),
        agent_session: session.clone(),
        version: 1,
        basis_digest: basis.digest().unwrap(),
        basis,
        concern: "Begin useful work from prepared owner context".into(),
        practice_refs: vec![ResourceRef::parse("skill/aikit/operation").unwrap()],
        items: vec![NowContextItem {
            source_ref: ResourceRef::parse("context-source/proof").unwrap(),
            source_revision: "r1".into(),
            title: "Prepared proof source".into(),
            excerpt: excerpt.into(),
            route: Some("central.file-map.resolve".into()),
            agent_visibility: AgentVisibility::Payload,
            external_egress: ExternalEgress::Denied,
        }],
        neighbours: vec![],
        factory: None,
        knowledge_frames: vec![],
        continuation: Some("continue from the exact prepared version".into()),
        jev_invocation_ref: None,
        prepared_at_unix_ms: 1,
    }
}

fn prompt(service: &EncounterService, session: &ResourceRef, text: &str) {
    let view = service
        .apply(EncounterRequest::View {
            agent_session: session.clone(),
            before: None,
        })
        .unwrap();
    let draft = service
        .apply(EncounterRequest::Draft {
            agent_session: session.clone(),
            basis: view["draft"]["revision"].as_u64().unwrap(),
            text: text.into(),
        })
        .unwrap();
    service
        .apply(EncounterRequest::Prompt {
            agent_session: session.clone(),
            draft_revision: draft["revision"].as_u64().unwrap(),
        })
        .unwrap();
}

#[test]
fn redis_prepared_now_is_delivered_before_turn_and_verifier_context_is_isolated() {
    let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
        eprintln!("skipping: AIKIT_TEST_REDIS_ADDR is not set");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let app = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/redis-now-proof").unwrap();
    let session = ResourceRef::parse("agent-session/redis-now-worker").unwrap();
    let verifier = ResourceRef::parse("agent-session/redis-now-verifier").unwrap();
    let project = ProjectRef::parse("project:redis-now-proof").unwrap();
    let project_resource = ResourceRef::parse(project.as_str()).unwrap();

    app.apply(
        &app.stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("Redis NOW proof".into()),
            },
        )
        .unwrap(),
    )
    .unwrap();
    app.apply(
        &app.stage(
            Some(&space),
            SessionSpaceMutation::BindProjectContext {
                binding: Box::new(
                    SessionSpaceProjectContextBinding::new(
                        project.clone(),
                        project_evidence(&project),
                    )
                    .unwrap(),
                ),
            },
        )
        .unwrap(),
    )
    .unwrap();
    app.apply(
        &app.stage(
            Some(&space),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: session.clone(),
                    purpose: Some("worker".into()),
                    provenance: vec!["controlled proof".into()],
                },
            },
        )
        .unwrap(),
    )
    .unwrap();

    let redis = RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address,
        database: 0,
        key_prefix: format!("aikit-encounter-proof-{}", std::process::id()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1_000,
        io_timeout_ms: 1_000,
        prepared_ttl_seconds: 3_600,
        coordination_retention_seconds: 3_600,
    };
    let store = RedisNowStore::new(redis.clone()).unwrap();
    store
        .publish(
            &prepared(
                &project_resource,
                &session,
                &session,
                "WORKER-PREPARED-CONTEXT",
            ),
            0,
            None,
        )
        .unwrap();
    store
        .publish(
            &prepared(
                &project_resource,
                &verifier,
                &verifier,
                "VERIFIER-PRIVATE-CANARY",
            ),
            0,
            None,
        )
        .unwrap();
    assert_eq!(
        store
            .append_change(
                &session,
                &NowContextChange {
                    change_id: "return-1".into(),
                    kind: "factory-return".into(),
                    source_ref: ResourceRef::parse("context-source/proof").unwrap(),
                    source_revision: "r2".into(),
                    detail: "RELATED-WORKER-RETURN".into(),
                    observed_at_unix_ms: 2,
                },
                None,
            )
            .unwrap(),
        1
    );

    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/agent_session_protocol.py");
    EncounterService::configure(
        &home,
        EncounterProvider {
            protocol: Default::default(),
            id: "redis-now-controlled".into(),
            label: "Controlled peer for Redis NOW delivery".into(),
            argv: vec![
                "python3".into(),
                "-u".into(),
                script.display().to_string(),
                "echo-prompt".into(),
            ],
            required_context: None,
            model_policy: None,
            now_context: Some(EncounterNowContextConfig {
                redis,
                prepare_request: None,
                required: true,
                external_provider: false,
            }),
        },
    )
    .unwrap();
    let service = EncounterService::new(home).unwrap();
    service
        .apply(EncounterRequest::Open {
            space: space.clone(),
            agent_session: session.clone(),
            provider: "redis-now-controlled".into(),
            cwd: temp.path().into(),
        })
        .unwrap();
    prompt(&service, &session, "START-USEFUL-WORK");

    let deadline = Instant::now() + Duration::from_secs(5);
    let view = loop {
        let view = service
            .apply(EncounterRequest::View {
                agent_session: session.clone(),
                before: None,
            })
            .unwrap();
        let blocks = view["blocks"].to_string();
        if blocks.contains("WORKER-PREPARED-CONTEXT") && blocks.contains("RELATED-WORKER-RETURN") {
            break view;
        }
        assert!(
            Instant::now() < deadline,
            "provider never echoed prepared NOW context: {view}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let blocks = view["blocks"].to_string();
    assert!(blocks.contains("START-USEFUL-WORK"));
    assert!(blocks.contains("WORKER-PREPARED-CONTEXT"));
    assert!(blocks.contains("RELATED-WORKER-RETURN"));
    assert!(!blocks.contains("VERIFIER-PRIVATE-CANARY"));

    let receipt = store.last_delivery(&session, None).unwrap().unwrap();
    assert_eq!(receipt.prepared_version, 1);
    assert_eq!(receipt.change_cursor, 1);
    assert_eq!(store.ack_cursor(&session, None).unwrap(), 1);

    store.revoke(&session, "disclosure-1", None).unwrap();
    let before = service
        .apply(EncounterRequest::View {
            agent_session: session.clone(),
            before: None,
        })
        .unwrap();
    let draft = service
        .apply(EncounterRequest::Draft {
            agent_session: session.clone(),
            basis: before["draft"]["revision"].as_u64().unwrap(),
            text: "MUST-NOT-CROSS-REVOKED-BOUNDARY".into(),
        })
        .unwrap();
    let failure = service
        .apply(EncounterRequest::Prompt {
            agent_session: session.clone(),
            draft_revision: draft["revision"].as_u64().unwrap(),
        })
        .unwrap_err();
    assert_eq!(failure.code(), "now_context.disclosure_revoked");
}
