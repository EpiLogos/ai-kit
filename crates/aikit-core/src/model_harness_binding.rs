//! The typed seam between a harness profile's models layer and the model
//! roster's candidate facts.
//!
//! [`ModelsLayer`] speaks harness truth (what the harness natively binds);
//! a roster candidate speaks candidate truth (`harness_compatible`,
//! `harness_capabilities`, the `harness_composition` scope string). Without
//! one translation on this side of the join, every adapter would hand-roll
//! the mapping and the conventions — the scope string, the capability names,
//! what "no gate" means — would drift per harness. This module owns those
//! conventions once, so a demand-side scope and a candidate-side scope
//! written by different hands still agree.
//!
//! The one non-obvious decision: `Ungated` — a profile whose harness declares
//! no native provider binding — yields a *passing* gate, never a refusal. A
//! missing gate is the absence of a constraint, not a failed one; refusing
//! there would let the profile's honesty become the candidate's
//! ineligibility. The disclosed reason travels in the gate's explanation
//! instead of being swallowed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::harness_profile::{ModelDispatchPosture, ModelsLayer};

/// The demand-side facts a harness profile contributes to a model roster
/// read, derived from its models layer. Exactly one determination is present,
/// mirroring the three dispatch postures: a native binding (provider +
/// selector surface), provider-plural, or no model surface with its reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessModelDemand {
    /// The provider the harness natively binds, when its dispatch posture is
    /// a native provider binding.
    pub native_provider_ref: Option<String>,
    /// The kind of selector surface the harness exposes (argv flag, config
    /// key, ACP model selector), when natively bound.
    pub selector_kind: Option<String>,
    /// The name of that selector surface as the harness spells it.
    pub selector_name: Option<String>,
    /// Whether the harness is provider-plural: several providers coexist and
    /// encounter-level selection decides.
    pub provider_plural: bool,
    /// The catalog's disclosed reason that this harness has no model surface.
    /// Carried, never swallowed: "none" without a reason is not a position.
    pub no_surface_reason: Option<String>,
}

/// The candidate-side provider gate a harness profile imposes on roster
/// candidates. Derived once from the dispatch posture so demand-side and
/// candidate-side readings cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessProviderGate {
    /// The harness binds one provider natively; only candidates served by
    /// that provider are harness-compatible.
    LimitedTo { provider_ref: String },
    /// The harness is provider-plural: any provider may serve, and
    /// encounter-level selection decides.
    Open,
    /// The profile cannot gate on provider. The catalog's reason is carried
    /// in the gate verdict, not swallowed — this is a pass with disclosure,
    /// never a refusal.
    Ungated { reason: String },
}

/// The demand-side facts one harness profile contributes to the roster.
pub fn demand(models: &ModelsLayer) -> HarnessModelDemand {
    match &models.dispatch {
        ModelDispatchPosture::NativeProviderBinding {
            provider_ref,
            selector_kind,
            selector_name,
        } => HarnessModelDemand {
            native_provider_ref: Some(provider_ref.clone()),
            selector_kind: Some(selector_kind.clone()),
            selector_name: Some(selector_name.clone()),
            provider_plural: false,
            no_surface_reason: None,
        },
        ModelDispatchPosture::ProviderPlural => HarnessModelDemand {
            native_provider_ref: None,
            selector_kind: None,
            selector_name: None,
            provider_plural: true,
            no_surface_reason: None,
        },
        ModelDispatchPosture::None { reason } => HarnessModelDemand {
            native_provider_ref: None,
            selector_kind: None,
            selector_name: None,
            provider_plural: false,
            no_surface_reason: Some(reason.clone()),
        },
    }
}

/// The provider gate one harness profile imposes on roster candidates.
pub fn provider_gate(models: &ModelsLayer) -> HarnessProviderGate {
    match &models.dispatch {
        ModelDispatchPosture::NativeProviderBinding { provider_ref, .. } => {
            HarnessProviderGate::LimitedTo {
                provider_ref: provider_ref.clone(),
            }
        }
        ModelDispatchPosture::ProviderPlural => HarnessProviderGate::Open,
        ModelDispatchPosture::None { reason } => HarnessProviderGate::Ungated {
            reason: reason.clone(),
        },
    }
}

