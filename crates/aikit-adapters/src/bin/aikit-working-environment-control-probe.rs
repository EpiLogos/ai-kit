use std::env;

use aikit_adapters::{
    AgentSessionSurfaceBinding, AgentSessionWorkingEnvironmentProvider, NativeBindingKind,
    NativeSessionBinding, SessionOpenMode, WorkingEnvironmentControlClient,
    WorkingEnvironmentProvider,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {}", error.code(), error.message());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let address = env::var("AIKIT_WORKING_ENVIRONMENT_CONTROL_ADDR").map_err(|error| {
        AikitError::new(
            "working_environment.probe_address_missing",
            format!("AIKIT_WORKING_ENVIRONMENT_CONTROL_ADDR is required: {error}"),
        )
    })?;
    let mut provider = WorkingEnvironmentControlClient::connect(address)?;
    let capabilities = provider.capabilities();
    if !capabilities.conversation_surface || !capabilities.agent_session_attach_detach {
        return Err(AikitError::new(
            "working_environment.probe_capability_missing",
            "IDE provider must expose conversation Surface plus AgentSession attach/detach",
        ));
    }

    let agent_session = ResourceRef::parse("agent-session/vscode-provider-proof")?;
    let conversation_surface = ResourceRef::parse("surface/vscode/provider-proof-conversation")?;

    let first = binding(
        &agent_session,
        &conversation_surface,
        "acp-native-session-a",
        SessionOpenMode::Load,
    )?;
    provider.attach_agent_session(&first)?;
    let attached = provider.observe()?;
    expect_agent_session_binding(&attached, &agent_session, "acp-native-session-a")?;
    expect_surface_binding(&attached, &conversation_surface)?;

    provider.detach_agent_session(&agent_session)?;
    let detached = provider.observe()?;
    if detached.canonical_native_id(&agent_session).is_some()
        || detached.canonical_native_id(&conversation_surface).is_some()
    {
        return Err(AikitError::new(
            "working_environment.probe_detach_failed",
            "provider retained canonical AgentSession or conversation Surface binding after detach",
        ));
    }

    provider.attach_agent_session(&first)?;
    let second = binding(
        &agent_session,
        &conversation_surface,
        "acp-native-session-b",
        SessionOpenMode::Load,
    )?;
    provider.rebind_agent_session(&second)?;
    let rebound = provider.observe()?;
    expect_agent_session_binding(&rebound, &agent_session, "acp-native-session-b")?;
    expect_surface_binding(&rebound, &conversation_surface)?;
    if rebound
        .bindings
        .iter()
        .any(|binding| binding.native_id == "acp-native-session-a")
    {
        return Err(AikitError::new(
            "working_environment.probe_stale_native_binding",
            "provider retained the previous native session after canonical AgentSession rebind",
        ));
    }

    provider.focus_surface(&conversation_surface)?;
    provider.detach_surface(&conversation_surface)?;
    let surface_detached = provider.observe()?;
    if surface_detached.canonical_native_id(&agent_session).is_some()
        || surface_detached
            .canonical_native_id(&conversation_surface)
            .is_some()
    {
        return Err(AikitError::new(
            "working_environment.probe_surface_detach_failed",
            "detaching the bound conversation Surface did not remove the provider relation",
        ));
    }

    println!(
        "provider={} AgentSession lifecycle: attach -> detach -> attach -> rebind -> focus -> surface detach: PASS",
        provider.provider_ref()
    );
    Ok(())
}

fn binding(
    agent_session: &ResourceRef,
    surface: &ResourceRef,
    native_session_id: &str,
    opened_as: SessionOpenMode,
) -> Result<AgentSessionSurfaceBinding> {
    let mut session = NativeSessionBinding::unbound(native_session_id, opened_as)
        .bind_agent_session(agent_session.clone());
    session.provenance.push(
        "explicit connection-native session supplied by aikit.connection-adapter/v1".into(),
    );
    AgentSessionSurfaceBinding::new(session, surface.clone())
}

fn expect_agent_session_binding(
    observation: &aikit_adapters::WorkingEnvironmentObservation,
    agent_session: &ResourceRef,
    native_session_id: &str,
) -> Result<()> {
    let found = observation.bindings.iter().any(|binding| {
        binding.kind == NativeBindingKind::AgentSession
            && binding.canonical_ref.as_ref() == Some(agent_session)
            && binding.native_id == native_session_id
    });
    if found {
        Ok(())
    } else {
        Err(AikitError::new(
            "working_environment.probe_agent_session_binding_missing",
            format!(
                "provider observation did not bind canonical {agent_session} to native {native_session_id}"
            ),
        ))
    }
}

fn expect_surface_binding(
    observation: &aikit_adapters::WorkingEnvironmentObservation,
    surface: &ResourceRef,
) -> Result<()> {
    let found = observation.bindings.iter().any(|binding| {
        binding.kind == NativeBindingKind::Surface
            && binding.canonical_ref.as_ref() == Some(surface)
    });
    if found {
        Ok(())
    } else {
        Err(AikitError::new(
            "working_environment.probe_surface_binding_missing",
            format!("provider observation did not expose canonical conversation Surface {surface}"),
        ))
    }
}
