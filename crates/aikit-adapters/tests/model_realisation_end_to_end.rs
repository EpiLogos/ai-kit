//! End to end: Agent → model selection → viable route → Actuation → Workcell
//! → usage Return.
//!
//! One test walks the whole chain in its authored direction, and the rest
//! defend the distinctions the chain exists to preserve. Only the process
//! boundaries are stubbed (what `actuation` and `workcell` reply); every
//! contract, join, roster and receipt on the path is the real one.
//!
//! The direction matters as much as the steps:
//!
//! ```text
//! catalogue -> availability -> selection -> actualisation -> body -> usage
//! ```
//!
//! and usage is the *bottom* of it. The last test here holds that line: a
//! recorded model-usage observation cannot become an availability or catalogue
//! feed, because there is no input through which it could enter.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use aikit_adapters::actuation_harness_detection::intake_actuation_detection;
use aikit_adapters::actuation_model_routes::{
    join_model_routes, observed_provider_models, CredentialEvidence,
};
use aikit_adapters::model_realisation::{
    instantiation_receipt, material_body_plan, realise, MaterialBodyOutcome, RealisationOutcome,
    RealisationRequest,
};
use aikit_adapters::provider_catalog_source::{
    observed_router_routes, parse_openrouter_catalog, ProviderCatalogOutcome,
};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::resource::{
    canonical_model_ref, candidates_from_routes, catalogue_from_observations, rank_model_roster,
    select_model, CredentialCondition, DeclaredRoute, ModelCatalogue, ModelCatalogueEntry,
    ModelRankingPolicy, ModelRosterCandidate, ModelRosterDemand, ModelRouteKind, ModelRouteSet,
    ProviderRef, ResourceRef, SourceRef,
};
use aikit_core::Result;

// --- the world this test stands in ----------------------------------------

/// Actuation detection: one live local model provider offering two models, and
/// one agent harness that is present but is not a model provider.
const DETECTION: &str = r#"{"schema":"actuation.harness-detection/v1","document":"detection",
"detection_ref":"detection:2026-09-09T12:00:00Z","observed_at":"2026-09-09T12:00:00Z",
"catalog_revision":6,"detector":{"implementation":"actuation surface-probes","version":"0.1.0"},
"harnesses":[
 {"slug":"ollama","harness_ref":"harness/ollama","native_kind":"model-provider","state":"detected",
  "receipts":{"executable":"/usr/local/bin/ollama"},
  "probes":[{"kind":"service","result":"pass","detail":"http 200 from http://127.0.0.1:11434"}],
  "facets":[{"kind":"models","path":"~/.ollama/models","exists":true,"count":3,
   "inventory":[{"id":"smollm2:135m"},{"id":"unclaimed-by-anyone:7b"}],
   "inventory_receipt":{"kind":"http-json","source":"http://127.0.0.1:11434/api/tags",
    "observed_at":"2026-09-09T12:00:00Z","item_count":2}}]},
 {"slug":"claude-code","harness_ref":"harness/claude-code","native_kind":"harness","state":"detected",
  "receipts":{"executable":"/usr/local/bin/claude"},
  "probes":[{"kind":"executable","result":"pass","detail":"/usr/local/bin/claude"}]}],
"absent":[],"availability":"complete"}"#;

/// A Provider Source listing: one model published by a router, reachable at the
/// router and (declared, unproven) at its own vendor.
const LISTING: &str = r#"{"data":[
 {"id":"anthropic/claude-sonnet-5","canonical_slug":"anthropic/claude-sonnet-5-20260101",
  "name":"Anthropic: Claude Sonnet 5","context_length":200000,
  "pricing":{"prompt":"0.000003","completion":"0.000015"}}]}"#;

struct Replies(Vec<(String, Output)>, Arc<Mutex<Vec<Vec<String>>>>);
impl CommandRunner for Replies {
    fn run(&self, argv: &[String]) -> Result<Output> {
        self.1.lock().unwrap().push(argv.to_vec());
        let joined = argv.join(" ");
        for (needle, output) in &self.0 {
            if joined.contains(needle) {
                return Ok(output.clone());
            }
        }
        panic!("unstubbed command: {joined}");
    }
}

fn ok(stdout: &str) -> Output {
    Output::success(stdout)
}

