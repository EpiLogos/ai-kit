//! Bounded control transport for external working-environment providers.
//!
//! `working_environment` owns the provider semantics. This module only carries
//! those already-defined operations across a local JSON-line control boundary so
//! an editor/IDE extension can participate without importing Rust internals. It
//! does not define SessionSpace, Surface, AgentSession or connection identity.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::agent_connection::NativeSessionBinding;
use crate::working_environment::{
    WorkingEnvironmentCapabilities, WorkingEnvironmentObservation, WorkingEnvironmentProvider,
};

pub const WORKING_ENVIRONMENT_CONTROL_VERSION: &str =
    "aikit.working-environment-control/v1";

/// Explicit relation between an already-opened connection-native session and a
/// caller-owned canonical conversation Surface. The provider is allowed to bind
/// these identities; it is not allowed to mint either of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionSurfaceBinding {
    pub session: NativeSessionBinding,
    pub surface: ResourceRef,
}

impl AgentSessionSurfaceBinding {
    pub fn new(session: NativeSessionBinding, surface: ResourceRef) -> Result<Self> {
        if session.agent_session.is_none() {
            return Err(AikitError::new(
                "working_environment.agent_session_canonical_required",
                "AgentSession provider binding requires an explicit canonical AgentSession ref",
            ));
        }
        Ok(Self { session, surface })
    }

    pub fn agent_session(&self) -> &ResourceRef {
        self.session
            .agent_session
            .as_ref()
            .expect("AgentSessionSurfaceBinding validates canonical identity at construction")
    }
}

/// Optional lifecycle extension for providers which advertise
/// `agent_session_attach_detach`.
///
/// Connection/session continuity remains owned below this seam. Attach means
/// making an explicit already-existing `NativeSessionBinding` encounterable in
/// the provider; detach removes that provider relation; rebind changes the
/// provider-native session relation while preserving caller-supplied canonical
/// AgentSession identity.
pub trait AgentSessionWorkingEnvironmentProvider: WorkingEnvironmentProvider {
    fn attach_agent_session(&mut self, binding: &AgentSessionSurfaceBinding) -> Result<()>;
    fn detach_agent_session(&mut self, agent_session: &ResourceRef) -> Result<()>;
    fn rebind_agent_session(&mut self, binding: &AgentSessionSurfaceBinding) -> Result<()>;
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
enum ControlRequest {
    Describe { schema: String },
    Observe { schema: String },
    Open { schema: String },
    FocusSurface { schema: String, surface: ResourceRef },
    DetachSurface { schema: String, surface: ResourceRef },
    AttachAgentSession {
        schema: String,
        binding: AgentSessionSurfaceBinding,
    },
    DetachAgentSession {
        schema: String,
        agent_session: ResourceRef,
    },
    RebindAgentSession {
        schema: String,
        binding: AgentSessionSurfaceBinding,
    },
}

#[derive(Debug, Deserialize)]
struct ControlResponse {
    schema: String,
    ok: bool,
    #[serde(default)]
    provider: Option<ResourceRef>,
    #[serde(default)]
    capabilities: Option<WorkingEnvironmentCapabilities>,
    #[serde(default)]
    observation: Option<WorkingEnvironmentObservation>,
    #[serde(default)]
    error: Option<String>,
}

impl ControlResponse {
    fn validate(self) -> Result<Self> {
        if self.schema != WORKING_ENVIRONMENT_CONTROL_VERSION {
            return Err(AikitError::new(
                "working_environment.control_schema_mismatch",
                format!(
                    "provider control schema `{}` does not match `{WORKING_ENVIRONMENT_CONTROL_VERSION}`",
                    self.schema
                ),
            ));
        }
        if !self.ok {
            return Err(AikitError::new(
                "working_environment.control_remote_error",
                self.error
                    .clone()
                    .unwrap_or_else(|| "provider control operation failed without detail".into()),
            ));
        }
        Ok(self)
    }
}

/// Loopback/external-style control client for an editor/IDE provider fixture.
///
/// A fresh TCP connection is used for each request. The transport is deliberately
/// tiny and carries only the operations already named by the public provider
/// contract and its optional AgentSession lifecycle extension.
pub struct WorkingEnvironmentControlClient {
    address: String,
    provider: ResourceRef,
    capabilities: WorkingEnvironmentCapabilities,
}

impl WorkingEnvironmentControlClient {
    pub fn connect(address: impl Into<String>) -> Result<Self> {
        let address = address.into();
        let response = Self::request_at(
            &address,
            &ControlRequest::Describe {
                schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
            },
        )?;
        let provider = response.provider.ok_or_else(|| {
            AikitError::new(
                "working_environment.control_missing_provider",
                "provider describe response omitted canonical provider ref",
            )
        })?;
        let capabilities = response.capabilities.ok_or_else(|| {
            AikitError::new(
                "working_environment.control_missing_capabilities",
                "provider describe response omitted capabilities",
            )
        })?;
        Ok(Self {
            address,
            provider,
            capabilities,
        })
    }

