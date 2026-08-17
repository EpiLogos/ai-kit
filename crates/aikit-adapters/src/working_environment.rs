//! Bounded public working-environment conformance seam for SessionSpace.
//!
//! This module does not define a new multiplexer, layout language, workspace
//! store, or connection stack. It gives an external working-environment provider
//! a small way to expose observed truth while keeping provider-native ids as
//! bindings/provenance only. The existing [`crate::mux::MuxAdapter`] remains the
//! implementation boundary for tmux and cmux.

use std::collections::BTreeMap;

use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionPlan;
use aikit_core::session_space::{
    SessionSpaceActivationDriver, SessionSpaceActivationObservation,
    SessionSpaceActivationRequest,
};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::mux::{MuxAdapter, MuxTarget, ReconcileMode, SessionBinding};

pub const WORKING_ENVIRONMENT_PROVIDER_VERSION: &str =
    "aikit.working-environment-provider/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkingEnvironmentHealth {
    Healthy,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeBindingKind {
    Session,
    View,
    Surface,
    Project,
    AgentSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkingEnvironmentCapabilities {
    pub discover: bool,
    pub open: bool,
    pub focus: bool,
    pub select: bool,
    pub multi_project: bool,
    pub editor_surface: bool,
    pub terminal_surface: bool,
    pub conversation_surface: bool,
    pub diff_surface: bool,
    pub preview_surface: bool,
    pub test_surface: bool,
    pub surface_attach_detach: bool,
    pub agent_session_attach_detach: bool,
    pub reconstruct: bool,
}

/// One provider-native fact. `canonical_ref` is populated only by an explicit
/// caller/provider binding. It is never derived from `native_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderNativeBinding {
    pub kind: NativeBindingKind,
    pub native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_ref: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingEnvironmentObservation {
    pub schema: String,
    pub provider: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    pub health: WorkingEnvironmentHealth,
    pub capabilities: WorkingEnvironmentCapabilities,
    #[serde(default)]
    pub bindings: Vec<ProviderNativeBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_native_id: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl WorkingEnvironmentObservation {
    pub fn canonical_native_id(&self, canonical: &ResourceRef) -> Option<&str> {
        self.bindings
            .iter()
            .find(|binding| binding.canonical_ref.as_ref() == Some(canonical))
            .map(|binding| binding.native_id.as_str())
    }
}

/// Small public participation seam for a mux/IDE/desktop provider.
///
/// Canonical identity is always supplied by the caller in provider-specific
/// binding configuration. An implementation which only knows a native id must
/// report it with `canonical_ref = None`.
pub trait WorkingEnvironmentProvider {
    fn provider_ref(&self) -> &ResourceRef;
    fn capabilities(&self) -> WorkingEnvironmentCapabilities;
    fn observe(&mut self) -> Result<WorkingEnvironmentObservation>;
    fn open(&mut self) -> Result<WorkingEnvironmentObservation>;
    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()>;
    fn detach_surface(&mut self, surface: &ResourceRef) -> Result<()>;
}

/// Conformance wrapper over the existing shared mux contract. It contains no
/// provider geometry of its own; plan logical keys are explicitly bound to
/// canonical Surface refs and resolved by `SessionBinding` after the real mux
/// operation runs.
pub struct MuxWorkingEnvironment<A> {
    adapter: A,
    plan: SessionPlan,
    provider: ResourceRef,
    surface_bindings: BTreeMap<ResourceRef, String>,
    project_bindings: BTreeMap<ResourceRef, String>,
    agent_session_bindings: BTreeMap<ResourceRef, String>,
    last_binding: Option<SessionBinding>,
}

impl<A: MuxAdapter> MuxWorkingEnvironment<A> {
    pub fn new(adapter: A, plan: SessionPlan, provider: ResourceRef) -> Self {
        Self {
            adapter,
            plan,
            provider,
            surface_bindings: BTreeMap::new(),
            project_bindings: BTreeMap::new(),
            agent_session_bindings: BTreeMap::new(),
            last_binding: None,
        }
    }

    /// `plan_key` is the existing mux plan key (`view/pane`), not a canonical id.
    #[must_use]
    pub fn bind_surface(mut self, surface: ResourceRef, plan_key: impl Into<String>) -> Self {
        self.surface_bindings.insert(surface, plan_key.into());
        self
    }

    /// The native project/workspace marker is provenance only. Supplying it is
    /// an explicit relation and never changes Project identity.
    #[must_use]
    pub fn bind_project(mut self, project: ResourceRef, native_id: impl Into<String>) -> Self {
        self.project_bindings.insert(project, native_id.into());
        self
    }

    #[must_use]
    pub fn bind_agent_session(
        mut self,
        agent_session: ResourceRef,
        native_id: impl Into<String>,
    ) -> Self {
        self.agent_session_bindings
            .insert(agent_session, native_id.into());
        self
    }

    pub fn adapter(&self) -> &A {
        &self.adapter
    }

    pub fn adapter_mut(&mut self) -> &mut A {
        &mut self.adapter
    }

    pub fn plan(&self) -> &SessionPlan {
        &self.plan
    }

    pub fn last_binding(&self) -> Option<&SessionBinding> {
        self.last_binding.as_ref()
    }

    fn mux_capabilities(&self) -> WorkingEnvironmentCapabilities {
        let mux = self.adapter.capabilities();
        WorkingEnvironmentCapabilities {
            discover: true,
            open: mux.workspaces,
            focus: mux.workspaces || mux.panes,
            select: mux.workspaces || mux.panes,
            // Projects are AIKit semantic membership. Both first-party muxes can
            // display several explicitly-bound project paths without owning them.
            multi_project: true,
            editor_surface: false,
            terminal_surface: mux.panes || mux.workspaces,
            conversation_surface: false,
            diff_surface: false,
            preview_surface: mux.browser_surface,
            test_surface: false,
            surface_attach_detach: mux.panes || mux.workspaces,
            // A mux has no AgentSession protocol. It can host an explicitly bound
            // AgentSession surface, but cannot create/resume that identity itself.
            agent_session_attach_detach: false,
            reconstruct: mux.workspaces,
        }
    }

    fn observation_from_binding(
        &self,
        binding: Option<&SessionBinding>,
    ) -> Result<WorkingEnvironmentObservation> {
        let presence = self.adapter.detect()?;
        let capabilities = self.mux_capabilities();
        let health = if !presence.installed {
            WorkingEnvironmentHealth::Unavailable
        } else if presence.detail.is_some() && !presence.server_running && binding.is_some() {
            WorkingEnvironmentHealth::Degraded
        } else {
            WorkingEnvironmentHealth::Healthy
        };
        let mut bindings = Vec::new();
        if let Some(binding) = binding {
            bindings.push(ProviderNativeBinding {
                kind: NativeBindingKind::Session,
                native_id: binding.session.clone(),
                canonical_ref: None,
                provenance: vec![format!("{} session binding", self.adapter.kind())],
            });
            for (logical, native) in &binding.views {
                bindings.push(ProviderNativeBinding {
                    kind: NativeBindingKind::View,
                    native_id: native.clone(),
                    canonical_ref: None,
                    provenance: vec![format!(
                        "{} view logical_key={logical}",
                        self.adapter.kind()
                    )],
                });
            }
            for (canonical, logical) in &self.surface_bindings {
                if let Some(native) = binding.surfaces.get(logical) {
                    bindings.push(ProviderNativeBinding {
                        kind: NativeBindingKind::Surface,
                        native_id: native.clone(),
                        canonical_ref: Some(canonical.clone()),
                        provenance: vec![format!(
                            "explicit {} surface binding logical_key={logical}",
                            self.adapter.kind()
                        )],
                    });
                }
            }
        }
        bindings.extend(self.project_bindings.iter().map(|(canonical, native)| {
            ProviderNativeBinding {
                kind: NativeBindingKind::Project,
                native_id: native.clone(),
                canonical_ref: Some(canonical.clone()),
                provenance: vec!["explicit Project/provider binding".into()],
            }
        }));
        bindings.extend(
            self.agent_session_bindings
                .iter()
                .map(|(canonical, native)| ProviderNativeBinding {
                    kind: NativeBindingKind::AgentSession,
                    native_id: native.clone(),
                    canonical_ref: Some(canonical.clone()),
                    provenance: vec!["explicit AgentSession/provider binding".into()],
                }),
        );
        Ok(WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: self.provider.clone(),
            provider_version: presence.version,
            health,
            capabilities,
            bindings,
            focused_native_id: self
                .adapter
                .current_location()
                .ok()
                .map(|location| location.target().selector())
                .filter(|id| !id.is_empty()),
            provenance: vec![format!(
                "{} via aikit MuxAdapter; provider-native ids are provenance",
                self.adapter.kind()
            )],
        })
    }

    fn ensure_current_binding(&mut self) -> Result<&SessionBinding> {
        if self.last_binding.is_none() {
            let binding = self.adapter.inspect_session(&self.plan)?;
            self.last_binding = Some(binding);
        }
        Ok(self
            .last_binding
            .as_ref()
            .expect("binding is populated above"))
    }
}

impl<A: MuxAdapter> WorkingEnvironmentProvider for MuxWorkingEnvironment<A> {
    fn provider_ref(&self) -> &ResourceRef {
        &self.provider
    }

    fn capabilities(&self) -> WorkingEnvironmentCapabilities {
        self.mux_capabilities()
    }

    fn observe(&mut self) -> Result<WorkingEnvironmentObservation> {
        if !self.adapter.session_exists(&self.plan)? {
            self.last_binding = None;
            return self.observation_from_binding(None);
        }
        let binding = self.adapter.inspect_session(&self.plan)?;
        self.last_binding = Some(binding);
        self.observation_from_binding(self.last_binding.as_ref())
    }

    fn open(&mut self) -> Result<WorkingEnvironmentObservation> {
        let binding = self
            .adapter
            .ensure_session(&self.plan, ReconcileMode::CreateOrAttach)?;
        self.last_binding = Some(binding);
        self.observation_from_binding(self.last_binding.as_ref())
    }

    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        let logical = self.surface_bindings.get(surface).cloned().ok_or_else(|| {
            AikitError::new(
                "working_environment.surface_unbound",
                format!("Surface {surface} has no explicit provider binding"),
            )
        })?;
        let binding = self.ensure_current_binding()?;
        let native = binding.surfaces.get(&logical).cloned().ok_or_else(|| {
            AikitError::new(
                "working_environment.surface_native_absent",
                format!("provider has no live surface for logical key {logical}"),
            )
        })?;
        self.adapter
            .focus(&MuxTarget::surface(self.adapter.kind(), native))
    }

    fn detach_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        let logical = self.surface_bindings.get(surface).cloned().ok_or_else(|| {
            AikitError::new(
                "working_environment.surface_unbound",
                format!("Surface {surface} has no explicit provider binding"),
            )
        })?;
        let binding = self.ensure_current_binding()?;
        let native = binding.surfaces.get(&logical).cloned().ok_or_else(|| {
            AikitError::new(
                "working_environment.surface_native_absent",
                format!("provider has no live surface for logical key {logical}"),
            )
        })?;
        self.adapter
            .close(&MuxTarget::surface(self.adapter.kind(), native))?;
        self.last_binding = None;
        Ok(())
    }
}

