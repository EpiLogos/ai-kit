//! Read models over resolved [`HarnessComposition`] state.
//!
//! These views are deliberately derived from the composition resolver's evidence.
//! Explain/History consumers do not re-derive provider or lifecycle semantics.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition::{
    ActivationScope, ComponentContribution, CompositionActivationMode, CompositionCatalog,
    HarnessComposition, LifetimeOwner, ResolutionScope, SurfaceDescriptor,
};
use crate::resource::ResourceRef;
use crate::{AikitError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum RequirementResolution {
    Provider {
        provider: ResourceRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_component: Option<ResourceRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_native_provider: Option<String>,
    },
    DirectResource,
    Absent { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementExplanation {
    pub requirement: ResourceRef,
    pub required: bool,
    pub reactive: bool,
    pub resolution: RequirementResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentCompositionExplanation {
    pub component: ResourceRef,
    pub resolution_scope: ResolutionScope,
    pub activation_scope: ActivationScope,
    pub lifetime_owner: LifetimeOwner,
    pub activation_mode: CompositionActivationMode,
    pub requirements: Vec<RequirementExplanation>,
    pub contributions: Vec<ComponentContribution>,
    pub surfaces: Vec<SurfaceDescriptor>,
}

/// Explain one mounted Component using the resolver-owned bindings and absences.
pub fn explain_composed_component(
    catalog: &CompositionCatalog,
    composition: &HarnessComposition,
    component: &ResourceRef,
) -> Result<ComponentCompositionExplanation> {
    let binding = composition
        .component_bindings
        .iter()
        .find(|binding| &binding.component == component)
        .ok_or_else(|| {
            AikitError::new(
                "composition.explain_component_not_mounted",
                format!("component {component} is not mounted in this HarnessComposition"),
            )
        })?;
    let descriptor = catalog.component(component).ok_or_else(|| {
        AikitError::new(
            "composition.explain_component_missing_descriptor",
            format!("component {component} is mounted but has no descriptor in this catalog"),
        )
    })?;

    let mut requirements = descriptor
        .requirements
        .iter()
        .map(|requirement| {
            let resolution = composition
                .contract_bindings
                .iter()
                .find(|bound| {
                    &bound.consumer_component == component
                        && bound.contract == requirement.resource
                })
                .map(|bound| RequirementResolution::Provider {
                    provider: bound.provider.clone(),
                    provider_component: bound.provider_component.clone(),
                    target_native_provider: bound.target_native_provider.clone(),
                })
                .or_else(|| {
                    composition
                        .absences
                        .iter()
                        .find(|absence| {
                            &absence.component == component
                                && absence.requirement == requirement.resource
                        })
                        .map(|absence| RequirementResolution::Absent {
                            reason: absence.reason.clone(),
                        })
                })
                // A successful composition can satisfy a requirement as a direct
                // canonical resource instead of via a Contract/provider binding.
                .unwrap_or(RequirementResolution::DirectResource);
            RequirementExplanation {
                requirement: requirement.resource.clone(),
                required: requirement.strength.is_required(),
                reactive: requirement.reactive,
                resolution,
            }
        })
        .collect::<Vec<_>>();
    requirements.sort_by(|left, right| left.requirement.cmp(&right.requirement));

    let mut contributions = composition
        .contributions
        .iter()
        .filter(|contribution| &contribution.component == component)
        .cloned()
        .collect::<Vec<_>>();
    contributions.sort_by(|left, right| left.id.cmp(&right.id));

    let referenced_surfaces = contributions
        .iter()
        .filter_map(|contribution| contribution.surface.clone())
        .chain(descriptor.supported_surfaces.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut surfaces = composition
        .surfaces
        .iter()
        .filter(|surface| referenced_surfaces.contains(&surface.resource))
        .cloned()
        .collect::<Vec<_>>();
    surfaces.sort_by(|left, right| left.resource.cmp(&right.resource));

    Ok(ComponentCompositionExplanation {
        component: component.clone(),
        resolution_scope: binding.resolution_scope.clone(),
        activation_scope: binding.activation_scope.clone(),
        lifetime_owner: binding.lifetime_owner.clone(),
        activation_mode: binding.activation_mode,
        requirements,
        contributions,
        surfaces,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractRebinding {
    pub consumer_component: ResourceRef,
    pub contract: ResourceRef,
    pub before_provider: ResourceRef,
    pub after_provider: ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionDiff {
    pub before_fingerprint: String,
    pub after_fingerprint: String,
    pub mounted_components: Vec<ResourceRef>,
    pub retracted_components: Vec<ResourceRef>,
    pub rebound_contracts: Vec<ContractRebinding>,
    pub added_contributions: Vec<ResourceRef>,
    pub removed_contributions: Vec<ResourceRef>,
    pub added_surfaces: Vec<ResourceRef>,
    pub removed_surfaces: Vec<ResourceRef>,
}

/// Compare two body resolutions only when their semantic actor/project/harness
/// anchors agree. A caller comparing different actors or harnesses must say so
/// explicitly rather than receiving a misleading "body change" diff.
pub fn diff_harness_compositions(
    before: &HarnessComposition,
    after: &HarnessComposition,
) -> Result<HarnessCompositionDiff> {
    if before.harness != after.harness
        || before.project != after.project
        || before.agent != after.agent
        || before.agency != after.agency
    {
        return Err(AikitError::new(
            "composition.diff_identity_mismatch",
            "HarnessComposition history can only diff bodies with the same Harness/Project/Agent/Agency anchors",
        ));
    }

    let before_components = before
        .component_bindings
        .iter()
        .map(|binding| binding.component.clone())
        .collect::<BTreeSet<_>>();
    let after_components = after
        .component_bindings
        .iter()
        .map(|binding| binding.component.clone())
        .collect::<BTreeSet<_>>();

    let before_contracts = before
        .contract_bindings
        .iter()
        .map(|binding| {
            (
                (binding.consumer_component.clone(), binding.contract.clone()),
                binding.provider.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let after_contracts = after
        .contract_bindings
        .iter()
        .map(|binding| {
            (
                (binding.consumer_component.clone(), binding.contract.clone()),
                binding.provider.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut rebound_contracts = Vec::new();
    for (key, before_provider) in &before_contracts {
        if let Some(after_provider) = after_contracts.get(key) {
            if before_provider != after_provider {
                rebound_contracts.push(ContractRebinding {
                    consumer_component: key.0.clone(),
                    contract: key.1.clone(),
                    before_provider: before_provider.clone(),
                    after_provider: after_provider.clone(),
                });
            }
        }
    }

    let before_contributions = before
        .contributions
        .iter()
        .map(|contribution| contribution.id.clone())
        .collect::<BTreeSet<_>>();
    let after_contributions = after
        .contributions
        .iter()
        .map(|contribution| contribution.id.clone())
        .collect::<BTreeSet<_>>();
    let before_surfaces = before
        .surfaces
        .iter()
        .map(|surface| surface.resource.clone())
        .collect::<BTreeSet<_>>();
    let after_surfaces = after
        .surfaces
        .iter()
        .map(|surface| surface.resource.clone())
        .collect::<BTreeSet<_>>();

    Ok(HarnessCompositionDiff {
        before_fingerprint: before.fingerprint.clone(),
        after_fingerprint: after.fingerprint.clone(),
        mounted_components: after_components
            .difference(&before_components)
            .cloned()
            .collect(),
        retracted_components: before_components
            .difference(&after_components)
            .cloned()
            .collect(),
        rebound_contracts,
        added_contributions: after_contributions
            .difference(&before_contributions)
            .cloned()
            .collect(),
        removed_contributions: before_contributions
            .difference(&after_contributions)
            .cloned()
            .collect(),
        added_surfaces: after_surfaces.difference(&before_surfaces).cloned().collect(),
        removed_surfaces: before_surfaces.difference(&after_surfaces).cloned().collect(),
    })
}
