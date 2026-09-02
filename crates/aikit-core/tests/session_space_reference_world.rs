use std::collections::BTreeSet;

use aikit_core::resource::ResourceRef;
use aikit_core::{
    ActivationScope, ActivationScopeKind, ComponentBinding, CompositionActivationMode,
    CompositionState, HarnessComposition, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
    ScopeKind, SessionSpaceActivationDriver, SessionSpaceActivationObservation,
    SessionSpaceActivationRequest, SessionSpaceActivationState, SessionSpaceAgentSession,
    SessionSpaceAuthorityState, SessionSpaceConnection, SessionSpaceConnectionState,
    SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime, SurfaceDescriptor, SurfaceKind,
    HARNESS_COMPOSITION_VERSION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn component(id: &str) -> ComponentBinding {
    ComponentBinding {
        component: r(id),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "reference-world fixture"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("agent-session/reference-world"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession)
            .with_reference("agent-session/reference-world"),
        activation_mode: CompositionActivationMode::LiveMounted,
        implementation: None,
    }
}

fn reference_composition() -> HarnessComposition {
    HarnessComposition {
        version: HARNESS_COMPOSITION_VERSION.into(),
        harness: r("harness/reference-world"),
        project: Some(r("project/reference-world")),
        agent: Some(r("agent/reference")),
        agency: Some(r("agency/reference")),
        session: Some("native-reference-agent".into()),
        model: None,
        component_bindings: vec![
            component("component/reference/herdr"),
            component("component/reference/hyprland"),
            component("component/reference/harness-native"),
        ],
        contract_bindings: vec![],
        contributions: vec![],
        surfaces: vec![
            SurfaceDescriptor {
                resource: r("surface/reference/terminal"),
                kind: SurfaceKind::Tui,
                target_native_id: Some("herdr-pane-7".into()),
                owner_component: Some(r("component/reference/herdr")),
            },
            SurfaceDescriptor {
                resource: r("surface/reference/graphical"),
                kind: SurfaceKind::Web,
                target_native_id: Some("0x41a".into()),
                owner_component: Some(r("component/reference/hyprland")),
            },
            SurfaceDescriptor {
                resource: r("surface/reference/harness"),
                kind: SurfaceKind::Tui,
                target_native_id: Some("dsh:reference".into()),
                owner_component: Some(r("component/reference/harness-native")),
            },
        ],
        projections: vec![],
        absences: vec![],
        state: CompositionState::Resolved,
        target_revision: Some("reference-world-fixture/v1".into()),
        generation: Some("deterministic".into()),
        fingerprint: "reference-world/body-v1".into(),
    }
}

struct ReferenceDriver;

impl SessionSpaceActivationDriver for ReferenceDriver {
    fn activate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> aikit_core::Result<SessionSpaceActivationObservation> {
        let provider = match request.component.component.to_string().as_str() {
            "component/reference/herdr" => r("provider/herdr"),
            "component/reference/hyprland" => r("provider/hyprland"),
            "component/reference/harness-native" => r("provider/harness-native"),
            other => panic!("unexpected reference component {other}"),
        };
        Ok(SessionSpaceActivationObservation::Active {
            provider,
            provenance: vec!["deterministic reference-provider observation".into()],
        })
    }

    fn deactivate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> aikit_core::Result<SessionSpaceActivationObservation> {
        Ok(SessionSpaceActivationObservation::Deactivated {
            provider: match request.component.component.to_string().as_str() {
                "component/reference/herdr" => r("provider/herdr"),
                "component/reference/hyprland" => r("provider/hyprland"),
                "component/reference/harness-native" => r("provider/harness-native"),
                other => panic!("unexpected reference component {other}"),
            },
            provenance: vec!["deterministic provider deactivation".into()],
        })
    }
}

fn connection(
    id: &str,
    provider: &str,
    protocol: &str,
    component: Option<&str>,
    surface: Option<&str>,
    native_session_id: &str,
) -> SessionSpaceConnection {
    SessionSpaceConnection {
        connection: r(id),
        provider: r(provider),
        protocol: protocol.into(),
        agent_session: r("agent-session/reference-world"),
        component: component.map(r),
        surface: surface.map(r),
        state: SessionSpaceConnectionState::Connected,
        native_session_id: Some(native_session_id.into()),
        authority: SessionSpaceAuthorityState::default(),
        reason: None,
        provenance: vec![format!("explicit {provider} reference-world connection")],
    }
}

#[test]
fn one_session_space_keeps_identity_across_several_provider_relations_and_isolates_loss() {
    let definition = SessionSpaceDefinition::new(
        SessionSpaceRef::parse("session-space/reference-world").unwrap(),
    )
    .with_project(r("project/reference-world"))
    .with_provenance("#139 deterministic H3 reference-world fixture");
    let mut runtime = SessionSpaceRuntime::open(definition).unwrap();
    let lease = runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: r("agent-session/reference-world"),
            harness: r("harness/reference-world"),
            native_session_id: Some("native-reference-agent".into()),
            provider: Some(r("provider/gateway")),
            provenance: vec!["canonical AgentSession retained across providers".into()],
        })
        .unwrap();

    runtime
        .admit_composition(&lease, reference_composition())
        .unwrap();
    let mut driver = ReferenceDriver;
    for id in [
        "component/reference/herdr",
        "component/reference/hyprland",
        "component/reference/harness-native",
    ] {
        assert_eq!(
            runtime
                .activate_component(&lease, &r(id), &mut driver)
                .unwrap(),
            SessionSpaceActivationState::Active
        );
    }

    for observed in [
        connection(
            "connection/reference/herdr",
            "provider/herdr",
            "working-environment/herdr-v1",
            Some("component/reference/herdr"),
            Some("surface/reference/terminal"),
            "herdr-workspace-7",
        ),
        connection(
            "connection/reference/hyprland",
            "provider/hyprland",
            "working-environment/hyprland-v1",
            Some("component/reference/hyprland"),
            Some("surface/reference/graphical"),
            "hyprland:0x41a",
        ),
        connection(
            "connection/reference/harness-native",
            "provider/harness-native",
            "harness/native-v1",
            Some("component/reference/harness-native"),
            Some("surface/reference/harness"),
            "dsh:reference",
        ),
        connection(
            "connection/reference/gateway",
            "provider/gateway",
            "gateway/agency-v1",
            None,
            None,
            "gateway-stream-reference",
        ),
    ] {
        runtime.observe_connection(&lease, observed).unwrap();
    }

    let before = runtime.read_model();
    assert_eq!(before.id.to_string(), "session-space/reference-world");
    assert_eq!(before.agent_sessions.len(), 1);
    assert_eq!(
        before.agent_sessions[0].agent_session,
        r("agent-session/reference-world")
    );
    assert_eq!(before.connections.len(), 4);
    assert!(before
        .connections
        .iter()
        .all(|connection| connection.agent_session == r("agent-session/reference-world")));
    let native_ids = before
        .connections
        .iter()
        .filter_map(|connection| connection.native_session_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        native_ids.len(),
        4,
        "native identities remain distinct evidence"
    );
    let canonical_surfaces = before
        .surfaces
        .iter()
        .map(|surface| surface.surface.clone())
        .collect::<BTreeSet<_>>();

    runtime
        .observe_provider_unavailable(&r("provider/herdr"), "Herdr provider disappeared")
        .unwrap();

    let after = runtime.read_model();
    assert_eq!(after.id, before.id);
    assert_eq!(after.agent_sessions, before.agent_sessions);
    assert_eq!(
        after
            .surfaces
            .iter()
            .map(|surface| surface.surface.clone())
            .collect::<BTreeSet<_>>(),
        canonical_surfaces,
        "provider loss must not mint or retract canonical Surface identity"
    );

    let herdr = after
        .connections
        .iter()
        .find(|connection| connection.connection == r("connection/reference/herdr"))
        .unwrap();
    assert_eq!(herdr.state, SessionSpaceConnectionState::Unavailable);
    assert_eq!(herdr.reason.as_deref(), Some("Herdr provider disappeared"));
    for id in [
        "connection/reference/hyprland",
        "connection/reference/harness-native",
        "connection/reference/gateway",
    ] {
        assert_eq!(
            after
                .connections
                .iter()
                .find(|connection| connection.connection == r(id))
                .unwrap()
                .state,
            SessionSpaceConnectionState::Connected,
            "unrelated provider relation {id} must survive Herdr loss"
        );
    }

    assert_eq!(
        after
            .components
            .iter()
            .find(|component| component.component == r("component/reference/herdr"))
            .unwrap()
            .state,
        SessionSpaceActivationState::Degraded
    );
    for id in [
        "component/reference/hyprland",
        "component/reference/harness-native",
    ] {
        assert_eq!(
            after
                .components
                .iter()
                .find(|component| component.component == r(id))
                .unwrap()
                .state,
            SessionSpaceActivationState::Active
        );
    }
    assert_eq!(
        after
            .surfaces
            .iter()
            .find(|surface| surface.surface == r("surface/reference/terminal"))
            .unwrap()
            .state,
        SessionSpaceActivationState::Degraded
    );
    assert_eq!(
        after
            .surfaces
            .iter()
            .find(|surface| surface.surface == r("surface/reference/graphical"))
            .unwrap()
            .state,
        SessionSpaceActivationState::Active
    );
}
