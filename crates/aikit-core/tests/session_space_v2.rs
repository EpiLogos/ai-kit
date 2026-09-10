use aikit_core::resource::ResourceRef;
use aikit_core::{
    ActivationScope, ActivationScopeKind, ComponentBinding, CompositionActivationMode,
    CompositionState, HarnessComposition, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
    ScopeKind, SessionSpaceActivationDriver, SessionSpaceActivationObservation,
    SessionSpaceActivationRequest, SessionSpaceActivationState, SessionSpaceAgentSession,
    SessionSpaceAuthorityState, SessionSpaceConnection, SessionSpaceConnectionState,
    SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime, SurfaceDescriptor, SurfaceKind,
    TargetNativeComponentBinding, HARNESS_COMPOSITION_VERSION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn space(raw: &str) -> SessionSpaceRuntime {
    SessionSpaceRuntime::open(
        SessionSpaceDefinition::new(SessionSpaceRef::parse(raw).unwrap())
            .with_provenance("session-space-v2 test fixture"),
    )
    .unwrap()
}

fn binding(native: &str) -> SessionSpaceAgentSession {
    SessionSpaceAgentSession {
        agent_session: r("agent-session/demo"),
        harness: r("harness/deepseek"),
        native_session_id: Some(native.into()),
        provider: Some(r("provider/acp")),
        provenance: vec!["explicit AgentSession binding".into()],
    }
}

fn live_component(id: &str) -> ComponentBinding {
    ComponentBinding {
        component: r(id),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "project composition"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("agent-session/demo"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession)
            .with_reference("agent-session/demo"),
        activation_mode: CompositionActivationMode::LiveMounted,
        implementation: Some(TargetNativeComponentBinding {
            implementation_target: "deepseek-ai/deepseek-harness".into(),
            native_id: id.into(),
            revision: Some("47f943859bef60e4160492346772ded9b24f765a".into()),
        }),
    }
}

fn composition(native: &str, components: &[&str]) -> HarnessComposition {
    let component_bindings: Vec<_> = components.iter().map(|id| live_component(id)).collect();
    let mut surfaces = Vec::new();
    for (index, component) in components.iter().enumerate() {
        surfaces.push(SurfaceDescriptor {
            resource: r(&format!("surface/demo/{index}/primary")),
            kind: SurfaceKind::Tui,
            target_native_id: Some(format!("native-surface-{index}")),
            owner_component: Some(r(component)),
        });
        if index == 0 {
            surfaces.push(SurfaceDescriptor {
                resource: r("surface/demo/0/secondary"),
                kind: SurfaceKind::Web,
                target_native_id: Some("native-secondary".into()),
                owner_component: Some(r(component)),
            });
        }
    }
    HarnessComposition {
        version: HARNESS_COMPOSITION_VERSION.into(),
        harness: r("harness/deepseek"),
        project: Some(r("project/demo")),
        agent: Some(r("agent/demo")),
        agency: None,
        session: Some(native.into()),
        model: None,
        component_bindings,
        contract_bindings: vec![],
        contributions: vec![],
        surfaces,
        projections: vec![],
        absences: vec![],
        state: CompositionState::Resolved,
        target_revision: Some("47f943859bef60e4160492346772ded9b24f765a".into()),
        generation: None,
        fingerprint: format!("fixture/{native}/{}", components.join("+")),
    }
}

#[derive(Default)]
struct LiveDriver {
    active: Vec<ResourceRef>,
}

impl SessionSpaceActivationDriver for LiveDriver {
    fn activate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> aikit_core::Result<SessionSpaceActivationObservation> {
        self.active.push(request.component.component.clone());
        Ok(SessionSpaceActivationObservation::Active {
            provider: r("provider/deepseek/cordis"),
            provenance: vec![format!(
                "live activation for {} in {}",
                request.component.component, request.space
            )],
        })
    }

    fn deactivate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> aikit_core::Result<SessionSpaceActivationObservation> {
        self.active
            .retain(|component| component != &request.component.component);
        Ok(SessionSpaceActivationObservation::Unavailable {
            provider: r("provider/deepseek/cordis"),
            reason: "live component retracted".into(),
            provenance: vec!["provider confirmed retraction".into()],
        })
    }
}

#[test]
fn two_spaces_with_the_same_component_identity_remain_isolated() {
    let mut left = space("session-space/left");
    let mut right = space("session-space/right");
    let left_lease = left.bind_agent_session(binding("native-left")).unwrap();
    let right_lease = right.bind_agent_session(binding("native-right")).unwrap();
    left.admit_composition(
        &left_lease,
        composition("native-left", &["component/shared"]),
    )
    .unwrap();
    right
        .admit_composition(
            &right_lease,
            composition("native-right", &["component/shared"]),
        )
        .unwrap();

    let mut driver = LiveDriver::default();
    left.activate_component(&left_lease, &r("component/shared"), &mut driver)
        .unwrap();

    assert_eq!(
        left.read_model().components[0].state,
        SessionSpaceActivationState::Active
    );
    assert_eq!(
        right.read_model().components[0].state,
        SessionSpaceActivationState::Eligible
    );
    assert_eq!(left.read_model().id.to_string(), "session-space/left");
    assert_eq!(right.read_model().id.to_string(), "session-space/right");
}

#[test]
fn one_component_can_contribute_multiple_surfaces_and_surface_identity_survives_recomposition() {
    let mut runtime = space("session-space/surfaces");
    let lease = runtime.bind_agent_session(binding("native-surface")).unwrap();
    runtime
        .admit_composition(
            &lease,
            composition("native-surface", &["component/ui", "component/worker"]),
        )
        .unwrap();
    let before: Vec<_> = runtime
        .read_model()
        .surfaces
        .into_iter()
        .filter(|surface| surface.component.as_ref() == Some(&r("component/ui")))
        .map(|surface| surface.surface)
        .collect();
    assert_eq!(before.len(), 2);

    runtime
        .admit_composition(
            &lease,
            composition("native-surface", &["component/ui"]),
        )
        .unwrap();
    let after: Vec<_> = runtime
        .read_model()
        .surfaces
        .into_iter()
        .map(|surface| surface.surface)
        .collect();
    assert_eq!(after, before);
    assert_eq!(runtime.read_model().components.len(), 2);
    assert!(runtime
        .read_model()
        .components
        .iter()
        .any(|component| component.component == r("component/worker")
            && component.state == SessionSpaceActivationState::Removed));
}

#[test]
fn stale_binding_cannot_mutate_after_rebind_or_space_close() {
    let mut runtime = space("session-space/rebind");
    let stale = runtime.bind_agent_session(binding("native-old")).unwrap();
    let fresh = runtime.bind_agent_session(binding("native-new")).unwrap();

    let error = runtime
        .admit_composition(&stale, composition("native-old", &["component/stale"]))
        .unwrap_err();
    assert_eq!(error.code(), "session_space.stale_lease");

    runtime
        .admit_composition(&fresh, composition("native-new", &["component/live"]))
        .unwrap();
    runtime.close().unwrap();
    let error = runtime
        .admit_composition(&fresh, composition("native-new", &["component/late"]))
        .unwrap_err();
    assert_eq!(error.code(), "session_space.closed");
    assert!(runtime.read_model().agent_sessions.is_empty());
    assert!(runtime.read_model().connections.is_empty());
    assert!(runtime.read_model().surfaces.is_empty());
}

#[test]
fn connection_presence_never_invents_capability_or_action_authority() {
    let mut runtime = space("session-space/authority");
    let lease = runtime.bind_agent_session(binding("native-auth")).unwrap();
    runtime
        .admit_composition(
            &lease,
            composition("native-auth", &["component/conversation"]),
        )
        .unwrap();
    runtime
        .observe_connection(
            &lease,
            SessionSpaceConnection {
                connection: r("connection/acp/demo"),
                provider: r("provider/acp"),
                protocol: "acp/v1".into(),
                agent_session: r("agent-session/demo"),
                component: Some(r("component/conversation")),
                surface: Some(r("surface/demo/0/primary")),
                state: SessionSpaceConnectionState::Connected,
                native_session_id: Some("native-auth".into()),
                authority: SessionSpaceAuthorityState {
                    capability: Some(r("capability/tools/bash")),
                    capability_available: true,
                    capability_granted: false,
                    action: Some(r("action/bash/run")),
                    action_authorised: false,
                    provenance: vec!["policy owner withheld grant".into()],
                },
                reason: None,
                provenance: vec!["ACP connection established".into()],
            },
        )
        .unwrap();

    let connection = &runtime.read_model().connections[0];
    assert_eq!(connection.state, SessionSpaceConnectionState::Connected);
    assert!(connection.authority.capability_available);
    assert!(!connection.authority.capability_granted);
    assert!(!connection.authority.action_authorised);
    assert!(!connection.authority.has_authority());
}

#[test]
fn provider_disappearance_degrades_active_component_and_surface_without_fake_continuity() {
    let mut runtime = space("session-space/provider-loss");
    let lease = runtime.bind_agent_session(binding("native-loss")).unwrap();
    runtime
        .admit_composition(
            &lease,
            composition("native-loss", &["component/conversation"]),
        )
        .unwrap();
    let mut driver = LiveDriver::default();
    runtime
        .activate_component(&lease, &r("component/conversation"), &mut driver)
        .unwrap();
    runtime
        .observe_provider_unavailable(
            &r("provider/deepseek/cordis"),
            "Cordis provider process exited",
        )
        .unwrap();

    let model = runtime.read_model();
    assert_eq!(model.components[0].state, SessionSpaceActivationState::Degraded);
    assert!(model
        .surfaces
        .iter()
        .all(|surface| surface.state == SessionSpaceActivationState::Degraded));
}

#[test]
fn next_session_descriptors_cannot_be_promoted_to_live_active() {
    let mut runtime = space("session-space/not-live");
    let lease = runtime.bind_agent_session(binding("native-next")).unwrap();
    let mut body = composition("native-next", &["component/next"]);
    body.component_bindings[0].activation_mode = CompositionActivationMode::NextSession;
    runtime.admit_composition(&lease, body).unwrap();
    let mut driver = LiveDriver::default();
    let error = runtime
        .activate_component(&lease, &r("component/next"), &mut driver)
        .unwrap_err();
    assert_eq!(error.code(), "session_space.activation_not_live");
    assert_eq!(
        runtime.read_model().components[0].state,
        SessionSpaceActivationState::Eligible
    );
}