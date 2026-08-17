use aikit_core::resource::ResourceRef;
use aikit_core::{
    CompositionActivationMode, SessionSpaceActivationState, SessionSpaceAgentSession,
    SessionSpaceAuthorityState, SessionSpaceComponent, SessionSpaceConnection,
    SessionSpaceConnectionState, SessionSpaceLifecycle, SessionSpaceReadModel, SessionSpaceRef,
    SessionSpaceSurfaceReading, SurfaceDescriptor, SurfaceKind, SESSION_SPACE_VERSION,
};
use aikit_tui::{working_field_from_session_space, WorkingFieldAvailability};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn model(component_state: SessionSpaceActivationState) -> SessionSpaceReadModel {
    SessionSpaceReadModel {
        version: SESSION_SPACE_VERSION.into(),
        id: SessionSpaceRef::parse("session-space/live-field").unwrap(),
        lifecycle: SessionSpaceLifecycle::Open,
        revision: 7,
        projects: vec![r("project/demo")],
        agent_sessions: vec![SessionSpaceAgentSession {
            agent_session: r("agent-session/live"),
            harness: r("harness/deepseek"),
            native_session_id: Some("native-live".into()),
            provider: Some(r("provider/acp")),
            provenance: vec!["explicit live AgentSession binding".into()],
        }],
        components: vec![SessionSpaceComponent {
            agent_session: r("agent-session/live"),
            component: r("component/deepseek/client-ui-conversation"),
            harness: r("harness/deepseek"),
            activation_mode: CompositionActivationMode::LiveMounted,
            state: component_state,
            provider: Some(r("provider/deepseek/cordis-web")),
            observed_composition_fingerprint: Some("sha256:live-field-body".into()),
            reason: (component_state == SessionSpaceActivationState::Degraded)
                .then(|| "Cordis provider disappeared".into()),
            provenance: vec!["deepseek-ai/deepseek-harness@47f9438".into()],
        }],
        surfaces: vec![SessionSpaceSurfaceReading {
            agent_session: r("agent-session/live"),
            surface: r("surface/deepseek/web-conversation"),
            component: Some(r("component/deepseek/client-ui-conversation")),
            descriptor: SurfaceDescriptor {
                resource: r("surface/deepseek/web-conversation"),
                kind: SurfaceKind::Web,
                target_native_id: Some("client-ui-conversation".into()),
                owner_component: Some(r("component/deepseek/client-ui-conversation")),
            },
            state: component_state,
            provenance: vec!["Cordis Web contribution".into()],
        }],
        connections: vec![SessionSpaceConnection {
            connection: r("connection/acp/live"),
            provider: r("connection-adapter/acp/v1"),
            protocol: "acp/1".into(),
            agent_session: r("agent-session/live"),
            component: Some(r("component/deepseek/client-ui-conversation")),
            surface: None,
            state: SessionSpaceConnectionState::Connected,
            native_session_id: Some("native-live".into()),
            authority: SessionSpaceAuthorityState {
                capability: Some(r("capability/tools")),
                capability_available: true,
                capability_granted: false,
                action: Some(r("action/tools/run")),
                action_authorised: false,
                provenance: vec!["policy withheld grant".into()],
            },
            reason: None,
            provenance: vec!["stable ACP v1".into()],
        }],
        provenance: vec!["AIKit live SessionSpace".into()],
    }
}

#[test]
fn tui_projects_same_live_space_session_component_surface_and_connection() {
    let field = working_field_from_session_space(&model(SessionSpaceActivationState::Active)).unwrap();
    assert_eq!(field.enclosing_world, Some(r("session-space/live-field")));
    assert!(matches!(
        field
            .item(&r("session-space/live-field"))
            .unwrap()
            .availability,
        WorkingFieldAvailability::Available
    ));
    assert!(field.item(&r("agent-session/live")).is_some());

    let component = field
        .item(&r("component/deepseek/client-ui-conversation"))
        .unwrap();
    assert!(matches!(component.availability, WorkingFieldAvailability::Available));
    assert_eq!(component.surfaces.len(), 1);
    assert!(!component.surfaces[0].terminal_representation);
    assert!(component.surfaces[0]
        .alternate_reason
        .as_deref()
        .unwrap()
        .contains("peer projection"));

    let connection = field.item(&r("connection/acp/live")).unwrap();
    assert!(connection.permission.meaning.contains("not granted"));
    assert!(connection.actions.is_empty());
}

#[test]
fn tui_observes_degradation_instead_of_counterfeiting_active() {
    let field = working_field_from_session_space(&model(SessionSpaceActivationState::Degraded)).unwrap();
    let component = field
        .item(&r("component/deepseek/client-ui-conversation"))
        .unwrap();
    assert!(matches!(
        &component.availability,
        WorkingFieldAvailability::Degraded { reason } if reason.contains("provider disappeared")
    ));
}
