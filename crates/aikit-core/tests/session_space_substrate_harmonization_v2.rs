use aikit_core::resource::ResourceRef;
use aikit_core::{
    ActivationScope, ActivationScopeKind, ComponentBinding, CompositionActivationMode,
    CompositionState, HarnessComposition, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
    ScopeKind, SessionSpaceActivationDriver, SessionSpaceActivationObservation,
    SessionSpaceActivationRequest, SessionSpaceActivationState, SessionSpaceAgentSession,
    SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime, SurfaceDescriptor, SurfaceKind,
    HARNESS_COMPOSITION_VERSION,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn bind(runtime: &mut SessionSpaceRuntime) -> aikit_core::SessionSpaceLease {
    runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: r("agent-session/harmonization"),
            harness: r("harness/harmonization"),
            native_session_id: Some("native-harmonization".into()),
            provider: None,
            provenance: vec!["explicit test AgentSession binding".into()],
        })
        .unwrap()
}

fn live_component() -> ComponentBinding {
    ComponentBinding {
        component: r("component/harmonization/runtime"),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, "canonical project composition"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("agent-session/harmonization"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession)
            .with_reference("agent-session/harmonization"),
        activation_mode: CompositionActivationMode::LiveMounted,
        implementation: None,
    }
}

fn surface(id: &str) -> SurfaceDescriptor {
    SurfaceDescriptor {
        resource: r(id),
        kind: SurfaceKind::Tui,
        target_native_id: None,
        owner_component: Some(r("component/harmonization/runtime")),
    }
}

fn body(fingerprint: &str, surfaces: Vec<SurfaceDescriptor>) -> HarnessComposition {
    HarnessComposition {
        version: HARNESS_COMPOSITION_VERSION.into(),
        harness: r("harness/harmonization"),
        project: Some(r("project/observed-from-body")),
        agent: Some(r("agent/harmonization")),
        agency: None,
        session: Some("native-harmonization".into()),
        model: None,
        component_bindings: vec![live_component()],
        contract_bindings: vec![],
        contributions: vec![],
        surfaces,
        projections: vec![],
        absences: vec![],
        state: CompositionState::Resolved,
        target_revision: None,
        generation: None,
        fingerprint: fingerprint.into(),
    }
}

#[derive(Default)]
struct ObservingDriver;

impl SessionSpaceActivationDriver for ObservingDriver {
    fn activate(
        &mut self,
        _request: &SessionSpaceActivationRequest,
    ) -> aikit_core::Result<SessionSpaceActivationObservation> {
        Ok(SessionSpaceActivationObservation::Active {
            provider: r("provider/harmonization/live"),
            provenance: vec!["provider observed exact requested body".into()],
        })
    }

    fn deactivate(
        &mut self,
        _request: &SessionSpaceActivationRequest,
    ) -> aikit_core::Result<SessionSpaceActivationObservation> {
        Ok(SessionSpaceActivationObservation::Deactivated {
            provider: r("provider/harmonization/live"),
            provenance: vec!["provider confirmed deactivation".into()],
        })
    }
}

#[test]
fn admitted_composition_project_provenance_never_authors_session_space_membership() {
    let mut runtime = SessionSpaceRuntime::open(SessionSpaceDefinition::new(
        SessionSpaceRef::parse("session-space/no-implicit-project").unwrap(),
    ))
    .unwrap();
    let lease = bind(&mut runtime);
    runtime
        .admit_composition(
            &lease,
            body("body/project-provenance", vec![surface("surface/harmonization/main")]),
        )
        .unwrap();

    assert!(
        runtime.read_model().projects.is_empty(),
        "a resolved HarnessComposition may carry Project provenance but cannot author SessionSpace membership"
    );

    let explicit = SessionSpaceRuntime::open(
        SessionSpaceDefinition::new(SessionSpaceRef::parse("session-space/explicit-project").unwrap())
            .with_project(r("project/explicit")),
    )
    .unwrap();
    assert_eq!(explicit.read_model().projects, vec![r("project/explicit")]);
}

#[test]
fn changed_composition_fingerprint_invalidates_live_evidence_and_exact_surface_membership() {
    let mut runtime = SessionSpaceRuntime::open(SessionSpaceDefinition::new(
        SessionSpaceRef::parse("session-space/body-evidence").unwrap(),
    ))
    .unwrap();
    let lease = bind(&mut runtime);
    runtime
        .admit_composition(
            &lease,
            body(
                "body/v1",
                vec![
                    surface("surface/harmonization/main"),
                    surface("surface/harmonization/secondary"),
                ],
            ),
        )
        .unwrap();

    let mut driver = ObservingDriver;
    runtime
        .activate_component(
            &lease,
            &r("component/harmonization/runtime"),
            &mut driver,
        )
        .unwrap();
    let active = runtime.read_model();
    let component = &active.components[0];
    assert_eq!(component.state, SessionSpaceActivationState::Active);
    assert_eq!(
        component.observed_composition_fingerprint.as_deref(),
        Some("body/v1")
    );
    assert_eq!(active.surfaces.len(), 2);

    runtime
        .admit_composition(
            &lease,
            body("body/v2", vec![surface("surface/harmonization/main")]),
        )
        .unwrap();
    let recomposed = runtime.read_model();
    let component = &recomposed.components[0];
    assert_eq!(
        component.state,
        SessionSpaceActivationState::Eligible,
        "provider evidence for body/v1 cannot prove body/v2 active"
    );
    assert!(component.provider.is_none());
    assert!(component.observed_composition_fingerprint.is_none());
    assert_eq!(
        recomposed
            .surfaces
            .iter()
            .map(|reading| reading.surface.clone())
            .collect::<Vec<_>>(),
        vec![r("surface/harmonization/main")],
        "Surface membership follows the exact canonical body even when its owner Component remains"
    );
}
