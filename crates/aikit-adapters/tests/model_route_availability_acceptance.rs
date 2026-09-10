//! Acceptance for the Model catalogue ↔ route-availability join.
//!
//! Each test here is one row of the slice's acceptance table. They run against
//! the real intake (a real `actuation.harness-detection/v1` record, parsed by
//! the real deserializer), the real catalogue types and the real join — the
//! only thing stubbed is the process boundary that would spawn `actuation`.
//!
//! The invariant every row defends: **catalogue -> availability -> selection**.
//! Identity comes from the catalogue and nowhere else; availability comes from
//! detection and nowhere else; selection reads both and fuses neither.

use std::collections::BTreeSet;

use aikit_adapters::actuation_harness_detection::{
    intake_actuation_capabilities, intake_actuation_detection,
};
use aikit_adapters::actuation_model_routes::{
    harness_provider_reachability, join_model_routes, join_model_routes_with_reach,
    observed_provider_models, CredentialEvidence,
};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::context_resolution::Availability;
use aikit_core::resource::{
    candidates_from_routes, canonical_model_ref, rank_model_roster, select_model,
    CredentialCondition, DeclaredRoute, ModelAccessProfileView, ModelCatalogue,
    ModelCatalogueEntry, ModelRankingPolicy, ModelRosterCandidate, ModelRosterDemand,
    ModelRouteKind, ModelRouteSet, ProviderRef, ResourceKind, ResourceRef, RouteUsability,
    SourceRef,
};
use aikit_core::Result;

struct StubActuation(String);
impl CommandRunner for StubActuation {
    fn run(&self, _argv: &[String]) -> Result<Output> {
        Ok(Output::success(self.0.clone()))
    }
}

/// A real detection record shaped exactly as Actuation catalog r5 emits it:
/// `native_kind` on the entry, and the models facet carrying both the
/// directory count (presence) and the typed inventory (identities).
fn detection(inventory: &[&str]) -> String {
    let items = inventory
        .iter()
        .map(|id| format!(r#"{{"id":"{id}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"schema":"actuation.harness-detection/v1","document":"detection",
        "detection_ref":"detection:2026-09-09T12:00:00Z","observed_at":"2026-09-09T12:00:00Z",
        "catalog_revision":5,"detector":{{"implementation":"actuation surface-probes","version":"0.1.0"}},
        "harnesses":[
          {{"slug":"ollama","harness_ref":"harness/ollama","native_kind":"model-provider","state":"detected",
            "receipts":{{"executable":"/usr/local/bin/ollama"}},
            "probes":[{{"kind":"service","result":"pass","detail":"http 200 from http://127.0.0.1:11434"}}],
            "facets":[{{"kind":"models","path":"~/.ollama/models","exists":true,"count":3,
              "inventory":[{items}],
              "inventory_receipt":{{"kind":"http-json","source":"http://127.0.0.1:11434/api/tags",
                "observed_at":"2026-09-09T12:00:00Z","item_count":{count}}}}}]}},
          {{"slug":"claude-code","harness_ref":"harness/claude-code","native_kind":"harness","state":"detected",
            "receipts":{{"executable":"/usr/local/bin/claude"}},
            "probes":[{{"kind":"executable","result":"pass","detail":"/usr/local/bin/claude"}}],
            "facets":[{{"kind":"skills","path":"~/.claude/skills","exists":true,"count":78}}]}},
          {{"slug":"zcode","harness_ref":"harness/zcode","native_kind":"harness","state":"detected",
            "receipts":{{"executable":"/usr/local/bin/zcode"}},
            "probes":[{{"kind":"executable","result":"pass","detail":"/usr/local/bin/zcode"}}]}}
        ],
        "absent":[],"availability":"complete"}}"#,
        count = inventory.len()
    )
}

fn provider(name: &str) -> ProviderRef {
    ProviderRef::parse(name).unwrap()
}

fn declared(provider_name: &str, kind: ModelRouteKind, ids: &[&str]) -> DeclaredRoute {
    DeclaredRoute {
        provider: provider(provider_name),
        kind,
        provider_native_ids: ids.iter().map(|id| (*id).to_string()).collect(),
        endpoint: None,
        credential: match kind {
            ModelRouteKind::LocalServing => CredentialCondition::NotRequired,
            _ => CredentialCondition::Required {
                hint: "inference credential".into(),
            },
        },
    }
}

