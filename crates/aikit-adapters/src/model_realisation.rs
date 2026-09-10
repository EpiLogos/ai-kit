//! The actualisation boundary: a selected Model and one of its viable routes
//! becoming a real live relation.
//!
//! Everything upstream of here is AIKit's: the canonical catalogue, the route
//! availability join, selection. Everything at and past here belongs to other
//! owners, and this module only speaks to them in their own native terms:
//!
//! * **Actuation** owns actualisation. A selected `(ModelRef, route)` is
//!   expressed as an `actuation.instantiation/v1` receipt and handed to
//!   `actuation instantiation record`, which re-runs live detection and
//!   refuses to bind a harness it cannot prove is present with receipts. That
//!   refusal is the evidence gate, and it is Actuation's to enforce — AIKit
//!   never writes a bound receipt itself.
//! * **Workcell** owns material bodies. Where a route needs one that is not
//!   already there, the requirement is expressed as an ordinary opaque
//!   `ExecutionDemand` through Workcell's own CLI. Workcell is told a model
//!   subject and an engine, and it never learns what a ModelRef means.
//!
//! No new contract is introduced. `actuation.instantiation/v1` already carries
//! exactly the slots a route needs — `model_relation.engine.provider_ref` for
//! the provider, `variant_ref` for the provider-native id, `material.placement`
//! for where it runs — which is why the route survives the boundary intact
//! instead of collapsing into "a model was chosen".

use serde_json::{json, Value};

use aikit_core::resource::{ModelRoute, ModelRouteKind, ResourceRef};
use aikit_core::{AikitError, Result};

use crate::runner::CommandRunner;

pub const ACTUATION_INSTANTIATION_SCHEMA: &str = "actuation.instantiation/v1";

/// The inference contract a route speaks. A ref, not a capability claim: it
/// says which native surface the caller will be talking to.
fn inference_contract(kind: ModelRouteKind) -> &'static str {
    match kind {
        ModelRouteKind::LocalServing => "contract:local-serving-inference",
        ModelRouteKind::RouterRoute => "contract:router-inference",
        ModelRouteKind::ProviderNative => "contract:provider-native-inference",
        ModelRouteKind::HarnessNative => "contract:harness-native-dispatch",
    }
}

/// Where the route actually runs, in Actuation's own closed placement enum.
fn placement(kind: ModelRouteKind) -> &'static str {
    match kind {
        ModelRouteKind::LocalServing => "local",
        ModelRouteKind::RouterRoute | ModelRouteKind::ProviderNative => "remote",
        // A harness dispatches wherever it dispatches; claiming to know is a
        // guess, and `opaque` is the honest member of that enum.
        ModelRouteKind::HarnessNative => "opaque",
    }
}

/// What AIKit hands to Actuation to actualise.
#[derive(Debug, Clone, PartialEq)]
pub struct RealisationRequest {
    pub actuation_ref: String,
    pub agency_ref: String,
    pub world_binding_ref: String,
    pub agent_session_ref: Option<String>,
    /// The harness this runs inside, when one is bound. Actuation's evidence
    /// gate applies to it: an unprovable harness refuses the instantiation.
    pub harness_ref: Option<String>,
    pub model: ResourceRef,
    pub route: ModelRoute,
    /// Where the route's availability came from, carried so the receipt can be
    /// traced back to the evidence that justified it.
    pub evidence_refs: Vec<String>,
}

/// Compose the instantiation receipt for one selected model-bearing route.
///
/// The Model is named canonically (`model:<stable-id>`); the provider-native
/// id rides as `variant_ref`, where it belongs — route metadata, not identity.
pub fn instantiation_receipt(request: &RealisationRequest) -> Result<Value> {
    if !request.route.is_viable() {
        return Err(AikitError::new(
            "model_realisation.route_not_viable",
            format!(
                "route {} via {} has not been observed; an unproven route is not actualised",
                request.route.provider_native_id, request.route.provider
            ),
        ));
    }
    let mut engine = json!({ "provider_ref": request.route.provider.to_string() });
    if let Some(endpoint) = &request.route.endpoint {
        engine["facts"] = json!({ "endpoint": endpoint });
    }
    let mut evidence: Vec<String> = request.evidence_refs.clone();
    evidence.extend(request.route.provenance.iter().cloned());
    evidence.sort();
    evidence.dedup();

    let mut receipt = json!({
        "schema": ACTUATION_INSTANTIATION_SCHEMA,
        "actuation_ref": request.actuation_ref,
        "agency_ref": request.agency_ref,
        "world_binding_ref": request.world_binding_ref,
        "model_relation": {
            "schema": ACTUATION_INSTANTIATION_SCHEMA,
            "model_ref": request.model.to_string(),
            "variant_ref": request.route.provider_native_id,
            "engine": engine,
            "material": { "placement": placement(request.route.kind) },
            "inference_surface": { "contract_ref": inference_contract(request.route.kind) },
        },
        // The access this instantiation asserts is inference against exactly
        // the model that was selected. Control and interior access are not
        // granted by choosing a model, so they stay empty rather than
        // inheriting anything.
        "access_profile": {
            "schema": ACTUATION_INSTANTIATION_SCHEMA,
            "inference": { "allowed": [request.model.to_string()], "denied": [] },
            "control": { "allowed": [], "denied": [] },
            "interior": { "depth": "opaque" },
        },
        "evidence_refs": evidence,
    });
    if let Some(harness) = &request.harness_ref {
        receipt["harness_ref"] = json!(harness);
    }
    if let Some(session) = &request.agent_session_ref {
        receipt["agent_session_ref"] = json!(session);
    }
    Ok(receipt)
}

