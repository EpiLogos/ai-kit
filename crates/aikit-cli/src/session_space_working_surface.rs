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
use aikit_core::working_environment::WorkingEnvironmentObservation;
use aikit_core::{AikitError, Result};
use aikit_tui::live_field::{WorkingEnvironmentOperation, WorkingEnvironmentOutcome};
use serde::Serialize;
use std::collections::BTreeMap;

use crate::working_environment_field;

pub const SESSION_SPACE_WORKING_SURFACE_VERSION: &str = "aikit.session-space-working-surface/v1";

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
    /// The persisted binding updated with provider-native evidence this open
    /// created, when the caller must persist it (Herdr workspaces/panes).
    /// Applying it is the caller's act: this operation never writes state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_binding: Option<SessionSpaceWorkingSurfaceBinding>,
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
    // The plan's declared place technology is an open name, so the reading
    // names it explicitly: a provider that this build cannot drive stays a
    // declared fact of the binding rather than a silent absence.
    let mut provenance = vec![
        "persisted SessionSpace binding selected before provider operation".into(),
        "provider-native ids are re-observed and remain provenance".into(),
    ];
    if let Some(technology) = &binding.plan.mux {
        provenance.push(format!("plan declares place technology `{technology}`"));
    }
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
        provenance,
    })
}

/// Read the exact persisted binding and its current provider observation.
pub fn observe(
    state: &SessionSpaceAuthoredState,
    binding_ref: &ResourceRef,
) -> Result<WorkingSurfaceResult> {
    let binding = binding(state, binding_ref)?;
    Ok(WorkingSurfaceResult {
        reading: read(
            state,
            binding,
            WorkingSurfaceNativeStanding::ReobservedUnproven,
        )?,
        outcome: None,
        refreshed_binding: None,
    })
}

/// Create-or-attach the binding's declared provider plan, then re-observe.
///
/// The result reports only the provider's fresh native fact. It does not claim
/// AgentSession continuity merely because a terminal plan was created again.
/// When the open created provider-native material (a Herdr workspace and its
/// root pane), `refreshed_binding` carries the binding with that evidence
/// recorded; applying it is the caller's separate persisted write.
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
    let reading = read(
        state,
        binding,
        WorkingSurfaceNativeStanding::ReboundByExplicitOpen,
    )?;
    let refreshed_binding = herdr_binding_refresh(binding, &reading, Some(&outcome))?;
    Ok(WorkingSurfaceResult {
        reading,
        outcome: Some(outcome),
        refreshed_binding,
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
    let before = read(
        state,
        binding,
        WorkingSurfaceNativeStanding::ReobservedUnproven,
    )?;
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
        reading: read(
            state,
            binding,
            WorkingSurfaceNativeStanding::ReobservedUnproven,
        )?,
        outcome: Some(outcome),
        refreshed_binding: None,
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

/// Derive the updated persisted binding after an explicit Herdr open.
///
/// Evidence created by this open travels on the outcome; the open's fresh
/// provider observation is the fallback source. The workspace binding and
/// every observed Surface pane id are copied into the plan's
/// `backend_extensions.herdr` table so later observes re-derive the exact
/// provider bindings instead of losing them. Returns `None` for any non-Herdr
/// provider, for opens without usable evidence, and when the evidence equals
/// what the plan already records — a repeated open must not stage a no-op
/// write.
fn herdr_binding_refresh(
    binding: &SessionSpaceWorkingSurfaceBinding,
    reading: &WorkingSurfaceReading,
    outcome: Option<&WorkingEnvironmentOutcome>,
) -> Result<Option<SessionSpaceWorkingSurfaceBinding>> {
    use aikit_adapters::herdr::herdr_provider_ref_uri;
    use aikit_adapters::working_environment::NativeBindingKind;

    if binding.provider.as_str() != herdr_provider_ref_uri() {
        return Ok(None);
    }
    // Created material travels on the outcome itself; a plain observation is
    // the fallback for opens that only ensured already-recorded material.
    let bindings: &[aikit_adapters::working_environment::ProviderNativeBinding] = match outcome {
        Some(WorkingEnvironmentOutcome::Opened {
            created: Some(created),
            ..
        }) => created,
        _ => match &reading.provider_observation {
            Some(observation) => &observation.bindings,
            None => return Ok(None),
        },
    };
    let surfaces = working_environment_field::plan_surfaces(&binding.plan);
    let mut workspace_id = None;
    let mut recorded: BTreeMap<String, String> = BTreeMap::new();
    for native in bindings {
        match (&native.kind, &native.canonical_ref) {
            (NativeBindingKind::Session, None) => {
                workspace_id = Some(native.native_id.clone());
            }
            (NativeBindingKind::Surface, Some(surface)) => {
                if let Some((_, logical)) = surfaces.iter().find(|(known, _)| known == surface) {
                    recorded.insert(logical.clone(), native.native_id.clone());
                }
            }
            _ => {}
        }
    }
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    if recorded.is_empty() {
        return Ok(None);
    }

    let mut plan = binding.plan.clone();
    let mut herdr = toml::map::Map::new();
    herdr.insert("workspace-id".into(), toml::Value::String(workspace_id));
    let mut surfaces = toml::map::Map::new();
    for (logical, pane) in recorded {
        surfaces.insert(logical, toml::Value::String(pane));
    }
    herdr.insert("surfaces".into(), toml::Value::Table(surfaces));
    if plan.backend_extensions.get("herdr") == Some(&herdr) {
        return Ok(None);
    }
    plan.backend_extensions.insert("herdr".into(), herdr);
    let mut updated = binding.clone();
    updated.plan = plan;
    Ok(Some(updated))
}
