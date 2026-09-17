//! The Model catalogue ↔ route-availability join.
//!
//! AIKit owns the canonical Model catalogue; Actuation owns live detection and
//! route actuality. This module is the seam between them, and it joins — it
//! does not mint. A provider-native id that a catalogue entry claims becomes a
//! *route* on that entry's stable `ModelRef`. A provider-native id no entry
//! claims becomes an *unmatched offer*, visible as such until an owner or a
//! Provider Source admits or maps it. Detection never creates a Model, because
//! then Model identity would be a function of whatever happens to be installed
//! on this machine today.
//!
//! Only entries that Actuation classifies as `native_kind: "model-provider"`
//! contribute model routes. An agent harness with a `models` facet is not
//! thereby a model provider, and an entry with no `native_kind` at all is
//! unclassified — read as neither, never as a default.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::context_resolution::{Availability, ResolvedResource};
use aikit_core::resource::{
    CredentialCondition, ModelCatalogue, ModelRoute, ModelRouteKind, ModelRouteSet, ProviderOffer,
    ProviderRef, ProviderState, ResourceLocator, ResourceRecord, ResourceSource, RouteAvailability,
    SourceRef, SourceState, UnmatchedModelOffer,
};

use crate::actuation_harness_detection::{
    CapabilityOutcome, DetectionEntry, DetectionOutcome, DetectionState,
};

/// The declared native kind that makes a detected entry a source of model
/// routes. Anything else — including absence — contributes none.
pub const MODEL_PROVIDER_NATIVE_KIND: &str = "model-provider";

/// The source ref stamped on route evidence, mirroring the harness-detection
/// intake's `source/actuation-detection`.
pub const ROUTE_SOURCE: &str = "source/actuation-model-route";

/// A provider a *detected harness* can dispatch to.
///
/// This is the harness-workability half of availability, and it answers a
/// different question from a provider inventory. An inventory says "this
/// provider is offering these model ids". This says "a harness that is
/// actually installed here can reach that provider at all" — which is what
/// turns a declared provider-native route from unproven into reachable,
/// without anyone enumerating a hosted provider's catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReachability {
    pub provider: ProviderRef,
    /// The harness that supplies the reach.
    pub through: String,
    /// How that harness names a model natively.
    pub selector: String,
    pub credential_required: bool,
    pub credential_hint: Option<String>,
    pub provenance: Vec<String>,
}

/// Which providers the harnesses detected on this machine can dispatch to.
///
/// Both halves are required and neither is assumed: a harness must be
/// *detected* (Actuation's receipts law) and must *declare* a model dispatch
/// binding (its capability descriptor). A harness with no capability
/// descriptor contributes nothing — not knowing is not the same as knowing it
/// binds nothing, and a declared `kind: "none"` is the honest way to say the
/// second.
pub fn harness_provider_reachability(
    detection: &DetectionOutcome,
    capabilities: &CapabilityOutcome,
) -> (Vec<ProviderReachability>, Vec<String>) {
    let mut reachable = Vec::new();
    let mut notes = Vec::new();
    let DetectionOutcome::Record(record) = detection else {
        return (reachable, notes);
    };
    if let CapabilityOutcome::Unavailable { reason } = capabilities {
        notes.push(format!(
            "harness capability catalogue unavailable: {reason} — no harness-native route was \
             claimed either way"
        ));
        return (reachable, notes);
    }
    for entry in &record.harnesses {
        if !matches!(entry.state, DetectionState::Detected) {
            continue;
        }
        let Some(dispatch) = capabilities.dispatch_for(&entry.slug) else {
            continue;
        };
        if !dispatch.binds_providers() {
            notes.push(format!(
                "{} declares no model dispatch binding{} — it supplies no harness-native route",
                entry.slug,
                dispatch
                    .notes
                    .as_ref()
                    .map(|note| format!(" ({note})"))
                    .unwrap_or_default()
            ));
            continue;
        }
        for provider in &dispatch.providers {
            let Ok(reference) = ProviderRef::parse(&provider.provider_ref) else {
                notes.push(format!(
                    "{} declares an unaddressable provider {:?}",
                    entry.slug, provider.provider_ref
                ));
                continue;
            };
            reachable.push(ProviderReachability {
                provider: reference,
                through: entry.harness_ref.clone(),
                selector: format!("{} {}", provider.selector.kind, provider.selector.name),
                credential_required: provider.credential.required,
                credential_hint: provider.credential.hint.clone(),
                provenance: vec![
                    record.detection_ref.clone(),
                    format!(
                        "actuation.harness-capability/v1 r{}",
                        capabilities.catalog_revision().unwrap_or_default()
                    ),
                ],
            });
        }
    }
    (reachable, notes)
}

