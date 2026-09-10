use std::collections::BTreeSet;
use std::time::Duration;

use aikit_adapters::{
    connection_into_session_space, deepseek_live_cordis_composition, deepseek_maximal_conformance,
    ConnectionCapabilities, ConnectionDescriptor, ConnectionProtocol, ConnectionProtocolFamily,
    ConnectionState, CordisActivationGrant, CordisActivationOperation,
    CordisProcessActivationDriver, CordisProcessSpec, DeepSeekShellProvider, NativeSessionBinding,
    SessionOpenMode, DEEPSEEK_HARNESS_UPSTREAM_REVISION, DEEPSEEK_LIVE_CORDIS_COMPONENTS,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{
    resolve_harness_composition, CompositionActivationMode, SessionSpaceActivationState,
    SessionSpaceAgentSession, SessionSpaceAuthorityState, SessionSpaceConnectionState,
    SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn current_deepseek_adapter_resolves_proven_live_modes_into_the_canonical_fingerprint() {
    let baseline_specimen = deepseek_maximal_conformance(DeepSeekShellProvider::Local).specimen;
    let baseline =
        resolve_harness_composition(&baseline_specimen.catalog, baseline_specimen.request).unwrap();

    let live = deepseek_live_cordis_composition(DeepSeekShellProvider::Local).unwrap();
    let repeated = deepseek_live_cordis_composition(DeepSeekShellProvider::Local).unwrap();
    let expected: BTreeSet<_> = DEEPSEEK_LIVE_CORDIS_COMPONENTS.iter().copied().collect();
    let observed: BTreeSet<_> = live
        .composition
        .component_bindings
        .iter()
        .filter(|binding| binding.activation_mode == CompositionActivationMode::LiveMounted)
        .map(|binding| binding.component.as_str())
        .collect();
    assert_eq!(observed, expected);

    for next_session in [
        "component/deepseek/tool-bash",
        "component/deepseek/agent-loop",
    ] {
        assert!(live.composition.component_bindings.iter().any(|binding| {
            binding.component == r(next_session)
                && binding.activation_mode == CompositionActivationMode::NextSession
        }));
    }
    assert!(live.composition.component_bindings.iter().all(|binding| {
        binding
            .implementation
            .as_ref()
            .and_then(|implementation| implementation.revision.as_deref())
            == Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION)
    }));

    assert_ne!(
        live.composition.fingerprint, baseline.fingerprint,
        "target-proven activation modes are canonical resolver inputs and must change body identity"
    );
    assert_eq!(
        live.composition.fingerprint, repeated.composition.fingerprint,
        "identical explicit target evidence must resolve deterministically"
    );
}

#[test]
fn acp_connection_requires_explicit_agent_session_binding_and_preserves_withheld_authority() {
    let descriptor = ConnectionDescriptor {
        adapter_ref: r("connection-adapter/acp/v1"),
        connection_ref: r("connection/acp/live"),
        protocol: ConnectionProtocol {
            family: ConnectionProtocolFamily::Acp,
            version: "1".into(),
        },
        capabilities: ConnectionCapabilities::default(),
        provenance: vec!["stable ACP v1".into()],
    };
    let unbound = NativeSessionBinding::unbound("native-1", SessionOpenMode::Create);
    let error = connection_into_session_space(
        &descriptor,
        ConnectionState::Connected,
        &unbound,
        None,
        None,
        SessionSpaceAuthorityState::default(),
    )
    .unwrap_err();
    assert_eq!(
        error.code(),
        "session_space.connection_unbound_agent_session"
    );

    let binding = unbound.bind_agent_session(r("agent-session/live"));
    let connection = connection_into_session_space(
        &descriptor,
        ConnectionState::Connected,
        &binding,
        Some(r("component/deepseek/agent-loop")),
        None,
        SessionSpaceAuthorityState {
            capability: Some(r("capability/tools")),
            capability_available: true,
            capability_granted: false,
            action: Some(r("action/tool/run")),
            action_authorised: false,
            provenance: vec!["owning policy withheld grant".into()],
        },
    )
    .unwrap();
    assert_eq!(connection.agent_session, r("agent-session/live"));
    assert_eq!(connection.native_session_id.as_deref(), Some("native-1"));
    assert_eq!(connection.protocol, "acp/1");
    assert!(!connection.authority.has_authority());
}

#[test]
fn acp_degradation_does_not_counterfeit_classic_harness_failure() {
    let mut runtime = SessionSpaceRuntime::open(SessionSpaceDefinition::new(
        SessionSpaceRef::parse("session-space/mixed-connections").unwrap(),
    ))
    .unwrap();
    let lease = runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: r("agent-session/mixed"),
            harness: r("harness/mixed"),
            native_session_id: None,
            provider: None,
            provenance: vec!["mixed connection fixture".into()],
        })
        .unwrap();

    let acp = ConnectionDescriptor {
        adapter_ref: r("connection-adapter/acp/v1"),
        connection_ref: r("connection/acp/mixed"),
        protocol: ConnectionProtocol {
            family: ConnectionProtocolFamily::Acp,
            version: "1".into(),
        },
        capabilities: ConnectionCapabilities::default(),
        provenance: vec!["ACP provider".into()],
    };
    let classic = ConnectionDescriptor {
        adapter_ref: r("connection-adapter/classic/process"),
        connection_ref: r("connection/classic/mixed"),
        protocol: ConnectionProtocol {
            family: ConnectionProtocolFamily::ClassicProcess,
            version: "1".into(),
        },
        capabilities: ConnectionCapabilities::default(),
        provenance: vec!["classic process provider".into()],
    };
    let acp_binding = NativeSessionBinding::unbound("native-acp", SessionOpenMode::Attach)
        .bind_agent_session(r("agent-session/mixed"));
    let classic_binding = NativeSessionBinding::unbound("native-classic", SessionOpenMode::Attach)
        .bind_agent_session(r("agent-session/mixed"));

    runtime
        .observe_connection(
            &lease,
            connection_into_session_space(
                &acp,
                ConnectionState::Degraded,
                &acp_binding,
                None,
                None,
                SessionSpaceAuthorityState::default(),
            )
            .unwrap(),
        )
        .unwrap();
    runtime
        .observe_connection(
            &lease,
            connection_into_session_space(
                &classic,
                ConnectionState::Connected,
                &classic_binding,
                None,
                None,
                SessionSpaceAuthorityState::default(),
            )
            .unwrap(),
        )
        .unwrap();

    let model = runtime.read_model();
    assert!(model.connections.iter().any(|connection| {
        connection.connection == r("connection/acp/mixed")
            && connection.state == SessionSpaceConnectionState::Degraded
    }));
    assert!(model.connections.iter().any(|connection| {
        connection.connection == r("connection/classic/mixed")
            && connection.state == SessionSpaceConnectionState::Connected
    }));
}