/// What actualisation yielded.
#[derive(Debug, Clone, PartialEq)]
pub enum RealisationOutcome {
    /// Actuation bound the relation and stamped its detection evidence.
    Instantiated {
        receipt: Box<Value>,
        detection_ref: Option<String>,
    },
    /// Actuation refused: the evidence gate did not pass. A refusal is a
    /// result, not a failure of this seam.
    Refused { reason: String },
    /// Actuation could not be reached or answered unusably.
    Unavailable { reason: String },
}

/// Hand a composed receipt to Actuation for evidence-gated actualisation.
///
/// Actuation re-runs live detection itself; nothing AIKit believes about
/// presence is taken on trust here. The bound receipt that comes back carries
/// `detection_ref` and `harness_receipts` stamped by that run.
pub fn realise(
    runner: &dyn CommandRunner,
    actuation_bin: &str,
    request: &RealisationRequest,
) -> RealisationOutcome {
    let receipt = match instantiation_receipt(request) {
        Ok(receipt) => receipt,
        Err(error) => {
            return RealisationOutcome::Refused {
                reason: error.to_string(),
            }
        }
    };
    let file = match tempfile::Builder::new()
        .prefix("aikit-instantiation-")
        .suffix(".json")
        .tempfile()
    {
        Ok(file) => file,
        Err(error) => {
            return RealisationOutcome::Unavailable {
                reason: format!("could not stage the instantiation receipt: {error}"),
            }
        }
    };
    if let Err(error) = std::fs::write(file.path(), receipt.to_string()) {
        return RealisationOutcome::Unavailable {
            reason: format!("could not write the instantiation receipt: {error}"),
        };
    }
    let mut argv = vec![
        actuation_bin.to_string(),
        "instantiation".to_string(),
        "record".to_string(),
    ];
    if request.harness_ref.is_none() {
        // No harness was bound, so there is nothing for the evidence gate to
        // prove. Actuation records that honestly as unattributed rather than
        // pretending a harness was involved.
        argv.push("--allow-unattributed".to_string());
    }
    argv.push(file.path().display().to_string());
    argv.push("--json".to_string());

    let output = match runner.run(&argv) {
        Ok(output) => output,
        Err(error) => {
            return RealisationOutcome::Unavailable {
                reason: format!("could not run {actuation_bin}: {error}"),
            }
        }
    };
    if output.status != 0 {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        return RealisationOutcome::Refused {
            reason: detail.chars().take(400).collect::<String>(),
        };
    }
    match serde_json::from_str::<Value>(&output.stdout) {
        Ok(bound) => {
            let detection_ref = bound
                .pointer("/receipt/detection_ref")
                .or_else(|| bound.get("detection_ref"))
                .and_then(Value::as_str)
                .map(str::to_string);
            RealisationOutcome::Instantiated {
                receipt: Box::new(bound),
                detection_ref,
            }
        }
        Err(error) => RealisationOutcome::Unavailable {
            reason: format!("instantiation output unparsable: {error}"),
        },
    }
}

/// Whether a material body is needed, and what Workcell said about supplying it.
#[derive(Debug, Clone, PartialEq)]
pub enum MaterialBodyOutcome {
    /// The route is already served; nothing needs materialising.
    NotRequired { reason: String },
    /// Workcell can supply the body.
    Satisfiable { plan_ref: Option<String> },
    /// Workcell ran and cannot supply it, with its own reasons.
    Unsatisfiable { omissions: Vec<String> },
    /// Workcell could not be reached or answered unusably.
    Unavailable { reason: String },
}

/// The engine name a route implies, in Workcell's own opaque vocabulary.
fn inference_engine(route: &ModelRoute) -> Option<&'static str> {
    match route.kind {
        ModelRouteKind::LocalServing => Some(match route.provider.as_str() {
            "provider:ollama" => "ollama",
            "provider:llama-cpp" => "llama.cpp",
            "provider:vllm" => "vllm",
            _ => "opaque",
        }),
        // A remote or harness-dispatched route has no local body to materialise.
        _ => None,
    }
}