/// Which providers have a credential bound on this machine.
///
/// Credentials decide whether an observed route can be taken *today*. They are
/// never part of Model identity and never part of whether a route exists — a
/// route whose key is missing is unusable, not absent. Nothing here reads,
/// materialises or carries a secret: only the fact that a binding is recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialEvidence {
    bound: BTreeMap<String, String>,
}

impl CredentialEvidence {
    /// Build from recorded credential bindings. A binding is attributed to a
    /// provider by the vendor segment of its ref: `credential:openai/research`
    /// answers for `provider:openai`. That is a convention, not a proof of
    /// scope, so it is recorded as evidence of a binding and never as proof
    /// that the key works.
    pub fn from_binding_refs(refs: impl IntoIterator<Item = String>) -> Self {
        let mut bound = BTreeMap::new();
        for reference in refs {
            let Some(rest) = reference.strip_prefix("credential:") else {
                continue;
            };
            let vendor = rest.split(['/', ':']).next().unwrap_or_default();
            if vendor.is_empty() {
                continue;
            }
            bound
                .entry(format!("provider:{vendor}"))
                .or_insert(reference);
        }
        Self { bound }
    }

    pub fn binding_for(&self, provider: &ProviderRef) -> Option<&str> {
        self.bound.get(provider.as_str()).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.bound.is_empty()
    }

    pub fn providers(&self) -> Vec<&str> {
        self.bound.keys().map(String::as_str).collect()
    }

    /// Resolve a declared credential condition against what is actually bound.
    fn resolve(
        &self,
        provider: &ProviderRef,
        declared: &CredentialCondition,
    ) -> CredentialCondition {
        match declared {
            CredentialCondition::NotRequired => CredentialCondition::NotRequired,
            CredentialCondition::Required { hint }
            | CredentialCondition::Satisfied { hint, .. } => match self.binding_for(provider) {
                Some(binding_ref) => CredentialCondition::Satisfied {
                    hint: hint.clone(),
                    binding_ref: binding_ref.to_string(),
                },
                None => CredentialCondition::Required { hint: hint.clone() },
            },
        }
    }
}

/// What the join yielded.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRouteJoin {
    /// One `ResourceKind::Model` record per catalogued Model considered,
    /// carrying its observed routes as provider offers.
    pub models: Vec<ResolvedResource>,
    /// The typed route sets, routes plural, parallel to `models`.
    pub route_sets: Vec<ModelRouteSet>,
    /// Observed provider-native identities no catalogue entry claims.
    pub unmatched: Vec<UnmatchedModelOffer>,
    /// What an operator needs to know about how this join went.
    pub notes: Vec<String>,
}

impl ModelRouteJoin {
    pub fn available_models(&self) -> Vec<&ModelRouteSet> {
        self.route_sets
            .iter()
            .filter(|set| set.is_available())
            .collect()
    }
}

/// One provider-native model identity as detection observed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedProviderModel {
    pub provider: ProviderRef,
    pub kind: ModelRouteKind,
    pub provider_native_id: String,
    /// The provider's other spellings for the same offering, if it gave any.
    pub also_known_as: Vec<String>,
    pub endpoint: Option<String>,
    pub detection_ref: String,
    pub inventory_source: Option<String>,
}

