use aikit_core::resource::ResourceRef;
use aikit_core::{
    CompositionState, HarnessComposition, SessionSpaceActivationState, SessionSpaceAgentSession,
    SessionSpaceAuthorityState, SessionSpaceConnection, SessionSpaceConnectionState,
    SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime, SurfaceDescriptor, SurfaceKind,
    HARNESS_COMPOSITION_VERSION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn runtime(id: &str) -> SessionSpaceRuntime {
    SessionSpaceRuntime::open(SessionSpaceDefinition::new(
        SessionSpaceRef::parse(id).unwrap(),
    ))
    .unwrap()
}

fn bind(runtime: &mut SessionSpaceRuntime, agent_session: &str) -> aikit_core::SessionSpaceLease {
    runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: r(agent_session),
            harness: r("harness/test"),
            native_session_id: None,
            provider: None,
            provenance: vec![format!("explicit binding for {agent_session}")],
        })
        .unwrap()
}

fn empty_composition(session: &str, surfaces: Vec<SurfaceDescriptor>) -> HarnessComposition {
    HarnessComposition {
        version: HARNESS_COMPOSITION_VERSION.into(),
        harness: r("harness/test"),
        project: Some(r("project/adversarial")),
        agent: Some(r("agent/adversarial")),
        agency: None,
        session: None,
        model: None,
        component_bindings: vec![],
        contract_bindings: vec![],
        contributions: vec![],
        surfaces,
        projections: vec![],
        absences: vec![],
        state: CompositionState::Resolved,
        target_revision: None,
        generation: None,
        fingerprint: format!("adversarial/{session}"),
    }
}

#[test]
fn surface_without_a_live_component_is_declared_not_active() {
    let mut runtime = runtime("session-space/declared-surface");
    let lease = bind(&mut runtime, "agent-session/declared-surface");
    runtime
        .admit_composition(
            &lease,
            empty_composition(
                "declared-surface",
                vec![SurfaceDescriptor {
                    resource: r("surface/orphan/web"),
                    kind: SurfaceKind::Web,
                    target_native_id: Some("orphan-web".into()),
                    owner_component: None,
                }],
            ),
        )
        .unwrap();

    let model = runtime.read_model();
    assert!(model.components.is_empty());
    assert_eq!(model.surfaces.len(), 1);
    assert_eq!(
        model.surfaces[0].state,
        SessionSpaceActivationState::Declared
    );
}

#[test]
fn capability_grant_still_does_not_authorise_an_action() {
    let authority = SessionSpaceAuthorityState {
        capability: Some(r("capability/project/write")),
        capability_available: true,
        capability_granted: true,
        action: Some(r("action/project/apply")),
        action_authorised: false,
        provenance: vec!["capability owner granted; Action owner withheld".into()],
    };

    assert!(authority.capability_available);
    assert!(authority.capability_granted);
    assert!(!authority.action_authorised);
    assert!(!authority.has_authority());
}

#[test]
fn closing_a_space_with_a_live_connection_removes_the_runtime_binding() {
    let mut runtime = runtime("session-space/close-live-connection");
    let lease = bind(&mut runtime, "agent-session/close-live-connection");
    runtime
        .observe_connection(
            &lease,
            SessionSpaceConnection {
                connection: r("connection/classic/live"),
                provider: r("connection-adapter/classic/process"),
                protocol: "classic-process/1".into(),
                agent_session: r("agent-session/close-live-connection"),
                component: None,
                surface: None,
                state: SessionSpaceConnectionState::Connected,
                native_session_id: Some("native-classic".into()),
                authority: SessionSpaceAuthorityState::default(),
                reason: None,
                provenance: vec!["live classic connection".into()],
            },
        )
        .unwrap();
    assert_eq!(runtime.read_model().connections.len(), 1);

    runtime.close().unwrap();
    assert!(runtime.read_model().connections.is_empty());
    assert!(runtime.read_model().agent_sessions.is_empty());
}

#[test]
fn multiple_agent_sessions_coexist_without_becoming_one_session_identity() {
    let mut runtime = runtime("session-space/multi-session");
    bind(&mut runtime, "agent-session/alpha");
    bind(&mut runtime, "agent-session/beta");

    let model = runtime.read_model();
    assert_eq!(model.agent_sessions.len(), 2);
    assert!(model
        .agent_sessions
        .iter()
        .any(|session| session.agent_session == r("agent-session/alpha")));
    assert!(model
        .agent_sessions
        .iter()
        .any(|session| session.agent_session == r("agent-session/beta")));
}
