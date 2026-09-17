//! Model routes: the ways a canonical Model can actually be reached.
//!
//! A route is not a Model. Canonical Model identity is a `ResourceRef` of the
//! form `model:<stable-id>` and stays stable across provider renames; a route
//! carries the provider, the provider-native id that provider happens to use
//! today, and the evidence that the route was observed. Provider-native ids
//! (`llama3.2:latest`, `anthropic/claude-sonnet-4`) are opaque route metadata
//! and are never promoted to identity.
//!
//! Direction: catalogue -> availability -> selection -> actualisation. Routes
//! are the availability half. They are produced by joining the canonical
//! catalogue against Actuation's live detection evidence; they never mint a
//! Model, and usage telemetry never feeds back into them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::resource::{ProviderRef, ResourceRef};

pub const MODEL_ROUTE_VERSION: &str = "aikit.model-route/v1";

/// How a route reaches the model. The kind is route shape, never identity:
/// the same `ModelRef` may be reachable through several of these at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelRouteKind {
    /// The model's own provider, addressed directly (an inference API).
    ProviderNative,
    /// A model router (NineRouter/OpenRouter-style) that fronts many
    /// providers. A router is a route provider, not an identity system.
    RouterRoute,
    /// A harness that carries its own model binding (the harness dispatches).
    HarnessNative,
    /// A locally served model (Ollama, llama.cpp, vLLM and kin).
    LocalServing,
}

impl ModelRouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderNative => "provider-native",
            Self::RouterRoute => "router-route",
            Self::HarnessNative => "harness-native",
            Self::LocalServing => "local-serving",
        }
    }
}

/// Whether the route was actually seen. The three states are the same law
/// Actuation's detection contract enforces one level up: proven, unproven,
/// and proven-absent are three different facts and never collapse into two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum RouteAvailability {
    /// The provider was observed offering this provider-native id.
    Observed { detection_ref: String },
    /// Nothing has been observed about this route. Declared is not available.
    Unobserved { reason: String },
    /// The route was looked for and is not there, or could not be proven.
    Unavailable { reason: String },
}

impl RouteAvailability {
    pub fn is_observed(&self) -> bool {
        matches!(self, Self::Observed { .. })
    }
}

/// Credentials qualify a route's usability. They are never part of Model
/// identity: a route whose key is missing is a route that cannot be used
/// today, not a different Model and not an absent one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "condition", rename_all = "kebab-case")]
pub enum CredentialCondition {
    /// A local route that needs no credential to be usable.
    NotRequired,
    /// A credential is required and none has been observed bound for it.
    Required { hint: String },
    /// A credential is required and one is bound. This records that a binding
    /// exists — it never materialises, reads or carries the secret itself.
    Satisfied { hint: String, binding_ref: String },
}

impl CredentialCondition {
    /// Does this route still need a credential it does not have?
    pub fn is_unsatisfied(&self) -> bool {
        matches!(self, Self::Required { .. })
    }

    pub fn requires_credential(&self) -> bool {
        !matches!(self, Self::NotRequired)
    }
}

/// The three tiers the brief keeps apart, in one reading:
/// `model supported` (the route exists at all) -> `route observed`
/// (someone proved it is there) -> `route presently usable` (and the
/// credential it needs is bound). Collapsing any two of these is how a
/// catalogue starts lying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "usability", rename_all = "kebab-case")]
pub enum RouteUsability {
    Usable,
    NeedsCredential { hint: String },
    NotObserved { reason: String },
}

/// One way to reach one canonical Model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRoute {
    pub model: ResourceRef,
    pub provider: ProviderRef,
    pub kind: ModelRouteKind,
    /// Opaque provider-native identity. Never the ModelRef.
    pub provider_native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub availability: RouteAvailability,
    pub credential: CredentialCondition,
    /// Where this route's evidence came from: detection refs, catalogue
    /// source refs, the provider inventory receipt that observed it.
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ModelRoute {
    /// A route is viable when it was actually observed. A missing credential
    /// does not make it unviable — it makes it presently unusable, which
    /// [`Self::usability`] reports separately. A route dropped here would
    /// silently disappear from selection instead of explaining itself.
    pub fn is_viable(&self) -> bool {
        self.availability.is_observed()
    }

    /// Whether this route can be taken right now, and if not, why not.
    pub fn usability(&self) -> RouteUsability {
        match (&self.availability, &self.credential) {
            (RouteAvailability::Observed { .. }, CredentialCondition::Required { hint }) => {
                RouteUsability::NeedsCredential { hint: hint.clone() }
            }
            (RouteAvailability::Observed { .. }, _) => RouteUsability::Usable,
            (RouteAvailability::Unobserved { reason }, _)
            | (RouteAvailability::Unavailable { reason }, _) => RouteUsability::NotObserved {
                reason: reason.clone(),
            },
        }
    }

    pub fn is_usable(&self) -> bool {
        matches!(self.usability(), RouteUsability::Usable)
    }
}

