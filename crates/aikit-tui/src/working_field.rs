//! Terminal-native projection of the composed O:I working field.
//!
//! This is a read-model/presentation contract over product-owned Refs, Actions,
//! owners, provenance and permission meaning. It owns no Factory, Central,
//! Actuation, package or SessionSpace state. Selection is handed back to the one
//! existing `TuiState` reducer by canonical `ResourceRef`.

use std::collections::BTreeSet;

use aikit_core::resource::ResourceRef;
use aikit_core::{
    AikitError, Result, SessionSpaceActivationState, SessionSpaceConnectionState,
    SessionSpaceLifecycle, SessionSpaceReadModel, SurfaceKind,
};
use serde::{Deserialize, Serialize};

use crate::application::{reduce_tui, TuiReduction, TuiState, UiAction};

pub const TERMINAL_WORKING_FIELD_VERSION: &str = "aikit.tui-working-field/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalContributionKind {
    Reading,
    Relation,
    Inspector,
    ActionBinding,
    Trajectory,
    Form,
    CommandNavigation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceProjection {
    pub surface: ResourceRef,
    pub terminal_representation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionProjection {
    /// Product/policy owner which gives this permission statement meaning.
    pub authority_owner: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_ref: Option<ResourceRef>,
    /// Human-readable meaning supplied by the native contract. The TUI displays
    /// it; it does not reinterpret it into a TUI-local permission model.
    pub meaning: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum WorkingFieldAvailability {
    Available,
    Degraded { reason: String },
    ContractFixture { live_gate: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingFieldItem {
    pub subject: ResourceRef,
    /// Native/public-contract kind label. This is deliberately not a new
    /// ResourceKind taxonomy and can carry product/read-model types without
    /// turning presentation vocabulary into ontology.
    pub semantic_kind: String,
    pub owner: ResourceRef,
    #[serde(default)]
    pub actions: Vec<ResourceRef>,
    #[serde(default)]
    pub surfaces: Vec<SurfaceProjection>,
    #[serde(default)]
    pub contribution_kinds: BTreeSet<TerminalContributionKind>,
    pub permission: PermissionProjection,
    #[serde(default)]
    pub provenance: Vec<String>,
    pub availability: WorkingFieldAvailability,
}

impl WorkingFieldItem {
    pub fn terminal_surfaces(&self) -> impl Iterator<Item = &ResourceRef> {
        self.surfaces
            .iter()
            .filter(|surface| surface.terminal_representation)
            .map(|surface| &surface.surface)
    }

    pub fn alternate_surfaces(&self) -> impl Iterator<Item = &SurfaceProjection> {
        self.surfaces
            .iter()
            .filter(|surface| !surface.terminal_representation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalWorkingField {
    pub version: String,
    pub revision: String,
    /// Current live enclosing SessionSpace when this field is projected from the
    /// SessionSpace read model. Other provider-owned field fixtures may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing_world: Option<ResourceRef>,
    pub items: Vec<WorkingFieldItem>,
}

impl TerminalWorkingField {
    pub fn new(revision: impl Into<String>, items: Vec<WorkingFieldItem>) -> Result<Self> {
        let field = Self {
            version: TERMINAL_WORKING_FIELD_VERSION.into(),
            revision: revision.into(),
            enclosing_world: None,
            items,
        };
        field.validate()?;
        Ok(field)
    }

    pub fn with_enclosing_world(mut self, enclosing_world: ResourceRef) -> Self {
        self.enclosing_world = Some(enclosing_world);
        self
    }

    pub fn item(&self, subject: &ResourceRef) -> Option<&WorkingFieldItem> {
        self.items.iter().find(|item| &item.subject == subject)
    }

    pub fn items_owned_by<'a>(
        &'a self,
        owner: &'a ResourceRef,
    ) -> impl Iterator<Item = &'a WorkingFieldItem> + 'a {
        self.items.iter().filter(move |item| &item.owner == owner)
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = BTreeSet::new();
        for item in &self.items {
            if !seen.insert(item.subject.clone()) {
                return Err(AikitError::new(
                    "tui.working_field.duplicate_subject",
                    format!("working field contains {} more than once", item.subject),
                ));
            }
            if item.semantic_kind.trim().is_empty() {
                return Err(AikitError::new(
                    "tui.working_field.missing_semantic_kind",
                    format!("{} has no native semantic kind", item.subject),
                ));
            }
            if item.provenance.is_empty() {
                return Err(AikitError::new(
                    "tui.working_field.missing_provenance",
                    format!("{} has no source/provenance", item.subject),
                ));
            }
            for surface in &item.surfaces {
                if !surface.terminal_representation && surface.alternate_reason.is_none() {
                    return Err(AikitError::new(
                        "tui.working_field.alternate_surface_reason_required",
                        format!(
                            "alternate Surface {} for {} must explain why terminal parity is unavailable",
                            surface.surface, item.subject
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Project the same UI-neutral SessionSpace runtime that desktop/other consumers
/// can read into the existing TUI field. No SessionSpace semantics are rebuilt in
/// the terminal layer.
pub fn working_field_from_session_space(
    model: &SessionSpaceReadModel,
) -> Result<TerminalWorkingField> {
    let aikit = r("ai-kit")?;
    let tui_surface = r("surface/aikit/tui")?;
    let mut items = Vec::new();

    items.push(WorkingFieldItem {
        subject: model.id.as_resource_ref().clone(),
        semantic_kind: "SessionSpace".into(),
        owner: aikit.clone(),
        actions: vec![],
        surfaces: vec![SurfaceProjection {
            surface: tui_surface,
            terminal_representation: true,
            alternate_reason: None,
        }],
        contribution_kinds: BTreeSet::from([
            TerminalContributionKind::Reading,
            TerminalContributionKind::Relation,
            TerminalContributionKind::CommandNavigation,
        ]),
        permission: PermissionProjection {
            authority_owner: aikit.clone(),
            policy_ref: None,
            meaning: "SessionSpace composition transfers reference/possibility, never ambient trust or Action authority".into(),
        },
        provenance: model.provenance.clone(),
        availability: match model.lifecycle {
            SessionSpaceLifecycle::Open => WorkingFieldAvailability::Available,
            SessionSpaceLifecycle::Closed => WorkingFieldAvailability::Unavailable {
                reason: "SessionSpace is closed".into(),
            },
        },
    });

    for session in &model.agent_sessions {
        items.push(WorkingFieldItem {
            subject: session.agent_session.clone(),
            semantic_kind: "AgentSession".into(),
            owner: aikit.clone(),
            actions: vec![],
            surfaces: vec![],
            contribution_kinds: BTreeSet::from([
                TerminalContributionKind::Reading,
                TerminalContributionKind::Relation,
            ]),
            permission: PermissionProjection {
                authority_owner: aikit.clone(),
                policy_ref: None,
                meaning: "AgentSession binding supplies execution continuity only; it is not SessionSpace or ambient authority".into(),
            },
            provenance: session.provenance.clone(),
            availability: WorkingFieldAvailability::Available,
        });
    }

    for component in &model.components {
        let owner = component.provider.clone().unwrap_or_else(|| aikit.clone());
        let surfaces = model
            .surfaces
            .iter()
            .filter(|surface| {
                surface.agent_session == component.agent_session
                    && surface.component.as_ref() == Some(&component.component)
            })
            .map(|surface| SurfaceProjection {
                surface: surface.surface.clone(),
                terminal_representation: matches!(
                    surface.descriptor.kind,
                    SurfaceKind::Tui | SurfaceKind::Cli
                ),
                alternate_reason: (!matches!(
                    surface.descriptor.kind,
                    SurfaceKind::Tui | SurfaceKind::Cli
                ))
                .then(|| {
                    format!(
                        "native {:?} Surface remains a peer projection; terminal shows its runtime state without cloning it",
                        surface.descriptor.kind
                    )
                }),
            })
            .collect();
        items.push(WorkingFieldItem {
            subject: component.component.clone(),
            semantic_kind: "Component".into(),
            owner,
            actions: vec![],
            surfaces,
            contribution_kinds: BTreeSet::from([
                TerminalContributionKind::Reading,
                TerminalContributionKind::Relation,
                TerminalContributionKind::Inspector,
            ]),
            permission: PermissionProjection {
                authority_owner: aikit.clone(),
                policy_ref: None,
                meaning: "Component/Surface presence and live activation do not grant Capability or Action authority".into(),
            },
            provenance: component.provenance.clone(),
            availability: component_availability(component.state, component.reason.as_deref()),
        });
    }

    for connection in &model.connections {
        let authority = &connection.authority;
        let meaning = match (
            authority.capability_available,
            authority.capability_granted,
            authority.action.as_ref(),
            authority.action_authorised,
        ) {
            (true, false, _, _) => "connection available; capability is visible but not granted",
            (true, true, Some(_), false) => "capability granted; Action remains unauthorised",
            (true, true, Some(_), true) => {
                "capability granted and named Action authorised by its owning policy"
            }
            (true, true, None, _) => "capability granted; no Action authorisation is implied",
            _ => "connection presence supplies no capability or Action authority",
        };
        items.push(WorkingFieldItem {
            subject: connection.connection.clone(),
            semantic_kind: "AgentConnection".into(),
            owner: connection.provider.clone(),
            actions: vec![],
            surfaces: connection
                .surface
                .iter()
                .cloned()
                .map(|surface| SurfaceProjection {
                    surface,
                    terminal_representation: true,
                    alternate_reason: None,
                })
                .collect(),
            contribution_kinds: BTreeSet::from([
                TerminalContributionKind::Reading,
                TerminalContributionKind::Relation,
            ]),
            permission: PermissionProjection {
                authority_owner: aikit.clone(),
                policy_ref: None,
                meaning: meaning.into(),
            },
            provenance: connection.provenance.clone(),
            availability: connection_availability(connection.state, connection.reason.as_deref()),
        });
    }

    TerminalWorkingField::new(format!("session-space/{}", model.revision), items)
        .map(|field| field.with_enclosing_world(model.id.as_resource_ref().clone()))
}

/// Route working-field selection through the one existing semantic reducer. The
/// field is a projection; it does not gain a second cursor/controller/store.
pub fn select_working_field_subject(
    state: TuiState,
    field: &TerminalWorkingField,
    subject: ResourceRef,
) -> Result<TuiReduction> {
    if field.item(&subject).is_none() {
        return Err(AikitError::new(
            "tui.working_field.subject_absent",
            format!(
                "{subject} is not in working-field revision {}",
                field.revision
            ),
        ));
    }
    Ok(reduce_tui(state, UiAction::Select(subject)))
}

fn component_availability(
    state: SessionSpaceActivationState,
    reason: Option<&str>,
) -> WorkingFieldAvailability {
    match state {
        SessionSpaceActivationState::Active => WorkingFieldAvailability::Available,
        SessionSpaceActivationState::Activating
        | SessionSpaceActivationState::Degraded
        | SessionSpaceActivationState::Eligible => WorkingFieldAvailability::Degraded {
            reason: reason
                .unwrap_or(match state {
                    SessionSpaceActivationState::Eligible => "eligible but not live-active",
                    SessionSpaceActivationState::Activating => "live activation in progress",
                    _ => "live provider/component degraded",
                })
                .into(),
        },
        SessionSpaceActivationState::Declared => WorkingFieldAvailability::Degraded {
            reason: "declared but not eligible/active".into(),
        },
        SessionSpaceActivationState::Unavailable => WorkingFieldAvailability::Unavailable {
            reason: reason.unwrap_or("component unavailable").into(),
        },
        SessionSpaceActivationState::Removed => WorkingFieldAvailability::Unavailable {
            reason: reason.unwrap_or("component removed").into(),
        },
        SessionSpaceActivationState::Closed => WorkingFieldAvailability::Unavailable {
            reason: reason.unwrap_or("SessionSpace/component closed").into(),
        },
    }
}

fn connection_availability(
    state: SessionSpaceConnectionState,
    reason: Option<&str>,
) -> WorkingFieldAvailability {
    match state {
        SessionSpaceConnectionState::Connected | SessionSpaceConnectionState::Available => {
            WorkingFieldAvailability::Available
        }
        SessionSpaceConnectionState::Connecting
        | SessionSpaceConnectionState::Degraded
        | SessionSpaceConnectionState::Disconnected => WorkingFieldAvailability::Degraded {
            reason: reason
                .unwrap_or("connection is not currently healthy/connected")
                .into(),
        },
        SessionSpaceConnectionState::Unavailable | SessionSpaceConnectionState::Closed => {
            WorkingFieldAvailability::Unavailable {
                reason: reason.unwrap_or("connection unavailable/closed").into(),
            }
        }
    }
}

fn r(raw: &str) -> Result<ResourceRef> {
    ResourceRef::parse(raw)
}
