//! Terminal-native projection of the composed O:I working field.
//!
//! This is a read-model/presentation contract over product-owned Refs, Actions,
//! owners, provenance and permission meaning. It owns no Factory, Central,
//! Actuation, package or SessionSpace state. Selection is handed back to the one
//! existing `TuiState` reducer by canonical `ResourceRef`.

use std::collections::BTreeSet;

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
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
    /// ResourceKind taxonomy and can carry future product types such as
    /// SessionSpace before AIKit owns an implementation for them.
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
    /// Once #61/#62 land this may point at the current live SessionSpace. Before
    /// that ordering gate it may be absent while contract fixtures remain usable.
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
            format!("{subject} is not in working-field revision {}", field.revision),
        ));
    }
    Ok(reduce_tui(state, UiAction::Select(subject)))
}