/// Ask Workcell whether it can supply the material body a route needs.
///
/// Workcell is handed an ordinary `ExecutionDemand` — a model subject, a
/// variant subject, an engine, a placement, and a logical inference
/// connection. It is never told what a ModelRef means, and it never returns
/// model ontology: it answers the narrow physical question and nothing else.
pub fn material_body_plan(
    runner: &dyn CommandRunner,
    workcell_bin: &str,
    model: &ResourceRef,
    route: &ModelRoute,
) -> MaterialBodyOutcome {
    let Some(engine) = inference_engine(route) else {
        return MaterialBodyOutcome::NotRequired {
            reason: format!(
                "a {} route is served by its provider; there is no local body to materialise",
                route.kind.as_str()
            ),
        };
    };
    if route.is_viable() {
        return MaterialBodyOutcome::NotRequired {
            reason: format!(
                "{} is already serving {}; the body exists",
                route.provider, route.provider_native_id
            ),
        };
    }
    let slug = route
        .provider
        .as_str()
        .rsplit(':')
        .next()
        .unwrap_or("provider");
    let argv = vec![
        workcell_bin.to_string(),
        "--json".to_string(),
        "plan".to_string(),
        "--demand-ref".to_string(),
        format!("demand:model:{slug}"),
        "--subject".to_string(),
        format!("model={model}"),
        "--subject".to_string(),
        format!("variant={}", route.provider_native_id),
        "--connect".to_string(),
        "inference:caller-owned-service".to_string(),
        "--extension".to_string(),
        format!("inference-engine={engine}"),
        "--extension".to_string(),
        "placement=local".to_string(),
    ];
    let output = match runner.run(&argv) {
        Ok(output) => output,
        Err(error) => {
            return MaterialBodyOutcome::Unavailable {
                reason: format!("could not run {workcell_bin}: {error}"),
            }
        }
    };
    // Workcell exits non-zero on an unsatisfiable plan, and still prints one.
    // The plan is the answer; the exit status alone is not.
    let plan = output
        .stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("status").is_some());
    let Some(plan) = plan else {
        return MaterialBodyOutcome::Unavailable {
            reason: format!(
                "{workcell_bin} returned no plan ({}): {}",
                output.status,
                output.stderr.trim().chars().take(200).collect::<String>()
            ),
        };
    };
    match plan.get("status").and_then(Value::as_str) {
        Some("satisfiable") => MaterialBodyOutcome::Satisfiable {
            plan_ref: plan.get("plan_ref").and_then(Value::as_str).map(str::to_string),
        },
        Some("unsatisfiable") => MaterialBodyOutcome::Unsatisfiable {
            omissions: plan
                .get("omissions")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            format!(
                                "{}: {}",
                                item.get("requirement")
                                    .and_then(Value::as_str)
                                    .unwrap_or("requirement"),
                                item.get("reason").and_then(Value::as_str).unwrap_or("no reason")
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        other => MaterialBodyOutcome::Unavailable {
            reason: format!("unexpected plan status {other:?}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::Output;
    use aikit_core::resource::{
        CredentialCondition, ModelRoute, ProviderRef, RouteAvailability,
    };

    fn route(kind: ModelRouteKind, provider: &str, native: &str, observed: bool) -> ModelRoute {
        ModelRoute {
            model: ResourceRef::parse("model:smollm2-135m").unwrap(),
            provider: ProviderRef::parse(provider).unwrap(),
            kind,
            provider_native_id: native.into(),
            endpoint: Some("http://127.0.0.1:11434".into()),
            availability: if observed {
                RouteAvailability::Observed {
                    detection_ref: "detection:2026-09-09T00:00:00Z".into(),
                }
            } else {
                RouteAvailability::Unobserved {
                    reason: "nothing observed".into(),
                }
            },
            credential: CredentialCondition::NotRequired,
            provenance: vec!["source/aikit-model-catalogue".into()],
        }
    }

    fn request(route: ModelRoute, harness: Option<&str>) -> RealisationRequest {
        RealisationRequest {
            actuation_ref: "actuation:test".into(),
            agency_ref: "agency:test".into(),
            world_binding_ref: "binding:test".into(),
            agent_session_ref: None,
            harness_ref: harness.map(str::to_string),
            model: ResourceRef::parse("model:smollm2-135m").unwrap(),
            route,
            evidence_refs: vec!["detection:2026-09-09T00:00:00Z".into()],
        }
    }

    #[test]
    fn the_receipt_names_the_model_canonically_and_the_route_as_route_metadata() {
        let receipt = instantiation_receipt(&request(
            route(ModelRouteKind::LocalServing, "provider:ollama", "smollm2:135m", true),
            Some("harness/claude-code"),
        ))
        .unwrap();
        assert_eq!(receipt["model_relation"]["model_ref"], "model:smollm2-135m");
        assert_eq!(receipt["model_relation"]["variant_ref"], "smollm2:135m");
        assert_eq!(
            receipt["model_relation"]["engine"]["provider_ref"],
            "provider:ollama"
        );
        assert_eq!(receipt["model_relation"]["material"]["placement"], "local");
        assert_eq!(receipt["harness_ref"], "harness/claude-code");
        // Choosing a model grants inference on that model and nothing else.
        assert_eq!(
            receipt["access_profile"]["inference"]["allowed"][0],
            "model:smollm2-135m"
        );
        assert_eq!(
            receipt["access_profile"]["control"]["allowed"],
            serde_json::json!([])
        );
    }

    #[test]
    fn a_remote_route_is_placed_remote_and_a_harness_route_stays_opaque() {
        let remote = instantiation_receipt(&request(
            route(ModelRouteKind::RouterRoute, "provider:openrouter", "openai/gpt-5.4", true),
            None,
        ))
        .unwrap();
        assert_eq!(remote["model_relation"]["material"]["placement"], "remote");
        assert_eq!(
            remote["model_relation"]["inference_surface"]["contract_ref"],
            "contract:router-inference"
        );
        let harness = instantiation_receipt(&request(
            route(ModelRouteKind::HarnessNative, "provider:anthropic", "claude-opus-5", true),
            Some("harness/claude-code"),
        ))
        .unwrap();
        assert_eq!(harness["model_relation"]["material"]["placement"], "opaque");
    }

    #[test]
    fn an_unobserved_route_is_never_actualised() {
        let error = instantiation_receipt(&request(
            route(ModelRouteKind::LocalServing, "provider:ollama", "smollm2:135m", false),
            None,
        ))
        .unwrap_err();
        assert_eq!(error.code(), "model_realisation.route_not_viable");
    }

    struct Refusing;
    impl CommandRunner for Refusing {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output {
                status: 1,
                stdout: String::new(),
                stderr: "instantiation refused: harness harness/ghost is not detected with receipts"
                    .into(),
            })
        }
    }

    #[test]
    fn actuations_evidence_gate_refusal_is_a_result_not_a_crash() {
        let outcome = realise(
            &Refusing,
            "actuation",
            &request(
                route(ModelRouteKind::LocalServing, "provider:ollama", "smollm2:135m", true),
                Some("harness/ghost"),
            ),
        );
        match outcome {
            RealisationOutcome::Refused { reason } => {
                assert!(reason.contains("not detected with receipts"))
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    struct Unsatisfiable;
    impl CommandRunner for Unsatisfiable {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output {
                status: 1,
                stdout: r#"{"status":"unsatisfiable","demand_ref":"demand:model:ollama","omissions":[{"requirement":"connectivity:inference:caller-owned-service","reason":"no offer supports this material requirement"}]}
{"error":{"kind":"unsatisfied-demand"},"ok":false}"#
                    .into(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn a_body_that_already_serves_is_never_asked_of_workcell() {
        let outcome = material_body_plan(
            &Unsatisfiable,
            "workcell",
            &ResourceRef::parse("model:smollm2-135m").unwrap(),
            &route(ModelRouteKind::LocalServing, "provider:ollama", "smollm2:135m", true),
        );
        assert!(matches!(outcome, MaterialBodyOutcome::NotRequired { .. }));
    }

    #[test]
    fn a_remote_route_needs_no_material_body() {
        let outcome = material_body_plan(
            &Unsatisfiable,
            "workcell",
            &ResourceRef::parse("model:gpt-5.4").unwrap(),
            &route(ModelRouteKind::RouterRoute, "provider:openrouter", "openai/gpt-5.4", false),
        );
        assert!(matches!(outcome, MaterialBodyOutcome::NotRequired { .. }));
    }

    #[test]
    fn workcells_own_reasons_are_carried_rather_than_flattened_to_a_failure() {
        let outcome = material_body_plan(
            &Unsatisfiable,
            "workcell",
            &ResourceRef::parse("model:smollm2-135m").unwrap(),
            &route(ModelRouteKind::LocalServing, "provider:ollama", "smollm2:135m", false),
        );
        match outcome {
            MaterialBodyOutcome::Unsatisfiable { omissions } => {
                assert_eq!(omissions.len(), 1);
                assert!(omissions[0].contains("inference:caller-owned-service"));
                assert!(omissions[0].contains("no offer supports"));
            }
            other => panic!("expected an unsatisfiable plan, got {other:?}"),
        }
    }
}
