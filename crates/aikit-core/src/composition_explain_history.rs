//! Explain projections for resolver-owned HarnessComposition evidence.
//!
//! These functions are pure read models. They do not resolve a body, activate a
//! target or persist history; they only classify evidence the composition resolver
//! already produced.

use crate::composition::{CompositionCatalog, HarnessComposition};
use crate::composition_mutation::{HarnessCompositionMutation, HarnessCompositionPreview};
use crate::composition_view::{explain_composed_component, RequirementResolution};
use crate::explain_history::{EvidenceProvenance, ExplainEvidence, ExplainFact};
use crate::resource::{ResourceRef, SourceAuthority};
use crate::Result;

pub fn explain_harness_composition_preview(preview: &HarnessCompositionPreview) -> ExplainEvidence {
    let subject = preview.projected.harness.clone();
    let mut staged_refs = Vec::new();
    for mutation in preview.staged.mutations() {
        match mutation {
            HarnessCompositionMutation::Select { selection } => {
                staged_refs.push(selection.component.clone());
            }
            HarnessCompositionMutation::Retract { component } => {
                staged_refs.push(component.clone());
            }
        }
    }
    staged_refs.sort();
    staged_refs.dedup();

    let mut changed_refs = preview.diff.mounted_components.clone();
    changed_refs.extend(preview.diff.retracted_components.iter().cloned());
    changed_refs.extend(preview.diff.added_contributions.iter().cloned());
    changed_refs.extend(preview.diff.removed_contributions.iter().cloned());
    changed_refs.extend(preview.diff.added_surfaces.iter().cloned());
    changed_refs.extend(preview.diff.removed_surfaces.iter().cloned());
    for rebound in &preview.diff.rebound_contracts {
        changed_refs.extend([
            rebound.consumer_component.clone(),
            rebound.contract.clone(),
            rebound.before_provider.clone(),
            rebound.after_provider.clone(),
        ]);
    }
    changed_refs.sort();
    changed_refs.dedup();

    let mut facts = vec![
        ExplainFact {
            relation: "staged-runtime-body-intent".into(),
            authority: Some(SourceAuthority::Authored),
            summary: format!(
                "{} staged Component mutation{} awaiting apply",
                preview.staged.len(),
                plural(preview.staged.len())
            ),
            canonical_refs: staged_refs,
            provenance: Vec::new(),
        },
        ExplainFact {
            relation: "projected-runtime-body-diff".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "body {} -> {}: +{} Component{}, -{} Component{}, {} Contract rebind{}",
                preview.before_fingerprint,
                preview.projected.fingerprint,
                preview.diff.mounted_components.len(),
                plural(preview.diff.mounted_components.len()),
                preview.diff.retracted_components.len(),
                plural(preview.diff.retracted_components.len()),
                preview.diff.rebound_contracts.len(),
                plural(preview.diff.rebound_contracts.len())
            ),
            canonical_refs: changed_refs,
            provenance: Vec::new(),
        },
    ];

    // Activation mode is a resolver/adapter contract about how the desired body
    // would become effective. A preview never upgrades it into an observation that
    // a target has actually mounted the body.
    for binding in &preview.projected.component_bindings {
        facts.push(ExplainFact {
            relation: "projected-component-activation".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "{} resolves from {:?} ({}) into {:?} / {:?}; lifetime {:?} / {:?}; target effect {:?}",
                binding.component,
                binding.resolution_scope.scope,
                binding.resolution_scope.origin,
                binding.activation_scope.kind,
                binding.activation_scope.reference,
                binding.lifetime_owner.kind,
                binding.lifetime_owner.reference,
                binding.activation_mode
            ),
            canonical_refs: vec![binding.component.clone()],
            provenance: binding
                .implementation
                .as_ref()
                .map(|implementation| EvidenceProvenance {
                    revision: implementation.revision.clone(),
                    native_id: Some(implementation.native_id.clone()),
                    ..EvidenceProvenance::default()
                })
                .into_iter()
                .collect(),
        });
    }

    ExplainEvidence {
        schema: crate::EXPLAIN_HISTORY_VERSION.into(),
        subject,
        facts,
    }
}