fn failed(stderr: &str) -> Output {
    Output {
        status: 1,
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

/// The catalogue an owner and a Provider Source together produce. Note what is
/// *not* here: nothing derived from the detection record. Identity never comes
/// from what happens to be installed.
fn catalogue() -> ModelCatalogue {
    let mut catalogue = ModelCatalogue::default();
    catalogue
        .insert(ModelCatalogueEntry {
            model: canonical_model_ref("model:smollm2-135m").unwrap(),
            name: "SmolLM2 135M".into(),
            description: "owner entry".into(),
            superseded_refs: BTreeSet::new(),
            routes: vec![
                DeclaredRoute {
                    provider: ProviderRef::parse("provider:ollama").unwrap(),
                    kind: ModelRouteKind::LocalServing,
                    provider_native_ids: vec!["smollm2:135m".into()],
                    endpoint: Some("http://127.0.0.1:11434".into()),
                    credential: CredentialCondition::NotRequired,
                },
                DeclaredRoute {
                    provider: ProviderRef::parse("provider:llama-cpp").unwrap(),
                    kind: ModelRouteKind::LocalServing,
                    provider_native_ids: vec!["smollm2-135m.gguf".into()],
                    endpoint: None,
                    credential: CredentialCondition::NotRequired,
                },
            ],
            source: SourceRef::parse("source/owner").unwrap(),
            freshness: None,
        })
        .unwrap();
    let published =
        catalogue_from_observations(&parse_openrouter_catalog(LISTING, "2026-09-09T12:00:00Z").unwrap())
            .unwrap();
    let mut layered = published;
    layered.extend(catalogue);
    layered
}

/// Everything observed about routes, from both independent kinds of evidence.
fn observed() -> Vec<aikit_adapters::actuation_model_routes::ObservedProviderModel> {
    let detection = intake_actuation_detection(
        &Replies(vec![("detect".into(), ok(DETECTION))], Default::default()),
        "actuation",
    );
    let (mut observed, _) = observed_provider_models(&detection);
    observed.extend(observed_router_routes(&ProviderCatalogOutcome::Observed {
        observations: parse_openrouter_catalog(LISTING, "2026-09-09T12:00:00Z").unwrap(),
        source: "https://openrouter.ai/api/v1/models".into(),
        observed_at: "2026-09-09T12:00:00Z".into(),
    }));
    observed
}

fn base_candidate(model: &ResourceRef) -> ModelRosterCandidate {
    ModelRosterCandidate {
        model: model.clone(),
        variant: model.to_string(),
        provider: ProviderRef::parse("provider:unresolved").unwrap(),
        provider_revision: None,
        available: false,
        authorised: true,
        provider_usable: false,
        policy_allowed: true,
        contract_compatible: true,
        harness_compatible: true,
        harness_composition: None,
        native_capabilities: Default::default(),
        harness_capabilities: Default::default(),
        profile_skills: Default::default(),
        modalities: Default::default(),
        tool_support: Default::default(),
        contracts: Default::default(),
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
        access: Default::default(),
        provenance: Vec::new(),
    }
}

fn demand() -> ModelRosterDemand {
    ModelRosterDemand {
        project: None,
        profile: None,
        agency: None,
        use_type: "compose".into(),
        required_capabilities: Default::default(),
        required_modalities: Default::default(),
        required_tools: Default::default(),
        required_contracts: Default::default(),
        context_characteristics: Default::default(),
        independence_from: Default::default(),
        estimated_input_tokens: None,
        estimated_output_tokens: None,
        cost_ceiling_usd: None,
    }
}

fn set_for(sets: &[ModelRouteSet], model: &str) -> ModelRouteSet {
    sets.iter()
        .find(|set| set.model.as_str() == model)
        .expect("catalogued model")
        .clone()
}

/// A bound receipt as Actuation returns it: the input receipt plus the
/// evidence its own fresh detection run stamped on.
fn bound_receipt() -> String {
    r#"{"schema":"actuation.instantiation/v1","actuation_ref":"actuation:test",
    "agency_ref":"agency:test","world_binding_ref":"binding:test","harness_ref":"harness/claude-code",
    "model_relation":{"schema":"actuation.instantiation/v1","model_ref":"model:smollm2-135m",
      "variant_ref":"smollm2:135m","engine":{"provider_ref":"provider:ollama"},
      "material":{"placement":"local"},
      "inference_surface":{"contract_ref":"contract:local-serving-inference"}},
    "access_profile":{"schema":"actuation.instantiation/v1",
      "inference":{"allowed":["model:smollm2-135m"],"denied":[]},
      "control":{"allowed":[],"denied":[]},"interior":{"depth":"opaque"}},
    "detection_ref":"detection:2026-09-09T12:00:00Z",
    "harness_receipts":{"executable":"/usr/local/bin/claude","sha256":"aa"}}"#
        .to_string()
}

// --- the chain -------------------------------------------------------------

#[test]
fn agent_to_model_to_route_to_actuation_to_body_to_usage() {
    // 1. An Agent that carries no model. Selection is still possible, because
    //    the model field does not come from the Agent.
    let agent = ResourceRef::parse("agent/epilogos/oi-development").unwrap();

    // 2. Catalogue meets availability. Two independent evidence kinds, joined.
    let join = join_model_routes(&catalogue(), &observed(), &CredentialEvidence::default());
    assert_eq!(
        join.unmatched.len(),
        1,
        "a provider model no entry claims stays an offer"
    );
    assert_eq!(join.unmatched[0].provider_native_id, "unclaimed-by-anyone:7b");

    let routes = set_for(&join.route_sets, "model:smollm2-135m");
    assert_eq!(routes.routes.len(), 2, "both declared routes are described");
    assert_eq!(routes.viable().len(), 1, "one of them was actually observed");

    // 3. Selection. One Model; its viable routes survive intact.
    let roster = rank_model_roster(
        demand(),
        ModelRankingPolicy::Balanced,
        candidates_from_routes(&routes, &base_candidate(&routes.model)),
    );
    let selection = select_model(&roster, &routes, None).expect("an available model selects");
    assert_eq!(selection.model.as_str(), "model:smollm2-135m");
    assert_eq!(selection.viable_routes.len(), 1);
    assert!(selection.is_resolvable());

    // 4. The material body. The chosen route is already served, so Workcell is
    //    correctly not asked to materialise anything.
    let calls: Arc<Mutex<Vec<Vec<String>>>> = Default::default();
    let runner = Replies(
        vec![
            ("instantiation record".into(), ok(&bound_receipt())),
            ("workcell".into(), ok(r#"{"status":"satisfiable","plan_ref":"plan:x"}"#)),
        ],
        calls.clone(),
    );
    let chosen = &selection.viable_routes[0];
    let body = material_body_plan(&runner, "workcell", &selection.model, chosen);
    assert!(matches!(body, MaterialBodyOutcome::NotRequired { .. }));
    assert!(
        calls.lock().unwrap().is_empty(),
        "Workcell is not consulted for a body that already exists"
    );

    // 5. Actualisation. Actuation applies its own evidence gate and stamps the
    //    detection that justified the binding.
    let request = RealisationRequest {
        actuation_ref: "actuation:test".into(),
        agency_ref: "agency:test".into(),
        world_binding_ref: "binding:test".into(),
        agent_session_ref: None,
        harness_ref: Some("harness/claude-code".into()),
        model: selection.model.clone(),
        route: chosen.clone(),
        evidence_refs: vec!["detection:2026-09-09T12:00:00Z".into()],
    };
    let outcome = realise(&runner, "actuation", &request);
    let RealisationOutcome::Instantiated {
        receipt,
        detection_ref,
    } = outcome
    else {
        panic!("expected an instantiation, got {outcome:?}");
    };
    assert_eq!(
        detection_ref.as_deref(),
        Some("detection:2026-09-09T12:00:00Z")
    );
    assert_eq!(receipt["model_relation"]["model_ref"], "model:smollm2-135m");
    assert_eq!(receipt["model_relation"]["variant_ref"], "smollm2:135m");
    assert_eq!(
        receipt["model_relation"]["engine"]["provider_ref"],
        "provider:ollama"
    );
    assert!(
        receipt["harness_receipts"]["executable"].is_string(),
        "the live relation is named with the evidence that proved it"
    );

    // The Agent is unchanged by any of this: an Agent is not a model.
    assert_eq!(agent.as_str(), "agent/epilogos/oi-development");

    // 6. Usage returns afterward, referring back to what ran. It is the bottom
    //    of the chain, and the next test proves it stays there.
    let usage = model_usage_observation(&receipt);
    assert_eq!(usage["actuation_ref"], receipt["actuation_ref"]);
    assert_eq!(usage["model"]["ref"], "model:smollm2-135m");
    assert_eq!(usage["provenance"]["raw_evidence_refs"][0], "detection:2026-09-09T12:00:00Z");
}

/// One `actuation.model-usage/v1`-shaped observation, as a caller composes it
/// after an invocation completes.
fn model_usage_observation(receipt: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "schema": "actuation.model-usage/v1",
        "usage_ref": "usage:end-to-end-1",
        "actuation_ref": receipt["actuation_ref"],
        "invocation_ref": "invocation:end-to-end-1",
        "correlation": { "harness_ref": receipt["harness_ref"] },
        "provider": { "standing": "observed", "ref": "provider:ollama" },
        "model": { "standing": "observed", "ref": "model:smollm2-135m", "variant": "smollm2:135m" },
        "tokens": { "standing": "provider-reported", "input": 12, "output": 34 },
        "cache": { "standing": "not-reported" },
        "timing": { "latency": { "standing": "observed", "milliseconds": 210 } },
        "cost": { "standing": "not-reported" },
        "outcome": { "state": "completed", "standing": "observed" },
        "provenance": {
            "reporter_ref": "reporter:aikit",
            "native_event_ref": "native:ollama-1",
            "native_schema": "ollama/generate",
            "observed_at": "2026-09-09T12:00:05Z",
            "raw_evidence_refs": ["detection:2026-09-09T12:00:00Z"],
        },
    })
}

// --- the distinctions the chain exists to preserve -------------------------

#[test]
fn usage_is_the_bottom_of_the_chain_and_can_never_feed_availability() {
    let catalogue = catalogue();
    let before = join_model_routes(&catalogue, &observed(), &CredentialEvidence::default());

    // A completed invocation produces telemetry naming a model. If usage could
    // leak upward, this is exactly where a model would gain an availability it
    // had not proven — including the unmatched offer, which usage names too.
    let usage = model_usage_observation(&serde_json::json!({
        "actuation_ref": "actuation:test", "harness_ref": "harness/claude-code"
    }));
    assert_eq!(usage["model"]["ref"], "model:smollm2-135m");

    let after = join_model_routes(&catalogue, &observed(), &CredentialEvidence::default());
    assert_eq!(
        before.route_sets, after.route_sets,
        "availability is a function of catalogue and observation only"
    );
    assert_eq!(before.unmatched, after.unmatched);
    // Structurally: the join takes a catalogue, observations and credential
    // evidence. There is no parameter through which usage could enter.
}

#[test]
fn provider_a_disappears_while_provider_b_remains() {
    // Both local providers are serving this model.
    let mut both = observed();
    both.push(aikit_adapters::actuation_model_routes::ObservedProviderModel {
        provider: ProviderRef::parse("provider:llama-cpp").unwrap(),
        kind: ModelRouteKind::LocalServing,
        provider_native_id: "smollm2-135m.gguf".into(),
        also_known_as: Vec::new(),
        endpoint: Some("http://127.0.0.1:8080".into()),
        detection_ref: "detection:2026-09-09T12:00:00Z".into(),
        inventory_source: None,
    });
    let catalogue = catalogue();
    let before = join_model_routes(&catalogue, &both, &CredentialEvidence::default());
    let before_set = set_for(&before.route_sets, "model:smollm2-135m");
    assert_eq!(before_set.viable().len(), 2, "one Model, two verified routes");

    let selection_before = {
        let roster = rank_model_roster(
            demand(),
            ModelRankingPolicy::Balanced,
            candidates_from_routes(&before_set, &base_candidate(&before_set.model)),
        );
        select_model(&roster, &before_set, None).unwrap()
    };

    // Provider A goes away. Nothing else changes.
    let remaining: Vec<_> = both
        .into_iter()
        .filter(|item| item.provider.as_str() != "provider:ollama")
        .collect();
    let after = join_model_routes(&catalogue, &remaining, &CredentialEvidence::default());
    let after_set = set_for(&after.route_sets, "model:smollm2-135m");

    let selection_after = {
        let roster = rank_model_roster(
            demand(),
            ModelRankingPolicy::Balanced,
            candidates_from_routes(&after_set, &base_candidate(&after_set.model)),
        );
        select_model(&roster, &after_set, None).unwrap()
    };

    assert_eq!(
        selection_before.model, selection_after.model,
        "same Model — provider loss is not identity loss"
    );
    assert_eq!(selection_after.viable_routes.len(), 1);
    assert_eq!(
        selection_after.viable_routes[0].provider.as_str(),
        "provider:llama-cpp",
        "the route re-resolves through the survivor"
    );
}

#[test]
fn a_route_whose_body_workcell_cannot_supply_is_skipped_with_its_reason() {
    // A declared local route nobody is serving: this is where a material body
    // would have to come from, so Workcell is asked — and answers honestly.
    let catalogue = catalogue();
    let join = join_model_routes(&catalogue, &[], &CredentialEvidence::default());
    let routes = set_for(&join.route_sets, "model:smollm2-135m");
    let unserved = routes
        .routes
        .iter()
        .find(|route| route.provider.as_str() == "provider:llama-cpp")
        .unwrap();

    let calls: Arc<Mutex<Vec<Vec<String>>>> = Default::default();
    let runner = Replies(
        vec![(
            "workcell".into(),
            Output {
                status: 1,
                stdout: r#"{"status":"unsatisfiable","omissions":[{"requirement":"connectivity:inference:caller-owned-service","reason":"no offer supports this material requirement"}]}"#.into(),
                stderr: String::new(),
            },
        )],
        calls.clone(),
    );
    let body = material_body_plan(&runner, "workcell", &routes.model, unserved);
    match body {
        MaterialBodyOutcome::Unsatisfiable { omissions } => {
            assert!(omissions[0].contains("no offer supports"));
        }
        other => panic!("expected Workcell's own reasons, got {other:?}"),
    }
    // Workcell was handed opaque subjects and an engine — never model ontology.
    let argv = calls.lock().unwrap()[0].join(" ");
    assert!(argv.contains("--subject model=model:smollm2-135m"));
    assert!(argv.contains("--extension inference-engine=llama.cpp"));
    assert!(!argv.contains("ModelRef") && !argv.contains("catalogue"));
}

#[test]
fn an_unprovable_harness_refuses_the_instantiation_rather_than_binding_it() {
    let join = join_model_routes(&catalogue(), &observed(), &CredentialEvidence::default());
    let routes = set_for(&join.route_sets, "model:smollm2-135m");
    let runner = Replies(
        vec![(
            "instantiation record".into(),
            failed("actuation: instantiation refused: harness harness/ghost is not detected with receipts"),
        )],
        Default::default(),
    );
    let outcome = realise(
        &runner,
        "actuation",
        &RealisationRequest {
            actuation_ref: "actuation:test".into(),
            agency_ref: "agency:test".into(),
            world_binding_ref: "binding:test".into(),
            agent_session_ref: None,
            harness_ref: Some("harness/ghost".into()),
            model: routes.model.clone(),
            route: routes.viable()[0].clone(),
            evidence_refs: Vec::new(),
        },
    );
    match outcome {
        RealisationOutcome::Refused { reason } => {
            assert!(reason.contains("not detected with receipts"))
        }
        other => panic!("an unprovable harness must refuse, got {other:?}"),
    }
}

#[test]
fn a_router_route_reaches_actuation_as_a_remote_relation_without_becoming_the_model() {
    let join = join_model_routes(&catalogue(), &observed(), &CredentialEvidence::default());
    let routes = set_for(&join.route_sets, "model:claude-sonnet-5");
    let router = routes.viable()[0];
    assert_eq!(router.provider.as_str(), "provider:openrouter");
    assert_eq!(router.provider_native_id, "anthropic/claude-sonnet-5");

    let receipt = instantiation_receipt(&RealisationRequest {
        actuation_ref: "actuation:test".into(),
        agency_ref: "agency:test".into(),
        world_binding_ref: "binding:test".into(),
        agent_session_ref: None,
        harness_ref: None,
        model: routes.model.clone(),
        route: router.clone(),
        evidence_refs: Vec::new(),
    })
    .unwrap();
    // The router id rides as route metadata; identity stays canonical.
    assert_eq!(receipt["model_relation"]["model_ref"], "model:claude-sonnet-5");
    assert_eq!(
        receipt["model_relation"]["variant_ref"],
        "anthropic/claude-sonnet-5"
    );
    assert_eq!(receipt["model_relation"]["material"]["placement"], "remote");
}
