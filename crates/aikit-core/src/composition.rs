//! Harness/body composition for V2 composition-capable runtimes.
//!
//! This module is deliberately harness-neutral. It models the cross-system facts
//! AIKit must preserve when a Harness body is dynamically composed, without
//! importing Cordis (or any other plugin framework) as the ontology.
//!
//! The principal identity law is that a runtime body is a derived resolution:
//! changing Components, providers, Surfaces, or activation modes never rewrites
//! Project, Agent, Agency, Harness, Action, Capability, or ContextSource identity.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::ResourceKind;
use crate::resource::ResourceRef;
use crate::scope::ScopeKind;
use crate::{AikitError, Result};

pub const HARNESS_COMPOSITION_VERSION: &str = "aikit.harness-composition/v2";

/// Why a declaration participated in the resolution. This is deliberately the
/// existing deterministic precedence scope, not the target activation region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionScope {
    pub scope: ScopeKind,
    pub origin: String,
}

impl ResolutionScope {
    pub fn new(scope: ScopeKind, origin: impl Into<String>) -> Self {
        Self {
            scope,
            origin: origin.into(),
        }
    }
}

/// Where a resolved contribution is actually visible/operative in the target.
/// This must remain independent from `ResolutionScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationScopeKind {
    Global,
    Host,
    Project,
    AgentSession,
    Task,
    RuntimeRegion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationScope {
    pub kind: ActivationScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl ActivationScope {
    pub fn new(kind: ActivationScopeKind) -> Self {
        Self {
            kind,
            reference: None,
        }
    }

    #[must_use]
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }
}

/// What owns the active contribution and therefore owns its retraction/rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifetimeOwnerKind {
    Generation,
    AgentSession,
    Task,
    ComponentContext,
    Procedure,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifetimeOwner {
    pub kind: LifetimeOwnerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl LifetimeOwner {
    pub fn new(kind: LifetimeOwnerKind) -> Self {
        Self {
            kind,
            reference: None,
        }
    }

    #[must_use]
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }
}

/// How a selected component/contribution becomes effective. These are target
/// truths, not UI labels; adapters may only claim LiveMounted when they can prove
/// the target actually activates/retracts it live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionActivationMode {
    Generated,
    LiveMounted,
    NextSession,
    ProcedureMediated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetractionMode {
    Live,
    Restart,
    NextSession,
    ProcedureMediated,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequirementStrength {
    Required,
    Optional,
}

impl RequirementStrength {
    pub fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }
}

/// A condition supplied by the surrounding runtime. `resource` may identify a
/// Contract or another canonical resource. Reactive coeffect semantics are only
/// claimed when the target adapter reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentRequirement {
    pub resource: ResourceRef,
    pub strength: RequirementStrength,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_scope: Option<ActivationScope>,
    #[serde(default)]
    pub reactive: bool,
}

impl ComponentRequirement {
    pub fn required(resource: ResourceRef) -> Self {
        Self {
            resource,
            strength: RequirementStrength::Required,
            compatibility: None,
            activation_scope: None,
            reactive: false,
        }
    }

    pub fn optional(resource: ResourceRef) -> Self {
        Self {
            resource,
            strength: RequirementStrength::Optional,
            compatibility: None,
            activation_scope: None,
            reactive: false,
        }
    }

    #[must_use]
    pub fn with_compatibility(mut self, compatibility: impl Into<String>) -> Self {
        self.compatibility = Some(compatibility.into());
        self
    }

    #[must_use]
    pub fn reactive(mut self) -> Self {
        self.reactive = true;
        self
    }
}

/// Provider is a role/binding, not the identity of the Contract it satisfies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractProvider {
    pub contract: ResourceRef,
    pub provider: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ResourceRef>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub compatibility: BTreeSet<String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_native_id: Option<String>,
}

impl ContractProvider {
    pub fn available(contract: ResourceRef, provider: ResourceRef) -> Self {
        Self {
            contract,
            provider,
            component: None,
            priority: 0,
            compatibility: BTreeSet::new(),
            available: true,
            unavailable_reason: None,
            target_native_id: None,
        }
    }

    #[must_use]
    pub fn supplied_by(mut self, component: ResourceRef) -> Self {
        self.component = Some(component);
        self
    }

