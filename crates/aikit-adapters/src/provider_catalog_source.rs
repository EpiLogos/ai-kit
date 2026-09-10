//! Provider Sources: the catalogue's other half.
//!
//! The first-party seed in `aikit-core` knows a handful of Models. A Provider
//! Source knows what a provider or router actually publishes. This module reads
//! one — OpenRouter's public model list, which needs no credential — and turns
//! it into `aikit.provider-catalog-observation/v1` records, the schema this
//! product already carries.
//!
//! Two things this is careful about:
//!
//! * **A router is a route provider, not an identity system.** `openai/gpt-5.4`
//!   is how OpenRouter spells one offering; the canonical ModelRef is
//!   `model:gpt-5.4`. Variant suffixes (`:free`, `:batch`, `:thinking`) name
//!   routes to the same Model, not different Models — which is why one entry
//!   routinely ends up with several routes.
//! * **A listing proves the router offers it. It proves nothing about the
//!   underlying provider's own API.** The router route is therefore observed
//!   and the provider-native route stays unobserved with a reason, rather than
//!   both being quietly marked available.
//!
//! This is a catalogue source, not detection. It never asserts that any route
//! is presently usable — credentials decide that, downstream and separately.

use std::collections::BTreeMap;

use aikit_core::resource::{
    canonical_model_ref, ModelRouteKind, ProviderCatalogObservation, ProviderRef,
    PROVIDER_CATALOG_OBSERVATION_SCHEMA,
};
use aikit_core::{AikitError, Result};

use crate::actuation_model_routes::ObservedProviderModel;
use crate::runner::CommandRunner;

pub const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
pub const OPENROUTER_ENDPOINT: &str = "https://openrouter.ai/api/v1";
pub const OPENROUTER_PROVIDER: &str = "provider:openrouter";

/// What reading a Provider Source yielded. A failed read is disclosed; it is
/// never an empty catalogue read as "this router publishes nothing".
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderCatalogOutcome {
    Observed {
        observations: Vec<ProviderCatalogObservation>,
        source: String,
        observed_at: String,
    },
    Unavailable {
        reason: String,
    },
}

impl ProviderCatalogOutcome {
    pub fn observations(&self) -> &[ProviderCatalogObservation] {
        match self {
            Self::Observed { observations, .. } => observations,
            Self::Unavailable { .. } => &[],
        }
    }
}

const FRESHNESS: &str =
    "point-in-time router listing; refresh before treating as current catalogue truth";

/// Fetch OpenRouter's public model list. No credential is used or needed —
/// this is the catalogue half, and it is deliberately separable from the
/// credential half so a machine with no keys still knows what exists.
pub fn fetch_openrouter_catalog(
    runner: &dyn CommandRunner,
    observed_at: &str,
) -> ProviderCatalogOutcome {
    let argv = vec![
        "curl".to_string(),
        "-sS".to_string(),
        "--max-time".to_string(),
        "20".to_string(),
        "-f".to_string(),
        OPENROUTER_MODELS_URL.to_string(),
    ];
    let output = match runner.run(&argv) {
        Ok(output) => output,
        Err(error) => {
            return ProviderCatalogOutcome::Unavailable {
                reason: format!("could not read {OPENROUTER_MODELS_URL}: {error}"),
            };
        }
    };
    if output.status != 0 {
        return ProviderCatalogOutcome::Unavailable {
            reason: format!(
                "{OPENROUTER_MODELS_URL} returned status {}: {}",
                output.status,
                output.stderr.trim().chars().take(200).collect::<String>()
            ),
        };
    }
    match parse_openrouter_catalog(&output.stdout, observed_at) {
        Ok(observations) => ProviderCatalogOutcome::Observed {
            observations,
            source: OPENROUTER_MODELS_URL.to_string(),
            observed_at: observed_at.to_string(),
        },
        Err(error) => ProviderCatalogOutcome::Unavailable {
            reason: format!("{OPENROUTER_MODELS_URL} unparsable: {error}"),
        },
    }
}

/// Split a router id into (vendor prefix, stable slug, variant suffix).
///
/// `openai/gpt-5.4:batch` -> ("openai", "gpt-5.4", Some("batch")). The variant
/// is a route distinction, never an identity one: `:free` and `:batch` are two
/// ways to reach one Model.
fn split_router_id(id: &str) -> Option<(String, String, Option<String>)> {
    let (vendor, rest) = id.split_once('/')?;
    if vendor.trim().is_empty() || rest.trim().is_empty() {
        return None;
    }
    let (slug, variant) = match rest.split_once(':') {
        Some((slug, variant)) if !slug.is_empty() && !variant.is_empty() => {
            (slug, Some(variant.to_string()))
        }
        _ => (rest, None),
    };
    Some((vendor.to_string(), slug.to_string(), variant))
}

/// A provider ref for a router vendor prefix. Vendor prefixes are lowercase
/// slugs already; anything that is not stays out rather than being coerced.
fn vendor_provider(vendor: &str) -> Option<ProviderRef> {
    if vendor.is_empty()
        || !vendor
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }
    ProviderRef::parse(format!("provider:{vendor}")).ok()
}

