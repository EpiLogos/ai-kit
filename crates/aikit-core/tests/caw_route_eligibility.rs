use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::{
    ModelAccessProfileView, ModelRosterCandidate, ModelRosterDemand, ProviderRef, ResourceRef,
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
fn credential_absence_is_ineligibility_not_route_disappearance() {
    use aikit_core::resource::*;
    let model = r("model:caw");
    let mut routes = ModelRouteSet::new(model.clone());
    routes.routes.push(ModelRoute {
        model: model.clone(),
        provider: ProviderRef::parse("provider:test").unwrap(),
        kind: ModelRouteKind::ProviderNative,
        provider_native_id: "native-caw".into(),
        endpoint: None,
        availability: RouteAvailability::Observed {
            detection_ref: "fixture:detection".into(),
        },
        credential: CredentialCondition::Required {
            hint: "missing".into(),
        },
        provenance: vec![],
    });
    let candidates = candidates_from_routes(&routes, &candidate("model:caw"));
    assert_eq!(
        candidates.len(),
        1,
        "unusable observation remains explainable"
    );
    assert!(
        !candidates[0].provider_usable,
        "missing credential cannot pass usability"
    );
    let roster = rank_model_roster(
        demand("agency:caw"),
        ModelRankingPolicy::Balanced,
        candidates,
    );
    assert!(select_model(&roster, &routes, None).is_none());
}

#[test]
fn an_eligible_route_does_not_launder_a_denied_sibling_or_override_a_pin() {
    use aikit_core::resource::*;
    let model = r("model:caw");
    let route = |variant: &str| ModelRoute {
        model: model.clone(),
        provider: ProviderRef::parse("provider:test").unwrap(),
        kind: ModelRouteKind::HarnessNative,
        provider_native_id: variant.into(),
        endpoint: None,
        availability: RouteAvailability::Observed {
            detection_ref: "fixture:detection".into(),
        },
        credential: CredentialCondition::NotRequired,
        provenance: vec![],
    };
    let routes = ModelRouteSet {
        model: model.clone(),
        routes: vec![route("eligible"), route("denied")],
    };
    let mut candidates = candidates_from_routes(&routes, &candidate("model:caw"));
    candidates[1].authorised = false;
    let roster = rank_model_roster(
        demand("agency:caw"),
        ModelRankingPolicy::Balanced,
        candidates,
    );
    let selection = select_model(&roster, &routes, None).unwrap();
    assert_eq!(selection.viable_routes.len(), 1);
    assert_eq!(selection.viable_routes[0].provider_native_id, "eligible");
    assert!(select_model(
        &roster,
        &routes,
        Some(&ProviderRef::parse("provider:other").unwrap())
    )
    .is_none());
}

#[test]
fn an_explicit_cost_ceiling_is_binding_under_every_ranking_policy() {
    use aikit_core::resource::*;
    let mut requested = demand("agency:caw");
    requested.cost_ceiling_usd = Some(0.05);
    let roster = rank_model_roster(
        requested,
        ModelRankingPolicy::Balanced,
        vec![candidate("model:caw")],
    );
    assert!(
        !roster.entries[0].explanation.eligible,
        "unknown price cannot satisfy an explicit ceiling"
    );
}
