use std::collections::BTreeSet;
use std::time::Duration;

use aikit_adapters::{
    connection_into_session_space, deepseek_live_cordis_composition, ConnectionCapabilities,
    ConnectionDescriptor, ConnectionProtocol, ConnectionProtocolFamily, ConnectionState,
    CordisProcessActivationDriver, CordisProcessSpec, DeepSeekShellProvider, NativeSessionBinding,
    SessionOpenMode, DEEPSEEK_HARNESS_UPSTREAM_REVISION, DEEPSEEK_LIVE_CORDIS_COMPONENTS,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{
    CompositionActivationMode, SessionSpaceActivationState, SessionSpaceAgentSession,
    SessionSpaceAuthorityState, SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn current_deepseek_adapter_upgrades_only_proven_cordis_web_components_to_live_mounted() {
    let live = deepseek_live_cordis_composition(DeepSeekShellProvider::Local).unwrap();
    let expected: BTreeSet<_> = DEEPSEEK_LIVE_CORDIS_COMPONENTS.iter().copied().collect();
    let observed: BTreeSet<_> = live
        .composition
        .component_bindings
        .iter()
        .filter(|binding| binding.activation_mode == CompositionActivationMode::LiveMounted)
        .map(|binding| binding.component.as_str())
        .collect();
    assert_eq!(observed, expected);

    assert!(live.composition.component_bindings.iter().any(|binding| {
        binding.component == r("component/deepseek/tool-bash")
            && binding.activation_mode == CompositionActivationMode::NextSession
    }));
    assert!(live.composition.component_bindings.iter().all(|binding| {
        binding
            .implementation
            .as_ref()
            .and_then(|implementation| implementation.revision.as_deref())
            == Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION)
    }));
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
    assert_eq!(error.code(), "session_space.connection_unbound_agent_session");

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

#[cfg(unix)]
#[test]
fn session_space_active_transition_is_backed_by_a_real_provider_process_lifecycle() {
    let live = deepseek_live_cordis_composition(DeepSeekShellProvider::Local).unwrap();
    let mut runtime = SessionSpaceRuntime::open(SessionSpaceDefinition::new(
        SessionSpaceRef::parse("session-space/process-proof").unwrap(),
    ))
    .unwrap();
    let lease = runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: r("agent-session/process-proof"),
            harness: live.composition.harness.clone(),
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
    let component = r("component/deepseek/profile-root");
    let state = runtime
        .activate_component(&lease, &component, &mut driver)
        .unwrap();
    assert_eq!(state, SessionSpaceActivationState::Active);
    assert!(driver.is_running().unwrap());
    assert_eq!(
        runtime
            .read_model()
            .components
            .iter()
            .find(|reading| reading.component == component)
            .unwrap()
            .state,
        SessionSpaceActivationState::Active
    );

    let state = runtime
        .deactivate_component(&lease, &component, &mut driver)
        .unwrap();
    assert_eq!(state, SessionSpaceActivationState::Removed);
    assert!(!driver.is_running().unwrap());
}
