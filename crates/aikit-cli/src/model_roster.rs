//! CLI/headless projection of the same ModelRoster application object used by TUI.
//! No eligibility or ranking logic lives here.

use aikit_core::resource::ModelRoster;

pub fn model_roster_text(roster: &ModelRoster) -> String {
    aikit_tui::model_roster_matrix(roster).join("\n")
}

pub fn model_roster_json(roster: &ModelRoster) -> serde_json::Result<String> {
    serde_json::to_string_pretty(roster)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use aikit_core::resource::{
        rank_model_roster, ModelAccessProfileView, ModelRankingPolicy, ModelRosterCandidate,
        ModelRosterDemand, ProviderRef, ResourceRef,
    };

    use super::*;

    fn sample() -> ModelRoster {
        rank_model_roster(
            ModelRosterDemand {
                project: Some(ResourceRef::parse("project:factory").unwrap()),
                profile: None,
                agency: None,
                use_type: "coding".into(),
                required_capabilities: BTreeSet::from(["reasoning".into()]),
                required_modalities: BTreeSet::from(["text".into()]),
                required_tools: BTreeSet::new(),
                required_contracts: BTreeSet::new(),
                context_characteristics: BTreeSet::new(),
                independence_from: BTreeSet::new(),
                estimated_input_tokens: None,
                estimated_output_tokens: None,
                cost_ceiling_usd: None,
            },
            ModelRankingPolicy::Balanced,
            vec![ModelRosterCandidate {
                model: ResourceRef::parse("model:test").unwrap(),
                variant: "test".into(),
                provider: ProviderRef::parse("provider:test").unwrap(),
                provider_revision: None,
                available: true,
                authorised: true,
                provider_usable: true,
                policy_allowed: true,
                contract_compatible: true,
                harness_compatible: true,
                harness_composition: None,
                native_capabilities: BTreeSet::from(["reasoning".into()]),
                harness_capabilities: BTreeSet::new(),
                profile_skills: BTreeSet::new(),
                modalities: BTreeSet::from(["text".into()]),
                tool_support: BTreeSet::new(),
                contracts: BTreeSet::new(),
                task_fitness: BTreeMap::from([("coding".into(), 0.9)]),
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
                access: ModelAccessProfileView::default(),
                provenance: vec!["fixture".into()],
            }],
        )
    }

    #[test]
    fn text_and_json_expose_the_same_core_winner() {
        let roster = sample();
        let winner = roster.entries[0].model.to_string();
        assert!(model_roster_text(&roster).contains(&winner));
        let json = model_roster_json(&roster).unwrap();
        assert!(json.contains(&winner));
        assert!(json.contains("policy_score"));
        assert!(json.contains("missing_data"));
    }
}
