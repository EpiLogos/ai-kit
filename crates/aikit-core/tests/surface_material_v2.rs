use std::collections::BTreeMap;

use aikit_core::composition::{
    CompositionState, HarnessComposition, ProjectionBinding, SurfaceDescriptor, SurfaceKind,
};
use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::surface_material::{
    disclose_surface_material, persistent_service_target_fixtures, SurfaceAccessObservation,
    SurfaceExposureScope, SurfaceMaterialHealth, SurfaceMaterialObservation,
    HERMES_GATEWAY_CONFORMANCE_REVISION, OPENCLAW_GATEWAY_CONFORMANCE_REVISION,
    WORKCELL_COLLAPSED_LOCAL_CONFORMANCE_REVISION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn body(session: &str) -> HarnessComposition {
    HarnessComposition {
        version: "aikit.harness-composition/v2".into(),
        harness: r("harness/pi"),
        project: Some(r("project/epi-logos")),
        agent: Some(r("agent/parasakti")),
        agency: Some(r("agency/design")),
        session: Some(session.into()),
        model: None,
        component_bindings: vec![],
        contract_bindings: vec![],
        contributions: vec![],
        surfaces: vec![
            SurfaceDescriptor {
                resource: r("surface/messaging"),
                kind: SurfaceKind::Messaging,
                target_native_id: Some("hermes.messaging".into()),
                owner_component: None,
            },
            SurfaceDescriptor {
                resource: r("surface/api"),
                kind: SurfaceKind::Api,
                target_native_id: Some("openclaw.http".into()),
                owner_component: None,
            },
            SurfaceDescriptor {
                resource: r("surface/webhook"),
                kind: SurfaceKind::Webhook,
                target_native_id: Some("openclaw.hooks".into()),
                owner_component: None,
            },
        ],
        projections: vec![
            ProjectionBinding {
                canonical_ref: r("action/respond"),
                canonical_kind: ResourceKind::Action,
                contribution: r("contribution/respond-messaging"),
                component: r("component/communications"),
                surface: r("surface/messaging"),
                target_native_surface: Some("hermes.messaging".into()),
            },
            ProjectionBinding {
                canonical_ref: r("action/respond"),
                canonical_kind: ResourceKind::Action,
                contribution: r("contribution/respond-api"),
                component: r("component/communications"),
                surface: r("surface/api"),
                target_native_surface: Some("openclaw.http".into()),
            },
            ProjectionBinding {
                canonical_ref: r("knowledge/runtime-reading"),
                canonical_kind: ResourceKind::KnowledgeNode,
                contribution: r("contribution/runtime-reading"),
                component: r("component/observer"),
                surface: r("surface/api"),
                target_native_surface: Some("openclaw.http".into()),
            },
        ],
        absences: vec![],
        state: CompositionState::ObservedActive,
        target_revision: Some("target-revision".into()),
        generation: Some("generation-11".into()),
        fingerprint: "composition-body-11".into(),
    }
}

fn observation(
    surface: &str,
    logical: &str,
    material: &str,
    provider: &str,
    endpoint: &str,
) -> SurfaceMaterialObservation {
    SurfaceMaterialObservation {
        surface: r(surface),
        logical_service_ref: logical.into(),
        material_ref: material.into(),
        workcell_ref: Some("workcell/collapsed-local".into()),
        provider_ref: Some(provider.into()),
        endpoint: Some(endpoint.into()),
        transport: Some("tcp".into()),
        health: SurfaceMaterialHealth::Healthy,
        reachable: Some(true),
        access: SurfaceAccessObservation::Authenticated,
        exposure: SurfaceExposureScope::Local,
        target: None,
        target_revision: None,
        provenance: BTreeMap::new(),
    }
}

#[test]
fn multiple_surfaces_bind_independently_and_missing_or_failed_material_is_explicit() {
    let mut api = observation(
        "surface/api",
        "openclaw:operator-api",
        "service:managed-host:api-1",
        "provider:workcell-local",
        "http://127.0.0.1:18789",
    );
    api.health = SurfaceMaterialHealth::Unavailable;
    api.reachable = Some(false);
    api.provenance.insert(
        "unavailable_reason".into(),
        "managed process disappeared".into(),
    );

    let reading = disclose_surface_material(
        &body("agent-session-53"),
        [
            observation(
                "surface/messaging",
                "hermes:messaging",
                "service:managed-host:msg-1",
                "provider:workcell-local",
                "tcp://127.0.0.1:9001",
            ),
            api,
        ],
    )
    .unwrap();

    assert_eq!(reading.surfaces.len(), 3);
    assert_eq!(reading.unavailable.len(), 2);
    assert!(reading
        .unavailable
        .iter()
        .any(|absence| absence.surface == r("surface/api")
            && absence.reason == "managed process disappeared"));
    assert!(reading
        .unavailable
        .iter()
        .any(|absence| absence.surface == r("surface/webhook")));
    let messaging = reading
        .surfaces
        .iter()
        .find(|surface| surface.surface == r("surface/messaging"))
        .unwrap();
    assert_eq!(messaging.kind, SurfaceKind::Messaging);
    assert_eq!(
        messaging.material.as_ref().unwrap().health,
        SurfaceMaterialHealth::Healthy
    );
}

#[test]
fn material_rebinding_preserves_surface_actor_and_agent_session_identity() {
    let composition = body("agent-session-53");
    let first = disclose_surface_material(
        &composition,
        [observation(
            "surface/messaging",
            "hermes:messaging",
            "service:managed-host:first",
            "provider:workcell-local-a",
            "tcp://127.0.0.1:9001",
        )],
    )
    .unwrap();
    let second = disclose_surface_material(
        &composition,
        [observation(
            "surface/messaging",
            "hermes:messaging",
            "service:managed-host:replacement",
            "provider:workcell-local-b",
            "tcp://127.0.0.1:9002",
        )],
    )
    .unwrap();

    assert_eq!(first.project, second.project);
    assert_eq!(first.agent, second.agent);
    assert_eq!(first.agency, second.agency);
    assert_eq!(first.harness, second.harness);
    assert_eq!(first.agent_session, second.agent_session);
    assert_eq!(
        first.harness_composition_fingerprint,
        second.harness_composition_fingerprint
    );

    let first_surface = first
        .surfaces
        .iter()
        .find(|surface| surface.surface == r("surface/messaging"))
        .unwrap();
    let second_surface = second
        .surfaces
        .iter()
        .find(|surface| surface.surface == r("surface/messaging"))
        .unwrap();
    assert_eq!(first_surface.surface, second_surface.surface);
    assert_eq!(first_surface.action_refs, second_surface.action_refs);
    assert_eq!(
        first_surface
            .material
            .as_ref()
            .unwrap()
            .logical_service_ref,
        second_surface
            .material
            .as_ref()
            .unwrap()
            .logical_service_ref
    );
    assert_ne!(
        first_surface.material.as_ref().unwrap().material_ref,
        second_surface.material.as_ref().unwrap().material_ref
    );
    assert_ne!(
        first_surface.material.as_ref().unwrap().provider_ref,
        second_surface.material.as_ref().unwrap().provider_ref
    );
    assert_ne!(
        first_surface.material.as_ref().unwrap().endpoint,
        second_surface.material.as_ref().unwrap().endpoint
    );
}

#[test]
fn agent_session_replacement_is_not_gateway_or_material_restart() {
    let material = observation(
        "surface/messaging",
        "hermes:messaging",
        "service:managed-host:stable",
        "provider:workcell-local",
        "tcp://127.0.0.1:9001",
    );
    let before = disclose_surface_material(&body("agent-session-before"), [material.clone()]).unwrap();
    let after = disclose_surface_material(&body("agent-session-after"), [material]).unwrap();

    assert_ne!(before.agent_session, after.agent_session);
    assert_eq!(before.agent, after.agent);
    assert_eq!(before.harness, after.harness);
    let before_material = before
        .surfaces
        .iter()
        .find_map(|surface| surface.material.as_ref())
        .unwrap();
    let after_material = after
        .surfaces
        .iter()
        .find_map(|surface| surface.material.as_ref())
        .unwrap();
    assert_eq!(before_material.material_ref, after_material.material_ref);
    assert_eq!(
        before_material.logical_service_ref,
        after_material.logical_service_ref
    );
}

#[test]
fn one_action_projects_through_multiple_surfaces_without_retyping_readings() {
    let reading = disclose_surface_material(
        &body("agent-session-53"),
        [
            observation(
                "surface/messaging",
                "hermes:messaging",
                "service:managed-host:msg",
                "provider:workcell-local",
                "tcp://127.0.0.1:9001",
            ),
            observation(
                "surface/api",
                "openclaw:operator-api",
                "service:managed-host:api",
                "provider:workcell-local",
                "http://127.0.0.1:18789",
            ),
        ],
    )
    .unwrap();

    let messaging = reading
        .surfaces
        .iter()
        .find(|surface| surface.surface == r("surface/messaging"))
        .unwrap();
    let api = reading
        .surfaces
        .iter()
        .find(|surface| surface.surface == r("surface/api"))
        .unwrap();
    assert_eq!(messaging.action_refs, vec![r("action/respond")]);
    assert_eq!(api.action_refs, vec![r("action/respond")]);
    assert!(messaging.non_action_refs.is_empty());
    assert_eq!(api.non_action_refs, vec![r("knowledge/runtime-reading")]);
}

#[test]
fn invalid_material_observations_fail_closed() {
    let unknown = disclose_surface_material(
        &body("agent-session-53"),
        [observation(
            "surface/not-in-composition",
            "unknown:service",
            "service:managed-host:unknown",
            "provider:workcell-local",
            "tcp://127.0.0.1:9999",
        )],
    )
    .unwrap_err();
    assert_eq!(unknown.code(), "surface_material.unknown_surface");

    let duplicate = disclose_surface_material(
        &body("agent-session-53"),
        [
            observation(
                "surface/messaging",
                "hermes:messaging",
                "service:managed-host:a",
                "provider:workcell-local",
                "tcp://127.0.0.1:9001",
            ),
            observation(
                "surface/messaging",
                "hermes:messaging",
                "service:managed-host:b",
                "provider:workcell-local",
                "tcp://127.0.0.1:9002",
            ),
        ],
    )
    .unwrap_err();
    assert_eq!(duplicate.code(), "surface_material.duplicate_surface_binding");
}

#[test]
fn source_pinned_targets_keep_native_management_meanings_distinct() {
    assert_eq!(
        WORKCELL_COLLAPSED_LOCAL_CONFORMANCE_REVISION,
        "8184f9ba95c5efc0077701a4b58c1abb2943bd6e"
    );
    let fixtures = persistent_service_target_fixtures();
    assert_eq!(fixtures[0].upstream_revision, HERMES_GATEWAY_CONFORMANCE_REVISION);
    assert_eq!(fixtures[1].upstream_revision, OPENCLAW_GATEWAY_CONFORMANCE_REVISION);
    assert_ne!(fixtures[0].management_entrypoint, fixtures[1].management_entrypoint);
    assert_ne!(fixtures[0].lifecycle_operations, fixtures[1].lifecycle_operations);
    assert_ne!(fixtures[0].native_surfaces, fixtures[1].native_surfaces);
    assert!(fixtures[0].native_surfaces.contains(&"messaging-platform-gateway"));
    assert!(fixtures[1].native_surfaces.contains(&"websocket-control-rpc"));
    assert!(fixtures[1].native_surfaces.contains(&"hooks"));

    assert_eq!(
        serde_json::to_string(&SurfaceKind::Messaging).unwrap(),
        "\"messaging\""
    );
    assert_eq!(
        serde_json::to_string(&SurfaceKind::Webhook).unwrap(),
        "\"webhook\""
    );
}
