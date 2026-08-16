//! Attribute the existing #66 interactive connection seam to a live SessionSpace.
//!
//! This module is deliberately small: it does not create another channel or
//! lifecycle controller. It converts the already-normalised connection/session
//! facts into the core SessionSpace read model, requiring an explicit canonical
//! AgentSession binding and explicit authority disclosure from the owning policy.

use aikit_core::resource::ResourceRef;
use aikit_core::{
    AikitError, Result, SessionSpaceAuthorityState, SessionSpaceConnection,
    SessionSpaceConnectionState,
};

use crate::agent_connection::{
    ConnectionDescriptor, ConnectionProtocolFamily, ConnectionState, NativeSessionBinding,
};

pub fn connection_into_session_space(
    descriptor: &ConnectionDescriptor,
    state: ConnectionState,
    binding: &NativeSessionBinding,
    component: Option<ResourceRef>,
    surface: Option<ResourceRef>,
    authority: SessionSpaceAuthorityState,
) -> Result<SessionSpaceConnection> {
    let agent_session = binding.agent_session.clone().ok_or_else(|| {
        AikitError::new(
            "session_space.connection_unbound_agent_session",
            format!(
                "native session {} has no explicit canonical AgentSession binding",
                binding.native_session_id
            ),
        )
    })?;

    let protocol = match descriptor.protocol.family {
        ConnectionProtocolFamily::Acp => format!("acp/{}", descriptor.protocol.version),
        ConnectionProtocolFamily::ClassicProcess => {
            format!("classic-process/{}", descriptor.protocol.version)
        }
    };
    let mut provenance = descriptor.provenance.clone();
    provenance.extend(binding.provenance.iter().cloned());
    provenance.push(format!(
        "connection adapter={} native_session={} opened_as={:?}",
        descriptor.adapter_ref, binding.native_session_id, binding.opened_as
    ));

    Ok(SessionSpaceConnection {
        connection: descriptor.connection_ref.clone(),
        // The current #66 descriptor exposes the adapter/provider identity as the
        // concrete implementation boundary. Do not fabricate a second provider id.
        provider: descriptor.adapter_ref.clone(),
        protocol,
        agent_session,
        component,
        surface,
        state: map_connection_state(state),
        native_session_id: Some(binding.native_session_id.clone()),
        authority,
        reason: None,
        provenance,
    })
}

fn map_connection_state(state: ConnectionState) -> SessionSpaceConnectionState {
    match state {
        ConnectionState::Connecting => SessionSpaceConnectionState::Connecting,
        ConnectionState::Connected => SessionSpaceConnectionState::Connected,
        ConnectionState::Degraded | ConnectionState::Reconnecting => {
            SessionSpaceConnectionState::Degraded
        }
        ConnectionState::Closed => SessionSpaceConnectionState::Closed,
    }
}
