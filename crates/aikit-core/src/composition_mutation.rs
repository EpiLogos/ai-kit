//! Explicit staged mutation for the canonical V2 HarnessComposition grammar.
//!
//! This is the runtime-composition counterpart of AI Kit's normal
//! stage -> preview/explain -> confirm -> apply discipline. It deliberately does
//! **not** translate Components, Surfaces or projection bindings into Capsules.
//! The existing composition resolver remains the only authority for the projected
//! body, and applying a confirmed preview accepts a new *desired resolved body*;
//! it does not claim that a material target has mounted it live.

use serde::{Deserialize, Serialize};

use crate::composition::{
    resolve_harness_composition, ComponentSelection, CompositionCatalog, HarnessComposition,
    HarnessCompositionRequest,
};
use crate::composition_view::{diff_harness_compositions, HarnessCompositionDiff};
use crate::resource::ResourceRef;
use crate::Result;

/// One explicit edit to the desired body of a Harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HarnessCompositionMutation {
    /// Mount or replace the selection for a canonical Component identity.
    Select { selection: ComponentSelection },
    /// Retract the selected Component while preserving every unrelated identity.
    Retract { component: ResourceRef },
}

/// User/agent-authored intent waiting for preview. This contains no resolved
/// provider bindings, contributions or Surfaces; those remain resolver output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedHarnessComposition {
    #[serde(default)]
    mutations: Vec<HarnessCompositionMutation>,
}

impl StagedHarnessComposition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select(&mut self, selection: ComponentSelection) {
        // One staged answer per Component identity. Re-staging replaces the prior
        // answer rather than creating ordering-sensitive duplicate intent.
        self.mutations.retain(|mutation| match mutation {
            HarnessCompositionMutation::Select { selection: existing } => {
                existing.component != selection.component
            }
            HarnessCompositionMutation::Retract { component } => component != &selection.component,
        });
        self.mutations
            .push(HarnessCompositionMutation::Select { selection });
    }

    pub fn retract(&mut self, component: ResourceRef) {
        self.mutations.retain(|mutation| match mutation {
            HarnessCompositionMutation::Select { selection } => selection.component != component,
            HarnessCompositionMutation::Retract { component: existing } => existing != &component,
        });
        self.mutations
            .push(HarnessCompositionMutation::Retract { component });
    }

    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    pub fn mutations(&self) -> &[HarnessCompositionMutation] {
        &self.mutations
    }

    fn apply_to(&self, request: &mut HarnessCompositionRequest) {
        for mutation in &self.mutations {
            match mutation {
                HarnessCompositionMutation::Select { selection } => {
                    request
                        .selections
                        .retain(|existing| existing.component != selection.component);
                    request.selections.push(selection.clone());
                }
                HarnessCompositionMutation::Retract { component } => {
                    request
                        .selections
                        .retain(|existing| &existing.component != component);
                }
            }
        }
    }
}

/// Resolver-owned preview of a staged body change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionPreview {
    pub staged: StagedHarnessComposition,
    pub before_fingerprint: String,
    pub projected: HarnessComposition,
    pub diff: HarnessCompositionDiff,
}

impl HarnessCompositionPreview {
    /// Confirmation is a distinct type transition so callers cannot accidentally
    /// apply a preview merely because it was successfully resolved.
    pub fn confirm(self) -> ConfirmedHarnessCompositionPreview {
        ConfirmedHarnessCompositionPreview { preview: self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedHarnessCompositionPreview {
    preview: HarnessCompositionPreview,
}

impl ConfirmedHarnessCompositionPreview {
    pub fn preview(&self) -> &HarnessCompositionPreview {
        &self.preview
    }
}

/// Preview staged Component intent by reusing the canonical resolver.
///
/// The current resolved body is converted back only into its explicit Component
/// selections and stable actor/harness anchors. Provider bindings, contributions,
/// Surfaces and projections are recomputed by `resolve_harness_composition`.
pub fn preview_harness_composition_change(
    catalog: &CompositionCatalog,
    current: &HarnessComposition,
    staged: StagedHarnessComposition,
) -> Result<HarnessCompositionPreview> {
    let mut request = request_from_resolved(current);
    staged.apply_to(&mut request);
    let projected = resolve_harness_composition(catalog, request)?;
    let diff = diff_harness_compositions(current, &projected)?;
    Ok(HarnessCompositionPreview {
        staged,
        before_fingerprint: current.fingerprint.clone(),
        projected,
        diff,
    })
}

/// Apply a *confirmed semantic composition* as the new desired body.
///
/// This function has no target adapter and therefore cannot claim live mounting,
/// process state or Workcell materialisation. The returned composition remains in
/// resolver-owned `CompositionState::Resolved` until an owning target/provider
/// separately observes stronger material truth.
pub fn apply_confirmed_harness_composition(
    confirmed: ConfirmedHarnessCompositionPreview,
) -> HarnessComposition {
    confirmed.preview.projected
}

fn request_from_resolved(current: &HarnessComposition) -> HarnessCompositionRequest {
    HarnessCompositionRequest {
        harness: current.harness.clone(),
        project: current.project.clone(),
        agent: current.agent.clone(),
        agency: current.agency.clone(),
        session: current.session.clone(),
        model: current.model.clone(),
        selections: current
            .component_bindings
            .iter()
            .map(|binding| ComponentSelection {
                component: binding.component.clone(),
                resolution_scope: binding.resolution_scope.clone(),
                activation_scope: binding.activation_scope.clone(),
                lifetime_owner: binding.lifetime_owner.clone(),
                activation_mode: binding.activation_mode,
            })
            .collect(),
        target_revision: current.target_revision.clone(),
        generation: current.generation.clone(),
    }
}
