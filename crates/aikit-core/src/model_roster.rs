//! Contextual Model roster and capability-fit application read model.
//!
//! This module deliberately does not create another Model registry. Every row
//! points at the canonical `ResourceRef` for a Model and combines authored,
//! provider-observed, execution-observed and derived facts only for the current
//! demand. A `policy_score` is query-local explanation data, never Model identity.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::{ProviderRef, ResourceRef};

pub const MODEL_ROSTER_VERSION: &str = "aikit.model-roster/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRosterDemand {
    pub project: Option<ResourceRef>,
    pub profile: Option<ResourceRef>,
    pub agency: Option<ResourceRef>,
    pub use_type: String,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub required_modalities: BTreeSet<String>,
    #[serde(default)]
    pub required_tools: BTreeSet<String>,
    #[serde(default)]
    pub required_contracts: BTreeSet<String>,
    #[serde(default)]
    pub context_characteristics: BTreeSet<String>,
    #[serde(default)]
    pub independence_from: BTreeSet<ResourceRef>,
    pub estimated_input_tokens: Option<u64>,
    pub estimated_output_tokens: Option<u64>,
    pub cost_ceiling_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPriceObservation {
    pub source: String,
    pub provider: ProviderRef,
    pub model_variant: String,
    pub currency: String,
    pub unit: String,
    pub input_per_unit: Option<f64>,
    pub cached_input_per_unit: Option<f64>,
    pub output_per_unit: Option<f64>,
    pub cache_write_per_unit: Option<f64>,
    #[serde(default)]
    pub other_charges: BTreeMap<String, f64>,
    pub observed_at: String,
    pub source_revision: Option<String>,
    pub freshness_note: Option<String>,
}