#[cfg(unix)]
#[test]
fn session_space_active_transition_is_backed_by_a_real_authorised_provider_process_lifecycle() {
    let live = deepseek_live_cordis_composition(DeepSeekShellProvider::Local).unwrap();
    let body_fingerprint = live.composition.fingerprint.clone();
    let space = SessionSpaceRef::parse("session-space/process-proof").unwrap();
    let agent_session = r("agent-session/process-proof");
    let harness = live.composition.harness.clone();
    let mut runtime =
        SessionSpaceRuntime::open(SessionSpaceDefinition::new(space.clone())).unwrap();
    let lease = runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: agent_session.clone(),
            harness: harness.clone(),
            native_session_id: None,
            provider: Some(r("provider/test-process")),
            provenance: vec!["test AgentSession binding".into()],
        })
        .unwrap();
    runtime.admit_composition(&lease, live.composition).unwrap();

    let spec = CordisProcessSpec {
        provider: r("provider/test-process"),
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "sleep 30".into()],
        working_directory: std::env::current_dir().unwrap(),
        readiness: None,
        startup_timeout: Duration::from_secs(2),
        provenance: vec!["real child-process lifecycle fixture".into()],
    };
    let mut driver = CordisProcessActivationDriver::new(spec);
    let component = r("component/deepseek/client-ui-conversation");

    let denied = runtime
        .activate_component(&lease, &component, &mut driver)
        .unwrap_err();
    assert_eq!(denied.code(), "cordis.activation.authority_required");
    assert!(!driver.is_running().unwrap());

    for (operation, grant_ref) in [
        (CordisActivationOperation::Activate, "authority/activate"),
        (
            CordisActivationOperation::Deactivate,
            "authority/deactivate",
        ),
    ] {
        driver
            .register_activation_grant(CordisActivationGrant {
                grant_ref: grant_ref.into(),
                authority_ref: format!("actuation/determination/{grant_ref}"),
                operation,
                space: space.clone(),
                agent_session: agent_session.clone(),
                harness: harness.clone(),
                component: component.clone(),
                composition_fingerprint: body_fingerprint.clone(),
                implementation_revision: DEEPSEEK_HARNESS_UPSTREAM_REVISION.into(),
                expires_at_unix_ms: u64::MAX,
                max_uses: 1,
            })
            .unwrap();
    }

    let state = runtime
        .activate_component(&lease, &component, &mut driver)
        .unwrap();
    assert_eq!(state, SessionSpaceActivationState::Active);
    assert!(driver.is_running().unwrap());
    let active_model = runtime.read_model();
    let active = active_model
        .components
        .iter()
        .find(|reading| reading.component == component)
        .unwrap();
    assert_eq!(active.state, SessionSpaceActivationState::Active);
    assert_eq!(
        active.observed_composition_fingerprint.as_deref(),
        Some(body_fingerprint.as_str())
    );
    assert!(active
        .provenance
        .iter()
        .any(|line| line.contains("actuation/determination/authority/activate")));
    let contributed_surfaces = active_model
        .surfaces
        .iter()
        .filter(|surface| surface.component.as_ref() == Some(&component))
        .count();
    assert!(contributed_surfaces > 0);

    let replay = runtime
        .activate_component(&lease, &component, &mut driver)
        .unwrap_err();
    assert_eq!(replay.code(), "cordis.activation.authority_exhausted");
    assert!(driver.is_running().unwrap());

    let state = runtime
        .deactivate_component(&lease, &component, &mut driver)
        .unwrap();
    assert_eq!(
        state,
        SessionSpaceActivationState::Eligible,
        "provider deactivation changes observed live truth; it is not canonical recomposition"
    );
    assert!(!driver.is_running().unwrap());
    let deactivated = runtime.read_model();
    assert!(deactivated.components.iter().any(|reading| {
        reading.component == component && reading.state == SessionSpaceActivationState::Eligible
    }));
    assert_eq!(
        deactivated
            .surfaces
            .iter()
            .filter(|surface| surface.component.as_ref() == Some(&component))
            .count(),
        contributed_surfaces,
        "desired Surface membership survives provider deactivation"
    );
}