/// Apply the profile's provider gate to one candidate's provider reference.
///
/// Returns `(harness_compatible, why)`. `LimitedTo` matches only the bound
/// provider; `Open` matches any provider, including a candidate that carries
/// no provider reference at all, because this gate constrains provider
/// identity and selection — not candidate completeness — decides; `Ungated`
/// always passes, carrying the profile's disclosed reason in `why` (asserted
/// by `an_ungated_profile_passes_candidates_and_carries_the_disclosed_reason`).
pub fn gate_candidate(
    gate: &HarnessProviderGate,
    candidate_provider_ref: Option<&str>,
) -> (bool, String) {
    match gate {
        HarnessProviderGate::LimitedTo { provider_ref } => match candidate_provider_ref {
            Some(found) if found == provider_ref => (
                true,
                format!("harness natively binds {provider_ref}; candidate provider matches"),
            ),
            Some(found) => (
                false,
                format!(
                    "harness natively binds {provider_ref}; candidate provider {found:?} does \
                     not match, so it cannot serve this harness"
                ),
            ),
            None => (
                false,
                format!(
                    "harness natively binds {provider_ref}; candidate carries no provider \
                     reference to compare against"
                ),
            ),
        },
        HarnessProviderGate::Open => (
            true,
            "harness is provider-plural; every provider may serve and encounter-level \
             selection decides"
                .to_string(),
        ),
        HarnessProviderGate::Ungated { reason } => (
            true,
            format!("profile gates no provider ({reason}); candidate passes ungated"),
        ),
    }
}

/// The canonical `harness_composition` scope string for a profile slug. The
/// demand side (roster `profile` ref) and the candidate side
/// (`harness_composition`) must spell the scope identically for fitness
/// observations to bind, so both are derived through this one convention:
/// `harness-profile/<slug>`.
pub fn fitness_scope(slug: &str) -> String {
    format!("harness-profile/{slug}")
}

/// The `harness_capabilities` disclosure map a candidate carries for this
/// harness, derived honestly from the dispatch posture. No invented
/// capabilities: the map says only which of the two dispatch shapes the
/// harness has, and never claims a shape it does not have.
pub fn capabilities(models: &ModelsLayer) -> BTreeMap<String, bool> {
    let (native_binding, provider_plural) = match &models.dispatch {
        ModelDispatchPosture::NativeProviderBinding { .. } => (true, false),
        ModelDispatchPosture::ProviderPlural => (false, true),
        ModelDispatchPosture::None { .. } => (false, false),
    };
    BTreeMap::from([
        ("native-provider-binding".to_string(), native_binding),
        ("provider-plural".to_string(), provider_plural),
    ])
}

/// The composed roster facts a context's bound harnesses contribute,
/// assembled from the harness profiles the composition binds. This is the
/// single translation the demand side (`ModelRosterDemand.profile`) and the
/// candidate side (`harness_compatible`, `harness_composition`) read, so a
/// compose site never re-decides the conventions this module owns.
///
/// The composition laws, stated once:
///
/// - A composition binding no profiled harness imposes no gate. The absence
///   of a constraint is not a failed one (the same law as [`HarnessProviderGate::Ungated`]);
///   candidates pass with `harness_compatible = true` and no scope.
/// - A composition binding exactly one profiled harness discloses its
///   `harness-profile/<slug>` scope, so fitness observations can bind, and
///   gates candidates through that profile's provider gate.
/// - A composition binding several profiled harnesses gates a candidate
///   through every bound profile (a candidate must be able to serve the
///   whole composition) and discloses no single scope, because no single
///   scope string is true of the composition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HarnessCompositionFacts {
    /// The `harness-profile/<slug>` scope of the one profiled harness this
    /// composition binds, when exactly one is bound.
    pub scope: Option<String>,
    gates: Vec<(String, HarnessProviderGate)>,
    capability_names: std::collections::BTreeSet<String>,
}

impl HarnessCompositionFacts {
    /// Assemble the facts from the models layers of the bound profiles,
    /// each named by its catalog slug.
    pub fn from_layers(layers: &[(&str, &ModelsLayer)]) -> Self {
        let mut gates = Vec::new();
        let mut capability_names = std::collections::BTreeSet::new();
        for (slug, models) in layers {
            gates.push(((*slug).to_string(), provider_gate(models)));
            for (name, supported) in capabilities(models) {
                if supported {
                    capability_names.insert(name);
                }
            }
        }
        let scope = if gates.len() == 1 {
            Some(fitness_scope(&gates[0].0))
        } else {
            None
        };
        Self {
            scope,
            gates,
            capability_names,
        }
    }