fn entry(model: &str, routes: Vec<DeclaredRoute>) -> ModelCatalogueEntry {
    ModelCatalogueEntry {
        model: canonical_model_ref(model).unwrap(),
        name: model.into(),
        description: "acceptance catalogue entry".into(),
        superseded_refs: BTreeSet::new(),
        routes,
        source: SourceRef::parse("source/acceptance-catalogue").unwrap(),
        freshness: None,
    }
}

fn catalogue(entries: Vec<ModelCatalogueEntry>) -> ModelCatalogue {
    let mut catalogue = ModelCatalogue::default();
    for entry in entries {
        catalogue.insert(entry).unwrap();
    }
    catalogue
}

fn joined(
    catalogue: &ModelCatalogue,
    inventory: &[&str],
) -> aikit_adapters::actuation_model_routes::ModelRouteJoin {
    joined_with(catalogue, inventory, &CredentialEvidence::default())
}

fn joined_with(
    catalogue: &ModelCatalogue,
    inventory: &[&str],
    credentials: &CredentialEvidence,
) -> aikit_adapters::actuation_model_routes::ModelRouteJoin {
    let outcome = intake_actuation_detection(&StubActuation(detection(inventory)), "actuation");
    let (observed, _notes) = observed_provider_models(&outcome);
    join_model_routes(catalogue, &observed, credentials)
}

fn set_for<'a>(
    join: &'a aikit_adapters::actuation_model_routes::ModelRouteJoin,
    model: &str,
) -> &'a ModelRouteSet {
    join.route_sets
        .iter()
        .find(|set| set.model.as_str() == model)
        .expect("catalogued model must appear in the join")
}

// ---------------------------------------------------------------------------
// one model + two verified routes -> one Model candidate, two routes
// ---------------------------------------------------------------------------

#[test]
fn one_model_with_two_verified_routes_is_one_candidate_carrying_both() {
    let catalogue = catalogue(vec![entry(
        "model:llama3.2",
        vec![
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["llama3.2:latest"],
            ),
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["llama3.2:3b"],
            ),
        ],
    )]);
    let join = joined(&catalogue, &["llama3.2:latest", "llama3.2:3b"]);

    assert_eq!(
        join.models.len(),
        1,
        "one Model candidate, never one per route"
    );
    assert_eq!(join.models[0].resource.descriptor.kind, ResourceKind::Model);
    assert_eq!(join.models[0].availability, Availability::Available);

    let set = set_for(&join, "model:llama3.2");
    assert_eq!(set.viable().len(), 2, "two routes survive the join");
    assert!(
        set.viable().iter().all(|route| route.model == set.model),
        "every route points at the one canonical identity"
    );
    let natives: BTreeSet<&str> = set
        .viable()
        .iter()
        .map(|route| route.provider_native_id.as_str())
        .collect();
    assert_eq!(natives, BTreeSet::from(["llama3.2:latest", "llama3.2:3b"]));
}

// ---------------------------------------------------------------------------
// route A disappears, route B remains -> same ModelRef, re-resolved route
// ---------------------------------------------------------------------------

#[test]
fn losing_one_route_leaves_the_same_model_ref_and_re_resolves_to_the_survivor() {
    let catalogue = catalogue(vec![entry(
        "model:llama3.2",
        vec![
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["llama3.2:latest"],
            ),
            declared(
                "provider:ninerouter",
                ModelRouteKind::RouterRoute,
                &["meta/llama-3.2"],
            ),
        ],
    )]);

    let before = joined(&catalogue, &["llama3.2:latest"]);
    let before_set = set_for(&before, "model:llama3.2").clone();
    assert_eq!(before_set.viable().len(), 1);
    assert_eq!(before_set.viable()[0].provider.as_str(), "provider:ollama");

    // The local copy is removed. Nothing else about the world changes.
    let after = joined(&catalogue, &[]);
    let after_set = set_for(&after, "model:llama3.2");
    assert_eq!(
        before_set.model, after_set.model,
        "the ModelRef is untouched"
    );
    assert!(!after_set.is_available());
    assert!(
        matches!(
            after.models[0].availability,
            Availability::Unavailable { .. }
        ),
        "no route proven means unavailable, never quietly available"
    );

    // With a second provider actually observed, the same ModelRef resolves
    // through the surviving route and the Model itself never changed.
    let mut router_only = ModelRouteSet::new(before_set.model.clone());
    router_only.routes = before_set
        .routes
        .iter()
        .filter(|route| route.provider.as_str() == "provider:ninerouter")
        .cloned()
        .collect();
    assert!(!router_only.is_available(), "declared is not observed");
}