    #[must_use]
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    #[must_use]
    pub fn with_compatibility(mut self, tag: impl Into<String>) -> Self {
        self.compatibility.insert(tag.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContributionKind {
    Service,
    Tool,
    ActionProjection,
    HumanCommand,
    ContextSection,
    Policy,
    ContextSourceFaculty,
    ReadModel,
    UiNode,
    Trajectory,
    Observer,
    ModelAdapter,
    LoopRuntime,
    Subagent,
    Filesystem,
    Shell,
    Sandbox,
}

/// A lifecycle-owned effect produced by one mounted Component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentContribution {
    pub id: ResourceRef,
    pub component: ResourceRef,
    pub kind: ContributionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_contract: Option<ResourceRef>,
    /// Canonical thing exposed by this contribution, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed_ref: Option<ResourceRef>,
    /// Preserves whether the exposed thing is an Action, Capability, Reading-like
    /// knowledge object, etc. Surface projection never rewrites this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed_kind: Option<ResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<ResourceRef>,
    pub activation_scope: ActivationScope,
    pub lifetime_owner: LifetimeOwner,
    pub activation_mode: CompositionActivationMode,
    pub retraction_mode: RetractionMode,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceKind {
    Cli,
    AgentTool,
    Conversation,
    Trajectory,
    Tui,
    Web,
    Api,
    Automation,
    Editor,
}

/// Surface is encounter/operation locus; ProjectionBinding below records what is
/// represented there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceDescriptor {
    pub resource: ResourceRef,
    pub kind: SurfaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_native_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_component: Option<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetNativeComponentBinding {
    pub implementation_target: String,
    pub native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// Cross-system descriptor for one composable runtime unit. Target-private plugin
/// configuration remains target-private; this contains only interoperability facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDescriptor {
    pub resource: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<TargetNativeComponentBinding>,
    #[serde(default)]
    pub requirements: Vec<ComponentRequirement>,
    #[serde(default)]
    pub provisions: Vec<ResourceRef>,
    #[serde(default)]
    pub contributions: Vec<ComponentContribution>,
    #[serde(default)]
    pub supported_surfaces: Vec<ResourceRef>,
    #[serde(default)]
    pub activation_modes: BTreeSet<CompositionActivationMode>,
}

impl ComponentDescriptor {
    pub fn new(resource: ResourceRef) -> Self {
        Self {
            resource,
            implementation: None,
            requirements: Vec::new(),
            provisions: Vec::new(),
            contributions: Vec::new(),
            supported_surfaces: Vec::new(),
            activation_modes: BTreeSet::new(),
        }
    }
}

/// The three different scope/lifetime answers travel together in a selection but
/// never collapse into one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentSelection {
    pub component: ResourceRef,
    pub resolution_scope: ResolutionScope,
    pub activation_scope: ActivationScope,
    pub lifetime_owner: LifetimeOwner,
    pub activation_mode: CompositionActivationMode,
}

#[derive(Debug, Clone, Default)]
pub struct CompositionCatalog {
    components: BTreeMap<ResourceRef, ComponentDescriptor>,
    surfaces: BTreeMap<ResourceRef, SurfaceDescriptor>,
    providers: Vec<ContractProvider>,
    available_resources: BTreeSet<ResourceRef>,
}

impl CompositionCatalog {
    pub fn insert_component(
        &mut self,
        component: ComponentDescriptor,
    ) -> Option<ComponentDescriptor> {
        self.components
            .insert(component.resource.clone(), component)
    }

    pub fn insert_surface(&mut self, surface: SurfaceDescriptor) -> Option<SurfaceDescriptor> {
        self.surfaces.insert(surface.resource.clone(), surface)
    }

    pub fn add_provider(&mut self, provider: ContractProvider) {
        self.providers.push(provider);
    }

    pub fn mark_resource_available(&mut self, resource: ResourceRef) {
        self.available_resources.insert(resource);
    }

    pub fn component(&self, resource: &ResourceRef) -> Option<&ComponentDescriptor> {
        self.components.get(resource)
    }

    pub fn surface(&self, resource: &ResourceRef) -> Option<&SurfaceDescriptor> {
        self.surfaces.get(resource)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionRequest {
    pub harness: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ResourceRef>,
    #[serde(default)]
    pub selections: Vec<ComponentSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentBinding {
    pub component: ResourceRef,
    pub resolution_scope: ResolutionScope,
    pub activation_scope: ActivationScope,
    pub lifetime_owner: LifetimeOwner,
    pub activation_mode: CompositionActivationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<TargetNativeComponentBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractBinding {
    pub consumer_component: ResourceRef,
    pub contract: ResourceRef,
    pub provider: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_component: Option<ResourceRef>,
    pub required: bool,
    pub reactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_native_provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionBinding {
    pub canonical_ref: ResourceRef,
    pub canonical_kind: ResourceKind,
    pub contribution: ResourceRef,
    pub component: ResourceRef,
    pub surface: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_native_surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionAbsence {
    pub component: ResourceRef,
    pub requirement: ResourceRef,
    pub required: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionState {
    Resolved,
    ObservedActive,
}

/// The derived current/desired body of one Harness + actor/session relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessComposition {
    pub version: String,
    pub harness: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ResourceRef>,
    pub component_bindings: Vec<ComponentBinding>,
    pub contract_bindings: Vec<ContractBinding>,
    pub contributions: Vec<ComponentContribution>,
    pub surfaces: Vec<SurfaceDescriptor>,
    pub projections: Vec<ProjectionBinding>,
    pub absences: Vec<CompositionAbsence>,
    pub state: CompositionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    /// Deterministic fingerprint of the resolved body, independent from Project,
    /// Agent and Harness semantic identities.
    pub fingerprint: String,
}

/// Distinct relation meanings available to common list/tree/graph renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionRelationKind {
    Contain,
    Federate,
    Frame,
    Compose,
    Require,
    Provide,
    Contribute,
    Scope,
    Project,
    Bind,
    Map,
    Refract,
}

pub fn resolve_harness_composition(
    catalog: &CompositionCatalog,
    request: HarnessCompositionRequest,
) -> Result<HarnessComposition> {
    let mut selections = request.selections;
    selections.sort_by(|left, right| left.component.cmp(&right.component));
    for pair in selections.windows(2) {
        if pair[0].component == pair[1].component {
            return Err(AikitError::new(
                "composition.duplicate_component",
                format!(
                    "component {} was selected more than once",
                    pair[0].component
                ),
            ));
        }
    }

    let selected: BTreeSet<_> = selections
        .iter()
        .map(|selection| selection.component.clone())
        .collect();
    let mut component_bindings = Vec::new();
    let mut contract_bindings = Vec::new();
    let mut contributions = Vec::new();
    let mut surfaces = BTreeMap::<ResourceRef, SurfaceDescriptor>::new();
    let mut projections = Vec::new();
    let mut absences = Vec::new();

    for selection in &selections {
        let descriptor = catalog.component(&selection.component).ok_or_else(|| {
            AikitError::new(
                "composition.unknown_component",
                format!(
                    "selected component {} is not present in the composition catalog",
                    selection.component
                ),
            )
        })?;
        if !descriptor.activation_modes.is_empty()
            && !descriptor
                .activation_modes
                .contains(&selection.activation_mode)
        {
            return Err(AikitError::new(
                "composition.unsupported_activation_mode",
                format!(
                    "component {} does not advertise activation mode {:?}",
                    descriptor.resource, selection.activation_mode
                ),
            ));
        }

        component_bindings.push(ComponentBinding {
            component: selection.component.clone(),
            resolution_scope: selection.resolution_scope.clone(),
            activation_scope: selection.activation_scope.clone(),
            lifetime_owner: selection.lifetime_owner.clone(),
            activation_mode: selection.activation_mode,
            implementation: descriptor.implementation.clone(),
        });

        for requirement in &descriptor.requirements {
            if let Some(provider) = select_provider(catalog, requirement, &selected) {
                contract_bindings.push(ContractBinding {
                    consumer_component: descriptor.resource.clone(),
                    contract: requirement.resource.clone(),
                    provider: provider.provider.clone(),
                    provider_component: provider.component.clone(),
                    required: requirement.strength.is_required(),
                    reactive: requirement.reactive,
                    target_native_provider: provider.target_native_id.clone(),
                });
                continue;
            }

            if catalog.available_resources.contains(&requirement.resource)
                || selected.contains(&requirement.resource)
            {
                // A direct canonical resource requirement is satisfied without a
                // Contract-provider binding. Its identity remains the resource.
                continue;
            }

            let reason = provider_absence_reason(catalog, requirement, &selected);
            if requirement.strength.is_required() {
                return Err(AikitError::new(
                    "composition.required_requirement_unsatisfied",
                    format!(
                        "component {} requires {}: {reason}",
                        descriptor.resource, requirement.resource
                    ),
                )
                .with("component", descriptor.resource.to_string())
                .with("requirement", requirement.resource.to_string()));
            }
            absences.push(CompositionAbsence {
                component: descriptor.resource.clone(),
                requirement: requirement.resource.clone(),
                required: false,
                reason,
            });
        }

        for surface_ref in &descriptor.supported_surfaces {
            let surface = catalog.surface(surface_ref).ok_or_else(|| {
                AikitError::new(
                    "composition.unknown_surface",
                    format!(
                        "component {} references unknown surface {surface_ref}",
                        descriptor.resource
                    ),
                )
            })?;
            surfaces.insert(surface.resource.clone(), surface.clone());
        }

        for contribution in &descriptor.contributions {
            if contribution.component != descriptor.resource {
                return Err(AikitError::new(
                    "composition.contribution_owner_mismatch",
                    format!(
                        "contribution {} claims component {}, but is declared by {}",
                        contribution.id, contribution.component, descriptor.resource
                    ),
                ));
            }
            if let Some(surface_ref) = contribution.surface.as_ref() {
                let surface = catalog.surface(surface_ref).ok_or_else(|| {
                    AikitError::new(
                        "composition.unknown_surface",
                        format!(
                            "contribution {} references unknown surface {surface_ref}",
                            contribution.id
                        ),
                    )
                })?;
                surfaces.insert(surface.resource.clone(), surface.clone());
                match (&contribution.exposed_ref, contribution.exposed_kind) {
                    (Some(canonical_ref), Some(canonical_kind)) => projections.push(ProjectionBinding {
                        canonical_ref: canonical_ref.clone(),
                        canonical_kind,
                        contribution: contribution.id.clone(),
                        component: descriptor.resource.clone(),
                        surface: surface_ref.clone(),
                        target_native_surface: surface.target_native_id.clone(),
                    }),
                    (None, None) => {}
                    _ => {
                        return Err(AikitError::new(
                            "composition.incomplete_projection_identity",
                            format!(
                                "contribution {} must supply both exposed_ref and exposed_kind, or neither",
                                contribution.id
                            ),
                        ))
                    }
                }
            }
            contributions.push(contribution.clone());
        }
    }

    contract_bindings.sort_by(|left, right| {
        left.consumer_component
            .cmp(&right.consumer_component)
            .then_with(|| left.contract.cmp(&right.contract))
            .then_with(|| left.provider.cmp(&right.provider))
    });
    contributions.sort_by(|left, right| left.id.cmp(&right.id));
    projections.sort_by(|left, right| {
        left.canonical_ref
            .cmp(&right.canonical_ref)
            .then_with(|| left.surface.cmp(&right.surface))
    });
    absences.sort_by(|left, right| {
        left.component
            .cmp(&right.component)
            .then_with(|| left.requirement.cmp(&right.requirement))
    });
    let surfaces = surfaces.into_values().collect::<Vec<_>>();

    let fingerprint = fingerprint(&(
        &request.harness,
        &request.model,
        &component_bindings,
        &contract_bindings,
        &contributions,
        &surfaces,
        &projections,
        &absences,
        &request.target_revision,
        &request.generation,
    ))?;

    Ok(HarnessComposition {
        version: HARNESS_COMPOSITION_VERSION.to_string(),
        harness: request.harness,
        project: request.project,
        agent: request.agent,
        agency: request.agency,
        session: request.session,
        model: request.model,
        component_bindings,
        contract_bindings,
        contributions,
        surfaces,
        projections,
        absences,
        state: CompositionState::Resolved,
        target_revision: request.target_revision,
        generation: request.generation,
        fingerprint,
    })
}

fn select_provider<'a>(
    catalog: &'a CompositionCatalog,
    requirement: &ComponentRequirement,
    selected: &BTreeSet<ResourceRef>,
) -> Option<&'a ContractProvider> {
    let mut providers = catalog
        .providers
        .iter()
        .filter(|provider| provider.contract == requirement.resource)
        .filter(|provider| provider.available)
        .filter(|provider| {
            provider
                .component
                .as_ref()
                .is_none_or(|component| selected.contains(component))
        })
        .filter(|provider| {
            requirement
                .compatibility
                .as_ref()
                .is_none_or(|tag| provider.compatibility.contains(tag))
        })
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.provider.cmp(&right.provider))
    });
    providers.into_iter().next()
}

fn provider_absence_reason(
    catalog: &CompositionCatalog,
    requirement: &ComponentRequirement,
    selected: &BTreeSet<ResourceRef>,
) -> String {
    let candidates = catalog
        .providers
        .iter()
        .filter(|provider| provider.contract == requirement.resource)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return "no provider is declared".into();
    }
    if candidates.iter().all(|provider| !provider.available) {
        return candidates
            .iter()
            .filter_map(|provider| provider.unavailable_reason.clone())
            .next()
            .unwrap_or_else(|| "all declared providers are unavailable".into());
    }
    if candidates.iter().all(|provider| {
        provider
            .component
            .as_ref()
            .is_some_and(|component| !selected.contains(component))
    }) {
        return "provider exists but its supplying Component is not selected".into();
    }
    if let Some(tag) = requirement.compatibility.as_ref() {
        if candidates
            .iter()
            .filter(|provider| provider.available)
            .all(|provider| !provider.compatibility.contains(tag))
        {
            return format!("no available provider satisfies compatibility `{tag}`");
        }
    }
    "no provider is currently bindable".into()
}

fn fingerprint(value: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(value).map_err(|error| {
        AikitError::new(
            "composition.fingerprint_failed",
            format!("could not encode harness composition: {error}"),
        )
    })?;
    Ok(blake3::hash(&encoded).to_hex().to_string())
}