/// Read the provider-native model identities out of one detection outcome.
///
/// A run that failed, or a provider whose inventory could not be read, yields
/// nothing here *and* a note — never an empty list read as "this provider
/// offers no models".
pub fn observed_provider_models(
    detection: &DetectionOutcome,
) -> (Vec<ObservedProviderModel>, Vec<String>) {
    let mut observed = Vec::new();
    let mut notes = Vec::new();
    let record = match detection {
        DetectionOutcome::Record(record) => record.as_ref(),
        DetectionOutcome::Unavailable { reason } => {
            notes.push(format!(
                "model-route detection unavailable: {reason} — no route availability was \
                 observed; catalogued Models stay known-but-unproven, not absent"
            ));
            return (observed, notes);
        }
    };
    for entry in &record.harnesses {
        if entry.native_kind.as_deref() != Some(MODEL_PROVIDER_NATIVE_KIND) {
            continue;
        }
        if !matches!(entry.state, DetectionState::Detected) {
            notes.push(format!(
                "model provider {} is {} — it supplies no routes in this resolution",
                entry.slug,
                state_word(entry),
            ));
            continue;
        }
        let provider = match ProviderRef::parse(format!("provider:{}", entry.slug)) {
            Ok(provider) => provider,
            Err(error) => {
                notes.push(format!(
                    "model provider {} could not be addressed as a provider ref: {error}",
                    entry.slug
                ));
                continue;
            }
        };
        let endpoint = entry.probes.as_ref().and_then(|probes| {
            probes
                .iter()
                .find(|probe| probe.kind == "service" && probe.result == "pass")
                .and_then(|probe| probe.detail.clone())
                .and_then(|detail| detail.split_once(" from ").map(|(_, url)| url.to_string()))
        });
        let mut named_any = false;
        for facet in entry.facets.iter().flatten() {
            if facet.kind != "models" {
                continue;
            }
            if let Some(reason) = &facet.inventory_unavailable_reason {
                notes.push(format!(
                    "model provider {} is present but its inventory could not be read: {reason} — \
                     no route is claimed either way",
                    entry.slug
                ));
                continue;
            }
            let Some(inventory) = &facet.inventory else {
                notes.push(format!(
                    "model provider {} discloses a models facet with no typed inventory{} — \
                     a count is not an identity, so no route can be joined from it",
                    entry.slug,
                    facet
                        .count
                        .map(|count| format!(" (count {count})"))
                        .unwrap_or_default()
                ));
                continue;
            };
            named_any = true;
            for item in inventory {
                observed.push(ObservedProviderModel {
                    provider: provider.clone(),
                    kind: ModelRouteKind::LocalServing,
                    provider_native_id: item.id.clone(),
                    also_known_as: item.also_known_as.clone().unwrap_or_default(),
                    endpoint: endpoint.clone(),
                    detection_ref: record.detection_ref.clone(),
                    inventory_source: facet
                        .inventory_receipt
                        .as_ref()
                        .map(|receipt| receipt.source.clone()),
                });
            }
        }
        if !named_any && entry.facets.is_some() {
            // Already noted above per facet; nothing more to say.
        } else if entry.facets.is_none() {
            notes.push(format!(
                "model provider {} is detected but disclosed no models facet — no route \
                 availability was observed for it",
                entry.slug
            ));
        }
    }
    (observed, notes)
}

fn state_word(entry: &DetectionEntry) -> String {
    match entry.state {
        DetectionState::Detected => "detected".into(),
        DetectionState::NotInstalled => "not installed".into(),
        DetectionState::Unavailable => format!(
            "unprovable ({})",
            entry
                .unavailable_reason
                .as_deref()
                .unwrap_or("no reason captured")
        ),
    }
}

/// Join the canonical catalogue against observed route availability.
///
/// Every catalogued Model appears in the result, whether or not a route to it
/// was proven: known-but-unavailable is a fact worth carrying, and it is not
/// the same fact as "not catalogued".
pub fn join_model_routes(
    catalogue: &ModelCatalogue,
    observed: &[ObservedProviderModel],
    credentials: &CredentialEvidence,
) -> ModelRouteJoin {
    join_model_routes_with_reach(catalogue, observed, &[], credentials)
}

