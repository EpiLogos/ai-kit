use std::path::PathBuf;

use aikit_adapters::{
    deepseek_live_cordis_composition, CordisProcessActivationDriver, DeepSeekShellProvider,
    DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{
    SessionSpaceActivationState, SessionSpaceAgentSession, SessionSpaceDefinition,
    SessionSpaceRef, SessionSpaceRuntime,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn real_pinned_deepseek_cordis_web_activates_inside_session_space() {
    let Some(checkout) = std::env::var_os("AIKIT_DEEPSEEK_HARNESS_CHECKOUT") else {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_DEEPSEEK_CORDIS_REAL").is_none(),
            "AIKIT_REQUIRE_DEEPSEEK_CORDIS_REAL is set but AIKIT_DEEPSEEK_HARNESS_CHECKOUT is absent"
        );
        return;
    };
    let checkout = PathBuf::from(checkout);
    assert!(checkout
        .join("packages/bundle/web-app/cordis.patch.yml")
        .is_file());

    let live = deepseek_live_cordis_composition(DeepSeekShellProvider::Local).unwrap();
    assert_eq!(
        live.composition.target_revision.as_deref(),
        Some(DEEPSEEK_HARNESS_UPSTREAM_REVISION)
    );
    let body_fingerprint = live.composition.fingerprint.clone();

    let mut runtime = SessionSpaceRuntime::open(
        SessionSpaceDefinition::new(SessionSpaceRef::parse("session-space/deepseek-real").unwrap())
            .with_provenance(format!(
                "real DeepSeek Harness Cordis acceptance @{DEEPSEEK_HARNESS_UPSTREAM_REVISION}"
            )),
    )
    .unwrap();
    let lease = runtime
        .bind_agent_session(SessionSpaceAgentSession {
            agent_session: r("agent-session/deepseek-real"),
            harness: live.composition.harness.clone(),
            native_session_id: None,
            provider: Some(r("provider/deepseek/cordis-web")),
            provenance: vec!["real provider acceptance binding".into()],
        })
        .unwrap();
    runtime.admit_composition(&lease, live.composition).unwrap();

    let component = r("component/deepseek/profile-root");
    let mut driver = CordisProcessActivationDriver::deepseek_web(&checkout);
    let state = runtime
        .activate_component(&lease, &component, &mut driver)
        .unwrap();
    assert_eq!(state, SessionSpaceActivationState::Active);
    assert!(driver.is_running().unwrap());
    let read_model = runtime.read_model();
    let active = read_model
        .components
        .iter()
        .find(|reading| reading.component == component)
        .unwrap();
    assert_eq!(active.state, SessionSpaceActivationState::Active);
    assert_eq!(
        active.provider.as_ref(),
        Some(&r("provider/deepseek/cordis-web"))
    );
    assert_eq!(
        active.observed_composition_fingerprint.as_deref(),
        Some(body_fingerprint.as_str())
    );
    assert!(active
        .provenance
        .iter()
        .any(|source| source.contains(DEEPSEEK_HARNESS_UPSTREAM_REVISION)));

    let deactivated = runtime
        .deactivate_component(&lease, &component, &mut driver)
        .unwrap();
    assert_eq!(
        deactivated,
        SessionSpaceActivationState::Eligible,
        "provider teardown must not counterfeit canonical recomposition"
    );
    assert!(!driver.is_running().unwrap());
    assert!(runtime.read_model().surfaces.iter().any(|surface| {
        surface.component.as_ref() == Some(&component)
    }));
}
