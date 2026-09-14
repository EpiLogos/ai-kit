//! Owner-side operations for one durable, exact SessionSpace working Surface.
//!
//! This module consumes the persisted binding selected through SessionSpace
//! authority, then delegates native I/O to the established working-environment
//! provider field. It never builds mux argv or promotes a provider-native pane
//! id into a canonical Surface identity.

use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAuthoredState, SessionSpaceWorkingSurfaceBinding,
};
use aikit_core::{AikitError, Result};
use aikit_core::working_environment::WorkingEnvironmentObservation;
use aikit_tui::live_field::{WorkingEnvironmentOperation, WorkingEnvironmentOutcome};
use serde::Serialize;

use crate::working_environment_field;

pub const SESSION_SPACE_WORKING_SURFACE_VERSION: &str =
    "aikit.session-space-working-surface/v1";

#[derive(Debug, Clone, Copy)]
pub enum WorkingSurfaceOperation {
    Observe,
    Open,
    Focus,
}

/// Provider-native survival is never inferred from a matching plan or pane
/// name. A focus observes a presently live binding; an explicit open may have
/// recreated the provider material and is returned as such.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkingSurfaceNativeStanding {
    ReobservedUnproven,
    ReboundByExplicitOpen,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkingSurfaceReading {
    pub schema: String,
    pub space: SessionSpaceRef,
    pub binding: ResourceRef,
    pub surface: ResourceRef,
    pub agent_session: ResourceRef,
    pub provider: ResourceRef,
    pub plan_id: String,
    pub plan_name: String,
    pub plan_key: String,
    pub provider_observation: Option<WorkingEnvironmentObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_native_id: Option<String>,
    pub native_standing: WorkingSurfaceNativeStanding,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkingSurfaceResult {
    pub reading: WorkingSurfaceReading,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<WorkingEnvironmentOutcome>,
}

fn binding<'a>(
    state: &'a SessionSpaceAuthoredState,
    binding: &ResourceRef,
) -> Result<&'a SessionSpaceWorkingSurfaceBinding> {
    state.working_surfaces.get(binding).ok_or_else(|| {
        AikitError::new(
            "session_space.working_surface_unbound",
            format!(
                "SessionSpace {} has no persisted working-environment binding {binding}",
                state.id()
            ),
        )
    })
}

fn read(
    state: &SessionSpaceAuthoredState,
    binding: &SessionSpaceWorkingSurfaceBinding,
    native_standing: WorkingSurfaceNativeStanding,
) -> Result<WorkingSurfaceReading> {
    let provider_observation = working_environment_field::observe(&binding.plan)?
        .into_iter()
        .find(|observation| observation.provider == binding.provider);
    let live_native_id = provider_observation
        .as_ref()
        .and_then(|observation| observation.canonical_native_id(&binding.surface))
        .map(ToString::to_string);
    Ok(WorkingSurfaceReading {
        schema: SESSION_SPACE_WORKING_SURFACE_VERSION.into(),
        space: state.id().clone(),
        binding: binding.binding.clone(),
        surface: binding.surface.clone(),
        agent_session: binding.agent_session.clone(),
        provider: binding.provider.clone(),
        plan_id: binding.plan.id.clone(),
        plan_name: binding.plan.name.clone(),
        plan_key: binding.plan_key.clone(),
        provider_observation,
        live_native_id,
        native_standing,
        provenance: vec![
            "persisted SessionSpace binding selected before provider operation".into(),
            "provider-native ids are re-observed and remain provenance".into(),
        ],
    })
}

/// Read the exact persisted binding and its current provider observation.
pub fn observe(
    state: &SessionSpaceAuthoredState,
    binding_ref: &ResourceRef,
) -> Result<WorkingSurfaceResult> {
    let binding = binding(state, binding_ref)?;
    Ok(WorkingSurfaceResult {
        reading: read(state, binding, WorkingSurfaceNativeStanding::ReobservedUnproven)?,
        outcome: None,
    })
}

/// Create-or-attach the binding's declared provider plan, then re-observe.
///
/// The result reports only the provider's fresh native fact. It does not claim
/// AgentSession continuity merely because a terminal plan was created again.
pub fn open(
    state: &SessionSpaceAuthoredState,
    binding_ref: &ResourceRef,
) -> Result<WorkingSurfaceResult> {
    let binding = binding(state, binding_ref)?;
    let outcome = working_environment_field::act(
        &binding.plan,
        &binding.provider,
        &binding.surface,
        WorkingEnvironmentOperation::Open,
    )?;
    Ok(WorkingSurfaceResult {
        reading: read(
            state,
            binding,
            WorkingSurfaceNativeStanding::ReboundByExplicitOpen,
        )?,
        outcome: Some(outcome),
    })
}

/// Focus only a currently observed bound Surface.
///
/// A missing live pane is a withheld operation rather than an implicit plan
/// recreation. Callers that want creation must use [`open`] explicitly.
pub fn focus(
    state: &SessionSpaceAuthoredState,
    binding_ref: &ResourceRef,
) -> Result<WorkingSurfaceResult> {
    let binding = binding(state, binding_ref)?;
    let before = read(state, binding, WorkingSurfaceNativeStanding::ReobservedUnproven)?;
    let outcome = if before.live_native_id.is_some() {
        working_environment_field::act(
            &binding.plan,
            &binding.provider,
            &binding.surface,
            WorkingEnvironmentOperation::Focus,
        )?
    } else {
        WorkingEnvironmentOutcome::NotExposed {
            provider: binding.provider.clone(),
            subject: binding.surface.clone(),
            reason: "the persisted working Surface is not currently live; open it explicitly before focus".into(),
        }
    };
    Ok(WorkingSurfaceResult {
        reading: read(state, binding, WorkingSurfaceNativeStanding::ReobservedUnproven)?,
        outcome: Some(outcome),
    })
}

/// Resolve a terminal-client attachment for this exact persisted binding.
///
/// Unlike [`open`], this operation never creates or reconciles provider work.
/// It succeeds only when the provider can freshly prove the planned pane is
/// live, so a recycled tmux name cannot stand in for the previous material.
pub fn terminal_attachment(
    state: &SessionSpaceAuthoredState,
    binding_ref: &ResourceRef,
) -> Result<crate::working_environment_field::WorkingEnvironmentTerminalAttachment> {
    let binding = binding(state, binding_ref)?;
    crate::working_environment_field::terminal_attachment(
        &binding.plan,
        &binding.provider,
        &binding.surface,
    )
}