    fn request(&self, request: &ControlRequest) -> Result<ControlResponse> {
        Self::request_at(&self.address, request)
    }

    fn request_at(address: &str, request: &ControlRequest) -> Result<ControlResponse> {
        let mut stream = TcpStream::connect(address).map_err(|error| {
            AikitError::new(
                "working_environment.control_connect_failed",
                format!("could not connect to working-environment provider at {address}: {error}"),
            )
        })?;
        let timeout = Some(Duration::from_secs(15));
        stream.set_read_timeout(timeout).map_err(|error| {
            AikitError::new(
                "working_environment.control_timeout_failed",
                format!("could not set provider read timeout: {error}"),
            )
        })?;
        stream.set_write_timeout(timeout).map_err(|error| {
            AikitError::new(
                "working_environment.control_timeout_failed",
                format!("could not set provider write timeout: {error}"),
            )
        })?;

        let encoded = serde_json::to_string(request).map_err(|error| {
            AikitError::new(
                "working_environment.control_encode_failed",
                format!("could not encode provider control request: {error}"),
            )
        })?;
        stream.write_all(encoded.as_bytes()).map_err(|error| {
            AikitError::new(
                "working_environment.control_write_failed",
                format!("could not write provider control request: {error}"),
            )
        })?;
        stream.write_all(b"\n").map_err(|error| {
            AikitError::new(
                "working_environment.control_write_failed",
                format!("could not terminate provider control request: {error}"),
            )
        })?;
        stream.flush().map_err(|error| {
            AikitError::new(
                "working_environment.control_write_failed",
                format!("could not flush provider control request: {error}"),
            )
        })?;

        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).map_err(|error| {
            AikitError::new(
                "working_environment.control_read_failed",
                format!("could not read provider control response: {error}"),
            )
        })?;
        if line.trim().is_empty() {
            return Err(AikitError::new(
                "working_environment.control_disconnected",
                "working-environment provider closed without a response",
            ));
        }
        serde_json::from_str::<ControlResponse>(line.trim())
            .map_err(|error| {
                AikitError::new(
                    "working_environment.control_decode_failed",
                    format!("could not decode provider control response: {error}"),
                )
            })?
            .validate()
    }

    fn expect_observation(response: ControlResponse) -> Result<WorkingEnvironmentObservation> {
        response.observation.ok_or_else(|| {
            AikitError::new(
                "working_environment.control_missing_observation",
                "provider control response omitted working-environment observation",
            )
        })
    }

    fn ensure_agent_session_lifecycle(&self) -> Result<()> {
        if !self.capabilities.agent_session_attach_detach {
            return Err(AikitError::new(
                "working_environment.agent_session_lifecycle_unsupported",
                format!(
                    "provider {} does not advertise AgentSession attach/detach",
                    self.provider
                ),
            ));
        }
        Ok(())
    }
}

impl WorkingEnvironmentProvider for WorkingEnvironmentControlClient {
    fn provider_ref(&self) -> &ResourceRef {
        &self.provider
    }

    fn capabilities(&self) -> WorkingEnvironmentCapabilities {
        self.capabilities.clone()
    }

    fn observe(&mut self) -> Result<WorkingEnvironmentObservation> {
        Self::expect_observation(self.request(&ControlRequest::Observe {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
        })?)
    }

    fn open(&mut self) -> Result<WorkingEnvironmentObservation> {
        Self::expect_observation(self.request(&ControlRequest::Open {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
        })?)
    }

    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        self.request(&ControlRequest::FocusSurface {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
            surface: surface.clone(),
        })?;
        Ok(())
    }

    fn detach_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        self.request(&ControlRequest::DetachSurface {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
            surface: surface.clone(),
        })?;
        Ok(())
    }
}

impl AgentSessionWorkingEnvironmentProvider for WorkingEnvironmentControlClient {
    fn attach_agent_session(&mut self, binding: &AgentSessionSurfaceBinding) -> Result<()> {
        self.ensure_agent_session_lifecycle()?;
        self.request(&ControlRequest::AttachAgentSession {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
            binding: binding.clone(),
        })?;
        Ok(())
    }

    fn detach_agent_session(&mut self, agent_session: &ResourceRef) -> Result<()> {
        self.ensure_agent_session_lifecycle()?;
        self.request(&ControlRequest::DetachAgentSession {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
            agent_session: agent_session.clone(),
        })?;
        Ok(())
    }

    fn rebind_agent_session(&mut self, binding: &AgentSessionSurfaceBinding) -> Result<()> {
        self.ensure_agent_session_lifecycle()?;
        self.request(&ControlRequest::RebindAgentSession {
            schema: WORKING_ENVIRONMENT_CONTROL_VERSION.into(),
            binding: binding.clone(),
        })?;
        Ok(())
    }
}
