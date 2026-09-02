//! Human-facing matrix projection of AIKit's core ModelRoster read model.
//!
//! This module performs formatting only. Eligibility and ranking are computed in
//! `aikit-core`, so the terminal cannot create a second selection policy.

use aikit_core::resource::{ModelRoster, ModelRosterEntry};

pub fn model_roster_matrix(roster: &ModelRoster) -> Vec<String> {
    let mut lines = vec![format!(
        "MODEL ROSTER · {:?} · {}",
        roster.policy, roster.demand.use_type
    )];
    lines
        .push("RANK  MODEL  PROVIDER  ELIGIBLE  TASK  PROFILE  COST(USD)  ACCESS  WHY".to_string());
    lines.extend(roster.entries.iter().map(render_entry));
    lines
}

fn render_entry(entry: &ModelRosterEntry) -> String {
    let rank = entry
        .rank
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".into());
    let task = component(entry, "task-fit");
    let profile = component(entry, "profile-fit");
    let cost = entry
        .explanation
        .estimated_cost_usd
        .map(|v| format!("{v:.4}"))
        .unwrap_or_else(|| "UNKNOWN".into());
    let access = format!(
        "i{}c{}x{}l{}",
        bit(entry.access.inference_access),
        bit(entry.access.control_access),
        bit(entry.access.interior_access),
        bit(entry.access.local_placement)
    );
    let why = if entry.explanation.eligible {
        entry
            .explanation
            .why_lost_to_winner
            .clone()
            .unwrap_or_else(|| "winner under current demand/policy".into())
    } else {
        format!("ineligible: {}", entry.explanation.failed_gates.join(","))
    };
    format!(
        "{rank:<4}  {}  {}  {:<8}  {task:<7}  {profile:<7}  {cost:<9}  {access:<8}  {why}",
        entry.model, entry.provider, entry.explanation.eligible
    )
}

fn component(entry: &ModelRosterEntry, name: &str) -> String {
    entry
        .explanation
        .components
        .iter()
        .find(|component| component.name == name)
        .and_then(|component| component.value)
        .map(|value| format!("{value:.3}"))
        .unwrap_or_else(|| "UNKNOWN".into())
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use aikit_core::resource::{
        rank_model_roster, ModelAccessProfileView, ModelRankingPolicy, ModelRosterCandidate,
        ModelRosterDemand, ProviderRef, ResourceRef,
    };

    use super::*;

    #[test]
    fn matrix_is_projection_of_core_ranking_and_keeps_unknown_price_visible() {
        let model = ResourceRef::parse("model:local").unwrap();
        let roster = rank_model_roster(
            ModelRosterDemand {
                project: None,
                profile: None,
                agency: None,
                use_type: "review".into(),
                required_capabilities: BTreeSet::from(["reasoning".into()]),
                required_modalities: BTreeSet::from(["text".into()]),
                required_tools: BTreeSet::new(),
                required_contracts: BTreeSet::new(),
                context_characteristics: BTreeSet::new(),
                independence_from: BTreeSet::new(),
                estimated_input_tokens: Some(1000),
                estimated_output_tokens: Some(1000),
                cost_ceiling_usd: None,
            },
            ModelRankingPolicy::TaskFit,
            vec![ModelRosterCandidate {
                model: model.clone(),
                variant: "local".into(),
                provider: ProviderRef::parse("provider:local").unwrap(),
                provider_revision: None,
                available: true,
                authorised: true,
                provider_usable: true,
                policy_allowed: true,
                contract_compatible: true,
                harness_compatible: true,
                harness_composition: Some("pi-local".into()),
                native_capabilities: BTreeSet::from(["reasoning".into()]),
                harness_capabilities: BTreeSet::new(),
                profile_skills: BTreeSet::new(),
                modalities: BTreeSet::from(["text".into()]),
                tool_support: BTreeSet::new(),
                contracts: BTreeSet::new(),
                task_fitness: BTreeMap::from([("review".into(), 0.8)]),
                role_fitness: BTreeMap::new(),
                profile_fit: None,
                authored_preference: None,
                frecency: None,
                latency_ms: None,
                reliability: None,
                context_window_tokens: None,
                price: None,
                exact_spend: vec![],
                observed_fitness: vec![],
                access: ModelAccessProfileView {
                    inference_access: true,
                    control_access: true,
                    interior_access: true,
                    local_placement: true,
                    ..Default::default()
                },
                provenance: vec!["test".into()],
            }],
        );
        let matrix = model_roster_matrix(&roster).join("\n");
        assert!(matrix.contains(model.as_str()));
        assert!(matrix.contains("UNKNOWN"));
        assert!(matrix.contains("i1c1x1l1"));
    }
}