fn per_1m(pricing: &serde_json::Value, key: &str) -> Option<f64> {
    pricing
        .get(key)?
        .as_str()?
        .parse::<f64>()
        .ok()
        .map(|value| value * 1_000_000.0)
        .filter(|value| *value > 0.0)
}

pub fn parse_openrouter_catalog(
    body: &str,
    observed_at: &str,
) -> Result<Vec<ProviderCatalogObservation>> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| {
        AikitError::new("provider_catalog.unparsable", error.to_string())
    })?;
    let listed_by = ProviderRef::parse(OPENROUTER_PROVIDER)?;
    let entries = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            AikitError::new(
                "provider_catalog.unexpected_shape",
                "OpenRouter listing has no `data` array",
            )
        })?;
    let mut observations = Vec::new();
    for entry in entries {
        let Some(id) = entry.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some((vendor, slug, _variant)) = split_router_id(id) else {
            continue;
        };
        let Some(provider) = vendor_provider(&vendor) else {
            continue;
        };
        let Ok(model_ref) = canonical_model_ref(format!("model:{slug}")) else {
            continue;
        };
        let architecture = entry.get("architecture");
        let strings = |node: Option<&serde_json::Value>, key: &str| -> Vec<String> {
            node.and_then(|node| node.get(key))
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut pricing_usd_per_1m = BTreeMap::new();
        if let Some(pricing) = entry.get("pricing") {
            for (from, to) in [
                ("prompt", "input"),
                ("completion", "output"),
                ("input_cache_read", "cached_input"),
                ("input_cache_write", "cache_write"),
            ] {
                if let Some(value) = per_1m(pricing, from) {
                    pricing_usd_per_1m.insert(to.to_string(), value);
                }
            }
        }
        observations.push(ProviderCatalogObservation {
            schema_version: PROVIDER_CATALOG_OBSERVATION_SCHEMA.to_string(),
            observation_kind: "provider_catalog".to_string(),
            source: OPENROUTER_MODELS_URL.to_string(),
            observed_at: observed_at.to_string(),
            provider_ref: provider,
            listed_by: listed_by.clone(),
            model_ref,
            listed_variant: id.to_string(),
            provider_native_id: slug.clone(),
            canonical_variant: entry
                .get("canonical_slug")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            name: entry
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id)
                .to_string(),
            context_window_tokens: entry
                .get("context_length")
                .and_then(serde_json::Value::as_u64),
            max_output_tokens: entry
                .get("top_provider")
                .and_then(|node| node.get("max_completion_tokens"))
                .and_then(serde_json::Value::as_u64),
            input_modalities: strings(architecture, "input_modalities"),
            output_modalities: strings(architecture, "output_modalities"),
            supported_parameters: entry
                .get("supported_parameters")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            pricing_usd_per_1m,
            freshness: FRESHNESS.to_string(),
        });
    }
    if observations.is_empty() {
        return Err(AikitError::new(
            "provider_catalog.empty",
            "OpenRouter listing parsed to no usable observations",
        ));
    }
    Ok(observations)
}

