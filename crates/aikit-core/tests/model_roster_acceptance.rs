use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::{
    rank_model_roster, ModelAccessProfileView, ModelRankingPolicy, ModelRosterCandidate,
    ModelRosterDemand, ProviderRef, ResourceRef,
};

fn r(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn candidate(id: &str) -> ModelRosterCandidate {
    ModelRosterCandidate {
        model: r(id),
        variant: id.into(),
        provider: ProviderRef::parse("provider:test").unwrap(),
        provider_revision: None,
        available: true,
        authorised: true,
        provider_usable: true,
        policy_allowed: true,
        contract_compatible: true,
        harness_compatible: true,
        harness_composition: Some("pi+tools".into()),
        native_capabilities: BTreeSet::from(["reasoning".into()]),
        harness_capabilities: BTreeSet::new(),
        profile_skills: BTreeSet::new(),
        modalities: BTreeSet::from(["text".into()]),
        tool_support: BTreeSet::new(),
        contracts: BTreeSet::new(),
        task_fitness: BTreeMap::from([("coding".into(), 0.8)]),
        role_fitness: BTreeMap::new(),
        profile_fit: Some(0.8),
        authored_preference: None,
        frecency: None,
        latency_ms: None,
        reliability: None,
        context_window_tokens: None,
        price: None,
        exact_spend: vec![],
        observed_fitness: vec![],
        access: ModelAccessProfileView::default(),
        provenance: vec!["acceptance-fixture".into()],
    }
}

fn demand(agency: &str) -> ModelRosterDemand {
    ModelRosterDemand {
        project: Some(r("project:factory")),
        profile: Some(r("profile:default")),
        agency: Some(r(agency)),
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
    }
}

#[test]
fn unavailable_candidate_cannot_win_even_if_role_fit_is_highest() {
    let mut unavailable = candidate("model:unavailable");
    unavailable.available = false;
    unavailable
        .role_fitness
        .insert("agency:builder".into(), 1.0);
    let mut available = candidate("model:available");
    available.role_fitness.insert("agency:builder".into(), 0.4);

    let roster = rank_model_roster(
        demand("agency:builder"),
        ModelRankingPolicy::RoleFit,
        vec![unavailable.clone(), available.clone()],
    );
    assert_eq!(roster.entries[0].model, available.model);
    let rejected = roster
        .entries
        .iter()
        .find(|entry| entry.model == unavailable.model)
        .unwrap();
    assert!(rejected
        .explanation
        .failed_gates
        .contains(&"available".to_string()));
}

#[test]
fn agency_context_can_reverse_the_same_roster_without_global_model_quality() {
    let mut architecture = candidate("model:architecture");
    architecture.role_fitness = BTreeMap::from([
        ("agency:architect".into(), 0.95),
        ("agency:reviewer".into(), 0.45),
    ]);
    let mut reviewer = candidate("model:reviewer");
    reviewer.role_fitness = BTreeMap::from([
        ("agency:architect".into(), 0.55),
        ("agency:reviewer".into(), 0.98),
    ]);

    let architect_view = rank_model_roster(
        demand("agency:architect"),
        ModelRankingPolicy::RoleFit,
        vec![architecture.clone(), reviewer.clone()],
    );
    let reviewer_view = rank_model_roster(
        demand("agency:reviewer"),
        ModelRankingPolicy::RoleFit,
        vec![architecture.clone(), reviewer.clone()],
    );

    assert_eq!(architect_view.entries[0].model, architecture.model);
    assert_eq!(reviewer_view.entries[0].model, reviewer.model);
}