    /// Whether any profiled harness is bound. A composition binding none
    /// gates nothing and discloses nothing.
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
    }

    /// Gate one candidate provider through every bound profile's gate.
    /// Returns `(harness_compatible, why)` with the disclosed reasons of
    /// every gate verdict carried, never swallowed.
    pub fn gate(&self, candidate_provider_ref: Option<&str>) -> (bool, String) {
        if self.gates.is_empty() {
            return (
                true,
                "the composition binds no profiled harness; no harness gate applies".to_string(),
            );
        }
        let mut all = true;
        let mut whys = Vec::new();
        for (slug, gate) in &self.gates {
            let (compatible, why) = gate_candidate(gate, candidate_provider_ref);
            whys.push(format!("{slug}: {why}"));
            all &= compatible;
        }
        (all, whys.join("; "))
    }

    /// The capability names this composition carries (the union of the bound
    /// profiles' supported dispatch capabilities).
    pub fn capability_names(&self) -> std::collections::BTreeSet<String> {
        self.capability_names.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_profile::LayerPosture;
    use crate::resource::{
        ModelAccessProfileView, ModelRosterCandidate, ModelRosterDemand, ProviderRef, ResourceRef,
    };
    use std::collections::BTreeSet;

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn p(value: &str) -> ProviderRef {
        ProviderRef::parse(value).unwrap()
    }

    fn native_binding_layer(provider_ref: &str) -> ModelsLayer {
        ModelsLayer {
            posture: LayerPosture::Observed,
            dispatch: ModelDispatchPosture::NativeProviderBinding {
                provider_ref: provider_ref.to_string(),
                selector_kind: "argv-flag".to_string(),
                selector_name: "--model".to_string(),
            },
            roster_note: None,
            compatibility_note: None,
        }
    }

    fn provider_plural_layer() -> ModelsLayer {
        ModelsLayer {
            posture: LayerPosture::Observed,
            dispatch: ModelDispatchPosture::ProviderPlural,
            roster_note: None,
            compatibility_note: None,
        }
    }

    fn no_surface_layer(reason: &str) -> ModelsLayer {
        ModelsLayer {
            posture: LayerPosture::Observed,
            dispatch: ModelDispatchPosture::None {
                reason: reason.to_string(),
            },
            roster_note: None,
            compatibility_note: None,
        }
    }

    #[test]
    fn a_native_provider_binding_demand_names_the_provider_and_its_selector_surface() {
        let demand = demand(&native_binding_layer("provider:example"));
        assert_eq!(
            demand.native_provider_ref.as_deref(),
            Some("provider:example")
        );
        assert_eq!(demand.selector_kind.as_deref(), Some("argv-flag"));
        assert_eq!(demand.selector_name.as_deref(), Some("--model"));
        assert!(!demand.provider_plural);
        assert_eq!(demand.no_surface_reason, None);
    }

    #[test]
    fn a_provider_plural_harness_demands_no_single_provider_and_carries_no_reason() {
        let demand = demand(&provider_plural_layer());
        assert!(demand.provider_plural);
        assert_eq!(demand.native_provider_ref, None);
        assert_eq!(demand.selector_kind, None);
        assert_eq!(demand.selector_name, None);
        assert_eq!(demand.no_surface_reason, None);
    }

    #[test]
    fn a_profile_with_no_model_surface_discloses_the_catalog_reason() {
        let demand = demand(&no_surface_layer(
            "catalog-declared: no native provider binding",
        ));
        assert_eq!(
            demand.no_surface_reason.as_deref(),
            Some("catalog-declared: no native provider binding")
        );
        assert_eq!(demand.native_provider_ref, None);
        assert!(!demand.provider_plural);
    }

    #[test]
    fn a_limited_gate_passes_only_the_bound_provider() {
        let gate = provider_gate(&native_binding_layer("provider:example"));
        assert_eq!(
            gate,
            HarnessProviderGate::LimitedTo {
                provider_ref: "provider:example".to_string(),
            }
        );
        let (compatible, why) = gate_candidate(&gate, Some("provider:example"));
        assert!(compatible, "the bound provider must pass: {why}");
        let (compatible, why) = gate_candidate(&gate, Some("provider:other"));
        assert!(
            !compatible,
            "a foreign provider must fail the bound gate: {why}"
        );
        assert!(
            why.contains("provider:example"),
            "the refusal names the bound provider: {why}"
        );
        let (compatible, why) = gate_candidate(&gate, None);
        assert!(
            !compatible,
            "a candidate with no provider cannot match a bound gate: {why}"
        );
    }

    #[test]
    fn an_open_gate_passes_any_provider_because_selection_decides() {
        let gate = provider_gate(&provider_plural_layer());
        assert_eq!(gate, HarnessProviderGate::Open);
        for candidate in [Some("provider:a"), Some("provider:b"), None] {
            let (compatible, why) = gate_candidate(&gate, candidate);
            assert!(
                compatible,
                "a provider-plural harness gates no provider ({candidate:?}): {why}"
            );
        }
    }

    #[test]
    fn an_ungated_profile_passes_candidates_and_carries_the_disclosed_reason() {
        let reason = "catalog-declared: no native provider binding";
        let gate = provider_gate(&no_surface_layer(reason));
        assert_eq!(
            gate,
            HarnessProviderGate::Ungated {
                reason: reason.to_string(),
            }
        );
        for candidate in [Some("provider:a"), None] {
            let (compatible, why) = gate_candidate(&gate, candidate);
            assert!(
                compatible,
                "no gate is not a refusal ({candidate:?}): {why}"
            );
            assert!(
                why.contains(reason),
                "the pass must carry the disclosed reason: {why}"
            );
        }
    }

    #[test]
    fn the_harness_composition_scope_convention_is_stable_across_both_sides_of_the_join() {
        assert_eq!(fitness_scope("openclaw"), "harness-profile/openclaw");
        // The demand side spells its profile ref through the same convention,
        // so a scope written on the candidate side always parses as the
        // demand-side profile ref.
        let demand_ref = r(&fitness_scope("openclaw"));
        assert_eq!(demand_ref, r("harness-profile/openclaw"));
    }

    #[test]
    fn capabilities_disclose_the_dispatch_posture_without_inventing_entries() {
        assert_eq!(
            capabilities(&native_binding_layer("provider:example")),
            BTreeMap::from([
                ("native-provider-binding".to_string(), true),
                ("provider-plural".to_string(), false),
            ])
        );
        assert_eq!(
            capabilities(&provider_plural_layer()),
            BTreeMap::from([
                ("native-provider-binding".to_string(), false),
                ("provider-plural".to_string(), true),
            ])
        );
        assert_eq!(
            capabilities(&no_surface_layer("catalog-declared: none")),
            BTreeMap::from([
                ("native-provider-binding".to_string(), false),
                ("provider-plural".to_string(), false),
            ])
        );
    }

    #[test]
    fn a_composition_binding_no_harness_gates_nothing_and_discloses_no_scope() {
        let facts = HarnessCompositionFacts::from_layers(&[]);
        assert!(facts.is_empty());
        assert_eq!(facts.scope, None);
        let (compatible, why) = facts.gate(Some("provider:whatever"));
        assert!(compatible, "no bound harness is no failed gate: {why}");
        let (compatible, _) = facts.gate(None);
        assert!(compatible);
        assert!(facts.capability_names().is_empty());
    }

    #[test]
    fn a_single_harness_composition_discloses_its_scope_and_gates_through_it() {
        let models = native_binding_layer("provider:example");
        let facts = HarnessCompositionFacts::from_layers(&[("solo", &models)]);
        assert_eq!(facts.scope.as_deref(), Some("harness-profile/solo"));
        let (compatible, _) = facts.gate(Some("provider:example"));
        assert!(compatible);
        let (compatible, why) = facts.gate(Some("provider:other"));
        assert!(
            !compatible,
            "a foreign provider must fail the bound gate: {why}"
        );
        assert!(
            why.contains("solo"),
            "the verdict names the gating slug: {why}"
        );
        assert_eq!(
            facts.capability_names(),
            BTreeSet::from(["native-provider-binding".to_string()])
        );
    }

    #[test]
    fn a_multi_harness_composition_gates_through_every_bound_profile_and_names_no_scope() {
        let bound = native_binding_layer("provider:example");
        let plural = provider_plural_layer();
        let facts = HarnessCompositionFacts::from_layers(&[("bound", &bound), ("plural", &plural)]);
        assert_eq!(
            facts.scope, None,
            "no single scope is true of the composition"
        );
        let (compatible, _) = facts.gate(Some("provider:example"));
        assert!(compatible, "a candidate both profiles can serve passes");
        let (compatible, why) = facts.gate(Some("provider:other"));
        assert!(
            !compatible,
            "the bound profile refuses a foreign provider even when the other is plural: {why}"
        );
        assert!(
            why.contains("bound") && why.contains("plural"),
            "the verdict carries every gate's disclosure: {why}"
        );
        // Capability disclosure unions the bound profiles.
        assert_eq!(
            facts.capability_names(),
            BTreeSet::from([
                "native-provider-binding".to_string(),
                "provider-plural".to_string()
            ])
        );
    }

    fn roster_demand() -> ModelRosterDemand {
        ModelRosterDemand {
            project: None,
            profile: Some(r("harness-profile/openclaw")),
            agency: None,
            use_type: "coding".to_string(),
            required_capabilities: BTreeSet::new(),
            required_modalities: BTreeSet::new(),
            required_tools: BTreeSet::new(),
            required_contracts: BTreeSet::new(),
            context_characteristics: BTreeSet::new(),
            independence_from: BTreeSet::new(),
            estimated_input_tokens: Some(1_000),
            estimated_output_tokens: Some(1_000),
            cost_ceiling_usd: None,
        }
    }

    fn roster_candidate(
        id: &str,
        provider: &str,
        task_fit: f64,
        harness_compatible: bool,
    ) -> ModelRosterCandidate {
        ModelRosterCandidate {
            model: r(id),
            variant: id.into(),
            provider: p(provider),
            provider_revision: None,
            available: true,
            authorised: true,
            provider_usable: true,
            policy_allowed: true,
            contract_compatible: true,
            harness_compatible,
            harness_composition: Some(fitness_scope("openclaw")),
            native_capabilities: BTreeSet::new(),
            harness_capabilities: BTreeSet::new(),
            profile_skills: BTreeSet::new(),
            modalities: BTreeSet::new(),
            tool_support: BTreeSet::new(),
            contracts: BTreeSet::new(),
            task_fitness: BTreeMap::from([("coding".to_string(), task_fit)]),
            role_fitness: BTreeMap::new(),
            profile_fit: None,
            authored_preference: None,
            frecency: None,
            latency_ms: None,
            reliability: None,
            context_window_tokens: None,
            price: None,
            exact_spend: Vec::new(),
            observed_fitness: Vec::new(),
            access: ModelAccessProfileView::default(),
            provenance: Vec::new(),
        }
    }

    #[test]
    fn the_profile_gate_decides_harness_compatibility_through_roster_ranking() {
        let gate = provider_gate(&native_binding_layer("provider:example"));
        // The foreign-provider candidate is the fitter model on every other
        // axis; the gate must still decide, because harness compatibility is
        // a hard gate and task fitness is not.
        let bound = roster_candidate(
            "model:bound",
            "provider:example",
            0.4,
            gate_candidate(&gate, Some("provider:example")).0,
        );
        let foreign = roster_candidate(
            "model:foreign",
            "provider:other",
            0.95,
            gate_candidate(&gate, Some("provider:other")).0,
        );
        let roster = crate::resource::rank_model_roster(
            roster_demand(),
            crate::resource::ModelRankingPolicy::TaskFit,
            vec![bound.clone(), foreign.clone()],
        );
        assert_eq!(
            roster.entries[0].model, bound.model,
            "the gate must outrank task fitness"
        );
        let foreign_entry = roster
            .entries
            .iter()
            .find(|entry| entry.model == foreign.model)
            .unwrap();
        assert!(!foreign_entry.explanation.eligible);
        assert!(
            foreign_entry
                .explanation
                .failed_gates
                .contains(&"harness-compatible".to_string()),
            "the refusal names the hard gate: {:?}",
            foreign_entry.explanation.failed_gates
        );
        let bound_entry = roster
            .entries
            .iter()
            .find(|entry| entry.model == bound.model)
            .unwrap();
        assert!(bound_entry.explanation.eligible);
        assert!(bound_entry
            .explanation
            .hard_gates
            .contains(&"harness-compatible".to_string()));
    }

    #[test]
    fn an_ungated_profile_leaves_roster_candidates_eligible_while_disclosing_why() {
        let gate = provider_gate(&no_surface_layer(
            "catalog-declared: no native provider binding",
        ));
        let candidate = roster_candidate(
            "model:anywhere",
            "provider:anywhere",
            0.5,
            gate_candidate(&gate, Some("provider:anywhere")).0,
        );
        let roster = crate::resource::rank_model_roster(
            roster_demand(),
            crate::resource::ModelRankingPolicy::TaskFit,
            vec![candidate],
        );
        let entry = &roster.entries[0];
        assert!(
            entry.explanation.eligible,
            "ungated is not refused: {:?}",
            entry.explanation.failed_gates
        );
        assert!(entry
            .explanation
            .hard_gates
            .contains(&"harness-compatible".to_string()));
    }
}