pub fn explain_harness_component(
    catalog: &CompositionCatalog,
    composition: &HarnessComposition,
    component: &ResourceRef,
) -> Result<ExplainEvidence> {
    let explanation = explain_composed_component(catalog, composition, component)?;
    let mut facts = vec![ExplainFact {
        relation: "component-composition".into(),
        authority: Some(SourceAuthority::Derived),
        summary: format!(
            "resolution {:?} ({}) · activation {:?} / {:?} · lifetime {:?} / {:?} · effect {:?}",
            explanation.resolution_scope.scope,
            explanation.resolution_scope.origin,
            explanation.activation_scope.kind,
            explanation.activation_scope.reference,
            explanation.lifetime_owner.kind,
            explanation.lifetime_owner.reference,
            explanation.activation_mode
        ),
        canonical_refs: vec![component.clone()],
        provenance: Vec::new(),
    }];

    for requirement in explanation.requirements {
        let mut refs = vec![requirement.requirement.clone()];
        let mut provenance = Vec::new();
        let resolution = match requirement.resolution {
            RequirementResolution::Provider {
                provider,
                provider_component,
                target_native_provider,
            } => {
                refs.push(provider.clone());
                if let Some(provider_component) = provider_component {
                    refs.push(provider_component.clone());
                }
                if target_native_provider.is_some() {
                    provenance.push(EvidenceProvenance {
                        provider: Some(provider.clone()),
                        native_id: target_native_provider,
                        ..EvidenceProvenance::default()
                    });
                }
                format!("provided by {provider}")
            }
            RequirementResolution::DirectResource => "satisfied directly".into(),
            RequirementResolution::Absent { reason } => format!("absent: {reason}"),
        };
        facts.push(ExplainFact {
            relation: "component-requirement".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "{} · {} · {} · {resolution}",
                requirement.requirement,
                if requirement.required { "required" } else { "optional" },
                if requirement.reactive { "reactive" } else { "non-reactive" }
            ),
            canonical_refs: refs,
            provenance,
        });
    }

    for contribution in explanation.contributions {
        let mut refs = vec![contribution.id.clone(), contribution.component.clone()];
        refs.extend(contribution.target_contract.iter().cloned());
        refs.extend(contribution.exposed_ref.iter().cloned());
        refs.extend(contribution.surface.iter().cloned());
        facts.push(ExplainFact {
            relation: "component-contribution".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!(
                "{} contributes {:?}; exposed kind {:?}; activation {:?}; retraction {:?}",
                contribution.id,
                contribution.kind,
                contribution.exposed_kind,
                contribution.activation_mode,
                contribution.retraction_mode
            ),
            canonical_refs: refs,
            provenance: Vec::new(),
        });
    }

    for surface in explanation.surfaces {
        let mut refs = vec![surface.resource.clone()];
        refs.extend(surface.owner_component.iter().cloned());
        facts.push(ExplainFact {
            relation: "component-surface".into(),
            authority: Some(SourceAuthority::Derived),
            summary: format!("{} exposes {:?} Surface", surface.resource, surface.kind),
            canonical_refs: refs,
            provenance: surface
                .target_native_id
                .map(|native_id| EvidenceProvenance {
                    native_id: Some(native_id),
                    ..EvidenceProvenance::default()
                })
                .into_iter()
                .collect(),
        });
    }

    Ok(ExplainEvidence {
        schema: crate::EXPLAIN_HISTORY_VERSION.into(),
        subject: component.clone(),
        facts,
    })
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::{
        ActivationScope, ActivationScopeKind, ComponentBinding, CompositionActivationMode,
        CompositionState, HarnessComposition, LifetimeOwner, LifetimeOwnerKind, ResolutionScope,
    };
    use crate::composition_mutation::StagedHarnessComposition;
    use crate::composition_view::HarnessCompositionDiff;
    use crate::scope::ScopeKind;

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    #[test]
    fn staged_preview_is_authored_but_projected_activation_is_only_derived() {
        let component = r("component/editor");
        let mut staged = StagedHarnessComposition::new();
        staged.retract(component.clone());
        let projected = HarnessComposition {
            version: "test".into(),
            harness: r("harness/test"),
            project: Some(r("project/test")),
            agent: None,
            agency: None,
            session: None,
            model: None,
            component_bindings: vec![ComponentBinding {
                component: component.clone(),
                resolution_scope: ResolutionScope::new(ScopeKind::Project, "project/test"),
                activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
                lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
                activation_mode: CompositionActivationMode::LiveMounted,
                implementation: None,
            }],
            contract_bindings: Vec::new(),
            contributions: Vec::new(),
            surfaces: Vec::new(),
            projections: Vec::new(),
            absences: Vec::new(),
            state: CompositionState::Resolved,
            target_revision: None,
            generation: None,
            fingerprint: "body-1".into(),
        };
        let preview = HarnessCompositionPreview {
            staged,
            before_fingerprint: "body-0".into(),
            diff: HarnessCompositionDiff {
                before_fingerprint: "body-0".into(),
                after_fingerprint: "body-1".into(),
                mounted_components: Vec::new(),
                retracted_components: vec![component.clone()],
                rebound_contracts: Vec::new(),
                added_contributions: Vec::new(),
                removed_contributions: Vec::new(),
                added_surfaces: Vec::new(),
                removed_surfaces: Vec::new(),
            },
            projected,
        };

        let explanation = explain_harness_composition_preview(&preview);
        assert_eq!(explanation.facts[0].authority, Some(SourceAuthority::Authored));
        assert_eq!(explanation.facts[1].authority, Some(SourceAuthority::Derived));
        assert_eq!(explanation.facts[2].authority, Some(SourceAuthority::Derived));
        assert!(explanation.facts[2].summary.contains("LiveMounted"));
    }
}