impl ModelPriceObservation {
    pub fn estimate_usd(&self, demand: &ModelRosterDemand) -> Option<f64> {
        if self.currency != "USD" || self.unit != "1m-tokens" {
            return None;
        }
        let input = demand.estimated_input_tokens? as f64 / 1_000_000.0;
        let output = demand.estimated_output_tokens? as f64 / 1_000_000.0;
        Some(input * self.input_per_unit? + output * self.output_per_unit?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelAccessProfileView {
    pub inference_access: bool,
    pub control_access: bool,
    pub interior_access: bool,
    pub local_placement: bool,
    #[serde(default)]
    pub material_requirements: BTreeSet<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FitnessScope {
    pub project: Option<ResourceRef>,
    pub profile: Option<ResourceRef>,
    pub agency: Option<ResourceRef>,
    pub use_type: Option<String>,
    pub harness_composition: Option<String>,
    #[serde(default)]
    pub context_characteristics: BTreeSet<String>,
}

impl FitnessScope {
    fn applies_to(&self, demand: &ModelRosterDemand, harness: &Option<String>) -> bool {
        same_optional_ref(&self.project, &demand.project)
            && same_optional_ref(&self.profile, &demand.profile)
            && same_optional_ref(&self.agency, &demand.agency)
            && self
                .use_type
                .as_ref()
                .map(|v| v == &demand.use_type)
                .unwrap_or(true)
            && self
                .harness_composition
                .as_ref()
                .map(|v| harness.as_ref() == Some(v))
                .unwrap_or(true)
            && self
                .context_characteristics
                .iter()
                .all(|v| demand.context_characteristics.contains(v))
    }
}

fn same_optional_ref(scope: &Option<ResourceRef>, demand: &Option<ResourceRef>) -> bool {
    scope.as_ref().map(|v| Some(v) == demand.as_ref()).unwrap_or(true)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitnessObservation {
    pub score: f64,
    pub scope: FitnessScope,
    pub observed_at: String,
    pub provider_revision: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactSpendObservation {
    pub amount: f64,
    pub currency: String,
    pub execution_ref: String,
    pub observed_at: String,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRosterCandidate {
    pub model: ResourceRef,
    pub variant: String,
    pub provider: ProviderRef,
    pub provider_revision: Option<String>,
    pub available: bool,
    pub authorised: bool,
    pub provider_usable: bool,
    pub policy_allowed: bool,
    pub contract_compatible: bool,
    pub harness_compatible: bool,
    pub harness_composition: Option<String>,
    #[serde(default)]
    pub native_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub harness_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub profile_skills: BTreeSet<String>,
    #[serde(default)]
    pub modalities: BTreeSet<String>,
    #[serde(default)]
    pub tool_support: BTreeSet<String>,
    #[serde(default)]
    pub contracts: BTreeSet<String>,
    #[serde(default)]
    pub task_fitness: BTreeMap<String, f64>,
    #[serde(default)]
    pub role_fitness: BTreeMap<String, f64>,
    pub profile_fit: Option<f64>,
    pub authored_preference: Option<i32>,
    pub frecency: Option<f64>,
    pub latency_ms: Option<u64>,
    pub reliability: Option<f64>,
    pub context_window_tokens: Option<u64>,
    pub price: Option<ModelPriceObservation>,
    #[serde(default)]
    pub exact_spend: Vec<ExactSpendObservation>,
    #[serde(default)]
    pub observed_fitness: Vec<FitnessObservation>,
    pub access: ModelAccessProfileView,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ModelRosterCandidate {
    fn effective_capabilities(&self) -> BTreeSet<String> {
        self.native_capabilities
            .iter()
            .chain(self.harness_capabilities.iter())
            .chain(self.profile_skills.iter())
            .cloned()
            .collect()
    }

    fn observed_fit_for(&self, demand: &ModelRosterDemand) -> Option<f64> {
        let relevant: Vec<f64> = self
            .observed_fitness
            .iter()
            .filter(|observation| observation.scope.applies_to(demand, &self.harness_composition))
            .map(|observation| observation.score)
            .collect();
        if relevant.is_empty() {
            None
        } else {
            Some(relevant.iter().sum::<f64>() / relevant.len() as f64)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelRankingPolicy {
    CheapestEligible,
    TaskFit,
    RoleFit,
    ProfileFit,
    QualityUnderBudget,
    Balanced,
    IndependentReviewer,
    LocalInspectability,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingComponent {
    pub name: String,
    pub value: Option<f64>,
    pub weight: Option<f64>,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRankingExplanation {
    pub eligible: bool,
    pub hard_gates: Vec<String>,
    pub failed_gates: Vec<String>,
    pub components: Vec<RankingComponent>,
    pub missing_data: Vec<String>,
    pub authored_preference: Option<i32>,
    pub frecency: Option<f64>,
    pub policy_score: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub observed_fitness_used: Option<f64>,
    pub provenance: Vec<String>,
    pub why_lost_to_winner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRosterEntry {
    pub rank: Option<usize>,
    pub model: ResourceRef,
    pub variant: String,
    pub provider: ProviderRef,
    pub native_capabilities: BTreeSet<String>,
    pub harness_capabilities: BTreeSet<String>,
    pub profile_skills: BTreeSet<String>,
    pub access: ModelAccessProfileView,
    pub price: Option<ModelPriceObservation>,
    pub explanation: ModelRankingExplanation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRoster {
    pub schema_version: String,
    pub demand: ModelRosterDemand,
    pub policy: ModelRankingPolicy,
    pub entries: Vec<ModelRosterEntry>,
}

pub fn rank_model_roster(
    demand: ModelRosterDemand,
    policy: ModelRankingPolicy,
    candidates: Vec<ModelRosterCandidate>,
) -> ModelRoster {
    let mut entries: Vec<ModelRosterEntry> = candidates
        .iter()
        .map(|candidate| evaluate(&demand, policy, candidate))
        .collect();

    entries.sort_by(|a, b| match (a.explanation.eligible, b.explanation.eligible) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (true, true) => b
            .explanation
            .policy_score
            .partial_cmp(&a.explanation.policy_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.model.as_str().cmp(b.model.as_str())),
        (false, false) => a.model.as_str().cmp(b.model.as_str()),
    });

    let winner = entries
        .iter()
        .find(|entry| entry.explanation.eligible)
        .map(|entry| entry.model.clone());
    let mut rank = 0usize;
    for entry in &mut entries {
        if entry.explanation.eligible {
            rank += 1;
            entry.rank = Some(rank);
            if rank > 1 {
                entry.explanation.why_lost_to_winner = winner.as_ref().map(|winner| {
                    format!(
                        "{} ranked lower than {} under {:?}; inspect policy components and missing data",
                        entry.model, winner, policy
                    )
                });
            }
        }
    }

    ModelRoster {
        schema_version: MODEL_ROSTER_VERSION.to_string(),
        demand,
        policy,
        entries,
    }
}

fn evaluate(
    demand: &ModelRosterDemand,
    policy: ModelRankingPolicy,
    candidate: &ModelRosterCandidate,
) -> ModelRosterEntry {
    let effective_capabilities = candidate.effective_capabilities();
    let mut passed = Vec::new();
    let mut failed = Vec::new();
    gate(candidate.available, "available", &mut passed, &mut failed);
    gate(candidate.authorised, "authorised", &mut passed, &mut failed);
    gate(candidate.provider_usable, "provider-usable", &mut passed, &mut failed);
    gate(candidate.policy_allowed, "policy-allowed", &mut passed, &mut failed);
    gate(candidate.contract_compatible, "contract-compatible", &mut passed, &mut failed);
    gate(candidate.harness_compatible, "harness-compatible", &mut passed, &mut failed);
    for capability in &demand.required_capabilities {
        gate(
            effective_capabilities.contains(capability),
            &format!("capability:{capability}"),
            &mut passed,
            &mut failed,
        );
    }
    for modality in &demand.required_modalities {
        gate(
            candidate.modalities.contains(modality),
            &format!("modality:{modality}"),
            &mut passed,
            &mut failed,
        );
    }
    for tool in &demand.required_tools {
        gate(
            candidate.tool_support.contains(tool),
            &format!("tool:{tool}"),
            &mut passed,
            &mut failed,
        );
    }
    for contract in &demand.required_contracts {
        gate(
            candidate.contracts.contains(contract),
            &format!("contract:{contract}"),
            &mut passed,
            &mut failed,
        );
    }
    if policy == ModelRankingPolicy::IndependentReviewer {
        gate(
            !demand.independence_from.contains(&candidate.model),
            "independent-reviewer",
            &mut passed,
            &mut failed,
        );
    }

    let estimated_cost = candidate.price.as_ref().and_then(|p| p.estimate_usd(demand));
    if policy == ModelRankingPolicy::QualityUnderBudget {
        let budget_ok = match (demand.cost_ceiling_usd, estimated_cost) {
            (Some(ceiling), Some(cost)) => cost <= ceiling,
            _ => false,
        };
        gate(budget_ok, "known-cost-within-budget", &mut passed, &mut failed);
    }

    let task = candidate.task_fitness.get(&demand.use_type).copied();
    let role = demand
        .agency
        .as_ref()
        .and_then(|agency| candidate.role_fitness.get(agency.as_str()).copied());
    let profile = candidate.profile_fit;
    let observed = candidate.observed_fit_for(demand);
    let reliability = candidate.reliability;
    let latency = candidate.latency_ms.map(|ms| 1.0 / (1.0 + ms as f64 / 1000.0));
    let local_inspectability = Some(
        [
            candidate.access.local_placement,
            candidate.access.inference_access,
            candidate.access.control_access,
            candidate.access.interior_access,
        ]
        .iter()
        .filter(|value| **value)
        .count() as f64
            / 4.0,
    );

    let mut missing = Vec::new();
    if candidate.price.is_none() {
        missing.push("catalog-price-unknown".to_string());
    } else if estimated_cost.is_none() {
        missing.push("cost-estimate-unavailable-for-demand".to_string());
    }
    if task.is_none() {
        missing.push("task-fitness-unknown".to_string());
    }
    if observed.is_none() {
        missing.push("relevant-observed-fitness-unknown".to_string());
    }

    let mut components = vec![
        component("task-fit", task, None, &candidate.provenance),
        component("role-fit", role, None, &candidate.provenance),
        component("profile-fit", profile, None, &candidate.provenance),
        component("observed-fit", observed, None, &fitness_provenance(candidate, demand)),
        component("reliability", reliability, None, &candidate.provenance),
        component("latency", latency, None, &candidate.provenance),
        component("local-inspectability", local_inspectability, None, &candidate.access.provenance),
    ];

    let policy_score = if failed.is_empty() {
        match policy {
            ModelRankingPolicy::CheapestEligible => {
                // Unknown price is never zero/free; known lower cost ranks first.
                estimated_cost.map(|cost| 1.0 / (1.0 + cost)).or(Some(-1.0))
            }
            ModelRankingPolicy::TaskFit => task.or(observed).or(Some(0.0)),
            ModelRankingPolicy::RoleFit => role.or(Some(0.0)),
            ModelRankingPolicy::ProfileFit => profile.or(Some(0.0)),
            ModelRankingPolicy::QualityUnderBudget => blend(
                &mut components,
                &[("task-fit", 0.55), ("observed-fit", 0.30), ("reliability", 0.15)],
            ),
            ModelRankingPolicy::Balanced => blend(
                &mut components,
                &[
                    ("task-fit", 0.30),
                    ("role-fit", 0.15),
                    ("profile-fit", 0.15),
                    ("observed-fit", 0.20),
                    ("reliability", 0.10),
                    ("latency", 0.05),
                    ("local-inspectability", 0.05),
                ],
            ),
            ModelRankingPolicy::IndependentReviewer => task.or(observed).or(Some(0.0)),
            ModelRankingPolicy::LocalInspectability => local_inspectability,
        }
    } else {
        None
    };

    ModelRosterEntry {
        rank: None,
        model: candidate.model.clone(),
        variant: candidate.variant.clone(),
        provider: candidate.provider.clone(),
        native_capabilities: candidate.native_capabilities.clone(),
        harness_capabilities: candidate.harness_capabilities.clone(),
        profile_skills: candidate.profile_skills.clone(),
        access: candidate.access.clone(),
        price: candidate.price.clone(),
        explanation: ModelRankingExplanation {
            eligible: failed.is_empty(),
            hard_gates: passed,
            failed_gates: failed,
            components,
            missing_data: missing,
            authored_preference: candidate.authored_preference,
            frecency: candidate.frecency,
            policy_score,
            estimated_cost_usd: estimated_cost,
            observed_fitness_used: observed,
            provenance: candidate.provenance.clone(),
            why_lost_to_winner: None,
        },
    }
}

fn gate(ok: bool, name: &str, passed: &mut Vec<String>, failed: &mut Vec<String>) {
    if ok {
        passed.push(name.to_string());
    } else {
        failed.push(name.to_string());
    }
}

fn component(name: &str, value: Option<f64>, weight: Option<f64>, provenance: &[String]) -> RankingComponent {
    RankingComponent {
        name: name.to_string(),
        value,
        weight,
        provenance: provenance.to_vec(),
    }
}

fn fitness_provenance(candidate: &ModelRosterCandidate, demand: &ModelRosterDemand) -> Vec<String> {
    candidate
        .observed_fitness
        .iter()
        .filter(|observation| observation.scope.applies_to(demand, &candidate.harness_composition))
        .flat_map(|observation| observation.provenance.clone())
        .collect()
}

fn blend(components: &mut [RankingComponent], weights: &[(&str, f64)]) -> Option<f64> {
    let mut score = 0.0;
    let mut weight_used = 0.0;
    for (name, weight) in weights {
        if let Some(component) = components.iter_mut().find(|component| component.name == *name) {
            component.weight = Some(*weight);
            if let Some(value) = component.value {
                score += value * weight;
                weight_used += weight;
            }
        }
    }
    (weight_used > 0.0).then_some(score / weight_used)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(value: &str) -> ResourceRef { ResourceRef::parse(value).unwrap() }
    fn p(value: &str) -> ProviderRef { ProviderRef::parse(value).unwrap() }

    fn demand(use_type: &str) -> ModelRosterDemand {
        ModelRosterDemand {
            project: Some(r("project:factory")),
            profile: Some(r("profile:code")),
            agency: Some(r("agency:builder")),
            use_type: use_type.to_string(),
            required_capabilities: BTreeSet::from(["reasoning".into()]),
            required_modalities: BTreeSet::from(["text".into()]),
            required_tools: BTreeSet::new(),
            required_contracts: BTreeSet::new(),
            context_characteristics: BTreeSet::from(["rust".into()]),
            independence_from: BTreeSet::new(),
            estimated_input_tokens: Some(1_000_000),
            estimated_output_tokens: Some(100_000),
            cost_ceiling_usd: Some(20.0),
        }
    }

    fn candidate(id: &str, input: Option<f64>, output: Option<f64>, coding: f64, research: f64) -> ModelRosterCandidate {
        ModelRosterCandidate {
            model: r(id), variant: id.into(), provider: p("provider:example"), provider_revision: None,
            available: true, authorised: true, provider_usable: true, policy_allowed: true,
            contract_compatible: true, harness_compatible: true, harness_composition: Some("pi+tools/v1".into()),
            native_capabilities: BTreeSet::from(["reasoning".into(), "text".into()]),
            harness_capabilities: BTreeSet::new(), profile_skills: BTreeSet::from(["rust-method".into()]),
            modalities: BTreeSet::from(["text".into()]), tool_support: BTreeSet::new(), contracts: BTreeSet::new(),
            task_fitness: BTreeMap::from([("coding".into(), coding), ("research".into(), research)]),
            role_fitness: BTreeMap::from([("agency:builder".into(), coding)]), profile_fit: Some(coding),
            authored_preference: None, frecency: None, latency_ms: Some(500), reliability: Some(0.99),
            context_window_tokens: Some(1_000_000),
            price: input.zip(output).map(|(i,o)| ModelPriceObservation {
                source:"provider-price-page".into(), provider:p("provider:example"), model_variant:id.into(), currency:"USD".into(), unit:"1m-tokens".into(),
                input_per_unit:Some(i), cached_input_per_unit:None, output_per_unit:Some(o), cache_write_per_unit:None,
                other_charges:BTreeMap::new(), observed_at:"2026-08-17T09:30:00+01:00".into(), source_revision:None, freshness_note:Some("point-in-time provider observation".into())
            }),
            exact_spend:Vec::new(), observed_fitness:Vec::new(), access:ModelAccessProfileView::default(), provenance:vec!["fixture".into()]
        }
    }

    #[test]
    fn cheapest_and_task_fit_can_choose_different_models() {
        let cheap = candidate("model:cheap", Some(0.2), Some(1.0), 0.55, 0.70);
        let strong = candidate("model:strong", Some(2.5), Some(15.0), 0.95, 0.80);
        assert_eq!(rank_model_roster(demand("coding"), ModelRankingPolicy::CheapestEligible, vec![cheap.clone(), strong.clone()]).entries[0].model, cheap.model);
        assert_eq!(rank_model_roster(demand("coding"), ModelRankingPolicy::TaskFit, vec![cheap, strong.clone()]).entries[0].model, strong.model);
    }

    #[test]
    fn use_type_changes_ranking() {
        let a = candidate("model:a", Some(1.0), Some(2.0), 0.95, 0.40);
        let b = candidate("model:b", Some(1.0), Some(2.0), 0.60, 0.90);
        assert_eq!(rank_model_roster(demand("coding"), ModelRankingPolicy::TaskFit, vec![a.clone(), b.clone()]).entries[0].model, a.model);
        assert_eq!(rank_model_roster(demand("research"), ModelRankingPolicy::TaskFit, vec![a, b.clone()]).entries[0].model, b.model);
    }

    #[test]
    fn profile_and_agency_are_contextual_not_global_quality() {
        let mut a = candidate("model:a", Some(1.0), Some(2.0), 0.8, 0.8);
        let mut b = candidate("model:b", Some(1.0), Some(2.0), 0.8, 0.8);
        a.profile_fit = Some(0.9); b.profile_fit = Some(0.4);
        assert_eq!(rank_model_roster(demand("coding"), ModelRankingPolicy::ProfileFit, vec![a.clone(), b.clone()]).entries[0].model, a.model);
        a.profile_fit = Some(0.2); b.profile_fit = Some(0.95);
        assert_eq!(rank_model_roster(demand("coding"), ModelRankingPolicy::ProfileFit, vec![a, b.clone()]).entries[0].model, b.model);
    }

    #[test]
    fn capability_and_availability_are_hard_gates() {
        let mut missing = candidate("model:missing", Some(0.01), Some(0.01), 1.0, 1.0);
        missing.native_capabilities.clear();
        let mut denied = candidate("model:denied", Some(0.01), Some(0.01), 1.0, 1.0);
        denied.authorised = false;
        let good = candidate("model:good", Some(10.0), Some(20.0), 0.5, 0.5);
        let roster = rank_model_roster(demand("coding"), ModelRankingPolicy::CheapestEligible, vec![missing, denied, good.clone()]);
        assert_eq!(roster.entries[0].model, good.model);
        assert!(roster.entries.iter().filter(|e| !e.explanation.eligible).count() == 2);
    }

    #[test]
    fn missing_price_is_unknown_not_free() {
        let unknown = candidate("model:unknown", None, None, 0.8, 0.8);
        let known = candidate("model:known", Some(10.0), Some(20.0), 0.8, 0.8);
        let roster = rank_model_roster(demand("coding"), ModelRankingPolicy::CheapestEligible, vec![unknown.clone(), known.clone()]);
        assert_eq!(roster.entries[0].model, known.model);
        let entry = roster.entries.iter().find(|e| e.model == unknown.model).unwrap();
        assert!(entry.explanation.missing_data.contains(&"catalog-price-unknown".into()));
        assert_eq!(entry.explanation.estimated_cost_usd, None);
    }

    #[test]
    fn observed_fitness_is_used_only_when_scope_matches() {
        let mut model = candidate("model:scoped", Some(1.0), Some(2.0), 0.4, 0.4);
        model.observed_fitness.push(FitnessObservation { score:0.99, scope:FitnessScope { use_type:Some("research".into()), ..Default::default() }, observed_at:"now".into(), provider_revision:None, provenance:vec!["run:research".into()] });
        let coding = rank_model_roster(demand("coding"), ModelRankingPolicy::Balanced, vec![model.clone()]);
        assert_eq!(coding.entries[0].explanation.observed_fitness_used, None);
        let research = rank_model_roster(demand("research"), ModelRankingPolicy::Balanced, vec![model]);
        assert_eq!(research.entries[0].explanation.observed_fitness_used, Some(0.99));
    }

    #[test]
    fn authored_preference_and_frecency_remain_separate_from_fitness() {
        let mut preferred = candidate("model:preferred", Some(1.0), Some(2.0), 0.2, 0.2);
        preferred.authored_preference = Some(100); preferred.frecency = Some(999.0);
        let fit = candidate("model:fit", Some(1.0), Some(2.0), 0.9, 0.9);
        let roster = rank_model_roster(demand("coding"), ModelRankingPolicy::TaskFit, vec![preferred.clone(), fit.clone()]);
        assert_eq!(roster.entries[0].model, fit.model);
        let p = roster.entries.iter().find(|e| e.model == preferred.model).unwrap();
        assert_eq!(p.explanation.authored_preference, Some(100));
        assert_eq!(p.explanation.frecency, Some(999.0));
    }

    #[test]
    fn provider_replacement_does_not_change_model_identity_and_access_axes_stay_visible() {
        let mut a = candidate("model:stable", Some(1.0), Some(2.0), 0.8, 0.8);
        a.provider = p("provider:a"); a.access = ModelAccessProfileView { inference_access:true, control_access:false, interior_access:true, local_placement:true, ..Default::default() };
        let mut b = a.clone(); b.provider = p("provider:b"); b.provider_revision = Some("replacement".into());
        assert_eq!(a.model, b.model);
        assert_ne!(a.provider, b.provider);
        let roster = rank_model_roster(demand("coding"), ModelRankingPolicy::LocalInspectability, vec![b]);
        assert!(roster.entries[0].access.inference_access && roster.entries[0].access.interior_access && !roster.entries[0].access.control_access);
    }

    #[test]
    fn explanation_reconstructs_winner_and_independence_is_explicit() {
        let a = candidate("model:a", Some(1.0), Some(2.0), 0.9, 0.9);
        let b = candidate("model:b", Some(1.0), Some(2.0), 0.7, 0.7);
        let mut d = demand("coding"); d.independence_from.insert(a.model.clone());
        let roster = rank_model_roster(d, ModelRankingPolicy::IndependentReviewer, vec![a.clone(), b.clone()]);
        assert_eq!(roster.entries[0].model, b.model);
        let rejected = roster.entries.iter().find(|e| e.model == a.model).unwrap();
        assert!(rejected.explanation.failed_gates.contains(&"independent-reviewer".into()));
        assert!(roster.entries[0].explanation.components.iter().any(|c| c.name == "task-fit"));
    }
}