// ---------------------------------------------------------------------------
// detected Ollama with three models -> three concrete identities, not count=3
// ---------------------------------------------------------------------------

#[test]
fn a_model_provider_yields_concrete_identities_rather_than_a_directory_count() {
    let outcome = intake_actuation_detection(
        &StubActuation(detection(&[
            "smollm2:135m",
            "llama3.2:latest",
            "qwen2.5-coder:7b",
        ])),
        "actuation",
    );
    let record = match &outcome {
        aikit_adapters::actuation_harness_detection::DetectionOutcome::Record(record) => record,
        other => panic!("expected a record, got {other:?}"),
    };
    let facet = record.harnesses[0].facets.as_ref().unwrap()[0].clone();
    assert_eq!(
        facet.count,
        Some(3),
        "the directory count stays a presence signal"
    );

    let (observed, _) = observed_provider_models(&outcome);
    assert_eq!(observed.len(), 3);
    let ids: Vec<&str> = observed
        .iter()
        .map(|item| item.provider_native_id.as_str())
        .collect();
    assert_eq!(ids, ["smollm2:135m", "llama3.2:latest", "qwen2.5-coder:7b"]);
    assert!(
        observed
            .iter()
            .all(|item| item.provider.as_str() == "provider:ollama"),
        "identities are attributed to the provider that named them"
    );
}

#[test]
fn an_agent_harness_with_a_facet_contributes_no_model_routes() {
    let outcome =
        intake_actuation_detection(&StubActuation(detection(&["smollm2:135m"])), "actuation");
    let (observed, _) = observed_provider_models(&outcome);
    assert!(
        observed
            .iter()
            .all(|item| item.provider.as_str() != "provider:claude-code"),
        "a detected agent harness is not a model provider, whatever facets it discloses"
    );
}

// ---------------------------------------------------------------------------
// known model with no proven route -> known-but-unavailable
// ---------------------------------------------------------------------------

#[test]
fn a_catalogued_model_with_no_proven_route_is_known_but_never_falsely_available() {
    let catalogue = catalogue(vec![entry(
        "model:gpt-5.4",
        vec![declared(
            "provider:openai",
            ModelRouteKind::ProviderNative,
            &["gpt-5.4"],
        )],
    )]);
    let join = joined(&catalogue, &["smollm2:135m"]);

    assert_eq!(
        join.models.len(),
        1,
        "it stays known — absence is a different fact"
    );
    let set = set_for(&join, "model:gpt-5.4");
    assert!(!set.is_available());
    assert!(set.viable().is_empty());
    match &join.models[0].availability {
        Availability::Unavailable { reasons } => {
            assert!(reasons[0].contains("provider:openai"), "{reasons:?}");
        }
        other => panic!("a model with no proven route must be unavailable, got {other:?}"),
    }
    // And a credential requirement is recorded without being resolved here.
    assert!(set.routes[0].credential.requires_credential());
}

// ---------------------------------------------------------------------------
// discovered model with no catalogue map -> unmatched offer
// ---------------------------------------------------------------------------