/// SessionSpace activation over the bounded working-environment seam. This is a
/// conformance bridge, not a SessionSpace application service: it mutates no
/// Project/Profile/SkillSet state and persists nothing.
pub struct MuxSessionSpaceActivationDriver<A> {
    environment: MuxWorkingEnvironment<A>,
}

impl<A: MuxAdapter> MuxSessionSpaceActivationDriver<A> {
    pub fn new(environment: MuxWorkingEnvironment<A>) -> Self {
        Self { environment }
    }

    pub fn environment(&self) -> &MuxWorkingEnvironment<A> {
        &self.environment
    }

    pub fn environment_mut(&mut self) -> &mut MuxWorkingEnvironment<A> {
        &mut self.environment
    }

    pub fn into_environment(self) -> MuxWorkingEnvironment<A> {
        self.environment
    }

    fn observation_provenance(observation: &WorkingEnvironmentObservation) -> Vec<String> {
        let mut provenance = observation.provenance.clone();
        for binding in &observation.bindings {
            provenance.push(format!(
                "provider-native {:?} id={} canonical={}",
                binding.kind,
                binding.native_id,
                binding
                    .canonical_ref
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "unbound".into())
            ));
        }
        provenance
    }
}

impl<A: MuxAdapter> SessionSpaceActivationDriver for MuxSessionSpaceActivationDriver<A> {
    fn activate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation> {
        for surface in &request.surfaces {
            if !self.environment.surface_bindings.contains_key(&surface.resource) {
                return Ok(SessionSpaceActivationObservation::Degraded {
                    provider: self.environment.provider.clone(),
                    reason: format!(
                        "Surface {} has no explicit provider-native binding",
                        surface.resource
                    ),
                    provenance: vec![
                        "provider-native ids are not allowed to mint Surface identity".into(),
                    ],
                });
            }
        }
        let observation = self.environment.open()?;
        let provider = observation.provider.clone();
        let provenance = Self::observation_provenance(&observation);
        Ok(match observation.health {
            WorkingEnvironmentHealth::Healthy => SessionSpaceActivationObservation::Active {
                provider,
                provenance,
            },
            WorkingEnvironmentHealth::Degraded => SessionSpaceActivationObservation::Degraded {
                provider,
                reason: "working-environment provider reported degraded health".into(),
                provenance,
            },
            WorkingEnvironmentHealth::Unavailable => {
                SessionSpaceActivationObservation::Unavailable {
                    provider,
                    reason: "working-environment provider is unavailable".into(),
                    provenance,
                }
            }
        })
    }

    fn deactivate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation> {
        if request.surfaces.is_empty() {
            return Ok(SessionSpaceActivationObservation::Degraded {
                provider: self.environment.provider.clone(),
                reason: "component has no explicitly bound Surface to detach".into(),
                provenance: vec!["no provider-native detach target was invented".into()],
            });
        }
        for surface in &request.surfaces {
            if let Err(error) = self.environment.detach_surface(&surface.resource) {
                return Ok(SessionSpaceActivationObservation::Degraded {
                    provider: self.environment.provider.clone(),
                    reason: error.to_string(),
                    provenance: vec![format!(
                        "provider detach failed for canonical Surface {}",
                        surface.resource
                    )],
                });
            }
        }
        Ok(SessionSpaceActivationObservation::Deactivated {
            provider: self.environment.provider.clone(),
            provenance: vec![
                "provider Surface detached; canonical Surface and AgentSession identities retained"
                    .into(),
            ],
        })
    }
}
