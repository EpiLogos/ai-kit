//! Live, provider-neutral SessionSpace runtime for V2.
//!
//! A SessionSpace is the durable semantic workspace identity in which one or more
//! AgentSessions can participate in composable Harness bodies. It is deliberately
//! distinct from Project, Context, AgentSession, HarnessComposition and any mux /
//! desktop / terminal projection.
//!
//! The runtime in this module owns SessionSpace bindings and observed runtime
//! readings only. Canonical Project membership is explicit, canonical Surface
//! identity remains `SurfaceDescriptor`, and admitted `HarnessComposition` bodies
//! remain resolver-owned desired state. A target adapter is required to prove live
//! activation through [`SessionSpaceActivationDriver`] before a `LiveMounted`
//! Component can become `Active`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition::{
    ComponentBinding, CompositionActivationMode, HarnessComposition, SurfaceDescriptor,
};
use crate::resource::ResourceRef;
use crate::{AikitError, Result};

pub const SESSION_SPACE_VERSION: &str = "aikit.session-space/v1";

/// Stable SessionSpace identity. The durable identity can outlive any individual
/// SessionSpaceRuntime, AgentSession, mux projection or target provider process.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionSpaceRef(ResourceRef);

impl SessionSpaceRef {
    pub fn parse(raw: &str) -> Result<Self> {
        if !raw.starts_with("session-space/") {
            return Err(AikitError::new(
                "session_space.invalid_ref",
                format!("SessionSpace ref `{raw}` must begin with `session-space/`"),
            ));
        }
        Ok(Self(ResourceRef::parse(raw)?))
    }

    pub fn as_resource_ref(&self) -> &ResourceRef {
        &self.0
    }
}