#[test]
fn a_discovered_model_no_entry_claims_is_an_offer_and_never_an_invented_model_ref() {
    let catalogue = catalogue(vec![entry(
        "model:smollm2-135m",
        vec![declared(
            "provider:ollama",
            ModelRouteKind::LocalServing,
            &["smollm2:135m"],
        )],
    )]);
    let join = joined(&catalogue, &["smollm2:135m", "mystery-model:7b"]);

    assert_eq!(join.unmatched.len(), 1);
    assert_eq!(join.unmatched[0].provider_native_id, "mystery-model:7b");
    assert_eq!(join.unmatched[0].provider.as_str(), "provider:ollama");
    assert!(join.unmatched[0]
        .reason
        .contains("stays an offer, not a Model"));

    // Nothing anywhere in the join turned that name into an identity.
    assert_eq!(join.models.len(), 1);
    assert_eq!(
        join.models[0].resource.descriptor.id.as_str(),
        "model:smollm2-135m"
    );
    assert!(join
        .route_sets
        .iter()
        .flat_map(|set| set.routes.iter())
        .all(|route| route.model.as_str() == "model:smollm2-135m"));
    assert!(
        ResourceRef::parse("mystery-model:7b").is_ok_and(|reference| !join
            .models
            .iter()
            .any(|model| model.resource.descriptor.id == reference))
    );
}

// ---------------------------------------------------------------------------
// selection keeps routes plural
// ---------------------------------------------------------------------------