/// As [`join_model_routes`], additionally consulting which providers a
/// detected harness can dispatch to.
pub fn join_model_routes_with_reach(
    catalogue: &ModelCatalogue,
    observed: &[ObservedProviderModel],
    reachable: &[ProviderReachability],
    credentials: &CredentialEvidence,
) -> ModelRouteJoin {
    let mut route_sets: Vec<ModelRouteSet> = Vec::new();
    let mut models: Vec<ResolvedResource> = Vec::new();
    let mut unmatched: Vec<UnmatchedModelOffer> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut claimed: BTreeSet<(String, String)> = BTreeSet::new();

    for entry in catalogue.entries() {
        let mut set = ModelRouteSet::new(entry.model.clone());
        for declared in &entry.routes {
            let matches: Vec<&ObservedProviderModel> = observed
                .iter()
                .filter(|item| {
                    item.provider == declared.provider
                        && (declared.claims(&item.provider_native_id)
                            || item
                                .also_known_as
                                .iter()
                                .any(|alias| declared.claims(alias)))
                })
                .collect();
            if matches.is_empty() {
                // Before calling it unproven: can a harness that is actually
                // installed here reach this provider? That is real evidence of
                // a route, and it is the only evidence a hosted provider can
                // have short of calling its API.
                if let Some(reach) = reachable
                    .iter()
                    .find(|reach| reach.provider == declared.provider)
                {
                    let declared_credential = if reach.credential_required {
                        CredentialCondition::Required {
                            hint: reach
                                .credential_hint
                                .clone()
                                .unwrap_or_else(|| format!("{} credential", declared.provider)),
                        }
                    } else {
                        CredentialCondition::NotRequired
                    };
                    let mut provenance = vec![entry.source.to_string()];
                    provenance.extend(reach.provenance.iter().cloned());
                    set.routes.push(ModelRoute {
                        model: entry.model.clone(),
                        provider: declared.provider.clone(),
                        kind: ModelRouteKind::HarnessNative,
                        provider_native_id: declared
                            .provider_native_ids
                            .first()
                            .cloned()
                            .unwrap_or_default(),
                        endpoint: Some(format!("{} via {}", reach.selector, reach.through)),
                        availability: RouteAvailability::Observed {
                            detection_ref: reach
                                .provenance
                                .first()
                                .cloned()
                                .unwrap_or_else(|| reach.through.clone()),
                        },
                        credential: credentials.resolve(&declared.provider, &declared_credential),
                        provenance,
                    });
                    continue;
                }
                // Declared is not available. The route stays described so an
                // owner can see what would have to appear for it to work.
                set.routes.push(ModelRoute {
                    model: entry.model.clone(),
                    provider: declared.provider.clone(),
                    kind: declared.kind,
                    provider_native_id: declared
                        .provider_native_ids
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    endpoint: declared.endpoint.clone(),
                    availability: RouteAvailability::Unobserved {
                        reason: format!(
                            "{} was not observed offering any of [{}]",
                            declared.provider,
                            declared.provider_native_ids.join(", ")
                        ),
                    },
                    credential: credentials.resolve(&declared.provider, &declared.credential),
                    provenance: vec![entry.source.to_string()],
                });
                continue;
            }
            for item in matches {
                claimed.insert((item.provider.to_string(), item.provider_native_id.clone()));
                let mut provenance = vec![entry.source.to_string(), item.detection_ref.clone()];
                if let Some(source) = &item.inventory_source {
                    provenance.push(source.clone());
                }
                set.routes.push(ModelRoute {
                    model: entry.model.clone(),
                    provider: item.provider.clone(),
                    kind: declared.kind,
                    provider_native_id: item.provider_native_id.clone(),
                    endpoint: item.endpoint.clone().or_else(|| declared.endpoint.clone()),
                    availability: RouteAvailability::Observed {
                        detection_ref: item.detection_ref.clone(),
                    },
                    credential: credentials.resolve(&item.provider, &declared.credential),
                    provenance,
                });
            }
        }
        models.push(model_resource(entry.resource_record(), &set));
        route_sets.push(set);
    }

    for item in observed {
        if claimed.contains(&(item.provider.to_string(), item.provider_native_id.clone())) {
            continue;
        }
        unmatched.push(UnmatchedModelOffer {
            provider: item.provider.clone(),
            kind: item.kind,
            provider_native_id: item.provider_native_id.clone(),
            endpoint: item.endpoint.clone(),
            reason: format!(
                "no catalogue entry claims `{}` from {} — it stays an offer, not a Model, until \
                 a catalogue entry or a Provider Source admits or maps it",
                item.provider_native_id, item.provider
            ),
            provenance: vec![item.detection_ref.clone()],
        });
    }

    let available = route_sets.iter().filter(|set| set.is_available()).count();
    let usable = route_sets.iter().filter(|set| set.is_usable()).count();
    notes.push(format!(
        "model catalogue ↔ route join: {} catalogued Model(s), {} with at least one observed \
         route, {} presently usable (observed and credentialled), {} unmatched provider offer(s)",
        route_sets.len(),
        available,
        usable,
        unmatched.len()
    ));
    ModelRouteJoin {
        models,
        route_sets,
        unmatched,
        notes,
    }
}