impl std::fmt::Display for SessionSpaceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Durable, serializable identity/membership declaration. Runtime provider state
/// is intentionally not persisted here. `projects` is authored SessionSpace
/// membership; observing a Project on an admitted HarnessComposition never mutates
/// it. Rich Project/Context-resolution bindings are an application-layer extension
/// of this explicit relation, not an implicit resolver side effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceDefinition {
    pub version: String,
    pub id: SessionSpaceRef,
    #[serde(default)]
    pub projects: BTreeSet<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl SessionSpaceDefinition {
    pub fn new(id: SessionSpaceRef) -> Self {
        Self {
            version: SESSION_SPACE_VERSION.into(),
            id,
            projects: BTreeSet::new(),
            provenance: vec!["AIKit SessionSpace definition".into()],
        }
    }

    /// Add an explicit authored Project membership relation. This method does not
    /// resolve Context and is never called implicitly by composition admission.
    #[must_use]
    pub fn with_project(mut self, project: ResourceRef) -> Self {
        self.projects.insert(project);
        self
    }

    #[must_use]
    pub fn with_provenance(mut self, provenance: impl Into<String>) -> Self {
        self.provenance.push(provenance.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionSpaceLifecycle {
    Open,
    Closed,
}

/// Runtime activation truth. This is intentionally separate from the declarative
/// `CompositionActivationMode`: `LiveMounted` is eligibility for live activation,
/// while this enum records what actually happened at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionSpaceActivationState {
    Declared,
    Eligible,
    Activating,
    Active,
    Degraded,
    Unavailable,
    Removed,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionSpaceConnectionState {
    Available,
    Connecting,
    Connected,
    Degraded,
    Disconnected,
    Unavailable,
    Closed,
}

/// Explicit authority disclosure carried beside visible/available state. Presence
/// in a SessionSpace does not mutate these values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionSpaceAuthorityState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<ResourceRef>,
    #[serde(default)]
    pub capability_available: bool,
    #[serde(default)]
    pub capability_granted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ResourceRef>,
    #[serde(default)]
    pub action_authorised: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl SessionSpaceAuthorityState {
    pub fn has_authority(&self) -> bool {
        self.capability_granted && self.action.as_ref().is_none_or(|_| self.action_authorised)
    }
}

/// Canonical AgentSession binding into one SessionSpace. `native_session_id` is a
/// provider/harness session identity and never becomes the canonical AgentSession
/// ref by implication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceAgentSession {
    pub agent_session: ResourceRef,
    pub harness: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Mutation capability for one exact AgentSession binding incarnation. Rebinding
/// increments the epoch so stale handles cannot mutate the replacement binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpaceLease {
    pub space: SessionSpaceRef,
    pub agent_session: ResourceRef,
    epoch: u64,
}

/// Observed runtime reading for one canonical Component ref in an AgentSession.
/// This is not a second Component identity or descriptor family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceComponent {
    pub agent_session: ResourceRef,
    pub component: ResourceRef,
    pub harness: ResourceRef,
    pub activation_mode: CompositionActivationMode,
    pub state: SessionSpaceActivationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    /// Exact resolver-owned body against which the latest provider observation was
    /// made. A changed body fingerprint invalidates prior activation evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_composition_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// SessionSpace-local *reading* over the canonical `SurfaceDescriptor`. The
/// descriptor and ResourceRef remain the Surface identity; this wrapper adds only
/// AgentSession attribution and observed runtime state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceSurfaceReading {
    pub agent_session: ResourceRef,
    pub surface: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ResourceRef>,
    pub descriptor: SurfaceDescriptor,
    pub state: SessionSpaceActivationState,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceConnection {
    pub connection: ResourceRef,
    pub provider: ResourceRef,
    pub protocol: String,
    pub agent_session: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<ResourceRef>,
    pub state: SessionSpaceConnectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    #[serde(default)]
    pub authority: SessionSpaceAuthorityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceReadModel {
    pub version: String,
    pub id: SessionSpaceRef,
    pub lifecycle: SessionSpaceLifecycle,
    pub revision: u64,
    #[serde(default)]
    pub projects: Vec<ResourceRef>,
    #[serde(default)]
    pub agent_sessions: Vec<SessionSpaceAgentSession>,
    #[serde(default)]
    pub components: Vec<SessionSpaceComponent>,
    #[serde(default)]
    pub surfaces: Vec<SessionSpaceSurfaceReading>,
    #[serde(default)]
    pub connections: Vec<SessionSpaceConnection>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpaceActivationRequest {
    pub space: SessionSpaceRef,
    pub agent_session: ResourceRef,
    pub harness: ResourceRef,
    pub component: ComponentBinding,
    pub composition_fingerprint: String,
    pub surfaces: Vec<SurfaceDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSpaceActivationObservation {
    Active {
        provider: ResourceRef,
        provenance: Vec<String>,
    },
    /// Provider-confirmed successful live deactivation. The Component remains in
    /// the admitted desired composition and therefore returns to `Eligible`; only
    /// recomposition can make the canonical membership `Removed`.
    Deactivated {
        provider: ResourceRef,
        provenance: Vec<String>,
    },
    Degraded {
        provider: ResourceRef,
        reason: String,
        provenance: Vec<String>,
    },
    Unavailable {
        provider: ResourceRef,
        reason: String,
        provenance: Vec<String>,
    },
}

/// Target-owned live activation seam. The core never synthesizes `Active`; a
/// provider adapter must return an observation after executing its real operation.
pub trait SessionSpaceActivationDriver {
    fn activate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation>;

    fn deactivate(
        &mut self,
        request: &SessionSpaceActivationRequest,
    ) -> Result<SessionSpaceActivationObservation>;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SessionComponentKey {
    agent_session: ResourceRef,
    component: ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SessionSurfaceKey {
    agent_session: ResourceRef,
    surface: ResourceRef,
}

/// One open in-memory runtime materialisation of a durable SessionSpace.
///
/// This is deliberately an owned value rather than a process-global singleton.
/// Different runtimes cannot bleed state unless an explicit caller copies data
/// between them.
pub struct SessionSpaceRuntime {
    definition: SessionSpaceDefinition,
    lifecycle: SessionSpaceLifecycle,
    revision: u64,
    next_epoch: u64,
    agent_sessions: BTreeMap<ResourceRef, (SessionSpaceAgentSession, u64)>,
    /// Resolver-owned desired bodies. SessionSpace never rewrites their fields to
    /// encode target observation; observed state lives in `components`/`surfaces`.
    compositions: BTreeMap<ResourceRef, HarnessComposition>,
    components: BTreeMap<SessionComponentKey, SessionSpaceComponent>,
    surfaces: BTreeMap<SessionSurfaceKey, SessionSpaceSurfaceReading>,
    connections: BTreeMap<ResourceRef, SessionSpaceConnection>,
}

impl SessionSpaceRuntime {
    pub fn open(definition: SessionSpaceDefinition) -> Result<Self> {
        if definition.version != SESSION_SPACE_VERSION {
            return Err(AikitError::new(
                "session_space.unsupported_version",
                format!(
                    "SessionSpace {} uses unsupported version {}",
                    definition.id, definition.version
                ),
            ));
        }
        Ok(Self {
            definition,
            lifecycle: SessionSpaceLifecycle::Open,
            revision: 0,
            next_epoch: 1,
            agent_sessions: BTreeMap::new(),
            compositions: BTreeMap::new(),
            components: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            connections: BTreeMap::new(),
        })
    }

    pub fn definition(&self) -> &SessionSpaceDefinition {
        &self.definition
    }

    pub fn bind_agent_session(
        &mut self,
        binding: SessionSpaceAgentSession,
    ) -> Result<SessionSpaceLease> {
        self.ensure_open()?;
        let epoch = self.next_epoch;
        self.next_epoch += 1;
        let session = binding.agent_session.clone();
        self.agent_sessions.insert(session.clone(), (binding, epoch));
        self.compositions.remove(&session);
        self.components
            .retain(|key, _| key.agent_session != session);
        self.surfaces.retain(|key, _| key.agent_session != session);
        self.connections
            .retain(|_, connection| connection.agent_session != session);
        self.bump_revision();
        Ok(SessionSpaceLease {
            space: self.definition.id.clone(),
            agent_session: session,
            epoch,
        })
    }

    pub fn unbind_agent_session(&mut self, lease: &SessionSpaceLease) -> Result<()> {
        self.ensure_lease(lease)?;
        self.agent_sessions.remove(&lease.agent_session);
        self.compositions.remove(&lease.agent_session);
        self.components
            .retain(|key, _| key.agent_session != lease.agent_session);
        self.surfaces
            .retain(|key, _| key.agent_session != lease.agent_session);
        self.connections
            .retain(|_, connection| connection.agent_session != lease.agent_session);
        self.bump_revision();
        Ok(())
    }

    pub fn admit_composition(
        &mut self,
        lease: &SessionSpaceLease,
        composition: HarnessComposition,
    ) -> Result<()> {
        let binding = self.ensure_lease(lease)?.0.clone();
        if composition.harness != binding.harness {
            return Err(AikitError::new(
                "session_space.harness_mismatch",
                format!(
                    "composition harness {} does not match AgentSession {} harness {}",
                    composition.harness, binding.agent_session, binding.harness
                ),
            ));
        }
        if let (Some(expected), Some(observed)) = (
            binding.native_session_id.as_deref(),
            composition.session.as_deref(),
        ) {
            if expected != observed {
                return Err(AikitError::new(
                    "session_space.native_session_mismatch",
                    format!(
                        "composition native session {observed} does not match bound native session {expected}"
                    ),
                ));
            }
        }

        // Project membership is explicit authored SessionSpace state. A resolved
        // Harness body may carry Project provenance without silently binding that
        // Project into this space.
        let session = lease.agent_session.clone();
        let previous_fingerprint = self
            .compositions
            .get(&session)
            .map(|current| current.fingerprint.as_str());
        let same_body = previous_fingerprint == Some(composition.fingerprint.as_str());
        let new_components: BTreeSet<_> = composition
            .component_bindings
            .iter()
            .map(|component| component.component.clone())
            .collect();
        let new_surfaces: BTreeSet<_> = composition
            .surfaces
            .iter()
            .map(|surface| surface.resource.clone())
            .collect();

        for (key, component) in &mut self.components {
            if key.agent_session == session && !new_components.contains(&key.component) {
                component.state = SessionSpaceActivationState::Removed;
                component.provider = None;
                component.observed_composition_fingerprint = None;
                component.reason = Some("removed by canonical HarnessComposition recomposition".into());
            }
        }
        // Surface membership follows the canonical resolved body exactly. Runtime
        // deactivation never deletes a Surface reading; recomposition may.
        self.surfaces.retain(|key, _| {
            key.agent_session != session || new_surfaces.contains(&key.surface)
        });

        for component in &composition.component_bindings {
            let key = SessionComponentKey {
                agent_session: session.clone(),
                component: component.component.clone(),
            };
            let blocked: Vec<_> = composition
                .absences
                .iter()
                .filter(|absence| absence.component == component.component && absence.required)
                .map(|absence| absence.reason.clone())
                .collect();
            let prior = self.components.get(&key).cloned();
            let preserve_observation = same_body
                && prior.as_ref().is_some_and(|existing| {
                    existing.observed_composition_fingerprint.as_deref()
                        == Some(composition.fingerprint.as_str())
                });
            let state = if !blocked.is_empty() {
                SessionSpaceActivationState::Unavailable
            } else if preserve_observation {
                prior
                    .as_ref()
                    .map_or(SessionSpaceActivationState::Eligible, |existing| existing.state)
            } else {
                // A changed canonical body invalidates provider activation evidence
                // until the provider observes that exact new fingerprint.
                SessionSpaceActivationState::Eligible
            };
            let mut provenance = component_provenance(&composition, component);
            if preserve_observation {
                if let Some(existing) = &prior {
                    provenance.extend(existing.provenance.iter().cloned());
                    provenance.sort();
                    provenance.dedup();
                }
            }
            self.components.insert(
                key,
                SessionSpaceComponent {
                    agent_session: session.clone(),
                    component: component.component.clone(),
                    harness: composition.harness.clone(),
                    activation_mode: component.activation_mode,
                    state,
                    provider: preserve_observation
                        .then(|| prior.as_ref().and_then(|existing| existing.provider.clone()))
                        .flatten(),
                    observed_composition_fingerprint: preserve_observation
                        .then(|| {
                            prior.as_ref().and_then(|existing| {
                                existing.observed_composition_fingerprint.clone()
                            })
                        })
                        .flatten(),
                    reason: if !blocked.is_empty() {
                        Some(blocked.join("; "))
                    } else if preserve_observation {
                        prior.as_ref().and_then(|existing| existing.reason.clone())
                    } else {
                        None
                    },
                    provenance,
                },
            );
        }

        for surface in &composition.surfaces {
            let key = SessionSurfaceKey {
                agent_session: session.clone(),
                surface: surface.resource.clone(),
            };
            let owner_state = surface
                .owner_component
                .as_ref()
                .and_then(|component| {
                    self.components.get(&SessionComponentKey {
                        agent_session: session.clone(),
                        component: component.clone(),
                    })
                })
                .map_or(SessionSpaceActivationState::Declared, |component| component.state);
            self.surfaces.insert(
                key,
                SessionSpaceSurfaceReading {
                    agent_session: session.clone(),
                    surface: surface.resource.clone(),
                    component: surface.owner_component.clone(),
                    descriptor: surface.clone(),
                    state: owner_state,
                    provenance: surface_provenance(&composition, surface),
                },
            );
        }

        self.compositions.insert(session, composition);
        self.refresh_surface_states(&lease.agent_session);
        self.bump_revision();
        Ok(())
    }

    pub fn activate_component<D: SessionSpaceActivationDriver>(
        &mut self,
        lease: &SessionSpaceLease,
        component: &ResourceRef,
        driver: &mut D,
    ) -> Result<SessionSpaceActivationState> {
        let binding = self.ensure_lease(lease)?.0.clone();
        let key = SessionComponentKey {
            agent_session: lease.agent_session.clone(),
            component: component.clone(),
        };
        let current = self.components.get(&key).cloned().ok_or_else(|| {
            AikitError::new(
                "session_space.component_absent",
                format!("component {component} is not admitted to {}", self.definition.id),
            )
        })?;
        if current.activation_mode != CompositionActivationMode::LiveMounted {
            return Err(AikitError::new(
                "session_space.activation_not_live",
                format!(
                    "component {component} is {:?}, not live-mounted; SessionSpace will not counterfeit ACTIVE",
                    current.activation_mode
                ),
            ));
        }
        if matches!(
            current.state,
            SessionSpaceActivationState::Unavailable
                | SessionSpaceActivationState::Removed
                | SessionSpaceActivationState::Closed
        ) {
            return Err(AikitError::new(
                "session_space.component_not_eligible",
                format!("component {component} is {:?}", current.state),
            ));
        }
        let composition = self.compositions.get(&lease.agent_session).ok_or_else(|| {
            AikitError::new(
                "session_space.composition_absent",
                format!("AgentSession {} has no admitted composition", lease.agent_session),
            )
        })?;
        let component_binding = composition
            .component_bindings
            .iter()
            .find(|candidate| candidate.component == *component)
            .cloned()
            .ok_or_else(|| {
                AikitError::new(
                    "session_space.component_binding_absent",
                    format!("component {component} has no binding in the admitted composition"),
                )
            })?;
        let surfaces = composition
            .surfaces
            .iter()
            .filter(|surface| surface.owner_component.as_ref() == Some(component))
            .cloned()
            .collect();
        let request = SessionSpaceActivationRequest {
            space: self.definition.id.clone(),
            agent_session: lease.agent_session.clone(),
            harness: binding.harness,
            component: component_binding,
            composition_fingerprint: composition.fingerprint.clone(),
            surfaces,
        };

        if let Some(runtime_component) = self.components.get_mut(&key) {
            runtime_component.state = SessionSpaceActivationState::Activating;
            runtime_component.reason = None;
        }
        self.refresh_surface_states(&lease.agent_session);
        self.bump_revision();

        let observation = match driver.activate(&request) {
            Ok(observation) => observation,
            Err(error) => {
                if let Some(runtime_component) = self.components.get_mut(&key) {
                    runtime_component.state = SessionSpaceActivationState::Degraded;
                    runtime_component.reason = Some(error.to_string());
                }
                self.refresh_surface_states(&lease.agent_session);
                self.bump_revision();
                return Err(error);
            }
        };
        let state = self.apply_activation_observation(
            &key,
            &request.composition_fingerprint,
            observation,
        );
        self.refresh_surface_states(&lease.agent_session);
        self.bump_revision();
        Ok(state)
    }

    pub fn activate_all<D: SessionSpaceActivationDriver>(
        &mut self,
        lease: &SessionSpaceLease,
        driver: &mut D,
    ) -> Result<Vec<(ResourceRef, SessionSpaceActivationState)>> {
        self.ensure_lease(lease)?;
        let components: Vec<_> = self
            .components
            .iter()
            .filter(|(key, component)| {
                key.agent_session == lease.agent_session
                    && component.state != SessionSpaceActivationState::Removed
            })
            .map(|(_, component)| component.component.clone())
            .collect();
        let mut states = Vec::with_capacity(components.len());
        for component in components {
            let state = self.activate_component(lease, &component, driver)?;
            states.push((component, state));
        }
        Ok(states)
    }

    pub fn deactivate_component<D: SessionSpaceActivationDriver>(
        &mut self,
        lease: &SessionSpaceLease,
        component: &ResourceRef,
        driver: &mut D,
    ) -> Result<SessionSpaceActivationState> {
        let binding = self.ensure_lease(lease)?.0.clone();
        let key = SessionComponentKey {
            agent_session: lease.agent_session.clone(),
            component: component.clone(),
        };
        let composition = self.compositions.get(&lease.agent_session).ok_or_else(|| {
            AikitError::new(
                "session_space.composition_absent",
                format!("AgentSession {} has no admitted composition", lease.agent_session),
            )
        })?;
        let component_binding = composition
            .component_bindings
            .iter()
            .find(|candidate| candidate.component == *component)
            .cloned()
            .ok_or_else(|| {
                AikitError::new(
                    "session_space.component_binding_absent",
                    format!("component {component} has no binding in the admitted composition"),
                )
            })?;
        let request = SessionSpaceActivationRequest {
            space: self.definition.id.clone(),
            agent_session: lease.agent_session.clone(),
            harness: binding.harness,
            component: component_binding,
            composition_fingerprint: composition.fingerprint.clone(),
            surfaces: composition
                .surfaces
                .iter()
                .filter(|surface| surface.owner_component.as_ref() == Some(component))
                .cloned()
                .collect(),
        };
        let observation = driver.deactivate(&request)?;
        let state = self.apply_activation_observation(
            &key,
            &request.composition_fingerprint,
            observation,
        );
        // Deactivation changes observed live truth only. The canonical desired
        // composition still contains this Component and its Surfaces; only an
        // admitted recomposition may remove those membership relations.
        self.refresh_surface_states(&lease.agent_session);
        self.bump_revision();
        Ok(state)
    }

    pub fn observe_connection(
        &mut self,
        lease: &SessionSpaceLease,
        connection: SessionSpaceConnection,
    ) -> Result<()> {
        self.ensure_lease(lease)?;
        if connection.agent_session != lease.agent_session {
            return Err(AikitError::new(
                "session_space.connection_session_mismatch",
                format!(
                    "connection {} belongs to {}, not lease AgentSession {}",
                    connection.connection, connection.agent_session, lease.agent_session
                ),
            ));
        }
        if let Some(component) = &connection.component {
            let key = SessionComponentKey {
                agent_session: lease.agent_session.clone(),
                component: component.clone(),
            };
            if !self.components.contains_key(&key) {
                return Err(AikitError::new(
                    "session_space.connection_component_absent",
                    format!("connection {} references unadmitted component {component}", connection.connection),
                ));
            }
        }
        self.connections
            .insert(connection.connection.clone(), connection);
        self.bump_revision();
        Ok(())
    }

    pub fn detach_connection(
        &mut self,
        lease: &SessionSpaceLease,
        connection: &ResourceRef,
    ) -> Result<()> {
        self.ensure_lease(lease)?;
        let Some(existing) = self.connections.get(connection) else {
            return Ok(());
        };
        if existing.agent_session != lease.agent_session {
            return Err(AikitError::new(
                "session_space.connection_session_mismatch",
                format!("connection {connection} is owned by another AgentSession"),
            ));
        }
        self.connections.remove(connection);
        self.bump_revision();
        Ok(())
    }

    /// Provider health is an observed external fact, not an AgentSession mutation.
    /// It can degrade active runtime state but never grants capability/Action authority.
    pub fn observe_provider_unavailable(
        &mut self,
        provider: &ResourceRef,
        reason: impl Into<String>,
    ) -> Result<()> {
        self.ensure_open()?;
        let reason = reason.into();
        for connection in self.connections.values_mut() {
            if &connection.provider == provider {
                connection.state = SessionSpaceConnectionState::Unavailable;
                connection.reason = Some(reason.clone());
            }
        }
        let sessions: BTreeSet<_> = self
            .components
            .iter_mut()
            .filter_map(|(key, component)| {
                if component.provider.as_ref() == Some(provider)
                    && matches!(
                        component.state,
                        SessionSpaceActivationState::Active
                            | SessionSpaceActivationState::Activating
                    )
                {
                    component.state = SessionSpaceActivationState::Degraded;
                    component.reason = Some(reason.clone());
                    Some(key.agent_session.clone())
                } else {
                    None
                }
            })
            .collect();
        for session in sessions {
            self.refresh_surface_states(&session);
        }
        self.bump_revision();
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        self.ensure_open()?;
        self.lifecycle = SessionSpaceLifecycle::Closed;
        for component in self.components.values_mut() {
            component.state = SessionSpaceActivationState::Closed;
            component.reason = Some("SessionSpace closed".into());
        }
        self.surfaces.clear();
        self.connections.clear();
        self.agent_sessions.clear();
        self.compositions.clear();
        self.bump_revision();
        Ok(())
    }

    pub fn read_model(&self) -> SessionSpaceReadModel {
        SessionSpaceReadModel {
            version: SESSION_SPACE_VERSION.into(),
            id: self.definition.id.clone(),
            lifecycle: self.lifecycle,
            revision: self.revision,
            projects: self.definition.projects.iter().cloned().collect(),
            agent_sessions: self
                .agent_sessions
                .values()
                .map(|(binding, _)| binding.clone())
                .collect(),
            components: self.components.values().cloned().collect(),
            surfaces: self.surfaces.values().cloned().collect(),
            connections: self.connections.values().cloned().collect(),
            provenance: self.definition.provenance.clone(),
        }
    }

    fn ensure_open(&self) -> Result<()> {
        if self.lifecycle == SessionSpaceLifecycle::Closed {
            return Err(AikitError::new(
                "session_space.closed",
                format!("SessionSpace {} is closed", self.definition.id),
            ));
        }
        Ok(())
    }

    fn ensure_lease(
        &self,
        lease: &SessionSpaceLease,
    ) -> Result<&(SessionSpaceAgentSession, u64)> {
        self.ensure_open()?;
        if lease.space != self.definition.id {
            return Err(AikitError::new(
                "session_space.lease_space_mismatch",
                format!("lease belongs to {}, not {}", lease.space, self.definition.id),
            ));
        }
        let binding = self.agent_sessions.get(&lease.agent_session).ok_or_else(|| {
            AikitError::new(
                "session_space.agent_session_unbound",
                format!("AgentSession {} is not bound to {}", lease.agent_session, self.definition.id),
            )
        })?;
        if binding.1 != lease.epoch {
            return Err(AikitError::new(
                "session_space.stale_lease",
                format!("AgentSession {} was rebound; lease is stale", lease.agent_session),
            ));
        }
        Ok(binding)
    }

    fn apply_activation_observation(
        &mut self,
        key: &SessionComponentKey,
        composition_fingerprint: &str,
        observation: SessionSpaceActivationObservation,
    ) -> SessionSpaceActivationState {
        let component = self
            .components
            .get_mut(key)
            .expect("activation target is validated before provider operation");
        match observation {
            SessionSpaceActivationObservation::Active {
                provider,
                provenance,
            } => {
                component.state = SessionSpaceActivationState::Active;
                component.provider = Some(provider);
                component.observed_composition_fingerprint =
                    Some(composition_fingerprint.to_string());
                component.reason = None;
                component.provenance.extend(provenance);
            }
            SessionSpaceActivationObservation::Deactivated {
                provider,
                provenance,
            } => {
                component.state = SessionSpaceActivationState::Eligible;
                component.provider = Some(provider);
                component.observed_composition_fingerprint =
                    Some(composition_fingerprint.to_string());
                component.reason = None;
                component.provenance.extend(provenance);
            }
            SessionSpaceActivationObservation::Degraded {
                provider,
                reason,
                provenance,
            } => {
                component.state = SessionSpaceActivationState::Degraded;
                component.provider = Some(provider);
                component.observed_composition_fingerprint =
                    Some(composition_fingerprint.to_string());
                component.reason = Some(reason);
                component.provenance.extend(provenance);
            }
            SessionSpaceActivationObservation::Unavailable {
                provider,
                reason,
                provenance,
            } => {
                component.state = SessionSpaceActivationState::Unavailable;
                component.provider = Some(provider);
                component.observed_composition_fingerprint =
                    Some(composition_fingerprint.to_string());
                component.reason = Some(reason);
                component.provenance.extend(provenance);
            }
        }
        component.provenance.sort();
        component.provenance.dedup();
        component.state
    }

    fn refresh_surface_states(&mut self, agent_session: &ResourceRef) {
        for (key, surface) in &mut self.surfaces {
            if &key.agent_session != agent_session {
                continue;
            }
            surface.state = surface
                .component
                .as_ref()
                .and_then(|component| {
                    self.components.get(&SessionComponentKey {
                        agent_session: agent_session.clone(),
                        component: component.clone(),
                    })
                })
                .map_or(SessionSpaceActivationState::Declared, |component| component.state);
        }
    }

    fn bump_revision(&mut self) {
        self.revision += 1;
    }
}

fn component_provenance(
    composition: &HarnessComposition,
    component: &ComponentBinding,
) -> Vec<String> {
    let mut provenance = BTreeSet::new();
    if let Some(implementation) = &component.implementation {
        let revision = implementation
            .revision
            .as_deref()
            .map_or_else(String::new, |revision| format!("@{revision}"));
        provenance.insert(format!(
            "{}{}:{}",
            implementation.implementation_target, revision, implementation.native_id
        ));
    }
    for contribution in &composition.contributions {
        if contribution.component == component.component {
            provenance.extend(contribution.provenance.iter().cloned());
        }
    }
    if provenance.is_empty() {
        provenance.insert(format!("HarnessComposition {}", composition.fingerprint));
    }
    provenance.into_iter().collect()
}

fn surface_provenance(
    composition: &HarnessComposition,
    surface: &SurfaceDescriptor,
) -> Vec<String> {
    let mut provenance = BTreeSet::new();
    for contribution in &composition.contributions {
        if contribution.surface.as_ref() == Some(&surface.resource) {
            provenance.extend(contribution.provenance.iter().cloned());
        }
    }
    if provenance.is_empty() {
        provenance.insert(format!("HarnessComposition {}", composition.fingerprint));
    }
    provenance.into_iter().collect()
}