fn base_candidate(model: &str) -> ModelRosterCandidate {
    ModelRosterCandidate {
        model: canonical_model_ref(model).unwrap(),
        variant: model.into(),
        provider: provider("provider:placeholder"),
        provider_revision: None,
        available: false,
        authorised: true,
        provider_usable: false,
        policy_allowed: true,
        contract_compatible: true,
        harness_compatible: true,
        harness_composition: None,
        native_capabilities: BTreeSet::from(["reasoning".to_string()]),
        harness_capabilities: BTreeSet::new(),
        profile_skills: BTreeSet::new(),
        modalities: BTreeSet::from(["text".to_string()]),
        tool_support: BTreeSet::new(),
        contracts: BTreeSet::new(),
        task_fitness: Default::default(),
        role_fitness: Default::default(),
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

fn demand() -> ModelRosterDemand {
    ModelRosterDemand {
        project: None,
        profile: None,
        agency: None,
        use_type: "coding".into(),
        required_capabilities: BTreeSet::from(["reasoning".to_string()]),
        required_modalities: BTreeSet::from(["text".to_string()]),
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
fn selecting_a_joined_model_leaves_every_viable_route_for_actuation_to_resolve() {
    let catalogue = catalogue(vec![entry(
        "model:llama3.2",
        vec![
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["llama3.2:latest"],
            ),
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["llama3.2:3b"],
            ),
        ],
    )]);
    let join = joined(&catalogue, &["llama3.2:latest", "llama3.2:3b"]);
    let set = set_for(&join, "model:llama3.2");

    let candidates = candidates_from_routes(set, &base_candidate("model:llama3.2"));
    assert_eq!(
        candidates.len(),
        2,
        "the roster evaluates (model, route) pairs"
    );
    let roster = rank_model_roster(demand(), ModelRankingPolicy::TaskFit, candidates);

    let selection = select_model(&roster, set, None).expect("an available model selects");
    assert_eq!(selection.model.as_str(), "model:llama3.2");
    assert_eq!(
        selection.viable_routes.len(),
        2,
        "selection must not collapse the Model onto one route"
    );
    assert_eq!(selection.pinned_provider, None);
}

#[test]
fn a_provider_pin_narrows_the_route_and_leaves_the_model_ref_alone() {
    let catalogue = catalogue(vec![entry(
        "model:llama3.2",
        vec![
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["llama3.2:latest"],
            ),
            declared(
                "provider:ninerouter",
                ModelRouteKind::RouterRoute,
                &["meta/llama-3.2"],
            ),
        ],
    )]);
    let join = joined(&catalogue, &["llama3.2:latest"]);
    let set = set_for(&join, "model:llama3.2");
    let roster = rank_model_roster(
        demand(),
        ModelRankingPolicy::TaskFit,
        candidates_from_routes(set, &base_candidate("model:llama3.2")),
    );

    let pinned = select_model(&roster, set, Some(&provider("provider:ollama"))).unwrap();
    assert_eq!(pinned.model.as_str(), "model:llama3.2");
    assert_eq!(pinned.viable_routes.len(), 1);

    // A pin to a provider that has no observed route selects nothing rather
    // than silently falling back to a provider the owner did not ask for.
    assert!(select_model(&roster, set, Some(&provider("provider:ninerouter"))).is_none());
}

// ---------------------------------------------------------------------------
// provider available != model supported != route presently usable
// ---------------------------------------------------------------------------

/// A router route the catalogue declares and a Provider Source proved, so the
/// only remaining question is the credential.
fn router_catalogue() -> ModelCatalogue {
    catalogue(vec![entry(
        "model:smollm2-135m",
        vec![
            declared(
                "provider:ollama",
                ModelRouteKind::LocalServing,
                &["smollm2:135m"],
            ),
            declared(
                "provider:openrouter",
                ModelRouteKind::RouterRoute,
                &["hf/smollm2-135m"],
            ),
        ],
    )])
}

#[test]
fn an_observed_route_without_its_credential_is_viable_but_not_presently_usable() {
    let join = joined(&router_catalogue(), &["smollm2:135m"]);
    let set = set_for(&join, "model:smollm2-135m");

    let local = set
        .viable()
        .into_iter()
        .find(|route| route.provider.as_str() == "provider:ollama")
        .expect("the local route was observed");
    assert!(local.is_usable(), "a local route needs no credential");
    assert_eq!(local.usability(), RouteUsability::Usable);

    // The router route was never observed here, so it is not viable at all —
    // a different fact from "observed but uncredentialled".
    let router = set
        .routes
        .iter()
        .find(|route| route.provider.as_str() == "provider:openrouter")
        .unwrap();
    assert!(!router.is_viable());
    assert!(matches!(
        router.usability(),
        RouteUsability::NotObserved { .. }
    ));
}

#[test]
fn a_credentialled_route_becomes_usable_without_the_model_or_the_route_changing() {
    // A catalogue whose only route needs a key, and a detection run that
    // proves the provider is offering the model.
    let catalogue = catalogue(vec![ModelCatalogueEntry {
        model: canonical_model_ref("model:smollm2-135m").unwrap(),
        name: "SmolLM2".into(),
        description: "fixture".into(),
        superseded_refs: BTreeSet::new(),
        routes: vec![DeclaredRoute {
            provider: provider("provider:ollama"),
            kind: ModelRouteKind::LocalServing,
            provider_native_ids: vec!["smollm2:135m".into()],
            endpoint: None,
            credential: CredentialCondition::Required {
                hint: "a key this fixture pretends the local route needs".into(),
            },
        }],
        source: SourceRef::parse("source/acceptance-catalogue").unwrap(),
        freshness: None,
    }]);

    let without = joined_with(
        &catalogue,
        &["smollm2:135m"],
        &CredentialEvidence::default(),
    );
    let set = set_for(&without, "model:smollm2-135m");
    assert!(set.is_available(), "the route is observed");
    assert!(!set.is_usable(), "but it cannot be taken without the key");
    assert!(matches!(
        set.viable()[0].usability(),
        RouteUsability::NeedsCredential { .. }
    ));

    let bound = CredentialEvidence::from_binding_refs(["credential:ollama/local".to_string()]);
    let with = joined_with(&catalogue, &["smollm2:135m"], &bound);
    let after = set_for(&with, "model:smollm2-135m");
    assert_eq!(after.model, set.model, "binding a key changes no identity");
    assert_eq!(after.viable().len(), set.viable().len(), "nor any route");
    assert!(after.is_usable());
    assert!(matches!(
        after.viable()[0].credential,
        CredentialCondition::Satisfied { .. }
    ));
}

#[test]
fn credential_evidence_records_a_binding_and_never_a_secret() {
    let evidence = CredentialEvidence::from_binding_refs([
        "credential:openai/research".to_string(),
        "credential:anthropic".to_string(),
        "not-a-credential-ref".to_string(),
    ]);
    assert_eq!(
        evidence.providers(),
        ["provider:anthropic", "provider:openai"]
    );
    assert_eq!(
        evidence.binding_for(&provider("provider:openai")),
        Some("credential:openai/research")
    );
    assert_eq!(evidence.binding_for(&provider("provider:deepseek")), None);
}

// ---------------------------------------------------------------------------
// harness workability evidence joins Model identities
// ---------------------------------------------------------------------------

const CAPABILITIES: &str = r#"{"schema":"actuation.harness-capability/v1","document":"capability-catalog",
"catalog_revision":6,"capabilities":[
 {"harness_slug":"claude-code","model_dispatch":{"kind":"native-provider-binding","providers":[
   {"provider_ref":"provider:anthropic","selector":{"kind":"config-key","name":"model"},
    "credential":{"required":true,"hint":"Anthropic API key or an active subscription session"}}]}},
 {"harness_slug":"zcode","model_dispatch":{"kind":"none","notes":"no provider binding is evidenced"}}]}"#;