/// One canonical Model with every route currently known for it — plural by
/// construction. Selecting the model must leave this set intact so Actuation
/// can resolve, and re-resolve, an actual route from current evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRouteSet {
    pub model: ResourceRef,
    pub routes: Vec<ModelRoute>,
}

impl ModelRouteSet {
    pub fn new(model: ResourceRef) -> Self {
        Self {
            model,
            routes: Vec::new(),
        }
    }

    pub fn viable(&self) -> Vec<&ModelRoute> {
        self.routes
            .iter()
            .filter(|route| route.is_viable())
            .collect()
    }

    /// Viable routes whose credential condition is also met. A Model with
    /// viable-but-unusable routes is a real and useful state: it is reachable
    /// the moment a key is bound, which is different from unreachable.
    pub fn usable(&self) -> Vec<&ModelRoute> {
        self.routes
            .iter()
            .filter(|route| route.is_usable())
            .collect()
    }

    pub fn is_usable(&self) -> bool {
        self.routes.iter().any(ModelRoute::is_usable)
    }

    /// A Model is available when at least one route to it was observed. A
    /// catalogued Model with no proven route is known-but-unavailable, which
    /// is a different fact from "not in the catalogue".
    pub fn is_available(&self) -> bool {
        self.routes.iter().any(ModelRoute::is_viable)
    }

    pub fn providers(&self) -> BTreeSet<ProviderRef> {
        self.viable()
            .into_iter()
            .map(|route| route.provider.clone())
            .collect()
    }

    /// Viable routes narrowed to an explicitly pinned provider. Pinning is a
    /// constraint on the route, never a change of Model identity.
    pub fn viable_pinned(&self, provider: &ProviderRef) -> Vec<&ModelRoute> {
        self.viable()
            .into_iter()
            .filter(|route| &route.provider == provider)
            .collect()
    }
}

/// A provider-native model identity that was observed but that no catalogue
/// entry claims. It is deliberately not a Model: minting a `ModelRef` from
/// whatever happens to be installed today would make identity a function of
/// this machine, which is exactly what stable-identity-across-provider-renames
/// forbids. It stays visible as an offer until a catalogue entry or a Provider
/// Source admits or maps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmatchedModelOffer {
    pub provider: ProviderRef,
    pub kind: ModelRouteKind,
    pub provider_native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// What an owner would do to adopt it.
    pub reason: String,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, provider: &str, observed: bool) -> ModelRoute {
        ModelRoute {
            model: ResourceRef::parse("model:stable").unwrap(),
            provider: ProviderRef::parse(provider).unwrap(),
            kind: ModelRouteKind::LocalServing,
            provider_native_id: id.into(),
            endpoint: None,
            availability: if observed {
                RouteAvailability::Observed {
                    detection_ref: "detection:2026-09-09T00:00:00Z".into(),
                }
            } else {
                RouteAvailability::Unobserved {
                    reason: "declared by the catalogue; no provider has been observed offering it"
                        .into(),
                }
            },
            credential: CredentialCondition::NotRequired,
            provenance: Vec::new(),
        }
    }

    #[test]
    fn one_model_carries_several_routes_without_fusing_identity() {
        let mut set = ModelRouteSet::new(ResourceRef::parse("model:stable").unwrap());
        set.routes
            .push(route("llama3.2:latest", "provider:ollama", true));
        set.routes
            .push(route("meta/llama-3.2", "provider:ninerouter", true));
        assert_eq!(set.viable().len(), 2);
        assert_eq!(set.providers().len(), 2);
        // One identity, two ways to reach it.
        assert!(set.routes.iter().all(|r| r.model == set.model));
    }

    #[test]
    fn a_catalogued_model_with_no_observed_route_is_known_but_unavailable() {
        let mut set = ModelRouteSet::new(ResourceRef::parse("model:stable").unwrap());
        set.routes
            .push(route("llama3.2:latest", "provider:ollama", false));
        assert!(!set.is_available());
        assert!(set.viable().is_empty());
        // Known-but-unavailable is not absence: the route is still described.
        assert_eq!(set.routes.len(), 1);
    }

    #[test]
    fn losing_one_route_leaves_the_model_and_its_other_route_intact() {
        let mut set = ModelRouteSet::new(ResourceRef::parse("model:stable").unwrap());
        set.routes
            .push(route("llama3.2:latest", "provider:ollama", true));
        set.routes
            .push(route("meta/llama-3.2", "provider:ninerouter", true));
        let before = set.model.clone();
        set.routes
            .retain(|route| route.provider.as_str() != "provider:ollama");
        assert_eq!(set.model, before);
        assert!(set.is_available());
        assert_eq!(set.viable()[0].provider.as_str(), "provider:ninerouter");
    }

    #[test]
    fn pinning_a_provider_narrows_routes_without_changing_the_model() {
        let mut set = ModelRouteSet::new(ResourceRef::parse("model:stable").unwrap());
        set.routes
            .push(route("llama3.2:latest", "provider:ollama", true));
        set.routes
            .push(route("meta/llama-3.2", "provider:ninerouter", true));
        let pinned = set.viable_pinned(&ProviderRef::parse("provider:ollama").unwrap());
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].model, set.model);
    }
}