/// The routes a Provider Source itself proves.
///
/// A router listing proves the *router* offers each id. It does not prove the
/// underlying provider's own API does, so only the router route is returned
/// here; the provider-native route stays declared-but-unobserved and the join
/// says so.
pub fn observed_router_routes(outcome: &ProviderCatalogOutcome) -> Vec<ObservedProviderModel> {
    let ProviderCatalogOutcome::Observed {
        observations,
        source,
        observed_at,
    } = outcome
    else {
        return Vec::new();
    };
    let catalog_ref = format!("provider-catalog:{observed_at}");
    observations
        .iter()
        .map(|observation| ObservedProviderModel {
            provider: observation.listed_by.clone(),
            kind: ModelRouteKind::RouterRoute,
            provider_native_id: observation.listed_variant.clone(),
            also_known_as: observation
                .canonical_variant
                .clone()
                .into_iter()
                .collect(),
            endpoint: Some(OPENROUTER_ENDPOINT.to_string()),
            detection_ref: catalog_ref.clone(),
            inventory_source: Some(source.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::Output;
    use aikit_core::resource::catalogue_from_observations;

    const LISTING: &str = r#"{"data":[
      {"id":"openai/gpt-5.4","canonical_slug":"openai/gpt-5.4-20260305","name":"OpenAI: GPT-5.4",
       "context_length":1050000,"top_provider":{"max_completion_tokens":128000},
       "architecture":{"input_modalities":["text","image"],"output_modalities":["text"]},
       "pricing":{"prompt":"0.0000025","completion":"0.000015"},
       "supported_parameters":["tools","structured_outputs"]},
      {"id":"openai/gpt-5.4:batch","name":"OpenAI: GPT-5.4 (batch)","context_length":1050000,
       "pricing":{"prompt":"0.00000125","completion":"0.0000075"}},
      {"id":"anthropic/claude-sonnet-5","name":"Anthropic: Claude Sonnet 5","context_length":200000,
       "pricing":{"prompt":"0.000003","completion":"0.000015"}},
      {"id":"meta-llama/llama-3.2-3b-instruct:free","name":"Meta: Llama 3.2 3B (free)",
       "pricing":{"prompt":"0","completion":"0"}},
      {"id":"malformed-no-vendor","name":"ignored"}
    ]}"#;

    struct Listing;
    impl CommandRunner for Listing {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output::success(LISTING))
        }
    }

    struct Broken;
    impl CommandRunner for Broken {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Err(AikitError::new("runner.failed", "no network"))
        }
    }

    fn observations() -> Vec<ProviderCatalogObservation> {
        parse_openrouter_catalog(LISTING, "2026-09-09T00:00:00Z").unwrap()
    }

    #[test]
    fn a_router_id_yields_a_canonical_model_ref_never_the_router_id_itself() {
        let observed = observations();
        let gpt = observed
            .iter()
            .find(|o| o.listed_variant == "openai/gpt-5.4")
            .unwrap();
        assert_eq!(gpt.model_ref.as_str(), "model:gpt-5.4");
        assert_eq!(gpt.provider_ref.as_str(), "provider:openai");
        assert_eq!(gpt.listed_by.as_str(), "provider:openrouter");
        assert_eq!(gpt.context_window_tokens, Some(1_050_000));
        assert_eq!(gpt.pricing_usd_per_1m.get("input"), Some(&2.5));
        assert_eq!(gpt.pricing_usd_per_1m.get("output"), Some(&15.0));
    }

    #[test]
    fn a_variant_suffix_is_another_route_to_the_same_model_not_another_model() {
        let observed = observations();
        let refs: Vec<&str> = observed
            .iter()
            .filter(|o| o.listed_variant.starts_with("openai/gpt-5.4"))
            .map(|o| o.model_ref.as_str())
            .collect();
        assert_eq!(refs, ["model:gpt-5.4", "model:gpt-5.4"]);
        let catalogue = catalogue_from_observations(&observed).unwrap();
        let entry = catalogue
            .get(&canonical_model_ref("model:gpt-5.4").unwrap())
            .unwrap();
        let router = entry
            .routes
            .iter()
            .find(|route| route.kind == ModelRouteKind::RouterRoute)
            .unwrap();
        assert_eq!(
            router.provider_native_ids,
            ["openai/gpt-5.4", "openai/gpt-5.4:batch"]
        );
    }

    #[test]
    fn each_entry_declares_a_router_route_and_a_provider_native_route() {
        let catalogue = catalogue_from_observations(&observations()).unwrap();
        let entry = catalogue
            .get(&canonical_model_ref("model:claude-sonnet-5").unwrap())
            .unwrap();
        let kinds: Vec<ModelRouteKind> = entry.routes.iter().map(|route| route.kind).collect();
        assert_eq!(
            kinds,
            [ModelRouteKind::RouterRoute, ModelRouteKind::ProviderNative]
        );
        let native = entry.routes.last().unwrap();
        assert_eq!(native.provider.as_str(), "provider:anthropic");
        assert_eq!(native.provider_native_ids, ["claude-sonnet-5"]);
        assert!(native.credential.requires_credential());
    }

    #[test]
    fn only_the_router_route_is_proven_by_a_router_listing() {
        let outcome = fetch_openrouter_catalog(&Listing, "2026-09-09T00:00:00Z");
        let routes = observed_router_routes(&outcome);
        assert!(routes
            .iter()
            .all(|route| route.provider.as_str() == "provider:openrouter"));
        assert!(routes
            .iter()
            .all(|route| route.kind == ModelRouteKind::RouterRoute));
        assert!(routes
            .iter()
            .any(|route| route.provider_native_id == "openai/gpt-5.4"));
        // The listing never claims provider:openai's own API was observed.
        assert!(!routes
            .iter()
            .any(|route| route.provider.as_str() == "provider:openai"));
    }

    #[test]
    fn a_malformed_listing_row_is_skipped_rather_than_coerced_into_an_identity() {
        let observed = observations();
        assert!(observed
            .iter()
            .all(|o| o.listed_variant != "malformed-no-vendor"));
        assert_eq!(observed.len(), 4);
    }

    #[test]
    fn a_failed_read_is_disclosed_never_an_empty_catalogue() {
        let outcome = fetch_openrouter_catalog(&Broken, "2026-09-09T00:00:00Z");
        match outcome {
            ProviderCatalogOutcome::Unavailable { reason } => assert!(reason.contains("no network")),
            ProviderCatalogOutcome::Observed { .. } => panic!("a failed read must not observe"),
        }
        assert!(observed_router_routes(&ProviderCatalogOutcome::Unavailable {
            reason: "x".into()
        })
        .is_empty());
    }

    #[test]
    fn observations_carry_the_schema_this_product_already_defines() {
        for observation in observations() {
            assert_eq!(observation.schema_version, PROVIDER_CATALOG_OBSERVATION_SCHEMA);
            assert_eq!(observation.observation_kind, "provider_catalog");
            assert!(observation.source.starts_with("https://"));
            assert!(!observation.freshness.is_empty());
        }
    }
}