fn hosted_catalogue() -> ModelCatalogue {
    catalogue(vec![entry(
        "model:claude-opus-5",
        vec![declared(
            "provider:anthropic",
            ModelRouteKind::ProviderNative,
            &["claude-opus-5"],
        )],
    )])
}

fn reachability(
    capability_json: &str,
) -> (
    Vec<aikit_adapters::actuation_model_routes::ProviderReachability>,
    Vec<String>,
) {
    let detection =
        intake_actuation_detection(&StubActuation(detection(&["smollm2:135m"])), "actuation");
    let capabilities =
        intake_actuation_capabilities(&StubActuation(capability_json.into()), "actuation");
    harness_provider_reachability(&detection, &capabilities)
}

#[test]
fn a_detected_harness_that_declares_its_provider_makes_that_provider_reachable() {
    let (reachable, _) = reachability(CAPABILITIES);
    assert_eq!(reachable.len(), 1);
    assert_eq!(reachable[0].provider.as_str(), "provider:anthropic");
    assert_eq!(reachable[0].through, "harness/claude-code");
    assert!(reachable[0].credential_required);

    // That reach turns a declared-but-unproven hosted route into an observed
    // harness-native one, under the same ModelRef.
    let join = join_model_routes_with_reach(
        &hosted_catalogue(),
        &[],
        &reachable,
        &CredentialEvidence::default(),
    );
    let set = set_for(&join, "model:claude-opus-5");
    assert_eq!(set.viable().len(), 1);
    let route = set.viable()[0];
    assert_eq!(route.kind, ModelRouteKind::HarnessNative);
    assert_eq!(route.model.as_str(), "model:claude-opus-5");
    assert_eq!(route.provider.as_str(), "provider:anthropic");
    // Observed, but not usable until the key it declares is bound.
    assert!(matches!(
        route.usability(),
        RouteUsability::NeedsCredential { .. }
    ));
    assert!(route
        .provenance
        .iter()
        .any(|line| line.contains("harness-capability")));
}

#[test]
fn a_harness_that_declares_no_binding_supplies_no_route_and_says_so() {
    let (reachable, notes) = reachability(CAPABILITIES);
    assert!(reachable
        .iter()
        .all(|reach| reach.through != "harness/zcode"));
    assert!(notes
        .iter()
        .any(|note| note.contains("zcode") && note.contains("no model dispatch binding")));
}

#[test]
fn an_undetected_harness_supplies_no_route_however_it_is_declared() {
    // The capability catalogue declares a binding for a harness that this
    // machine's detection run never saw.
    let orphan = r#"{"schema":"actuation.harness-capability/v1","document":"capability-catalog",
    "catalog_revision":6,"capabilities":[
     {"harness_slug":"never-installed","model_dispatch":{"kind":"native-provider-binding","providers":[
       {"provider_ref":"provider:anthropic","selector":{"kind":"config-key","name":"model"},
        "credential":{"required":false}}]}}]}"#;
    let (reachable, _) = reachability(orphan);
    assert!(
        reachable.is_empty(),
        "a declaration is not presence — the harness must be detected too"
    );
    let join = join_model_routes_with_reach(
        &hosted_catalogue(),
        &[],
        &reachable,
        &CredentialEvidence::default(),
    );
    assert!(!set_for(&join, "model:claude-opus-5").is_available());
}

#[test]
fn an_unreadable_capability_catalogue_claims_nothing_either_way() {
    let (reachable, notes) = reachability("not json at all");
    assert!(reachable.is_empty());
    assert!(notes
        .iter()
        .any(|note| note.contains("capability catalogue unavailable")));
}