/// Attach observed routes to the catalogue's Model record as provider offers,
/// so operational availability is derived by the same rule every other
/// resource uses. The catalogue source stays on the descriptor: identity is
/// authored, availability is observed, and neither is folded into the other.
fn model_resource(mut record: ResourceRecord, routes: &ModelRouteSet) -> ResolvedResource {
    for route in &routes.routes {
        record.providers.push(ProviderOffer {
            provider: route.provider.clone(),
            locator: Some(ResourceLocator::Opaque(route.provider_native_id.clone())),
            state: match &route.availability {
                RouteAvailability::Observed { .. } => ProviderState::Available,
                RouteAvailability::Unobserved { reason } => ProviderState::Unavailable {
                    reason: reason.clone(),
                },
                RouteAvailability::Unavailable { reason } => ProviderState::Unavailable {
                    reason: reason.clone(),
                },
            },
        });
        if let RouteAvailability::Observed { detection_ref } = &route.availability {
            if let Ok(source) = SourceRef::parse(ROUTE_SOURCE) {
                let observed = ResourceSource {
                    source,
                    authority: Some(aikit_core::resource::SourceAuthority::Observed),
                    revision: None,
                    locator: None,
                    state: SourceState::Available,
                };
                if !record.descriptor.sources.contains(&observed) {
                    record.descriptor.sources.push(observed);
                }
                record
                    .descriptor
                    .annotations
                    .insert("detection_ref".into(), detection_ref.clone());
            }
        }
    }
    record.descriptor.annotations.insert(
        "viable_routes".into(),
        routes
            .viable()
            .into_iter()
            .map(|route| format!("{} via {}", route.provider_native_id, route.provider))
            .collect::<Vec<_>>()
            .join("; "),
    );
    // A catalogued Model with no observed route is known-but-unavailable — a
    // real, useful state that must never read as available.
    let availability = if routes.is_available() {
        Availability::Available
    } else {
        Availability::Unavailable {
            reasons: routes
                .routes
                .iter()
                .map(|route| match &route.availability {
                    RouteAvailability::Observed { .. } => unreachable!(),
                    RouteAvailability::Unobserved { reason }
                    | RouteAvailability::Unavailable { reason } => reason.clone(),
                })
                .collect(),
        }
    };
    ResolvedResource {
        resource: record,
        availability,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actuation_harness_detection::{
        intake_actuation_detection, DetectionInventoryItem, DetectionInventoryReceipt,
    };
    use crate::runner::{CommandRunner, Output};
    use aikit_core::resource::{
        canonical_model_ref, CredentialCondition, DeclaredRoute, ModelCatalogueEntry,
    };
    use aikit_core::Result;
    use std::collections::BTreeSet;

    fn provider(name: &str) -> ProviderRef {
        ProviderRef::parse(name).unwrap()
    }

    fn catalogue(pairs: &[(&str, &str, &[&str])]) -> ModelCatalogue {
        let mut catalogue = ModelCatalogue::default();
        for (model, provider_name, ids) in pairs {
            catalogue
                .insert(ModelCatalogueEntry {
                    model: canonical_model_ref(model).unwrap(),
                    name: (*model).into(),
                    description: "fixture".into(),
                    superseded_refs: BTreeSet::new(),
                    routes: vec![DeclaredRoute {
                        provider: provider(provider_name),
                        kind: ModelRouteKind::LocalServing,
                        provider_native_ids: ids.iter().map(|id| (*id).to_string()).collect(),
                        endpoint: None,
                        credential: CredentialCondition::NotRequired,
                    }],
                    source: SourceRef::parse("source/test-catalogue").unwrap(),
                    freshness: None,
                })
                .unwrap();
        }
        catalogue
    }

    fn seen(provider_name: &str, id: &str) -> ObservedProviderModel {
        ObservedProviderModel {
            provider: provider(provider_name),
            kind: ModelRouteKind::LocalServing,
            provider_native_id: id.into(),
            also_known_as: Vec::new(),
            endpoint: Some("http://127.0.0.1:11434".into()),
            detection_ref: "detection:2026-09-09T00:00:00Z".into(),
            inventory_source: Some("http://127.0.0.1:11434/api/tags".into()),
        }
    }

    #[test]
    fn one_model_reached_by_two_providers_is_one_candidate_with_two_routes() {
        let mut catalogue = ModelCatalogue::default();
        catalogue
            .insert(ModelCatalogueEntry {
                model: canonical_model_ref("model:llama3.2").unwrap(),
                name: "Llama 3.2".into(),
                description: "fixture".into(),
                superseded_refs: BTreeSet::new(),
                routes: vec![
                    DeclaredRoute {
                        provider: provider("provider:ollama"),
                        kind: ModelRouteKind::LocalServing,
                        provider_native_ids: vec!["llama3.2:latest".into()],
                        endpoint: None,
                        credential: CredentialCondition::NotRequired,
                    },
                    DeclaredRoute {
                        provider: provider("provider:ninerouter"),
                        kind: ModelRouteKind::RouterRoute,
                        provider_native_ids: vec!["meta/llama-3.2".into()],
                        endpoint: None,
                        credential: CredentialCondition::Required {
                            hint: "router key".into(),
                        },
                    },
                ],
                source: SourceRef::parse("source/test-catalogue").unwrap(),
                freshness: None,
            })
            .unwrap();
        let join = join_model_routes(
            &catalogue,
            &[
                seen("provider:ollama", "llama3.2:latest"),
                seen("provider:ninerouter", "meta/llama-3.2"),
            ],
            &CredentialEvidence::default(),
        );
        assert_eq!(
            join.models.len(),
            1,
            "one Model candidate, not one per route"
        );
        assert_eq!(join.route_sets.len(), 1);
        assert_eq!(join.route_sets[0].viable().len(), 2, "two routes");
        assert_eq!(join.models[0].availability, Availability::Available);
        assert!(join.unmatched.is_empty());
    }

    #[test]
    fn losing_one_route_keeps_the_same_model_ref_and_the_other_route() {
        let catalogue = catalogue(&[("model:llama3.2", "provider:ollama", &["llama3.2:latest"])]);
        let before = join_model_routes(
            &catalogue,
            &[seen("provider:ollama", "llama3.2:latest")],
            &CredentialEvidence::default(),
        );
        let after = join_model_routes(&catalogue, &[], &CredentialEvidence::default());
        assert_eq!(before.route_sets[0].model, after.route_sets[0].model);
        assert!(before.route_sets[0].is_available());
        assert!(!after.route_sets[0].is_available());
        // Known-but-unavailable, never falsely available and never absent.
        assert_eq!(after.models.len(), 1);
        assert!(matches!(
            after.models[0].availability,
            Availability::Unavailable { .. }
        ));
    }

    #[test]
    fn a_discovered_model_with_no_catalogue_map_is_an_offer_not_an_invented_ref() {
        let catalogue = catalogue(&[("model:llama3.2", "provider:ollama", &["llama3.2:latest"])]);
        let join = join_model_routes(
            &catalogue,
            &[seen("provider:ollama", "mystery-model:7b")],
            &CredentialEvidence::default(),
        );
        assert_eq!(join.unmatched.len(), 1);
        assert_eq!(join.unmatched[0].provider_native_id, "mystery-model:7b");
        // No Model was invented for it.
        assert!(join
            .route_sets
            .iter()
            .all(|set| set.model.as_str() == "model:llama3.2"));
        assert!(join
            .models
            .iter()
            .all(|model| model.resource.descriptor.id.as_str() != "mystery-model:7b"));
    }

    #[test]
    fn an_alias_the_provider_supplied_still_joins_to_the_catalogued_identity() {
        let catalogue = catalogue(&[("model:llama3.2", "provider:ollama", &["llama3.2:latest"])]);
        let mut observed = seen("provider:ollama", "llama3.2");
        observed.also_known_as = vec!["llama3.2:latest".into()];
        let join = join_model_routes(&catalogue, &[observed], &CredentialEvidence::default());
        assert!(join.unmatched.is_empty());
        assert_eq!(
            join.route_sets[0].viable()[0].provider_native_id,
            "llama3.2"
        );
    }

    struct StubRunner(String);
    impl CommandRunner for StubRunner {
        fn run(&self, _argv: &[String]) -> Result<Output> {
            Ok(Output::success(self.0.clone()))
        }
    }

    fn detection_json(native_kind: &str, facets: &str) -> String {
        format!(
            r#"{{"schema":"actuation.harness-detection/v1","document":"detection",
            "detection_ref":"detection:2026-09-09T00:00:00Z","observed_at":"2026-09-09T00:00:00Z",
            "catalog_revision":5,"detector":{{"implementation":"t","version":"0"}},
            "harnesses":[{{"slug":"ollama","harness_ref":"harness/ollama","native_kind":"{native_kind}",
              "state":"detected","receipts":{{"executable":"/usr/local/bin/ollama"}},
              "probes":[{{"kind":"service","result":"pass","detail":"http 200 from http://127.0.0.1:11434"}}],
              "facets":{facets}}}],
            "absent":[],"availability":"complete"}}"#
        )
    }

    #[test]
    fn only_a_declared_model_provider_supplies_routes() {
        let facets = r#"[{"kind":"models","path":"~/.ollama/models","exists":true,"count":3,
            "inventory":[{"id":"llama3.2:latest"}],
            "inventory_receipt":{"kind":"http-json","source":"http://127.0.0.1:11434/api/tags",
              "observed_at":"2026-09-09T00:00:00Z","item_count":1}}]"#;
        let as_provider = intake_actuation_detection(
            &StubRunner(detection_json("model-provider", facets)),
            "actuation",
        );
        let (observed, _) = observed_provider_models(&as_provider);
        assert_eq!(observed.len(), 1);

        // The identical record, classified as an agent harness, contributes
        // nothing: a models facet does not make something a model provider.
        let as_harness =
            intake_actuation_detection(&StubRunner(detection_json("harness", facets)), "actuation");
        let (observed, _) = observed_provider_models(&as_harness);
        assert!(observed.is_empty());
    }

    #[test]
    fn a_counted_facet_without_a_typed_inventory_yields_no_route_and_says_why() {
        let facets = r#"[{"kind":"models","path":"~/.ollama/models","exists":true,"count":3}]"#;
        let detection = intake_actuation_detection(
            &StubRunner(detection_json("model-provider", facets)),
            "actuation",
        );
        let (observed, notes) = observed_provider_models(&detection);
        assert!(observed.is_empty());
        assert!(notes
            .iter()
            .any(|note| note.contains("a count is not an identity")));
    }

    #[test]
    fn an_unreadable_inventory_is_disclosed_not_read_as_an_empty_provider() {
        let facets = r#"[{"kind":"models","path":"~/.ollama/models","exists":true,"count":3,
            "inventory_unavailable_reason":"inventory read failed: curl exit 7"}]"#;
        let detection = intake_actuation_detection(
            &StubRunner(detection_json("model-provider", facets)),
            "actuation",
        );
        let (observed, notes) = observed_provider_models(&detection);
        assert!(observed.is_empty());
        assert!(notes.iter().any(|note| note.contains("curl exit 7")));
    }

    #[test]
    fn a_failed_detection_run_leaves_catalogued_models_unproven_never_absent() {
        let detection = DetectionOutcome::Unavailable {
            reason: "could not run actuation".into(),
        };
        let (observed, notes) = observed_provider_models(&detection);
        assert!(observed.is_empty());
        assert!(notes[0].contains("known-but-unproven, not absent"));
        let catalogue = catalogue(&[("model:llama3.2", "provider:ollama", &["llama3.2:latest"])]);
        let join = join_model_routes(&catalogue, &observed, &CredentialEvidence::default());
        assert_eq!(join.models.len(), 1);
        assert!(!join.route_sets[0].is_available());
    }

    #[test]
    fn inventory_items_round_trip_the_typed_shape() {
        let item = DetectionInventoryItem {
            id: "llama3.2:latest".into(),
            also_known_as: Some(vec!["llama3.2".into()]),
        };
        let receipt = DetectionInventoryReceipt {
            kind: "http-json".into(),
            source: "http://127.0.0.1:11434/api/tags".into(),
            observed_at: "2026-09-09T00:00:00Z".into(),
            item_count: 1,
        };
        assert_eq!(item.id, "llama3.2:latest");
        assert_eq!(receipt.item_count, 1);
    }
}
