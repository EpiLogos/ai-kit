from pathlib import Path

composition = Path('crates/aikit-core/src/composition.rs')
text = composition.read_text()

const = 'pub const HARNESS_COMPOSITION_VERSION: &str = "aikit.harness-composition/v2";'
if text.count(const) != 1:
    raise SystemExit('unexpected HarnessComposition version declaration')
text = text.replace(
    const,
    'pub const COMPOSITION_BODY_VERSION: &str = "aikit.composition-body/v1";\n' + const,
    1,
)

harness_request = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionRequest {'''
if text.count(harness_request) != 1:
    raise SystemExit('unexpected HarnessCompositionRequest declaration')
body_request = '''/// Scope-neutral request for the reusable Component/Contract/Contribution/Surface body.
///
/// Harness, Project, Agent, Agency, AgentSession and provider-local host identity
/// deliberately do not participate here. Those relations wrap or bind this body
/// at their own semantic altitude.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionBodyRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ResourceRef>,
    #[serde(default)]
    pub selections: Vec<ComponentSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

'''
text = text.replace(harness_request, body_request + harness_request, 1)

harness_comment = '''/// The derived current/desired body of one Harness + actor/session relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessComposition {'''
if text.count(harness_comment) != 1:
    raise SystemExit('unexpected HarnessComposition declaration')
body_struct = '''/// Scope-neutral resolved composition body. This is the reusable kernel beneath
/// HarnessComposition and host/environment composition. Provider-local bindings
/// remain observations outside this body unless represented by canonical
/// Component/Contract/Contribution/Surface relations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionBody {
    pub version: String,
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
    /// Deterministic fingerprint of only the scope-neutral resolved body.
    pub fingerprint: String,
}

'''
text = text.replace(harness_comment, body_struct + harness_comment, 1)

old_comment = '''    /// Deterministic fingerprint of the resolved body, independent from Project,
    /// Agent and Harness semantic identities.
    pub fingerprint: String,'''
new_comment = '''    /// Deterministic fingerprint of this Harness-scoped wrapper. Project, Agent,
    /// Agency and session identity remain outside it; Harness identity participates
    /// for backward-compatible HarnessComposition history. Use CompositionBody's
    /// fingerprint for scope-neutral composition identity.
    pub fingerprint: String,'''
if text.count(old_comment) != 1:
    raise SystemExit('unexpected HarnessComposition fingerprint comment')
text = text.replace(old_comment, new_comment, 1)

resolver_start = text.index('pub fn resolve_harness_composition(')
resolver_end = text.index('\nfn select_provider', resolver_start)
new_resolvers = r'''pub fn resolve_harness_composition(
    catalog: &CompositionCatalog,
    request: HarnessCompositionRequest,
) -> Result<HarnessComposition> {
    let HarnessCompositionRequest {
        harness,
        project,
        agent,
        agency,
        session,
        model,
        selections,
        target_revision,
        generation,
    } = request;

    let body = resolve_composition_body(
        catalog,
        CompositionBodyRequest {
            model,
            selections,
            target_revision,
            generation,
        },
    )?;

    // Preserve the existing HarnessComposition fingerprint contract while the
    // reusable body now has its own identity independent of Harness semantics.
    let fingerprint = fingerprint(&(
        &harness,
        &body.model,
        &body.component_bindings,
        &body.contract_bindings,
        &body.contributions,
        &body.surfaces,
        &body.projections,
        &body.absences,
        &body.target_revision,
        &body.generation,
    ))?;

    Ok(HarnessComposition {
        version: HARNESS_COMPOSITION_VERSION.to_string(),
        harness,
        project,
        agent,
        agency,
        session,
        model: body.model,
        component_bindings: body.component_bindings,
        contract_bindings: body.contract_bindings,
        contributions: body.contributions,
        surfaces: body.surfaces,
        projections: body.projections,
        absences: body.absences,
        state: body.state,
        target_revision: body.target_revision,
        generation: body.generation,
        fingerprint,
    })
}

/// Resolve the reusable composition body without requiring a Harness, actor,
/// SessionSpace, host, renderer or provider-native identity.
pub fn resolve_composition_body(
    catalog: &CompositionCatalog,
    request: CompositionBodyRequest,
) -> Result<CompositionBody> {
    let mut selections = request.selections;
    selections.sort_by(|left, right| left.component.cmp(&right.component));
    for pair in selections.windows(2) {
        if pair[0].component == pair[1].component {
            return Err(AikitError::new(
                "composition.duplicate_component",
                format!("component {} was selected more than once", pair[0].component),
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
            && !descriptor.activation_modes.contains(&selection.activation_mode)
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
                    (Some(canonical_ref), Some(canonical_kind)) => {
                        projections.push(ProjectionBinding {
                            canonical_ref: canonical_ref.clone(),
                            canonical_kind,
                            contribution: contribution.id.clone(),
                            component: descriptor.resource.clone(),
                            surface: surface_ref.clone(),
                            target_native_surface: surface.target_native_id.clone(),
                        })
                    }
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

    Ok(CompositionBody {
        version: COMPOSITION_BODY_VERSION.to_string(),
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
'''
text = text[:resolver_start] + new_resolvers + text[resolver_end:]
composition.write_text(text)

lib = Path('crates/aikit-core/src/lib.rs')
lib_text = lib.read_text()
old_exports = '''pub use composition::{
    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentBinding,
    ComponentContribution, ComponentDescriptor, ComponentRequirement, ComponentSelection,
    CompositionAbsence, CompositionActivationMode, CompositionCatalog, CompositionRelationKind,
    CompositionState, ContractBinding, ContractProvider, ContributionKind, HarnessComposition,
    HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind, ProjectionBinding,
    RequirementStrength, ResolutionScope, RetractionMode, SurfaceDescriptor, SurfaceKind,
    TargetNativeComponentBinding, HARNESS_COMPOSITION_VERSION,
};'''
new_exports = '''pub use composition::{
    resolve_composition_body, resolve_harness_composition, ActivationScope, ActivationScopeKind,
    ComponentBinding, ComponentContribution, ComponentDescriptor, ComponentRequirement,
    ComponentSelection, CompositionAbsence, CompositionActivationMode, CompositionBody,
    CompositionBodyRequest, CompositionCatalog, CompositionRelationKind, CompositionState,
    ContractBinding, ContractProvider, ContributionKind, HarnessComposition,
    HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind, ProjectionBinding,
    RequirementStrength, ResolutionScope, RetractionMode, SurfaceDescriptor, SurfaceKind,
    TargetNativeComponentBinding, COMPOSITION_BODY_VERSION, HARNESS_COMPOSITION_VERSION,
};'''
if lib_text.count(old_exports) != 1:
    raise SystemExit('unexpected composition root export block')
lib.write_text(lib_text.replace(old_exports, new_exports, 1))

test = Path('crates/aikit-core/tests/composition_body_v1.rs')
test.write_text(r'''use aikit_core::{
    resolve_composition_body, resolve_harness_composition, ActivationScope, ActivationScopeKind,
    ComponentDescriptor, ComponentSelection, CompositionActivationMode, CompositionBodyRequest,
    CompositionCatalog, HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind,
    ResolutionScope, SurfaceDescriptor, SurfaceKind, COMPOSITION_BODY_VERSION,
};
use aikit_core::resource::ResourceRef;
use aikit_core::scope::ScopeKind;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn fixture() -> (CompositionCatalog, ComponentSelection) {
    let component = r("component/reference-world/shell");
    let surface = r("surface/reference-world/ambient");
    let mut catalog = CompositionCatalog::default();
    catalog.insert_surface(SurfaceDescriptor {
        resource: surface.clone(),
        kind: SurfaceKind::Tui,
        target_native_id: None,
        owner_component: Some(component.clone()),
    });
    let mut descriptor = ComponentDescriptor::new(component.clone());
    descriptor.supported_surfaces.push(surface);
    catalog.insert_component(descriptor);
    let selection = ComponentSelection {
        component,
        resolution_scope: ResolutionScope::new(ScopeKind::Session, "reference-world fixture"),
        activation_scope: ActivationScope::new(ActivationScopeKind::Host),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation),
        activation_mode: CompositionActivationMode::Generated,
    };
    (catalog, selection)
}

#[test]
fn scope_neutral_body_does_not_require_harness_or_actor_identity() {
    let (catalog, selection) = fixture();
    let body = resolve_composition_body(
        &catalog,
        CompositionBodyRequest {
            model: None,
            selections: vec![selection],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();

    assert_eq!(body.version, COMPOSITION_BODY_VERSION);
    assert_eq!(body.component_bindings.len(), 1);
    assert_eq!(body.surfaces.len(), 1);
    assert!(!body.fingerprint.is_empty());
}

#[test]
fn harness_composition_wraps_the_same_body_without_defining_body_identity() {
    let (catalog, selection) = fixture();
    let body = resolve_composition_body(
        &catalog,
        CompositionBodyRequest {
            model: None,
            selections: vec![selection.clone()],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();

    let harness_a = resolve_harness_composition(
        &catalog,
        HarnessCompositionRequest {
            harness: r("harness/a"),
            project: Some(r("project/reference")),
            agent: None,
            agency: None,
            session: Some("session-a".into()),
            model: None,
            selections: vec![selection.clone()],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();
    let harness_b = resolve_harness_composition(
        &catalog,
        HarnessCompositionRequest {
            harness: r("harness/b"),
            project: Some(r("project/reference")),
            agent: None,
            agency: None,
            session: Some("session-b".into()),
            model: None,
            selections: vec![selection],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();

    assert_eq!(harness_a.component_bindings, body.component_bindings);
    assert_eq!(harness_b.component_bindings, body.component_bindings);
    assert_eq!(harness_a.surfaces, body.surfaces);
    assert_eq!(harness_b.surfaces, body.surfaces);
    assert_ne!(harness_a.fingerprint, harness_b.fingerprint);

    let body_again = resolve_composition_body(
        &catalog,
        CompositionBodyRequest {
            model: None,
            selections: vec![ComponentSelection {
                component: harness_a.component_bindings[0].component.clone(),
                resolution_scope: harness_a.component_bindings[0].resolution_scope.clone(),
                activation_scope: harness_a.component_bindings[0].activation_scope.clone(),
                lifetime_owner: harness_a.component_bindings[0].lifetime_owner.clone(),
                activation_mode: harness_a.component_bindings[0].activation_mode,
            }],
            target_revision: Some("provider-revision-1".into()),
            generation: Some("generation-1".into()),
        },
    )
    .unwrap();
    assert_eq!(body.fingerprint, body_again.fingerprint);
}
''')

Path('.github/oi158_composition_extract.py').unlink()
Path('.github/workflows/oi158-composition-extract.yml').unlink()
